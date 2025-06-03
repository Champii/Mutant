use std::sync::{Arc, RwLock, Mutex};

use eframe::egui::{self, RichText};
use egui_dock::TabIndex; // Ensure TabIndex is imported

use mutant_protocol::KeyDetails;
use serde::{Deserialize, Serialize};

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
    /// Unique dock area ID for the internal dock area within this FsWindow
    #[serde(skip)]
    dock_area_id: String,
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
            dock_area_id: generate_unique_dock_area_id(),
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
    fn add_file_tab(&mut self, file_details: KeyDetails) {
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
                                    file_tab.downloaded_bytes = downloaded_bytes;
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
    pub fn add_put_tab(&mut self) {
        log::info!("FsWindow: Creating new Put window tab");

        // Check if a Put tab already exists
        let tab_exists = self.internal_dock.iter_all_tabs().any(|(_, existing_tab)| {
            matches!(existing_tab, crate::app::fs::internal_tab::FsInternalTab::Put(_))
        });

        if !tab_exists {
            // Create a new Put window
            let put_window = PutWindow::new();
            let tab = crate::app::fs::internal_tab::FsInternalTab::Put(put_window);

            // Add to the internal dock system
            if self.internal_dock.iter_all_tabs().next().is_none() {
                // If the dock is empty, create a new surface
                self.internal_dock = egui_dock::DockState::new(vec![tab]);
            } else {
                // Add to the existing surface
                self.internal_dock.main_surface_mut().push_to_focused_leaf(tab);
            }

            log::info!("FsWindow: Successfully added Put tab to internal dock");
        } else {
            log::info!("FsWindow: Put tab already exists in internal dock");
        }
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

    /// Handles the main "Upload" action from the application menu.
    /// It will either find an existing PutWindow tab and trigger file selection,
    /// or create a new PutWindow tab and then trigger file selection.
    pub fn handle_upload_action(&mut self) {
        log::info!("FsWindow: Handling upload action, preparing to trigger file selection.");
        let mut put_window_found_and_triggered = false;
        let mut found_tab_id: Option<TabIndex> = None;

        // Iterate to find an existing PutWindow tab
        for (tab_id, tab) in self.internal_dock.iter_all_tabs_mut() {
            if let super::fs::internal_tab::FsInternalTab::Put(ref mut put_window) = tab {
                put_window.trigger_file_selection_and_prepare_window();
                put_window_found_and_triggered = true;
                found_tab_id = Some(tab_id);
                log::info!("FsWindow: Found existing PutWindow tab ({:?}), triggered file selection.", tab_id);
                break;
            }
        }

        // If an existing tab was found and triggered, focus it
        if let Some(tab_id) = found_tab_id {
            self.internal_dock.set_active_tab(tab_id);
            log::info!("FsWindow: Focused existing PutWindow tab ({:?}).", tab_id);
        }

        // If no PutWindow was found, create and add a new one
        if !put_window_found_and_triggered {
            log::info!("FsWindow: No existing PutWindow tab found. Creating a new one.");
            let mut new_put_window = super::put::PutWindow::new(); // Path should be correct
            new_put_window.trigger_file_selection_and_prepare_window();

            let tab = super::fs::internal_tab::FsInternalTab::Put(new_put_window); // Path should be correct

            if self.internal_dock.iter_all_tabs().next().is_none() {
                self.internal_dock = egui_dock::DockState::new(vec![tab]);
                log::info!("FsWindow: Created new dock with PutWindow tab because no tabs existed.");
            } else {
                self.internal_dock.main_surface_mut().push_to_focused_leaf(tab);
                log::info!("FsWindow: Added PutWindow tab to main surface's focused leaf.");
                // Focusing might require finding the tab_id after adding,
                // but push_to_focused_leaf often makes it active.
            }
        }
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
        let needs_rebuild = self.root.children.is_empty() ||
                           self.count_tree_files(&self.root) != current_key_count;

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

                // Draw the root folder '/' as a proper collapsible folder
                let mut root_expanded = !self.root.children.is_empty(); // Default to expanded if there are children

                // Draw the root folder with refresh button positioned like a file's download button
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

                    // Root folder as collapsible header
                    let icon = if root_expanded { "📂" } else { "📁" };
                    let text = RichText::new(format!("{} /", icon))
                        .size(12.0)  // Match other folder sizes
                        .color(super::theme::MutantColors::ACCENT_ORANGE);  // Special distinguishing color for folders

                    let header = egui::CollapsingHeader::new(text)
                        .id_salt(format!("mutant_fs_root_{}", self.window_id))
                        .default_open(root_expanded);

                    root_expanded = header.show(ui, |ui| {
                        // Sort children: directories first, then files
                        let mut sorted_children: Vec<_> = self.root.children.iter_mut().collect();
                        sorted_children.sort_by(|(_, a), (_, b)| {
                            match (a.is_dir(), b.is_dir()) {
                                (true, false) => std::cmp::Ordering::Less,    // Directories come before files
                                (false, true) => std::cmp::Ordering::Greater, // Files come after directories
                                _ => a.name.cmp(&b.name),                     // Sort alphabetically within each group
                            }
                        });

                        // Draw the sorted children with indentation level 1 (since they are children of the root '/')
                        for (_, child) in sorted_children {
                            let (view_details, download_details) = child.ui(ui, 1, selected_path_ref, &self.window_id);
                            if view_details.is_some() {
                                view_details_clicked = view_details;
                            }
                            if download_details.is_some() {
                                download_details_clicked = download_details;
                            }
                        }
                    }).header_response.clicked() || root_expanded;
                });

                // Draw refresh button at the same position as file download buttons
                let text_baseline_y = row_rect.top() + (row_rect.height() - 12.0) / 2.0;
                let button_width = 20.0;
                let current_x = row_rect.right() - 4.0; // Start with right padding

                let button_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(current_x - button_width, text_baseline_y),
                    egui::Vec2::new(button_width, 12.0)
                );

                // Draw refresh button manually
                let button_response = ui.allocate_rect(button_rect, egui::Sense::click());
                if button_response.clicked() {
                    // Trigger a refresh of the file list
                    let ctx = crate::app::context::context();
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = ctx.list_keys().await;
                    });
                }
                if button_response.hovered() {
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



                // Add some bottom padding
                ui.add_space(8.0);

                (view_details_clicked, download_details_clicked)
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
    }




}
