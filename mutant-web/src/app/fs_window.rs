use std::sync::{Arc, RwLock, Mutex}; // Added Mutex

use eframe::egui::{self, RichText};

use mutant_protocol::KeyDetails;
use serde::{Deserialize, Serialize};
use lazy_static::lazy_static;
use web_sys::Blob;
use js_sys::Uint8Array;

// Updated use statements
use crate::app::components::multimedia;
use crate::app::Window;
use crate::app::window_system::generate_unique_dock_area_id;
use crate::app::{put::PutWindow, stats::StatsWindow, colony_window::ColonyWindow};

// Direct imports for moved types are no longer needed if always fully qualified.
// Example: use crate::app::fs::tree::TreeNode; // No longer needed if using full path

// Global/static functions from submodules might need to be imported if used frequently without full path
// For now, assuming they will be called with full paths like crate::app::fs::global::get_main_fs_window()

// Code moved to mutant-web/src/app/fs/global.rs

// DownloadStatus, ActiveDownload, and related functions moved to mutant-web/src/app/fs/download.rs

// TreeNode struct and impl moved to mutant-web/src/app/fs/tree.rs
// humanize_size function also moved to mutant-web/src/app/fs/tree.rs

// FileViewerTab struct and impl moved to mutant-web/src/app/fs/viewer_tab.rs

// FsInternalTab and FsInternalTabViewer moved to mutant-web/src/app/fs/internal_tab.rs

// Global callback for adding PutWindow after file selection
lazy_static! {
    static ref PENDING_PUT_WINDOW: Arc<RwLock<Option<PutWindow>>> = Arc::new(RwLock::new(None));
}

/// Add a PutWindow to the main FsWindow from anywhere (used by file dialog callback)
pub fn add_put_window_to_main_fs(put_window: PutWindow) {
    if let Some(fs_window_ref) = crate::app::fs::global::get_main_fs_window() {
        let mut fs_window = fs_window_ref.write().unwrap();

        // Check if a Put tab already exists
        let tab_exists = fs_window.internal_dock.iter_all_tabs().any(|(_, existing_tab)| {
            matches!(existing_tab, crate::app::fs::internal_tab::FsInternalTab::Put(_))
        });

        if !tab_exists {
            let tab = crate::app::fs::internal_tab::FsInternalTab::Put(put_window);

            // Add to the internal dock system
            if fs_window.internal_dock.iter_all_tabs().next().is_none() {
                // If the dock is empty, create a new surface
                fs_window.internal_dock = egui_dock::DockState::new(vec![tab]);
            } else {
                // Add to the existing surface
                fs_window.internal_dock.main_surface_mut().push_to_focused_leaf(tab);
            }

            log::info!("Successfully added PutWindow tab to main FsWindow");
        } else {
            log::info!("Put tab already exists in main FsWindow");
        }
    } else {
        log::warn!("Main FsWindow reference not available for adding PutWindow");
    }
}

/// Start immediate file transfer from browser to daemon
fn start_immediate_file_transfer(file: web_sys::File, filename: String) {
    use wasm_bindgen_futures::spawn_local;

    log::info!("Starting immediate file transfer for: {}", filename);

    spawn_local(async move {
        // Read the file in chunks and start streaming to daemon
        let file_size = file.size() as u64;
        const CHUNK_SIZE: u64 = 256 * 1024; // 256KB chunks
        let total_chunks = ((file_size + CHUNK_SIZE - 1) / CHUNK_SIZE) as usize;

        log::info!("File size: {} bytes, will be read in {} chunks", file_size, total_chunks);

        // Update the PutWindow to show that file reading has started
        update_put_window_file_reading_state(true, 0.0, 0);

        let mut offset = 0u64;
        let mut chunk_index = 0usize;

        while offset < file_size {
            let chunk_size = std::cmp::min(CHUNK_SIZE, file_size - offset);
            let end_offset = offset + chunk_size;

            // Create a blob slice for this chunk
            let blob_slice = file.slice_with_i32_and_i32(offset as i32, end_offset as i32).unwrap();

            // Read this chunk
            match read_blob_chunk(blob_slice).await {
                Ok(chunk_data) => {
                    log::debug!("Read chunk {} ({} bytes) at offset {}", chunk_index, chunk_data.len(), offset);

                    // Update progress
                    let progress = (end_offset as f32) / (file_size as f32);
                    update_put_window_file_reading_state(true, progress, end_offset);

                    // TODO: Send chunk to daemon here
                    // For now, we'll just simulate the transfer

                    chunk_index += 1;
                    offset = end_offset;
                },
                Err(e) => {
                    log::error!("Failed to read chunk {}: {}", chunk_index, e);
                    update_put_window_error_state(format!("Failed to read file: {}", e));
                    return;
                }
            }

            // Small delay to prevent blocking the UI
            gloo_timers::future::TimeoutFuture::new(10).await;
        }

        log::info!("File reading completed for: {}", filename);
        update_put_window_file_reading_state(false, 1.0, file_size);
    });
}

/// Read a blob chunk asynchronously
async fn read_blob_chunk(blob: Blob) -> Result<Vec<u8>, String> {
    use wasm_bindgen_futures::JsFuture;
    use web_sys::Response;

    // Use the Response API to read the blob as array buffer
    let response = Response::new_with_opt_blob(Some(&blob))
        .map_err(|e| format!("Failed to create response: {:?}", e))?;

    let array_buffer_promise = response.array_buffer()
        .map_err(|e| format!("Failed to get array buffer: {:?}", e))?;

    let array_buffer = JsFuture::from(array_buffer_promise).await
        .map_err(|e| format!("Failed to read array buffer: {:?}", e))?;

    let uint8_array = Uint8Array::new(&array_buffer);
    let mut data = vec![0u8; uint8_array.length() as usize];
    uint8_array.copy_to(&mut data);

    Ok(data)
}

/// Update the PutWindow file reading state
fn update_put_window_file_reading_state(is_reading: bool, progress: f32, bytes_read: u64) {
    if let Some(fs_window_ref) = crate::app::fs::global::get_main_fs_window() {
        let mut fs_window = fs_window_ref.write().unwrap();

        // Find the PutWindow tab and update its state
        for (_, tab) in fs_window.internal_dock.iter_all_tabs_mut() {
            if let crate::app::fs::internal_tab::FsInternalTab::Put(put_window) = tab {
                // Update the file reading state using the public method
                put_window.update_file_reading_state(is_reading, progress, bytes_read);
                break;
            }
        }
    }
}

/// Update the PutWindow error state
fn update_put_window_error_state(error: String) {
    if let Some(fs_window_ref) = crate::app::fs::global::get_main_fs_window() {
        let mut fs_window = fs_window_ref.write().unwrap();

        // Find the PutWindow tab and update its error state
        for (_, tab) in fs_window.internal_dock.iter_all_tabs_mut() {
            if let crate::app::fs::internal_tab::FsInternalTab::Put(put_window) = tab {
                // Set error message using the public method
                put_window.set_error_message(error);
                break;
            }
        }
    }
}

/// The filesystem tree window
#[derive(Clone, Serialize, Deserialize)]
pub struct FsWindow {
    keys: Arc<RwLock<Vec<KeyDetails>>>,
    root: crate::app::fs::tree::TreeNode, // This was already updated in a previous step, ensure it's correct
    /// Path of the selected file (for highlighting in the tree)
    selected_path: Option<String>,
    /// Internal dock system for all tabs within this window
    internal_dock: egui_dock::DockState<crate::app::fs::internal_tab::FsInternalTab>, // This was also updated
    #[serde(skip)] // active_downloads should not be serialized
    active_downloads: Arc<Mutex<Vec<crate::app::fs::download::ActiveDownload>>>, // Updated path for ActiveDownload
    /// Unique identifier for this window instance to avoid widget ID conflicts
    #[serde(skip)]
    window_id: String,
    /// Pending delete confirmation dialog
    #[serde(skip)]
    pending_delete: Option<KeyDetails>,
    /// Unique dock area ID for the internal dock area within this FsWindow
    #[serde(skip)]
    dock_area_id: String,
    /// Drag and drop state for the filesystem tree
    #[serde(skip)]
    drag_state: crate::app::fs::tree::DragDropState,
}

impl Default for FsWindow {
    fn default() -> Self {
        Self {
            keys: crate::app::context::context().get_key_cache(),
            root: crate::app::fs::tree::TreeNode::default(),
            selected_path: None,
            internal_dock: egui_dock::DockState::new(vec![]),
            active_downloads: Arc::new(Mutex::new(Vec::new())),
            window_id: uuid::Uuid::new_v4().to_string(),
            pending_delete: None,
            dock_area_id: generate_unique_dock_area_id(),
            drag_state: crate::app::fs::tree::DragDropState::default(),
        }
    }
}

impl Window for FsWindow {
    fn name(&self) -> String {
        "MutAnt Files".to_string()
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        self.build_tree();

        // Use a horizontal layout with two panels - improved styling
        egui::SidePanel::left(format!("mutant_fs_tree_panel_{}", self.window_id))
            .resizable(true)
            .min_width(220.0)
            .default_width(320.0)
            .max_width(500.0)
            .show_separator_line(true)
            .show_inside(ui, |ui| {
                // Apply consistent background styling
                ui.style_mut().visuals.panel_fill = super::theme::MutantColors::BACKGROUND_MEDIUM;
                self.draw_tree(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            // Show the internal dock system for all tabs
            if self.internal_dock.iter_all_tabs().next().is_none() {
                // Show instructions when no tabs are open - properly centered both horizontally and vertically
                let available_rect = ui.available_rect_before_wrap();

                // Use vertical centering with flexible spacing
                ui.vertical_centered(|ui| {
                    // Add flexible space to push content to vertical center
                    let content_height = 120.0; // Approximate height of our content
                    let top_space = (available_rect.height() - content_height) / 2.0;
                    if top_space > 0.0 {
                        ui.add_space(top_space);
                    }

                    // Main heading with dimmed color
                    ui.label(
                        egui::RichText::new("MutAnt Workspace")
                            .size(24.0)
                            .color(super::theme::MutantColors::TEXT_MUTED)
                    );

                    ui.add_space(15.0);

                    // Instructions with even more dimmed color
                    ui.label(
                        egui::RichText::new("Click on a file in the tree to open it here.")
                            .size(14.0)
                            .color(super::theme::MutantColors::TEXT_DISABLED)
                    );

                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new("Use the left menu to open Upload or Stats windows.")
                            .size(14.0)
                            .color(super::theme::MutantColors::TEXT_DISABLED)
                    );

                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new("All windows will dock in this area.")
                            .size(14.0)
                            .color(super::theme::MutantColors::TEXT_DISABLED)
                    );
                });
            } else {
                // FORCE egui style override directly in the UI context
                ui.style_mut().visuals.widgets.active.weak_bg_fill = super::theme::MutantColors::BACKGROUND_LIGHT;
                ui.style_mut().visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, super::theme::MutantColors::ACCENT_ORANGE);
                ui.style_mut().visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, super::theme::MutantColors::ACCENT_ORANGE);

                ui.style_mut().visuals.widgets.inactive.weak_bg_fill = super::theme::MutantColors::BACKGROUND_MEDIUM;
                ui.style_mut().visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
                ui.style_mut().visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, super::theme::MutantColors::TEXT_MUTED);

                ui.style_mut().visuals.widgets.hovered.weak_bg_fill = super::theme::MutantColors::SURFACE_HOVER;
                ui.style_mut().visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, super::theme::MutantColors::ACCENT_ORANGE);
                ui.style_mut().visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, super::theme::MutantColors::TEXT_PRIMARY);

                // Create a custom tab viewer that handles colors
                let mut tab_viewer = crate::app::fs::internal_tab::FsInternalTabViewer {};

                // Show the internal dock area with a unique ID
                egui_dock::DockArea::new(&mut self.internal_dock)
                    .id(egui::Id::new(format!("fs_internal_dock_{}", self.dock_area_id)))
                    .show_inside(ui, &mut tab_viewer);
            }
        });

        // Draw delete confirmation modal if needed
        self.draw_delete_confirmation_modal(ui);

        // // Draw Active Downloads UI
        // ui.separator(); // Visually separate from file viewer
        // ui.heading("Active Downloads");
        // egui::ScrollArea::vertical().show(ui, |ui| {
        //     let mut downloads_guard = self.active_downloads.lock().unwrap();
        //     // Remove completed/failed downloads after a delay or via a clear button (not implemented here)
        //     // For now, just display them.
        //     for download in downloads_guard.iter() {
        //         ui.label(format!("File: {}", download.file_name));
        //         ui.label(format!("Status: {:?}", download.status));
        //         if download.key_details.total_size > 0 {
        //             ui.add(egui::ProgressBar::new(download.progress_val).show_percentage());
        //             ui.label(format!("{} / {} bytes",
        //                 humansize::format_size(download.downloaded_bytes, humansize::BINARY),
        //                 humansize::format_size(download.key_details.total_size, humansize::BINARY)
        //             ));
        //         } else {
        //              ui.label(format!("{} downloaded", humansize::format_size(download.downloaded_bytes, humansize::BINARY)));
        //         }
        //         if let Some(err) = &download.error_message {
        //             ui.colored_label(crate::app::theme::MutantColors::ERROR, format!("Error: {}", err));
        //         }
        //         ui.separator();
        //     }
        //     // Keep only non-completed/failed for now, or implement manual clearing
        //     downloads_guard.retain(|d| match d.status {
        //         DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled => false,
        //         _ => true,
        //     });
        // });
    }
}

impl FsWindow {
    pub fn new() -> Self {
        // Use default implementation which already includes unique window_id
        Self::default()
    }

    // Download related methods moved to mutant-web/src/app/fs/download.rs

    // find_tab methods removed - use WindowSystem methods directly

    /// Add a new tab for a file - adds to the internal dock system
    pub fn add_file_tab(&mut self, file_details: KeyDetails) {
        log::info!("FsWindow: Creating new file viewer tab for: {}", file_details.key);

        // Check if a tab for this file already exists in the internal dock
        let tab_exists = self.internal_dock.iter_all_tabs().any(|(_, existing_tab)| {
            match existing_tab {
                crate::app::fs::internal_tab::FsInternalTab::FileViewer(file_tab) => file_tab.file.key == file_details.key,
                _ => false,
            }
        });

        if !tab_exists {
            // Create a new tab
            let file_tab = crate::app::fs::viewer_tab::FileViewerTab::new(file_details.clone());
            let tab = crate::app::fs::internal_tab::FsInternalTab::FileViewer(file_tab);

            // Add directly to the internal dock system
            if self.internal_dock.iter_all_tabs().next().is_none() {
                // If the dock is empty, create a new surface
                self.internal_dock = egui_dock::DockState::new(vec![tab]);
            } else {
                // Add to the existing surface
                self.internal_dock.main_surface_mut().push_to_focused_leaf(tab);
            }

            log::info!("FsWindow: Successfully added tab to internal dock for: {}", file_details.key);

            // Determine if this is a video file to skip full download
            let key_to_find = file_details.key.clone();
            let mut tab_updated_for_video = false;

            let file_type_hint = crate::app::components::multimedia::detect_file_type(&[], &key_to_find);

            if file_type_hint == crate::app::components::multimedia::FileType::Video {
                log::info!("Hint suggests {} is a video. Attempting to set up for streaming.", key_to_find);
                // The tab was just added, find it to update its state for streaming
                for (_surface_index, tab_mut) in self.internal_dock.iter_all_tabs_mut() {
                    if let crate::app::fs::internal_tab::FsInternalTab::FileViewer(file_viewer_tab_mut) = tab_mut {
                        if file_viewer_tab_mut.file.key == key_to_find {
                            log::info!("Found tab for video {}. Updating for streaming.", key_to_find);
                            // Call update_content on FileViewerTab to set it up for streaming
                            file_viewer_tab_mut.update_content(
                                Vec::new(), // Empty data, as we will stream
                                Some(crate::app::components::multimedia::FileType::Video),
                                true // is_initial_load = true
                            );
                            // is_loading is now set inside update_content
                            tab_updated_for_video = true;
                            break;
                        }
                    }
                }
                if !tab_updated_for_video {
                    log::warn!("Could not find the newly added video tab for {} to update for streaming. Falling back to full load.", key_to_find);
                    // Fallback to normal load if tab not found immediately (should not happen)
                    self.load_file_content(file_details);
                } else {
                    log::info!("Video tab {} configured for streaming, skipping full download.", key_to_find);
                }
            } else {
                log::info!("{} is not a video according to hint, proceeding with full download.", key_to_find);
                // Start loading the file content asynchronously for non-video files
                self.load_file_content(file_details);
            }
        } else {
            log::info!("FsWindow: Tab for file {} already exists in internal dock", file_details.key);
        }
    }

    /// Load file content asynchronously and update the tab when complete
    fn load_file_content(&self, file_details: KeyDetails) {
        let key = file_details.key.clone();
        let is_public = file_details.is_public;

        log::info!("Starting async file loading for: {}", key);

        // Spawn async task to load file content
        wasm_bindgen_futures::spawn_local(async move {
            let ctx = crate::app::context::context();

            // Get the total file size for progress calculation
            let total_file_size = Some(file_details.total_size);
            let key_for_progress = key.clone();

            match ctx.get_file_for_viewing_with_progress(
                &key,
                is_public,
                move |downloaded_bytes, _total_bytes| {
                    // Update progress in the UI using the known total file size
                    if let Some(fs_window_ref) = crate::app::fs::global::get_main_fs_window() { // Updated path
                        let mut fs_window = fs_window_ref.write().unwrap();

                        // Look for the file tab in the FsWindow's internal dock
                        for (_, internal_tab) in fs_window.internal_dock.iter_all_tabs_mut() {
                            if let crate::app::fs::internal_tab::FsInternalTab::FileViewer(file_tab) = internal_tab {
                                if file_tab.file.key == key_for_progress {
                                    file_tab.downloaded_bytes = downloaded_bytes as u64;
                                    file_tab.total_bytes = total_file_size;

                                    // Calculate progress using the known total file size
                                    if let Some(total) = total_file_size {
                                        if total > 0 {
                                            file_tab.loading_progress = (downloaded_bytes as f32 / total as f32).min(1.0);
                                        } else {
                                            file_tab.loading_progress = 0.0;
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            ).await {
                Ok(data) => {
                    log::info!("Successfully loaded file content for: {} ({} bytes)", key, data.len());

                    // Update the tab with the loaded content using the global FsWindow reference
                    if let Some(fs_window_ref) = crate::app::fs::global::get_main_fs_window() {
                        let mut fs_window = fs_window_ref.write().unwrap();

                        // Look for the file tab in the FsWindow's internal dock
                        for (_, internal_tab) in fs_window.internal_dock.iter_all_tabs_mut() {
                            if let crate::app::fs::internal_tab::FsInternalTab::FileViewer(file_tab) = internal_tab {
                                if file_tab.file.key == key {
                                    let file_type_detected_from_data = crate::app::components::multimedia::detect_file_type(&data, &key);
                                    file_tab.update_content(data, Some(file_type_detected_from_data), true);
                                    // is_loading is set inside update_content
                                    log::info!("Updated file tab content for: {}", key);
                                    return; // Exit early since we found and updated the tab
                                }
                            }
                        }
                        log::warn!("Could not find file tab for key: {} in main FsWindow", key);
                    } else {
                        log::warn!("Main FsWindow reference not available for key: {}", key);
                    }
                },
                Err(e) => {
                    log::error!("Failed to load file content for {}: {}", key, e);
                    // Update the tab with error state using the global FsWindow reference
                    if let Some(fs_window_ref) = crate::app::fs::global::get_main_fs_window() { // Updated path
                        let mut fs_window = fs_window_ref.write().unwrap();
                        // Look for the file tab in the FsWindow's internal dock
                        for (_, internal_tab) in fs_window.internal_dock.iter_all_tabs_mut() {
                            if let crate::app::fs::internal_tab::FsInternalTab::FileViewer(file_tab) = internal_tab {
                                if file_tab.file.key == key {
                                    let error_message = format!("Error loading file: {}", e);
                                    file_tab.update_content(
                                        error_message.into_bytes(),
                                        Some(multimedia::FileType::Other), // Treat error display as 'Other'
                                        true // is_initial_load = true
                                    );
                                    // is_loading is set inside update_content
                                    return; // Exit early since we found and updated the tab
                                }
                            }
                        }
                        log::warn!("Could not find file tab for error update, key: {} in main FsWindow", key);
                    } else {
                        log::warn!("Main FsWindow reference not available for error update, key: {}", key);
                    }
                }
            }
        });
    }

    /// Add a new Put window tab to the internal dock system
    /// This now immediately triggers the file dialog and only creates the window after file selection
    pub fn add_put_tab(&mut self) {
        log::info!("FsWindow: Triggering file dialog for upload");

        // Check if a Put tab already exists
        let tab_exists = self.internal_dock.iter_all_tabs().any(|(_, existing_tab)| {
            matches!(existing_tab, crate::app::fs::internal_tab::FsInternalTab::Put(_))
        });

        if !tab_exists {
            // Trigger file dialog immediately and only create PutWindow after successful selection
            self.trigger_file_dialog_for_upload();
        } else {
            log::info!("FsWindow: Put tab already exists in internal dock");
        }
    }

    /// Trigger file dialog and create PutWindow only after successful file selection
    fn trigger_file_dialog_for_upload(&mut self) {
        use wasm_bindgen::{JsCast, closure::Closure};
        use web_sys::Event;

        log::info!("Triggering file dialog for upload");

        // Create file input element
        let document = web_sys::window().unwrap().document().unwrap();
        let input = document
            .create_element("input")
            .unwrap()
            .dyn_into::<web_sys::HtmlInputElement>()
            .unwrap();

        input.set_type("file");
        input.set_accept("*/*");

        // We need to capture self in a way that works with the closure
        // Since we can't move self into the closure, we'll use a different approach
        let input_clone = input.clone();

        let onchange = Closure::once(move |_event: Event| {
            if let Some(files) = input_clone.files() {
                if files.length() > 0 {
                    if let Some(file) = files.get(0) {
                        let file_name = file.name();
                        let file_size_js = file.size();
                        let file_type_js = file.type_();

                        log::info!("File selected: {} ({} bytes, type: {})", file_name, file_size_js, file_type_js);

                        // Create PutWindow with the selected file information
                        let mut put_window = PutWindow::new();

                        // Set the file information in the PutWindow using the setter method
                        let file_type_str = if file_type_js.is_empty() {
                            "Unknown".to_string()
                        } else {
                            file_type_js
                        };
                        put_window.set_file_info(file_name.clone(), file_size_js as u64, file_type_str);

                        // Add the PutWindow to the main FsWindow using the global function
                        add_put_window_to_main_fs(put_window);

                        // Start immediate file transfer from browser to daemon
                        start_immediate_file_transfer(file, file_name.clone());

                        crate::app::notifications::info(format!("File selected: {}. Upload window opened.", file_name));
                    }
                } else {
                    log::info!("File selection cancelled - no PutWindow will be created");
                }
            }
        });

        input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget();

        // Trigger file picker
        input.click();
    }

    /// Add a new Stats window tab to the internal dock system
    pub fn add_stats_tab(&mut self) {
        log::info!("FsWindow: Creating new Stats window tab");

        // Check if a Stats tab already exists
        let tab_exists = self.internal_dock.iter_all_tabs().any(|(_, existing_tab)| {
            matches!(existing_tab, crate::app::fs::internal_tab::FsInternalTab::Stats(_))
        });

        if !tab_exists {
            // Create a new Stats window
            let stats_window = StatsWindow::new();
            let tab = crate::app::fs::internal_tab::FsInternalTab::Stats(stats_window);

            // Add to the internal dock system
            if self.internal_dock.iter_all_tabs().next().is_none() {
                // If the dock is empty, create a new surface
                self.internal_dock = egui_dock::DockState::new(vec![tab]);
            } else {
                // Add to the existing surface
                self.internal_dock.main_surface_mut().push_to_focused_leaf(tab);
            }

            log::info!("FsWindow: Successfully added Stats tab to internal dock");
        } else {
            log::info!("FsWindow: Stats tab already exists in internal dock");
        }
    }

    /// Add a new Colony window tab to the internal dock system
    pub fn add_colony_tab(&mut self) {
        log::info!("FsWindow: Creating new Colony window tab");

        // Check if a Colony tab already exists
        let tab_exists = self.internal_dock.iter_all_tabs().any(|(_, existing_tab)| {
            matches!(existing_tab, crate::app::fs::internal_tab::FsInternalTab::Colony(_))
        });

        if !tab_exists {
            // Create a new Colony window
            let colony_window = ColonyWindow::default();
            let tab = crate::app::fs::internal_tab::FsInternalTab::Colony(colony_window);

            // Add to the internal dock system
            if self.internal_dock.iter_all_tabs().next().is_none() {
                // If the dock is empty, create a new surface
                self.internal_dock = egui_dock::DockState::new(vec![tab]);
            } else {
                // Add to the existing surface
                self.internal_dock.main_surface_mut().push_to_focused_leaf(tab);
            }

            log::info!("FsWindow: Successfully added Colony tab to internal dock");
        } else {
            log::info!("FsWindow: Colony tab already exists in internal dock");
        }
    }

    /// Check if a specific window type is currently open in the internal dock
    pub fn is_window_open(&self, window_name: &str) -> bool {
        self.internal_dock.iter_all_tabs().any(|(_, tab)| {
            match tab {
                crate::app::fs::internal_tab::FsInternalTab::Put(_) => window_name == "MutAnt Upload",
                crate::app::fs::internal_tab::FsInternalTab::Stats(_) => window_name == "MutAnt Stats",
                crate::app::fs::internal_tab::FsInternalTab::Colony(_) => window_name == "Colony",
                crate::app::fs::internal_tab::FsInternalTab::FileViewer(_) => false, // File viewers don't count for menu highlighting
            }
        })
    }

    /// Build the tree from the current keys
    /// If force_rebuild is true, the tree will be rebuilt even if it's not empty
    pub fn build_tree(&mut self) {
        // Get the current key count to check if we need to rebuild
        let keys = self.keys.read().unwrap();
        let current_key_count = keys.len();

        // Check if we need to rebuild the tree
        // Rebuild if:
        // 1. The tree is empty (first time or reset)
        // 2. The number of keys has changed (new keys added or removed)
        // 3. The key paths have changed (files moved between folders)
        let needs_rebuild = self.root.children.is_empty() ||
                           self.count_tree_files(&self.root) != current_key_count ||
                           self.tree_keys_have_changed(&keys);

        if needs_rebuild {
            log::info!("Rebuilding file tree with {} keys", current_key_count);

            // Create a fresh root
            self.root = crate::app::fs::tree::TreeNode::new_dir("root");

            // Add each key to the tree
            for key in keys.iter() {
                let path = &key.key;

                // Ensure the path starts with a slash
                let normalized_path = if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{}", path)
                };

                // Split the path into parts, skipping empty parts (consecutive slashes)
                let parts: Vec<&str> = normalized_path.split('/')
                    .filter(|part| !part.is_empty())
                    .collect();

                // Insert into the tree
                self.root.insert_key(&parts, key.clone(), "");
            }

            // Expand all directories recursively by default
            self.root.expand_all();

            log::info!("File tree rebuilt successfully");
        }
    }

    /// Count the number of file nodes in the tree (for comparison with key count)
    fn count_tree_files(&self, node: &crate::app::fs::tree::TreeNode) -> usize {
        let mut count = 0;

        // If this is a file node, count it
        if !node.is_dir() {
            count += 1;
        }

        // Count files in all children
        for (_, child) in &node.children {
            count += self.count_tree_files(child);
        }

        count
    }

    /// Check if the key paths in the tree have changed compared to the current key cache
    fn tree_keys_have_changed(&self, current_keys: &[KeyDetails]) -> bool {
        // Collect all key paths from the current tree
        let mut tree_keys = std::collections::HashSet::new();
        self.collect_tree_keys(&self.root, &mut tree_keys);

        // Collect all key paths from the current cache
        let cache_keys: std::collections::HashSet<String> = current_keys.iter()
            .map(|k| k.key.clone())
            .collect();

        // If the sets are different, the tree needs to be rebuilt
        tree_keys != cache_keys
    }

    /// Recursively collect all key paths from the tree
    fn collect_tree_keys(&self, node: &crate::app::fs::tree::TreeNode, keys: &mut std::collections::HashSet<String>) {
        // If this is a file node, add its key to the set
        if !node.is_dir() {
            if let Some(key_details) = &node.key_details {
                keys.insert(key_details.key.clone());
            }
        }

        // Recursively collect keys from all children
        for (_, child) in &node.children {
            self.collect_tree_keys(child, keys);
        }
    }

    /// Draw the tree UI
    fn draw_tree(&mut self, ui: &mut egui::Ui) {
        // Add minimal top padding
        ui.add_space(8.0);

        // Draw the tree with improved styling
        let scroll_response = egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // Draw the continuous gradient bar FIRST (behind the content)
                let available_rect = ui.available_rect_before_wrap();
                let metadata_width = 100.0; // Balanced width for metadata area
                let fade_width = 40.0; // Balanced fade width for smooth gradient
                let fade_start = available_rect.width() - metadata_width - fade_width;
                let background_color = super::theme::MutantColors::BACKGROUND_MEDIUM;

                // Create a smooth continuous gradient bar that spans the entire available height
                let gradient_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(available_rect.left() + fade_start, available_rect.top()),
                    egui::Vec2::new(fade_width + metadata_width, available_rect.height())
                );

                // Create a darker version of the background color for the gradient target
                let darker_color = egui::Color32::from_rgb(
                    (background_color.r() as f32 * 0.4) as u8,
                    (background_color.g() as f32 * 0.4) as u8,
                    (background_color.b() as f32 * 0.4) as u8,
                );

                // Draw the gradient with more steps for smoother transition
                let steps = 30;
                let step_width = fade_width / steps as f32;

                for i in 0..steps {
                    let progress = i as f32 / (steps - 1) as f32; // 0.0 to 1.0

                    // Start with transparent (alpha = 0) and gradually increase alpha to show darker color
                    let alpha = (progress * 180.0) as u8; // Max alpha of 180 for subtle effect

                    let step_rect = egui::Rect::from_min_size(
                        egui::Pos2::new(gradient_rect.left() + i as f32 * step_width, gradient_rect.top()),
                        egui::Vec2::new(step_width, gradient_rect.height())
                    );

                    let fade_color = egui::Color32::from_rgba_unmultiplied(
                        darker_color.r(),
                        darker_color.g(),
                        darker_color.b(),
                        alpha
                    );

                    ui.painter().rect_filled(step_rect, 0.0, fade_color);
                }

                // Draw a solid darker background for the metadata area with alpha blending
                let metadata_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(available_rect.right() - metadata_width, available_rect.top()),
                    egui::Vec2::new(metadata_width, available_rect.height())
                );
                let metadata_color = egui::Color32::from_rgba_unmultiplied(
                    darker_color.r(),
                    darker_color.g(),
                    darker_color.b(),
                    180 // Same alpha as the end of the gradient
                );
                ui.painter().rect_filled(metadata_rect, 0.0, metadata_color);

                // Add some top padding
                ui.add_space(12.0);

                // Get the currently selected path for highlighting
                let selected_path_ref = self.selected_path.as_deref();

                // Track clicked nodes to handle after the loop
                let mut view_details_clicked: Option<KeyDetails> = None;
                let mut download_details_clicked: Option<KeyDetails> = None;
                let mut delete_details_clicked: Option<KeyDetails> = None;
                let mut drag_drop_result = crate::app::fs::tree::DragDropResult::None;

                // Draw the root folder '/' as an uncollapsible folder (always expanded)
                // Draw the root folder with buttons positioned like a file's download button
                let row_response = ui.allocate_response(
                    egui::Vec2::new(ui.available_width(), 20.0),
                    egui::Sense::click()
                );

                let row_rect = row_response.rect;

                ui.horizontal(|ui| {
                    // Draw visual delimiter line for root folder
                    let line_color = super::theme::MutantColors::BORDER_LIGHT;
                    let line_x = 6.0; // Position line at the left edge
                    let line_start = ui.cursor().top();
                    let line_end = line_start + 20.0; // Height of one row

                    ui.painter().line_segment(
                        [egui::Pos2::new(line_x, line_start), egui::Pos2::new(line_x, line_end)],
                        egui::Stroke::new(1.0, line_color)
                    );

                    // Root folder as static text (uncollapsible)
                    let icon = "📂"; // Always open folder icon
                    let text = RichText::new(format!("{} /", icon))
                        .size(12.0)  // Match other folder sizes
                        .color(super::theme::MutantColors::ACCENT_ORANGE);  // Special distinguishing color for folders

                    // Draw the root folder text
                    ui.add_space(1.5); // Match indentation from tree.rs
                    ui.label(text);
                });

                // Check if root directory is a drop target
                if self.drag_state.is_dragging {
                    // Create a reasonable drop zone based on the current UI area
                    let current_rect = ui.cursor();
                    let drop_zone_rect = egui::Rect::from_min_size(
                        current_rect.left_top(),
                        egui::Vec2::new(ui.available_width().min(400.0), 25.0) // Constrained width and height
                    );
                    let pointer_pos = ui.ctx().pointer_latest_pos();

                    if let Some(pos) = pointer_pos {
                        if drop_zone_rect.contains(pos) {
                            log::info!("Setting drop target to root directory");
                            self.drag_state.drop_target = Some("/".to_string());
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);

                            // Enhanced visual feedback for drop target
                            // Draw a filled background
                            ui.painter().rect_filled(
                                drop_zone_rect,
                                6.0,
                                egui::Color32::from_rgba_premultiplied(255, 140, 0, 30) // Orange with transparency
                            );

                            // Draw a bright border
                            ui.painter().rect_stroke(
                                drop_zone_rect,
                                6.0,
                                egui::Stroke::new(3.0, super::theme::MutantColors::ACCENT_ORANGE),
                                egui::epaint::StrokeKind::Outside
                            );
                        }
                    }
                }

                // Sort children: files first, then directories
                let mut sorted_children: Vec<_> = self.root.children.iter_mut().collect();
                sorted_children.sort_by(|(_, a), (_, b)| {
                    match (a.is_dir(), b.is_dir()) {
                        (true, false) => std::cmp::Ordering::Greater, // Directories come after files
                        (false, true) => std::cmp::Ordering::Less,    // Files come before directories
                        _ => a.name.cmp(&b.name),                     // Sort alphabetically within each group
                    }
                });

                // Draw the sorted children with indentation level 1 (since they are children of the root '/')
                for (_, child) in sorted_children {
                    let (view_details, download_details, delete_details, child_drag_result) = child.ui(ui, 1, selected_path_ref, &self.window_id, &mut self.drag_state);
                    if view_details.is_some() {
                        view_details_clicked = view_details;
                    }
                    if download_details.is_some() {
                        download_details_clicked = download_details;
                    }
                    if delete_details.is_some() {
                        delete_details_clicked = delete_details;
                    }
                    if !matches!(child_drag_result, crate::app::fs::tree::DragDropResult::None) {
                        drag_drop_result = child_drag_result;
                    }
                }

                // Draw buttons at the same position as file download buttons
                let text_baseline_y = row_rect.top() + (row_rect.height() - 12.0) / 2.0;
                let button_width = 20.0;
                let mut current_x = row_rect.right() - 4.0; // Start with right padding

                // Refresh button (rightmost)
                let refresh_button_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(current_x - button_width, text_baseline_y),
                    egui::Vec2::new(button_width, 12.0)
                );

                // Draw refresh button manually
                let refresh_button_response = ui.allocate_rect(refresh_button_rect, egui::Sense::click());
                if refresh_button_response.clicked() {
                    // Force a tree rebuild by clearing the current tree
                    self.root = crate::app::fs::tree::TreeNode::new_dir("root");

                    // Trigger a refresh of the file list
                    let ctx = crate::app::context::context();
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = ctx.list_keys().await;
                    });
                }
                if refresh_button_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                // Draw the refresh icon at the exact text baseline
                let refresh_galley = ui.painter().layout_no_wrap(
                    "🔄".to_string(),
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    super::theme::MutantColors::TEXT_MUTED
                );
                ui.painter().galley(
                    egui::Pos2::new(current_x - button_width / 2.0 - refresh_galley.rect.width() / 2.0, text_baseline_y),
                    refresh_galley,
                    super::theme::MutantColors::TEXT_MUTED
                );

                current_x -= button_width + 4.0; // Move left for expand/collapse button

                // Expand/Collapse all button (to the left of refresh button)
                let expand_button_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(current_x - button_width, text_baseline_y),
                    egui::Vec2::new(button_width, 12.0)
                );

                // Draw expand/collapse button manually
                let expand_button_response = ui.allocate_rect(expand_button_rect, egui::Sense::click());
                if expand_button_response.clicked() {
                    // Check if any directories are expanded to determine action
                    if self.root.has_expanded_dirs() {
                        // Some directories are expanded, so collapse all
                        self.root.collapse_all();
                    } else {
                        // All directories are collapsed, so expand all
                        self.root.expand_all();
                    }
                }
                if expand_button_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                // Draw the expand/collapse icon based on current state
                let expand_icon = if self.root.has_expanded_dirs() {
                    "⊟" // Collapse all icon (minus in box)
                } else {
                    "⊞" // Expand all icon (plus in box)
                };
                let expand_galley = ui.painter().layout_no_wrap(
                    expand_icon.to_string(),
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    super::theme::MutantColors::TEXT_MUTED
                );
                ui.painter().galley(
                    egui::Pos2::new(current_x - button_width / 2.0 - expand_galley.rect.width() / 2.0, text_baseline_y),
                    expand_galley,
                    super::theme::MutantColors::TEXT_MUTED
                );



                // Add some bottom padding
                ui.add_space(8.0);

                (view_details_clicked, download_details_clicked, delete_details_clicked, drag_drop_result)
            });



        // Handle the view clicked node outside the loop to avoid borrow issues
        if let Some(details) = scroll_response.inner.0 {
            // Update the selected path for highlighting in the tree
            self.selected_path = Some(details.key.clone());

            // Add a new tab for this file using the unified dock system
            // The add_file_tab method now automatically triggers async file loading
            self.add_file_tab(details);
        }

        // Handle the download click
        if let Some(details) = scroll_response.inner.1 {
            // self.initiate_download(details, ui.ctx().clone());
            // Call the new free-standing function
            crate::app::fs::download::initiate_download(
                Arc::clone(&self.active_downloads),
                details,
                ui.ctx().clone(),
            );
        }

        // Handle the delete click
        if let Some(details) = scroll_response.inner.2 {
            // Show confirmation dialog and delete the file
            self.handle_delete_request(details);
        }

        // Handle drag and drop operations
        match scroll_response.inner.3 {
            crate::app::fs::tree::DragDropResult::Move(old_path, new_path) => {
                self.handle_move_operation(old_path, new_path);
            }
            crate::app::fs::tree::DragDropResult::MoveDirectory(old_dir_path, new_dir_path) => {
                self.handle_directory_move_operation(old_dir_path, new_dir_path);
            }
            crate::app::fs::tree::DragDropResult::None => {
                // No operation
            }
        }

        // Draw dragged item preview
        if let Some((dragged_path, is_dir)) = &self.drag_state.dragged_item {
            if let Some(drag_pos) = self.drag_state.drag_pos {
                self.draw_drag_preview(ui, dragged_path, *is_dir, drag_pos);
            }
        }
    }

    /// Handle delete request with confirmation
    fn handle_delete_request(&mut self, details: KeyDetails) {
        // Store the details for the confirmation dialog
        self.pending_delete = Some(details);
    }

    /// Perform the actual delete operation
    fn perform_delete(&mut self, key: String) {
        // Spawn async task to delete the file
        wasm_bindgen_futures::spawn_local(async move {
            let ctx = crate::app::context::context();
            match ctx.rm(&key).await {
                Ok(_) => {
                    log::info!("Successfully deleted key: {}", key);
                    // Refresh the file list to update the UI
                    let _ = ctx.list_keys().await;
                }
                Err(e) => {
                    log::error!("Failed to delete key {}: {}", key, e);
                    // You could show an error notification here
                }
            }
        });
    }

    /// Handle move operation from drag and drop
    fn handle_move_operation(&mut self, old_path: String, new_path: String) {
        log::info!("Moving '{}' to '{}'", old_path, new_path);

        // Spawn async task to perform the move operation
        wasm_bindgen_futures::spawn_local(async move {
            let ctx = crate::app::context::context();
            match ctx.mv(&old_path, &new_path).await {
                Ok(_) => {
                    log::info!("Successfully moved '{}' to '{}'", old_path, new_path);
                    // Refresh the file list to update the UI
                    let _ = ctx.list_keys().await;
                }
                Err(e) => {
                    log::error!("Failed to move '{}' to '{}': {}", old_path, new_path, e);
                    // You could show an error notification here
                }
            }
        });
    }

    /// Handle directory move operation from drag and drop
    /// This recursively moves all files and subdirectories within the directory to the new location
    fn handle_directory_move_operation(&mut self, old_dir_path: String, new_dir_path: String) {
        log::info!("Moving directory '{}' to '{}' (recursive)", old_dir_path, new_dir_path);

        // Get all keys to find files in the directory and its subdirectories
        let keys = self.keys.read().unwrap().clone();

        // Find all files that are within the directory being moved (including subdirectories)
        let files_to_move: Vec<_> = keys.iter()
            .filter(|key| {
                let key_path = &key.key;
                // Check if this file is in the directory we're moving or any of its subdirectories
                self.is_file_in_directory(key_path, &old_dir_path)
            })
            .cloned()
            .collect();

        if files_to_move.is_empty() {
            log::info!("No files found in directory '{}' to move", old_dir_path);
            return;
        }

        log::info!("Found {} files to move from directory '{}' (including subdirectories)", files_to_move.len(), old_dir_path);

        // Spawn async task to move all files
        wasm_bindgen_futures::spawn_local(async move {
            let ctx = crate::app::context::context();
            let mut success_count = 0;
            let mut error_count = 0;

            for file in files_to_move {
                let old_file_path = &file.key;

                // Calculate the new file path by replacing the directory prefix
                let new_file_path = Self::calculate_new_file_path(old_file_path, &old_dir_path, &new_dir_path);

                log::info!("Moving file '{}' to '{}'", old_file_path, new_file_path);

                match ctx.mv(old_file_path, &new_file_path).await {
                    Ok(_) => {
                        log::info!("Successfully moved file '{}' to '{}'", old_file_path, new_file_path);
                        success_count += 1;
                    }
                    Err(e) => {
                        log::error!("Failed to move file '{}' to '{}': {}", old_file_path, new_file_path, e);
                        error_count += 1;
                    }
                }
            }

            log::info!("Directory move completed: {} successful, {} failed", success_count, error_count);

            // Refresh the file list to update the UI
            let _ = ctx.list_keys().await;
        });
    }

    /// Check if a file path is within a directory (including subdirectories)
    fn is_file_in_directory(&self, file_path: &str, dir_path: &str) -> bool {
        // Handle exact match (file with same name as directory)
        if file_path == dir_path {
            return true;
        }

        // Handle files within the directory or its subdirectories
        if file_path.starts_with(&format!("{}/", dir_path)) {
            return true;
        }

        false
    }

    /// Calculate the new file path when moving from old_dir_path to new_dir_path
    fn calculate_new_file_path(old_file_path: &str, old_dir_path: &str, new_dir_path: &str) -> String {
        if old_file_path == old_dir_path {
            // This is a file with the exact directory name
            new_dir_path.to_string()
        } else if old_file_path.starts_with(&format!("{}/", old_dir_path)) {
            // This is a file inside the directory or its subdirectories
            // Replace the old directory prefix with the new one, preserving the relative path
            let relative_path = &old_file_path[old_dir_path.len() + 1..];
            format!("{}/{}", new_dir_path, relative_path)
        } else {
            // This shouldn't happen based on our filtering, but handle it gracefully
            log::warn!("Unexpected file path '{}' when moving directory '{}' to '{}'", old_file_path, old_dir_path, new_dir_path);
            old_file_path.to_string()
        }
    }

    /// Draw a preview of the item being dragged
    fn draw_drag_preview(&self, ui: &mut egui::Ui, dragged_path: &str, is_dir: bool, drag_pos: egui::Pos2) {
        // Extract just the filename from the path
        let filename = dragged_path.split('/').last().unwrap_or(dragged_path);

        // Create the preview text with appropriate icon
        let icon = if is_dir { "📁" } else { "📄" };
        let preview_text = format!("{} {}", icon, filename);

        // Calculate text size
        let font_id = egui::FontId::default();
        let text_galley = ui.fonts(|f| f.layout_no_wrap(preview_text.clone(), font_id.clone(), super::theme::MutantColors::TEXT_PRIMARY));

        // Create a semi-transparent background rect
        let padding = egui::Vec2::new(8.0, 4.0);
        let rect_size = text_galley.size() + padding * 2.0;
        let rect = egui::Rect::from_min_size(
            drag_pos + egui::Vec2::new(10.0, -rect_size.y / 2.0), // Offset slightly from cursor
            rect_size
        );

        // Draw background
        ui.painter().rect_filled(
            rect,
            4.0,
            egui::Color32::from_rgba_premultiplied(40, 40, 40, 200) // Dark semi-transparent
        );

        // Draw border
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, super::theme::MutantColors::ACCENT_ORANGE),
            egui::epaint::StrokeKind::Outside
        );

        // Draw text
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            preview_text,
            font_id,
            super::theme::MutantColors::TEXT_PRIMARY
        );
    }

    /// Draw the delete confirmation modal dialog
    fn draw_delete_confirmation_modal(&mut self, ui: &mut egui::Ui) {
        if let Some(ref details) = self.pending_delete.clone() {
            let modal_id = egui::Id::new("delete_confirmation_modal");

            egui::Window::new("Confirm Delete")
                .id(modal_id)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .fixed_size(egui::Vec2::new(400.0, 180.0))
                .show(ui.ctx(), |ui| {
                    // Apply MutAnt theme styling
                    ui.style_mut().visuals.window_fill = super::theme::MutantColors::BACKGROUND_DARK;
                    ui.style_mut().visuals.panel_fill = super::theme::MutantColors::BACKGROUND_DARK;

                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);

                        // Warning icon and title
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("⚠")
                                    .size(24.0)
                                    .color(super::theme::MutantColors::WARNING)
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Delete File")
                                    .size(18.0)
                                    .color(super::theme::MutantColors::TEXT_PRIMARY)
                            );
                        });

                        ui.add_space(15.0);

                        // File name with subtle background
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Are you sure you want to delete:")
                                    .size(14.0)
                                    .color(super::theme::MutantColors::TEXT_SECONDARY)
                            );
                        });

                        ui.add_space(8.0);

                        // File name in a subtle frame
                        egui::Frame::new()
                            .fill(super::theme::MutantColors::BACKGROUND_MEDIUM)
                            .corner_radius(4.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&details.key)
                                        .size(13.0)
                                        .color(super::theme::MutantColors::TEXT_PRIMARY)
                                        .monospace()
                                );
                            });

                        ui.add_space(8.0);

                        ui.label(
                            egui::RichText::new("This action cannot be undone.")
                                .size(12.0)
                                .color(super::theme::MutantColors::TEXT_MUTED)
                                .italics()
                        );

                        ui.add_space(20.0);

                        // Buttons
                        ui.horizontal(|ui| {
                            // Cancel button (left)
                            if ui.add_sized(
                                [120.0, 32.0],
                                egui::Button::new(
                                    egui::RichText::new("Cancel")
                                        .size(14.0)
                                        .color(super::theme::MutantColors::TEXT_PRIMARY)
                                )
                                .fill(super::theme::MutantColors::BACKGROUND_MEDIUM)
                                .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_LIGHT))
                            ).clicked() {
                                self.pending_delete = None;
                            }

                            ui.add_space(20.0);

                            // Delete button (right) - red/danger styling
                            if ui.add_sized(
                                [120.0, 32.0],
                                egui::Button::new(
                                    egui::RichText::new("Delete")
                                        .size(14.0)
                                        .color(egui::Color32::WHITE)
                                )
                                .fill(egui::Color32::from_rgb(180, 40, 40))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 60, 60)))
                            ).clicked() {
                                let key = details.key.clone();
                                self.pending_delete = None;
                                self.perform_delete(key);
                            }
                        });
                    });
                });
        }
    }




}
