use crate::error::Error;
use crate::index::PadInfo;
use crate::internal_events::invoke_put_callback;
use crate::network::NetworkError;
use crate::ops::worker::{self, PoolError, WorkerPoolConfig};
use futures::future::BoxFuture;
use log::{debug, error, info, warn};
use mutant_protocol::{PutCallback, PutEvent};
use std::sync::Arc;

use super::context::Context;
use super::context::PutTaskContext;
use super::task::PutTaskProcessor;

// Define the recycling logic function
pub async fn recycle_put_pad(
    context: Context,
    error_cause: Error,
    pad_to_recycle: PadInfo,
) -> Result<Option<PadInfo>, Error> {
    warn!(
        "Recycling pad {} for key '{}' due to error: {:?}",
        pad_to_recycle.address, context.name, error_cause
    );

    // Log the pad status before recycling
    debug!(
        "Pad to recycle: address={}, status={:?}, chunk_index={}, size={}",
        pad_to_recycle.address,
        pad_to_recycle.status,
        pad_to_recycle.chunk_index,
        pad_to_recycle.size
    );

    match context
        .index
        .write()
        .await
        .recycle_errored_pad(&context.name, &pad_to_recycle.address)
        .await
    {
        Ok(new_pad) => {
            debug!(
                "Successfully recycled pad {} -> {} for key '{}', returning to pool. New pad status: {:?}",
                pad_to_recycle.address, new_pad.address, context.name, new_pad.status
            );

            // Return the new pad to be processed by the worker pool
            Ok(Some(new_pad))
        }
        Err(recycle_err) => {
            error!(
                "Failed to recycle pad {} for key '{}': {}. Skipping this pad.",
                pad_to_recycle.address, context.name, recycle_err
            );
            // We'll just skip this pad and log the error rather than halting the entire process
            Ok(None)
        }
    }
}

pub async fn write_pipeline(
    context: Context,
    pads: Vec<PadInfo>,
    no_verify: bool,
    put_callback: Option<PutCallback>,
) -> Result<(), Error> {
    let key_name = context.name.clone();

    // Count pads by status before filtering
    let total_chunks = pads.len();
    let initial_written_count = pads
        .iter()
        .filter(|p| p.status == PadStatus::Written || p.status == PadStatus::Confirmed)
        .count();
    let initial_confirmed_count = pads
        .iter()
        .filter(|p| p.status == PadStatus::Confirmed)
        .count();
    let chunks_to_reserve = pads
        .iter()
        .filter(|p| p.status == PadStatus::Generated)
        .count();

    // Send Starting event with pad counts
    info!(
        "Sending Starting event for key '{}': total={}, written={}, confirmed={}, to_reserve={}",
        key_name, total_chunks, initial_written_count, initial_confirmed_count, chunks_to_reserve
    );

    invoke_put_callback(
        &put_callback,
        PutEvent::Starting {
            total_chunks,
            initial_written_count,
            initial_confirmed_count,
            chunks_to_reserve,
        },
    )
    .await
    .map_err(|e| Error::Internal(format!("Callback error on Starting event: {:?}", e)))?;

    // Filter out already confirmed pads - these don't need processing
    let pads_to_process: Vec<PadInfo> = pads
        .into_iter()
        .filter(|p| p.status != PadStatus::Confirmed)
        .collect();
    let initial_process_count = pads_to_process.len();

    if initial_process_count == 0 {
        info!("All pads for key '{}' already confirmed.", key_name);
        // Invoke Complete callback immediately if nothing to do?
        invoke_put_callback(&put_callback, PutEvent::Complete)
            .await
            .map_err(|e| Error::Internal(format!("Callback error: {:?}", e)))?;
        return Ok(());
    }

    // 1. Create Context for Task Processor
    let put_task_context = Arc::new(PutTaskContext {
        base_context: context.clone(), // Clone base context Arc
        no_verify: Arc::new(no_verify),
        put_callback: put_callback.clone(),
    });

    // 2. Create Task Processor
    let task_processor = PutTaskProcessor::new(put_task_context.clone());

    // 3. Create WorkerPoolConfig
    // For the total_items_hint, we use the number of pads we're actually processing
    // This ensures the worker pool will only wait for the pads we're sending to it
    let config = WorkerPoolConfig {
        network: context.network.clone(), // Clone network Arc
        client_config: crate::network::client::Config::Put, // Use crate path
        task_processor,
        enable_recycling: true, // Ensure recycling is enabled for PUT
        total_items_hint: initial_process_count, // Use the number of pads we're actually processing
    };

    debug!(
        "Created WorkerPoolConfig for PUT with recycling enabled, total_items_hint={}",
        initial_process_count
    );

    // Define the recycling closure
    let recycle_fn = {
        let context_clone = context.clone(); // Clone context for the closure
        let key_name_for_log = context.name.to_string(); // Clone the key name for logging

        Arc::new(move |error: Error, pad: PadInfo| {
            let context_inner = context_clone.clone(); // Clone again for the async block
            let key_name_inner = key_name_for_log.clone(); // Clone for the async block

            info!(
                "Creating recycling function for key '{}', pad {}",
                key_name_inner, pad.address
            );

            Box::pin(async move {
                info!(
                    "Executing recycling function for key '{}', pad {}",
                    key_name_inner, pad.address
                );
                recycle_put_pad(context_inner, error, pad).await
            }) as BoxFuture<'static, Result<Option<PadInfo>, Error>>
        })
    };

    // 4. Build WorkerPool
    let pool = match worker::build(config, Some(recycle_fn.clone())).await {
        Ok(pool) => pool,
        Err(e) => {
            error!(
                "Failed to build worker pool for PUT '{}': {:?}",
                key_name, e
            );
            return match e {
                PoolError::ClientAcquisitionError(msg) => {
                    Err(Error::Network(NetworkError::ClientAccessError(msg)))
                }
                _ => Err(Error::Internal(format!("Pool build failed: {:?}", e))),
            };
        }
    };

    // 5. Send pads to the pool
    if let Err(e) = pool.send_items(pads_to_process).await {
        error!(
            "Failed to send initial pads to worker pool for PUT '{}': {:?}",
            key_name, e
        );
        // If sending fails, the pool might be in a bad state.
        return match e {
            PoolError::PoolSetupError(msg) => Err(Error::Internal(msg)),
            // Other PoolErrors might be relevant here depending on send_items implementation
            _ => Err(Error::Internal(format!("Pool send_items failed: {:?}", e))),
        };
    }

    // 6. Run the Worker Pool (recycling is now internal, driven by passed fn)
    // Make sure to pass the recycle_fn to ensure the recycling mechanism is active
    let pool_result = pool.run(Some(recycle_fn)).await;

    // 7. Process Pool Results
    match pool_result {
        Ok(_results) => {
            // For PUT, successful completion of the pool.run() without error is the main success signal,
            // assuming the recycler handled intermediate task errors.
            // We could potentially verify the final state in the index here if needed.
            info!("PUT operation seemingly successful for key '{}'. Final state verification might be needed.", key_name);
            // Invoke final completion callback
            invoke_put_callback(&put_callback, PutEvent::Complete)
                .await
                .map_err(|e| Error::Internal(format!("Callback error: {:?}", e)))?;
            Ok(())
        }
        Err(pool_error) => {
            error!(
                "PUT worker pool failed for key '{}': {:?}",
                key_name, pool_error
            );
            // Map PoolError to crate::Error
            match pool_error {
                PoolError::TaskError(task_err) => Err(task_err), // Task error that couldn't be recycled
                PoolError::JoinError(join_err) => Err(Error::Internal(format!(
                    "Worker task join error: {:?}",
                    join_err
                ))),
                PoolError::PoolSetupError(msg) => {
                    Err(Error::Internal(format!("Pool setup error: {}", msg)))
                }
                PoolError::ClientAcquisitionError(msg) => {
                    Err(Error::Network(NetworkError::ClientAccessError(msg)))
                }
            }
        }
    }
}

use crate::index::PadStatus;
