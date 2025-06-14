use std::sync::{Arc, RwLock};
use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use mutant_protocol::FileSystemEntry;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

use crate::app::context;
use crate::app::theme::MutantColors;

/// Format file size in human-readable format
fn format_file_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size as u64, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

/// Format Unix timestamp to human-readable date
fn format_modified_time(timestamp: u64) -> String {
    // For web, we'll use a simple format since we can't easily use chrono
    // This is a basic implementation - in a real app you might want to use js_sys
    let now = js_sys::Date::now() / 1000.0; // Current time in seconds
    let diff = now - timestamp as f64;

    if diff < 60.0 {
        "Just now".to_string()
    } else if diff < 3600.0 {
        format!("{:.0}m ago", diff / 60.0)
    } else if diff < 86400.0 {
        format!("{:.0}h ago", diff / 3600.0)
    } else if diff < 2592000.0 {
        format!("{:.0}d ago", diff / 86400.0)
    } else {
        // For older files, show a basic date format
        let date = js_sys::Date::new(&(timestamp as f64 * 1000.0).into());
        format!("{}/{}/{}",
            date.get_month() + 1,
            date.get_date(),
            date.get_full_year()
        )
    }
}

/// A node in the file picker tree
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilePickerNode {
    /// Name of this node (file or directory name)
    pub name: String,
    /// Full path to this node
    pub path: String,
    /// Whether this is a directory
    pub is_directory: bool,
    /// File size (only for files)
    pub size: Option<u64>,
    /// Modified timestamp
    pub modified: Option<u64>,
    /// Child nodes (only for directories)
    pub children: BTreeMap<String, FilePickerNode>,
    /// Whether this directory is expanded
    pub expanded: bool,
    /// Whether this directory has been loaded
    pub loaded: bool,
}

impl FilePickerNode {
    /// Create a new directory node
    pub fn new_dir(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            is_directory: true,
            size: None,
            modified: None,
            children: BTreeMap::new(),
            expanded: false,
            loaded: false,
        }
    }

    /// Create a new file node
    pub fn new_file(entry: &FileSystemEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: entry.path.clone(),
            is_directory: false,
            size: entry.size,
            modified: entry.modified,
            children: BTreeMap::new(),
            expanded: false,
            loaded: true, // Files are always "loaded"
        }
    }

    /// Insert entries from a directory listing into this node
    pub fn insert_entries(&mut self, entries: &[FileSystemEntry]) {
        self.children.clear();
        self.loaded = true;

        for entry in entries {
            if entry.is_directory {
                let child = FilePickerNode::new_dir(&entry.name, &entry.path);
                self.children.insert(entry.name.clone(), child);
            } else {
                let child = FilePickerNode::new_file(entry);
                self.children.insert(entry.name.clone(), child);
            }
        }
    }

    /// Find a node by path
    pub fn find_node_mut(&mut self, path: &str) -> Option<&mut FilePickerNode> {
        if self.path == path {
            return Some(self);
        }

        for child in self.children.values_mut() {
            if let Some(node) = child.find_node_mut(path) {
                return Some(node);
            }
        }

        None
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FilePicker {
    /// Root node of the file tree
    root: Arc<RwLock<FilePickerNode>>,
    /// Currently selected file path
    selected_file: Arc<RwLock<Option<String>>>,
    /// Loading state for directory operations
    is_loading: Arc<RwLock<bool>>,
    /// Error message if any
    error_message: Arc<RwLock<Option<String>>>,
    /// Whether to show only files (not directories for selection)
    files_only: bool,
    /// Whether to show hidden files (files starting with '.')
    show_hidden: Arc<RwLock<bool>>,
}

impl Default for FilePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl FilePicker {
    pub fn new() -> Self {
        // Start with "/" as root - the daemon will map this to the user's home directory
        // This provides security isolation while keeping the web interface simple
        let root_path = "/";

        // Create root node starting from "/" - the name will be updated when we get the response
        let mut root = FilePickerNode::new_dir("Loading...", root_path);
        root.expanded = true; // Expand the root by default

        let picker = Self {
            root: Arc::new(RwLock::new(root)),
            selected_file: Arc::new(RwLock::new(None)),
            is_loading: Arc::new(RwLock::new(false)),
            error_message: Arc::new(RwLock::new(None)),
            files_only: true,
            show_hidden: Arc::new(RwLock::new(false)),
        };

        // Load root directory contents (daemon will resolve to home directory)
        picker.load_directory(root_path);

        picker
    }

    pub fn with_files_only(mut self, files_only: bool) -> Self {
        self.files_only = files_only;
        self
    }

    /// Get the currently selected file path (relative to home directory)
    pub fn selected_file(&self) -> Option<String> {
        self.selected_file.read().unwrap().clone()
    }

    /// Get the currently selected file path as a full filesystem path
    pub fn selected_file_full_path(&self) -> Option<String> {
        if let Some(relative_path) = self.selected_file.read().unwrap().clone() {
            // Get the home directory from the root node name (which was set by the daemon response)
            let root_guard = self.root.read().unwrap();
            let home_directory = &root_guard.name;

            if relative_path == "/" {
                Some(home_directory.clone())
            } else {
                Some(format!("{}{}", home_directory, relative_path))
            }
        } else {
            None
        }
    }

    /// Toggle showing hidden files
    pub fn toggle_hidden_files(&self) {
        let mut show_hidden = self.show_hidden.write().unwrap();
        *show_hidden = !*show_hidden;
        // TODO: Refresh the tree view to apply filter
    }

    /// Check if a file should be shown based on hidden file filter
    fn should_show_entry(&self, entry: &FileSystemEntry) -> bool {
        let show_hidden = *self.show_hidden.read().unwrap();
        if show_hidden {
            true
        } else {
            // Hide files and directories starting with '.'
            !entry.name.starts_with('.')
        }
    }

    /// Load directory contents asynchronously
    fn load_directory(&self, path: &str) {
        let path = path.to_string();
        let root = self.root.clone();
        let is_loading = self.is_loading.clone();
        let error_message = self.error_message.clone();

        *is_loading.write().unwrap() = true;
        *error_message.write().unwrap() = None;

        spawn_local(async move {
            let ctx = context::context();

            match ctx.list_directory(&path).await {
                Ok(response) => {
                    // Update the tree node
                    let mut root_guard = root.write().unwrap();

                    // Check if this is the root path first
                    if path == root_guard.path {
                        // Update the root node name to show the full home directory path
                        root_guard.name = response.home_directory.clone();
                        root_guard.insert_entries(&response.entries);
                    } else if let Some(node) = root_guard.find_node_mut(&path) {
                        node.insert_entries(&response.entries);
                    }
                }
                Err(e) => {
                    *error_message.write().unwrap() = Some(format!("Failed to load directory: {}", e));
                }
            }

            *is_loading.write().unwrap() = false;
        });
    }

    /// Navigate to a directory (expand and load if needed)
    pub fn navigate_to(&self, path: &str) {
        // Find the node and expand it
        let mut root = self.root.write().unwrap();
        if let Some(node) = root.find_node_mut(path) {
            if node.is_directory && !node.loaded {
                drop(root); // Release the lock before async operation
                self.load_directory(path);
            } else if node.is_directory {
                node.expanded = !node.expanded;
            }
        }
    }

    /// Navigate to parent directory (for tree view, this collapses the current level)
    pub fn navigate_up(&self) {
        // In tree view, navigation up is handled by the tree structure itself
        // This method is kept for compatibility but doesn't need to do anything special
    }

    /// Select a file
    pub fn select_file(&self, path: &str) {
        if self.files_only {
            // Only allow file selection, not directories
            spawn_local({
                let path = path.to_string();
                let selected_file = self.selected_file.clone();
                let error_message = self.error_message.clone();

                async move {
                    let ctx = context::context();
                    match ctx.get_file_info(&path).await {
                        Ok(info) => {
                            if info.exists && !info.is_directory {
                                *selected_file.write().unwrap() = Some(path);
                                *error_message.write().unwrap() = None;
                            } else if info.is_directory {
                                *error_message.write().unwrap() = Some("Please select a file, not a directory".to_string());
                            } else {
                                *error_message.write().unwrap() = Some("File does not exist".to_string());
                            }
                        }
                        Err(e) => {
                            *error_message.write().unwrap() = Some(format!("Failed to get file info: {}", e));
                        }
                    }
                }
            });
        } else {
            *self.selected_file.write().unwrap() = Some(path.to_string());
        }
    }



    /// Draw the file picker UI
    pub fn draw(&mut self, ui: &mut egui::Ui) -> bool {
        let mut file_selected = false;

        // Use all available space
        let available_rect = ui.available_rect_before_wrap();

        ui.allocate_ui_with_layout(
            available_rect.size(),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                // Header for file picker - adapt based on files_only mode
                let (header_icon, header_text, description_text) = if self.files_only {
                    ("📁", "Select File", "Choose a file from the daemon's filesystem:")
                } else {
                    ("📂", "Select Folder", "Choose a folder from the daemon's filesystem:")
                };

                ui.heading(RichText::new(format!("{} {}", header_icon, header_text)).size(18.0).color(MutantColors::TEXT_PRIMARY));
                ui.add_space(8.0);
                ui.label(RichText::new(description_text).color(MutantColors::TEXT_SECONDARY));
                ui.add_space(10.0);
                // Professional header with subtle background
                egui::Frame::new()
                    .fill(MutantColors::BACKGROUND_MEDIUM)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Modern file browser icon and title
                            ui.label(
                                RichText::new("📁")
                                    .size(16.0)
                                    .color(MutantColors::ACCENT_BLUE)
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("File Browser")
                                    .size(14.0)
                                    .color(MutantColors::TEXT_PRIMARY)
                                    .strong()
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Sleek toggle button for hidden files
                                let show_hidden = *self.show_hidden.read().unwrap();
                                let (icon, text, color) = if show_hidden {
                                    ("👁", "Hidden", MutantColors::ACCENT_GREEN)
                                } else {
                                    ("", "Hidden", MutantColors::TEXT_MUTED)
                                };

                                let button = egui::Button::new(
                                    RichText::new(format!("{} {}", icon, text))
                                        .size(11.0)
                                        .color(color)
                                )
                                .fill(if show_hidden {
                                    MutantColors::ACCENT_GREEN.gamma_multiply(0.1)
                                } else {
                                    MutantColors::BACKGROUND_DARK
                                })
                                .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.3)))
                                .corner_radius(4.0);

                                if ui.add(button).clicked() {
                                    self.toggle_hidden_files();
                                }
                            });
                        });
                    });

                // Subtle separator line
                ui.add_space(1.0);
                ui.separator();
                ui.add_space(1.0);

                // Error message with professional styling
                if let Some(error) = &*self.error_message.read().unwrap() {
                    egui::Frame::new()
                        .fill(MutantColors::ERROR.gamma_multiply(0.1))
                        .stroke(egui::Stroke::new(1.0, MutantColors::ERROR.gamma_multiply(0.3)))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("⚠").color(MutantColors::ERROR).size(14.0));
                                ui.add_space(6.0);
                                ui.label(RichText::new(error).color(MutantColors::ERROR).size(12.0));
                            });
                        });
                    ui.add_space(8.0);
                }

                // Loading indicator with professional styling
                if *self.is_loading.read().unwrap() {
                    egui::Frame::new()
                        .fill(MutantColors::ACCENT_BLUE.gamma_multiply(0.1))
                        .stroke(egui::Stroke::new(1.0, MutantColors::ACCENT_BLUE.gamma_multiply(0.3)))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.add_space(6.0);
                                ui.label(RichText::new("Loading directory...").color(MutantColors::ACCENT_BLUE).size(12.0));
                            });
                        });
                    return;
                }

                // Compact file tree with full available height
                let remaining_height = ui.available_height(); // Use ALL available height

                egui::Frame::new()
                    .fill(MutantColors::BACKGROUND_DARK)
                    .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_LIGHT))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::same(2)) // Reduced from 4 to 2
                    .show(ui, |ui| {
                        // Set compact spacing for the entire tree
                        ui.spacing_mut().item_spacing.y = 0.0; // Minimal spacing between items
                        ui.spacing_mut().indent = 12.0;

                        egui::ScrollArea::vertical()
                            .min_scrolled_height(remaining_height)
                            .max_height(remaining_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                // Ensure compact spacing in scroll area
                                ui.spacing_mut().item_spacing.y = 0.0;
                                // Clone necessary data to avoid borrowing issues
                                let show_hidden = *self.show_hidden.read().unwrap();
                                let selected_file = self.selected_file.read().unwrap().clone();

                                let mut root = self.root.write().unwrap();
                                file_selected = Self::draw_tree_node_static(
                                    ui,
                                    &mut *root,
                                    0,
                                    show_hidden,
                                    &selected_file,
                                    &self.selected_file,
                                    &self.is_loading,
                                    &self.error_message,
                                    self.files_only
                                );

                                // Check for expanded but unloaded directories and load them
                                let paths_to_load = Self::collect_unloaded_expanded_paths(&*root);
                                drop(root); // Release the lock before async operations

                                for path in paths_to_load {
                                    self.load_directory(&path);
                                }
                            });
                    });

                // No selection confirmation needed - files are selected immediately
            },
        );

        file_selected
    }

    /// Draw a tree node and its children (static version to avoid borrowing issues)
    fn draw_tree_node_static(
        ui: &mut egui::Ui,
        node: &mut FilePickerNode,
        indent_level: usize,
        show_hidden: bool,
        selected_file: &Option<String>,
        selected_file_arc: &Arc<RwLock<Option<String>>>,
        is_loading: &Arc<RwLock<bool>>,
        error_message: &Arc<RwLock<Option<String>>>,
        files_only: bool
    ) -> bool {
        let mut file_selected = false;

        // Skip hidden files if needed
        if !show_hidden && node.name.starts_with('.') {
            return false;
        }

        // Skip files if we're in directory-only mode
        if !files_only && !node.is_directory {
            return false;
        }

        // Compact indentation with maximum depth limit to prevent exponential growth
        let indent_per_level = 8.0; // Reduced from 12.0
        let max_indent = 80.0; // Maximum indentation to prevent excessive nesting
        let total_indent = (indent_per_level * (indent_level as f32)).min(max_indent);

        // Compact layout with minimal spacing
        ui.spacing_mut().item_spacing.y = 0.0; // Minimal vertical spacing
        ui.spacing_mut().indent = 8.0; // Compact indentation

        // Standard row height for both files and folders
        let row_height = 18.0;

        ui.horizontal(|ui| {
            // Apply indentation
            ui.add_space(total_indent);

            if node.is_directory {
                // Check if this directory is selected (only relevant when not in files_only mode)
                let is_selected = !files_only && selected_file
                    .as_ref()
                    .map_or(false, |selected| selected == &node.path);

                // Custom compact directory styling to match file rows
                let (folder_icon, icon_color) = if node.expanded {
                    ("📂", MutantColors::ACCENT_ORANGE)
                } else {
                    ("📁", MutantColors::ACCENT_BLUE)
                };

                // Expand/collapse arrow - using more reliable characters
                let arrow_icon = if node.expanded { "v" } else { ">" };

                // Use same button-like approach as files for consistent spacing
                let button_response = ui.allocate_response(
                    egui::Vec2::new(ui.available_width(), row_height),
                    egui::Sense::click()
                );

                let row_rect = button_response.rect;

                // Selection background for directories (when not in files_only mode)
                if is_selected {
                    ui.painter().rect_filled(
                        row_rect,
                        3.0,
                        MutantColors::ACCENT_BLUE.gamma_multiply(0.15)
                    );
                    ui.painter().rect_stroke(
                        row_rect,
                        3.0,
                        egui::Stroke::new(1.0, MutantColors::ACCENT_BLUE.gamma_multiply(0.6)),
                        egui::epaint::StrokeKind::Outside
                    );
                } else if button_response.hovered() {
                    ui.painter().rect_filled(
                        row_rect,
                        3.0,
                        MutantColors::BACKGROUND_MEDIUM.gamma_multiply(0.5)
                    );
                }

                // Draw directory content with compact spacing - properly aligned with selection background
                let font_id = egui::FontId::new(10.0, egui::FontFamily::Monospace); // Use monospace for better arrow rendering
                let icon_font_id = egui::FontId::new(11.0, egui::FontFamily::Proportional); // Use proportional for icons

                // Calculate proper vertical center position for the text within the row
                let text_y = row_rect.center().y;
                let text_pos = egui::Pos2::new(row_rect.left() + 4.0, text_y);

                // Draw expand/collapse arrow with better styling
                ui.painter().text(
                    text_pos,
                    egui::Align2::LEFT_CENTER,
                    arrow_icon,
                    font_id.clone(),
                    MutantColors::ACCENT_BLUE // More visible color
                );

                // Draw folder icon
                let icon_pos = egui::Pos2::new(text_pos.x + 12.0, text_y);
                ui.painter().text(
                    icon_pos,
                    egui::Align2::LEFT_CENTER,
                    folder_icon,
                    icon_font_id.clone(),
                    icon_color
                );

                // Draw folder name with selection indicator
                let name_pos = egui::Pos2::new(icon_pos.x + 16.0, text_y);
                let folder_name = if files_only {
                    format!("{}/", node.name)
                } else {
                    // In directory selection mode, add visual indicator that it's selectable
                    if is_selected {
                        format!("✓ {}/", node.name)
                    } else {
                        format!("{}/", node.name)
                    }
                };

                let name_color = if is_selected {
                    MutantColors::ACCENT_ORANGE
                } else {
                    icon_color
                };

                ui.painter().text(
                    name_pos,
                    egui::Align2::LEFT_CENTER,
                    folder_name,
                    icon_font_id,
                    name_color
                );

                // Handle directory interaction
                if button_response.clicked() {
                    if files_only {
                        // In files_only mode, just expand/collapse directories
                        node.expanded = !node.expanded;
                    } else {
                        // In directory selection mode, single click selects, double click expands
                        *selected_file_arc.write().unwrap() = Some(node.path.clone());
                        *error_message.write().unwrap() = None;
                        file_selected = true;
                    }
                }

                // Handle double-click to expand/collapse in directory selection mode
                if !files_only && button_response.double_clicked() {
                    node.expanded = !node.expanded;
                }

                if button_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            } else {
                // Compact file node styling - no extra spacing, files should align with folder content

                let is_selected = selected_file
                    .as_ref()
                    .map_or(false, |selected| selected == &node.path);

                // Determine file icon and colors based on extension
                let (file_icon, icon_color, bg_color) = Self::get_file_styling(&node.name, is_selected);

                let filename_color = if is_selected {
                    MutantColors::ACCENT_ORANGE
                } else {
                    MutantColors::TEXT_PRIMARY
                };

                // Use a more reliable button-like approach for single-click detection
                let button_response = ui.allocate_response(
                    egui::Vec2::new(ui.available_width(), 18.0),
                    egui::Sense::click()
                );

                let row_rect = button_response.rect;

                // Compact selection background
                if is_selected {
                    ui.painter().rect_filled(
                        row_rect,
                        3.0, // Reduced corner radius
                        bg_color
                    );
                    ui.painter().rect_stroke(
                        row_rect,
                        3.0,
                        egui::Stroke::new(1.0, MutantColors::ACCENT_ORANGE.gamma_multiply(0.6)),
                        egui::epaint::StrokeKind::Outside
                    );
                } else if button_response.hovered() {
                    ui.painter().rect_filled(
                        row_rect,
                        3.0,
                        MutantColors::BACKGROUND_MEDIUM.gamma_multiply(0.5)
                    );
                }

                // Draw file icon and name with compact spacing - properly aligned with selection background
                let font_id = egui::FontId::new(11.0, egui::FontFamily::Proportional);

                // Calculate proper vertical center position for the text within the row
                let text_y = row_rect.center().y;
                let text_pos = egui::Pos2::new(row_rect.left() + 4.0, text_y);

                // Draw icon with specific color
                ui.painter().text(
                    text_pos,
                    egui::Align2::LEFT_CENTER,
                    file_icon,
                    font_id.clone(),
                    icon_color
                );

                // Draw filename with compact spacing
                let filename_pos = egui::Pos2::new(text_pos.x + 16.0, text_y);
                ui.painter().text(
                    filename_pos,
                    egui::Align2::LEFT_CENTER,
                    &node.name,
                    font_id,
                    filename_color
                );

                // Compact file stats on the right
                if let Some(size) = node.size {
                    let size_text = format_file_size(size);
                    let size_pos = egui::Pos2::new(row_rect.right() - 4.0, text_y);
                    ui.painter().text(
                        size_pos,
                        egui::Align2::RIGHT_CENTER,
                        size_text,
                        egui::FontId::new(10.0, egui::FontFamily::Monospace), // Smaller font
                        MutantColors::TEXT_MUTED
                    );
                }

                // Handle file selection - immediately advance to next step
                // Simple single-click detection
                if button_response.clicked() {
                    if files_only {
                        // For files_only mode, immediately select the file since we know it's a file from the tree
                        // The tree already shows only files, so no need for async validation
                        *selected_file_arc.write().unwrap() = Some(node.path.clone());
                        *error_message.write().unwrap() = None;
                        file_selected = true; // Immediately signal that a file was selected
                    } else {
                        // For directory mode, allow any selection
                        *selected_file_arc.write().unwrap() = Some(node.path.clone());
                        file_selected = true; // Immediately signal that a file was selected
                    }
                }

                if button_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
        });

        // Draw children for expanded directories (outside the horizontal layout)
        if node.is_directory && node.expanded {
            // Ensure compact spacing for children
            ui.spacing_mut().item_spacing.y = 0.0;

            // Draw children
            let mut sorted_children: Vec<_> = node.children.iter_mut().collect();
            sorted_children.sort_by(|(_, a), (_, b)| {
                match (a.is_directory, b.is_directory) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                }
            });

            for (_, child) in sorted_children {
                // Skip files if we're in directory-only mode
                if !files_only && !child.is_directory {
                    continue;
                }

                if Self::draw_tree_node_static(
                    ui,
                    child,
                    indent_level + 1,
                    show_hidden,
                    selected_file,
                    selected_file_arc,
                    is_loading,
                    error_message,
                    files_only
                ) {
                    file_selected = true;
                }
            }
        }

        file_selected
    }

    /// Collect paths of directories that are expanded but not loaded
    fn collect_unloaded_expanded_paths(node: &FilePickerNode) -> Vec<String> {
        let mut paths = Vec::new();

        if node.is_directory && node.expanded && !node.loaded {
            paths.push(node.path.clone());
        }

        // Recursively check children
        for child in node.children.values() {
            paths.extend(Self::collect_unloaded_expanded_paths(child));
        }

        paths
    }

    /// Get professional file styling based on file extension
    fn get_file_styling(filename: &str, is_selected: bool) -> (&'static str, egui::Color32, egui::Color32) {
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        let (icon, base_color) = match extension.as_str() {
            // Code files
            "rs" | "rust" => ("🦀", MutantColors::ACCENT_ORANGE),
            "js" | "ts" | "jsx" | "tsx" => ("📜", MutantColors::WARNING),
            "py" | "python" => ("🐍", MutantColors::ACCENT_GREEN),
            "html" | "htm" => ("🌐", MutantColors::ACCENT_BLUE),
            "css" | "scss" | "sass" => ("🎨", MutantColors::ACCENT_BLUE),
            "json" | "yaml" | "yml" | "toml" => ("⚙", MutantColors::TEXT_MUTED),

            // Documents
            "md" | "markdown" => ("📝", MutantColors::TEXT_PRIMARY),
            "txt" | "log" => ("📄", MutantColors::TEXT_MUTED),
            "pdf" => ("📕", MutantColors::ERROR),

            // Images
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => ("🖼", MutantColors::ACCENT_GREEN),

            // Videos
            "mp4" | "avi" | "mkv" | "mov" | "webm" => ("🎬", MutantColors::ACCENT_ORANGE),

            // Archives
            "zip" | "tar" | "gz" | "rar" | "7z" => ("📦", MutantColors::WARNING),

            // Executables
            "exe" | "bin" | "app" => ("⚡", MutantColors::ERROR),

            // Default
            _ => ("📄", MutantColors::TEXT_MUTED),
        };

        let bg_color = if is_selected {
            base_color.gamma_multiply(0.15)
        } else {
            MutantColors::BACKGROUND_DARK
        };

        (icon, base_color, bg_color)
    }
}


