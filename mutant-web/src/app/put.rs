use std::sync::{Arc, RwLock};

use eframe::egui::{self, Color32, RichText};
use js_sys::Uint8Array;
use mutant_protocol::StorageMode;
use serde::{Deserialize, Serialize};

use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{FileReader, HtmlInputElement, HtmlElement, Event};
use web_time::{Duration, SystemTime};

use super::Window;
use super::components::progress::detailed_progress;
use super::context;
use super::notifications;

#[derive(Clone, Serialize, Deserialize)]
pub struct PutWindow {
    // File selection state
    selected_file: Arc<RwLock<Option<String>>>,
    file_size: Arc<RwLock<Option<u64>>>,
    #[serde(skip)] // Skip serializing file data to avoid localStorage quota issues
    file_data: Arc<RwLock<Option<Vec<u8>>>>,

    // Key name input
    key_name: Arc<RwLock<String>>,

    // Configuration options
    public: Arc<RwLock<bool>>,
    storage_mode: Arc<RwLock<StorageMode>>,
    no_verify: Arc<RwLock<bool>>,

    // Progress tracking
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
}

impl Default for PutWindow {
    fn default() -> Self {
        Self {
            selected_file: Arc::new(RwLock::new(None)),
            file_size: Arc::new(RwLock::new(None)),
            file_data: Arc::new(RwLock::new(None)),
            key_name: Arc::new(RwLock::new(String::new())),
            public: Arc::new(RwLock::new(false)),
            storage_mode: Arc::new(RwLock::new(StorageMode::Heaviest)),
            no_verify: Arc::new(RwLock::new(false)),
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

        log::debug!("Checking progress for put ID: {:?}", put_id_opt);

        if let Some(put_id) = put_id_opt {
            // Get the context
            let ctx = context::context();

            // Get the progress for this put operation
            if let Some(progress) = ctx.get_put_progress(&put_id) {
                // Read the progress
                let progress_guard = progress.read().unwrap();

                log::debug!("Found progress object for put ID: {}", put_id);

                // Check if we have an operation
                if let Some(op) = progress_guard.operation.get("put") {
                    log::info!("Found operation in progress: total_pads={}, reserved={}, written={}, confirmed={}",
                        op.total_pads, op.nb_reserved, op.nb_written, op.nb_confirmed);

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

                    log::info!("Calculated progress: reservation={:.2}%, upload={:.2}%, confirmation={:.2}%",
                        reservation_progress * 100.0, upload_progress * 100.0, confirmation_progress * 100.0);

                    // Update progress bars
                    *self.reservation_progress.write().unwrap() = reservation_progress;
                    *self.upload_progress.write().unwrap() = upload_progress;
                    *self.confirmation_progress.write().unwrap() = confirmation_progress;

                    // Update total chunks
                    *self.total_chunks.write().unwrap() = total_chunks;

                    log::info!("Updated progress bars: reservation={:.2}%, upload={:.2}%, confirmation={:.2}%",
                        *self.reservation_progress.read().unwrap() * 100.0,
                        *self.upload_progress.read().unwrap() * 100.0,
                        *self.confirmation_progress.read().unwrap() * 100.0);

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
                        log::info!("Refreshing keys list after upload completion");
                        spawn_local(async {
                            let ctx = context::context();
                            let _ = ctx.list_keys().await;
                            log::info!("Keys list refreshed successfully after upload completion");
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

    fn select_file(&self) {
        log::info!("File selection requested");

        // Get references to our state
        let selected_file = self.selected_file.clone();
        let file_size = self.file_size.clone();
        let file_data = self.file_data.clone();
        let key_name = self.key_name.clone();

        // Create a file input element
        let window = web_sys::window().expect("no global window exists");
        let document = window.document().expect("no document exists");

        let input: HtmlInputElement = document
            .create_element("input")
            .expect("failed to create input element")
            .dyn_into::<HtmlInputElement>()
            .expect("failed to cast to HtmlInputElement");

        // Set input attributes
        input.set_type("file");

        // Cast to HtmlElement to access style
        let input_html: &HtmlElement = input.dyn_ref::<HtmlElement>()
            .expect("input should be an HtmlElement");
        input_html.style().set_property("display", "none").expect("failed to set style");

        // Append to document body
        let body = document.body().expect("document should have a body");
        body.append_child(&input).expect("failed to append input to body");

        // Create onchange handler
        let onchange = Closure::once(move |event: Event| {
            let input: HtmlInputElement = event
                .target()
                .expect("event should have a target")
                .dyn_into::<HtmlInputElement>()
                .expect("target should be an HtmlInputElement");

            // Get the selected file
            let file_list = input.files().expect("input should have files");
            if let Some(file) = file_list.get(0) {
                let file_name = file.name();
                let file_size_js = file.size();

                // Update state with file name and size
                *selected_file.write().unwrap() = Some(file_name.clone());
                *file_size.write().unwrap() = Some(file_size_js as u64);

                // Auto-fill key name if it's empty
                {
                    let mut key_name_guard = key_name.write().unwrap();
                    if key_name_guard.is_empty() {
                        *key_name_guard = file_name.clone();
                        log::info!("Auto-filled key name with file name: {}", file_name);
                    }
                }

                // Read file content
                let reader = FileReader::new().expect("failed to create FileReader");
                let reader_clone = reader.clone();

                // Create onload handler for the reader
                let file_data_clone = file_data.clone();
                let file_name_clone = file_name.clone();

                let onload = Closure::once(move |_event: Event| {
                    // Get array buffer from reader
                    let array_buffer = reader_clone.result().expect("failed to get result");
                    let array = Uint8Array::new(&array_buffer);

                    // Convert to Rust Vec<u8>
                    let mut data = vec![0; array.length() as usize];
                    array.copy_to(&mut data);

                    // Update state with file data
                    *file_data_clone.write().unwrap() = Some(data);

                    // Show notification
                    notifications::info(format!("File selected: {}", file_name_clone));

                    // Remove the input element
                    if let Some(parent) = input.parent_node() {
                        parent.remove_child(&input).expect("failed to remove input");
                    }
                });

                // Set onload handler
                reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                onload.forget(); // Prevent closure from being dropped

                // Read the file as array buffer
                reader.read_as_array_buffer(&file).expect("failed to read file");
            }
        });

        // Set onchange handler
        input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget(); // Prevent closure from being dropped

        // Click the input to open file dialog
        input.click();
    }

    fn start_upload(&self) {
        log::info!("Starting upload");

        // Set upload state
        *self.is_uploading.write().unwrap() = true;
        *self.upload_complete.write().unwrap() = false;
        *self.start_time.write().unwrap() = Some(SystemTime::now());

        // Reset progress
        *self.reservation_progress.write().unwrap() = 0.0;
        *self.upload_progress.write().unwrap() = 0.0;
        *self.confirmation_progress.write().unwrap() = 0.0;

        // Get upload parameters
        let key_name = self.key_name.read().unwrap().clone();
        let public = *self.public.read().unwrap();
        let storage_mode = self.storage_mode.read().unwrap().clone();
        let no_verify = *self.no_verify.read().unwrap();
        let file_data = self.file_data.read().unwrap().clone().unwrap_or_default();
        let filename = self.selected_file.read().unwrap().clone().unwrap_or_default();

        log::info!("Upload parameters: key={}, filename={}, size={} bytes, public={}, mode={:?}, no_verify={}",
            key_name, filename, file_data.len(), public, storage_mode, no_verify);

        // Clone Arc references for the async task
        let current_put_id = self.current_put_id.clone();
        let error_message = self.error_message.clone();
        let is_uploading = self.is_uploading.clone();

        // Start the actual upload using the context
        spawn_local(async move {
            let ctx = context::context();

            // Create a progress object first
            let (put_id, progress) = ctx.create_progress(&key_name, &filename);
            log::info!("Created progress object with ID: {}", put_id);

            // Store the put ID for progress tracking immediately
            // This allows the UI to start showing progress before the put operation completes
            *current_put_id.write().unwrap() = Some(put_id.clone());
            log::info!("Stored put ID for progress tracking: {}", put_id);

            // Log the initial progress object
            {
                let progress_guard = progress.read().unwrap();
                log::info!("Initial progress object with {} operations", progress_guard.operation.len());
            }

            // Now start the put operation with the progress object
            match ctx.put(&key_name, file_data, &filename, storage_mode, public, no_verify, Some((put_id.clone(), progress.clone()))).await {
                Ok((_put_id, _progress)) => {
                    log::info!("Upload completed with put ID: {}", put_id);

                    // Log the final progress state
                    {
                        let progress_guard = progress.read().unwrap();
                        log::info!("Final progress state with {} operations", progress_guard.operation.len());

                        for (op_name, op) in &progress_guard.operation {
                            log::info!("Operation {}: total_pads={}, reserved={}, written={}, confirmed={}",
                                op_name, op.total_pads, op.nb_reserved, op.nb_written, op.nb_confirmed);
                        }
                    }

                    // Refresh the keys list to update the file explorer
                    log::info!("Refreshing keys list after successful upload");
                    spawn_local(async {
                        let ctx = context::context();
                        let _ = ctx.list_keys().await;
                        log::info!("Keys list refreshed successfully");
                    });
                },
                Err(e) => {
                    log::error!("Failed to complete upload: {}", e);
                    *error_message.write().unwrap() = Some(e.clone());
                    *is_uploading.write().unwrap() = false;

                    // Show notification
                    notifications::error(format!("Upload failed: {}", e));
                }
            }
        });
    }

    fn reset(&self) {
        // Reset all state for a new upload
        *self.selected_file.write().unwrap() = None;
        *self.file_size.write().unwrap() = None;
        *self.file_data.write().unwrap() = None;
        *self.key_name.write().unwrap() = String::new();
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

    fn draw_upload_form(&mut self, ui: &mut egui::Ui) {
        let is_uploading = *self.is_uploading.read().unwrap();
        let upload_complete = *self.upload_complete.read().unwrap();

        if !is_uploading && !upload_complete {
            // File selection section
            ui.heading("Upload File");
            ui.add_space(10.0);

            // Key name input
            ui.horizontal(|ui| {
                ui.label("Key Name:");
                let mut key_name = self.key_name.write().unwrap();
                ui.text_edit_singleline(&mut *key_name);
            });

            ui.add_space(5.0);

            // File selection button
            if ui.button("Select File").clicked() {
                self.select_file();
            }

            // Show selected file info
            if let Some(filename) = &*self.selected_file.read().unwrap() {
                ui.horizontal(|ui| {
                    ui.label("Selected File:");
                    ui.label(filename);
                });

                if let Some(size) = *self.file_size.read().unwrap() {
                    ui.label(format!("Size: {} bytes", size));
                }
            }

            ui.add_space(10.0);

            // Configuration options
            ui.collapsing("Upload Options", |ui| {
                // Public checkbox
                ui.horizontal(|ui| {
                    let mut public = self.public.write().unwrap();
                    ui.checkbox(&mut *public, "Public");
                    ui.label("Make this file publicly accessible");
                });

                // Storage mode selection
                ui.horizontal(|ui| {
                    ui.label("Storage Mode:");
                    let mut storage_mode = self.storage_mode.write().unwrap();

                    egui::ComboBox::new("storage_mode", "")
                        .selected_text(format!("{:?}", *storage_mode))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut *storage_mode, StorageMode::Light, "Light");
                            ui.selectable_value(&mut *storage_mode, StorageMode::Medium, "Medium");
                            ui.selectable_value(&mut *storage_mode, StorageMode::Heavy, "Heavy");
                            ui.selectable_value(&mut *storage_mode, StorageMode::Heaviest, "Heaviest");
                        });
                });

                // No verify checkbox
                ui.horizontal(|ui| {
                    let mut no_verify = self.no_verify.write().unwrap();
                    ui.checkbox(&mut *no_verify, "Skip Verification");
                    ui.label("Skip verification of uploaded data (faster but less safe)");
                });
            });

            ui.add_space(10.0);

            // Upload button
            let can_upload = self.selected_file.read().unwrap().is_some() &&
                            !self.key_name.read().unwrap().is_empty() &&
                            self.file_data.read().unwrap().is_some();

            if ui.add_enabled(can_upload, egui::Button::new("Upload")).clicked() {
                self.start_upload();
            }

            if !can_upload {
                ui.label(RichText::new("Please select a file and enter a key name").color(Color32::LIGHT_RED));
            }

            // Show error message if any
            if let Some(error) = &*self.error_message.read().unwrap() {
                ui.add_space(10.0);
                ui.label(RichText::new(format!("Error: {}", error)).color(Color32::RED));
            }
        } else if is_uploading {
            // Progress section
            ui.heading("Upload Progress");
            ui.add_space(10.0);

            let key_name = self.key_name.read().unwrap();
            ui.label(format!("Uploading: {}", *key_name));

            ui.add_space(5.0);

            // Calculate elapsed time
            let elapsed = if let Some(start_time) = *self.start_time.read().unwrap() {
                start_time.elapsed().unwrap()
            } else {
                *self.elapsed_time.read().unwrap()
            };

            let elapsed_str = format_elapsed_time(elapsed);

            // Get total chunks and current progress
            let total_chunks = *self.total_chunks.read().unwrap();
            let chunks_to_reserve = *self.chunks_to_reserve.read().unwrap();

            // Calculate current counts based on progress
            let reservation_progress = *self.reservation_progress.read().unwrap();
            let upload_progress = *self.upload_progress.read().unwrap();
            let confirmation_progress = *self.confirmation_progress.read().unwrap();

            // Get the latest progress values directly from the context
            let (reservation_progress, upload_progress, confirmation_progress, total_chunks) = {
                if let Some(put_id) = &*self.current_put_id.read().unwrap() {
                    let ctx = context::context();
                    if let Some(progress) = ctx.get_put_progress(put_id) {
                        let progress_guard = progress.read().unwrap();
                        if let Some(op) = progress_guard.operation.get("put") {
                            // Calculate progress percentages
                            let res_progress = if op.total_pads > 0 {
                                op.nb_reserved as f32 / op.total_pads as f32
                            } else {
                                0.0
                            };

                            let up_progress = if op.total_pads > 0 {
                                op.nb_written as f32 / op.total_pads as f32
                            } else {
                                0.0
                            };

                            let conf_progress = if op.total_pads > 0 {
                                op.nb_confirmed as f32 / op.total_pads as f32
                            } else {
                                0.0
                            };

                            // Update the stored progress values
                            *self.reservation_progress.write().unwrap() = res_progress;
                            *self.upload_progress.write().unwrap() = up_progress;
                            *self.confirmation_progress.write().unwrap() = conf_progress;
                            *self.total_chunks.write().unwrap() = op.total_pads;

                            (res_progress, up_progress, conf_progress, op.total_pads)
                        } else {
                            (reservation_progress, upload_progress, confirmation_progress, total_chunks)
                        }
                    } else {
                        (reservation_progress, upload_progress, confirmation_progress, total_chunks)
                    }
                } else {
                    (reservation_progress, upload_progress, confirmation_progress, total_chunks)
                }
            };

            // Calculate current counts for each stage
            let reserved_count = if chunks_to_reserve > 0 {
                (reservation_progress * total_chunks as f32) as usize
            } else {
                (reservation_progress * total_chunks as f32) as usize
            };

            let uploaded_count = (upload_progress * total_chunks as f32) as usize;
            let confirmed_count = (confirmation_progress * total_chunks as f32) as usize;

            log::debug!("Drawing progress bars: reservation={:.2}%, upload={:.2}%, confirmation={:.2}%",
                reservation_progress * 100.0, upload_progress * 100.0, confirmation_progress * 100.0);

            // Reservation progress bar
            ui.label("Reserving pads:");
            ui.add(detailed_progress(reservation_progress, reserved_count, total_chunks, elapsed_str.clone()));

            ui.add_space(5.0);

            // Upload progress bar
            ui.label("Uploading pads:");
            ui.add(detailed_progress(upload_progress, uploaded_count, total_chunks, elapsed_str.clone()));

            ui.add_space(5.0);

            // Confirmation progress bar
            ui.label("Confirming pads:");
            ui.add(detailed_progress(confirmation_progress, confirmed_count, total_chunks, elapsed_str));

            // Cancel button
            ui.add_space(10.0);
            if ui.button("Cancel").clicked() {
                // TODO: Implement cancellation
                *self.is_uploading.write().unwrap() = false;
                notifications::warning("Upload cancelled".to_string());
            }
        } else if upload_complete {
            // Upload complete section
            ui.heading("Upload Complete");
            ui.add_space(10.0);

            let key_name = self.key_name.read().unwrap();
            ui.label(format!("Successfully uploaded: {}", *key_name));

            // Show elapsed time
            let elapsed = *self.elapsed_time.read().unwrap();
            ui.label(format!("Time taken: {}", format_elapsed_time(elapsed)));

            // Show public address if available
            if let Some(address) = &*self.public_address.read().unwrap() {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.label("Public index address:");
                    ui.text_edit_singleline(&mut address.clone());
                });
            }

            ui.add_space(10.0);
            if ui.button("Upload Another File").clicked() {
                self.reset();
            }
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