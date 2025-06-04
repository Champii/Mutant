use std::collections::BTreeMap;
use eframe::egui;
use mutant_protocol::KeyDetails;
use serde::{Deserialize, Serialize};
use crate::app::theme;

use humansize::{format_size, BINARY}; // For humanize_size

/// Helper function to format file sizes in a human-readable way
fn humanize_size(size: u64) -> String {
    format_size(size, BINARY)
}

/// Drag and drop state for the filesystem tree
#[derive(Clone, Debug, Default)]
pub struct DragDropState {
    /// Currently dragged item (path and whether it's a directory)
    pub dragged_item: Option<(String, bool)>, // (path, is_directory)
    /// Current drag position for visual feedback
    pub drag_pos: Option<egui::Pos2>,
    /// Whether we're currently in a drag operation
    pub is_dragging: bool,
    /// Potential drop target path
    pub drop_target: Option<String>,
    /// Track if we've started a potential drag but haven't moved enough yet
    pub drag_candidate: Option<(String, bool, egui::Pos2)>, // (path, is_directory, start_pos)
}

/// Result of a drag and drop operation
#[derive(Clone, Debug)]
pub enum DragDropResult {
    /// No drag and drop operation occurred
    None,
    /// A move operation should be performed (old_path, new_path)
    Move(String, String),
    /// A directory move operation should be performed (old_directory_path, new_directory_path)
    MoveDirectory(String, String),
}

/// A node in the filesystem tree
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TreeNode {
    /// Name of this node (file or directory name)
    pub name: String,

    /// If this is a file, contains the key details
    pub key_details: Option<KeyDetails>,

    /// If this is a directory, contains child nodes
    pub children: BTreeMap<String, TreeNode>,

    /// Whether this directory is expanded (only relevant for directories)
    pub expanded: bool,

    /// Full path to this node (for debugging and unique IDs)
    pub path: String,
}

impl TreeNode {
    /// Create a new directory node
    pub fn new_dir(name: &str) -> Self {
        Self {
            name: name.to_string(),
            key_details: None,
            children: BTreeMap::new(),
            expanded: false,
            path: name.to_string(),
        }
    }

    /// Create a new file node
    pub fn new_file(name: &str, key_details: KeyDetails) -> Self {
        Self {
            name: name.to_string(),
            key_details: Some(key_details),
            children: BTreeMap::new(),
            expanded: false,
            path: name.to_string(),
        }
    }

    /// Check if this node is a directory
    pub fn is_dir(&self) -> bool {
        self.key_details.is_none()
    }

    /// Insert a key into the tree at the appropriate location
    pub fn insert_key(&mut self, path_parts: &[&str], key_details: KeyDetails, parent_path: &str) {
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
    /// Returns (view_clicked_details, download_clicked_details, delete_clicked_details, drag_drop_result)
    pub fn ui(&mut self, ui: &mut egui::Ui, indent_level: usize, selected_path: Option<&str>, window_id: &str, drag_state: &mut DragDropState) -> (Option<KeyDetails>, Option<KeyDetails>, Option<KeyDetails>, DragDropResult) {
        // Extremely compact indentation for maximum space efficiency
        let indent_per_level = 1.5;  // Reduced from 3.0 to 1.5
        let total_indent = indent_per_level * (indent_level as f32);

        let mut view_clicked_details = None;
        let mut download_clicked_details = None;
        let mut delete_clicked_details = None;
        let mut drag_drop_result = DragDropResult::None;

        // Add subtle vertical spacing between items
        if indent_level > 0 {
            ui.add_space(2.0);
        }

        ui.horizontal(|ui| {
            // Draw visual delimiter line for tree structure
            if indent_level > 0 {
                let line_color = theme::MutantColors::BORDER_LIGHT;
                let line_x = total_indent - 6.0; // Position line slightly left of content
                let line_start = ui.cursor().top();
                let line_end = line_start + 20.0; // Height of one row

                ui.painter().line_segment(
                    [egui::Pos2::new(line_x, line_start), egui::Pos2::new(line_x, line_end)],
                    egui::Stroke::new(1.0, line_color)
                );
            }

            // Apply the base indentation
            ui.add_space(total_indent);

            if self.is_dir() {
                // Directory node - use collapsing header for proper tree behavior
                let icon = if self.expanded { "📂" } else { "📁" };
                let text = egui::RichText::new(format!("{} {}/", icon, self.name))
                    .size(12.0)  // Match file font size for consistency
                    .color(theme::MutantColors::ACCENT_ORANGE);  // Special distinguishing color for folders

                // Create a unique ID for this header that includes the expanded state
                // This forces egui to reset its internal state when we change expansion programmatically
                let header_id = format!("mutant_fs_{}dir_{}_{}", window_id, self.path, self.expanded);
                let header = egui::CollapsingHeader::new(text)
                    .id_salt(header_id)
                    .default_open(self.expanded)
                    .show_background(false); // Remove background to have more control over positioning

                let mut child_view_details = None;
                let mut child_download_details = None;
                let mut child_delete_details = None;
                let mut child_drag_drop_result = DragDropResult::None;

                let header_result = header.show(ui, |ui| {
                    // Check if this directory is a drop target
                    if drag_state.is_dragging {
                        // Create a reasonable drop zone based on the current UI area
                        let current_rect = ui.cursor();
                        let drop_zone_rect = egui::Rect::from_min_size(
                            current_rect.left_top(),
                            egui::Vec2::new(ui.available_width().min(400.0), 25.0) // Constrained width and height
                        );
                        let pointer_pos = ui.ctx().pointer_latest_pos();

                        if let Some(pos) = pointer_pos {
                            if drop_zone_rect.contains(pos) {
                                log::info!("Setting drop target to directory: {}", self.path);
                                drag_state.drop_target = Some(self.path.clone());
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
                                    egui::Stroke::new(3.0, theme::MutantColors::ACCENT_ORANGE),
                                    egui::epaint::StrokeKind::Outside
                                );
                            }
                        }
                    }

                    // Sort children: files first, then directories
                    let mut sorted_children: Vec<_> = self.children.iter_mut().collect();
                    sorted_children.sort_by(|(_, a), (_, b)| {
                        match (a.is_dir(), b.is_dir()) {
                            (true, false) => std::cmp::Ordering::Greater, // Directories come after files
                            (false, true) => std::cmp::Ordering::Less,    // Files come before directories
                            _ => a.name.cmp(&b.name),                     // Sort alphabetically within each group
                        }
                    });

                    // Draw the sorted children with exactly one more level of indentation
                    for (_, child) in sorted_children {
                        let (view_details, down_details, delete_details, child_drag_result) = child.ui(ui, indent_level + 1, selected_path, window_id, drag_state);
                        if view_details.is_some() {
                            child_view_details = view_details;
                        }
                        if down_details.is_some() {
                            child_download_details = down_details;
                        }
                        if delete_details.is_some() {
                            child_delete_details = delete_details;
                        }
                        if !matches!(child_drag_result, DragDropResult::None) {
                            child_drag_drop_result = child_drag_result;
                        }
                    }
                });

                // Handle directory drag and drop by creating an invisible draggable overlay
                let header_response = &header_result.header_response;
                let header_rect = header_response.rect;

                // Check for drag initiation using global input state (without creating overlays)
                if !drag_state.is_dragging && drag_state.drag_candidate.is_none() {
                    // Check if mouse was pressed down on this header
                    if ui.ctx().input(|i| i.pointer.primary_pressed()) {
                        if let Some(pointer_pos) = ui.ctx().pointer_latest_pos() {
                            if header_rect.contains(pointer_pos) {
                                drag_state.drag_candidate = Some((self.path.clone(), true, pointer_pos));
                                log::info!("Started tracking potential drag for directory: {}", self.path);
                            }
                        }
                    }
                }

                // Check for drag movement if we have a candidate
                if let Some((candidate_path, is_dir, start_pos)) = &drag_state.drag_candidate {
                    if candidate_path == &self.path && *is_dir {
                        if ui.ctx().input(|i| i.pointer.primary_down()) {
                            if let Some(current_pos) = ui.ctx().pointer_latest_pos() {
                                let distance = (current_pos - *start_pos).length();
                                if distance > 5.0 && !drag_state.is_dragging {
                                    // We've moved enough, start the actual drag
                                    drag_state.dragged_item = Some((self.path.clone(), true));
                                    drag_state.is_dragging = true;
                                    drag_state.drag_candidate = None; // Clear the candidate
                                    log::info!("Started dragging directory: {}", self.path);
                                }
                            }
                        } else {
                            // Mouse was released without dragging - clear candidate
                            drag_state.drag_candidate = None;
                        }
                    }
                }

                // Handle ongoing drag operations
                if drag_state.is_dragging {
                    if let Some((dragged_path, is_dir)) = &drag_state.dragged_item {
                        if *is_dir && dragged_path == &self.path {
                            // Add visual feedback if this directory is being dragged
                            ui.painter().rect_filled(
                                header_rect,
                                4.0,
                                egui::Color32::from_rgba_premultiplied(255, 140, 0, 60) // Orange with transparency
                            );

                            // Update drag position
                            if let Some(pointer_pos) = ui.ctx().pointer_latest_pos() {
                                drag_state.drag_pos = Some(pointer_pos);
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            }

                            // Check for drag release
                            if !ui.ctx().input(|i| i.pointer.primary_down()) {
                                log::info!("Drag stopped for directory: {}", self.path);
                                if let Some(drop_target) = &drag_state.drop_target {
                                    log::info!("Drop target: {}", drop_target);
                                    // Calculate the new path for the move operation
                                    let new_path = if drop_target == "/" {
                                        // Moving to root - just use the directory name
                                        self.name.clone()
                                    } else {
                                        // Moving to a subdirectory
                                        format!("{}/{}", drop_target, self.name)
                                    };

                                    if dragged_path != &new_path {
                                        drag_drop_result = DragDropResult::MoveDirectory(dragged_path.clone(), new_path.clone());
                                        log::info!("Directory drop: moving directory '{}' to '{}'", dragged_path, new_path);
                                    } else {
                                        log::info!("Same path, no move needed: {} == {}", dragged_path, new_path);
                                    }
                                } else {
                                    log::info!("No drop target set");
                                }

                                // Reset all drag state
                                drag_state.dragged_item = None;
                                drag_state.is_dragging = false;
                                drag_state.drop_target = None;
                                drag_state.drag_pos = None;
                                drag_state.drag_candidate = None;
                            }
                        }
                    }
                }

                // Handle directory expansion - only sync with egui's state if user clicked
                // Don't override programmatic changes from expand_all/collapse_all
                if header_response.clicked() {
                    self.expanded = header_result.openness > 0.0;
                }



                // Propagate click from children
                if child_view_details.is_some() {
                    view_clicked_details = child_view_details;
                }
                if child_download_details.is_some() {
                    download_clicked_details = child_download_details;
                }
                if child_delete_details.is_some() {
                    delete_clicked_details = child_delete_details;
                }
                if !matches!(child_drag_drop_result, DragDropResult::None) {
                    drag_drop_result = child_drag_drop_result;
                }
            } else {
                // File node - calculate proper alignment with folder expand arrows
                // The expand arrow in CollapsingHeader typically takes about 16-20 pixels
                // We need to align the file icon with the folder icon, which comes after the arrow
                let expand_arrow_width = 16.0; // Standard width for expand arrow
                let icon_spacing = 4.0; // Small spacing after arrow before icon
                ui.add_space(expand_arrow_width + icon_spacing);

                let details = self.key_details.as_ref().unwrap();
                let is_selected = selected_path.map_or(false, |path| path == &details.key);

                // Determine file icon and color based on extension
                let (file_icon, icon_color) = self.get_file_icon_and_color();

                // Style the filename text - less bright color and smaller font
                let filename_color = if details.is_public {
                    theme::MutantColors::SUCCESS
                } else {
                    theme::MutantColors::TEXT_SECONDARY  // Less bright than TEXT_PRIMARY
                };

                // Create file display text with icon
                // We'll draw the text manually for better control over fade effects

                // Make the file node clickable for viewing
                let row_response = ui.allocate_response(
                    egui::Vec2::new(ui.available_width(), 20.0),
                    egui::Sense::click()
                );

                let row_rect = row_response.rect;

                // Add selection background for the entire row
                if is_selected {
                    ui.painter().rect_filled(
                        row_rect,
                        4.0,
                        theme::MutantColors::SELECTION
                    );
                }

                // Add visual feedback if this item is being dragged
                if let Some((dragged_path, _)) = &drag_state.dragged_item {
                    let file_key = &details.key;
                    if dragged_path == file_key {
                        ui.painter().rect_filled(
                            row_rect,
                            4.0,
                            egui::Color32::from_rgba_premultiplied(255, 140, 0, 60) // Orange with transparency
                        );
                    }
                }

                // Calculate metadata width and positioning
                let metadata_width = 100.0; // Balanced width for metadata area

                // Calculate text clipping area to prevent text from bleeding into the metadata area
                let fade_width = 40.0; // Balanced fade width for smooth gradient
                let text_clip_width = row_rect.width() - metadata_width - fade_width;

                // Create a clipping rectangle for the text to prevent bleeding
                let text_clip_rect = egui::Rect::from_min_size(
                    row_rect.left_top(),
                    egui::Vec2::new(text_clip_width, row_rect.height())
                );

                // Draw the icon and filename with different colors and smaller font
                // Center the text vertically in the row
                let text_pos = row_rect.left_top() + egui::Vec2::new(4.0, (row_rect.height() - 12.0) / 2.0);
                let font_id = egui::FontId::new(12.0, egui::FontFamily::Proportional);  // Smaller font size

                // Draw icon with its specific color
                let icon_galley = ui.painter().layout_no_wrap(
                    file_icon.to_string(),
                    font_id.clone(),
                    icon_color
                );
                let icon_width = icon_galley.rect.width() + 4.0; // Get width before moving
                ui.painter().with_clip_rect(text_clip_rect).galley(text_pos, icon_galley, icon_color);

                // Draw filename with less bright color, positioned after the icon
                let filename_pos = text_pos + egui::Vec2::new(icon_width, 0.0);
                let filename_galley = ui.painter().layout_no_wrap(
                    format!(" {}", self.name),
                    font_id.clone(),
                    filename_color
                );
                ui.painter().with_clip_rect(text_clip_rect).galley(filename_pos, filename_galley, filename_color);

                // Check for drag initiation using global input state (without creating overlays)
                if !drag_state.is_dragging && drag_state.drag_candidate.is_none() {
                    // Check if mouse was pressed down on this row
                    if ui.ctx().input(|i| i.pointer.primary_pressed()) {
                        if let Some(pointer_pos) = ui.ctx().pointer_latest_pos() {
                            if row_rect.contains(pointer_pos) {
                                // Use the actual key from key_details for files, not the tree path
                                let file_key = &details.key;
                                drag_state.drag_candidate = Some((file_key.clone(), false, pointer_pos));
                                log::info!("Started tracking potential drag for file: {}", file_key);
                            }
                        }
                    }
                }

                // Check for drag movement if we have a candidate
                if let Some((candidate_path, is_dir, start_pos)) = &drag_state.drag_candidate {
                    let file_key = &details.key;
                    if candidate_path == file_key && !*is_dir {
                        if ui.ctx().input(|i| i.pointer.primary_down()) {
                            if let Some(current_pos) = ui.ctx().pointer_latest_pos() {
                                let distance = (current_pos - *start_pos).length();
                                if distance > 5.0 && !drag_state.is_dragging {
                                    // We've moved enough, start the actual drag
                                    drag_state.dragged_item = Some((file_key.clone(), false));
                                    drag_state.is_dragging = true;
                                    drag_state.drag_candidate = None; // Clear the candidate
                                    log::info!("Started dragging file: {}", file_key);
                                }
                            }
                        } else {
                            // Mouse was released without dragging - clear candidate
                            drag_state.drag_candidate = None;
                        }
                    }
                }

                // Handle ongoing drag operations
                if drag_state.is_dragging {
                    if let Some((dragged_path, is_dir)) = &drag_state.dragged_item {
                        let file_key = &details.key;
                        if !*is_dir && dragged_path == file_key {
                            // Update drag position
                            if let Some(pointer_pos) = ui.ctx().pointer_latest_pos() {
                                drag_state.drag_pos = Some(pointer_pos);
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            }

                            // Check for drag release
                            if !ui.ctx().input(|i| i.pointer.primary_down()) {
                                log::info!("Drag stopped for file: {}", file_key);
                                if let Some(drop_target) = &drag_state.drop_target {
                                    log::info!("Drop target: {}", drop_target);
                                    // Calculate the new path for the move operation
                                    let new_path = if drop_target == "/" {
                                        // Moving to root - just use the filename
                                        self.name.clone()
                                    } else {
                                        // Moving to a subdirectory
                                        format!("{}/{}", drop_target, self.name)
                                    };

                                    if dragged_path != &new_path {
                                        drag_drop_result = DragDropResult::Move(dragged_path.clone(), new_path.clone());
                                        log::info!("File drop: moving '{}' to '{}'", dragged_path, new_path);
                                    } else {
                                        log::info!("Same path, no move needed: {} == {}", dragged_path, new_path);
                                    }
                                } else {
                                    log::info!("No drop target set");
                                }

                                // Reset all drag state
                                drag_state.dragged_item = None;
                                drag_state.is_dragging = false;
                                drag_state.drop_target = None;
                                drag_state.drag_pos = None;
                                drag_state.drag_candidate = None;
                            }
                        }
                    }
                }

                // Handle click on the entire row (this will work normally since we're not intercepting clicks)
                if row_response.clicked() {
                    view_clicked_details = Some(details.clone());
                }

                if row_response.hovered() && !drag_state.is_dragging {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                // Draw metadata manually at the exact same position as text
                let text_baseline_y = row_rect.top() + (row_rect.height() - 12.0) / 2.0;

                // Calculate positions for metadata elements from right to left
                let mut current_x = row_rect.right() - 4.0; // Start with right padding

                // Download button position
                let button_width = 20.0;
                let button_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(current_x - button_width, text_baseline_y),
                    egui::Vec2::new(button_width, 12.0)
                );

                // Draw download button manually
                let button_response = ui.allocate_rect(button_rect, egui::Sense::click());
                if button_response.clicked() {
                    download_clicked_details = Some(details.clone());
                }
                if button_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                // Draw the arrow icon at the exact text baseline
                let arrow_galley = ui.painter().layout_no_wrap(
                    "⬇".to_string(),
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    theme::MutantColors::ACCENT_GREEN
                );
                ui.painter().galley(
                    egui::Pos2::new(current_x - button_width / 2.0 - arrow_galley.rect.width() / 2.0, text_baseline_y),
                    arrow_galley,
                    theme::MutantColors::ACCENT_GREEN
                );

                current_x -= button_width + 4.0; // Move left for delete button

                // Delete button position (to the left of download button)
                let delete_button_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(current_x - button_width, text_baseline_y),
                    egui::Vec2::new(button_width, 12.0)
                );

                // Draw delete button manually
                let delete_button_response = ui.allocate_rect(delete_button_rect, egui::Sense::click());
                if delete_button_response.clicked() {
                    delete_clicked_details = Some(details.clone());
                }
                if delete_button_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                // Draw the red 'x' icon at the exact text baseline
                let delete_galley = ui.painter().layout_no_wrap(
                    "✖".to_string(),
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    egui::Color32::from_rgb(220, 50, 50) // Red color for delete
                );
                ui.painter().galley(
                    egui::Pos2::new(current_x - button_width / 2.0 - delete_galley.rect.width() / 2.0, text_baseline_y),
                    delete_galley,
                    egui::Color32::from_rgb(220, 50, 50)
                );

                current_x -= button_width + 8.0; // Move left for next element

                // File size text
                let size_text = humanize_size(details.total_size);
                let size_galley = ui.painter().layout_no_wrap(
                    size_text,
                    egui::FontId::new(11.0, egui::FontFamily::Proportional),
                    theme::MutantColors::TEXT_MUTED
                );
                current_x -= size_galley.rect.width();
                ui.painter().galley(
                    egui::Pos2::new(current_x, text_baseline_y),
                    size_galley,
                    theme::MutantColors::TEXT_MUTED
                );

                // Public indicator if needed
                if details.is_public {
                    current_x -= 4.0; // Small spacing
                    let pub_galley = ui.painter().layout_no_wrap(
                        "PUB".to_string(),
                        egui::FontId::new(10.0, egui::FontFamily::Proportional),
                        theme::MutantColors::SUCCESS
                    );
                    current_x -= pub_galley.rect.width();
                    ui.painter().galley(
                        egui::Pos2::new(current_x, text_baseline_y),
                        pub_galley,
                        theme::MutantColors::SUCCESS
                    );
                }
            }
        });

        (view_clicked_details, download_clicked_details, delete_clicked_details, drag_drop_result)
    }

    /// Get appropriate icon and color for file based on extension
    pub fn get_file_icon_and_color(&self) -> (&'static str, egui::Color32) {
        if let Some(extension) = std::path::Path::new(&self.name).extension() {
            match extension.to_string_lossy().to_lowercase().as_str() {
                // Code files - bright colors for different languages
                "rs" | "rust" => ("🦀", theme::MutantColors::ACCENT_ORANGE),
                "js" | "ts" | "jsx" | "tsx" => ("📜", theme::MutantColors::WARNING),
                "py" | "python" => ("🐍", theme::MutantColors::ACCENT_GREEN),
                "java" | "class" => ("☕", theme::MutantColors::ACCENT_ORANGE),
                "cpp" | "c" | "cc" | "cxx" | "h" | "hpp" => ("⚙️", theme::MutantColors::ACCENT_BLUE),
                "go" => ("🐹", theme::MutantColors::ACCENT_CYAN),
                "php" => ("🐘", theme::MutantColors::ACCENT_PURPLE),
                "rb" | "ruby" => ("💎", theme::MutantColors::ERROR),
                "swift" => ("🦉", theme::MutantColors::ACCENT_ORANGE),
                "kt" | "kotlin" => ("🎯", theme::MutantColors::ACCENT_PURPLE),
                "cs" | "csharp" => ("🔷", theme::MutantColors::ACCENT_BLUE),
                "html" | "htm" => ("🌐", theme::MutantColors::ACCENT_ORANGE),
                "css" | "scss" | "sass" | "less" => ("🎨", theme::MutantColors::ACCENT_BLUE),
                "json" | "yaml" | "yml" | "toml" | "xml" => ("📋", theme::MutantColors::TEXT_MUTED),

                // Images - green tones
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => ("📷", theme::MutantColors::ACCENT_GREEN),

                // Videos - purple/magenta tones
                "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" => ("🎬", theme::MutantColors::ACCENT_PURPLE),

                // Audio - cyan tones
                "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" => ("🎵", theme::MutantColors::ACCENT_CYAN),

                // Archives - orange tones
                "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => ("📦", theme::MutantColors::ACCENT_ORANGE),

                // Documents - different colors for different types
                "pdf" => ("📕", theme::MutantColors::ERROR),
                "doc" | "docx" => ("📘", theme::MutantColors::ACCENT_BLUE),
                "xls" | "xlsx" => ("📗", theme::MutantColors::ACCENT_GREEN),
                "ppt" | "pptx" => ("📙", theme::MutantColors::WARNING),
                "txt" | "md" | "readme" => ("📄", theme::MutantColors::TEXT_MUTED),

                // Executables - bright warning colors
                "exe" | "msi" | "deb" | "rpm" | "dmg" | "app" => ("⚡", theme::MutantColors::WARNING),

                _ => ("📄", theme::MutantColors::TEXT_MUTED)
            }
        } else {
            ("📄", theme::MutantColors::TEXT_MUTED)
        }
    }

    /// Expand all directories recursively
    pub fn expand_all(&mut self) {
        if self.is_dir() {
            self.expanded = true;
            for (_, child) in &mut self.children {
                child.expand_all();
            }
        }
    }

    /// Collapse all directories recursively
    pub fn collapse_all(&mut self) {
        if self.is_dir() {
            self.expanded = false;
            for (_, child) in &mut self.children {
                child.collapse_all();
            }
        }
    }

    /// Check if any directory is expanded (used to determine expand/collapse all button state)
    pub fn has_expanded_dirs(&self) -> bool {
        if self.is_dir() && self.expanded {
            return true;
        }
        for (_, child) in &self.children {
            if child.has_expanded_dirs() {
                return true;
            }
        }
        false
    }
}
