use std::sync::{Arc, RwLock};

use eframe::egui::{self, Color32, RichText};
use js_sys::Uint8Array;
use mutant_protocol::StorageMode;
use serde::{Deserialize, Serialize};

use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{File, FileReader, Event};
use web_time::{Duration, SystemTime};

use super::Window;
use super::components::progress::detailed_progress;
use super::context;
use super::notifications;
use super::theme::{danger_button, MutantColors, primary_button, success_button};

#[derive(Clone, Serialize, Deserialize)]
pub struct PutWindow {
    // File selection state
    selected_file: Arc<RwLock<Option<String>>>,
    file_size: Arc<RwLock<Option<u64>>>,
    #[serde(skip)] // Skip serializing file data to avoid localStorage quota issues
    file_data: Arc<RwLock<Option<Vec<u8>>>>,


    // Key name input
    key_name: Arc<RwLock<String>>,

    // Temp holder for the file to be uploaded
    #[serde(skip)]
    temp_file_holder: Arc<RwLock<Option<web_sys::File>>>,

    // Configuration options
    public: Arc<RwLock<bool>>,
    storage_mode: Arc<RwLock<StorageMode>>,
    no_verify: Arc<RwLock<bool>>,

    // File reading progress (Phase 1: File-to-Web)
    is_reading_file: Arc<RwLock<bool>>,
    file_read_progress: Arc<RwLock<f32>>,
    file_read_bytes: Arc<RwLock<u64>>,

    // Upload progress (Phase 2: Web-to-Daemon)
    is_uploading_to_daemon: Arc<RwLock<bool>>,
    daemon_upload_progress: Arc<RwLock<f32>>,
    daemon_upload_bytes: Arc<RwLock<u64>>,

    // Network progress tracking (Phase 3: Daemon-to-Network)
    reservation_progress: Arc<RwLock<f32>>,
    upload_progress: Arc<RwLock<f32>>,
    confirmation_progress: Arc<RwLock<f32>>,

    total_chunks: Arc<RwLock<usize>>,
    chunks_to_reserve: Arc<RwLock<usize>>,
    initial_written_count: Arc<RwLock<usize>>,
    initial_confirmed_count: Arc<RwLock<usize>>,

    // Upload state
    is_uploading: Arc<RwLock<bool>>,
    upload_complete: Arc<RwLock<bool>>,
    public_address: Arc<RwLock<Option<String>>>,
    error_message: Arc<RwLock<Option<String>>>,

    // Timing
    start_time: Arc<RwLock<Option<SystemTime>>>,
    elapsed_time: Arc<RwLock<Duration>>,
    last_progress_check: Arc<RwLock<SystemTime>>,

    // Progress tracking ID
    current_put_id: Arc<RwLock<Option<String>>>,

    /// Unique identifier for this window instance to avoid widget ID conflicts
    #[serde(skip)]
    window_id: String,
}

impl Default for PutWindow {
    fn default() -> Self {
        Self {
            selected_file: Arc::new(RwLock::new(None)),
            file_size: Arc::new(RwLock::new(None)),
            file_data: Arc::new(RwLock::new(None)),
            key_name: Arc::new(RwLock::new(String::new())),
            temp_file_holder: Arc::new(RwLock::new(None)),
            public: Arc::new(RwLock::new(false)),
            storage_mode: Arc::new(RwLock::new(StorageMode::Heaviest)),
            no_verify: Arc::new(RwLock::new(false)),

            // File reading progress (Phase 1)
            is_reading_file: Arc::new(RwLock::new(false)),
            file_read_progress: Arc::new(RwLock::new(0.0)),
            file_read_bytes: Arc::new(RwLock::new(0)),

            // Upload progress (Phase 2)
            is_uploading_to_daemon: Arc::new(RwLock::new(false)),
            daemon_upload_progress: Arc::new(RwLock::new(0.0)),
            daemon_upload_bytes: Arc::new(RwLock::new(0)),

            // Network progress (Phase 3)
            reservation_progress: Arc::new(RwLock::new(0.0)),
            upload_progress: Arc::new(RwLock::new(0.0)),
            confirmation_progress: Arc::new(RwLock::new(0.0)),
            total_chunks: Arc::new(RwLock::new(0)),
            chunks_to_reserve: Arc::new(RwLock::new(0)),
            initial_written_count: Arc::new(RwLock::new(0)),
            initial_confirmed_count: Arc::new(RwLock::new(0)),
            is_uploading: Arc::new(RwLock::new(false)),
            upload_complete: Arc::new(RwLock::new(false)),
            public_address: Arc::new(RwLock::new(None)),
            error_message: Arc::new(RwLock::new(None)),
            start_time: Arc::new(RwLock::new(None)),
            elapsed_time: Arc::new(RwLock::new(Duration::from_secs(0))),
            last_progress_check: Arc::new(RwLock::new(SystemTime::now())),
            current_put_id: Arc::new(RwLock::new(None)),
            window_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl Window for PutWindow {
    fn name(&self) -> String {
        "MutAnt Upload".to_string()
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        // Log that we're drawing the window
        log::debug!("Drawing PutWindow");

        // If we're uploading, check the progress before drawing the form
        // This ensures we have the latest progress values when drawing
        if *self.is_uploading.read().unwrap() && !*self.upload_complete.read().unwrap() {
            log::debug!("Upload in progress, checking progress");
            self.check_progress();
        } else {
            log::debug!("Upload not in progress: is_uploading={}, upload_complete={}",
                *self.is_uploading.read().unwrap(),
                *self.upload_complete.read().unwrap());
        }

        // Draw the form with the updated progress values
        self.draw_upload_form(ui);

        // Request a repaint to ensure we update frequently
        // This is crucial for smooth progress bar updates
        // Use a shorter interval (16ms = ~60fps) for smoother updates
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
}

impl PutWindow {
    pub fn new() -> Self {
        Self::default()
    }

    fn check_progress(&self) {
        // Check if we should update the progress based on the timer
        let now = SystemTime::now();
        let last_check = *self.last_progress_check.read().unwrap();

        // Only check progress every 50ms to avoid excessive updates but still be responsive
        let should_check = match now.duration_since(last_check) {
            Ok(duration) => duration.as_millis() >= 50,
            Err(_) => true, // If there's an error, just check anyway
        };

        if !should_check {
            return;
        }

        // Update the last check time
        *self.last_progress_check.write().unwrap() = now;

        // Get the current put ID
        let put_id_opt = self.current_put_id.read().unwrap().clone();

        log::info!("UI: Checking progress for put ID: {:?}", put_id_opt);

        if let Some(put_id) = put_id_opt {
            // Get the context
            let ctx = context::context();

            // Get the progress for this put operation
            if let Some(progress) = ctx.get_put_progress(&put_id) {
                // Read the progress
                let progress_guard = progress.read().unwrap();

                // Check if we have an operation
                if let Some(op) = progress_guard.operation.get("put") {

                    // Update UI based on progress
                    let total_chunks = op.total_pads;
                    let reserved_count = op.nb_reserved;
                    let written_count = op.nb_written;
                    let confirmed_count = op.nb_confirmed;

                    // Store the chunks to reserve
                    *self.chunks_to_reserve.write().unwrap() = op.nb_to_reserve;
                    *self.initial_written_count.write().unwrap() = op.nb_written;
                    *self.initial_confirmed_count.write().unwrap() = op.nb_confirmed;

                    // Calculate progress percentages
                    let reservation_progress = if total_chunks > 0 {
                        reserved_count as f32 / total_chunks as f32
                    } else {
                        0.0
                    };

                    let upload_progress = if total_chunks > 0 {
                        written_count as f32 / total_chunks as f32
                    } else {
                        0.0
                    };

                    let confirmation_progress = if total_chunks > 0 {
                        confirmed_count as f32 / total_chunks as f32
                    } else {
                        0.0
                    };

                    // Update progress bars
                    *self.reservation_progress.write().unwrap() = reservation_progress;
                    *self.upload_progress.write().unwrap() = upload_progress;
                    *self.confirmation_progress.write().unwrap() = confirmation_progress;

                    // Update total chunks
                    *self.total_chunks.write().unwrap() = total_chunks;

                    // Check if operation is complete
                    if confirmed_count == total_chunks && total_chunks > 0 {
                        // Mark upload as complete
                        *self.is_uploading.write().unwrap() = false;
                        *self.upload_complete.write().unwrap() = true;

                        // Set elapsed time
                        if let Some(start) = *self.start_time.read().unwrap() {
                            *self.elapsed_time.write().unwrap() = start.elapsed().unwrap();
                        }

                        // Show notification
                        notifications::info("Upload complete!".to_string());

                        // Refresh the keys list to update the file explorer
                        spawn_local(async {
                            let ctx = context::context();
                            let _ = ctx.list_keys().await;
                        });

                        // If this is a public upload, get the key details to find the public address
                        let public = *self.public.read().unwrap();
                        if public {
                            // Fetch the key details to get the public address
                            spawn_local({
                                let ctx = context::context();
                                let key_name = self.key_name.read().unwrap().clone();
                                let public_address_clone = self.public_address.clone();

                                async move {
                                    // Fetch the keys directly
                                    let keys = ctx.list_keys().await;

                                    // Find our key
                                    for key in keys {
                                        if key.key == key_name && key.is_public {
                                            if let Some(addr) = key.public_address {
                                                *public_address_clone.write().unwrap() = Some(addr);
                                                break;
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }
    }



    // TRUE streaming upload - reads file chunks and sends them directly to daemon
    fn start_streaming_upload_with_file(
        file: File,
        key_name: String,
        filename: String,
        file_size: f64,
        storage_mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
        current_put_id: Arc<RwLock<Option<String>>>,
        error_message: Arc<RwLock<Option<String>>>,
        is_uploading: Arc<RwLock<bool>>,
        is_uploading_to_daemon: Arc<RwLock<bool>>,
        daemon_upload_progress: Arc<RwLock<f32>>,
        daemon_upload_bytes: Arc<RwLock<u64>>,
    ) {


        spawn_local(async move {
            let ctx = context::context();
            let file_size_u64 = file_size as u64;

            // Step 1: Initialize streaming put with daemon
            match ctx.put_streaming_init(&key_name, file_size_u64, &filename, storage_mode, public, no_verify).await {
                Ok(task_id) => {
                    // Store the task ID for progress tracking
                    let task_id_string = task_id.to_string();
                    *current_put_id.write().unwrap() = Some(task_id_string);

                    // Step 2: Stream file chunks directly to daemon
                    const CHUNK_SIZE: u64 = 256 * 1024; // 256KB chunks
                    const DELAY_MS: u64 = 10; // Small delay between chunks

                    let mut offset = 0u64;
                    let mut chunk_index = 0usize;
                    let total_chunks = ((file_size_u64 + CHUNK_SIZE - 1) / CHUNK_SIZE) as usize;



                    while offset < file_size_u64 {
                        let chunk_size = std::cmp::min(CHUNK_SIZE, file_size_u64 - offset);
                        let end_offset = offset + chunk_size;
                        let is_last = end_offset >= file_size_u64;



                        // Create a blob slice for this chunk
                        let blob_slice = match file.slice_with_f64_and_f64(offset as f64, end_offset as f64) {
                            Ok(slice) => slice,
                            Err(e) => {
                                log::error!("Failed to slice file: {:?}", e);
                                *error_message.write().unwrap() = Some("Failed to read file chunk".to_string());
                                *is_uploading.write().unwrap() = false;
                                *is_uploading_to_daemon.write().unwrap() = false;
                                notifications::error("Failed to read file chunk".to_string());
                                return;
                            }
                        };

                        // Read this chunk
                        match Self::read_file_chunk_async(blob_slice).await {
                            Ok(chunk_data) => {
                                // Send this chunk directly to daemon
                                match ctx.put_streaming_chunk(task_id, chunk_index, total_chunks, chunk_data, is_last).await {
                                    Ok(_) => {
                                        offset = end_offset;
                                        chunk_index += 1;

                                        // Update daemon upload progress
                                        let progress = offset as f32 / file_size_u64 as f32;
                                        *daemon_upload_progress.write().unwrap() = progress;
                                        *daemon_upload_bytes.write().unwrap() = offset;

                                        // Small delay to keep UI responsive
                                        if !is_last {
                                            let start = web_time::SystemTime::now();
                                            while start.elapsed().unwrap().as_millis() < DELAY_MS as u128 {
                                                // Busy wait for a short time
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        log::error!("Failed to send chunk {} to daemon: {}", chunk_index, e);
                                        *error_message.write().unwrap() = Some(format!("Upload failed: {}", e));
                                        *is_uploading.write().unwrap() = false;
                                        *is_uploading_to_daemon.write().unwrap() = false;
                                        notifications::error(format!("Upload failed: {}", e));
                                        return;
                                    }
                                }
                            },
                            Err(e) => {
                                log::error!("Error reading file chunk {}: {}", chunk_index, e);
                                *error_message.write().unwrap() = Some(format!("Error reading file: {}", e));
                                *is_uploading.write().unwrap() = false;
                                *is_uploading_to_daemon.write().unwrap() = false;
                                notifications::error(format!("Error reading file: {}", e));
                                return;
                            }
                        }
                    }

                    *is_uploading_to_daemon.write().unwrap() = false;

                    // Final progress update - 100% complete
                    *daemon_upload_progress.write().unwrap() = 1.0;
                    *daemon_upload_bytes.write().unwrap() = file_size_u64;

                    notifications::info("File upload to daemon completed!".to_string());

                    // Refresh the keys list to update the file explorer
                    spawn_local(async {
                        let ctx = context::context();
                        let _ = ctx.list_keys().await;
                    });
                },
                Err(e) => {
                    log::error!("Failed to initialize streaming upload: {}", e);
                    *error_message.write().unwrap() = Some(format!("Failed to start upload: {}", e));
                    *is_uploading.write().unwrap() = false;
                    *is_uploading_to_daemon.write().unwrap() = false;
                    notifications::error(format!("Failed to start upload: {}", e));
                }
            }
        });
    }

    // Helper function to read a single file chunk asynchronously
    async fn read_file_chunk_async(blob: web_sys::Blob) -> Result<Vec<u8>, String> {
        // Create a FileReader for this chunk
        let reader = FileReader::new().map_err(|_| "Failed to create FileReader")?;

        // Create a promise that resolves when the chunk is read
        let (tx, rx) = futures::channel::oneshot::channel();
        let tx = std::rc::Rc::new(std::cell::RefCell::new(Some(tx)));

        // Set up onload handler
        let reader_clone = reader.clone();
        let tx_clone = tx.clone();
        let onload = Closure::once(move |_event: Event| {
            if let Some(sender) = tx_clone.borrow_mut().take() {
                match reader_clone.result() {
                    Ok(array_buffer) => {
                        let array = Uint8Array::new(&array_buffer);
                        let mut data = vec![0; array.length() as usize];
                        array.copy_to(&mut data);
                        let _ = sender.send(Ok(data));
                    },
                    Err(_) => {
                        let _ = sender.send(Err("Failed to read chunk".to_string()));
                    }
                }
            }
        });

        // Set up onerror handler
        let tx_error = tx.clone();
        let onerror = Closure::once(move |_event: Event| {
            if let Some(sender) = tx_error.borrow_mut().take() {
                let _ = sender.send(Err("Error reading chunk".to_string()));
            }
        });

        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
        reader.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        // Start reading the chunk
        reader.read_as_array_buffer(&blob).map_err(|_| "Failed to start reading chunk")?;

        // Don't forget the closures
        onload.forget();
        onerror.forget();

        // Wait for the result
        rx.await.map_err(|_| "Channel error".to_string())?
    }

    fn start_upload(&self) {

        // Get upload parameters
        let key_name = self.key_name.read().unwrap().clone();
        let public = *self.public.read().unwrap();
        let storage_mode = self.storage_mode.read().unwrap().clone();
        let no_verify = *self.no_verify.read().unwrap();

        if key_name.is_empty() {
            notifications::error("Please enter a key name".to_string());
            return;
        }

        // Set upload state
        *self.is_uploading.write().unwrap() = true;
        *self.upload_complete.write().unwrap() = false;
        *self.start_time.write().unwrap() = Some(SystemTime::now());
        *self.error_message.write().unwrap() = None; // Clear previous errors

        // Reset all progress
        *self.is_uploading_to_daemon.write().unwrap() = true; // We're streaming to daemon
        *self.daemon_upload_progress.write().unwrap() = 0.0;
        *self.daemon_upload_bytes.write().unwrap() = 0;
        *self.reservation_progress.write().unwrap() = 0.0;
        *self.upload_progress.write().unwrap() = 0.0;
        *self.confirmation_progress.write().unwrap() = 0.0;
        *self.current_put_id.write().unwrap() = None;


        // Attempt to take the file from the temporary holder
        let file_to_upload_opt = self.temp_file_holder.write().unwrap().take();
        let original_filename_opt = self.selected_file.read().unwrap().clone();
        let original_file_size_opt = *self.file_size.read().unwrap();

        if let (Some(file_to_upload), Some(original_filename), Some(original_file_size_u64)) =
            (file_to_upload_opt, original_filename_opt, original_file_size_opt)
        {
            let original_file_size_f64 = original_file_size_u64 as f64;

            log::info!(
                "Starting streaming upload for key: '{}', original filename: '{}', size: {}",
                key_name,
                original_filename,
                original_file_size_f64
            );

            Self::start_streaming_upload_with_file(
                file_to_upload,
                key_name, // This is the potentially renamed key
                original_filename, // This is the original file name
                original_file_size_f64,
                storage_mode,
                public,
                no_verify,
                self.current_put_id.clone(),
                self.error_message.clone(),
                self.is_uploading.clone(),
                self.is_uploading_to_daemon.clone(),
                self.daemon_upload_progress.clone(),
                self.daemon_upload_bytes.clone(),
            );
        } else {
            log::error!("Cannot start upload: File, filename, or filesize is missing. This indicates a logic error.");
            *self.error_message.write().unwrap() = Some(
                "Critical error: File details missing. Please re-select the file.".to_string(),
            );
            *self.is_uploading.write().unwrap() = false; // Revert upload state
            *self.is_uploading_to_daemon.write().unwrap() = false;
        }
    }

    // The `select_file_and_upload` function has been removed as its logic is
    // now integrated into `start_upload` and the upcoming file selection mechanism (Step 3).

    pub fn trigger_file_selection_and_prepare_window(&self) {
        log::info!("Triggering file selection and preparing PutWindow.");
        self.reset(); // Clear previous state

        // Create file input element
        let document = web_sys::window().unwrap().document().unwrap();
        let input = document
            .create_element("input")
            .unwrap()
            .dyn_into::<web_sys::HtmlInputElement>()
            .unwrap();

        input.set_type("file");
        input.set_accept("*/*"); // Allow all file types

        // Clone Arcs for the closure
        let selected_file_clone = self.selected_file.clone();
        let key_name_clone = self.key_name.clone();
        let file_size_clone = self.file_size.clone();
        let temp_file_holder_clone = self.temp_file_holder.clone();
        let error_message_clone = self.error_message.clone();
        let upload_complete_clone = self.upload_complete.clone();
        let is_uploading_clone = self.is_uploading.clone();
        // Clone input to be used in closure
        let input_clone = input.clone();


        let onchange = Closure::once(move |_event: Event| {
            if let Some(files) = input_clone.files() {
                if files.length() > 0 {
                    if let Some(file_object) = files.get(0) {
                        let file_name_str = file_object.name();
                        let file_size_f64 = file_object.size(); // size is f64 (double)

                        log::info!("File selected: {}, size: {}", file_name_str, file_size_f64);

                        // Populate PutWindow state
                        *selected_file_clone.write().unwrap() = Some(file_name_str.clone());
                        *key_name_clone.write().unwrap() = file_name_str; // Pre-fill key_name
                        *file_size_clone.write().unwrap() = Some(file_size_f64 as u64);
                        *temp_file_holder_clone.write().unwrap() = Some(file_object);

                        // Reset status fields
                        *error_message_clone.write().unwrap() = None;
                        *upload_complete_clone.write().unwrap() = false;
                        *is_uploading_clone.write().unwrap() = false;

                        log::info!("PutWindow state prepared for file upload.");
                        // UI should repaint due to state changes triggering redraw in the main loop
                    } else {
                        log::warn!("File selection event fired, but no file object found.");
                    }
                } else {
                    log::info!("File selection cancelled by user.");
                    // self.reset() was called at the beginning, so state is already clean.
                }
            } else {
                log::warn!("File selection event fired, but no files list found.");
            }
        });

        input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget(); // The closure is now responsible for its own lifetime

        // Trigger file picker
        input.click();
    }

    fn reset(&self) {
        // Reset all state for a new upload
        *self.selected_file.write().unwrap() = None;
        *self.file_size.write().unwrap() = None;
        *self.file_data.write().unwrap() = None; // Potentially clear this if it held large data
        *self.key_name.write().unwrap() = String::new();
        *self.temp_file_holder.write().unwrap() = None; // Clear the held file

        // Reset file reading progress (Phase 1) - This phase might be removed or refactored
        *self.is_reading_file.write().unwrap() = false;
        *self.file_read_progress.write().unwrap() = 0.0;
        *self.file_read_bytes.write().unwrap() = 0;

        // Reset upload progress (Phase 2)
        *self.is_uploading_to_daemon.write().unwrap() = false;
        *self.daemon_upload_progress.write().unwrap() = 0.0;
        *self.daemon_upload_bytes.write().unwrap() = 0;

        // Reset network progress (Phase 3)
        *self.reservation_progress.write().unwrap() = 0.0;
        *self.upload_progress.write().unwrap() = 0.0;
        *self.confirmation_progress.write().unwrap() = 0.0;
        *self.is_uploading.write().unwrap() = false;
        *self.upload_complete.write().unwrap() = false;
        *self.public_address.write().unwrap() = None;
        *self.error_message.write().unwrap() = None;
        *self.start_time.write().unwrap() = None;
        *self.elapsed_time.write().unwrap() = Duration::from_secs(0);
        *self.current_put_id.write().unwrap() = None;
        // Ensure public/storage_mode/no_verify are reset to defaults if needed, or retain user choice
        // For now, they retain their last set values, which is reasonable.
    }

    fn draw_upload_form(&mut self, ui: &mut egui::Ui) {
        let is_uploading = *self.is_uploading.read().unwrap();
        let upload_complete = *self.upload_complete.read().unwrap();
        let is_reading_file = *self.is_reading_file.read().unwrap();

        // Check if we're in any kind of processing state
        let is_processing = is_uploading || is_reading_file;

        if !is_processing && !upload_complete {
            let selected_file_guard = self.selected_file.read().unwrap();
            let file_is_selected = selected_file_guard.is_some();

            if file_is_selected {
                let filename = selected_file_guard.as_deref().unwrap_or_default();
                let filesize = *self.file_size.read().unwrap();

                ui.heading(RichText::new("📤 Upload Details").size(20.0).color(MutantColors::TEXT_PRIMARY));
                ui.add_space(15.0);

                // Display selected file info
                ui.group(|ui| {
                    ui.label(RichText::new("Selected File:").color(MutantColors::TEXT_SECONDARY));
                    ui.add_space(5.0);
                    ui.label(RichText::new(filename).color(MutantColors::ACCENT_BLUE).strong());
                    if let Some(size) = filesize {
                        ui.label(RichText::new(format!("Size: {} bytes", size)).color(MutantColors::TEXT_MUTED));
                    }
                });
                ui.add_space(10.0);

                // Key name input
                ui.group(|ui| {
                    ui.label(RichText::new("Key Name (can be renamed):").color(MutantColors::TEXT_PRIMARY));
                    ui.add_space(5.0);
                    let mut key_name_guard = self.key_name.write().unwrap();
                    ui.text_edit_singleline(&mut *key_name_guard);
                });
                ui.add_space(10.0);

                // Configuration options
                ui.group(|ui| {
                    ui.collapsing(RichText::new("⚙ Upload Options").color(MutantColors::TEXT_SECONDARY), |ui| {
                        ui.add_space(5.0);
                        // Public checkbox
                        ui.horizontal(|ui| {
                            let mut public = self.public.write().unwrap();
                            ui.checkbox(&mut *public, ""); // Label provided by RichText
                            ui.label(RichText::new("Public").color(MutantColors::TEXT_PRIMARY));
                            ui.label(RichText::new("- Make this file publicly accessible").color(MutantColors::TEXT_MUTED));
                        });
                        ui.add_space(5.0);
                        // Storage mode selection
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Storage Mode:").color(MutantColors::TEXT_PRIMARY));
                            ui.add_space(5.0);
                            let mut storage_mode = self.storage_mode.write().unwrap();
                            egui::ComboBox::new(format!("mutant_put_storage_mode_{}", self.window_id), "")
                                .selected_text(RichText::new(format!("{:?}", *storage_mode)).color(MutantColors::ACCENT_BLUE))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut *storage_mode, StorageMode::Light, RichText::new("Light").color(MutantColors::TEXT_PRIMARY));
                                    ui.selectable_value(&mut *storage_mode, StorageMode::Medium, RichText::new("Medium").color(MutantColors::TEXT_PRIMARY));
                                    ui.selectable_value(&mut *storage_mode, StorageMode::Heavy, RichText::new("Heavy").color(MutantColors::TEXT_PRIMARY));
                                    ui.selectable_value(&mut *storage_mode, StorageMode::Heaviest, RichText::new("Heaviest").color(MutantColors::TEXT_PRIMARY));
                                });
                        });
                        ui.add_space(5.0);
                        // No verify checkbox
                        ui.horizontal(|ui| {
                            let mut no_verify = self.no_verify.write().unwrap();
                            ui.checkbox(&mut *no_verify, ""); // Label provided by RichText
                            ui.label(RichText::new("Skip Verification").color(MutantColors::TEXT_PRIMARY));
                            ui.label(RichText::new("- Faster but less safe").color(MutantColors::TEXT_MUTED));
                        });
                        ui.add_space(5.0);
                    });
                });
                ui.add_space(15.0); // Increased space before buttons

                // Show error message if any (before buttons)
                if let Some(error) = &*self.error_message.read().unwrap() {
                    ui.add_space(10.0);
                    ui.group(|ui| {
                        ui.label(RichText::new(format!("❌ Error: {}", error)).color(MutantColors::ERROR));
                    });
                    ui.add_space(5.0); // Space between error and buttons
                }

                // Upload and Cancel buttons
                ui.horizontal(|ui| {
                    let key_name_empty = self.key_name.read().unwrap().is_empty();
                    let upload_button_enabled = file_is_selected && !key_name_empty;

                    if ui.add_enabled(upload_button_enabled, primary_button("🚀 Upload")).clicked() {
                        log::info!("Upload button clicked (pending full implementation)");
                        self.start_upload(); // This will be adapted later
                    }

                    if ui.add(danger_button("❌ Cancel")).clicked() {
                        self.reset();
                        log::info!("Cancel button clicked, state reset.");
                        // Closing window is out of scope for this step
                    }
                });

                if key_name_empty {
                    ui.add_space(5.0);
                    ui.label(RichText::new("⚠ Please enter a key name for the upload.").color(MutantColors::WARNING));
                }


            } else {
                // No file selected yet
                ui.heading(RichText::new("📤 Upload File").size(20.0).color(MutantColors::TEXT_PRIMARY));
                ui.add_space(15.0);
                ui.label(RichText::new("Please select a file via the main menu to begin.")
                    .color(MutantColors::TEXT_MUTED)
                    .italics());
                // Potentially add a button here to trigger file selection if the flow changes
                // For now, this matches the requirement that the window might open empty.
            }
        } else if is_reading_file {
            // File reading progress section
            ui.heading(RichText::new("⏳ Reading File...").size(20.0).color(MutantColors::TEXT_PRIMARY));
            ui.add_space(15.0);

            ui.group(|ui| {
                let file_name = self.selected_file.read().unwrap().clone().unwrap_or_default();
                ui.label(RichText::new(format!("File: {}", file_name)).color(MutantColors::TEXT_SECONDARY));
                ui.add_space(10.0);

                // File reading progress
                let file_read_progress = *self.file_read_progress.read().unwrap();
                let file_read_bytes = *self.file_read_bytes.read().unwrap();
                let file_size = self.file_size.read().unwrap().unwrap_or(0);

                ui.label(RichText::new("Loading file into memory:").color(MutantColors::TEXT_MUTED));
                ui.add_space(5.0);
                ui.add(detailed_progress(
                    file_read_progress,
                    file_read_bytes as usize,
                    file_size as usize,
                    RichText::new("Reading...").color(MutantColors::TEXT_SECONDARY).strong().to_string()
                ));
                ui.add_space(10.0);
                ui.label(RichText::new("Please wait while the file is being loaded...").color(MutantColors::TEXT_MUTED).italics());
            });

        } else if is_uploading {
            // Progress section
            ui.heading(RichText::new("🚀 Uploading...").size(20.0).color(MutantColors::TEXT_PRIMARY));
            ui.add_space(15.0);

            ui.group(|ui| {
                let key_name_display = self.key_name.read().unwrap();
                ui.label(RichText::new(format!("Key: {}", *key_name_display)).color(MutantColors::TEXT_SECONDARY));

                ui.add_space(10.0);

                // Check if we're in daemon upload phase
                let is_uploading_to_daemon = *self.is_uploading_to_daemon.read().unwrap();
                if is_uploading_to_daemon {
                    let daemon_upload_progress = *self.daemon_upload_progress.read().unwrap();
                    let daemon_upload_bytes = *self.daemon_upload_bytes.read().unwrap();
                    let file_size_val = self.file_size.read().unwrap().unwrap_or(0);

                    ui.label(RichText::new("Sending to daemon:").color(MutantColors::TEXT_MUTED));
                    ui.add_space(5.0);
                    ui.add(detailed_progress(
                        daemon_upload_progress,
                        daemon_upload_bytes as usize,
                        file_size_val as usize,
                        RichText::new("Uploading...").color(MutantColors::TEXT_SECONDARY).strong().to_string()
                    ));
                    ui.add_space(10.0);
                }

                // Calculate elapsed time
                let elapsed = if let Some(start_time_val) = *self.start_time.read().unwrap() {
                    start_time_val.elapsed().unwrap_or_default()
                } else {
                    *self.elapsed_time.read().unwrap()
                };
                let elapsed_str = format_elapsed_time(elapsed);

                let total_chunks_val = *self.total_chunks.read().unwrap();

                let reservation_progress_val = *self.reservation_progress.read().unwrap();
                let upload_progress_val = *self.upload_progress.read().unwrap();
                let confirmation_progress_val = *self.confirmation_progress.read().unwrap();

                let reserved_count = (reservation_progress_val * total_chunks_val as f32) as usize;
                let uploaded_count = (upload_progress_val * total_chunks_val as f32) as usize;
                let confirmed_count = (confirmation_progress_val * total_chunks_val as f32) as usize;

                // Reservation progress bar
                ui.label(RichText::new("Reserving pads:").color(MutantColors::TEXT_MUTED));
                ui.add_space(5.0);
                ui.add(detailed_progress(reservation_progress_val, reserved_count, total_chunks_val, elapsed_str.clone()));
                ui.add_space(10.0);

                // Upload progress bar
                ui.label(RichText::new("Uploading pads:").color(MutantColors::TEXT_MUTED));
                ui.add_space(5.0);
                ui.add(detailed_progress(upload_progress_val, uploaded_count, total_chunks_val, elapsed_str.clone()));
                ui.add_space(10.0);

                // Confirmation progress bar
                ui.label(RichText::new("Confirming pads:").color(MutantColors::TEXT_MUTED));
                ui.add_space(5.0);
                ui.add(detailed_progress(confirmation_progress_val, confirmed_count, total_chunks_val, elapsed_str));
                ui.add_space(10.0);

                // Cancel button
                if ui.button(RichText::new("❌ Cancel Upload").color(MutantColors::ERROR)).clicked() {
                    // TODO: Implement full cancellation logic (this only stops UI tracking)
                    *self.is_uploading.write().unwrap() = false;
                    notifications::warning("Upload cancelled by user.".to_string());
                }
            });
        } else if upload_complete {
            // Upload complete section
            ui.heading(RichText::new("✅ Upload Complete!").size(20.0).color(MutantColors::SUCCESS));
            ui.add_space(15.0);

            ui.group(|ui| {
                let key_name_val = self.key_name.read().unwrap();
                ui.label(RichText::new(format!("Successfully uploaded: {}", *key_name_val)).color(MutantColors::TEXT_PRIMARY));
                ui.add_space(5.0);

                // Show elapsed time
                let elapsed_val = *self.elapsed_time.read().unwrap();
                ui.label(RichText::new(format!("Time taken: {}", format_elapsed_time(elapsed_val))).color(MutantColors::TEXT_MUTED));

                // Show public address if available
                if let Some(address) = &*self.public_address.read().unwrap() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Public index address:").color(MutantColors::TEXT_SECONDARY));
                    ui.add_space(5.0);
                    let mut addr_clone = address.clone(); // text_edit_singleline needs mutable
                    ui.text_edit_singleline(&mut addr_clone);
                }
            });

            ui.add_space(15.0);
            ui.horizontal(|ui| {
                if ui.add(success_button("📤 Upload Another File")).clicked() {
                    self.reset();
                    self.trigger_file_selection_and_prepare_window();
                }
            });
        }
    }
}

// Helper function to format elapsed time (similar to CLI implementation)
fn format_elapsed_time(duration: Duration) -> String {
    let total_seconds = duration.as_secs();

    if total_seconds < 60 {
        format!("{}s", total_seconds)
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{}m {}s", minutes, seconds)
    } else {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        format!("{}h {}m {}s", hours, minutes, seconds)
    }
}
                            .selected_text(RichText::new(format!("{:?}", *storage_mode)).color(MutantColors::ACCENT_BLUE))
                            .show_ui(ui, |ui| {
                            ui.selectable_value(&mut *storage_mode, StorageMode::Light, RichText::new("Light").color(MutantColors::TEXT_PRIMARY));
                            ui.selectable_value(&mut *storage_mode, StorageMode::Medium, RichText::new("Medium").color(MutantColors::TEXT_PRIMARY));
                            ui.selectable_value(&mut *storage_mode, StorageMode::Heavy, RichText::new("Heavy").color(MutantColors::TEXT_PRIMARY));
                            ui.selectable_value(&mut *storage_mode, StorageMode::Heaviest, RichText::new("Heaviest").color(MutantColors::TEXT_PRIMARY));
                            });
                    });
                ui.add_space(5.0);
                    // No verify checkbox
                    ui.horizontal(|ui| {
                        let mut no_verify = self.no_verify.write().unwrap();
                    ui.checkbox(&mut *no_verify, ""); // Label provided by RichText
                    ui.label(RichText::new("Skip Verification").color(MutantColors::TEXT_PRIMARY));
                    ui.label(RichText::new("- Faster but less safe").color(MutantColors::TEXT_MUTED));
                    });
                ui.add_space(5.0);
                });
        });
        ui.add_space(15.0); // Increased space before buttons

        // Show error message if any (before buttons)
        if let Some(error) = &*self.error_message.read().unwrap() {
                ui.add_space(10.0);
            ui.group(|ui| {
                ui.label(RichText::new(format!("❌ Error: {}", error)).color(MutantColors::ERROR));
            });
            ui.add_space(5.0); // Space between error and buttons
        }

        // Upload and Cancel buttons
        ui.horizontal(|ui| {
            let key_name_empty = self.key_name.read().unwrap().is_empty();
            let upload_button_enabled = file_is_selected && !key_name_empty;

            if ui.add_enabled(upload_button_enabled, primary_button("🚀 Upload")).clicked() {
                log::info!("Upload button clicked (pending full implementation)");
                self.start_upload(); // This will be adapted later
                }

            if ui.add(danger_button("❌ Cancel")).clicked() {
                self.reset();
                log::info!("Cancel button clicked, state reset.");
                // Closing window is out of scope for this step
            }
        });

        if key_name_empty {
            ui.add_space(5.0);
            ui.label(RichText::new("⚠ Please enter a key name for the upload.").color(MutantColors::WARNING));
        }


    } else {
        // No file selected yet
        ui.heading(RichText::new("📤 Upload File").size(20.0).color(MutantColors::TEXT_PRIMARY));
        ui.add_space(15.0);
        ui.label(RichText::new("Please select a file via the main menu to begin.")
            .color(MutantColors::TEXT_MUTED)
            .italics());
        // Potentially add a button here to trigger file selection if the flow changes
        // For now, this matches the requirement that the window might open empty.
    }
} else if is_reading_file {
    // File reading progress section
    ui.heading(RichText::new("⏳ Reading File...").size(20.0).color(MutantColors::TEXT_PRIMARY));
    ui.add_space(15.0);

    ui.group(|ui| {
        let file_name = self.selected_file.read().unwrap().clone().unwrap_or_default();
        ui.label(RichText::new(format!("File: {}", file_name)).color(MutantColors::TEXT_SECONDARY));
            ui.add_space(10.0);

            // File reading progress
            let file_read_progress = *self.file_read_progress.read().unwrap();
            let file_read_bytes = *self.file_read_bytes.read().unwrap();
            let file_size = self.file_size.read().unwrap().unwrap_or(0);

        ui.label(RichText::new("Loading file into memory:").color(MutantColors::TEXT_MUTED));
        ui.add_space(5.0);
            ui.add(detailed_progress(
                file_read_progress,
                file_read_bytes as usize,
                file_size as usize,
            RichText::new("Reading...").color(MutantColors::TEXT_SECONDARY).strong().to_string()
            ));
            ui.add_space(10.0);
        ui.label(RichText::new("Please wait while the file is being loaded...").color(MutantColors::TEXT_MUTED).italics());
    });

} else if is_uploading {
    // Progress section
    ui.heading(RichText::new("🚀 Uploading...").size(20.0).color(MutantColors::TEXT_PRIMARY));
    ui.add_space(15.0);

    ui.group(|ui| {
        let key_name_display = self.key_name.read().unwrap();
        ui.label(RichText::new(format!("Key: {}", *key_name_display)).color(MutantColors::TEXT_SECONDARY));

        ui.add_space(10.0);

            // Check if we're in daemon upload phase
            let is_uploading_to_daemon = *self.is_uploading_to_daemon.read().unwrap();
            if is_uploading_to_daemon {
                let daemon_upload_progress = *self.daemon_upload_progress.read().unwrap();
                let daemon_upload_bytes = *self.daemon_upload_bytes.read().unwrap();
            let file_size_val = self.file_size.read().unwrap().unwrap_or(0);

            ui.label(RichText::new("Sending to daemon:").color(MutantColors::TEXT_MUTED));
            ui.add_space(5.0);
                ui.add(detailed_progress(
                    daemon_upload_progress,
                    daemon_upload_bytes as usize,
                file_size_val as usize,
                RichText::new("Uploading...").color(MutantColors::TEXT_SECONDARY).strong().to_string()
                ));
            ui.add_space(10.0);
            }

            // Calculate elapsed time
        let elapsed = if let Some(start_time_val) = *self.start_time.read().unwrap() {
            start_time_val.elapsed().unwrap_or_default()
            } else {
                *self.elapsed_time.read().unwrap()
            };
            let elapsed_str = format_elapsed_time(elapsed);

        let total_chunks_val = *self.total_chunks.read().unwrap();

        let reservation_progress_val = *self.reservation_progress.read().unwrap();
        let upload_progress_val = *self.upload_progress.read().unwrap();
        let confirmation_progress_val = *self.confirmation_progress.read().unwrap();

        let reserved_count = (reservation_progress_val * total_chunks_val as f32) as usize;
        let uploaded_count = (upload_progress_val * total_chunks_val as f32) as usize;
        let confirmed_count = (confirmation_progress_val * total_chunks_val as f32) as usize;

            // Reservation progress bar
        ui.label(RichText::new("Reserving pads:").color(MutantColors::TEXT_MUTED));
            ui.add_space(5.0);
        ui.add(detailed_progress(reservation_progress_val, reserved_count, total_chunks_val, elapsed_str.clone()));
        ui.add_space(10.0);

            // Upload progress bar
        ui.label(RichText::new("Uploading pads:").color(MutantColors::TEXT_MUTED));
            ui.add_space(5.0);
        ui.add(detailed_progress(upload_progress_val, uploaded_count, total_chunks_val, elapsed_str.clone()));
        ui.add_space(10.0);

            // Confirmation progress bar
        ui.label(RichText::new("Confirming pads:").color(MutantColors::TEXT_MUTED));
        ui.add_space(5.0);
        ui.add(detailed_progress(confirmation_progress_val, confirmed_count, total_chunks_val, elapsed_str));
        ui.add_space(10.0);

            // Cancel button
        if ui.button(RichText::new("❌ Cancel Upload").color(MutantColors::ERROR)).clicked() {
            // TODO: Implement full cancellation logic (this only stops UI tracking)
                *self.is_uploading.write().unwrap() = false;
            notifications::warning("Upload cancelled by user.".to_string());
            }
    });
} else if upload_complete {
    // Upload complete section
    ui.heading(RichText::new("✅ Upload Complete!").size(20.0).color(MutantColors::SUCCESS));
    ui.add_space(15.0);

    ui.group(|ui| {
        let key_name_val = self.key_name.read().unwrap();
        ui.label(RichText::new(format!("Successfully uploaded: {}", *key_name_val)).color(MutantColors::TEXT_PRIMARY));
        ui.add_space(5.0);

            // Show elapsed time
        let elapsed_val = *self.elapsed_time.read().unwrap();
        ui.label(RichText::new(format!("Time taken: {}", format_elapsed_time(elapsed_val))).color(MutantColors::TEXT_MUTED));

            // Show public address if available
            if let Some(address) = &*self.public_address.read().unwrap() {
            ui.add_space(10.0);
            ui.label(RichText::new("Public index address:").color(MutantColors::TEXT_SECONDARY));
                ui.add_space(5.0);
            let mut addr_clone = address.clone(); // text_edit_singleline needs mutable
            ui.text_edit_singleline(&mut addr_clone);
            }
    });

    ui.add_space(15.0);
            ui.horizontal(|ui| {
                if ui.add(success_button("📤 Upload Another File")).clicked() {
                    self.reset();
                    self.trigger_file_selection_and_prepare_window();
                }
            });
        }
    }
}

// Helper function to format elapsed time (similar to CLI implementation)
fn format_elapsed_time(duration: Duration) -> String {
    let total_seconds = duration.as_secs();

    if total_seconds < 60 {
        format!("{}s", total_seconds)
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{}m {}s", minutes, seconds)
    } else {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        format!("{}h {}m {}s", hours, minutes, seconds)
    }
}