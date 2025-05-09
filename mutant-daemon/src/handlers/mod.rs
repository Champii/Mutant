// Re-export all handlers and common functionality
mod common;
mod websocket;
mod dispatcher;
mod data_operations;
mod task_management;
mod system_operations;
mod metadata;
mod import_export;
mod key_management;

// Public exports
pub use websocket::handle_ws;
pub use task_management::{TaskEntry, TaskMap};
pub use key_management::{ActiveKeysMap, try_register_key, release_key};

/// Check if the daemon is running in public-only mode
pub fn is_public_only_mode() -> bool {
    // If the OnceCell hasn't been initialized yet, we're not in public-only mode
    crate::app::PUBLIC_ONLY_MODE.get().copied().unwrap_or(false)
}

/// Error message for operations that require a wallet when in public-only mode
pub const PUBLIC_ONLY_ERROR_MSG: &str = "This operation requires a wallet, but the daemon is running in public-only mode. To enable full functionality, please set up an Autonomi wallet using 'ant wallet import' or 'ant wallet create' and restart the daemon.";
