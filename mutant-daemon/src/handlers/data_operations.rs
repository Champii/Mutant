use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

use crate::error::Error as DaemonError;
use super::{TaskEntry, TaskMap, ActiveKeysMap, try_register_key, release_key, is_public_only_mode, PUBLIC_ONLY_ERROR_MSG};
use mutant_lib::storage::ScratchpadAddress;
use mutant_lib::MutAnt;
use mutant_protocol::{
    ErrorResponse, GetCallback, GetDataResponse, GetEvent, GetRequest, GetResult,
    PutCallback, PutEvent, PutRequest, PutResult, PutSource, Response, RmRequest, RmSuccessResponse,
    Task, TaskCreatedResponse, TaskProgress, TaskResult, TaskResultResponse, TaskResultType,
    TaskStatus, TaskType, TaskUpdateResponse
};

use super::common::UpdateSender;

pub(crate) async fn handle_put(
    req: PutRequest,
    update_tx: UpdateSender,
    mutant: Arc<MutAnt>,
    tasks: TaskMap,
    active_keys: ActiveKeysMap,
    original_request_str: &str,
) -> Result<(), DaemonError> {
    // Check if we're in public-only mode
    if is_public_only_mode() {
        return update_tx
            .send(Response::Error(ErrorResponse {
                error: PUBLIC_ONLY_ERROR_MSG.to_string(),
                original_request: Some(original_request_str.to_string()),
            }))
            .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)));
    }

    let task_id = Uuid::new_v4();
    let user_key = req.user_key.clone();

    // Get source info for logging
    let (source_info, data_arc) = match &req.source {
        PutSource::FilePath(path) => {
            let source_path = path.clone();

            // Try to register the key for this task
            try_register_key(
                &active_keys,
                &user_key,
                task_id,
                TaskType::Put,
                &update_tx,
                original_request_str,
            ).await?;

            // Read data into an Arc<Vec<u8>> to avoid cloning the whole data later.
            let data_bytes_vec = fs::read(path).await.map_err(|e| {
                // Release the key if we fail to read the file
                let active_keys_clone = active_keys.clone();
                let user_key_clone = user_key.clone();
                tokio::spawn(async move {
                    release_key(&active_keys_clone, &user_key_clone).await;
                });

                DaemonError::IoError(format!(
                    "Failed to read source file {}: {}",
                    path, e
                ))
            })?;

            (format!("file:{}", source_path), Arc::new(data_bytes_vec))
        },
        PutSource::Bytes(bytes) => {
            // Try to register the key for this task
            try_register_key(
                &active_keys,
                &user_key,
                task_id,
                TaskType::Put,
                &update_tx,
                original_request_str,
            ).await?;

            let filename = req.filename.clone().unwrap_or_else(|| "direct-bytes".to_string());
            (format!("bytes:{}", filename), Arc::new(bytes.clone()))
        }
    };

    let task = Task {
        id: task_id,
        task_type: TaskType::Put,
        status: TaskStatus::Pending,
        progress: None,
        result: TaskResult::Pending,
        key: Some(user_key.clone()),
    };
    // We will insert the TaskEntry after spawning the task and getting the handle

    update_tx
        .send(Response::TaskCreated(TaskCreatedResponse { task_id }))
        .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)))?;

    // Clone Arc handles *before* moving them into the async block
    let tasks_clone = tasks.clone();
    let mutant_clone = mutant.clone();
    let update_tx_clone_for_spawn = update_tx.clone();
    let data_arc_clone = data_arc.clone(); // Clone the Arc for the task
    let active_keys_clone = active_keys.clone();
    let user_key_clone = user_key.clone();

    let task_handle = tokio::spawn(async move {
        // Use the cloned handles inside the spawned task
        let tasks = tasks_clone;
        let mutant = mutant_clone;
        let update_tx = update_tx_clone_for_spawn;
        let data_to_put = data_arc_clone; // Use the cloned Arc
        let active_keys = active_keys_clone;
        let user_key = user_key_clone;

        log::info!("Starting PUT task: task_id={}, user_key={}, source={}", task_id, user_key, source_info);

        // Update status in TaskMap
        {
            let mut tasks_guard = tasks.write().await;
            if let Some(entry) = tasks_guard.get_mut(&task_id) {
                entry.task.status = TaskStatus::InProgress;
            }
        } // RwLockWriteGuard is dropped here

        // Create callback *inside* the task
        let update_tx_clone = update_tx.clone();
        let task_id_clone = task_id;
        let tasks_clone = tasks.clone();
        let callback: PutCallback = Arc::new(move |event: PutEvent| {
            let tx = update_tx_clone.clone();
            let task_id = task_id_clone;
            let tasks = tasks_clone.clone();
            Box::pin(async move {
                let progress = TaskProgress::Put(event);
                // Update task progress in map
                let mut tasks_guard = tasks.write().await;
                if let Some(entry) = tasks_guard.get_mut(&task_id) {
                    // Only update if the task is still considered InProgress
                    if entry.task.status == TaskStatus::InProgress {
                        entry.task.progress = Some(progress.clone());
                        // Send update via channel
                        let _ = tx.send(Response::TaskUpdate(TaskUpdateResponse {
                            task_id,
                            status: TaskStatus::InProgress,
                            progress: Some(progress),
                        }));
                    } else {
                        log::warn!("Received PUT progress update for task not InProgress (status: {:?}). Ignoring. task_id={}", entry.task.status, task_id);
                        return Ok(false); // Indicate to stop sending updates if task is no longer InProgress
                    }
                }
                drop(tasks_guard);
                Ok(true)
            })
        });

        // Call put with the callback
        let result = mutant
            .put(
                &user_key,
                data_to_put, // Pass the Arc<Vec<u8>>
                req.mode,
                req.public,
                req.no_verify,
                Some(callback), // Pass callback here
            )
            .await;

        let final_response = {
            let mut tasks_guard = tasks.write().await;
            // Check if task entry still exists and hasn't been stopped
            if let Some(entry) = tasks_guard.get_mut(&task_id) {
                // Only update if the task hasn't been stopped externally
                if entry.task.status != TaskStatus::Stopped {
                    match result {
                        Ok(_addr) => {
                            // For public keys, get the index pad address from the master index
                            let public_address = if req.public {
                                // Get the index pad address from the master index
                                match mutant.get_public_index_address(&user_key).await {
                                    Ok(addr) => Some(addr),
                                    Err(e) => {
                                        log::warn!("Failed to get public index address for key {}: {}", user_key, e);
                                        None
                                    }
                                }
                            } else {
                                None
                            };

                            entry.task.status = TaskStatus::Completed;
                            entry.task.result = TaskResult::Result(TaskResultType::Put(PutResult {
                                public_address,
                            }));
                            entry.abort_handle = None; // Task finished, remove handle
                            log::info!("PUT task completed successfully: task_id={}, user_key={}, source={}", task_id, user_key, source_info);
                            Some(Response::TaskResult(TaskResultResponse {
                                task_id,
                                status: TaskStatus::Completed,
                                result: entry.task.result.clone(),
                            }))
                        }
                        Err(e) => {
                            entry.task.status = TaskStatus::Failed;
                            entry.task.result = TaskResult::Error(e.to_string());
                            entry.abort_handle = None; // Task finished, remove handle
                            log::error!("PUT task failed: task_id={}, user_key={}, source={}, error={}", task_id, user_key, source_info, e);
                            Some(Response::TaskResult(TaskResultResponse {
                                task_id,
                                status: TaskStatus::Failed,
                                result: entry.task.result.clone(),
                            }))
                        }
                    }
                } else {
                    log::info!("PUT task was stopped before completion: task_id={}", task_id);
                    entry.abort_handle = None; // Ensure handle is cleared if stopped
                    None // No final result to send if stopped
                }
            } else {
                log::warn!("Task entry removed before PUT completion? task_id={}", task_id);
                None
            }
        };

        if let Some(response) = final_response {
            let _ = update_tx.send(response);
        }

        // Release the key when the operation completes
        release_key(&active_keys, &user_key).await;
        log::debug!("Released key '{}' after PUT operation", user_key);
    });

    // Get the abort handle and create the TaskEntry
    let abort_handle = task_handle.abort_handle();
    let task_entry = TaskEntry {
        task, // The task struct created earlier
        abort_handle: Some(abort_handle),
    };

    // Insert the TaskEntry into the map *after* spawning
    {
        tasks.write().await.insert(task_id, task_entry);
    }

    Ok(())
}

pub(crate) async fn handle_get(
    req: GetRequest,
    update_tx: UpdateSender,
    mutant: Arc<MutAnt>,
    tasks: TaskMap,
    active_keys: ActiveKeysMap,
    original_request_str: &str,
) -> Result<(), DaemonError> {
    let task_id = Uuid::new_v4();
    let user_key = req.user_key.clone();
    let destination_path = req.destination_path.clone(); // Keep path for logging and writing
    let stream_data = req.stream_data; // Whether to stream data back to client

    log::info!("DAEMON: Received GET request: key={}, destination_path={:?}, public={}, stream_data={}, original_request={}",
               user_key, destination_path, req.public, stream_data, original_request_str);

    // Try to register the key for this task
    log::info!("DAEMON: Attempting to register key '{}' for task {}", user_key, task_id);
    try_register_key(
        &active_keys,
        &user_key,
        task_id,
        TaskType::Get,
        &update_tx,
        original_request_str,
    ).await?;

    log::info!("DAEMON: Creating GET task: id={}, key={}", task_id, user_key);
    let task = Task {
        id: task_id,
        task_type: TaskType::Get,
        status: TaskStatus::Pending,
        progress: None,
        result: TaskResult::Pending,
        key: Some(user_key.clone()),
    };

    log::info!("DAEMON: Sending TaskCreated response for task {}", task_id);
    if let Err(e) = update_tx.send(Response::TaskCreated(TaskCreatedResponse { task_id })) {
        let err_msg = format!("Update channel send error: {}", e);
        log::error!("DAEMON: Failed to send TaskCreated response: {}", err_msg);
        return Err(DaemonError::Internal(err_msg));
    }
    log::info!("DAEMON: TaskCreated response sent successfully for task {}", task_id);

    // Clone Arc handles *before* moving them into the async block
    let tasks_clone = tasks.clone();
    let mutant_clone = mutant.clone();
    let update_tx_clone_for_spawn = update_tx.clone();
    let active_keys_clone = active_keys.clone();
    let user_key_clone = user_key.clone();

    let task_handle = tokio::spawn(async move {
        // Use the cloned handles inside the spawned task
        let tasks = tasks_clone;
        let mutant = mutant_clone;
        let update_tx = update_tx_clone_for_spawn;
        let active_keys = active_keys_clone;
        let user_key = user_key_clone;

        log::info!("Starting GET task: task_id={}, user_key={}, destination_path={:?}", task_id, user_key, destination_path);

        // Update status in TaskMap
        {
            let mut tasks_guard = tasks.write().await;
            if let Some(entry) = tasks_guard.get_mut(&task_id) {
                entry.task.status = TaskStatus::InProgress;
            }
        }

        // Create callback *inside* the task
        let update_tx_clone = update_tx.clone();
        let task_id_clone = task_id;
        let tasks_clone = tasks.clone();
        let stream_data_clone = stream_data;
        let total_chunks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let total_chunks_clone = total_chunks.clone();

        let callback: GetCallback = Arc::new(move |event: GetEvent| {
            let tx = update_tx_clone.clone();
            let task_id = task_id_clone;
            let tasks = tasks_clone.clone();
            let stream_data = stream_data_clone;
            let total_chunks = total_chunks_clone.clone();

            Box::pin(async move {
                // Log the event type
                match &event {
                    GetEvent::Starting { total_chunks } => {
                        log::info!("DAEMON CALLBACK: GET task {} - Starting event with {} total chunks", task_id, total_chunks);
                    },
                    GetEvent::PadFetched => {
                        log::debug!("DAEMON CALLBACK: GET task {} - PadFetched event", task_id);
                    },
                    GetEvent::PadData { chunk_index, data } => {
                        log::info!("DAEMON CALLBACK: GET task {} - PadData event for chunk {} with {} bytes",
                                  task_id, chunk_index, data.len());
                    },
                    GetEvent::Complete => {
                        log::info!("DAEMON CALLBACK: GET task {} - Complete event", task_id);
                    },
                }

                // If this is a Starting event, store the total chunks
                if let GetEvent::Starting { total_chunks: chunks } = &event {
                    total_chunks.store(*chunks, std::sync::atomic::Ordering::SeqCst);
                    log::info!("DAEMON CALLBACK: GET task {} - Stored total chunks: {}", task_id, chunks);
                }

                // Handle streaming data if enabled - send via GetData response (not PadData event)
                if stream_data {
                    if let GetEvent::PadData { chunk_index, data } = &event {
                        // Send the data chunk directly to the client via GetData response
                        let total = total_chunks.load(std::sync::atomic::Ordering::SeqCst);
                        let is_last = *chunk_index == total - 1;

                        log::info!("DAEMON CALLBACK: GET task {} - Sending data chunk {}/{} ({} bytes, is_last={})",
                                  task_id, chunk_index + 1, total, data.len(), is_last);

                        if let Err(e) = tx.send(Response::GetData(GetDataResponse {
                            task_id,
                            chunk_index: *chunk_index,
                            total_chunks: total,
                            data: data.clone(),
                            is_last,
                        })) {
                            log::error!("DAEMON CALLBACK: GET task {} - Failed to send GetData response: {}", task_id, e);
                        } else {
                            log::info!("DAEMON CALLBACK: GET task {} - Successfully sent GetData response for chunk {}",
                                      task_id, chunk_index);
                        }
                    }
                }

                // Always send the regular progress update
                let progress = TaskProgress::Get(event);
                log::info!("DAEMON CALLBACK: GET task {} - Preparing to send TaskUpdate", task_id);

                // Update task progress
                let mut tasks_guard = tasks.write().await;
                if let Some(entry) = tasks_guard.get_mut(&task_id) {
                    // Only update if the task is still considered InProgress
                    if entry.task.status == TaskStatus::InProgress {
                        entry.task.progress = Some(progress.clone());
                        // Send update via channel
                        log::info!("DAEMON CALLBACK: GET task {} - Sending TaskUpdate response", task_id);
                        if let Err(e) = tx.send(Response::TaskUpdate(TaskUpdateResponse {
                            task_id,
                            status: TaskStatus::InProgress,
                            progress: Some(progress),
                        })) {
                            log::error!("DAEMON CALLBACK: GET task {} - Failed to send TaskUpdate: {}", task_id, e);
                        } else {
                            log::info!("DAEMON CALLBACK: GET task {} - Successfully sent TaskUpdate", task_id);
                        }
                    } else {
                        log::warn!("DAEMON CALLBACK: Received GET progress update for task not InProgress (status: {:?}). Ignoring. task_id={}", entry.task.status, task_id);
                        return Ok(false); // Indicate to stop sending updates if task is no longer InProgress
                    }
                } else {
                    log::warn!("DAEMON CALLBACK: GET task {} - Task not found in tasks map", task_id);
                }
                drop(tasks_guard);
                Ok(true)
            })
        });

        log::info!("DAEMON: GET task {} - Preparing to execute get operation for key '{}', public={}, stream_data={}",
                 task_id, user_key, req.public, stream_data);

        // Check if the key exists first for private keys
        let get_result = if req.public {
            log::info!("DAEMON: GET task {} - Executing public get operation", task_id);
            // TODO: Fix public key handling if necessary, ScratchpadAddress requires valid hex
            match ScratchpadAddress::from_hex(&user_key) {
                Ok(address) => {
                    log::info!("DAEMON: GET task {} - Valid public address format, calling get_public", task_id);
                    let result = mutant.get_public(&address, Some(callback), stream_data).await;
                    match &result {
                        Ok(_) => log::info!("DAEMON: GET task {} - get_public operation succeeded", task_id),
                        Err(e) => log::error!("DAEMON: GET task {} - get_public operation failed: {}", task_id, e),
                    }
                    result
                },
                Err(hex_err) => {
                    // Wrap the underlying lib error in DaemonError::LibError
                    let error_msg = format!("Invalid public key hex format for '{}': {}", user_key, hex_err);
                    log::error!("DAEMON: GET task {} - {}", task_id, error_msg);
                    let lib_err = mutant_lib::error::Error::Internal(error_msg);
                    Err(lib_err)
                }
            }
        } else {
            log::info!("DAEMON: GET task {} - Executing private get operation", task_id);
            // Check if the key exists first for better error messages
            let key_exists = mutant.contains_key(&user_key).await;
            log::info!("DAEMON: GET task {} - Key '{}' exists: {}", task_id, user_key, key_exists);

            if !key_exists {
                let error_msg = format!("Key '{}' not found", user_key);
                log::error!("DAEMON: GET task {} - {}", task_id, error_msg);
                Err(mutant_lib::error::Error::Internal(error_msg))
            } else {
                log::info!("DAEMON: GET task {} - Calling get with callback and stream_data={}", task_id, stream_data);
                let result = mutant.get(&user_key, Some(callback), stream_data).await;
                match &result {
                    Ok(data) => log::info!("DAEMON: GET task {} - get operation succeeded, data size: {}", task_id, data.len()),
                    Err(e) => log::error!("DAEMON: GET task {} - get operation failed: {}", task_id, e),
                }
                result
            }
        };

        let write_result = match get_result {
            Ok(data_bytes) => {
                // If streaming is enabled, we don't need to write to a file
                // The data has already been sent to the client via the callback
                if stream_data {
                    // Just return the empty data bytes for success
                    Ok(data_bytes)
                } else if let Some(path) = &destination_path {
                    // Write the received bytes to the destination path
                    fs::write(path, &data_bytes)
                        .await
                        .map_err(|e| {
                            DaemonError::IoError(format!(
                                "Failed to write to destination file {}: {}",
                                path, e
                            ))
                        })
                        .map(|_| data_bytes) // Pass data_bytes through on success for tracing length maybe
                } else {
                    // No destination path provided but not streaming - this shouldn't happen
                    // but we'll handle it gracefully
                    log::warn!("No destination path provided for non-streaming GET: task_id={}", task_id);
                    Ok(data_bytes)
                }
            }
            Err(e) => Err(DaemonError::LibError(e)), // Propagate the lib error
        };

        let final_response = {
            let mut tasks_guard = tasks.write().await;
            if let Some(entry) = tasks_guard.get_mut(&task_id) {
                // Only update if the task hasn't been stopped externally
                if entry.task.status != TaskStatus::Stopped {
                    match write_result {
                        Ok(data_bytes) => {
                            entry.task.status = TaskStatus::Completed;
                            entry.task.result =
                                TaskResult::Result(TaskResultType::Get(GetResult {
                                    size: data_bytes.len(),
                                    streamed: stream_data,
                                }));
                            entry.abort_handle = None; // Task finished, remove handle

                            // Format log message based on whether we have a destination path
                            if let Some(path) = &destination_path {
                                log::info!("GET task completed successfully: task_id={}, user_key={}, destination_path={}, bytes_written={}",
                                    task_id, user_key, path, data_bytes.len());
                            } else {
                                log::info!("GET task completed successfully: task_id={}, user_key={}, streamed=true, bytes_processed={}",
                                    task_id, user_key, data_bytes.len());
                            }

                            Some(Response::TaskResult(TaskResultResponse {
                                task_id,
                                status: TaskStatus::Completed,
                                result: entry.task.result.clone(),
                            }))
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            entry.task.status = TaskStatus::Failed;
                            entry.task.result = TaskResult::Error(error_msg.clone());
                            entry.abort_handle = None; // Task finished, remove handle

                            // Format log message based on whether we have a destination path
                            if let Some(path) = &destination_path {
                                log::error!("GET task failed: task_id={}, user_key={}, destination_path={}, error={}",
                                    task_id, user_key, path, error_msg);
                            } else {
                                log::error!("GET task failed: task_id={}, user_key={}, streamed=true, error={}",
                                    task_id, user_key, error_msg);
                            }

                            Some(Response::TaskResult(TaskResultResponse {
                                task_id,
                                status: TaskStatus::Failed,
                                result: entry.task.result.clone(),
                            }))
                        }
                    }
                } else {
                    log::info!("GET task was stopped before completion: task_id={}", task_id);
                    entry.abort_handle = None; // Ensure handle is cleared if stopped
                    None // No final result to send if stopped
                }
            } else {
                log::warn!("Task entry removed before GET completion? task_id={}", task_id);
                None
            }
        };
        if let Some(response) = final_response {
            log::info!("DAEMON: GET task {} - Sending final TaskResult response", task_id);
            if let Err(e) = update_tx.send(response) {
                log::error!("DAEMON: GET task {} - Failed to send final result: {}", task_id, e);
            } else {
                log::info!("DAEMON: GET task {} - Successfully sent final result", task_id);
            }
        } else {
            log::warn!("DAEMON: GET task {} - No final response to send", task_id);
        }

        // Release the key when the operation completes
        log::info!("DAEMON: GET task {} - Releasing key '{}'", task_id, user_key);
        release_key(&active_keys, &user_key).await;
        log::info!("DAEMON: GET task {} - Released key '{}' after GET operation", task_id, user_key);
    });

    // Get the abort handle and create the TaskEntry
    log::info!("DAEMON: GET request - Creating TaskEntry for task {}", task_id);
    let abort_handle = task_handle.abort_handle();
    let task_entry = TaskEntry {
        task, // The task struct created earlier
        abort_handle: Some(abort_handle),
    };

    // Insert the TaskEntry into the map *after* spawning
    log::info!("DAEMON: GET request - Inserting TaskEntry into tasks map for task {}", task_id);
    {
        tasks.write().await.insert(task_id, task_entry);
    }
    log::info!("DAEMON: GET request - Successfully inserted TaskEntry for task {}", task_id);

    log::info!("DAEMON: GET request - Completed handling for key '{}', task {}", user_key, task_id);
    Ok(())
}



pub(crate) async fn handle_rm(
    req: RmRequest,
    update_tx: UpdateSender,
    mutant: Arc<MutAnt>,
    active_keys: ActiveKeysMap,
    original_request_str: &str,
) -> Result<(), DaemonError> {
    // Check if we're in public-only mode
    if is_public_only_mode() {
        return update_tx
            .send(Response::Error(ErrorResponse {
                error: PUBLIC_ONLY_ERROR_MSG.to_string(),
                original_request: Some(original_request_str.to_string()),
            }))
            .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)));
    }

    let task_id = Uuid::new_v4();
    let user_key = req.user_key.clone();
    log::info!("Starting RM task: user_key={}", user_key);

    // Check if the key exists first
    let key_exists = mutant.contains_key(&user_key).await;

    if !key_exists {
        log::info!("RM task for non-existent key: user_key={}", user_key);
        // Return an error for non-existent keys
        return update_tx
            .send(Response::Error(ErrorResponse {
                error: format!("Key '{}' not found", user_key),
                original_request: Some(original_request_str.to_string()),
            }))
            .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)));
    }

    // Try to register the key for this task
    try_register_key(
        &active_keys,
        &user_key,
        task_id,
        TaskType::Rm,
        &update_tx,
        original_request_str,
    ).await?;

    let result = mutant.rm(&user_key).await;

    // Release the key after the operation completes
    release_key(&active_keys, &user_key).await;

    let response = match result {
        Ok(_) => {
            log::info!("RM task completed successfully: user_key={}", user_key);
            Response::RmSuccess(RmSuccessResponse { user_key })
        }
        Err(e) => {
            log::error!("RM task failed: user_key={}, error={}", user_key, e);
            Response::Error(ErrorResponse {
                error: e.to_string(),
                original_request: Some(original_request_str.to_string()),
            })
        }
    };

    update_tx
        .send(response)
        .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)))?;

    Ok(())
}
