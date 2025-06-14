use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

use crate::error::Error as DaemonError;
use super::{TaskEntry, TaskMap, ActiveKeysMap, try_register_key, release_key, is_public_only_mode, PUBLIC_ONLY_ERROR_MSG};
use super::colony::get_colony_manager;
use mutant_lib::storage::ScratchpadAddress;
use mutant_lib::MutAnt;
use mutant_protocol::{
    ErrorResponse, GetCallback, GetDataResponse, GetEvent, GetRequest, GetResult,
    PutCallback, PutEvent, PutRequest, PutResult, PutSource, Response, RmRequest, RmSuccessResponse,
    MvRequest, MvSuccessResponse, Task, TaskCreatedResponse, TaskProgress, TaskResult, TaskResultResponse, TaskResultType,
    TaskStatus, TaskType, TaskUpdateResponse, PutDataRequest
};

use super::common::UpdateSender;

/// Helper function to auto-index public uploads to colony
async fn auto_index_public_upload(
    user_key: &str,
    filename: Option<String>,
    file_size: u64,
    public_address: &str,
) {
    // Try to get the colony manager and auto-index the upload
    // This is a best-effort operation that shouldn't fail the upload
    match get_colony_manager().await {
        Ok(colony_manager) => {
            if let Err(e) = colony_manager.auto_index_public_upload(
                user_key,
                filename,
                file_size,
                public_address.to_string(),
            ).await {
                log::warn!("Failed to auto-index public upload for key {}: {}", user_key, e);
            }
        }
        Err(e) => {
            log::debug!("Colony manager not available for auto-indexing: {}", e);
        }
    }
}

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

    // Clone values we might need later to avoid borrow checker issues
    let req_mode = req.mode.clone();
    let req_public = req.public;
    let req_no_verify = req.no_verify;

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

            (format!("file:{}", source_path), Some(Arc::new(data_bytes_vec)))
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
            (format!("bytes:{}", filename), Some(Arc::new(bytes.clone())))
        },
        PutSource::Stream { total_size } => {
            // Try to register the key for this task
            try_register_key(
                &active_keys,
                &user_key,
                task_id,
                TaskType::Put,
                &update_tx,
                original_request_str,
            ).await?;

            let req_filename = req.filename.clone();

            // Initialize streaming data storage
            let streaming_data = StreamingPutData {
                user_key: user_key.clone(),
                filename: req_filename.clone(),
                mode: req_mode.clone(),
                public: req_public,
                no_verify: req_no_verify,
                total_chunks: 0, // Will be set when first chunk arrives
                total_size: *total_size,
                received_chunks: HashMap::new(),
                received_size: 0,
            };

            {
                let mut streaming_map = STREAMING_PUT_DATA.lock().await;
                streaming_map.insert(task_id, streaming_data);
            }

            let filename = req_filename.unwrap_or_else(|| "stream".to_string());
            log::info!("DAEMON: Initialized streaming put for task {} ({} bytes)", task_id, total_size);

            // For streaming, we create the task entry but don't spawn the operation yet
            // The actual put operation will be triggered when all chunks are received
            (format!("stream: {}", filename), None)
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

    // Only spawn the actual put operation for non-streaming sources
    let task_handle = if let Some(data_arc) = data_arc {
        // Clone Arc handles *before* moving them into the async block
        let tasks_clone = tasks.clone();
        let mutant_clone = mutant.clone();
        let update_tx_clone_for_spawn = update_tx.clone();
        let data_arc_clone = data_arc.clone(); // Clone the Arc for the task
        let active_keys_clone = active_keys.clone();
        let user_key_clone = user_key.clone();

        Some(tokio::spawn(async move {
        // Use the cloned handles inside the spawned task
        let tasks = tasks_clone;
        let mutant = mutant_clone;
        let update_tx = update_tx_clone_for_spawn;
        let data_to_put = data_arc_clone; // Use the cloned Arc
        let active_keys = active_keys_clone;
        let user_key = user_key_clone;

        log::info!("Starting PUT task: task_id={}, user_key={}, source={}", task_id, user_key, source_info);

        // Store file size for colony indexing before moving data_to_put
        let file_size = data_to_put.len() as u64;

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
                req_mode,
                req_public,
                req_no_verify,
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
                            let public_address = if req_public {
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
                                public_address: public_address.clone(),
                            }));
                            entry.abort_handle = None; // Task finished, remove handle
                            log::info!("PUT task completed successfully: task_id={}, user_key={}, source={}", task_id, user_key, source_info);

                            // Auto-index public uploads to colony
                            if req_public {
                                if let Some(ref addr) = public_address {
                                    // Extract filename from source info
                                    let filename = if source_info.starts_with("file:") {
                                        std::path::Path::new(&source_info[5..])
                                            .file_name()
                                            .and_then(|name| name.to_str())
                                            .map(|s| s.to_string())
                                    } else if source_info.starts_with("bytes:") {
                                        Some(source_info[6..].to_string())
                                    } else {
                                        None
                                    };

                                    // Spawn auto-indexing task (don't block the response)
                                    let user_key_clone = user_key.clone();
                                    let addr_clone = addr.clone();
                                    tokio::spawn(async move {
                                        auto_index_public_upload(&user_key_clone, filename, file_size, &addr_clone).await;
                                    });
                                }
                            }

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
    }))
    } else {
        // For streaming operations, no task handle yet
        None
    };

    // Get the abort handle and create the TaskEntry
    let abort_handle = task_handle.as_ref().map(|h| h.abort_handle());
    let task_entry = TaskEntry {
        task, // The task struct created earlier
        abort_handle,
    };

    // Insert the TaskEntry into the map *after* spawning
    {
        tasks.write().await.insert(task_id, task_entry);
    }

    Ok(())
}

// Global storage for streaming put data
use std::collections::HashMap;
use tokio::sync::Mutex;

lazy_static::lazy_static! {
    static ref STREAMING_PUT_DATA: Mutex<HashMap<uuid::Uuid, StreamingPutData>> = Mutex::new(HashMap::new());
}

#[derive(Debug)]
#[allow(dead_code)]
struct StreamingPutData {
    user_key: String,
    filename: Option<String>,
    mode: mutant_protocol::StorageMode,
    public: bool,
    no_verify: bool,
    total_chunks: usize,
    total_size: u64,
    received_chunks: HashMap<usize, Vec<u8>>,
    received_size: u64,
}

pub(crate) async fn handle_put_data(
    req: PutDataRequest,
    update_tx: UpdateSender,
    tasks: TaskMap,
    mutant: Arc<MutAnt>,
    active_keys: ActiveKeysMap,
    original_request_str: &str,
) -> Result<(), DaemonError> {
    let task_id = req.task_id;

    log::info!("DAEMON: Received PutData chunk {}/{} for task {} ({} bytes, is_last={})",
               req.chunk_index + 1, req.total_chunks, task_id, req.data.len(), req.is_last);

    // Get or create streaming data entry
    let mut streaming_data = STREAMING_PUT_DATA.lock().await;

    if let Some(put_data) = streaming_data.get_mut(&task_id) {
        // Add this chunk to the existing data
        put_data.received_chunks.insert(req.chunk_index, req.data.clone());
        put_data.received_size += req.data.len() as u64;

        log::info!("DAEMON: PutData task {} - Added chunk {}, total received: {}/{} bytes",
                   task_id, req.chunk_index, put_data.received_size, put_data.total_size);

        // Check if we have all chunks
        if req.is_last && put_data.received_chunks.len() == req.total_chunks {
            log::info!("DAEMON: PutData task {} - All chunks received, assembling data", task_id);

            // Assemble the complete data
            let mut complete_data = Vec::with_capacity(put_data.total_size as usize);
            for i in 0..req.total_chunks {
                if let Some(chunk) = put_data.received_chunks.get(&i) {
                    complete_data.extend_from_slice(chunk);
                } else {
                    let error_msg = format!("Missing chunk {} for task {}", i, task_id);
                    log::error!("DAEMON: PutData - {}", error_msg);

                    // Clean up and return error
                    streaming_data.remove(&task_id);
                    return update_tx
                        .send(Response::Error(ErrorResponse {
                            error: error_msg,
                            original_request: Some(original_request_str.to_string()),
                        }))
                        .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)));
                }
            }

            // Remove from streaming data storage
            let put_data = streaming_data.remove(&task_id).unwrap();
            drop(streaming_data); // Release the lock

            log::info!("DAEMON: PutData task {} - Assembled complete data ({} bytes), starting put operation",
                       task_id, complete_data.len());

            // Now trigger the actual put operation with the assembled data
            let user_key = put_data.user_key.clone();
            let mode = put_data.mode.clone();
            let public = put_data.public;
            let no_verify = put_data.no_verify;
            let filename = put_data.filename.clone(); // Extract filename for colony indexing

            // Update task status to InProgress
            {
                let mut tasks_guard = tasks.write().await;
                if let Some(entry) = tasks_guard.get_mut(&task_id) {
                    entry.task.status = TaskStatus::InProgress;
                }
            }

            // Create callback for progress updates
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

            // Spawn the actual PUT operation
            let mutant_clone = mutant.clone();
            let update_tx_clone = update_tx.clone();
            let tasks_clone = tasks.clone();
            let active_keys_clone = active_keys.clone();
            let user_key_clone = user_key.clone();
            let complete_data_arc = Arc::new(complete_data);

            tokio::spawn(async move {
                log::info!("DAEMON: Starting PUT operation for streaming task {}", task_id);

                // Store file size for colony indexing before moving complete_data_arc
                let file_size = complete_data_arc.len() as u64;

                // Call put with the callback
                let result = mutant_clone
                    .put(
                        &user_key,
                        complete_data_arc,
                        mode,
                        public,
                        no_verify,
                        Some(callback),
                    )
                    .await;

                let final_response = {
                    let mut tasks_guard = tasks_clone.write().await;
                    // Check if task entry still exists and hasn't been stopped
                    if let Some(entry) = tasks_guard.get_mut(&task_id) {
                        // Only update if the task hasn't been stopped externally
                        if entry.task.status != TaskStatus::Stopped {
                            match result {
                                Ok(_addr) => {
                                    // For public keys, get the index pad address from the master index
                                    let public_address = if public {
                                        // Get the index pad address from the master index
                                        match mutant_clone.get_public_index_address(&user_key).await {
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
                                        public_address: public_address.clone(),
                                    }));
                                    entry.abort_handle = None; // Task finished, remove handle
                                    log::info!("PUT streaming task completed successfully: task_id={}, user_key={}", task_id, user_key);

                                    // Auto-index public uploads to colony
                                    if public {
                                        if let Some(ref addr) = public_address {
                                            // Spawn auto-indexing task (don't block the response)
                                            let user_key_clone = user_key.clone();
                                            let addr_clone = addr.clone();
                                            let filename_clone = filename.clone();
                                            tokio::spawn(async move {
                                                auto_index_public_upload(&user_key_clone, filename_clone, file_size, &addr_clone).await;
                                            });
                                        }
                                    }

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
                                    log::error!("PUT streaming task failed: task_id={}, user_key={}, error={}", task_id, user_key, e);
                                    Some(Response::TaskResult(TaskResultResponse {
                                        task_id,
                                        status: TaskStatus::Failed,
                                        result: entry.task.result.clone(),
                                    }))
                                }
                            }
                        } else {
                            log::info!("PUT streaming task was stopped before completion: task_id={}", task_id);
                            entry.abort_handle = None; // Ensure handle is cleared if stopped
                            None // No final result to send if stopped
                        }
                    } else {
                        log::warn!("Task entry removed before PUT streaming completion? task_id={}", task_id);
                        None
                    }
                };

                if let Some(response) = final_response {
                    let _ = update_tx_clone.send(response);
                }

                // Release the key when the operation completes
                release_key(&active_keys_clone, &user_key_clone).await;
                log::debug!("Released key '{}' after PUT streaming operation", user_key_clone);
            });

            log::info!("DAEMON: PutData task {} - Streaming assembly complete, PUT operation spawned", task_id);
        }
    } else {
        log::warn!("DAEMON: PutData - No streaming data found for task {}", task_id);
        return update_tx
            .send(Response::Error(ErrorResponse {
                error: format!("No streaming put operation found for task {}", task_id),
                original_request: Some(original_request_str.to_string()),
            }))
            .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)));
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

                // Handle streaming data if enabled
                if stream_data {
                    if let GetEvent::PadData { chunk_index, data } = &event {
                        // Break large pads into smaller chunks for better UI responsiveness
                        const MAX_CHUNK_SIZE: usize = 256 * 1024; // 256KB chunks for streaming
                        let total = total_chunks.load(std::sync::atomic::Ordering::SeqCst);
                        let is_last_pad = *chunk_index == total - 1;

                        log::info!("DAEMON CALLBACK: GET task {} - Processing PadData for chunk {}/{}, data size: {} bytes",
                                  task_id, chunk_index + 1, total, data.len());

                        // Split the data into smaller chunks if it's large
                        let mut offset = 0;
                        let mut sub_chunk_index = 0;

                        while offset < data.len() {
                            let end = std::cmp::min(offset + MAX_CHUNK_SIZE, data.len());
                            let sub_chunk = &data[offset..end];
                            let is_last_sub_chunk = end == data.len();
                            let is_final_chunk = is_last_pad && is_last_sub_chunk;

                            log::info!("DAEMON CALLBACK: GET task {} - Sending GetData sub-chunk {}.{} ({} bytes, is_final={})",
                                      task_id, chunk_index, sub_chunk_index, sub_chunk.len(), is_final_chunk);

                            if let Err(e) = tx.send(Response::GetData(GetDataResponse {
                                task_id,
                                chunk_index: (*chunk_index * 1000 + sub_chunk_index) as u64, // Unique sub-chunk index
                                total_chunks: total as u64,
                                data: sub_chunk.to_vec(),
                                is_last: is_final_chunk,
                            })) {
                                log::error!("DAEMON CALLBACK: GET task {} - Failed to send GetData sub-chunk: {}", task_id, e);
                            } else {
                                log::info!("DAEMON CALLBACK: GET task {} - Successfully sent GetData sub-chunk {}.{}",
                                          task_id, chunk_index, sub_chunk_index);
                            }

                            offset = end;
                            sub_chunk_index += 1;
                        }

                        // For streaming mode, don't send TaskUpdate for PadData events
                        // Only send GetData responses to avoid UI freezing
                        return Ok(true);
                    }
                }

                // Send regular progress update for non-PadData events or when not streaming
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

        // For video files with streaming enabled, try immediate pad streaming for better performance
        if stream_data && (user_key.ends_with(".mp4") || user_key.ends_with(".mkv") || user_key.ends_with(".avi") || user_key.ends_with(".webm")) {
            log::info!("DAEMON: GET task {} - Detected video file, attempting immediate pad streaming for: {}", task_id, user_key);

            let update_tx_for_streaming = update_tx.clone();
            let task_id_for_streaming = task_id;
            let user_key_for_streaming = user_key.clone();
            let mutant_for_streaming = mutant.clone();
            let req_public = req.public;

            // Spawn immediate streaming task
            tokio::spawn(async move {
                let streaming_result = if req_public {
                    // For public videos, try to get the public address and use immediate streaming
                    match ScratchpadAddress::from_hex(&user_key_for_streaming) {
                        Ok(address) => {
                            match mutant_for_streaming.get_public_with_immediate_streaming(&address, None).await {
                                Ok(mut pad_rx) => {
                                    log::info!("DAEMON: GET task {} - Started immediate pad streaming for public video", task_id_for_streaming);

                                    // Forward pads immediately as StreamingPad responses
                                    while let Some((pad_index, pad_data)) = pad_rx.recv().await {
                                        log::debug!("DAEMON: GET task {} - Forwarding pad {} ({} bytes) immediately",
                                                   task_id_for_streaming, pad_index, pad_data.len());

                                        let response = Response::StreamingPad(mutant_protocol::StreamingPadResponse {
                                            task_id: task_id_for_streaming,
                                            pad_index,
                                            total_pads: 0, // We don't know the total yet
                                            data: pad_data,
                                            is_last: false, // We'll determine this later
                                            key: user_key_for_streaming.clone(),
                                        });

                                        if let Err(e) = update_tx_for_streaming.send(response) {
                                            log::error!("DAEMON: GET task {} - Failed to send streaming pad: {}", task_id_for_streaming, e);
                                            break;
                                        }
                                    }

                                    log::info!("DAEMON: GET task {} - Completed immediate pad streaming for public video", task_id_for_streaming);
                                    Ok(())
                                }
                                Err(e) => {
                                    log::warn!("DAEMON: GET task {} - Failed immediate streaming for public video, falling back to regular streaming: {}", task_id_for_streaming, e);
                                    Err(e)
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("DAEMON: GET task {} - Invalid public address, falling back to regular streaming: {}", task_id_for_streaming, e);
                            Err(mutant_lib::error::Error::Internal(format!("Invalid address: {}", e)))
                        }
                    }
                } else {
                    // For private videos, use immediate streaming
                    match mutant_for_streaming.get_with_immediate_streaming(&user_key_for_streaming, None).await {
                        Ok(mut pad_rx) => {
                            log::info!("DAEMON: GET task {} - Started immediate pad streaming for private video", task_id_for_streaming);

                            // Forward pads immediately as StreamingPad responses
                            while let Some((pad_index, pad_data)) = pad_rx.recv().await {
                                log::debug!("DAEMON: GET task {} - Forwarding pad {} ({} bytes) immediately",
                                           task_id_for_streaming, pad_index, pad_data.len());

                                let response = Response::StreamingPad(mutant_protocol::StreamingPadResponse {
                                    task_id: task_id_for_streaming,
                                    pad_index,
                                    total_pads: 0, // We don't know the total yet
                                    data: pad_data,
                                    is_last: false, // We'll determine this later
                                    key: user_key_for_streaming.clone(),
                                });

                                if let Err(e) = update_tx_for_streaming.send(response) {
                                    log::error!("DAEMON: GET task {} - Failed to send streaming pad: {}", task_id_for_streaming, e);
                                    break;
                                }
                            }

                            log::info!("DAEMON: GET task {} - Completed immediate pad streaming for private video", task_id_for_streaming);
                            Ok(())
                        }
                        Err(e) => {
                            log::warn!("DAEMON: GET task {} - Failed immediate streaming for private video, falling back to regular streaming: {}", task_id_for_streaming, e);
                            Err(e)
                        }
                    }
                };

                // Log the result of immediate streaming attempt
                match streaming_result {
                    Ok(_) => log::info!("DAEMON: GET task {} - Immediate streaming completed successfully", task_id_for_streaming),
                    Err(e) => log::warn!("DAEMON: GET task {} - Immediate streaming failed: {}", task_id_for_streaming, e),
                }
            });
        }

        // Check if the key exists first for private keys
        let get_result = if req.public {
            log::info!("DAEMON: GET task {} - Executing public get operation", task_id);

            // First try to parse as direct hex address
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
                Err(_hex_err) => {
                    // Not a direct hex address, try to get the public index address for this user key
                    log::info!("DAEMON: GET task {} - Not a hex address, trying to get public index address for key '{}'", task_id, user_key);
                    match mutant.get_public_index_address(&user_key).await {
                        Ok(public_address_hex) => {
                            log::info!("DAEMON: GET task {} - Got public address for key '{}': {}", task_id, user_key, public_address_hex);
                            // Parse the hex address to ScratchpadAddress
                            match ScratchpadAddress::from_hex(&public_address_hex) {
                                Ok(address) => {
                                    log::info!("DAEMON: GET task {} - Calling get_public with resolved address", task_id);
                                    let result = mutant.get_public(&address, Some(callback), stream_data).await;
                                    match &result {
                                        Ok(_) => log::info!("DAEMON: GET task {} - get_public operation succeeded", task_id),
                                        Err(e) => log::error!("DAEMON: GET task {} - get_public operation failed: {}", task_id, e),
                                    }
                                    result
                                }
                                Err(parse_err) => {
                                    let error_msg = format!("Failed to parse public address '{}' for key '{}': {}", public_address_hex, user_key, parse_err);
                                    log::error!("DAEMON: GET task {} - {}", task_id, error_msg);
                                    Err(mutant_lib::error::Error::Internal(error_msg))
                                }
                            }
                        }
                        Err(index_err) => {
                            let error_msg = format!("Failed to get public index address for key '{}': {}", user_key, index_err);
                            log::error!("DAEMON: GET task {} - {}", task_id, error_msg);
                            Err(mutant_lib::error::Error::Internal(error_msg))
                        }
                    }
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
                                    size: data_bytes.len() as u64,
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

pub(crate) async fn handle_mv(
    req: MvRequest,
    update_tx: UpdateSender,
    mutant: Arc<MutAnt>,
    _active_keys: ActiveKeysMap,
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

    let old_key = req.old_key.clone();
    let new_key = req.new_key.clone();

    log::info!("DAEMON: Received MV request: old_key={}, new_key={}, original_request={}",
               old_key, new_key, original_request_str);

    // Perform the rename operation
    let response = match mutant.mv(&old_key, &new_key).await {
        Ok(_) => {
            log::info!("MV operation successful: '{}' -> '{}'", old_key, new_key);
            Response::MvSuccess(MvSuccessResponse {
                old_key: old_key.clone(),
                new_key: new_key.clone(),
            })
        }
        Err(e) => {
            log::error!("MV operation failed: old_key={}, new_key={}, error={}", old_key, new_key, e);
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
