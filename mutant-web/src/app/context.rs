use std::{collections::{BTreeMap, HashMap}, sync::{Arc, Mutex, RwLock}};

use lazy_static::lazy_static;
use log::error;
use mutant_protocol::{KeyDetails, StatsResponse, StorageMode, TaskListEntry, TaskId, TaskProgress, SearchResponse, AddContactResponse, ListContentResponse, SyncContactsResponse, GetUserContactResponse, ListContactsResponse};
use tokio::sync::mpsc;


// Import our client manager
// Assuming client_manager is correctly referenced. If it's in the same module or crate::client_manager
use crate::app::client_manager; 
// For Client and ClientSender, the original code used:
use crate::{Client, ClientSender}; // This might be from a higher level module, e.g. crate::client if Client is defined there.
                                   // Or, if Client and ClientSender are type aliases for client_manager types, this might need adjustment.
                                   // Given the existing code structure, `Client` seems to be `client_manager` itself or a wrapper.
                                   // The existing `self.client.list_keys()` implies `self.client` is an instance that has these methods.
                                   // The `Context::new()` does `let client = Client::spawn().await;`
                                   // `Client::spawn()` is not defined in the provided context.rs. It's likely `client_manager::spawn()` or similar.
                                   // For now, I will assume `crate::app::client_manager` is the correct path for the new static function call.
                                   // And `self.client` usage for other methods remains as is.

pub struct ProgressOperation {
    pub nb_to_reserve: usize,
    pub nb_reserved: usize,
    pub total_pads: usize,
    pub nb_written: usize,
    pub nb_confirmed: usize,
}

pub struct Progress {
    pub operation: BTreeMap<String, ProgressOperation>,
}

// Context struct to manage cached data
pub struct Context {
    client: ClientSender,
    keys_cache: Arc<RwLock<Vec<KeyDetails>>>,
    tasks_cache: Arc<RwLock<Vec<TaskListEntry>>>,
    stats_cache: Arc<RwLock<Option<StatsResponse>>>,
    wallet_balance_cache: Arc<RwLock<Option<mutant_protocol::WalletBalanceResponse>>>,
    daemon_status_cache: Arc<RwLock<Option<mutant_protocol::DaemonStatusResponse>>>,
    put_progress: Arc<RwLock<HashMap<String, Arc<RwLock<Progress>>>>>,
    get_progress: Arc<RwLock<HashMap<String, Arc<RwLock<Progress>>>>>,
}

// Create a global context instance
lazy_static! {
    static ref CONTEXT: Arc<Mutex<Option<Arc<Context>>>> = Arc::new(Mutex::new(None));
}

pub async fn init_context() {
    // First, clear any existing context to ensure clean initialization
    *CONTEXT.lock().unwrap() = None;
    log::info!("Cleared existing context for clean initialization");

    let context = Context::new().await;
    *CONTEXT.lock().unwrap() = Some(Arc::new(context));
    log::info!("Context initialization completed");
}

// Public function to get the context
pub fn context() -> Arc<Context> {
    CONTEXT.lock().unwrap().as_ref().expect("Context not initialized - call init_context() first").clone()
}

// Safe function to get the context that returns None if not initialized
pub fn try_context() -> Option<Arc<Context>> {
    CONTEXT.lock().unwrap().as_ref().cloned()
}

impl Context {
    async fn new() -> Self {
        let client = Client::spawn().await;

        // Clear any stale response handlers from previous sessions
        // This is crucial to prevent "request already pending" errors on page refresh
        {
            let responses = client.responses.write().unwrap();
            if !responses.is_empty() {
                log::info!("Cleared {} stale response handlers from previous session", responses.len());
            }
        }
        client.responses.write().unwrap().clear();

        Self {
            client,
            keys_cache: Arc::new(RwLock::new(Vec::new())),
            tasks_cache: Arc::new(RwLock::new(Vec::new())),
            stats_cache: Arc::new(RwLock::new(None)),
            wallet_balance_cache: Arc::new(RwLock::new(None)),
            daemon_status_cache: Arc::new(RwLock::new(None)),
            put_progress: Arc::new(RwLock::new(HashMap::new())),
            get_progress: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn list_keys(&self) -> Vec<KeyDetails> {
        // Create a simple direct call to avoid complex operations
        let result = self.client.list_keys().await;

        if let Ok(keys) = result {

            // Create a minimal copy to avoid complex operations
            let mut safe_keys = Vec::with_capacity(keys.len());

            for k in keys {
                let key_detail = KeyDetails {
                    key: k.key,
                    total_size: k.total_size,
                    pad_count: k.pad_count,
                    confirmed_pads: k.confirmed_pads,
                    is_public: k.is_public,
                    public_address: k.public_address,
                };

                safe_keys.push(key_detail);
            }

            // Update cache
            {
                let mut cache = self.keys_cache.write().unwrap();

                *cache = safe_keys.clone();
            }



            safe_keys
        } else {
            error!("Failed to list keys: {:?}", result.err());

            Vec::new()
        }
    }

    // Get list of tasks from cache only
    pub async fn list_tasks(&self) -> Vec<TaskListEntry> {
        // Create a simple direct call to avoid complex operations
        let result = self.client.list_tasks().await;

        if let Ok(tasks) = result {

            // Create a minimal copy to avoid complex operations
            let mut safe_tasks = Vec::with_capacity(tasks.len());

            for t in tasks {
                let task_entry = TaskListEntry {
                    task_id: t.task_id,
                    status: t.status,
                    task_type: t.task_type,
                };

                safe_tasks.push(task_entry);
            }

            // Update cache and connection state
            {
                let mut cache = self.tasks_cache.write().unwrap();

                *cache = safe_tasks.clone();
            }

            safe_tasks
        } else {
            error!("Failed to list tasks: {:?}", result.err());

            Vec::new()
        }
    }

    // Get stats from daemon and update cache
    pub async fn get_stats(&self) -> Option<StatsResponse> {
        // Create a simple direct call to avoid complex operations
        let result = self.client.get_stats().await;

        if let Ok(stats) = result {

            // Create a safe copy of the stats
            let safe_stats = StatsResponse {
                total_keys: stats.total_keys,
                total_pads: stats.total_pads,
                occupied_pads: stats.occupied_pads,
                free_pads: stats.free_pads,
                pending_verify_pads: stats.pending_verify_pads,
            };

            // Update cache and connection state
            {
                let mut cache = self.stats_cache.write().unwrap();
                *cache = Some(safe_stats.clone());
            }

            Some(safe_stats)
        } else {
            error!("Failed to get stats: {:?}", result.err());
            None
        }
    }

    // // Get task details (not cached)
    // pub async fn get_task(&self, task_id: TaskId) -> Result<Task, String> {
    //     self._get_task(task_id).await
    // }

    // // Get task details directly from daemon
    // pub async fn _get_task(&self, task_id: TaskId) -> Result<Task, String> {
    //     info!("Fetching task details for task {} from daemon", task_id);

    //     // Safely call the client manager
    //     let result = client_manager::get_task(task_id).await;

    //     match result {
    //         Ok(task) => {
    //             // Create a safe copy of the task
    //             let safe_task = Task {
    //                 id: task.id,
    //                 task_type: task.task_type,
    //                 status: task.status,
    //                 progress: task.progress.clone(),
    //                 result: task.result.clone(),
    //                 key: task.key.clone(),
    //             };

    //             Ok(safe_task)
    //         }
    //         Err(e) => {
    //             error!("Failed to get task details: {}", e);
    //             Err(e)
    //         }
    //     }
    // }

    // // Stop a task
    // pub async fn stop_task(&self, task_id: TaskId) -> Result<(), String> {
    //     self._stop_task(task_id).await
    // }

    // // Stop a task directly from daemon
    // pub async fn _stop_task(&self, task_id: TaskId) -> Result<(), String> {
    //     info!("Stopping task {} via daemon", task_id);

    //     // Safely call the client manager
    //     let result = client_manager::stop_task(task_id).await;

    //     match result {
    //         Ok(_) => {
    //             // Invalidate tasks cache
    //             *self.tasks_cache.write().unwrap() = None;
    //             Ok(())
    //         }
    //         Err(e) => {
    //             error!("Failed to stop task: {}", e);
    //             Err(e)
    //         }
    //     }
    // }

    // Get a key (not cached)
    pub async fn get_key(&self, name: &str, destination: &str) -> Result<(), String> {

        // Safely call the client manager
        let result = self.client.get(name.to_string(), Some(destination.to_string()), false).await;

        match result {
            Ok(_) => {
                Ok(())
            },
            Err(e) => {
                error!("Failed to get key: {}", e);
                Err(e)
            }
        }
    }





    pub async fn start_streamed_get(
        &self,
        user_key: &str,
        is_public: bool,
    ) -> Result<
        (
            TaskId,
            mpsc::UnboundedReceiver<Result<TaskProgress, String>>,
            mpsc::UnboundedReceiver<Result<Vec<u8>, String>>,
        ),
        String,
    > {

        // Note: `self.client` is of type `ClientSender` which seems to be the sender to the client_manager.
        // The `start_get_stream` function is a free function in `client_manager.rs`.
        // So, we should call `client_manager::start_get_stream` directly.
        client_manager::start_get_stream(user_key, is_public).await
    }



    // Get file content for viewing with progress callback
    pub async fn get_file_for_viewing_with_progress<F>(
        &self,
        user_key: &str,
        is_public: bool,
        mut progress_callback: F
    ) -> Result<Vec<u8>, String>
    where
        F: FnMut(usize, Option<usize>) + Send + 'static,
    {
        // Start the streaming get
        let (_task_id, mut _progress_receiver, mut data_receiver) = self.start_streamed_get(user_key, is_public).await?;

        let mut collected_data = Vec::new();

        // Collect all data chunks
        while let Some(data_result) = data_receiver.recv().await {
            match data_result {
                Ok(chunk) => {
                    collected_data.extend_from_slice(&chunk);

                    // Call progress callback
                    progress_callback(collected_data.len(), None);
                }
                Err(e) => {
                    error!("Error receiving data chunk for viewing: {}", e);
                    return Err(format!("Error receiving data chunk: {}", e));
                }
            }
        }


        Ok(collected_data)
    }

    pub fn get_key_cache(&self) -> Arc<RwLock<Vec<KeyDetails>>> {
        self.keys_cache.clone()
    }

    pub fn get_task_cache(&self) -> Arc<RwLock<Vec<TaskListEntry>>> {
        self.tasks_cache.clone()
    }

    pub fn get_stats_cache(&self) -> Arc<RwLock<Option<StatsResponse>>> {
        self.stats_cache.clone()
    }

    // Get wallet balance from daemon and update cache
    pub async fn get_wallet_balance(&self) -> Option<mutant_protocol::WalletBalanceResponse> {
        let result = self.client.get_wallet_balance().await;

        if let Ok(balance) = result {
            // Update cache
            {
                let mut cache = self.wallet_balance_cache.write().unwrap();
                *cache = Some(balance.clone());
            }

            Some(balance)
        } else {
            log::error!("Failed to get wallet balance: {:?}", result.err());
            None
        }
    }

    pub fn get_wallet_balance_cache(&self) -> Arc<RwLock<Option<mutant_protocol::WalletBalanceResponse>>> {
        self.wallet_balance_cache.clone()
    }

    // Get daemon status from daemon and update cache
    pub async fn get_daemon_status(&self) -> Option<mutant_protocol::DaemonStatusResponse> {
        let result = self.client.get_daemon_status().await;

        if let Ok(status) = result {
            // Update cache
            {
                let mut cache = self.daemon_status_cache.write().unwrap();
                *cache = Some(status.clone());
            }

            Some(status)
        } else {
            log::error!("Failed to get daemon status: {:?}", result.err());
            None
        }
    }

    pub fn get_daemon_status_cache(&self) -> Arc<RwLock<Option<mutant_protocol::DaemonStatusResponse>>> {
        self.daemon_status_cache.clone()
    }

    // --- Onboarding Methods ---

    /// Check if onboarding is needed
    pub async fn check_onboarding_status(&self) -> Result<mutant_protocol::OnboardingStatusResponse, String> {
        self.client.check_onboarding_status().await.map_err(|e| e.to_string())
    }

    /// Set credentials for the daemon
    pub async fn set_credentials(&self, private_key: Option<String>, colony_mnemonic: Option<String>) -> Result<mutant_protocol::SetCredentialsResponse, String> {
        self.client.set_credentials(private_key, colony_mnemonic).await.map_err(|e| e.to_string())
    }

    /// Initialize colony after credentials are set
    pub async fn initialize_colony(&self) -> Result<mutant_protocol::InitializeColonyResponse, String> {
        self.client.initialize_colony().await.map_err(|e| e.to_string())
    }

    /// Restart the daemon to reload credentials
    pub async fn restart_daemon(&self) -> Result<mutant_protocol::RestartDaemonResponse, String> {
        self.client.restart_daemon().await.map_err(|e| e.to_string())
    }

    /// Reload credentials without restarting the daemon
    pub async fn reload_credentials(&self) -> Result<mutant_protocol::ReloadCredentialsResponse, String> {
        self.client.reload_credentials().await.map_err(|e| e.to_string())
    }

    // Create a new progress object for tracking a put operation
    pub fn create_progress(&self, key: &str, filename: &str) -> (String, Arc<RwLock<Progress>>) {
        // Generate a unique ID for this put operation
        let put_id = format!("put_{}_{}", key, filename);

        // Create a new Progress object
        let progress = Arc::new(RwLock::new(Progress {
            operation: BTreeMap::new(),
        }));

        // Store the progress in our map
        {
            let mut put_progress = self.put_progress.write().unwrap();
            put_progress.insert(put_id.clone(), progress.clone());
        }


        (put_id, progress)
    }

    // Put a key (not cached)
    pub async fn put(
        &self,
        key: &str,
        data: Vec<u8>,
        filename: &str,
        mode: StorageMode,
        public: bool,
        no_verify: bool,
        progress_opt: Option<(String, Arc<RwLock<Progress>>)>,
    ) -> Result<(String, Arc<RwLock<Progress>>), String> {


        // Use the provided progress object or create a new one
        let (put_id, progress) = match progress_opt {
            Some((id, prog)) => (id, prog),
            None => self.create_progress(key, filename)
        };

        // Safely call the client manager with the progress object
        let result = self.client.put(
            key.to_string(),
            data,
            filename.to_string(),
            mode,
            public,
            no_verify,
            Some(progress.clone())
        ).await;

        match result {
            Ok(_task_result) => {
                // Return the put_id and progress
                Ok((put_id, progress))
            },
            Err(e) => {
                error!("Failed to put key: {}", e);
                Err(e)
            }
        }
    }

    // Create a new progress object for tracking a get operation
    pub fn create_get_progress(&self, key: &str) -> (String, Arc<RwLock<Progress>>) {
        // Generate a unique ID for this get operation
        let get_id = format!("get_{}", key);

        // Create a new Progress object
        let progress = Arc::new(RwLock::new(Progress {
            operation: BTreeMap::new(),
        }));

        // Store the progress in our map
        {
            let mut get_progress = self.get_progress.write().unwrap();
            get_progress.insert(get_id.clone(), progress.clone());
        }


        (get_id, progress)
    }

    // Get a progress object for a put operation
    pub fn get_put_progress(&self, put_id: &str) -> Option<Arc<RwLock<Progress>>> {
        let put_progress = self.put_progress.read().unwrap();
        put_progress.get(put_id).cloned()
    }



    // Get a progress object for a get operation
    pub fn get_get_progress(&self, get_id: &str) -> Option<Arc<RwLock<Progress>>> {
        let get_progress = self.get_progress.read().unwrap();
        get_progress.get(get_id).cloned()
    }

    // Get the client sender
    pub fn get_client_sender(&self) -> Arc<ClientSender> {
        Arc::new(self.client.clone())
    }

    pub async fn mv(&self, old_key: &str, new_key: &str) -> Result<(), String> {
        match self.client.mv(old_key.to_string(), new_key.to_string()).await {
            Ok(_) => {
                // Refresh the key list
                let _ = self.list_keys().await;
                Ok(())
            },
            Err(e) => {
                error!("Failed to rename key '{}' to '{}': {}", old_key, new_key, e);
                Err(e)
            }
        }
    }

    // Streaming put methods
    pub async fn put_streaming_init(
        &self,
        key_name: &str,
        total_size: u64,
        filename: &str,
        storage_mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
    ) -> Result<TaskId, String> {


        // Initialize the streaming put and get both task ID and progress receiver
        let (task_id, progress_rx) = self.client.put_streaming_init(key_name, total_size, filename, storage_mode, public, no_verify).await?;

        // Create a progress object for this streaming put operation
        let progress = Arc::new(RwLock::new(Progress {
            operation: BTreeMap::new(),
        }));

        // Store the progress in our context map using the task ID as the key
        {
            let mut put_progress = self.put_progress.write().unwrap();
            put_progress.insert(task_id.to_string(), progress.clone());
        }

        // CRITICAL: Also store the progress in the ClientSender's put_progress map
        // This is what the existing response handler uses to update progress
        let client_sender = self.get_client_sender();
        {
            let mut client_put_progress = client_sender.put_progress.write().unwrap();
            client_put_progress.insert(task_id.to_string(), progress.clone());
        }



        // Start listening for progress updates for this task using the progress receiver
        self.start_streaming_put_progress_listener(task_id, progress, progress_rx).await;

        Ok(task_id)
    }

    pub async fn put_streaming_chunk(
        &self,
        task_id: TaskId,
        chunk_index: usize,
        total_chunks: usize,
        data: Vec<u8>,
        is_last: bool,
    ) -> Result<(), String> {
        self.client.put_streaming_chunk(task_id, chunk_index, total_chunks, data, is_last).await
    }

    // Start listening for progress updates for a streaming put operation
    async fn start_streaming_put_progress_listener(&self, task_id: TaskId, progress: Arc<RwLock<Progress>>, progress_rx: mutant_client::ProgressReceiver) {

        // Spawn a task to handle progress updates
        wasm_bindgen_futures::spawn_local(async move {
            let mut progress_rx = progress_rx;
            let mut progress_count = 0;



            while let Some(progress_result) = progress_rx.recv().await {
                progress_count += 1;
                match progress_result {
                    Ok(task_progress) => {
                        match task_progress {
                            mutant_protocol::TaskProgress::Put(put_event) => {
                                match put_event {
                                    mutant_protocol::PutEvent::Starting { total_chunks, initial_written_count, initial_confirmed_count, chunks_to_reserve } => {
                                        // Initialize the operation in the progress object
                                        let mut progress_guard = progress.write().unwrap();
                                        progress_guard.operation.insert("put".to_string(), ProgressOperation {
                                            nb_to_reserve: chunks_to_reserve,
                                            nb_reserved: 0,
                                            total_pads: total_chunks,
                                            nb_written: initial_written_count,
                                            nb_confirmed: initial_confirmed_count,
                                        });
                                    },
                                    mutant_protocol::PutEvent::PadReserved => {
                                        // Update reserved count
                                        let mut progress_guard = progress.write().unwrap();
                                        if let Some(op) = progress_guard.operation.get_mut("put") {
                                            op.nb_reserved += 1;
                                        }
                                    },
                                    mutant_protocol::PutEvent::PadsWritten => {
                                        // Update written count
                                        let mut progress_guard = progress.write().unwrap();
                                        if let Some(op) = progress_guard.operation.get_mut("put") {
                                            op.nb_written += 1;
                                        }
                                    },
                                    mutant_protocol::PutEvent::PadsConfirmed => {
                                        // Update confirmed count
                                        let mut progress_guard = progress.write().unwrap();
                                        if let Some(op) = progress_guard.operation.get_mut("put") {
                                            op.nb_confirmed += 1;
                                        }
                                    },
                                    mutant_protocol::PutEvent::Complete => {
                                        // Mark operation as complete
                                        let mut progress_guard = progress.write().unwrap();
                                        if let Some(op) = progress_guard.operation.get_mut("put") {
                                            op.nb_confirmed = op.total_pads;
                                        }
                                        break; // Exit the loop when complete
                                    },
                                }
                            },
                            _ => {
                                // Unexpected progress type
                            }
                        }
                    },
                    Err(e) => {
                        log::error!("Streaming put progress #{} for task {}: Error: {:?}", progress_count, task_id, e);
                        break;
                    }
                }
            }


        });
    }

    // Colony integration methods

    /// Search for content using SPARQL queries
    pub async fn search(&self, query: serde_json::Value) -> Result<SearchResponse, String> {
        client_manager::search(query).await
    }

    /// Add a contact pod address to sync with
    pub async fn add_contact(&self, pod_address: &str, contact_name: Option<String>) -> Result<AddContactResponse, String> {
        client_manager::add_contact(pod_address, contact_name).await
    }

    /// List all available content from synced pods
    pub async fn list_content(&self) -> Result<ListContentResponse, String> {
        client_manager::list_content().await
    }

    /// Sync all contact pods to get the latest content
    pub async fn sync_contacts(&self) -> Result<SyncContactsResponse, String> {
        client_manager::sync_contacts().await
    }

    /// Get the user's own contact information that can be shared with friends
    pub async fn get_user_contact(&self) -> Result<GetUserContactResponse, String> {
        client_manager::get_user_contact().await
    }

    pub async fn list_contacts(&self) -> Result<ListContactsResponse, String> {
        client_manager::list_contacts().await
    }

    /// Remove a key from the daemon
    pub async fn rm(&self, user_key: &str) -> Result<(), String> {
        let result = client_manager::rm(user_key).await;

        // If successful, invalidate the keys cache to force a refresh
        if result.is_ok() {
            let mut cache = self.keys_cache.write().unwrap();
            cache.clear(); // Clear cache to force refresh on next list_keys call
        }

        result
    }

    // --- Filesystem Navigation Methods ---

    /// List the contents of a directory on the daemon's filesystem
    pub async fn list_directory(&self, path: &str) -> Result<mutant_protocol::ListDirectoryResponse, String> {
        client_manager::list_directory(path).await
    }

    /// Get information about a file or directory on the daemon's filesystem
    pub async fn get_file_info(&self, path: &str) -> Result<mutant_protocol::GetFileInfoResponse, String> {
        client_manager::get_file_info(path).await
    }

    /// Upload a file using its filesystem path (no streaming to web client)
    pub async fn put_file_path(
        &self,
        key: &str,
        file_path: &str,
        public: bool,
        mode: StorageMode,
        no_verify: bool,
    ) -> Result<(String, tokio::sync::mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>), String> {
        client_manager::put_file_path(key, file_path, mode, public, no_verify).await
    }

    /// Download a file directly to the filesystem path
    pub async fn download_to_path(
        &self,
        key: &str,
        destination_path: &str,
        is_public: bool,
    ) -> Result<(), String> {
        client_manager::download_to_path(key, destination_path, is_public).await
    }

    /// Download a file with progress tracking
    pub async fn download_to_path_with_progress(
        &self,
        key: &str,
        destination_path: &str,
        is_public: bool,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>, String> {
        client_manager::download_to_path_with_progress(key, destination_path, is_public).await
    }
}
