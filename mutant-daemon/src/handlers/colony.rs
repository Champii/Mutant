use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::OnceCell;
use mutant_lib::config::NetworkChoice;

use crate::error::Error as DaemonError;
use crate::colony::ColonyManager;
use mutant_protocol::{
    ErrorResponse, Response, SearchRequest, SearchResponse, IndexContentRequest,
    IndexContentResponse, GetMetadataRequest, GetMetadataResponse, AddContactRequest,
    AddContactResponse, ListContentRequest, ListContentResponse, SyncContactsRequest,
    SyncContactsResponse, GetUserContactRequest, GetUserContactResponse, ListContactsRequest,
    ListContactsResponse, ColonyEvent, ColonyProgressResponse
};

use super::common::UpdateSender;

/// Helper function to send colony progress events
fn send_colony_progress(update_tx: &UpdateSender, event: ColonyEvent, operation_id: Option<String>) {
    log::debug!("Sending colony progress event: {:?}", event);

    let response = Response::ColonyProgress(ColonyProgressResponse {
        event,
        operation_id,
    });

    if let Err(e) = update_tx.send(response) {
        log::error!("Failed to send colony progress event: {}", e);
    } else {
        log::debug!("Successfully sent colony progress event");
    }
}

// Global colony manager instance
static COLONY_MANAGER: OnceCell<Arc<ColonyManager>> = OnceCell::const_new();

// Global colony initialization parameters
static COLONY_INIT_PARAMS: OnceCell<(NetworkChoice, Option<String>)> = OnceCell::const_new();

// Global flag to track if colony initialization is in progress
static COLONY_INITIALIZATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Store colony initialization parameters for later use
pub async fn set_colony_init_params(network_choice: NetworkChoice, private_key_hex: Option<String>) {
    if let Err(_) = COLONY_INIT_PARAMS.set((network_choice, private_key_hex)) {
        log::warn!("Colony initialization parameters already set");
    } else {
        log::info!("Colony initialization parameters stored for on-demand initialization");
    }
}

/// Get colony initialization parameters
pub fn get_colony_init_params() -> Option<(NetworkChoice, Option<String>)> {
    COLONY_INIT_PARAMS.get().cloned()
}

/// Check if colony initialization is currently in progress
pub fn is_colony_initialization_in_progress() -> bool {
    COLONY_INITIALIZATION_IN_PROGRESS.load(Ordering::Relaxed)
}

/// Set colony initialization progress flag
fn set_colony_initialization_in_progress(in_progress: bool) {
    COLONY_INITIALIZATION_IN_PROGRESS.store(in_progress, Ordering::Relaxed);
    log::debug!("Colony initialization in progress flag set to: {}", in_progress);
}

/// Initialize the global colony manager
pub async fn init_colony_manager(wallet: autonomi::Wallet, network_choice: NetworkChoice, private_key_hex: Option<String>) -> Result<(), DaemonError> {
    log::info!("Starting colony manager initialization");

    // Set initialization in progress flag
    set_colony_initialization_in_progress(true);

    let result = async {
        let manager_opt = ColonyManager::new(wallet, network_choice, private_key_hex).await?;

        let manager = match manager_opt {
            Some(manager) => manager,
            None => {
                log::info!("Colony initialization skipped due to missing private key and COLONY_MNEMONIC");
                return Ok(());
            }
        };

        // Ensure the user's pod exists on the network
        log::info!("Ensuring user pod exists during initialization");
        if let Err(e) = manager.ensure_user_pod_exists().await {
            log::warn!("Failed to ensure user pod exists during initialization: {}. Pod will be created when needed.", e);
        } else {
            log::info!("User pod verified/created successfully during initialization");
        }

        COLONY_MANAGER.set(Arc::new(manager))
            .map_err(|_| DaemonError::ColonyError("Colony manager already initialized".to_string()))?;

        log::info!("Colony manager initialized successfully");
        Ok(())
    }.await;

    // Clear initialization in progress flag
    set_colony_initialization_in_progress(false);

    result
}

/// Initialize the global colony manager with progress updates
#[allow(dead_code)]
pub async fn init_colony_manager_with_progress(
    wallet: autonomi::Wallet,
    network_choice: NetworkChoice,
    private_key_hex: Option<String>,
    update_tx: UpdateSender
) -> Result<(), DaemonError> {
    log::info!("Starting colony manager initialization with progress tracking");

    // Set initialization in progress flag
    set_colony_initialization_in_progress(true);

    // Send progress event: initialization started
    send_colony_progress(&update_tx, ColonyEvent::InitializationStarted, None);

    let result = async {
        let manager_opt = ColonyManager::new_with_progress(wallet, network_choice, private_key_hex, update_tx.clone()).await?;

        let manager = match manager_opt {
            Some(manager) => manager,
            None => {
                log::info!("Colony initialization skipped due to missing private key and COLONY_MNEMONIC");
                // Send progress event: initialization skipped
                send_colony_progress(&update_tx, ColonyEvent::InitializationCompleted, Some("Skipped due to missing credentials".to_string()));
                return Ok(());
            }
        };

        // Send progress event: user pod check started
        send_colony_progress(&update_tx, ColonyEvent::UserPodCheckStarted, None);

        // Ensure the user's pod exists on the network
        log::info!("Ensuring user pod exists during initialization");
        if let Err(e) = manager.ensure_user_pod_exists_with_progress(update_tx.clone()).await {
            log::warn!("Failed to ensure user pod exists during initialization: {}. Pod will be created when needed.", e);

            // Send progress event: operation failed
            send_colony_progress(&update_tx, ColonyEvent::OperationFailed {
                operation: "user_pod_check".to_string(),
                error: e.to_string()
            }, None);
        } else {
            log::info!("User pod verified/created successfully during initialization");

            // Send progress event: user pod check completed
            send_colony_progress(&update_tx, ColonyEvent::UserPodCheckCompleted, None);
        }

        COLONY_MANAGER.set(Arc::new(manager))
            .map_err(|_| DaemonError::ColonyError("Colony manager already initialized".to_string()))?;

        // Send progress event: initialization completed
        send_colony_progress(&update_tx, ColonyEvent::InitializationCompleted, None);

        log::info!("Colony manager initialized successfully");
        Ok(())
    }.await;

    // Clear initialization in progress flag
    set_colony_initialization_in_progress(false);

    result
}

/// Initialize colony manager on-demand with progress events
pub async fn ensure_colony_manager_with_progress(
    mutant: Arc<mutant_lib::MutAnt>,
    update_tx: UpdateSender
) -> Result<Arc<ColonyManager>, DaemonError> {
    // Check if already initialized
    if let Some(manager) = COLONY_MANAGER.get() {
        return Ok(manager.clone());
    }

    // Get initialization parameters
    let (network_choice, private_key_hex) = COLONY_INIT_PARAMS.get()
        .ok_or_else(|| DaemonError::ColonyError("Colony initialization parameters not set".to_string()))?
        .clone();

    // Check if we have the necessary credentials
    let has_colony_mnemonic = std::env::var("COLONY_MNEMONIC").is_ok();
    if private_key_hex.is_none() && !has_colony_mnemonic {
        return Err(DaemonError::ColonyError("Colony manager cannot be initialized: no private key provided and no COLONY_MNEMONIC environment variable set".to_string()));
    }

    // Get wallet from MutAnt
    let wallet = mutant.get_wallet().await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to get wallet: {}", e)))?;

    // Initialize with progress events
    init_colony_manager_with_progress(wallet, network_choice, private_key_hex, update_tx).await?;

    // Return the initialized manager
    COLONY_MANAGER.get()
        .ok_or_else(|| DaemonError::ColonyError("Colony manager initialization failed".to_string()))
        .map(|manager| manager.clone())
}

/// Get the global colony manager instance
pub async fn get_colony_manager() -> Result<Arc<ColonyManager>, DaemonError> {
    COLONY_MANAGER.get()
        .ok_or_else(|| DaemonError::ColonyError("Colony manager not initialized".to_string()))
        .map(|manager| manager.clone())
}

/// Handle search requests
///
/// Executes SPARQL queries against the local RDF graph database to find content
/// based on various criteria (text, type, predicate, etc.).
pub async fn handle_search(
    req: SearchRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling search request: {:?}", req);

    // Send progress event: search started
    let query_type = if req.query.is_object() {
        req.query.get("type").and_then(|v| v.as_str()).unwrap_or("unknown").to_string()
    } else {
        "sparql".to_string()
    };

    send_colony_progress(&update_tx, ColonyEvent::SearchStarted {
        query_type: query_type.clone()
    }, None);

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;

    match colony_manager.search(req.query).await {
        Ok(results) => {
            // Count results for progress event
            let results_count = if let Some(bindings) = results.get("results")
                .and_then(|r| r.get("bindings"))
                .and_then(|b| b.as_array()) {
                bindings.len()
            } else {
                0
            };

            // Send progress event: search completed
            send_colony_progress(&update_tx, ColonyEvent::SearchCompleted {
                results_count
            }, None);

            let response = Response::Search(SearchResponse { results });

            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send search response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send search response: {}", e)));
            }

            log::debug!("Search request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("Search request failed: {}", e);

            // Send progress event: search failed
            send_colony_progress(&update_tx, ColonyEvent::OperationFailed {
                operation: format!("search_{}", query_type),
                error: e.to_string()
            }, None);

            let error_response = Response::Error(ErrorResponse {
                error: format!("Search failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            Err(e)
        }
    }
}

/// Handle content indexing requests
///
/// Creates metadata pods for stored content, enabling search and discovery.
/// The metadata should be in JSON-LD format following Schema.org conventions.
pub async fn handle_index_content(
    req: IndexContentRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling index content request: key={}, public_address={:?}",
               req.user_key, req.public_address);

    // Send progress event: indexing started
    send_colony_progress(&update_tx, ColonyEvent::IndexingStarted {
        user_key: req.user_key.clone()
    }, None);

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;

    match colony_manager.index_content(&req.user_key, req.metadata, req.public_address).await {
        Ok((success, pod_address)) => {
            // Send progress event: indexing completed
            send_colony_progress(&update_tx, ColonyEvent::IndexingCompleted {
                user_key: req.user_key.clone(),
                success
            }, None);

            let response = Response::IndexContent(IndexContentResponse {
                success,
                pod_address,
            });

            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send index content response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send index content response: {}", e)));
            }

            log::debug!("Index content request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("Index content request failed: {}", e);

            // Send progress event: indexing failed
            send_colony_progress(&update_tx, ColonyEvent::IndexingCompleted {
                user_key: req.user_key.clone(),
                success: false
            }, None);

            let error_response = Response::Error(ErrorResponse {
                error: format!("Content indexing failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            Err(e)
        }
    }
}

/// Handle metadata retrieval requests
///
/// Retrieves RDF metadata for a specific Autonomi address from the local graph database.
pub async fn handle_get_metadata(
    req: GetMetadataRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling get metadata request: address={}", req.address);

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;
    
    match colony_manager.get_metadata(&req.address).await {
        Ok(metadata) => {
            let response = Response::GetMetadata(GetMetadataResponse { metadata });
            
            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send get metadata response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send get metadata response: {}", e)));
            }
            
            log::debug!("Get metadata request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("Get metadata request failed: {}", e);
            let error_response = Response::Error(ErrorResponse {
                error: format!("Metadata retrieval failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });
            
            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }
            
            Err(e)
        }
    }
}

/// Handle add contact requests
///
/// Adds a pod address to the list of contacts to sync with.
pub async fn handle_add_contact(
    req: AddContactRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling add contact request: pod_address={}, name={:?}",
               req.pod_address, req.contact_name);

    // Send progress event: operation started
    send_colony_progress(&update_tx, ColonyEvent::AddContactStarted {
        pod_address: req.pod_address.clone()
    }, None);

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;

    // Send progress event: verifying contact pod
    send_colony_progress(&update_tx, ColonyEvent::ContactVerificationStarted {
        pod_address: req.pod_address.clone()
    }, None);

    match colony_manager.add_contact_with_progress(&req.pod_address, req.contact_name, update_tx.clone()).await {
        Ok(()) => {
            // Send progress event: operation completed
            send_colony_progress(&update_tx, ColonyEvent::AddContactCompleted {
                pod_address: req.pod_address.clone()
            }, None);

            let response = Response::AddContact(AddContactResponse {
                success: true,
            });

            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send add contact response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send add contact response: {}", e)));
            }

            log::debug!("Add contact request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("Add contact request failed: {}", e);

            // Send progress event: operation failed
            send_colony_progress(&update_tx, ColonyEvent::OperationFailed {
                operation: "add_contact".to_string(),
                error: e.to_string()
            }, None);

            let error_response = Response::Error(ErrorResponse {
                error: format!("Add contact failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            Err(e)
        }
    }
}

/// Handle list content requests
///
/// Lists all available content from synced pods.
pub async fn handle_list_content(
    _req: ListContentRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling list content request");

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;

    match colony_manager.list_all_content().await {
        Ok(content) => {
            let response = Response::ListContent(ListContentResponse { content });

            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send list content response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send list content response: {}", e)));
            }

            log::debug!("List content request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("List content request failed: {}", e);
            let error_response = Response::Error(ErrorResponse {
                error: format!("List content failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            Err(e)
        }
    }
}

/// Handle sync contacts requests
///
/// Syncs all contact pods to get the latest content.
pub async fn handle_sync_contacts(
    _req: SyncContactsRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling sync contacts request");

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;

    // Get contacts count first for progress tracking
    let contacts = match colony_manager.get_contacts().await {
        Ok(contacts) => contacts,
        Err(e) => {
            log::error!("Failed to get contacts list: {}", e);
            let error_response = Response::Error(ErrorResponse {
                error: format!("Failed to get contacts list: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            return Err(e);
        }
    };

    let total_contacts = contacts.len();

    log::debug!("handle_sync_contacts: Starting sync for {} contacts", total_contacts);

    // Send progress event: sync started
    send_colony_progress(&update_tx, ColonyEvent::SyncContactsStarted {
        total_contacts
    }, None);

    match colony_manager.sync_all_contacts_with_progress(update_tx.clone()).await {
        Ok(synced_count) => {
            // Send progress event: sync completed
            send_colony_progress(&update_tx, ColonyEvent::SyncContactsCompleted {
                synced_count
            }, None);

            let response = Response::SyncContacts(SyncContactsResponse { synced_count });

            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send sync contacts response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send sync contacts response: {}", e)));
            }

            log::debug!("Sync contacts request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("Sync contacts request failed: {}", e);

            // Send progress event: operation failed
            send_colony_progress(&update_tx, ColonyEvent::OperationFailed {
                operation: "sync_contacts".to_string(),
                error: e.to_string()
            }, None);

            let error_response = Response::Error(ErrorResponse {
                error: format!("Sync contacts failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            Err(e)
        }
    }
}

/// Handle get user contact requests
///
/// Returns the user's own contact information (wallet address or pod address) that can be shared with friends.
pub async fn handle_get_user_contact(
    _req: GetUserContactRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling get user contact request");

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;

    match colony_manager.get_user_contact_info().await {
        Ok((contact_address, contact_type, display_name)) => {
            let response = Response::GetUserContact(GetUserContactResponse {
                contact_address,
                contact_type,
                display_name,
            });

            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send get user contact response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send get user contact response: {}", e)));
            }

            log::debug!("Get user contact request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("Get user contact request failed: {}", e);
            let error_response = Response::Error(ErrorResponse {
                error: format!("Get user contact failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            Err(e)
        }
    }
}

/// Handle list contacts requests
///
/// Returns the list of contacts (pod addresses) that the user has added.
pub async fn handle_list_contacts(
    _req: ListContactsRequest,
    update_tx: UpdateSender,
    _original_request_str: &str,
    mutant: Arc<mutant_lib::MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling list contacts request");

    let colony_manager = ensure_colony_manager_with_progress(mutant, update_tx.clone()).await?;

    match colony_manager.get_contacts().await {
        Ok(contacts) => {
            let response = Response::ListContacts(ListContactsResponse { contacts });

            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send list contacts response: {}", e);
                return Err(DaemonError::Internal(format!("Failed to send list contacts response: {}", e)));
            }

            log::debug!("List contacts request completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("List contacts request failed: {}", e);
            let error_response = Response::Error(ErrorResponse {
                error: format!("List contacts failed: {}", e),
                original_request: Some(_original_request_str.to_string()),
            });

            if let Err(send_err) = update_tx.send(error_response) {
                log::error!("Failed to send error response: {}", send_err);
            }

            Err(e)
        }
    }
}

/// Refresh the colony cache from the network
///
/// This is a utility function that can be called periodically to update
/// the local RDF graph database with new pods from the network.
#[allow(dead_code)]
pub async fn refresh_colony_cache() -> Result<(), DaemonError> {
    log::info!("Refreshing colony cache from network");

    let colony_manager = get_colony_manager().await?;
    colony_manager.refresh_cache().await?;

    log::info!("Colony cache refresh completed");
    Ok(())
}

/// Refresh the colony cache from the network with progress updates
///
/// This version sends progress events via WebSocket for UI feedback.
#[allow(dead_code)]
pub async fn refresh_colony_cache_with_progress(update_tx: UpdateSender) -> Result<(), DaemonError> {
    log::info!("Refreshing colony cache from network");

    // Send progress event: cache refresh started
    send_colony_progress(&update_tx, ColonyEvent::CacheRefreshStarted, None);

    let colony_manager = get_colony_manager().await?;

    match colony_manager.refresh_cache().await {
        Ok(()) => {
            // Send progress event: cache refresh completed
            send_colony_progress(&update_tx, ColonyEvent::CacheRefreshCompleted, None);

            log::info!("Colony cache refresh completed");
            Ok(())
        }
        Err(e) => {
            // Send progress event: cache refresh failed
            send_colony_progress(&update_tx, ColonyEvent::OperationFailed {
                operation: "cache_refresh".to_string(),
                error: e.to_string()
            }, None);

            Err(e)
        }
    }
}

/// Save the colony key store
///
/// This ensures that derived keys are persisted to disk.
#[allow(dead_code)]
pub async fn save_colony_state() -> Result<(), DaemonError> {
    log::debug!("Saving colony state");
    
    let colony_manager = get_colony_manager().await?;
    colony_manager.save_key_store().await?;
    
    log::debug!("Colony state saved successfully");
    Ok(())
}

/// Check if the colony manager is initialized
#[allow(dead_code)]
pub fn is_colony_initialized() -> bool {
    COLONY_MANAGER.get().is_some()
}

/// Shutdown the colony manager
///
/// This saves the current state and cleans up resources.
#[allow(dead_code)]
pub async fn shutdown_colony() -> Result<(), DaemonError> {
    if is_colony_initialized() {
        log::info!("Shutting down colony manager");
        save_colony_state().await?;
        log::info!("Colony manager shutdown completed");
    }
    Ok(())
}
