use log::{debug, error, info, trace, warn};
use mutant_protocol::{
    ErrorResponse, ExportResponse, ImportResponse, ListKeysResponse, MvSuccessResponse, Response,
    RmSuccessResponse, Task, TaskCreatedResponse, TaskListResponse, TaskProgress,
    TaskResult, TaskResultResponse, TaskStatus, TaskStoppedResponse, TaskType,
    TaskUpdateResponse, WalletBalanceResponse, DaemonStatusResponse,
    // Colony integration responses
    SearchResponse, AddContactResponse, ListContentResponse, SyncContactsResponse, GetUserContactResponse, ListContactsResponse,
    ColonyEvent,
};

use crate::{
    error::ClientError, ClientTaskMap, PendingRequestKey, PendingRequestMap, PendingSender,
    TaskChannelsMap,
};

use super::MutantClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(target_arch = "wasm32")]
use std::sync::OnceLock;

// Global callback for colony progress events (WASM only)
#[cfg(target_arch = "wasm32")]
static COLONY_PROGRESS_CALLBACK: OnceLock<fn(ColonyEvent, Option<String>)> = OnceLock::new();

/// Set the global colony progress callback (WASM only)
#[cfg(target_arch = "wasm32")]
pub fn set_colony_progress_callback(callback: fn(ColonyEvent, Option<String>)) {
    let _ = COLONY_PROGRESS_CALLBACK.set(callback);
}

impl MutantClient {
    /// Processes a deserialized response from the server
    pub fn process_response(
        response: Response,
        tasks: &ClientTaskMap,
        task_channels: &TaskChannelsMap,
        pending_requests: &PendingRequestMap,
    ) {
        match response {
            Response::TaskCreated(TaskCreatedResponse { task_id }) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::TaskCreation);

                if let Some(PendingSender::TaskCreation(sender, channels, task_type)) =
                    pending_sender
                {
                    tasks.lock().unwrap().insert(
                        task_id,
                        Task {
                            id: task_id,
                            task_type,
                            status: TaskStatus::Pending,
                            progress: None,
                            result: TaskResult::Pending,
                            key: None,
                        },
                    );

                    task_channels.lock().unwrap().insert(task_id, channels);

                    if sender.send(Ok(task_id)).is_err() {
                        warn!("Failed to send TaskCreated response to waiting future (receiver dropped?)");
                        tasks.lock().unwrap().remove(&task_id);
                        task_channels.lock().unwrap().remove(&task_id);
                    }
                } else {
                    warn!(
                        "Received TaskCreated ({}) but no matching request was pending.",
                        task_id
                    );
                }
            }
            Response::TaskUpdate(TaskUpdateResponse {
                task_id,
                status,
                progress,
            }) => {
                let mut tasks_guard = tasks.lock().unwrap();
                let task_exists = tasks_guard.contains_key(&task_id);

                if let Some(task) = tasks_guard.get_mut(&task_id) {
                    task.status = status;
                    task.progress = progress.clone();

                    if let Some(progress_update) = progress {
                        if let Some((_, progress_tx, _)) = task_channels.lock().unwrap().get(&task_id)
                        {
                            if progress_tx.send(Ok(progress_update)).is_err() {
                                warn!("Failed to send progress update for task {}", task_id);
                            }
                        }
                    }
                } else {
                    warn!(
                        "Received TaskUpdate for unknown task {}, creating entry.",
                        task_id
                    );
                    let task_type = match &progress {
                        Some(TaskProgress::Put(_)) => TaskType::Put,
                        _ => TaskType::Get,
                    };
                    tasks_guard.insert(
                        task_id,
                        Task {
                            id: task_id,
                            task_type,
                            status,
                            progress,
                            result: TaskResult::Pending,
                            key: None,
                        },
                    );
                }

                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::QueryTask);
                if let Some(PendingSender::QueryTask(sender)) = pending_sender {
                    if let Some(task) = tasks_guard.get(&task_id) {
                        if sender.send(Ok(task.clone())).is_err() {
                            warn!("Failed to send TaskUpdate response to QueryTask request (receiver dropped)");
                        }
                    } else {
                        warn!("Task {} not found when trying to respond to QueryTask request after TaskUpdate.", task_id);
                        let _ = sender.send(Err(ClientError::TaskNotFound(task_id)));
                    }
                } else if task_exists {
                    // No pending query, just updated the task
                } else {
                    // No pending query and task was created by this update - already logged warning
                }
            }
            Response::TaskResult(TaskResultResponse {
                task_id,
                status,
                result,
            }) => {
                let mut tasks_guard = tasks.lock().unwrap();
                let _task_existed = tasks_guard.contains_key(&task_id);

                if let Some(task) = tasks_guard.get_mut(&task_id) {
                    task.status = status;
                    task.result = result.clone();

                    if let Some((completion_tx, _, _)) = task_channels.lock().unwrap().remove(&task_id)
                    {
                        if completion_tx.send(Ok(result.clone())).is_err() {
                            warn!(
                                "Failed to send final task result for task {} (receiver dropped)",
                                task_id
                            );
                        }
                    }
                } else {
                    warn!(
                        "Received TaskResult for unknown task {}, creating entry.",
                        task_id
                    );
                    // FIXME: Nonsense
                    let task_type = if let TaskResult::Error(_) = result {
                        TaskType::Get
                    } else {
                        TaskType::Put
                    };
                    tasks_guard.insert(
                        task_id,
                        Task {
                            id: task_id,
                            task_type,
                            status,
                            result,
                            progress: None,
                            key: None,
                        },
                    );
                }

                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::QueryTask);
                if let Some(PendingSender::QueryTask(sender)) = pending_sender {
                    if let Some(task) = tasks_guard.get(&task_id) {
                        if sender.send(Ok(task.clone())).is_err() {
                            warn!("Failed to send TaskResult response to QueryTask request (receiver dropped)");
                        }
                    } else {
                        warn!("Task {} not found when trying to respond to QueryTask request after TaskResult.", task_id);
                        let _ = sender.send(Err(ClientError::TaskNotFound(task_id)));
                    }
                }
            }
            Response::TaskList(TaskListResponse { tasks: task_list }) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::ListTasks);
                if let Some(PendingSender::ListTasks(sender)) = pending_sender {
                    if sender.send(Ok(task_list)).is_err() {
                        warn!("Failed to send TaskList response (receiver dropped)");
                    }
                } else {
                    warn!("Received TaskList but no ListTasks request was pending");
                }
            }
            Response::Error(ErrorResponse {
                error,
                original_request: _,
            }) => {
                error!(
                    "Server error received: {}. Check server logs for details.",
                    error
                );

                let mut requests = pending_requests.lock().unwrap();

                if let Some(PendingSender::TaskCreation(sender, _, _)) =
                    requests.remove(&PendingRequestKey::TaskCreation)
                {
                    error!("Error occurred during task creation: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::ListTasks(sender)) =
                    requests.remove(&PendingRequestKey::ListTasks)
                {
                    error!("Error occurred during task list request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::QueryTask(sender)) =
                    requests.remove(&PendingRequestKey::QueryTask)
                {
                    error!("Error occurred during task query request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::Rm(sender)) =
                    requests.remove(&PendingRequestKey::Rm)
                {
                    error!("Error occurred during rm request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::Mv(sender)) =
                    requests.remove(&PendingRequestKey::Mv)
                {
                    error!("Error occurred during mv request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::ListKeys(sender)) =
                    requests.remove(&PendingRequestKey::ListKeys)
                {
                    error!("Error occurred during list keys request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::Stats(sender)) =
                    requests.remove(&PendingRequestKey::Stats)
                {
                    error!("Error occurred during stats request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::WalletBalance(sender)) =
                    requests.remove(&PendingRequestKey::WalletBalance)
                {
                    error!("Error occurred during wallet balance request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::DaemonStatus(sender)) =
                    requests.remove(&PendingRequestKey::DaemonStatus)
                {
                    error!("Error occurred during daemon status request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::Import(sender)) =
                    requests.remove(&PendingRequestKey::Import)
                {
                    error!("Error occurred during import request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error)));
                } else if let Some(PendingSender::Export(sender)) =
                    requests.remove(&PendingRequestKey::Export)
                {
                    error!("Error occurred during export request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::Search(sender)) =
                    requests.remove(&PendingRequestKey::Search)
                {
                    error!("Error occurred during search request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::AddContact(sender)) =
                    requests.remove(&PendingRequestKey::AddContact)
                {
                    error!("Error occurred during add contact request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::ListContent(sender)) =
                    requests.remove(&PendingRequestKey::ListContent)
                {
                    error!("Error occurred during list content request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::SyncContacts(sender)) =
                    requests.remove(&PendingRequestKey::SyncContacts)
                {
                    error!("Error occurred during sync contacts request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::GetUserContact(sender)) =
                    requests.remove(&PendingRequestKey::GetUserContact)
                {
                    error!("Error occurred during get user contact request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::ListContacts(sender)) =
                    requests.remove(&PendingRequestKey::ListContacts)
                {
                    error!("Error occurred during list contacts request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::ListDirectory(sender)) =
                    requests.remove(&PendingRequestKey::ListDirectory)
                {
                    error!("Error occurred during list directory request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else if let Some(PendingSender::GetFileInfo(sender)) =
                    requests.remove(&PendingRequestKey::GetFileInfo)
                {
                    error!("Error occurred during get file info request: {}", error);
                    let _ = sender.send(Err(ClientError::ServerError(error.clone())));
                } else {
                    warn!("Received server error, but no matching pending request found.");
                }
            }
            Response::RmSuccess(RmSuccessResponse { user_key: _ }) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::Rm);
                if let Some(PendingSender::Rm(sender)) = pending_sender {
                    if sender.send(Ok(())).is_err() {
                        warn!("Failed to send RM success response (receiver dropped)");
                    }
                } else {
                    warn!("Received RM success response but no Rm request was pending");
                }
            }
            Response::MvSuccess(MvSuccessResponse { old_key: _, new_key: _ }) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::Mv);
                if let Some(PendingSender::Mv(sender)) = pending_sender {
                    if sender.send(Ok(())).is_err() {
                        warn!("Failed to send MV success response (receiver dropped)");
                    }
                } else {
                    warn!("Received MV success response but no Mv request was pending");
                }
            }
            Response::ListKeys(ListKeysResponse { keys }) => {
                log::debug!("Received ListKeys response with {} keys", keys.len());

                // Safely get the pending sender
                let pending_sender = match pending_requests.lock() {
                    Ok(mut guard) => guard.remove(&PendingRequestKey::ListKeys),
                    Err(e) => {
                        error!("Failed to lock pending_requests mutex: {:?}", e);
                        None
                    }
                };

                // Process the sender if it exists
                if let Some(PendingSender::ListKeys(sender)) = pending_sender {
                    debug!("Sending ListKeys response to waiting future");

                    // Clone the keys to avoid any potential memory issues
                    let keys_clone = keys.clone();

                    // Send the response and handle errors
                    match sender.send(Ok(keys_clone)) {
                        Ok(_) => debug!("Successfully sent ListKeys response"),
                        Err(_) => warn!("Failed to send ListKeys response (receiver dropped)")
                    }
                } else {
                    warn!("Received ListKeys response but no ListKeys request was pending");
                }
            }
            Response::Stats(stats_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::Stats);
                if let Some(PendingSender::Stats(sender)) = pending_sender {
                    if sender.send(Ok(stats_response)).is_err() {
                        warn!("Failed to send Stats response (receiver dropped)");
                    }
                } else {
                    warn!("Received Stats response but no Stats request was pending");
                }
            }
            Response::WalletBalance(wallet_balance_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::WalletBalance);
                if let Some(PendingSender::WalletBalance(sender)) = pending_sender {
                    if sender.send(Ok(wallet_balance_response)).is_err() {
                        warn!("Failed to send WalletBalance response (receiver dropped)");
                    }
                } else {
                    warn!("Received WalletBalance response but no WalletBalance request was pending");
                }
            }
            Response::DaemonStatus(daemon_status_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::DaemonStatus);
                if let Some(PendingSender::DaemonStatus(sender)) = pending_sender {
                    if sender.send(Ok(daemon_status_response)).is_err() {
                        warn!("Failed to send DaemonStatus response (receiver dropped)");
                    }
                } else {
                    warn!("Received DaemonStatus response but no DaemonStatus request was pending");
                }
            }
            Response::Import(ImportResponse { result }) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::Import);
                if let Some(PendingSender::Import(sender)) = pending_sender {
                    if sender.send(Ok(result)).is_err() {
                        warn!("Failed to send Import response (receiver dropped)");
                    }
                } else {
                    warn!("Received Import response but no Import request was pending");
                }
            }
            Response::Export(ExportResponse { result }) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::Export);
                if let Some(PendingSender::Export(sender)) = pending_sender {
                    if sender.send(Ok(result)).is_err() {
                        warn!("Failed to send Export response (receiver dropped)");
                    }
                } else {
                    warn!("Received Export response but no Export request was pending");
                }
            }
            Response::GetData(data_response) => {
                let task_id = data_response.task_id;
                debug!("Received GetData response for task {}, chunk {}/{}",
                       task_id, data_response.chunk_index + 1, data_response.total_chunks);

                // Get the data stream sender from the task channels
                if let Some((_, _, Some(data_stream_tx))) = task_channels.lock().unwrap().get(&task_id) {
                    // Send the data to the client
                    if data_stream_tx.send(Ok(data_response.data)).is_err() {
                        warn!("Failed to send data chunk for task {} (receiver dropped)", task_id);
                    }
                } else {
                    warn!("Received GetData for task {} but no data stream channel found", task_id);
                }
            },
            Response::TaskStopped(res) => handle_task_stopped(res, pending_requests.clone()),
            // Colony integration responses
            Response::Search(search_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::Search);
                if let Some(PendingSender::Search(sender)) = pending_sender {
                    if sender.send(Ok(search_response)).is_err() {
                        warn!("Failed to send Search response (receiver dropped)");
                    }
                } else {
                    warn!("Received Search response but no Search request was pending");
                }
            }
            Response::AddContact(add_contact_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::AddContact);
                if let Some(PendingSender::AddContact(sender)) = pending_sender {
                    if sender.send(Ok(add_contact_response)).is_err() {
                        warn!("Failed to send AddContact response (receiver dropped)");
                    }
                } else {
                    warn!("Received AddContact response but no AddContact request was pending");
                }
            }
            Response::ListContent(list_content_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::ListContent);
                if let Some(PendingSender::ListContent(sender)) = pending_sender {
                    if sender.send(Ok(list_content_response)).is_err() {
                        warn!("Failed to send ListContent response (receiver dropped)");
                    }
                } else {
                    warn!("Received ListContent response but no ListContent request was pending");
                }
            }
            Response::SyncContacts(sync_contacts_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::SyncContacts);
                if let Some(PendingSender::SyncContacts(sender)) = pending_sender {
                    if sender.send(Ok(sync_contacts_response)).is_err() {
                        warn!("Failed to send SyncContacts response (receiver dropped)");
                    }
                } else {
                    warn!("Received SyncContacts response but no SyncContacts request was pending");
                }
            }
            Response::GetUserContact(get_user_contact_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::GetUserContact);
                if let Some(PendingSender::GetUserContact(sender)) = pending_sender {
                    if sender.send(Ok(get_user_contact_response)).is_err() {
                        warn!("Failed to send GetUserContact response (receiver dropped)");
                    }
                } else {
                    warn!("Received GetUserContact response but no GetUserContact request was pending");
                }
            }
            Response::ListContacts(list_contacts_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::ListContacts);
                if let Some(PendingSender::ListContacts(sender)) = pending_sender {
                    if sender.send(Ok(list_contacts_response)).is_err() {
                        warn!("Failed to send ListContacts response (receiver dropped)");
                    }
                } else {
                    warn!("Received ListContacts response but no ListContacts request was pending");
                }
            }
            // Other colony responses that don't have direct client methods yet
            Response::StreamingPad(streaming_pad_response) => {
                let task_id = streaming_pad_response.task_id;
                info!("CLIENT: Received streaming pad for task {} (pad {}/{})",
                      task_id,
                      streaming_pad_response.pad_index + 1,
                      streaming_pad_response.total_pads);

                // Forward streaming pad data immediately to the data stream channel
                if let Some((_, _, Some(data_stream_tx))) = task_channels.lock().unwrap().get(&task_id) {
                    // Send the pad data to the client immediately
                    if data_stream_tx.send(Ok(streaming_pad_response.data)).is_err() {
                        warn!("CLIENT: Failed to send streaming pad data for task {} (receiver dropped)", task_id);
                    }
                } else {
                    warn!("CLIENT: No data stream channel found for streaming pad task {}", task_id);
                }
            },
            Response::IndexContent(_) | Response::GetMetadata(_) => {
                // These might be added later if needed
                warn!("Received unsupported colony response");
            },
            Response::ColonyProgress(progress_response) => {
                // Colony progress events are handled by the web UI directly
                debug!("Received colony progress event: {:?}", progress_response.event);

                // Call the global colony progress callback if it's set
                #[cfg(target_arch = "wasm32")]
                {
                    if let Some(callback) = COLONY_PROGRESS_CALLBACK.get() {
                        callback(progress_response.event, progress_response.operation_id);
                    }
                }
            },
            // Filesystem navigation responses
            Response::ListDirectory(list_directory_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::ListDirectory);
                if let Some(PendingSender::ListDirectory(sender)) = pending_sender {
                    if sender.send(Ok(list_directory_response)).is_err() {
                        warn!("Failed to send ListDirectory response (receiver dropped)");
                    }
                } else {
                    warn!("Received ListDirectory response but no ListDirectory request was pending");
                }
            },
            Response::GetFileInfo(get_file_info_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::GetFileInfo);
                if let Some(PendingSender::GetFileInfo(sender)) = pending_sender {
                    if sender.send(Ok(get_file_info_response)).is_err() {
                        warn!("Failed to send GetFileInfo response (receiver dropped)");
                    }
                } else {
                    warn!("Received GetFileInfo response but no GetFileInfo request was pending");
                }
            },
            // Onboarding responses
            Response::OnboardingStatus(onboarding_status_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::OnboardingStatus);
                if let Some(PendingSender::OnboardingStatus(sender)) = pending_sender {
                    if sender.send(Ok(onboarding_status_response)).is_err() {
                        warn!("Failed to send OnboardingStatus response (receiver dropped)");
                    }
                } else {
                    warn!("Received OnboardingStatus response but no OnboardingStatus request was pending");
                }
            },
            Response::SetCredentials(set_credentials_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::SetCredentials);
                if let Some(PendingSender::SetCredentials(sender)) = pending_sender {
                    if sender.send(Ok(set_credentials_response)).is_err() {
                        warn!("Failed to send SetCredentials response (receiver dropped)");
                    }
                } else {
                    warn!("Received SetCredentials response but no SetCredentials request was pending");
                }
            },
            Response::InitializeColony(initialize_colony_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::InitializeColony);
                if let Some(PendingSender::InitializeColony(sender)) = pending_sender {
                    if sender.send(Ok(initialize_colony_response)).is_err() {
                        warn!("Failed to send InitializeColony response (receiver dropped)");
                    }
                } else {
                    warn!("Received InitializeColony response but no InitializeColony request was pending");
                }
            },
            Response::RestartDaemon(restart_daemon_response) => {
                let pending_sender = pending_requests
                    .lock()
                    .unwrap()
                    .remove(&PendingRequestKey::RestartDaemon);
                if let Some(PendingSender::RestartDaemon(sender)) = pending_sender {
                    if sender.send(Ok(restart_daemon_response)).is_err() {
                        warn!("Failed to send RestartDaemon response (receiver dropped)");
                    }
                } else {
                    warn!("Received RestartDaemon response but no RestartDaemon request was pending");
                }
            },
        }
    }

    pub async fn next_response(&mut self) -> Option<Result<Response, ClientError>> {
        if let Some(receiver) = &mut self.receiver {
            // Use a loop to handle non-text messages without recursion
            loop {
                // Use the async next() method from nash-ws to wait for the next message
                match receiver.next().await {
                    Some(Ok(message)) => {
                        // Process the message
                        match message {
                            nash_ws::Message::Text(text) => {
                                match serde_json::from_str::<Response>(&text) {
                                    Ok(response) => {
                                        Self::process_response(
                                            response.clone(),
                                            &self.tasks,
                                            &self.task_channels,
                                            &self.pending_requests,
                                        );

                                        return Some(Ok(response));
                                    }
                                    Err(e) => {
                                        error!("CLIENT: Failed to deserialize response: {}", e);
                                        error!("CLIENT: Raw response text: {}", text);
                                        return Some(Err(ClientError::DeserializationError(e)));
                                    }
                                }
                            }
                            nash_ws::Message::Binary(_data) => {
                                // Continue the loop to wait for the next message
                                continue;
                            }
                            _ => {
                                // Continue the loop to wait for the next message
                                continue;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        error!("CLIENT: WebSocket error: {:?}", e);
                        return Some(Err(ClientError::WebSocketError(format!("{:?}", e))));
                    }
                    None => {
                        warn!("CLIENT: WebSocket receiver returned None - connection closed");
                        return None;
                    }
                }
            }
        } else {
            // This is the error we're seeing - receiver is None
            // This can happen during connection setup or if the receiver was moved
            warn!("CLIENT: WebSocket receiver is None (not connected or receiver moved to response handler)");

            // Instead of returning None immediately, we could wait a bit and retry
            // This helps with the race condition during connection setup
            #[cfg(target_arch = "wasm32")]
            {
                use wasm_bindgen_futures::JsFuture;
                use web_sys::js_sys;
                let promise = js_sys::Promise::new(&mut |resolve, _| {
                    web_sys::window()
                        .unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 10)
                        .unwrap();
                });
                let _ = JsFuture::from(promise).await;
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }

            // Return None to indicate no response available
            None
        }
    }
}

// Correctly define the function to accept Arc<Mutex<...>>
fn handle_task_stopped(
    res: TaskStoppedResponse,
    pending_requests_mutex: Arc<Mutex<HashMap<PendingRequestKey, PendingSender>>>,
) {
    trace!("Received TaskStopped response for task {}", res.task_id);
    // Lock the mutex to get mutable access to the map
    let mut pending_requests = match pending_requests_mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            error!(
                "Mutex poisoned when handling TaskStopped for task {}: {}. Recovering.",
                res.task_id, poisoned
            );
            poisoned.into_inner() // Recover the data even if poisoned
        }
    };

    // Find the corresponding sender using the correct key type
    if let Some(PendingSender::StopTask(sender)) =
        pending_requests.remove(&PendingRequestKey::StopTask)
    {
        // Sender expects Result<TaskStoppedResponse, ClientError>
        if sender.send(Ok(res.clone())).is_err() {
            warn!(
                "Failed to send TaskStopped result for task {}: receiver dropped",
                res.task_id
            );
        }
    } else {
        warn!(
            "Received TaskStopped response for unknown/mismatched request key (task_id: {})",
            res.task_id
        );
    }
    // Lock guard is dropped here, releasing the mutex
}
