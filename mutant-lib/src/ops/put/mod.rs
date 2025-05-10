mod context;
mod operations;
mod pipeline;
mod task;
#[cfg(test)]
mod tests;

use crate::error::Error;
use crate::network::Network;
use autonomi::ScratchpadAddress;
use log::info;
use mutant_protocol::{PutCallback, StorageMode};
use std::sync::Arc;
use tokio::sync::RwLock;

// Re-export the main operations
pub use operations::{first_store, resume, update};

/// Main entry point for put operations
pub(super) async fn put(
    index: Arc<RwLock<crate::index::master_index::MasterIndex>>,
    network: Arc<Network>,
    key_name: &str,
    content: Arc<Vec<u8>>,
    mode: StorageMode,
    public: bool,
    no_verify: bool,
    put_callback: Option<PutCallback>,
) -> Result<ScratchpadAddress, Error> {
    if index.read().await.contains_key(key_name) {
        if index
            .read()
            .await
            .verify_checksum(key_name, &content, mode.clone())
        {
            info!("Resume for {}", key_name);
            resume(
                index,
                network,
                key_name,
                content,
                mode,
                public,
                no_verify,
                put_callback,
            )
            .await
        } else {
            // Call the dedicated update function
            update(
                index,
                network,
                key_name,
                content,
                mode,
                public,
                no_verify,
                put_callback,
            )
            .await
        }
    } else {
        info!("First store for {}", key_name);
        first_store(
            index,
            network,
            key_name,
            content,
            mode,
            public,
            no_verify,
            put_callback,
        )
        .await
    }
}
