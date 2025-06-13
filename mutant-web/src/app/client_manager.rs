use std::sync::Arc;
use futures::{channel::oneshot, StreamExt};
use lazy_static::lazy_static;
use log::{error, info, warn};
use mutant_client::MutantClient;
use mutant_protocol::{KeyDetails, StatsResponse, Task, TaskId, TaskListEntry, TaskProgress, SearchResponse, AddContactResponse, ListContentResponse, SyncContactsResponse, GetUserContactResponse, ListContactsResponse};
use tokio::sync::mpsc;
use wasm_bindgen_futures::spawn_local;

// Function to get the WebSocket URL based on current domain
pub fn get_daemon_ws_url() -> String {
    let window = web_sys::window().expect("no global `window` exists");
    let location = window.location();
    let hostname = location.hostname().expect("should have a hostname");

    // Use the hostname with the default daemon port 3030
    let daemon_url = format!("ws://{}:3030/ws", hostname);
    log::info!("Dynamic WebSocket URL: {}", daemon_url);
    daemon_url
}

// Default WebSocket URL for the daemon (now dynamic)
pub fn default_ws_url() -> String {
    get_daemon_ws_url()
}

// Commands that can be sent to the client manager
enum ClientCommand {
    ListKeys(oneshot::Sender<Result<Vec<KeyDetails>, String>>),
    ListTasks(oneshot::Sender<Result<Vec<TaskListEntry>, String>>),
    GetStats(oneshot::Sender<Result<StatsResponse, String>>),
    GetTask(TaskId, oneshot::Sender<Result<Task, String>>),
    StopTask(TaskId, oneshot::Sender<Result<(), String>>),
    GetKey(String, String, oneshot::Sender<Result<(), String>>),
    Put(String, Vec<u8>, String, mutant_protocol::StorageMode, bool, bool, oneshot::Sender<Result<(TaskId, mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>), String>>),
    Rm(String, oneshot::Sender<Result<(), String>>), // user_key, response_sender
    StartGetStream(
        String, // name
        bool,   // is_public
        oneshot::Sender<
            Result<
                (
                    mutant_protocol::TaskId,
                    tokio::sync::mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>,
                    tokio::sync::mpsc::UnboundedReceiver<Result<Vec<u8>, String>>
                ),
                String // Error string
            >,
        >,
    ),
    Reconnect,
    Shutdown,
    // Colony integration commands
    Search(serde_json::Value, oneshot::Sender<Result<SearchResponse, String>>),
    AddContact(String, Option<String>, oneshot::Sender<Result<AddContactResponse, String>>),
    ListContent(oneshot::Sender<Result<ListContentResponse, String>>),
    SyncContacts(oneshot::Sender<Result<SyncContactsResponse, String>>),
    GetUserContact(oneshot::Sender<Result<GetUserContactResponse, String>>),
    ListContacts(oneshot::Sender<Result<ListContactsResponse, String>>),
    // Filesystem navigation
    ListDirectory(String, oneshot::Sender<Result<mutant_protocol::ListDirectoryResponse, String>>),
    GetFileInfo(String, oneshot::Sender<Result<mutant_protocol::GetFileInfoResponse, String>>),
    // File path upload
    PutFilePath(String, String, mutant_protocol::StorageMode, bool, bool, oneshot::Sender<Result<(String, mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>), String>>),
    // File path download
    DownloadToPath(String, String, bool, oneshot::Sender<Result<(), String>>),
    // File path download with progress
    DownloadToPathWithProgress(String, String, bool, oneshot::Sender<Result<mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>, String>>),
}

// Singleton channel to the client manager
lazy_static! {
    static ref CLIENT_COMMAND_SENDER: Arc<futures::channel::mpsc::UnboundedSender<ClientCommand>> = {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        spawn_client_manager(rx);
        Arc::new(tx)
    };
}

// Helper function to ensure connection and handle errors with retry logic
async fn ensure_connected(client: &mut MutantClient, connected: &mut bool) -> Result<(), String> {
    if *connected && client.is_connected() {
        return Ok(());
    }

    // If we think we're connected but the client says otherwise, reset our state
    if *connected && !client.is_connected() {
        warn!("Connection state mismatch detected, resetting connection");
        *connected = false;
        client.disconnect();
    }

    let ws_url = default_ws_url();
    info!("Attempting to connect to daemon at: {}", ws_url);

    // Use the new retry logic with 3 attempts and exponential backoff
    match client.connect_with_retry(&ws_url, 3, 500).await {
        Ok(_) => {
            info!("Successfully connected to daemon");
            *connected = true;
            Ok(())
        }
        Err(e) => {
            error!("Failed to connect to daemon after retries: {:?}", e);
            *connected = false;
            client.disconnect(); // Ensure clean state
            Err(format!("Failed to connect after retries: {:?}", e))
        }
    }
}

// Helper function to handle get operations in a separate task
async fn handle_get_operation(
    mut client: MutantClient,
    name: String,
    destination: String,
    sender: oneshot::Sender<Result<(), String>>
) {
    // Connect the client first since cloned clients don't have a connection
    let ws_url = default_ws_url();
    if let Err(e) = client.connect(&ws_url).await {
        error!("Failed to connect cloned client: {:?}", e);
        if !sender.is_canceled() {
            let _ = sender.send(Err(format!("Failed to connect: {:?}", e)));
        }
        return;
    }

    // Execute the command
    match client.get(&name, Some(&destination), false, false).await {
        Ok((_task_id, task_future, _progress_rx, _data_stream_rx)) => {
            // Wait for the task to complete
            match task_future.await {
                Ok(_result) => {
                    // Only send if the receiver is still interested
                    if !sender.is_canceled() {
                        let _ = sender.send(Ok(()));
                    }
                },
                Err(e) => {
                    error!("Get task failed: {:?}", e);
                    // Only send if the receiver is still interested
                    if !sender.is_canceled() {
                        let _ = sender.send(Err(format!("Get task failed: {:?}", e)));
                    }
                }
            }
        },
        Err(e) => {
            // Check if it's a connection error
            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                warn!("Connection lost during get operation");
            }

            // Only send if the receiver is still interested
            if !sender.is_canceled() {
                let _ = sender.send(Err(format!("Failed to start get task: {:?}", e)));
            }
        }
    }
}

/// Initialize the client manager early to ensure colony progress callback is set up
pub fn init_client_manager() {
    // Access the CLIENT_COMMAND_SENDER to trigger lazy_static initialization
    // This ensures the colony progress callback is set up early
    lazy_static::initialize(&CLIENT_COMMAND_SENDER);
    log::debug!("Client manager initialized early for colony progress callback setup");
}

// Start the client manager in a background task
fn spawn_client_manager(mut rx: futures::channel::mpsc::UnboundedReceiver<ClientCommand>) {
    spawn_local(async move {
        // Set up the colony progress callback with error handling
        #[cfg(target_arch = "wasm32")]
        {
            log::info!("CLIENT MANAGER: Setting up colony progress callback...");
            mutant_client::set_colony_progress_callback(|event, operation_id| {
                log::info!("CLIENT MANAGER: Received colony progress event via callback: {:?}", event);
                // Add extra safety by wrapping the callback in a simple error handler
                // Don't use catch_unwind as it's not properly supported in WebAssembly
                crate::app::colony_window::add_colony_progress_event(event.clone(), operation_id.clone());

                // Also update the global colony initialization state
                crate::app::onboarding::handle_daemon_colony_progress(event, operation_id);

                log::info!("CLIENT MANAGER: Colony progress event processed successfully");
            });
            log::info!("CLIENT MANAGER: Colony progress callback setup completed");
        }

        // Create a single client that will be used for all operations
        let mut client = MutantClient::new();
        let mut connected = false;

        // Process commands until shutdown
        while let Some(cmd) = rx.next().await {
            match cmd {
                ClientCommand::ListKeys(sender) => {
                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Execute the command with proper error handling
                    let result = match client.list_keys().await {
                        Ok(keys) => {
                            Ok(keys)
                        }
                        Err(e) => {
                            // Check if it's a connection error
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during list_keys, will reconnect on next request: {:?}", e);
                            } else {
                                error!("Failed to list keys: {:?}", e);
                            }
                            Err(format!("Failed to list keys: {:?}", e))
                        }
                    };

                    // Only send if the receiver is still interested
                    if !sender.is_canceled() {
                        // Create a safe copy of the result to avoid memory issues
                        let safe_result = match &result {
                            Ok(keys) => {
                                // Create a new vector with cloned keys to avoid memory issues
                                let safe_keys = keys.clone();
                                Ok(safe_keys)
                            },
                            Err(e) => {
                                // Create a new error string to avoid memory issues
                                Err(e.clone())
                            }
                        };

                        // Send the safe result
                        if let Err(e) = sender.send(safe_result) {
                            error!("Failed to send list_keys response: {:?}", e)
                        }
                    }
                }
                ClientCommand::ListTasks(sender) => {
                    info!("Processing ListTasks command");

                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        info!("Connection failed for ListTasks: {}", e);
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    info!("Connection established, sending list_tasks request");

                    // Execute the command with proper error handling
                    let result = match client.list_tasks().await {
                        Ok(tasks) => {
                            info!("Successfully retrieved {} tasks", tasks.len());
                            Ok(tasks)
                        }
                        Err(e) => {
                            // Check if it's a connection error
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during list_tasks, will reconnect on next request: {:?}", e);
                            } else {
                                error!("Failed to list tasks: {:?}", e);
                            }
                            Err(format!("Failed to list tasks: {:?}", e))
                        }
                    };

                    // Only send if the receiver is still interested
                    if !sender.is_canceled() {
                        info!("Sending list_tasks response to caller");

                        // Create a safe copy of the result to avoid memory issues
                        let safe_result = match &result {
                            Ok(tasks) => {
                                // Create a new vector with cloned tasks to avoid memory issues
                                let safe_tasks = tasks.clone();
                                Ok(safe_tasks)
                            },
                            Err(e) => {
                                // Create a new error string to avoid memory issues
                                Err(e.clone())
                            }
                        };

                        // Send the safe result
                        match sender.send(safe_result) {
                            Ok(_) => info!("Successfully sent list_tasks response to caller"),
                            Err(e) => error!("Failed to send list_tasks response: {:?}", e)
                        }
                    } else {
                        info!("Receiver canceled, not sending list_tasks response");
                    }
                }
                ClientCommand::GetStats(sender) => {
                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Execute the command
                    match client.get_stats().await {
                        Ok(stats) => {
                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(stats));
                            }
                        }
                        Err(e) => {
                            // Check if it's a connection error
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost, will reconnect on next request");
                            }

                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to get stats: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::GetTask(task_id, sender) => {
                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Execute the command
                    match client.query_task(task_id).await {
                        Ok(task) => {
                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(task));
                            }
                        }
                        Err(e) => {
                            // Check if it's a connection error
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost, will reconnect on next request");
                            }

                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to get task: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::StopTask(task_id, sender) => {
                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Execute the command
                    match client.stop_task(task_id).await {
                        Ok(_) => {
                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(()));
                            }
                        }
                        Err(e) => {
                            // Check if it's a connection error
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost, will reconnect on next request");
                            }

                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to stop task: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::GetKey(name, destination, sender) => {
                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Create a new client for the get operation to avoid lifetime issues
                    let get_client = client.clone();

                    // Spawn a separate task to handle the get operation
                    // This avoids the 'static lifetime requirement for the task future
                    spawn_local(handle_get_operation(get_client, name, destination, sender));
                }
                ClientCommand::Reconnect => {
                    // Force a reconnection
                    connected = false;
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        warn!("Reconnection failed: {}", e);
                    }
                }
                ClientCommand::Put(key, data, filename, mode, public, no_verify, sender) => {
                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Create a new client for the put operation to avoid lifetime issues
                    let mut put_client = client.clone();

                    // Spawn a separate task to handle the put operation
                    spawn_local(async move {
                        // Connect the client first since cloned clients don't have a connection
                        let ws_url = default_ws_url();
                        if let Err(e) = put_client.connect(&ws_url).await {
                            error!("Failed to connect cloned client: {:?}", e);
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to connect: {:?}", e)));
                            }
                            return;
                        }

                        // In a web context, we need to pass the data directly to the client

                        // Clone the data once before using it
                        let data_clone = data.clone();

                        info!("Uploading file: {} bytes", data_clone.len());
                        // Use standard upload
                        let result = put_client.put_bytes(
                            &key,
                            data_clone,
                            Some(filename.clone()),
                            mode,
                            public,
                            no_verify
                        ).await;

                            match result {
                                Ok((task_future, mut progress_rx, _)) => {
                                    // Create a channel to forward progress updates
                                    let (progress_tx, progress_rx_out) = mpsc::unbounded_channel();

                                    // Extract the task ID before we move the task_future into the spawn_local
                                    let task_id = TaskId::new_v4(); // We'll still use a placeholder for now

                                    // Forward progress updates in a separate task
                                    spawn_local(async move {
                                        while let Some(progress) = progress_rx.recv().await {
                                            match progress {
                                                Ok(progress) => {
                                                    let _ = progress_tx.send(Ok(progress));
                                                }
                                                Err(e) => {
                                                    let _ = progress_tx.send(Err(format!("Progress error: {:?}", e)));
                                                    break;
                                                }
                                            }
                                        }
                                    });

                                    // Send the task ID and progress receiver immediately
                                    // so the UI can start showing progress
                                    if !sender.is_canceled() {
                                        let _ = sender.send(Ok((task_id, progress_rx_out)));
                                    }

                                    // Now await the task_future directly
                                    // This is crucial - without this, the task won't actually start executing
                                    info!("Awaiting standard put task future");

                                    // We need to handle the task future here to avoid lifetime issues
                                    // with spawning another task
                                    match task_future.await {
                                        Ok(result) => {
                                            info!("Standard put task completed with result: {:?}", result);
                                        }
                                        Err(e) => {
                                            error!("Standard put task failed: {:?}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Check if it's a connection error
                                    if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                        warn!("Connection lost during standard put operation");
                                    }

                                    // Only send if the receiver is still interested
                                    if !sender.is_canceled() {
                                        let _ = sender.send(Err(format!("Failed to start standard put task: {:?}", e)));
                                    }
                                }
                            }
                    });
                }
                ClientCommand::Rm(user_key, sender) => {
                    // Ensure we're connected
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        // Only send if the receiver is still interested
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Execute the rm command
                    match client.rm(&user_key).await {
                        Ok(_) => {
                            info!("Successfully removed key: {}", user_key);
                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(()));
                            }
                        }
                        Err(e) => {
                            // Check if it's a connection error
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during rm operation, will reconnect on next request");
                            }

                            error!("Failed to remove key {}: {:?}", user_key, e);
                            // Only send if the receiver is still interested
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to remove key: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::Shutdown => {
                    break;
                }
                // Colony integration command handlers
                ClientCommand::Search(query, sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.search(query).await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during search, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to search: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::AddContact(pod_address, contact_name, sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.add_contact(&pod_address, contact_name).await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during add contact, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to add contact: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::ListContent(sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.list_content().await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during list content, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to list content: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::SyncContacts(sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.sync_contacts().await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during sync contacts, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to sync contacts: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::GetUserContact(sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.get_user_contact().await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during get user contact, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to get user contact: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::ListContacts(sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.list_contacts().await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during list contacts, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to list contacts: {:?}", e)));
                            }
                        }
                    }
                }
                // Filesystem navigation command handlers
                ClientCommand::ListDirectory(path, sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.list_directory(&path).await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during list directory, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to list directory: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::GetFileInfo(path, sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    match client.get_file_info(&path).await {
                        Ok(response) => {
                            if !sender.is_canceled() {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                connected = false;
                                warn!("Connection lost during get file info, will reconnect on next request");
                            }
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to get file info: {:?}", e)));
                            }
                        }
                    }
                }
                ClientCommand::PutFilePath(key, file_path, mode, public, no_verify, sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Create a new client for the put operation to avoid lifetime issues
                    let mut put_client = client.clone();

                    // Spawn a separate task to handle the put operation
                    spawn_local(async move {
                        // Connect the client first since cloned clients don't have a connection
                        let ws_url = default_ws_url();
                        if let Err(e) = put_client.connect(&ws_url).await {
                            error!("Failed to connect cloned client: {:?}", e);
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to connect: {:?}", e)));
                            }
                            return;
                        }

                        info!("Starting file path upload: {} -> {}", file_path, key);

                        // Use the file path put method
                        match put_client.put(&key, &file_path, mode, public, no_verify).await {
                            Ok((task_id, _task_future, progress_rx, _data_stream)) => {
                                // Now we have the real task ID from the daemon!
                                // Forward the progress receiver to the caller
                                if !sender.is_canceled() {
                                    // Forward progress_rx, mapping errors to String
                                    let (prog_tx_fwd, prog_rx_fwd) = mpsc::unbounded_channel();
                                    let mut original_progress_rx = progress_rx;
                                    spawn_local(async move {
                                        while let Some(res) = original_progress_rx.recv().await {
                                            if prog_tx_fwd.send(res.map_err(|e| e.to_string())).is_err() {
                                                break;
                                            }
                                        }
                                    });

                                    let _ = sender.send(Ok((task_id.to_string(), prog_rx_fwd)));
                                }
                            }
                            Err(e) => {
                                error!("Failed to start file path upload: {:?}", e);
                                if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                    // Mark as disconnected for reconnection
                                    // Note: we can't update the main client's connected state from here
                                    warn!("Connection lost during put file path, will reconnect on next request");
                                }
                                if !sender.is_canceled() {
                                    let _ = sender.send(Err(format!("Failed to start upload: {:?}", e)));
                                }
                            }
                        }
                    });
                }
                ClientCommand::DownloadToPath(key, destination_path, is_public, sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Create a new client for the download operation to avoid lifetime issues
                    let mut download_client = client.clone();

                    // Spawn a separate task to handle the download operation
                    spawn_local(async move {
                        // Connect the client first since cloned clients don't have a connection
                        let ws_url = default_ws_url();
                        if let Err(e) = download_client.connect(&ws_url).await {
                            error!("Failed to connect cloned client: {:?}", e);
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to connect: {:?}", e)));
                            }
                            return;
                        }

                        info!("Starting file path download: {} -> {}", key, destination_path);

                        // Use the get method with destination path (no streaming)
                        match download_client.get(&key, Some(&destination_path), is_public, false).await {
                            Ok((_task_id, task_future, _progress_rx, _data_stream)) => {
                                // Wait for the download to complete
                                match task_future.await {
                                    Ok(_result) => {
                                        info!("Download completed successfully: {} -> {}", key, destination_path);
                                        if !sender.is_canceled() {
                                            let _ = sender.send(Ok(()));
                                        }
                                    }
                                    Err(e) => {
                                        error!("Download task failed: {:?}", e);
                                        if !sender.is_canceled() {
                                            let _ = sender.send(Err(format!("Download failed: {:?}", e)));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to start file path download: {:?}", e);
                                if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                    warn!("Connection lost during download, will reconnect on next request");
                                }
                                if !sender.is_canceled() {
                                    let _ = sender.send(Err(format!("Failed to start download: {:?}", e)));
                                }
                            }
                        }
                    });
                }
                ClientCommand::DownloadToPathWithProgress(key, destination_path, is_public, sender) => {
                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }

                    // Create a new client for the download operation to avoid lifetime issues
                    let mut download_client = client.clone();

                    // Spawn a separate task to handle the download operation
                    spawn_local(async move {
                        // Connect the client first since cloned clients don't have a connection
                        let ws_url = default_ws_url();
                        if let Err(e) = download_client.connect(&ws_url).await {
                            error!("Failed to connect cloned client: {:?}", e);
                            if !sender.is_canceled() {
                                let _ = sender.send(Err(format!("Failed to connect: {:?}", e)));
                            }
                            return;
                        }

                        info!("Starting file path download with progress: {} -> {}", key, destination_path);

                        // Use the get method with destination path (no streaming)
                        match download_client.get(&key, Some(&destination_path), is_public, false).await {
                            Ok((_task_id, task_future, progress_rx, _data_stream)) => {
                                // Convert the progress receiver to use String errors
                                let (tx, rx) = mpsc::unbounded_channel();
                                let mut original_progress_rx = progress_rx;

                                // Spawn a task to forward progress with error conversion
                                spawn_local(async move {
                                    while let Some(res) = original_progress_rx.recv().await {
                                        if tx.send(res.map_err(|e| e.to_string())).is_err() {
                                            break;
                                        }
                                    }
                                });

                                // Return the converted progress receiver immediately
                                if !sender.is_canceled() {
                                    let _ = sender.send(Ok(rx));
                                }

                                // Wait for the download to complete in the background
                                match task_future.await {
                                    Ok(_result) => {
                                        info!("Download with progress completed successfully: {} -> {}", key, destination_path);
                                    }
                                    Err(e) => {
                                        error!("Download with progress task failed: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to start file path download with progress: {:?}", e);
                                if e.to_string().contains("connection") || e.to_string().contains("websocket") {
                                    warn!("Connection lost during download with progress, will reconnect on next request");
                                }
                                if !sender.is_canceled() {
                                    let _ = sender.send(Err(format!("Failed to start download: {:?}", e)));
                                }
                            }
                        }
                    });
                }
                ClientCommand::StartGetStream(name, is_public, sender) => {
                    info!("Processing StartGetStream command for key: {}", name);

                    if let Err(e) = ensure_connected(&mut client, &mut connected).await {
                        if !sender.is_canceled() {
                            let _ = sender.send(Err(e));
                        }
                        continue;
                    }
                    
                    // `MutantClient::get` now returns `Result<(TaskId, impl Future<...>, ProgressReceiver, Option<DataStreamReceiver>), ClientError>`
                    match client.get(&name, None, is_public, true).await {
                        Ok((task_id, _overall_completion_future, client_progress_rx, client_data_stream_rx_option)) => {
                            // We have the task_id directly.
                            // We drop _overall_completion_future as it's not awaited in client_manager.
                            info!("StartGetStream: Successfully initiated. TaskId: {} for key {}", task_id, name);

                            match client_data_stream_rx_option {
                                Some(client_data_stream_rx) => {
                                    if !sender.is_canceled() {
                                        // Forward progress_rx and data_stream_rx, mapping errors to String.
                                        let (prog_tx_fwd, prog_rx_fwd) = mpsc::unbounded_channel();
                                        let mut original_progress_rx = client_progress_rx; 
                                        spawn_local(async move {
                                            while let Some(res) = original_progress_rx.recv().await {
                                                if prog_tx_fwd.send(res.map_err(|e| e.to_string())).is_err() {
                                                    break; 
                                                }
                                            }
                                        });

                                        let (data_tx_fwd, data_rx_fwd) = mpsc::unbounded_channel();
                                        let mut original_data_stream_rx = client_data_stream_rx; 
                                        spawn_local(async move {
                                            while let Some(res) = original_data_stream_rx.recv().await {
                                                if data_tx_fwd.send(res.map_err(|e| e.to_string())).is_err() {
                                                    break; 
                                                }
                                            }
                                        });

                                        if sender.send(Ok((task_id, prog_rx_fwd, data_rx_fwd))).is_err() {
                                            error!("StartGetStream: response channel closed by caller before sending success for key {}.", name);
                                        }
                                    } else {
                                        info!("StartGetStream: sender cancelled for key {} before sending success", name);
                                    }
                                }
                                None => {
                                    error!("StartGetStream: Data stream receiver missing when stream_data=true for key {}", name);
                                    if !sender.is_canceled() {
                                        if sender.send(Err("Data stream receiver not available when stream_data=true".to_string())).is_err() {
                                            error!("StartGetStream: response channel closed by caller before sending error (None data_stream_rx) for key {}.", name);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => { // Error from client.get() itself
                            error!("StartGetStream: client.get() failed for key {}: {:?}", name, e);
                            if !sender.is_canceled() {
                                if sender.send(Err(format!("client.get() failed: {:?}", e.to_string()))).is_err() {
                                     error!("StartGetStream: response channel closed by caller before sending error (client.get failed) for key {}.", name);
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}

// Public API to send commands to the client manager
pub async fn list_keys() -> Result<Vec<KeyDetails>, String> {
    info!("Starting list_keys request");

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::ListKeys(tx)) {
        Ok(_) => {
            info!("ListKeys command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received list_keys response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive list_keys response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send list_keys command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn start_get_stream(name: &str, is_public: bool) -> Result<(TaskId, mpsc::UnboundedReceiver<Result<TaskProgress, String>>, mpsc::UnboundedReceiver<Result<Vec<u8>, String>>), String> {
    info!("Starting StartGetStream request for key {}", name);
    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::StartGetStream(name.to_string(), is_public, tx)) {
        Ok(_) => {
            info!("StartGetStream command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received StartGetStream response from client manager");
                    result
                }
                Err(e) => {
                    error!("Failed to receive StartGetStream response: {:?}", e);
                    Err(format!("Failed to receive StartGetStream response: {:?}", e))
                }
            }
        }
        Err(e) => {
            error!("Failed to send StartGetStream command: {:?}", e);
            Err(format!("Failed to send StartGetStream command: {:?}", e))
        }
    }
}

pub async fn list_tasks() -> Result<Vec<TaskListEntry>, String> {
    info!("Starting list_tasks request");

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::ListTasks(tx)) {
        Ok(_) => {
            info!("ListTasks command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received list_tasks response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive list_tasks response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send list_tasks command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn get_stats() -> Result<StatsResponse, String> {
    info!("Starting get_stats request");

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::GetStats(tx)) {
        Ok(_) => {
            info!("GetStats command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received get_stats response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive get_stats response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send get_stats command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn get_task(task_id: TaskId) -> Result<Task, String> {
    info!("Starting get_task request for task {}", task_id);

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::GetTask(task_id, tx)) {
        Ok(_) => {
            info!("GetTask command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received get_task response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive get_task response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send get_task command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn stop_task(task_id: TaskId) -> Result<(), String> {
    info!("Starting stop_task request for task {}", task_id);

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::StopTask(task_id, tx)) {
        Ok(_) => {
            info!("StopTask command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received stop_task response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive stop_task response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send stop_task command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn get_key(name: &str, destination: &str) -> Result<(), String> {
    info!("Starting get_key request for key {}", name);

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::GetKey(name.to_string(), destination.to_string(), tx)) {
        Ok(_) => {
            info!("GetKey command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received get_key response from client manager");

                    result
                },
                Err(e) => {
                    error!("Failed to receive get_key response: {:?}", e);

                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send get_key command: {:?}", e);

            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn put(
    key: &str,
    data: Vec<u8>,
    filename: &str,
    mode: mutant_protocol::StorageMode,
    public: bool,
    no_verify: bool,
) -> Result<(TaskId, mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>), String> {
    info!("Starting put request for key {}", key);

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::Put(
        key.to_string(),
        data,
        filename.to_string(),
        mode,
        public,
        no_verify,
        tx,
    )) {
        Ok(_) => {
            info!("Put command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received put response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive put response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send put command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn rm(user_key: &str) -> Result<(), String> {
    info!("Starting rm request for key {}", user_key);

    // Create a oneshot channel for the response
    let (tx, rx) = oneshot::channel();

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::Rm(user_key.to_string(), tx)) {
        Ok(_) => {
            info!("Rm command sent to client manager");

            // Wait for the response
            match rx.await {
                Ok(result) => {
                    info!("Received rm response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive rm response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send rm command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn reconnect() -> Result<(), String> {
    info!("Starting reconnect request");

    // Send the command to the client manager
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::Reconnect) {
        Ok(_) => {
            info!("Reconnect command sent to client manager");
            Ok(())
        },
        Err(e) => {
            error!("Failed to send reconnect command: {:?}", e);
            Err(format!("Failed to send reconnect command: {:?}", e))
        }
    }
}

// Colony integration public API functions

pub async fn search(query: serde_json::Value) -> Result<SearchResponse, String> {
    info!("Starting search request");

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::Search(query, tx)) {
        Ok(_) => {
            info!("Search command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received search response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive search response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send search command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn add_contact(pod_address: &str, contact_name: Option<String>) -> Result<AddContactResponse, String> {
    info!("Starting add contact request");

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::AddContact(pod_address.to_string(), contact_name, tx)) {
        Ok(_) => {
            info!("AddContact command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received add contact response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive add contact response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send add contact command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn list_content() -> Result<ListContentResponse, String> {
    info!("Starting list content request");

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::ListContent(tx)) {
        Ok(_) => {
            info!("ListContent command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received list content response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive list content response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send list content command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn sync_contacts() -> Result<SyncContactsResponse, String> {
    info!("Starting sync contacts request");

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::SyncContacts(tx)) {
        Ok(_) => {
            info!("SyncContacts command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received sync contacts response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive sync contacts response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send sync contacts command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn get_user_contact() -> Result<GetUserContactResponse, String> {
    info!("Starting get user contact request");

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::GetUserContact(tx)) {
        Ok(_) => {
            info!("GetUserContact command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received get user contact response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive get user contact response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send get user contact command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn list_contacts() -> Result<ListContactsResponse, String> {
    info!("Starting list contacts request");

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::ListContacts(tx)) {
        Ok(_) => {
            info!("ListContacts command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received list contacts response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive list contacts response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send list contacts command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

// Filesystem navigation public API functions

pub async fn list_directory(path: &str) -> Result<mutant_protocol::ListDirectoryResponse, String> {
    info!("Starting list directory request for path: {}", path);

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::ListDirectory(path.to_string(), tx)) {
        Ok(_) => {
            info!("ListDirectory command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received list directory response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive list directory response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send list directory command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn get_file_info(path: &str) -> Result<mutant_protocol::GetFileInfoResponse, String> {
    info!("Starting get file info request for path: {}", path);

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::GetFileInfo(path.to_string(), tx)) {
        Ok(_) => {
            info!("GetFileInfo command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received get file info response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive get file info response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send get file info command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn put_file_path(
    key: &str,
    file_path: &str,
    mode: mutant_protocol::StorageMode,
    public: bool,
    no_verify: bool,
) -> Result<(String, mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>), String> {
    info!("Starting put file path request for key {} from path {}", key, file_path);

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::PutFilePath(
        key.to_string(),
        file_path.to_string(),
        mode,
        public,
        no_verify,
        tx,
    )) {
        Ok(_) => {
            info!("PutFilePath command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received put file path response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive put file path response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send put file path command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn download_to_path(
    key: &str,
    destination_path: &str,
    is_public: bool,
) -> Result<(), String> {
    info!("Starting download to path request for key {} to path {}", key, destination_path);

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::DownloadToPath(
        key.to_string(),
        destination_path.to_string(),
        is_public,
        tx,
    )) {
        Ok(_) => {
            info!("DownloadToPath command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received download to path response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive download to path response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send download to path command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}

pub async fn download_to_path_with_progress(
    key: &str,
    destination_path: &str,
    is_public: bool,
) -> Result<mpsc::UnboundedReceiver<Result<mutant_protocol::TaskProgress, String>>, String> {
    info!("Starting download to path with progress request for key {} to path {}", key, destination_path);

    let (tx, rx) = oneshot::channel();
    match CLIENT_COMMAND_SENDER.unbounded_send(ClientCommand::DownloadToPathWithProgress(
        key.to_string(),
        destination_path.to_string(),
        is_public,
        tx,
    )) {
        Ok(_) => {
            info!("DownloadToPathWithProgress command sent to client manager");
            match rx.await {
                Ok(result) => {
                    info!("Received download to path with progress response from client manager");
                    result
                },
                Err(e) => {
                    error!("Failed to receive download to path with progress response: {:?}", e);
                    Err(format!("Failed to receive response: {:?}", e))
                }
            }
        },
        Err(e) => {
            error!("Failed to send download to path with progress command: {:?}", e);
            Err(format!("Failed to send command: {:?}", e))
        }
    }
}
