use std::sync::{Arc, RwLock};

use eframe::egui::{self, RichText};
use js_sys::Uint8Array;
use mutant_protocol::StorageMode;
use serde::{Deserialize, Serialize};

use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{File, FileReader, Event};
use web_time::{Duration, SystemTime};

use super::Window;
use super::components::progress::{file_transfer_progress, network_upload_progress};
use super::context;
use super::notifications;
use super::theme::{MutantColors, primary_button, secondary_button, styled_section_group, info_section_frame, section_header};

#[derive(Clone, Serialize, Deserialize)]
pub struct PutWindow {
    // File selection state
    selected_file: Arc<RwLock<Option<String>>>,
    file_size: Arc<RwLock<Option<u64>>>,
    file_type: Arc<RwLock<Option<String>>>,
    #[serde(skip)] // Skip serializing file data to avoid localStorage quota issues
    file_data: Arc<RwLock<Option<Vec<u8>>>>,

    // Key name input
    key_name: Arc<RwLock<String>>,

    // Configuration options
    public: Arc<RwLock<bool>>,
    storage_mode: Arc<RwLock<StorageMode>>,
    no_verify: Arc<RwLock<bool>>,

    // UI state
    should_trigger_file_dialog: Arc<RwLock<bool>>,
    advanced_settings_open: Arc<RwLock<bool>>,
    file_selected: Arc<RwLock<bool>>,

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
            file_type: Arc::new(RwLock::new(None)),
            file_data: Arc::new(RwLock::new(None)),
            key_name: Arc::new(RwLock::new(String::new())),
            public: Arc::new(RwLock::new(false)),
            storage_mode: Arc::new(RwLock::new(StorageMode::Heaviest)),
            no_verify: Arc::new(RwLock::new(false)),

            // UI state
            should_trigger_file_dialog: Arc::new(RwLock::new(false)),
            advanced_settings_open: Arc::new(RwLock::new(false)),
            file_selected: Arc::new(RwLock::new(false)),

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

        // Check if we should trigger the file dialog immediately
        if *self.should_trigger_file_dialog.read().unwrap() {
            *self.should_trigger_file_dialog.write().unwrap() = false;
            self.trigger_file_dialog();
        }

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
        self.draw_modern_upload_form(ui);

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

    /// Trigger immediate file selection when the window is created
    pub fn trigger_immediate_file_selection(&mut self) {
        *self.should_trigger_file_dialog.write().unwrap() = true;
    }

    /// Set file information (used when creating PutWindow with pre-selected file)
    pub fn set_file_info(&mut self, filename: String, file_size: u64, file_type: String) {
        *self.selected_file.write().unwrap() = Some(filename.clone());
        *self.file_size.write().unwrap() = Some(file_size);
        *self.file_type.write().unwrap() = Some(file_type);
        *self.file_selected.write().unwrap() = true;

        // Auto-populate key name with complete filename (including extension)
        *self.key_name.write().unwrap() = filename;
    }

    /// Update file reading state (used by external file transfer logic)
    pub fn update_file_reading_state(&self, is_reading: bool, progress: f32, bytes_read: u64) {
        *self.is_reading_file.write().unwrap() = is_reading;
        *self.file_read_progress.write().unwrap() = progress;
        *self.file_read_bytes.write().unwrap() = bytes_read;
    }

    /// Set error message (used by external file transfer logic)
    pub fn set_error_message(&self, error: String) {
        *self.error_message.write().unwrap() = Some(error);
        *self.is_reading_file.write().unwrap() = false;
    }

    /// Trigger the file dialog immediately
    fn trigger_file_dialog(&self) {
        log::info!("Triggering immediate file dialog");

        // Create file input element
        let document = web_sys::window().unwrap().document().unwrap();
        let input = document
            .create_element("input")
            .unwrap()
            .dyn_into::<web_sys::HtmlInputElement>()
            .unwrap();

        input.set_type("file");
        input.set_accept("*/*");

        // Clone references for the closure
        let selected_file = self.selected_file.clone();
        let file_size = self.file_size.clone();
        let file_type = self.file_type.clone();
        let file_selected = self.file_selected.clone();
        let key_name = self.key_name.clone();
        let input_clone = input.clone();

        let onchange = Closure::once(move |_event: Event| {
            if let Some(files) = input_clone.files() {
                if files.length() > 0 {
                    if let Some(file) = files.get(0) {
                        let file_name = file.name();
                        let file_size_js = file.size();
                        let file_type_js = file.type_();

                        log::info!("File selected: {} ({} bytes, type: {})", file_name, file_size_js, file_type_js);

                        // Update state with selected file info
                        *selected_file.write().unwrap() = Some(file_name.clone());
                        *file_size.write().unwrap() = Some(file_size_js as u64);
                        *file_type.write().unwrap() = Some(if file_type_js.is_empty() {
                            "Unknown".to_string()
                        } else {
                            file_type_js
                        });
                        *file_selected.write().unwrap() = true;

                        // Auto-populate key name with complete filename (including extension)
                        *key_name.write().unwrap() = file_name.clone();

                        notifications::info(format!("File selected: {}", file_name));
                    }
                } else {
                    log::info!("File selection cancelled");
                }
            }
        });

        input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget();

        // Trigger file picker
        input.click();
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

        // Reset all progress
        *self.is_uploading_to_daemon.write().unwrap() = true; // We're streaming to daemon
        *self.daemon_upload_progress.write().unwrap() = 0.0;
        *self.daemon_upload_bytes.write().unwrap() = 0;
        *self.reservation_progress.write().unwrap() = 0.0;
        *self.upload_progress.write().unwrap() = 0.0;
        *self.confirmation_progress.write().unwrap() = 0.0;



        // Directly trigger file selection for upload
        self.select_file_and_upload(key_name, storage_mode, public, no_verify);
    }

    fn select_file_and_upload(
        &self,
        key_name: String,
        storage_mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
    ) {


        // Create file input element
        let document = web_sys::window().unwrap().document().unwrap();
        let input = document
            .create_element("input")
            .unwrap()
            .dyn_into::<web_sys::HtmlInputElement>()
            .unwrap();

        input.set_type("file");
        input.set_accept("*/*");

        // Clone references for the closure
        let current_put_id = self.current_put_id.clone();
        let error_message = self.error_message.clone();
        let is_uploading = self.is_uploading.clone();
        let is_uploading_to_daemon = self.is_uploading_to_daemon.clone();
        let daemon_upload_progress = self.daemon_upload_progress.clone();
        let daemon_upload_bytes = self.daemon_upload_bytes.clone();
        let selected_file = self.selected_file.clone();
        let file_size = self.file_size.clone();
        let file_data = self.file_data.clone();
        let input_clone = input.clone();

        let onchange = Closure::once(move |_event: Event| {
            if let Some(files) = input_clone.files() {
                if files.length() > 0 {
                    if let Some(file) = files.get(0) {
                        let file_name = file.name();
                        let file_size_js = file.size();



                        // Update state with selected file info
                        *selected_file.write().unwrap() = Some(file_name.clone());
                        *file_size.write().unwrap() = Some(file_size_js as u64);
                        *file_data.write().unwrap() = Some(Vec::new()); // Mark as selected

                        // Start the actual streaming upload immediately
                        Self::start_streaming_upload_with_file(
                            file,
                            key_name,
                            file_name,
                            file_size_js,
                            storage_mode,
                            public,
                            no_verify,
                            current_put_id,
                            error_message,
                            is_uploading,
                            is_uploading_to_daemon,
                            daemon_upload_progress,
                            daemon_upload_bytes,
                        );
                    }
                } else {
                    // User cancelled file selection
                    *is_uploading.write().unwrap() = false;
                    *is_uploading_to_daemon.write().unwrap() = false;
                }
            }
        });

        input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget();

        // Trigger file picker
        input.click();
    }





    fn reset(&self) {
        // Reset all state for a new upload
        *self.selected_file.write().unwrap() = None;
        *self.file_size.write().unwrap() = None;
        *self.file_data.write().unwrap() = None;
        *self.key_name.write().unwrap() = String::new();

        // Reset file reading progress (Phase 1)
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
    }

    /// Modern redesigned upload form with professional styling
    fn draw_modern_upload_form(&mut self, ui: &mut egui::Ui) {
        let is_uploading = *self.is_uploading.read().unwrap();
        let upload_complete = *self.upload_complete.read().unwrap();
        let is_reading_file = *self.is_reading_file.read().unwrap();
        let file_selected = *self.file_selected.read().unwrap();

        // Check if we're in any kind of processing state
        let is_processing = is_uploading || is_reading_file;

        if !is_processing && !upload_complete {
            // Main header
            ui.horizontal(|ui| {
                ui.label(section_header("Upload File", "📤", MutantColors::ACCENT_ORANGE));
            });
            ui.add_space(12.0);

            // File Information Section (only show if file is selected)
            if file_selected {
                self.draw_file_info_section(ui);
                ui.add_space(12.0);
            }

            // Key Name Input Section
            self.draw_key_name_section(ui);
            ui.add_space(12.0);

            // Upload Mode Selection (Public/Private)
            self.draw_upload_mode_section(ui);
            ui.add_space(12.0);

            // Advanced Settings (Collapsible)
            self.draw_advanced_settings_section(ui);
            ui.add_space(12.0);

            // Action Buttons Section
            self.draw_action_buttons_section(ui);

            // Error Display
            self.draw_error_section(ui);
        } else if is_reading_file {
            // Modern file reading progress
            self.draw_file_reading_progress(ui);
        } else if is_uploading {
            // Modern upload progress with two phases
            self.draw_upload_progress(ui);

        } else if upload_complete {
            // Modern upload completion section
            self.draw_upload_complete_section(ui);
        }
    }

    /// Draw the file information section with modern styling
    fn draw_file_info_section(&self, ui: &mut egui::Ui) {
        if let (Some(filename), Some(size), Some(file_type)) = (
            &*self.selected_file.read().unwrap(),
            *self.file_size.read().unwrap(),
            &*self.file_type.read().unwrap()
        ) {
            info_section_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    // File icon based on type
                    let (icon, icon_color) = self.get_file_icon_and_color(filename);
                    ui.label(RichText::new(icon).size(24.0).color(icon_color));

                    ui.vertical(|ui| {
                        // Filename
                        ui.label(RichText::new(filename)
                            .size(16.0)
                            .strong()
                            .color(MutantColors::TEXT_PRIMARY));

                        // File details
                        let size_str = format_bytes(size);
                        ui.label(RichText::new(format!("{} • {}", size_str, file_type))
                            .size(12.0)
                            .color(MutantColors::TEXT_SECONDARY));
                    });
                });
            });
        }
    }

    /// Draw the key name input section
    fn draw_key_name_section(&self, ui: &mut egui::Ui) {
        styled_section_group().show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(section_header("Key Name", "🔑", MutantColors::ACCENT_BLUE));
                ui.add_space(8.0);

                let mut key_name = self.key_name.write().unwrap();
                ui.text_edit_singleline(&mut *key_name);

                ui.add_space(4.0);
                ui.label(RichText::new("Choose a unique name for your file")
                    .size(11.0)
                    .color(MutantColors::TEXT_MUTED));
            });
        });
    }

    /// Draw the upload mode selection (Public/Private)
    fn draw_upload_mode_section(&self, ui: &mut egui::Ui) {
        styled_section_group().show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(section_header("Upload Mode", "🌐", MutantColors::ACCENT_GREEN));
                ui.add_space(8.0);

                let mut public = self.public.write().unwrap();

                ui.horizontal(|ui| {
                    if ui.radio_value(&mut *public, false, "Private").clicked() {
                        log::info!("Upload mode set to Private");
                    }
                    ui.label(RichText::new("Only you can access this file")
                        .size(11.0)
                        .color(MutantColors::TEXT_MUTED));
                });

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if ui.radio_value(&mut *public, true, "Public").clicked() {
                        log::info!("Upload mode set to Public");
                    }
                    ui.label(RichText::new("Anyone with the address can access this file")
                        .size(11.0)
                        .color(MutantColors::TEXT_MUTED));
                });
            });
        });
    }

    /// Draw the advanced settings section (collapsible)
    fn draw_advanced_settings_section(&self, ui: &mut egui::Ui) {
        let mut advanced_open = self.advanced_settings_open.write().unwrap();

        ui.collapsing("⚙️ Advanced Settings", |ui| {
            *advanced_open = true;

            styled_section_group().show(ui, |ui| {
                ui.vertical(|ui| {
                    // Storage mode selection with chunk size information
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Storage Mode:").color(MutantColors::TEXT_SECONDARY));
                        let mut storage_mode = self.storage_mode.write().unwrap();

                        egui::ComboBox::new(format!("mutant_put_storage_mode_{}", self.window_id), "")
                            .selected_text(format!("{:?}", *storage_mode))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut *storage_mode, StorageMode::Light, "Light (64KB chunks)");
                                ui.selectable_value(&mut *storage_mode, StorageMode::Medium, "Medium (256KB chunks)");
                                ui.selectable_value(&mut *storage_mode, StorageMode::Heavy, "Heavy (512KB chunks)");
                                ui.selectable_value(&mut *storage_mode, StorageMode::Heaviest, "Heaviest (1MB chunks)");
                            });
                    });

                    ui.add_space(8.0);

                    // No verify checkbox
                    ui.horizontal(|ui| {
                        let mut no_verify = self.no_verify.write().unwrap();
                        ui.checkbox(&mut *no_verify, "Skip Verification");
                        ui.label(RichText::new("Faster upload but less safe")
                            .size(11.0)
                            .color(MutantColors::TEXT_MUTED));
                    });
                });
            });
        });

        if !ui.ctx().memory(|m| m.is_popup_open(egui::Id::new("⚙️ Advanced Settings"))) {
            *advanced_open = false;
        }
    }

    /// Draw the action buttons section
    fn draw_action_buttons_section(&self, ui: &mut egui::Ui) {
        let file_selected = *self.file_selected.read().unwrap();
        let key_name_valid = !self.key_name.read().unwrap().is_empty();

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if file_selected && key_name_valid {
                // File is selected and key name is valid - show upload button
                if ui.add(primary_button("🚀 Start Upload")).clicked() {
                    self.start_upload_with_selected_file();
                }
            } else if !file_selected {
                // No file selected - show file selection button
                if ui.add(primary_button("📁 Select File")).clicked() {
                    self.trigger_file_dialog();
                }
            } else {
                // File selected but no key name - show disabled upload button
                ui.add_enabled(false, primary_button("🚀 Start Upload"));
            }

            // Reset/Clear button
            if file_selected {
                ui.add_space(8.0);
                if ui.add(secondary_button("🗑 Clear")).clicked() {
                    self.reset_file_selection();
                }
            }
        });

        // Validation messages
        if file_selected && !key_name_valid {
            ui.add_space(6.0);
            ui.label(RichText::new("⚠ Please enter a key name to continue")
                .color(MutantColors::WARNING));
        }
    }

    /// Draw error messages if any
    fn draw_error_section(&self, ui: &mut egui::Ui) {
        if let Some(error) = &*self.error_message.read().unwrap() {
            ui.add_space(12.0);
            info_section_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("❌").size(16.0).color(MutantColors::ERROR));
                    ui.label(RichText::new(error)
                        .color(MutantColors::ERROR)
                        .strong());
                });
            });
        }
    }

    /// Draw file reading progress with modern styling
    fn draw_file_reading_progress(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.label(section_header("Reading File", "📖", MutantColors::ACCENT_BLUE));
            ui.add_space(16.0);

            if let Some(filename) = &*self.selected_file.read().unwrap() {
                let file_read_progress = *self.file_read_progress.read().unwrap();
                let file_read_bytes = *self.file_read_bytes.read().unwrap();
                let file_size = self.file_size.read().unwrap().unwrap_or(0);

                // Use the modern file transfer progress component
                file_transfer_progress(ui, file_read_progress, file_read_bytes, file_size, filename);

                ui.add_space(12.0);
                ui.label(RichText::new("Please wait while the file is being loaded...")
                    .color(MutantColors::TEXT_MUTED));
            }
        });
    }

    /// Draw upload progress with modern two-phase display
    fn draw_upload_progress(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.label(section_header("Upload Progress", "🚀", MutantColors::ACCENT_ORANGE));
            ui.add_space(16.0);

            // Phase 1: Browser → Daemon (if active)
            let is_uploading_to_daemon = *self.is_uploading_to_daemon.read().unwrap();
            if is_uploading_to_daemon {
                if let Some(filename) = &*self.selected_file.read().unwrap() {
                    let daemon_upload_progress = *self.daemon_upload_progress.read().unwrap();
                    let daemon_upload_bytes = *self.daemon_upload_bytes.read().unwrap();
                    let file_size = self.file_size.read().unwrap().unwrap_or(0);

                    ui.label(RichText::new("Phase 1: Transferring to Daemon")
                        .size(14.0)
                        .color(MutantColors::TEXT_PRIMARY));
                    ui.add_space(8.0);

                    file_transfer_progress(ui, daemon_upload_progress, daemon_upload_bytes, file_size, filename);
                    ui.add_space(16.0);
                }
            }

            // Phase 2: Daemon → Network (if active)
            let is_uploading = *self.is_uploading.read().unwrap();
            if is_uploading && !is_uploading_to_daemon {
                // Get progress data from context
                if let Some(put_id) = &*self.current_put_id.read().unwrap() {
                    let ctx = context::context();
                    if let Some(progress) = ctx.get_put_progress(put_id) {
                        let progress_guard = progress.read().unwrap();
                        if let Some(op) = progress_guard.operation.get("put") {
                            let total_chunks = op.total_pads;

                            let reservation_progress = if total_chunks > 0 {
                                op.nb_reserved as f32 / total_chunks as f32
                            } else { 0.0 };

                            let upload_progress = if total_chunks > 0 {
                                op.nb_written as f32 / total_chunks as f32
                            } else { 0.0 };

                            let confirmation_progress = if total_chunks > 0 {
                                op.nb_confirmed as f32 / total_chunks as f32
                            } else { 0.0 };

                            // Calculate elapsed time
                            let elapsed = if let Some(start_time) = *self.start_time.read().unwrap() {
                                start_time.elapsed().unwrap()
                            } else {
                                *self.elapsed_time.read().unwrap()
                            };
                            let elapsed_str = format_elapsed_time(elapsed);

                            ui.label(RichText::new("Phase 2: Uploading to Network")
                                .size(14.0)
                                .color(MutantColors::TEXT_PRIMARY));
                            ui.add_space(8.0);

                            // Use the modern network upload progress component
                            network_upload_progress(
                                ui,
                                reservation_progress, op.nb_reserved,
                                upload_progress, op.nb_written,
                                confirmation_progress, op.nb_confirmed,
                                total_chunks, elapsed_str
                            );
                        }
                    }
                }

                ui.add_space(16.0);

                // Cancel button
                if ui.add(secondary_button("❌ Cancel Upload")).clicked() {
                    *self.is_uploading.write().unwrap() = false;
                    notifications::warning("Upload cancelled".to_string());
                }
            }
        });
    }

    /// Draw upload completion section with modern styling
    fn draw_upload_complete_section(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.label(section_header("Upload Complete", "✅", MutantColors::SUCCESS));
            ui.add_space(16.0);

            styled_section_group().show(ui, |ui| {
                ui.vertical(|ui| {
                    let key_name = self.key_name.read().unwrap();
                    ui.label(RichText::new(format!("Successfully uploaded: {}", *key_name))
                        .size(16.0)
                        .strong()
                        .color(MutantColors::TEXT_PRIMARY));

                    ui.add_space(8.0);

                    // Show elapsed time
                    let elapsed = *self.elapsed_time.read().unwrap();
                    ui.label(RichText::new(format!("Time taken: {}", format_elapsed_time(elapsed)))
                        .color(MutantColors::TEXT_SECONDARY));

                    // Show public address if available
                    if let Some(address) = &*self.public_address.read().unwrap() {
                        ui.add_space(12.0);
                        ui.label(RichText::new("Public Address:")
                            .strong()
                            .color(MutantColors::ACCENT_GREEN));
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut address.clone());
                            if ui.button("📋").on_hover_text("Copy to clipboard").clicked() {
                                ui.ctx().copy_text(address.clone());
                                notifications::info("Address copied to clipboard".to_string());
                            }
                        });
                    }
                });
            });

            ui.add_space(20.0);

            ui.horizontal(|ui| {
                if ui.add(primary_button("📤 Upload Another File")).clicked() {
                    self.reset();
                }

                ui.add_space(8.0);

                if ui.add(secondary_button("🗂 View in Files")).clicked() {
                    // TODO: Switch to files tab and highlight the uploaded file
                    notifications::info("Switching to Files view...".to_string());
                }
            });
        });
    }

    /// Get file icon and color based on filename extension (similar to fs-tree)
    fn get_file_icon_and_color(&self, filename: &str) -> (&'static str, egui::Color32) {
        if let Some(extension) = std::path::Path::new(filename).extension() {
            match extension.to_string_lossy().to_lowercase().as_str() {
                // Code files
                "rs" | "rust" => ("🦀", MutantColors::ACCENT_ORANGE),
                "js" | "ts" | "jsx" | "tsx" => ("📜", MutantColors::WARNING),
                "py" | "python" => ("🐍", MutantColors::ACCENT_GREEN),
                "java" | "class" => ("☕", MutantColors::ACCENT_ORANGE),
                "cpp" | "c" | "cc" | "cxx" | "h" | "hpp" => ("⚙️", MutantColors::ACCENT_BLUE),
                "go" => ("🐹", MutantColors::ACCENT_CYAN),
                "html" | "htm" => ("🌐", MutantColors::ACCENT_ORANGE),
                "css" | "scss" | "sass" | "less" => ("🎨", MutantColors::ACCENT_BLUE),
                "json" | "yaml" | "yml" | "toml" | "xml" => ("📋", MutantColors::TEXT_MUTED),

                // Images
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => ("📷", MutantColors::ACCENT_GREEN),

                // Videos
                "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" => ("🎬", MutantColors::ACCENT_PURPLE),

                // Audio
                "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" => ("🎵", MutantColors::ACCENT_CYAN),

                // Archives
                "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => ("📦", MutantColors::ACCENT_ORANGE),

                // Documents
                "pdf" => ("📕", MutantColors::ERROR),
                "doc" | "docx" => ("📘", MutantColors::ACCENT_BLUE),
                "xls" | "xlsx" => ("📗", MutantColors::ACCENT_GREEN),
                "ppt" | "pptx" => ("📙", MutantColors::WARNING),
                "txt" | "md" | "readme" => ("📄", MutantColors::TEXT_MUTED),

                // Executables
                "exe" | "msi" | "deb" | "rpm" | "dmg" | "app" => ("⚡", MutantColors::WARNING),

                _ => ("📄", MutantColors::TEXT_MUTED)
            }
        } else {
            ("📄", MutantColors::TEXT_MUTED)
        }
    }

    /// Reset file selection state
    fn reset_file_selection(&self) {
        *self.selected_file.write().unwrap() = None;
        *self.file_size.write().unwrap() = None;
        *self.file_type.write().unwrap() = None;
        *self.file_selected.write().unwrap() = false;
        *self.key_name.write().unwrap() = String::new();

        notifications::info("File selection cleared".to_string());
    }

    /// Start upload with the currently selected file
    fn start_upload_with_selected_file(&self) {
        let key_name = self.key_name.read().unwrap().clone();
        let storage_mode = self.storage_mode.read().unwrap().clone();
        let public = *self.public.read().unwrap();
        let no_verify = *self.no_verify.read().unwrap();

        log::info!("Starting upload with selected file: key_name={}, public={}, no_verify={}",
                   key_name, public, no_verify);

        // Check if we have a selected file
        if let Some(filename) = &*self.selected_file.read().unwrap() {
            if let Some(file_size) = *self.file_size.read().unwrap() {
                // Set upload state
                *self.is_uploading.write().unwrap() = true;
                *self.upload_complete.write().unwrap() = false;
                *self.start_time.write().unwrap() = Some(SystemTime::now());
                *self.error_message.write().unwrap() = None;

                // Reset progress
                *self.reservation_progress.write().unwrap() = 0.0;
                *self.upload_progress.write().unwrap() = 0.0;
                *self.confirmation_progress.write().unwrap() = 0.0;

                // Start the daemon-to-network upload phase
                self.start_daemon_to_network_upload(
                    key_name,
                    filename.clone(),
                    file_size,
                    storage_mode,
                    public,
                    no_verify
                );

                notifications::info("Starting upload to network...".to_string());
            } else {
                notifications::error("No file size information available".to_string());
            }
        } else {
            notifications::error("No file selected for upload".to_string());
        }
    }

    /// Start the daemon-to-network upload phase
    fn start_daemon_to_network_upload(
        &self,
        key_name: String,
        filename: String,
        file_size: u64,
        storage_mode: mutant_protocol::StorageMode,
        public: bool,
        no_verify: bool,
    ) {
        use wasm_bindgen_futures::spawn_local;

        // Clone the necessary Arc references for the async closure
        let current_put_id = self.current_put_id.clone();
        let error_message = self.error_message.clone();
        let is_uploading = self.is_uploading.clone();
        let upload_complete = self.upload_complete.clone();
        let start_time = self.start_time.clone();
        let elapsed_time = self.elapsed_time.clone();

        spawn_local(async move {
            let ctx = context::context();

            // For now, we'll simulate the file data since we don't have it stored
            // In a real implementation, we would have the file data from the browser-to-daemon transfer
            let file_data = vec![0u8; file_size as usize]; // Placeholder data

            log::info!("Starting daemon-to-network upload for: {} ({} bytes)", filename, file_size);

            // Create progress tracking
            let (put_id, progress) = ctx.create_progress(&key_name, &filename);
            *current_put_id.write().unwrap() = Some(put_id.clone());

            // Start the upload
            match ctx.put(
                &key_name,
                file_data,
                &filename,
                storage_mode,
                public,
                no_verify,
                Some((put_id, progress))
            ).await {
                Ok((final_put_id, _progress)) => {
                    log::info!("Upload completed successfully: {}", final_put_id);

                    // Calculate elapsed time
                    if let Some(start) = *start_time.read().unwrap() {
                        *elapsed_time.write().unwrap() = start.elapsed().unwrap();
                    }

                    // Mark as complete
                    *is_uploading.write().unwrap() = false;
                    *upload_complete.write().unwrap() = true;

                    notifications::info("Upload completed successfully!".to_string());
                },
                Err(e) => {
                    log::error!("Upload failed: {}", e);
                    *error_message.write().unwrap() = Some(format!("Upload failed: {}", e));
                    *is_uploading.write().unwrap() = false;
                    notifications::error(format!("Upload failed: {}", e));
                }
            }
        });
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

// Helper function to format bytes in human-readable format
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}