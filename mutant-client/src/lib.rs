use std::future::Future;
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

use futures::channel::oneshot;
use log::{debug, error, info, warn};
use tokio::sync::mpsc;
use url::Url;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

use mutant_protocol::{
    ExportResult, HealthCheckResult, ImportResult, KeyDetails, PurgeResult, PutSource, Request,
    StatsResponse, StorageMode, SyncResult, Task, TaskId, TaskListEntry, TaskProgress,
    TaskResult, TaskStatus, TaskStoppedResponse, TaskType,
};

pub mod error;
mod macros;
mod request;
mod response;

use crate::error::ClientError;

// Shared state for tasks managed by the client (using Arc<Mutex> for thread safety)
type ClientTaskMap = Arc<Mutex<HashMap<TaskId, Task>>>;

// Key for the pending requests map
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PendingRequestKey {
    TaskCreation,
    ListTasks,
    QueryTask,
    Rm,
    Mv,
    ListKeys,
    Stats,
    Sync,
    Purge,
    Import,
    Export,
    HealthCheck,
    StopTask,
}

// Enum to hold the different sender types for the pending requests map
pub enum PendingSender {
    TaskCreation(
        oneshot::Sender<Result<TaskId, ClientError>>,
        TaskChannels,
        TaskType,
    ),
    ListTasks(oneshot::Sender<Result<Vec<TaskListEntry>, ClientError>>),
    QueryTask(oneshot::Sender<Result<Task, ClientError>>),
    Rm(oneshot::Sender<Result<(), ClientError>>),
    Mv(oneshot::Sender<Result<(), ClientError>>),
    ListKeys(oneshot::Sender<Result<Vec<KeyDetails>, ClientError>>),
    Stats(oneshot::Sender<Result<StatsResponse, ClientError>>),
    Sync(oneshot::Sender<Result<SyncResult, ClientError>>),
    Purge(oneshot::Sender<Result<PurgeResult, ClientError>>),
    Import(oneshot::Sender<Result<ImportResult, ClientError>>),
    Export(oneshot::Sender<Result<ExportResult, ClientError>>),
    HealthCheck(oneshot::Sender<Result<HealthCheckResult, ClientError>>),
    StopTask(oneshot::Sender<Result<TaskStoppedResponse, ClientError>>),
}

// The new map type for pending requests
type PendingRequestMap = Arc<Mutex<HashMap<PendingRequestKey, PendingSender>>>;

pub type CompletionReceiver = oneshot::Receiver<Result<TaskResult, ClientError>>;
pub type ProgressReceiver = mpsc::UnboundedReceiver<Result<TaskProgress, ClientError>>;
pub type DataStreamReceiver = mpsc::UnboundedReceiver<Result<Vec<u8>, ClientError>>;

type CompletionSender = oneshot::Sender<Result<TaskResult, ClientError>>;
type ProgressSender = mpsc::UnboundedSender<Result<TaskProgress, ClientError>>;
type DataStreamSender = mpsc::UnboundedSender<Result<Vec<u8>, ClientError>>;

type TaskChannels = (CompletionSender, ProgressSender, Option<DataStreamSender>);
type TaskChannelsMap = Arc<Mutex<HashMap<TaskId, TaskChannels>>>;

#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// A client for interacting with the Mutant Daemon over WebSocket (Cross-platform implementation).
pub struct MutantClient {
    sender: Option<nash_ws::WebSocketSender>,
    receiver: Option<nash_ws::WebSocketReceiver>,
    tasks: ClientTaskMap,
    task_channels: TaskChannelsMap,
    pending_requests: PendingRequestMap,
    state: Arc<Mutex<ConnectionState>>,
}

impl MutantClient {
    /// Creates a new client instance but does not connect yet.
    pub fn new() -> Self {
        Self {
            sender: None,
            receiver: None,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            task_channels: Arc::new(Mutex::new(HashMap::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            state: Arc::new(Mutex::new(ConnectionState::Disconnected)),
        }
    }

    /// Establishes a WebSocket connection to the Mutant Daemon.
    pub async fn connect(&mut self, addr: &str) -> Result<(), ClientError> {
        info!("CLIENT: Attempting to connect to WebSocket at {}", addr);

        if self.sender.is_some() {
            warn!("CLIENT: Already connected or connecting.");
            return Ok(());
        }

        // Parse the URL
        let url = match Url::parse(addr) {
            Ok(url) => {
                info!("CLIENT: Successfully parsed URL: {}", url);
                url
            },
            Err(e) => {
                error!("CLIENT: Failed to parse URL '{}': {:?}", addr, e);
                return Err(ClientError::UrlParseError(e));
            }
        };

        info!("CLIENT: Setting connection state to Connecting");
        *self.state.lock().unwrap() = ConnectionState::Connecting;

        // Connect using nash-ws
        info!("CLIENT: Attempting to create WebSocket connection to {}", url);
        let connection_result = nash_ws::WebSocket::new(url.as_str()).await;

        match connection_result {
            Ok((sender, receiver)) => {
                info!("CLIENT: Successfully established WebSocket connection");
                self.sender = Some(sender);
                self.receiver = Some(receiver);

                info!("CLIENT: Setting connection state to Connected");
                *self.state.lock().unwrap() = ConnectionState::Connected;

                info!("CLIENT: Creating response handling task");
                let mut client_clone = self.partial_take_receiver();

                #[cfg(target_arch = "wasm32")]
                {
                    info!("CLIENT: Spawning WASM response handler");
                    spawn_local(async move {
                        info!("CLIENT: WASM response handler started");
                        while let Some(response) = client_clone.next_response().await {
                            match response {
                                Ok(resp) => {
                                    info!("CLIENT: Received response: {:?}", resp);
                                },
                                Err(e) => {
                                    error!("CLIENT: Error processing response: {:?}", e);
                                }
                            }
                        }
                        info!("CLIENT: WASM response handler exited");
                    });
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    info!("CLIENT: Spawning native response handler");
                    tokio::spawn(async move {
                        info!("CLIENT: Native response handler started");
                        while let Some(response) = client_clone.next_response().await {
                            match response {
                                Ok(resp) => {
                                    info!("CLIENT: Received response: {:?}", resp);
                                },
                                Err(e) => {
                                    error!("CLIENT: Error processing response: {:?}", e);
                                }
                            }
                        }
                        info!("CLIENT: Native response handler exited");
                    });
                }

                info!("CLIENT: Connection setup complete");
                Ok(())
            },
            Err(e) => {
                let error_msg = format!("{:?}", e);
                error!("CLIENT: Failed to establish WebSocket connection: {}", error_msg);
                *self.state.lock().unwrap() = ConnectionState::Disconnected;
                Err(ClientError::WebSocketError(error_msg))
            }
        }
    }

    // --- Public API Methods ---
    // A simple request/response map or channels might be needed.

    pub async fn put<'a>(
        &'a mut self,
        user_key: &str,
        source_path: &str,
        mode: StorageMode,
        public: bool,
        no_verify: bool,
    ) -> Result<
        (
            impl Future<Output = Result<TaskResult, ClientError>> + 'a,
            ProgressReceiver,
            Option<DataStreamReceiver>,
        ),
        ClientError,
    > {
        long_request!(
            self,
            Put,
            PutRequest {
                user_key: user_key.to_string(),
                source: PutSource::FilePath(source_path.to_string()),
                filename: Some(std::path::Path::new(source_path)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| source_path.to_string())),
                mode,
                public,
                no_verify,
            }
        )
    }

    /// Put operation with direct byte data instead of a file path
    pub async fn put_bytes<'a>(
        &'a mut self,
        user_key: &str,
        data: Vec<u8>,
        filename: Option<String>,
        mode: StorageMode,
        public: bool,
        no_verify: bool,
    ) -> Result<
        (
            impl Future<Output = Result<TaskResult, ClientError>> + 'a,
            ProgressReceiver,
            Option<DataStreamReceiver>,
        ),
        ClientError,
    > {
        long_request!(
            self,
            Put,
            PutRequest {
                user_key: user_key.to_string(),
                source: PutSource::Bytes(data),
                filename,
                mode,
                public,
                no_verify,
            }
        )
    }

    pub async fn get(
        &mut self,
        user_key: &str,
        destination_path: Option<&str>,
        public: bool,
        stream_data: bool,
    ) -> Result<
        (
            impl Future<Output = Result<TaskResult, ClientError>> + '_,
            ProgressReceiver,
            Option<DataStreamReceiver>,
        ),
        ClientError,
    > {
        info!("CLIENT: get() called with user_key={}, destination_path={:?}, public={}, stream_data={}",
              user_key, destination_path, public, stream_data);

        // Check connection state
        let connection_state = self.state.lock().unwrap().clone();
        info!("CLIENT: Current connection state: {:?}", connection_state);

        if connection_state != ConnectionState::Connected {
            error!("CLIENT: Cannot send get request - not connected (state: {:?})", connection_state);
            return Err(ClientError::NotConnected);
        }

        // Create the request
        let key = PendingRequestKey::TaskCreation;
        info!("CLIENT: Creating Get request");
        let req = Request::Get(mutant_protocol::GetRequest {
            user_key: user_key.to_string(),
            destination_path: destination_path.map(|s| s.to_string()),
            public,
            stream_data,
        });
        info!("CLIENT: Created Get request: {:?}", req);

        // Check if there's already a pending request
        let pending_key_exists = self.pending_requests.lock().unwrap().contains_key(&key);
        if pending_key_exists {
            error!("CLIENT: Another put/get request is already pending");
            return Err(ClientError::InternalError(
                "Another put/get request is already pending".to_string(),
            ));
        }

        info!("CLIENT: Creating channels for get request");
        let (completion_tx, completion_rx) = oneshot::channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Create data stream channel if streaming is enabled
        let (data_stream_tx, data_stream_rx) = if stream_data {
            info!("CLIENT: Creating data stream channel for streaming");
            let (tx, rx) = mpsc::unbounded_channel();
            (Some(tx), Some(rx))
        } else {
            info!("CLIENT: No data stream channel needed (not streaming)");
            (None, None)
        };

        info!("CLIENT: Creating task creation channel");
        let (task_creation_tx, task_creation_rx) = oneshot::channel();

        info!("CLIENT: Inserting pending request");
        self.pending_requests.lock().unwrap().insert(
            key.clone(),
            PendingSender::TaskCreation(
                task_creation_tx,
                (completion_tx, progress_tx, data_stream_tx),
                TaskType::Get,
            ),
        );
        info!("CLIENT: Pending request inserted");

        // IMPORTANT: This future is not executed until it's awaited!
        let start_task = async move {
            info!("CLIENT: Starting get task execution - THIS MEANS THE FUTURE IS BEING AWAITED");
            info!("CLIENT: About to send request: {:?}", req);

            // Store a copy of the connection state for logging
            let state_copy = self.state.lock().unwrap().clone();
            info!("CLIENT: Connection state before sending: {:?}", state_copy);

            // Check if sender is available
            let sender_available = self.sender.is_some();
            info!("CLIENT: Sender available: {}", sender_available);

            match self.send_request(req).await {
                Ok(_) => {
                    info!("CLIENT: Get request sent successfully, waiting for TaskCreated response");
                    let task_id_result = task_creation_rx.await;

                    match task_id_result {
                        Ok(task_id_inner_result) => {
                            match task_id_inner_result {
                                Ok(task_id) => {
                                    info!("CLIENT: Task created with ID: {}", task_id);

                                    info!("CLIENT: Waiting for task completion");
                                    match completion_rx.await {
                                        Ok(result) => {
                                            info!("CLIENT: Task completed with result: {:?}", result);
                                            result
                                        },
                                        Err(e) => {
                                            error!("CLIENT: Completion channel canceled: {:?}", e);
                                            Err(ClientError::InternalError("Completion channel canceled".to_string()))
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("CLIENT: Failed to create task: {:?}", e);
                                    Err(e)
                                }
                            }
                        },
                        Err(e) => {
                            error!("CLIENT: TaskCreated channel canceled: {:?}", e);
                            Err(ClientError::InternalError("TaskCreated channel canceled".to_string()))
                        }
                    }
                },
                Err(e) => {
                    error!("CLIENT: Failed to send Get request: {:?}", e);
                    self.pending_requests.lock().unwrap().remove(&key);
                    Err(e)
                }
            }
        };

        info!("CLIENT: Returning get task future, progress_rx, and data_stream_rx");
        Ok((start_task, progress_rx, data_stream_rx))
    }

    pub async fn sync(
        &mut self,
        push_force: bool,
    ) -> Result<
        (
            impl Future<Output = Result<TaskResult, ClientError>> + '_,
            ProgressReceiver,
            Option<DataStreamReceiver>,
        ),
        ClientError,
    > {
        long_request!(self, Sync, SyncRequest { push_force })
    }

    pub async fn purge(
        &mut self,
        aggressive: bool,
    ) -> Result<
        (
            impl Future<Output = Result<TaskResult, ClientError>> + '_,
            ProgressReceiver,
            Option<DataStreamReceiver>,
        ),
        ClientError,
    > {
        long_request!(self, Purge, PurgeRequest { aggressive })
    }

    pub async fn health_check(
        &mut self,
        key_name: &str,
        recycle: bool,
    ) -> Result<
        (
            impl Future<Output = Result<TaskResult, ClientError>> + '_,
            ProgressReceiver,
            Option<DataStreamReceiver>,
        ),
        ClientError,
    > {
        long_request!(
            self,
            HealthCheck,
            HealthCheckRequest {
                key_name: key_name.to_string(),
                recycle,
            }
        )
    }

    /// Retrieves a list of all stored keys from the daemon.
    pub async fn list_keys(&mut self) -> Result<Vec<KeyDetails>, ClientError> {
        direct_request!(self, ListKeys, ListKeysRequest)
    }

    pub async fn rm(&mut self, user_key: &str) -> Result<(), ClientError> {
        direct_request!(
            self,
            Rm,
            RmRequest {
                user_key: user_key.to_string(),
            }
        )
    }

    pub async fn mv(&mut self, old_key: &str, new_key: &str) -> Result<(), ClientError> {
        direct_request!(
            self,
            Mv,
            MvRequest {
                old_key: old_key.to_string(),
                new_key: new_key.to_string(),
            }
        )
    }

    pub async fn list_tasks(&mut self) -> Result<Vec<TaskListEntry>, ClientError> {
        direct_request!(self, ListTasks, ListTasksRequest)
    }

    pub async fn query_task(&mut self, task_id: TaskId) -> Result<Task, ClientError> {
        direct_request!(self, QueryTask, QueryTaskRequest { task_id })
    }

    pub async fn get_stats(&mut self) -> Result<StatsResponse, ClientError> {
        direct_request!(self, Stats, StatsRequest {})
    }

    pub async fn import(&mut self, file_path: &str) -> Result<ImportResult, ClientError> {
        direct_request!(
            self,
            Import,
            ImportRequest {
                file_path: file_path.to_string()
            }
        )
    }

    pub async fn export(&mut self, destination_path: &str) -> Result<ExportResult, ClientError> {
        direct_request!(
            self,
            Export,
            ExportRequest {
                destination_path: destination_path.to_string()
            }
        )
    }

    /// Stops a running task on the daemon.
    pub async fn stop_task(&mut self, task_id: TaskId) -> Result<TaskStoppedResponse, ClientError> {
        direct_request!(self, StopTask, StopTaskRequest { task_id })
    }



    // --- Accessor methods for internal state ---

    pub fn get_task_status(&self, task_id: TaskId) -> Option<TaskStatus> {
        self.tasks
            .lock()
            .unwrap()
            .get(&task_id)
            .map(|t| t.status.clone())
    }

    pub fn get_task_result(&self, task_id: TaskId) -> Option<TaskResult> {
        self.tasks
            .lock()
            .unwrap()
            .get(&task_id)
            .map(|t| t.result.clone())
    }
}

impl Clone for MutantClient {
    fn clone(&self) -> Self {
        Self {
            sender: None,
            receiver: None,
            tasks: self.tasks.clone(),
            task_channels: self.task_channels.clone(),
            pending_requests: self.pending_requests.clone(), // Clone the new map
            state: self.state.clone(),
        }
    }
}

impl MutantClient {
    pub fn partial_take_receiver(&mut self) -> Self {
        let mut clone = self.clone();

        clone.receiver = self.receiver.take();

        clone
    }

    /// Create a new client with a WebSocket connection to the given URL
    /// This is useful for background tasks that need their own connection
    pub async fn new_with_connection(url: &str) -> Result<Self, ClientError> {
        let mut client = Self::new();
        client.connect(url).await?;
        Ok(client)
    }
}

// Need to run this once at the start of the WASM application
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}
