use async_channel::bounded;
use log::error;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;

use autonomi::Client;
use crate::error::Error as MutantError;
use crate::network::BATCH_SIZE;
use crate::network::NB_CLIENTS;
use super::async_task::AsyncTask;
use super::config::WorkerPoolConfig;
use super::error::PoolError;
use super::pool::WorkerPool;

#[allow(clippy::too_many_arguments)]
pub async fn build<Item, Context, Task, T, E>(
    config: WorkerPoolConfig<Task>,
    recycle_fn: Option<
        Arc<
            dyn Fn(
                    E,
                    Item,
                )
                    -> futures::future::BoxFuture<'static, Result<Option<Item>, MutantError>>
                + Send
                + Sync,
        >,
    >,
) -> Result<WorkerPool<Item, Context, Client, Task, T, E>, PoolError<E>>
where
    Item: Send + 'static,
    Context: Send + Sync + 'static,
    Task: AsyncTask<Item, Context, Client, T, E> + Send + Sync + 'static + Clone,
    T: Send + Sync + Clone + 'static,
    E: Debug + Send + Clone + 'static + From<MutantError>,
{
    if config.enable_recycling && recycle_fn.is_none() {
        return Err(PoolError::PoolSetupError(
            "Recycling enabled but no recycle_fn provided".to_string(),
        ));
    }

    let num_workers = NB_CLIENTS.min(config.total_items_hint);
    let batch_size = *BATCH_SIZE;

    // --- Channel Creation ---
    let mut worker_txs = Vec::with_capacity(num_workers);
    let mut worker_rxs = Vec::with_capacity(num_workers);
    let worker_bound = config.total_items_hint.saturating_add(1) / num_workers + batch_size;
    for _ in 0..num_workers {
        let (tx, rx) = bounded::<Item>(worker_bound);
        worker_txs.push(tx);
        worker_rxs.push(rx);
    }

    let global_bound = config.total_items_hint + num_workers * batch_size;
    let (global_tx, global_rx) = bounded::<Item>(global_bound);

    let (retry_sender, retry_receiver) = if config.enable_recycling {
        let (tx, rx) = bounded::<(E, Item)>(global_bound);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // --- Client Acquisition ---
    let mut clients = Vec::with_capacity(num_workers);
    for worker_id in 0..num_workers {
        match config
            .network
            .get_client(config.client_config.clone())
            .await
        {
            Ok(client) => clients.push(Arc::new(client)),
            Err(e) => {
                let err_msg = format!("Failed to get client for worker {}: {:?}", worker_id, e);
                error!("{}", err_msg);
                return Err(PoolError::ClientAcquisitionError(err_msg));
            }
        }
    }

    // Create the pool instance
    let pool = WorkerPool {
        task: Arc::new(config.task_processor),
        clients,
        worker_txs,
        worker_rxs,
        global_tx,
        global_rx,
        retry_sender,
        retry_rx: retry_receiver,
        total_items_hint: config.total_items_hint,
        _marker_context: PhantomData,
        _marker_result: PhantomData,
        _marker_error: PhantomData,
    };

    Ok(pool)
}
