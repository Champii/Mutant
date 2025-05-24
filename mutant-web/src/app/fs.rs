use std::sync::{Arc, RwLock};
use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText};
use egui_dock::DockState;
use humansize::{format_size, BINARY};
use mutant_protocol::KeyDetails;
use serde::{Deserialize, Serialize};
use base64::Engine;

use super::components::multimedia;
use super::Window;

/// Helper function to format file sizes in a human-readable way
fn humanize_size(size: usize) -> String {
    format_size(size, BINARY)
}

/// A node in the filesystem tree
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct TreeNode {
    /// Name of this node (file or directory name)
    name: String,

    /// If this is a file, contains the key details
    key_details: Option<KeyDetails>,

    /// If this is a directory, contains child nodes
    children: BTreeMap<String, TreeNode>,

    /// Whether this directory is expanded (only relevant for directories)
    expanded: bool,

    /// Full path to this node (for debugging and unique IDs)
    path: String,
}

impl TreeNode {
    /// Create a new directory node
    fn new_dir(name: &str) -> Self {
        Self {
            name: name.to_string(),
            key_details: None,
            children: BTreeMap::new(),
            expanded: false,
            path: name.to_string(),
        }
    }

    /// Create a new file node
    fn new_file(name: &str, key_details: KeyDetails) -> Self {
        Self {
            name: name.to_string(),
            key_details: Some(key_details),
            children: BTreeMap::new(),
            expanded: false,
            path: name.to_string(),
        }
    }

    /// Check if this node is a directory
    fn is_dir(&self) -> bool {
        self.key_details.is_none()
    }

    /// Insert a key into the tree at the appropriate location
    fn insert_key(&mut self, path_parts: &[&str], key_details: KeyDetails, parent_path: &str) {
        if path_parts.is_empty() {
            return;
        }

        let current = path_parts[0];
        let current_path = if parent_path.is_empty() {
            current.to_string()
        } else {
            format!("{}/{}", parent_path, current)
        };

        if path_parts.len() == 1 {
            // This is a file (leaf node)
            let mut file_node = TreeNode::new_file(current, key_details);
            file_node.path = current_path;
            self.children.insert(current.to_string(), file_node);
        } else {
            // This is a directory
            let dir_node = self.children
                .entry(current.to_string())
                .or_insert_with(|| {
                    let mut node = TreeNode::new_dir(current);
                    node.path = current_path.clone();
                    node
                });

            dir_node.insert_key(&path_parts[1..], key_details, &current_path);
        }
    }

    /// Draw this node and its children
    /// Returns (clicked, key_details) if a file in this node or its children was clicked
    fn ui(&mut self, ui: &mut egui::Ui, indent_level: usize, selected_path: Option<&str>) -> (bool, Option<KeyDetails>) {
        // Use a very small fixed indentation to avoid exponential growth
        let indent_per_level = 2.0;
        let total_indent = indent_per_level * (indent_level as f32);

        let mut file_clicked = false;
        let mut clicked_details = None;

        ui.horizontal(|ui| {
            // Apply the base indentation
            ui.add_space(total_indent);

            if self.is_dir() {
                // Directory node
                let icon = if self.expanded { "📂" } else { "📁" };
                let text = format!("{} {}/", icon, self.name); // Add '/' at the end of folder names

                let header = egui::CollapsingHeader::new(text)
                    .id_salt(format!("dir_{}", self.path)) // Use full path for unique ID
                    .default_open(self.expanded);

                let mut child_clicked = false;
                let mut child_details = None;

                self.expanded = header.show(ui, |ui| {
                    // Sort children: directories first, then files
                    let mut sorted_children: Vec<_> = self.children.iter_mut().collect();
                    sorted_children.sort_by(|(_, a), (_, b)| {
                        match (a.is_dir(), b.is_dir()) {
                            (true, false) => std::cmp::Ordering::Less,    // Directories come before files
                            (false, true) => std::cmp::Ordering::Greater, // Files come after directories
                            _ => a.name.cmp(&b.name),                     // Sort alphabetically within each group
                        }
                    });

                    // Draw the sorted children with exactly one more level of indentation
                    for (_, child) in sorted_children {
                        let (clicked, details) = child.ui(ui, indent_level + 1, selected_path);
                        if clicked {
                            child_clicked = true;
                            child_details = details;
                        }
                    }
                }).header_response.clicked() || self.expanded;

                // Propagate click from children
                if child_clicked {
                    file_clicked = true;
                    clicked_details = child_details;
                }
            } else {
                // File node - add extra space to align with folder names (accounting for arrow)
                ui.add_space(18.0); // Add space to compensate for the arrow in front of folders

                let icon = "📄";
                let details = self.key_details.as_ref().unwrap();

                // Check if this file is selected
                let is_selected = selected_path.map_or(false, |path| path == &details.key);

                // Style the text based on selection and public status
                let text = if details.is_public {
                    let text = RichText::new(format!("{} {} (public)", icon, self.name))
                        .color(Color32::from_rgb(0, 128, 0));

                    if is_selected {
                        // text = text.strong().background_color(Color32::from_rgb(30, 30, 30));
                    }

                    text
                } else {
                    let text = RichText::new(format!("{} {}", icon, self.name));

                    // Commented out selection styling
                    // let text = if is_selected {
                    //     text.strong().background_color(Color32::from_rgb(30, 30, 30))
                    // } else {
                    //     text
                    // };

                    text
                };

                // Make the file node clickable
                if ui.selectable_label(is_selected, text).clicked() {
                    // Set file_clicked to true and return the details
                    file_clicked = true;
                    clicked_details = Some(details.clone());
                }
            }
        });

        // Return whether a file was clicked and the details if available
        (file_clicked, clicked_details)
    }
}

/// A tab in the file viewer area
#[derive(Clone, Serialize, Deserialize)]
pub struct FileViewerTab {
    /// File details
    pub file: KeyDetails,
    /// Content of the file
    pub content: String,
    /// Whether we're currently loading file content
    pub is_loading: bool,
    /// File content as binary data for multimedia
    #[serde(skip)]
    pub file_binary: Option<Vec<u8>>,
    /// Current file type
    #[serde(skip)]
    pub file_type: Option<multimedia::FileType>,
    /// Image texture for image files
    #[serde(skip)]
    pub image_texture: Option<eframe::egui::TextureHandle>,
    /// Video URL for video files
    #[serde(skip)]
    pub video_url: Option<String>,
    /// Whether the file content has been modified
    #[serde(skip)]
    pub file_modified: bool,
    /// Processed file content for multimedia components
    #[serde(skip)]
    pub file_content: Option<multimedia::FileContent>,
}

impl FileViewerTab {
    /// Create a new file viewer tab
    pub fn new(file: KeyDetails) -> Self {
        // Initial content while loading
        let initial_content = "Loading file content...".to_string();

        // Create the FileContent immediately with default values
        // This will be updated when the actual content is loaded
        let file_content = multimedia::FileContent {
            file_type: multimedia::FileType::Text, // Default to text
            raw_data: initial_content.as_bytes().to_vec(),
            editable_content: Some(initial_content.clone()),
            content_modified: false,
            image_texture: None,
            video_url: None,
        };

        Self {
            file,
            content: initial_content,
            is_loading: true,
            file_binary: None,
            file_type: None,
            image_texture: None,
            video_url: None,
            file_modified: false,
            file_content: Some(file_content),
        }
    }

    /// Draw the file viewer tab
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        // Show file details at the top
        ui.heading(&self.file.key);

        // Store file details we need for display
        let file_size = humanize_size(self.file.total_size);
        let pad_count = self.file.pad_count;
        let confirmed_pads = self.file.confirmed_pads;
        let is_public = self.file.is_public;
        let public_address = self.file.public_address.clone();

        // Store current state
        let file_type = self.file_type.clone();
        let file_modified = self.file_modified;

        ui.horizontal(|ui| {
            ui.label(format!("Size: {}", file_size));
            ui.separator();
            ui.label(format!("Pads: {}/{}", confirmed_pads, pad_count));
            ui.separator();
            if is_public {
                ui.label(RichText::new("Public").color(Color32::from_rgb(0, 128, 0)));
                if let Some(addr) = &public_address {
                    ui.separator();
                    ui.label(format!("Address: {}", addr));
                }
            } else {
                ui.label("Private");
            }

            // Show file type if available
            if let Some(file_type) = &file_type {
                ui.separator();
                match file_type {
                    multimedia::FileType::Text => {
                        ui.label("Type: Text");
                    },
                    multimedia::FileType::Code(lang) => {
                        ui.label(format!("Type: Code ({})", lang));
                    },
                    multimedia::FileType::Image => {
                        ui.label("Type: Image");
                    },
                    multimedia::FileType::Video => {
                        ui.label("Type: Video");
                    },
                    multimedia::FileType::Other => {
                        ui.label("Type: Unknown");
                    },
                }
            }

            // Show modified indicator and save button if the file has been modified
            if file_modified {
                ui.separator();
                ui.label(RichText::new("Modified").color(Color32::YELLOW));

                if ui.button("Save").clicked() {
                    // Save the file
                    self.save_file(ui);
                }
            }
        });

        ui.separator();

        // Show loading indicator if we're loading content
        if self.is_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading file content...");
            });

            // Check if we have a progress object for this file
            let ctx = crate::app::context::context();
            let get_id = format!("get_{}", self.file.key);

            if let Some(progress) = ctx.get_get_progress(&get_id) {
                let progress_guard = progress.read().unwrap();

                // Check if we have a get operation in progress
                if let Some(op) = progress_guard.operation.get("get") {
                    if op.total_pads > 0 {
                        // Calculate progress percentage
                        let progress_value = op.nb_reserved as f32 / op.total_pads as f32;

                        // Show progress bar
                        ui.add_space(4.0);
                        ui.label(format!("Downloaded {} of {} pads", op.nb_reserved, op.total_pads));
                        ui.add_space(4.0);
                        let progress_bar = egui::ProgressBar::new(progress_value)
                            .show_percentage()
                            .animate(true);
                        ui.add(progress_bar);
                    }
                }
            }

            return;
        }

        // We should never need to initialize file_content here
        // This is a fallback for backward compatibility only
        if self.file_content.is_none() {
            log::warn!("FileContent not initialized for tab: {}", self.file.key);
        }

        // Display content based on file type
        if let Some(file_type) = &self.file_type {
            match file_type {
                multimedia::FileType::Text => {
                    // Use the stored FileContent
                    if let Some(file_content) = &mut self.file_content {
                        // Text viewer with syntax highlighting
                        multimedia::draw_text_viewer(ui, file_content);

                        // Check if content was modified
                        if file_content.content_modified {
                            // Update our content directly from the editable_content
                            if let Some(text) = &file_content.editable_content {
                                self.content = text.clone();
                                self.file_modified = true;
                                file_content.content_modified = false;

                                // Update the raw_data to match the new content
                                // file_content.raw_data = self.content.as_bytes().to_vec();
                            }
                        }
                    } else {
                        ui.label("Error: File content not available");
                    }
                },
                multimedia::FileType::Code(lang) => {
                    // Use the stored FileContent
                    if let Some(file_content) = &mut self.file_content {
                        // Code viewer with syntax highlighting
                        multimedia::draw_code_viewer(ui, file_content, lang);

                        // Check if content was modified
                        if file_content.content_modified {
                            // Update our content directly from the editable_content
                            if let Some(text) = &file_content.editable_content {
                                self.content = text.clone();
                                self.file_modified = true;
                                file_content.content_modified = false;

                                // Update the raw_data to match the new content
                                // file_content.raw_data = self.content.as_bytes().to_vec();
                            }
                        }
                    } else {
                        ui.label("Error: File content not available");
                    }
                },
                multimedia::FileType::Image => {
                    // Image viewer
                    if let Some(texture) = &self.image_texture {
                        multimedia::draw_image_viewer(ui, texture);
                    } else {
                        ui.label("Error: Image texture not loaded");
                    }
                },
                multimedia::FileType::Video => {
                    // Video player
                    if let Some(video_url) = &self.video_url {
                        multimedia::draw_video_player(ui, video_url);
                    } else {
                        ui.label("Error: Video URL not available");
                    }
                },
                multimedia::FileType::Other => {
                    // Unsupported file type with download option
                    let file_data = self.file_binary.as_deref();
                    let filename = std::path::Path::new(&self.file.key)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| self.file.key.clone());
                    multimedia::draw_unsupported_file(ui, file_data, &filename);
                },
            }
        } else {
            // Fallback to text display if file type is not determined
            if let Some(file_content) = &mut self.file_content {
                multimedia::draw_text_viewer(ui, file_content);

                // Check if content was modified
                if file_content.content_modified {
                    // Update our content directly from the editable_content
                    if let Some(text) = &file_content.editable_content {
                        self.content = text.clone();
                        self.file_modified = true;
                        file_content.content_modified = false;

                        // Update the raw_data to match the new content
                        // file_content.raw_data = self.content.as_bytes().to_vec();
                    }
                }
            } else {
                ui.label("No file content available");
            }
        }
    }

    /// Save the file content
    fn save_file(&mut self, ui: &mut egui::Ui) {
        let key = self.file.key.clone();
        let is_public = self.file.is_public;
        let content = self.content.clone();

        // Start loading
        self.is_loading = true;

        // Reset the modified flag
        self.file_modified = false;

        // Spawn a task to save the file
        let window_id = ui.id();
        wasm_bindgen_futures::spawn_local(async move {
            let ctx = crate::app::context::context();

            // Convert the content to bytes
            let data = content.into_bytes();

            // Get the filename from the key
            let filename = std::path::Path::new(&key)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| key.clone());

            // Save the file using the context's put method with all required parameters
            match ctx.put(
                &key,
                data,
                &filename,
                mutant_protocol::StorageMode::Heaviest,
                is_public,
                false, // no_verify
                None,  // progress
            ).await {
                Ok(_) => {
                    // Get a mutable reference to the window system
                    let mut window_system = crate::app::window_system::window_system_mut();

                    // Find our window and update it
                    if let Some(window) = window_system.get_window_mut::<FsWindow>(window_id) {
                        // Find the tab with this file and update it
                        if let Some(tab) = window.find_tab_mut(&key) {
                            tab.is_loading = false;
                        }

                        // Refresh the key list
                        let _ = ctx.list_keys().await;
                        window.build_tree();
                    }
                },
                Err(e) => {
                    // Get a mutable reference to the window system
                    let mut window_system = crate::app::window_system::window_system_mut();

                    // Find our window and update it
                    if let Some(window) = window_system.get_window_mut::<FsWindow>(window_id) {
                        // Find the tab with this file and update it
                        if let Some(tab) = window.find_tab_mut(&key) {
                            tab.is_loading = false;
                            tab.file_modified = true; // Keep the modified flag since save failed
                            tab.content = format!("Error saving file: {}", e);
                        }
                    }
                }
            }
        });
    }
}

/// Tab viewer for the file viewer area
#[derive(Clone, Serialize, Deserialize)]
struct FileViewerTabViewer {}

impl egui_dock::TabViewer for FileViewerTabViewer {
    type Tab = FileViewerTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        // Get the file name from the path
        let file_name = std::path::Path::new(&tab.file.key)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| tab.file.key.clone());

        // Add an icon based on the file type
        let icon = match &tab.file_type {
            Some(multimedia::FileType::Text) => "📄",
            Some(multimedia::FileType::Code(_)) => "📝",
            Some(multimedia::FileType::Image) => "🖼️",
            Some(multimedia::FileType::Video) => "🎬",
            Some(multimedia::FileType::Other) | None => "📄",
        };

        // Add a modified indicator if the file has been modified
        let modified_indicator = if tab.file_modified { "* " } else { "" };

        // Create a compact title that doesn't expand to fill all space
        let title = format!("{}{}{}", modified_indicator, icon, file_name);
        egui::RichText::new(title).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        tab.draw(ui);
    }

    // Allow all tabs to be closable
    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
    }
}

/// The filesystem tree window
#[derive(Clone, Serialize, Deserialize)]
pub struct FsWindow {
    keys: Arc<RwLock<Vec<KeyDetails>>>,
    root: TreeNode,
    /// Path of the selected file (for highlighting in the tree)
    selected_path: Option<String>,
    /// Dock state for the file viewer area
    #[serde(skip)]
    file_viewer_dock_state: Option<DockState<FileViewerTab>>,
    file_viewer_tab_viewer: FileViewerTabViewer,
}

impl Default for FsWindow {
    fn default() -> Self {
        Self {
            keys: crate::app::context::context().get_key_cache(),
            root: TreeNode::default(),
            selected_path: None,
            file_viewer_dock_state: Some(DockState::new(vec![])),
            file_viewer_tab_viewer: FileViewerTabViewer {},
        }
    }
}

impl Window for FsWindow {
    fn name(&self) -> String {
        "MutAnt Files".to_string()
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        self.build_tree();

        // Use a horizontal layout with two panels
        egui::SidePanel::left("file_tree_panel")
            .resizable(true)
            .min_width(200.0)
            .default_width(300.0)
            .show_inside(ui, |ui| {
                self.draw_tree(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            // Initialize the dock state if it doesn't exist
            if self.file_viewer_dock_state.is_none() {
                self.file_viewer_dock_state = Some(DockState::new(vec![]));
            }

            // Create a custom style for the dock area
            let mut style = egui_dock::Style::from_egui(ui.ctx().style().as_ref());

            // Configure the style to make docking more visible and user-friendly
            style.tab_bar.fill_tab_bar = false; // Don't fill the entire tab bar
            style.tab_bar.bg_fill = egui::Color32::from_rgb(40, 40, 40);
            style.tab.tab_body.bg_fill = egui::Color32::from_rgb(40, 40, 40);
            style.tab.active.bg_fill = egui::Color32::from_rgb(50, 50, 50);
            style.tab.focused.bg_fill = egui::Color32::from_rgb(50, 50, 50);
            style.tab_bar.hline_color = egui::Color32::from_rgb(70, 70, 70);

            // Draw the dock area
            if let Some(dock_state) = &mut self.file_viewer_dock_state {
                egui_dock::DockArea::new(dock_state)
                    .style(style)
                    .id(egui::Id::new("file_viewer_dock_area"))
                    .show_inside(ui, &mut self.file_viewer_tab_viewer);
            }

            // If no tabs are open, show a message
            if self.file_viewer_dock_state.as_ref().map_or(true, |ds| ds.iter_all_tabs().count() == 0) {
                ui.centered_and_justified(|ui| {
                    ui.heading("Select a file to view its content");
                });
            }
        });
    }
}

impl FsWindow {
    pub fn new() -> Self {
        Default::default()
    }

    /// Find a tab by key
    pub fn find_tab_mut(&mut self, key: &str) -> Option<&mut FileViewerTab> {
        if let Some(dock_state) = &mut self.file_viewer_dock_state {
            // Iterate through all tabs in the dock state
            for (_, tab) in dock_state.iter_all_tabs_mut() {
                if tab.file.key == key {
                    return Some(tab);
                }
            }
        }
        None
    }

    /// Add a new tab for a file
    fn add_file_tab(&mut self, file_details: KeyDetails) {
        // Create a new tab
        let tab = FileViewerTab::new(file_details.clone());

        // Add the tab to the dock state
        if let Some(dock_state) = &mut self.file_viewer_dock_state {
            // Check if a tab for this file already exists
            let tab_exists = dock_state.iter_all_tabs().any(|(_, t)| t.file.key == file_details.key);

            if !tab_exists {
                // Add the tab to the focused leaf or create a new surface if none exists
                if dock_state.main_surface().is_empty() {
                    // First tab - create a new surface
                    dock_state.push_to_focused_leaf(tab);
                } else {
                    // Add to the focused leaf
                    dock_state.main_surface_mut().push_to_focused_leaf(tab);
                }
            } else {
                // Focus the existing tab - we need to find it in the tree
                // For now, we'll just log that the tab exists
                log::info!("Tab for file {} already exists", file_details.key);

                // In the future, we could implement a way to focus the existing tab
                // This would require tracking node indices and tab indices
            }
        }
    }

    /// Build the tree from the current keys
    /// If force_rebuild is true, the tree will be rebuilt even if it's not empty
    fn build_tree(&mut self) {
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
            self.root = TreeNode::new_dir("root");

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
    fn count_tree_files(&self, node: &TreeNode) -> usize {
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
        ui.heading("File Explorer");

        ui.separator();

        // Draw the tree
        egui::ScrollArea::vertical().show(ui, |ui| {
            // First, show the root folder (always expanded)
            ui.horizontal(|ui| {
                // Root folder icon with zero indentation
                let icon = "📂";
                ui.label(format!("{} /", icon));
            });

            // Sort children: directories first, then files
            let mut sorted_children: Vec<_> = self.root.children.iter_mut().collect();
            sorted_children.sort_by(|(_, a), (_, b)| {
                match (a.is_dir(), b.is_dir()) {
                    (true, false) => std::cmp::Ordering::Less,    // Directories come before files
                    (false, true) => std::cmp::Ordering::Greater, // Files come after directories
                    _ => a.name.cmp(&b.name),                     // Sort alphabetically within each group
                }
            });

            // Get the currently selected path for highlighting
            let selected_path_ref = self.selected_path.as_deref();

            // Track clicked nodes to handle after the loop
            let mut clicked_details: Option<KeyDetails> = None;

            // Draw the sorted children with indentation and check if any file was clicked
            for (_, child) in sorted_children {
                let (clicked, details) = child.ui(ui, 1, selected_path_ref); // Start with indent level 1 since we're inside root
                if clicked && details.is_some() {
                    clicked_details = details;
                }
            }

            // Handle the clicked node outside the loop to avoid borrow issues
            if let Some(details) = clicked_details {
                // Update the selected path for highlighting in the tree
                self.selected_path = Some(details.key.clone());

                // Create a new tab for this file
                let file_details = details.clone();
                let key = file_details.key.clone();
                let is_public = file_details.is_public;

                // Add a new tab for this file
                self.add_file_tab(file_details);

                // Get the tab we just added
                if let Some(_tab) = self.find_tab_mut(&key) {
                    // Spawn a task to fetch the content
                    let window_id = ui.id();
                    let ctx_clone = ui.ctx().clone();
                    let key_clone = key.clone();

                    wasm_bindgen_futures::spawn_local(async move {
                        let ctx = crate::app::context::context();

                        // First try to get binary data
                        match ctx.get_file_binary(&key_clone, is_public).await {
                            Ok(binary_data) => {
                                // Get a mutable reference to the window system
                                let mut window_system = crate::app::window_system::window_system_mut();

                                // Find our window and update its content
                                if let Some(window) = window_system.get_window_mut::<FsWindow>(window_id) {
                                    // Find the tab with this file and update it
                                    if let Some(tab) = window.find_tab_mut(&key_clone) {
                                        // Store binary data
                                        tab.file_binary = Some(binary_data.clone());

                                        // Detect file type
                                        let file_type = multimedia::detect_file_type(&binary_data, &key_clone);
                                        tab.file_type = Some(file_type.clone());

                                        // Process based on file type
                                        match file_type {
                                            multimedia::FileType::Text => {
                                                // Try to convert to text
                                                if let Ok(text) = String::from_utf8(binary_data.clone()) {
                                                    tab.content = text;

                                                    // Update the existing FileContent for text viewing
                                                    if let Some(file_content) = &mut tab.file_content {
                                                        file_content.file_type = multimedia::FileType::Text;
                                                        file_content.raw_data = binary_data;
                                                        file_content.editable_content = Some(tab.content.clone());
                                                        file_content.content_modified = false;
                                                    }
                                                } else {
                                                    tab.content = "Error: File contains binary data that cannot be displayed as text".to_string();
                                                }
                                            },
                                            multimedia::FileType::Code(lang) => {
                                                // Try to convert to text for code display
                                                if let Ok(text) = String::from_utf8(binary_data.clone()) {
                                                    tab.content = text;
                                                    // Store the language in the file type for syntax highlighting
                                                    tab.file_type = Some(multimedia::FileType::Code(lang.clone()));

                                                    // Update the existing FileContent for code viewing
                                                    if let Some(file_content) = &mut tab.file_content {
                                                        file_content.file_type = multimedia::FileType::Code(lang);
                                                        file_content.raw_data = binary_data;
                                                        file_content.editable_content = Some(tab.content.clone());
                                                        file_content.content_modified = false;
                                                    }
                                                } else {
                                                    tab.content = "Error: File contains binary data that cannot be displayed as code".to_string();
                                                }
                                            },
                                            multimedia::FileType::Image => {
                                                // Load image
                                                if let Some(texture) = multimedia::load_image(&ctx_clone, &binary_data) {
                                                    tab.image_texture = Some(texture.clone());
                                                    tab.content = "Image file loaded successfully".to_string();

                                                    // Update the existing FileContent for image viewing
                                                    if let Some(file_content) = &mut tab.file_content {
                                                        file_content.file_type = multimedia::FileType::Image;
                                                        file_content.raw_data = binary_data;
                                                        file_content.image_texture = Some(texture);
                                                    }
                                                } else {
                                                    tab.content = "Error: Failed to load image data".to_string();

                                                    // Update FileContent to show error as text
                                                    if let Some(file_content) = &mut tab.file_content {
                                                        file_content.file_type = multimedia::FileType::Text;
                                                        file_content.editable_content = Some(tab.content.clone());
                                                    }
                                                }
                                            },
                                            multimedia::FileType::Video => {
                                                // Create a data URL for the video
                                                let mime_type = mime_guess::from_path(&key_clone).first_or_octet_stream().essence_str().to_string();
                                                let data_url = base64::engine::general_purpose::STANDARD.encode(&binary_data);
                                                let video_url = format!("data:{};base64,{}", mime_type, data_url);
                                                tab.video_url = Some(video_url.clone());
                                                tab.content = "Video file loaded successfully".to_string();

                                                // Update the existing FileContent for video viewing
                                                if let Some(file_content) = &mut tab.file_content {
                                                    file_content.file_type = multimedia::FileType::Video;
                                                    file_content.raw_data = binary_data;
                                                    file_content.video_url = Some(video_url);
                                                }
                                            },
                                            multimedia::FileType::Other => {
                                                // Try to convert to text anyway for viewing
                                                if let Ok(text) = String::from_utf8(binary_data.clone()) {
                                                    tab.content = text;
                                                    // Change to text type for better viewing
                                                    tab.file_type = Some(multimedia::FileType::Text);

                                                    // Update the existing FileContent for text viewing
                                                    if let Some(file_content) = &mut tab.file_content {
                                                        file_content.file_type = multimedia::FileType::Text;
                                                        file_content.raw_data = binary_data;
                                                        file_content.editable_content = Some(tab.content.clone());
                                                    }
                                                } else {
                                                    tab.content = "This file type is not supported for viewing".to_string();

                                                    // Update FileContent to show error as text
                                                    if let Some(file_content) = &mut tab.file_content {
                                                        file_content.file_type = multimedia::FileType::Other;
                                                        file_content.raw_data = binary_data;
                                                        file_content.editable_content = Some(tab.content.clone());
                                                    }
                                                }
                                            }
                                        }

                                        tab.is_loading = false;
                                    }
                                }
                            },
                            Err(e) => {
                                // Handle error
                                let mut window_system = crate::app::window_system::window_system_mut();
                                if let Some(window) = window_system.get_window_mut::<FsWindow>(window_id) {
                                    if let Some(tab) = window.find_tab_mut(&key_clone) {
                                        tab.content = format!("Error loading file content: {}", e);
                                        tab.is_loading = false;

                                        // Update FileContent to show error as text
                                        if let Some(file_content) = &mut tab.file_content {
                                            file_content.file_type = multimedia::FileType::Text;
                                            file_content.editable_content = Some(tab.content.clone());
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
            }
        });
    }


}
