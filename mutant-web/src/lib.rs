

use std::{collections::HashMap, sync::{Arc, RwLock}};

use app::{context::init_context, fs_window::FsWindow, Window, DEFAULT_WS_URL};
use futures::{channel::oneshot, StreamExt};
use log::{error, info};
use mutant_client::{MutantClient, ProgressReceiver};
use mutant_protocol::{TaskProgress, TaskResult};
use wasm_bindgen::prelude::*;

// mod app;
// mod cam_test;
// mod init;
// mod map;
pub mod utils;

mod app;

#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen(start)]
pub fn start() {
    init_panic_hook();
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));

    log::info!("Starting MutAnt Web Client");

    wasm_bindgen_futures::spawn_local(async move {
        async_start().await;
    });
}

pub async fn async_start() {
    init_context().await;
    run();

}

pub enum ClientRequest {
    Get(String, String, bool), // We'll keep this signature but handle None internally
    Put(String, Vec<u8>, String, mutant_protocol::StorageMode, bool, bool),
    Mv(String, String), // old_key, new_key
    ListKeys,
    ListTasks,
    GetStats,
    GetWalletBalance,
    PutStreamingInit {
        key_name: String,
        total_size: u64,
        filename: String,
        storage_mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
        response_name: String,
    },
    PutStreamingChunk {
        task_id: mutant_protocol::TaskId,
        chunk_index: usize,
        total_chunks: usize,
        data: Vec<u8>,
        is_last: bool,
        response_name: String,
    },
}

#[derive(Debug)]
pub enum ClientResponse {
    Get(Result<(TaskResult, Option<Vec<u8>>), String>),
    Put(Result<TaskResult, String>),
    Mv(Result<(), String>),
    ListKeys(Result<Vec<mutant_protocol::KeyDetails>, String>),
    ListTasks(Result<Vec<mutant_protocol::TaskListEntry>, String>),
    GetStats(Result<mutant_protocol::StatsResponse, String>),
    GetWalletBalance(Result<mutant_protocol::WalletBalanceResponse, String>),
    PutStreamingInit(Result<(mutant_protocol::TaskId, mutant_client::ProgressReceiver), String>),
    PutStreamingChunk(Result<(), String>),
}

pub struct Client {
    client: MutantClient,
    request_rx: futures::channel::mpsc::UnboundedReceiver<ClientRequest>,
}

impl Client {
    pub async fn spawn() -> ClientSender {
        info!("Spawning client");
        let (tx, rx) = futures::channel::mpsc::unbounded();

        let sender = ClientSender::new(tx);

        let responses = sender.responses.clone();

        spawn_local(async move {
            let mut this = Self {
                client: MutantClient::new(),
                request_rx: rx,
            };

            info!("Connecting to daemon");
            this.connect(&DEFAULT_WS_URL()).await.unwrap();
            info!("Client connected");

            // Note: Progress updates for streaming put operations are now handled
            // by the existing mutant-client progress system since we create proper
            // task channels in put_streaming_init

            while let Some(request) = this.request_rx.next().await {
                match request {
                    ClientRequest::Get(name, destination, public) => {
                        // Determine if we're streaming data (empty destination means we want the data directly)
                        let stream_data = destination.is_empty();
                        let dest_option = if stream_data { None } else { Some(&destination) };

                        // Generate the response name based on whether we're streaming
                        let response_name = if stream_data {
                            format!("get_{}_stream", name)
                        } else {
                            format!("get_{}_{}", name, destination)
                        };

                        info!("Client worker: Processing Get request for key={}, destination={}, public={}, stream_data={}",
                              name, destination, public, stream_data);

                        // Execute the get operation
                        info!("Client worker: Calling get method");
                        let result = this.get(&name, dest_option.map(|s| s.as_str()), public).await;

                        match &result {
                            Ok((task_result, data_opt)) => {
                                let data_size = data_opt.as_ref().map_or(0, |d| d.len());
                                info!("Client worker: Get operation completed successfully. Task result: {:?}, Data size: {}",
                                     task_result, data_size);
                            },
                            Err(e) => {
                                error!("Client worker: Get operation failed: {}", e);
                            }
                        }

                        // Send the response
                        info!("Client worker: Sending response for {}", response_name);
                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            if let Err(e) = tx.send(ClientResponse::Get(result)) {
                                error!("Client worker: Failed to send response: {:?}", e);
                            } else {
                                info!("Client worker: Response sent successfully");
                            }
                        } else {
                            error!("Client worker: No response channel found for {}", response_name);
                        }
                    }
                    ClientRequest::Put(key, data, filename, mode, public, no_verify) => {
                        let result = this.put(&key, data, &filename, mode, public, no_verify).await;
                        let response_name = format!("put_{}_{}", key, filename);
                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::Put(result));
                        }
                    }
                    ClientRequest::Mv(old_key, new_key) => {
                        let result = this.mv(&old_key, &new_key).await;
                        let response_name = format!("mv_{}_{}", old_key, new_key);
                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::Mv(result));
                        }
                    }
                    ClientRequest::ListKeys => {
                        let result = this.list_keys().await;
                        let response_name = "list_keys".to_string();
                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::ListKeys(result));
                        }
                    }
                    ClientRequest::ListTasks => {
                        let result = this.list_tasks().await;
                        let response_name = "list_tasks".to_string();
                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::ListTasks(result));
                        }
                    }
                    ClientRequest::GetStats => {
                        let result = this.get_stats().await;
                        let response_name = "get_stats".to_string();
                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::GetStats(result));
                        }
                    }
                    ClientRequest::GetWalletBalance => {
                        let result = this.get_wallet_balance().await;
                        let response_name = "get_wallet_balance".to_string();
                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::GetWalletBalance(result));
                        }
                    }
                    ClientRequest::PutStreamingInit { key_name, total_size, filename, storage_mode, public, no_verify, response_name } => {
                        // Use the proper streaming put method from mutant-client
                        let result = this.client.put_streaming_init(
                            &key_name,
                            total_size,
                            Some(filename),
                            storage_mode,
                            public,
                            no_verify
                        ).await.map_err(|e| format!("{:?}", e));

                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::PutStreamingInit(result));
                        }
                    }
                    ClientRequest::PutStreamingChunk { task_id, chunk_index, total_chunks, data, is_last, response_name } => {
                        // Use the proper streaming chunk method from mutant-client
                        let result = this.client.put_streaming_chunk(
                            task_id,
                            chunk_index,
                            total_chunks,
                            data,
                            is_last
                        ).await.map_err(|e| format!("{:?}", e));

                        if let Some(tx) = responses.write().unwrap().remove(&response_name) {
                            let _ = tx.send(ClientResponse::PutStreamingChunk(result));
                        }
                    }
                }
            }
        });

        sender
    }

    async fn connect(&mut self, url: &str) -> Result<(), String> {
        self.client
            .connect(url)
            .await
            .map_err(|e| format!("{:?}", e))
    }



    pub async fn get(&mut self, name: &str, destination: Option<&str>, public: bool) -> Result<(TaskResult, Option<Vec<u8>>), String> {
        // Determine if we're streaming data (no destination means we want the data directly)
        let stream_data = destination.is_none();
        info!("Client.get: name={}, destination={:?}, public={}, stream_data={}",
              name, destination, public, stream_data);

        match self.client.get(name, destination, public, stream_data).await {
            Ok((_task_id, task_future, progress_rx, data_stream_rx)) => {
                // Handle progress updates
                info!("Client.get: Got task_future, progress_rx, and data_stream_rx={:?}",
                      data_stream_rx.is_some());

                // Get the get_id for this operation
                let get_id = if stream_data {
                    format!("get_{}", name)
                } else {
                    format!("get_{}_{}", name, destination.as_deref().unwrap_or(""))
                };

                // Pass the get_id to the progress handler
                handle_get_progress(progress_rx, get_id);

                // CRITICAL FIX: First await the task_future to actually send the request
                // This is the key change - we need to start the task before collecting data
                info!("Client.get: FIRST awaiting task_future to start the request");
                let task_id = match task_future.await {
                    Ok(result) => {
                        info!("Client.get: Task completed with result: {:?}", result);
                        result
                    },
                    Err(e) => {
                        error!("Client.get: Task future error: {:?}", e);
                        return Err(format!("{:?}", e));
                    }
                };
                info!("Client.get: Request has been sent and task completed with ID: {:?}", task_id);

                // If we're streaming data, collect it
                let collected_data = if let Some(mut data_rx) = data_stream_rx {
                    info!("Client.get: Setting up data streaming collection");
                    // Create a vector to collect all data chunks
                    let mut all_data = Vec::new();

                    // Spawn a task to collect the data
                    let (tx, rx) = oneshot::channel();

                    spawn_local(async move {
                        info!("Client.get: Started data collection task");
                        let mut chunks_received = 0;
                        let mut total_bytes = 0;

                        while let Some(data_result) = data_rx.recv().await {
                            chunks_received += 1;
                            match data_result {
                                Ok(chunk) => {
                                    total_bytes += chunk.len();
                                    info!("Received data chunk #{}: {} bytes (total so far: {} bytes)",
                                          chunks_received, chunk.len(), total_bytes);
                                    all_data.extend(chunk);
                                }
                                Err(e) => {
                                    error!("Error receiving data chunk #{}: {:?}", chunks_received, e);
                                    break;
                                }
                            }
                        }

                        info!("Client.get: Finished collecting data: {} chunks, {} total bytes",
                              chunks_received, total_bytes);

                        // Send the collected data
                        if let Err(_) = tx.send(all_data) {
                            error!("Failed to send collected data through channel");
                        }
                    });

                    // Wait for all data to be collected
                    info!("Client.get: Waiting for data collection to complete");
                    match rx.await {
                        Ok(data) => {
                            info!("Client.get: Successfully collected all data: {} bytes", data.len());
                            Some(data)
                        },
                        Err(_) => {
                            error!("Failed to collect data chunks");
                            None
                        }
                    }
                } else {
                    info!("Client.get: No data streaming requested");
                    None
                };

                Ok((task_id, collected_data))
            }
            Err(e) => {
                error!("Failed to start get task: {:?}", e);
                Err(format!("{:?}", e))
            }
        }
    }

    pub async fn put(
        &mut self,
        key: &str,
        data: Vec<u8>,
        filename: &str,
        mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
    ) -> Result<TaskResult, String> {
        // Get the response name for this put operation
        let response_name = format!("put_{}_{}", key, filename);

        // Check if we have a progress object for this operation
        let progress_obj = {
            if let Some(sender) = Client::get_client_sender() {
                let put_progress = sender.put_progress.read().unwrap();
                put_progress.get(&response_name).cloned()
            } else {
                None
            }
        };

        match self.client.put_bytes(key, data, Some(filename.to_string()), mode, public, no_verify).await {
            Ok((task_future, progress_rx, _)) => {
                // If we have a progress object, update it based on the progress events
                if let Some(progress) = progress_obj {
                    // Spawn a task to update the progress object
                    spawn_local({
                        let mut progress_rx = progress_rx;
                        async move {
                            while let Some(progress_update) = progress_rx.recv().await {
                                match progress_update {
                                    Ok(TaskProgress::Put(event)) => {
                                        // Update the progress based on the event
                                        let mut progress_guard = progress.write().unwrap();
                                        match event {
                                            mutant_protocol::PutEvent::Starting {
                                                total_chunks,
                                                initial_written_count,
                                                initial_confirmed_count,
                                                chunks_to_reserve
                                            } => {
                                                log::info!("Progress update - Starting: total_chunks={}, initial_written={}, initial_confirmed={}, to_reserve={}",
                                                    total_chunks, initial_written_count, initial_confirmed_count, chunks_to_reserve);

                                                // Initialize the operation
                                                progress_guard.operation.insert("put".to_string(), app::context::ProgressOperation {
                                                    nb_to_reserve: chunks_to_reserve,
                                                    nb_reserved: total_chunks - chunks_to_reserve,
                                                    total_pads: total_chunks,
                                                    nb_written: initial_written_count,
                                                    nb_confirmed: initial_confirmed_count,
                                                });

                                                // Log the initial state
                                                if let Some(op) = progress_guard.operation.get("put") {
                                                    log::info!("Progress initialized: total_pads={}, reserved={}, written={}, confirmed={}",
                                                        op.total_pads, op.nb_reserved, op.nb_written, op.nb_confirmed);
                                                }
                                            },
                                            mutant_protocol::PutEvent::PadReserved => {
                                                // Update reserved count
                                                if let Some(op) = progress_guard.operation.get_mut("put") {
                                                    op.nb_reserved += 1;
                                                    log::info!("Progress update - PadReserved: reserved={}/{}", op.nb_reserved, op.total_pads);
                                                }
                                            },
                                            mutant_protocol::PutEvent::PadsWritten => {
                                                // Update written count
                                                if let Some(op) = progress_guard.operation.get_mut("put") {
                                                    op.nb_written += 1;
                                                    log::info!("Progress update - PadsWritten: written={}/{}", op.nb_written, op.total_pads);
                                                }
                                            },
                                            mutant_protocol::PutEvent::PadsConfirmed => {
                                                // Update confirmed count
                                                if let Some(op) = progress_guard.operation.get_mut("put") {
                                                    op.nb_confirmed += 1;
                                                    log::info!("Progress update - PadsConfirmed: confirmed={}/{}", op.nb_confirmed, op.total_pads);
                                                }
                                            },

                                            mutant_protocol::PutEvent::Complete => {
                                                // Mark operation as complete
                                                if let Some(op) = progress_guard.operation.get_mut("put") {
                                                    op.nb_reserved = op.total_pads;
                                                    op.nb_written = op.total_pads;
                                                    op.nb_confirmed = op.total_pads;
                                                    log::info!("Progress update - Complete: All {} pads reserved, written, and confirmed", op.total_pads);
                                                }
                                            }
                                        }
                                    },
                                    _ => {
                                        // Ignore other progress types
                                    }
                                }
                            }
                        }
                    });
                } else {
                    // If we don't have a progress object, just log the progress
                    handle_put_progress(progress_rx);
                }

                // Wait for the task to complete and return the result
                task_future.await.map_err(|e| format!("{:?}", e))
            }
            Err(e) => {
                error!("Failed to start put task: {:?}", e);
                Err(format!("{:?}", e))
            }
        }
    }

    // Helper method to get the ClientSender instance
    fn get_client_sender() -> Option<Arc<ClientSender>> {
        // Use the context function directly
        let ctx = app::context::context();
        Some(ctx.get_client_sender())
    }

    pub async fn list_keys(&mut self) -> Result<Vec<mutant_protocol::KeyDetails>, String> {
        self.client.list_keys().await.map_err(|e| format!("{:?}", e))
    }

    pub async fn list_tasks(&mut self) -> Result<Vec<mutant_protocol::TaskListEntry>, String> {
        self.client.list_tasks().await.map_err(|e| format!("{:?}", e))
    }

    pub async fn mv(&mut self, old_key: &str, new_key: &str) -> Result<(), String> {
        self.client.mv(old_key, new_key).await.map_err(|e| format!("{:?}", e))
    }

    pub async fn get_stats(&mut self) -> Result<mutant_protocol::StatsResponse, String> {
        self.client.get_stats().await.map_err(|e| format!("{:?}", e))
    }

    pub async fn get_wallet_balance(&mut self) -> Result<mutant_protocol::WalletBalanceResponse, String> {
        self.client.get_wallet_balance().await.map_err(|e| format!("{:?}", e))
    }

    // This method is no longer needed - we use the proper mutant-client streaming methods

    // Helper method to send a simple request (like PutData) that doesn't expect a response
    pub async fn send_request_simple(&mut self, request: mutant_protocol::Request) -> Result<(), String> {
        match self.client.send_request(request).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to send request: {:?}", e);
                Err(format!("Failed to send request: {:?}", e))
            }
        }
    }
}

pub struct ClientSender {
    tx: futures::channel::mpsc::UnboundedSender<ClientRequest>,
    responses: Arc<RwLock<HashMap<String, oneshot::Sender<ClientResponse>>>>,
    put_progress: Arc<RwLock<HashMap<String, Arc<RwLock<app::context::Progress>>>>>,
}

impl Clone for ClientSender {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            responses: self.responses.clone(),
            put_progress: self.put_progress.clone(),
        }
    }
}

impl ClientSender {
    pub fn new(tx: futures::channel::mpsc::UnboundedSender<ClientRequest>) -> Self {
        Self {
            tx,
            responses: Arc::new(RwLock::new(HashMap::new())),
            put_progress: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, name: String, destination: Option<String>, public: bool) -> Result<(TaskResult, Option<Vec<u8>>), String> {
        // Generate a unique response name
        let response_name = match &destination {
            Some(dest) => format!("get_{}_{}", name, dest),
            None => format!("get_{}_stream", name),
        };
        info!("ClientSender.get: name={}, destination={:?}, public={}, response_name={}",
              name, destination, public, response_name);

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("Get request already pending for {}", response_name);
            return Err("Get request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();
        self.responses.write().unwrap().insert(response_name.clone(), tx);
        info!("ClientSender.get: Created response channel for {}", response_name);

        // Send the request with the destination (which may be None)
        info!("ClientSender.get: Sending request to client worker");
        let dest = destination.unwrap_or_default();
        if let Err(e) = self.tx.unbounded_send(ClientRequest::Get(name.clone(), dest, public)) {
            error!("ClientSender.get: Failed to send request: {:?}", e);
            return Err(format!("Failed to send request: {:?}", e));
        }
        info!("ClientSender.get: Request sent, waiting for response");

        rx.await.map(|result| {
            info!("ClientSender.get: Received response for {}", response_name);
            match result {
                ClientResponse::Get(Ok(result)) => {
                    let data_size = result.1.as_ref().map_or(0, |d| d.len());
                    info!("ClientSender.get: Success response with data size: {}", data_size);
                    Ok(result)
                },
                ClientResponse::Get(Err(e)) => {
                    error!("ClientSender.get: Error response: {}", e);
                    Err(e)
                },
                _ => {
                    error!("ClientSender.get: Unexpected response type");
                    Err("Unexpected response".to_string())
                },
            }
        }).map_err(|e| {
            error!("ClientSender.get: Channel error: {:?}", e);
            format!("{:?}", e)
        })?
    }

    pub async fn put(
        &self,
        key: String,
        data: Vec<u8>,
        filename: String,
        mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
        progress: Option<Arc<RwLock<app::context::Progress>>>,
    ) -> Result<TaskResult, String> {
        let response_name = format!("put_{}_{}", key, filename);

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("Put request already pending for {}", response_name);
            return Err("Put request already pending".to_string());
        }

        // Store the progress object if provided
        if let Some(progress_obj) = progress {
            let mut put_progress = self.put_progress.write().unwrap();
            put_progress.insert(response_name.clone(), progress_obj);
        }

        let (tx, rx) = oneshot::channel();
        self.responses.write().unwrap().insert(response_name.clone(), tx);

        let _ = self.tx.unbounded_send(ClientRequest::Put(key, data, filename, mode, public, no_verify));

        rx.await.map(|result| {
            match result {
                ClientResponse::Put(Ok(result)) => Ok(result),
                ClientResponse::Put(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        }).map_err(|e| format!("{:?}", e))?
    }

    pub async fn list_keys(&self) -> Result<Vec<mutant_protocol::KeyDetails>, String> {
        let response_name = "list_keys".to_string();

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("ListKeys request already pending");
            return Err("ListKeys request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();

        self.responses.write().unwrap().insert(response_name, tx);

        let _ = self.tx.unbounded_send(ClientRequest::ListKeys);

        rx.await.map(|result| {
            match result {
                ClientResponse::ListKeys(Ok(result)) => Ok(result),
                ClientResponse::ListKeys(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        }).map_err(|e| format!("{:?}", e))?
    }

    pub async fn list_tasks(&self) -> Result<Vec<mutant_protocol::TaskListEntry>, String> {
        let response_name = "list_tasks".to_string();

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("ListTasks request already pending");
            return Err("ListTasks request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();

        self.responses.write().unwrap().insert(response_name, tx);

        let _ = self.tx.unbounded_send(ClientRequest::ListTasks);

        rx.await.map(|result| {
            match result {
                ClientResponse::ListTasks(Ok(result)) => Ok(result),
                ClientResponse::ListTasks(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        }).map_err(|e| format!("{:?}", e))?
    }

    pub async fn mv(&self, old_key: String, new_key: String) -> Result<(), String> {
        let response_name = format!("mv_{}_{}", old_key, new_key);

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("Mv request already pending for {}", response_name);
            return Err("Mv request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();
        self.responses.write().unwrap().insert(response_name.clone(), tx);

        let _ = self.tx.unbounded_send(ClientRequest::Mv(old_key, new_key));

        rx.await.map(|result| {
            match result {
                ClientResponse::Mv(Ok(result)) => Ok(result),
                ClientResponse::Mv(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        }).map_err(|e| format!("{:?}", e))?
    }

    pub async fn get_stats(&self) -> Result<mutant_protocol::StatsResponse, String> {
        let response_name = "get_stats".to_string();

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("GetStats request already pending");
            return Err("GetStats request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();
        self.responses.write().unwrap().insert(response_name, tx);

        let _ = self.tx.unbounded_send(ClientRequest::GetStats);

        rx.await.map(|result| {
            match result {
                ClientResponse::GetStats(Ok(result)) => Ok(result),
                ClientResponse::GetStats(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        }).map_err(|e| format!("{:?}", e))?
    }

    pub async fn get_wallet_balance(&self) -> Result<mutant_protocol::WalletBalanceResponse, String> {
        let response_name = "get_wallet_balance".to_string();

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("GetWalletBalance request already pending");
            return Err("GetWalletBalance request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();
        self.responses.write().unwrap().insert(response_name, tx);

        let _ = self.tx.unbounded_send(ClientRequest::GetWalletBalance);

        rx.await.map(|result| {
            match result {
                ClientResponse::GetWalletBalance(Ok(result)) => Ok(result),
                ClientResponse::GetWalletBalance(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        }).map_err(|e| format!("{:?}", e))?
    }

    // Streaming put methods - now using proper mutant-client methods
    pub async fn put_streaming_init(
        &self,
        key_name: &str,
        total_size: u64,
        filename: &str,
        storage_mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
    ) -> Result<(mutant_protocol::TaskId, mutant_client::ProgressReceiver), String> {
        let response_name = format!("put_streaming_init_{}", key_name);

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("PutStreamingInit request already pending for {}", key_name);
            return Err("PutStreamingInit request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();
        self.responses.write().unwrap().insert(response_name.clone(), tx);

        let request = ClientRequest::PutStreamingInit {
            key_name: key_name.to_string(),
            total_size,
            filename: filename.to_string(),
            storage_mode,
            public,
            no_verify,
            response_name: response_name.clone(),
        };

        self.tx.unbounded_send(request)
            .map_err(|e| format!("Failed to send put streaming init request: {}", e))?;

        rx.await.map_err(|e| format!("{:?}", e)).and_then(|response| {
            match response {
                ClientResponse::PutStreamingInit(Ok((task_id, progress_rx))) => Ok((task_id, progress_rx)),
                ClientResponse::PutStreamingInit(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        })
    }

    pub async fn put_streaming_chunk(
        &self,
        task_id: mutant_protocol::TaskId,
        chunk_index: usize,
        total_chunks: usize,
        data: Vec<u8>,
        is_last: bool,
    ) -> Result<(), String> {
        let response_name = format!("put_streaming_chunk_{}_{}", task_id, chunk_index);

        if self.responses.read().unwrap().contains_key(&response_name) {
            error!("PutStreamingChunk request already pending for {}:{}", task_id, chunk_index);
            return Err("PutStreamingChunk request already pending".to_string());
        }

        let (tx, rx) = oneshot::channel();
        self.responses.write().unwrap().insert(response_name.clone(), tx);

        let request = ClientRequest::PutStreamingChunk {
            task_id,
            chunk_index,
            total_chunks,
            data,
            is_last,
            response_name: response_name.clone(),
        };

        self.tx.unbounded_send(request)
            .map_err(|e| format!("Failed to send put streaming chunk request: {}", e))?;

        rx.await.map_err(|e| format!("{:?}", e)).and_then(|response| {
            match response {
                ClientResponse::PutStreamingChunk(Ok(())) => Ok(()),
                ClientResponse::PutStreamingChunk(Err(e)) => Err(e),
                _ => Err("Unexpected response".to_string()),
            }
        })
    }
}

fn handle_get_progress(mut progress_rx: ProgressReceiver, get_id: String) {
    spawn_local(async move {
        info!("Started get progress handler for {}", get_id);
        let mut progress_count = 0;
        let mut progress_obj: Option<Arc<RwLock<app::context::Progress>>> = None;
        let mut fetched_pads = 0;
        let mut total_pads = 0;

        // Try to find the progress object for this operation
        let ctx = app::context::context();
        if let Some(progress) = ctx.get_get_progress(&get_id) {
            progress_obj = Some(progress.clone());
            info!("Get progress: Found progress object for {}", get_id);
        }

        while let Some(progress) = progress_rx.recv().await {
            progress_count += 1;
            match progress {
                Ok(progress) => {
                    match &progress {
                        mutant_protocol::TaskProgress::Get(event) => {
                            match event {
                                mutant_protocol::GetEvent::Starting { total_chunks } => {
                                    info!("Get progress #{}: Starting with {} total chunks", progress_count, total_chunks);

                                    // Initialize progress tracking
                                    total_pads = *total_chunks;

                                    // Initialize the operation in the progress object if we have one
                                    if let Some(progress) = &progress_obj {
                                        let mut progress_guard = progress.write().unwrap();
                                        progress_guard.operation.insert("get".to_string(), app::context::ProgressOperation {
                                            nb_to_reserve: *total_chunks,
                                            nb_reserved: 0,
                                            total_pads: *total_chunks,
                                            nb_written: 0,
                                            nb_confirmed: 0,
                                        });
                                        info!("Get progress: Initialized progress object for {}", get_id);
                                    }
                                },
                                mutant_protocol::GetEvent::PadFetched => {
                                    fetched_pads += 1;
                                    info!("Get progress #{}: Pad fetched ({}/{})", progress_count, fetched_pads, total_pads);

                                    // Update the progress object if we have one
                                    if let Some(progress) = &progress_obj {
                                        let mut progress_guard = progress.write().unwrap();
                                        if let Some(op) = progress_guard.operation.get_mut("get") {
                                            op.nb_reserved = fetched_pads;
                                            op.nb_written = fetched_pads;
                                            info!("Get progress: Updated progress object: {}/{}", fetched_pads, total_pads);
                                        }
                                    }
                                },
                                mutant_protocol::GetEvent::PadData { chunk_index, data } => {
                                    info!("Get progress #{}: Received data for chunk {} ({} bytes)",
                                          progress_count, chunk_index, data.len());
                                },
                                mutant_protocol::GetEvent::Complete => {
                                    info!("Get progress #{}: Operation complete", progress_count);

                                    // Mark the operation as complete in the progress object
                                    if let Some(progress) = &progress_obj {
                                        let mut progress_guard = progress.write().unwrap();
                                        if let Some(op) = progress_guard.operation.get_mut("get") {
                                            op.nb_confirmed = op.total_pads;
                                            info!("Get progress: Marked operation as complete");
                                        }
                                    }
                                },
                            }
                        },
                        _ => {
                            info!("Get progress #{}: Unexpected progress type: {:?}", progress_count, progress);
                        }
                    }
                }
                Err(e) => {
                    error!("Get progress #{}: Error: {:?}", progress_count, e);
                    break;
                }
            }
        }

        info!("Get progress handler finished after {} progress updates", progress_count);
    });
}

fn handle_put_progress(mut progress_rx: ProgressReceiver) {
    // Simply log the progress for now
    spawn_local(async move {
        while let Some(progress) = progress_rx.recv().await {
            match progress {
                Ok(progress) => {
                    log::info!("Progress: {:?}", progress);
                }
                Err(e) => {
                    error!("Progress error: {:?}", e);
                    break;
                }
            }
        }
    });
}

use eframe::egui;
use wasm_bindgen_futures::spawn_local;

struct MyApp {
    fs_window: Arc<RwLock<FsWindow>>,
    sidebar_expanded: bool,
    balance_fetch_counter: u32,
}

impl Default for MyApp {
    fn default() -> Self {
        let fs_window = Arc::new(RwLock::new(FsWindow::new()));

        // Set the global reference for async tasks
        app::fs::global::set_main_fs_window(fs_window.clone());

        Self {
            fs_window,
            sidebar_expanded: false,
            balance_fetch_counter: 0,
        }
    }
}

impl MyApp {
    /// Draw the header bar at the top of the screen
    fn draw_header(&mut self, ctx: &egui::Context) {
        // Check if we need to fetch wallet balance (every ~1800 frames, roughly 30 seconds at 60fps)
        self.balance_fetch_counter += 1;
        let should_fetch_balance = self.balance_fetch_counter == 1 || self.balance_fetch_counter % 1800 == 0;

        if should_fetch_balance {
            let ctx_clone = app::context::context();

            spawn_local(async move {
                log::info!("Fetching wallet balance...");
                match ctx_clone.get_wallet_balance().await {
                    Some(balance) => {
                        log::info!("Wallet balance fetched successfully: tokens={}, gas={}", balance.token_balance, balance.gas_balance);
                    }
                    None => {
                        log::warn!("Failed to fetch wallet balance");
                    }
                }
            });
        }

        egui::TopBottomPanel::top("header")
            .exact_height(40.0)
            .frame(egui::Frame::new()
                .fill(app::theme::MutantColors::BACKGROUND_MEDIUM)
                .stroke(egui::Stroke::new(1.0, app::theme::MutantColors::BORDER_DARK)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(15.0);

                    // Left side - MutAnt title and connection status
                    ui.label(
                        egui::RichText::new("MutAnt")
                            .size(16.0)
                            .strong()
                            .color(app::theme::MutantColors::ACCENT_ORANGE)
                    );

                    ui.separator();

                    ui.label(
                        egui::RichText::new("Connected")
                            .size(14.0)
                            .color(app::theme::MutantColors::SUCCESS)
                    );

                    // Use allocate_ui_with_layout to properly position wallet balances on the right
                    let available_rect = ui.available_rect_before_wrap();
                    let (left_rect, right_rect) = available_rect.split_left_right_at_x(available_rect.right() - 200.0); // Reserve 200px for wallet balances

                    // Allocate the left space (this creates the expanding space)
                    ui.allocate_rect(left_rect, egui::Sense::hover());

                    // Now use the right space for wallet balances
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right_rect), |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(10.0); // Exactly 10px from the right border

                            // Get wallet balance data
                            let ctx = app::context::context();
                            let wallet_balance_cache = ctx.get_wallet_balance_cache();
                            let balance_option = wallet_balance_cache.read().unwrap().as_ref().cloned();

                            if let Some(balance) = balance_option {
                                // Format the balance values
                                let token_balance = self.format_balance(&balance.token_balance);
                                let gas_balance = self.format_balance(&balance.gas_balance);

                                // Stack wallet balances vertically on the right, centered
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("💰 {} ANT", token_balance))
                                            .size(10.0)
                                            .color(app::theme::MutantColors::ACCENT_BLUE)
                                    );

                                    ui.label(
                                        egui::RichText::new(format!("⛽ {} ETH", gas_balance))
                                            .size(10.0)
                                            .color(app::theme::MutantColors::ACCENT_GREEN)
                                    );
                                });
                            } else {
                                // Show loading indicator
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new("💰 Loading...")
                                            .size(10.0)
                                            .color(app::theme::MutantColors::TEXT_SECONDARY)
                                    );
                                    ui.label(
                                        egui::RichText::new("⛽ Loading...")
                                            .size(10.0)
                                            .color(app::theme::MutantColors::TEXT_SECONDARY)
                                    );
                                });
                            }
                        });
                    });
                });
            });
    }

    /// Format balance from wei to readable format
    fn format_balance(&self, balance_str: &str) -> String {
        // Parse the balance string as a big number (it's in wei)
        match balance_str.parse::<u128>() {
            Ok(balance_wei) => {
                // Convert from wei to ether (divide by 10^18)
                let balance_ether = balance_wei as f64 / 1_000_000_000_000_000_000.0;

                // Always show 18 decimal places for full precision
                format!("{:.18}", balance_ether)
            }
            Err(_) => {
                // If parsing fails, just show the raw string
                balance_str.to_string()
            }
        }
    }

    /// Draw the left menu bar
    fn draw_left_menu(&mut self, ctx: &egui::Context) {
        let sidebar_width = if self.sidebar_expanded { 200.0 } else { 60.0 };

        egui::SidePanel::left("left_menu")
            .exact_width(sidebar_width)
            .frame(egui::Frame::new()
                .fill(app::theme::MutantColors::BACKGROUND_MEDIUM)
                .stroke(egui::Stroke::new(1.0, app::theme::MutantColors::BORDER_DARK)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(15.0);

                    // Check window states before creating buttons
                    let upload_open = self.fs_window.read().unwrap().is_window_open("MutAnt Upload");
                    let stats_open = self.fs_window.read().unwrap().is_window_open("MutAnt Stats");
                    let colony_open = self.fs_window.read().unwrap().is_window_open("Colony");

                    // Helper function to create a button with different styling when active
                    let menu_button = |ui: &mut egui::Ui, icon: &str, label: &str, hover: &str, is_active: bool| {
                        if self.sidebar_expanded {
                            // Expanded mode: horizontal layout with icon and label
                            let button = if is_active {
                                egui::Button::new(
                                    egui::RichText::new(format!("{} {}", icon, label))
                                        .size(16.0)
                                        .strong()
                                        .color(app::theme::MutantColors::BACKGROUND_DARK)
                                )
                                .fill(app::theme::MutantColors::ACCENT_ORANGE)
                                .stroke(egui::Stroke::new(2.0, app::theme::MutantColors::ACCENT_ORANGE))
                                .min_size([180.0, 40.0].into())
                            } else {
                                egui::Button::new(
                                    egui::RichText::new(format!("{} {}", icon, label))
                                        .size(14.0)
                                        .color(app::theme::MutantColors::TEXT_SECONDARY)
                                )
                                .fill(app::theme::MutantColors::SURFACE)
                                .stroke(egui::Stroke::new(1.0, app::theme::MutantColors::BORDER_MEDIUM))
                                .min_size([180.0, 40.0].into())
                            };
                            ui.add(button)
                        } else {
                            // Collapsed mode: icon only (original behavior)
                            let button = if is_active {
                                egui::Button::new(
                                    egui::RichText::new(icon)
                                        .size(18.0)
                                        .strong()
                                        .color(app::theme::MutantColors::BACKGROUND_DARK)
                                )
                                .fill(app::theme::MutantColors::ACCENT_ORANGE)
                                .stroke(egui::Stroke::new(2.0, app::theme::MutantColors::ACCENT_ORANGE))
                                .min_size([40.0, 40.0].into())
                            } else {
                                egui::Button::new(
                                    egui::RichText::new(icon)
                                        .size(16.0)
                                        .color(app::theme::MutantColors::TEXT_SECONDARY)
                                )
                                .fill(app::theme::MutantColors::SURFACE)
                                .stroke(egui::Stroke::new(1.0, app::theme::MutantColors::BORDER_MEDIUM))
                                .min_size([40.0, 40.0].into())
                            };
                            ui.add(button).on_hover_text(hover)
                        }
                    };

                    if menu_button(ui, "📤", "Upload", "Upload", upload_open).clicked() {
                        self.fs_window.write().unwrap().add_put_tab();
                    }

                    if menu_button(ui, "📊", "Stats", "Stats", stats_open).clicked() {
                        self.fs_window.write().unwrap().add_stats_tab();
                    }

                    if menu_button(ui, "🌐", "Colony", "Colony", colony_open).clicked() {
                        self.fs_window.write().unwrap().add_colony_tab();
                    }

                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        ui.add_space(10.0);

                        // Version info at bottom - clickable to open GitHub
                        let version_text = format!("v{}", env!("CARGO_PKG_VERSION"));
                        let version_response = ui.add(
                            egui::Label::new(
                                egui::RichText::new(&version_text)
                                    .size(10.0)
                                    .color(app::theme::MutantColors::TEXT_MUTED)
                            )
                            .sense(egui::Sense::click())
                        );

                        if version_response.clicked() {
                            // Open GitHub repository
                            #[cfg(target_arch = "wasm32")]
                            {
                                let window = web_sys::window().unwrap();
                                let _ = window.open_with_url("https://github.com/Champii/MutAnt");
                            }
                        }

                        if version_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    });
                });
            });
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply the MutAnt theme
        app::theme::apply_mutant_theme(ctx);

        // Show the header
        self.draw_header(ctx);

        // Show the left menu
        self.draw_left_menu(ctx);

        // Draw the toggle button as a floating circular button that overlaps with the main content
        let sidebar_width = if self.sidebar_expanded { 200.0 } else { 60.0 };
        let toggle_icon = if self.sidebar_expanded { "◀" } else { "▶" };
        let button_size = 24.0;
        let button_pos = egui::pos2(sidebar_width - 12.0, 60.0); // Position it to overlap with the main content

        // Create a floating area for the toggle button
        egui::Area::new(egui::Id::new("sidebar_toggle"))
            .fixed_pos(button_pos)
            .show(ctx, |ui| {
                // Create a circular button with subtle styling
                let button = egui::Button::new(
                    egui::RichText::new(toggle_icon)
                        .size(12.0)
                        .color(app::theme::MutantColors::TEXT_SECONDARY)
                )
                .fill(app::theme::MutantColors::BACKGROUND_MEDIUM)
                .stroke(egui::Stroke::new(1.0, app::theme::MutantColors::BORDER_MEDIUM))
                .min_size([button_size, button_size].into())
                .corner_radius(egui::CornerRadius::same(12)); // Make it circular

                if ui.add(button).clicked() {
                    self.sidebar_expanded = !self.sidebar_expanded;
                }
            });

        // Show the main FS window in the remaining space
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(app::theme::MutantColors::BACKGROUND_DARK))
            .show(ctx, |ui| {
                // Render the static FS window directly
                // All windows now dock within the FS window's internal dock system
                self.fs_window.write().unwrap().draw(ui);
            });

        // Show notifications
        app::notifications::show_notifications(ctx);

        // Update video player z-index based on which tabs are currently visible
        app::fs::viewer_tab::update_video_player_z_index();
    }
}

// pub fn run() {
//     use wasm_bindgen::JsCast as _;

//     // Initialize the app
//     wasm_bindgen_futures::spawn_local(async {
//         let document = web_sys::window()
//             .expect("No window")
//             .document()
//             .expect("No document");

//         // Canvas is not used with run_native, but we still check if it exists
//         let _canvas = document
//             .get_element_by_id("canvas")
//             .expect("Failed to find the_canvas_id")
//             .dyn_into::<web_sys::HtmlCanvasElement>()
//             .expect("the_canvas_id was not a HtmlCanvasElement");

//         app::init().await;

//         // Create native options
//         let options = eframe::NativeOptions::default();

//         // Start the app
//         let start_result = eframe::run_native(
//             "MutAnt Web",
//             options,
//             Box::new(|_cc| Ok(Box::new(MyApp::default()))),
//         );

//         // Remove the loading text and spinner:
//         if let Some(loading_text) = document.get_element_by_id("loading_text") {
//             match start_result {
//                 Ok(_) => {
//                     loading_text.remove();
//                 }
//                 Err(e) => {
//                     loading_text.set_inner_html(
//                         "<p> The app has crashed. See the developer console for details. </p>",
//                     );
//                     panic!("Failed to start eframe: {e:?}");
//                 }
//             }
//         }
//     });
// }

pub fn run() {
    // Redirect `log` message to `console.log` and friends:
    // Use wasm_logger instead of eframe::WebLogger
    // wasm_logger is already initialized in the start function

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast as _;

        let web_options = eframe::WebOptions::default();

        wasm_bindgen_futures::spawn_local(async {
            let document = web_sys::window()
                .expect("No window")
                .document()
                .expect("No document");

            let canvas = document
                .get_element_by_id("canvas")
                .expect("Failed to find the_canvas_id")
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .expect("the_canvas_id was not a HtmlCanvasElement");

            // Set canvas to fill the viewport
            let window = web_sys::window().expect("No window");
            let inner_width = window.inner_width().unwrap().as_f64().unwrap() as u32;
            let inner_height = window.inner_height().unwrap().as_f64().unwrap() as u32;

            canvas.set_width(inner_width);
            canvas.set_height(inner_height);

            // Set CSS size to match
            let canvas_style = canvas.style();
            canvas_style.set_property("width", "100vw").unwrap();
            canvas_style.set_property("height", "100vh").unwrap();
            canvas_style.set_property("position", "fixed").unwrap();
            canvas_style.set_property("top", "0").unwrap();
            canvas_style.set_property("left", "0").unwrap();
            canvas_style.set_property("z-index", "1").unwrap();

            app::init().await;

            let start_result = eframe::WebRunner::new()
                .start(
                    canvas.clone(),
                    web_options,
                    Box::new(|_cc| Ok(Box::new(MyApp::default()))),
                )
                .await;

            // Add resize event listener to keep canvas full-screen
            let resize_closure = {
                let canvas = canvas.clone();
                let window = window.clone();
                wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                    let inner_width = window.inner_width().unwrap().as_f64().unwrap() as u32;
                    let inner_height = window.inner_height().unwrap().as_f64().unwrap() as u32;
                    canvas.set_width(inner_width);
                    canvas.set_height(inner_height);
                }) as Box<dyn Fn()>)
            };

            window
                .add_event_listener_with_callback("resize", resize_closure.as_ref().unchecked_ref())
                .unwrap();
            resize_closure.forget(); // Keep the closure alive

            // Remove the loading text and spinner:
            if let Some(loading_text) = document.get_element_by_id("loading_text") {
                match start_result {
                    Ok(_) => {
                        loading_text.remove();
                    }
                    Err(e) => {
                        loading_text.set_inner_html(
                            "<p> The app has crashed. See the developer console for details. </p>",
                        );
                        panic!("Failed to start eframe: {e:?}");
                    }
                }
            }
        });
    }
}