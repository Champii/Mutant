use std::sync::{Arc, RwLock};
use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText};
use mutant_protocol::KeyDetails;
use serde::{Deserialize, Serialize};

use super::Window;

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
}

impl TreeNode {
    /// Create a new directory node
    fn new_dir(name: &str) -> Self {
        Self {
            name: name.to_string(),
            key_details: None,
            children: BTreeMap::new(),
            expanded: false,
        }
    }

    /// Create a new file node
    fn new_file(name: &str, key_details: KeyDetails) -> Self {
        Self {
            name: name.to_string(),
            key_details: Some(key_details),
            children: BTreeMap::new(),
            expanded: false,
        }
    }

    /// Check if this node is a directory
    fn is_dir(&self) -> bool {
        self.key_details.is_none()
    }

    /// Insert a key into the tree at the appropriate location
    fn insert_key(&mut self, path_parts: &[&str], key_details: KeyDetails) {
        if path_parts.is_empty() {
            return;
        }

        let current = path_parts[0];

        if path_parts.len() == 1 {
            // This is a file (leaf node)
            self.children.insert(current.to_string(), TreeNode::new_file(current, key_details));
        } else {
            // This is a directory
            let dir_node = self.children
                .entry(current.to_string())
                .or_insert_with(|| TreeNode::new_dir(current));

            dir_node.insert_key(&path_parts[1..], key_details);
        }
    }

    /// Draw this node and its children
    fn ui(&mut self, ui: &mut egui::Ui, indent_level: usize) {
        let indent = (indent_level as f32) * 1.0; // Reduced from 20 to 10 pixels per indent level

        ui.horizontal(|ui| {
            ui.add_space(indent);

            if self.is_dir() {
                // Directory node
                let icon = if self.expanded { "📂" } else { "📁" };
                let text = format!("{} {}", icon, self.name);

                let header = egui::CollapsingHeader::new(text)
                    .id_salt(format!("dir_{}", self.name))
                    .default_open(self.expanded);

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

                    // Draw the sorted children
                    for (_, child) in sorted_children {
                        child.ui(ui, indent_level + 1);
                    }
                }).header_response.clicked() || self.expanded;
            } else {
                ui.add_space(indent_level as f32 * 3.0);
                // File node
                let icon = "📄";
                let details = self.key_details.as_ref().unwrap();

                let text = if details.is_public {
                    RichText::new(format!("{} {} (public)", icon, self.name))
                        .color(Color32::from_rgb(0, 128, 0))
                } else {
                    RichText::new(format!("{} {}", icon, self.name))
                };

                ui.label(text);
            }
        });
    }
}

/// The filesystem tree window
#[derive(Clone, Serialize, Deserialize)]
pub struct FsWindow {
    keys: Arc<RwLock<Vec<KeyDetails>>>,
    root: TreeNode,
}

impl Default for FsWindow {
    fn default() -> Self {
        Self {
            keys: crate::app::context::context().get_key_cache(),
            root: TreeNode::default(),
        }
    }
}

impl Window for FsWindow {
    fn name(&self) -> String {
        "MutAnt Files".to_string()
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        self.build_tree();
        self.draw_tree(ui);
    }
}

impl FsWindow {
    pub fn new() -> Self {
        Default::default()
    }

    /// Build the tree from the current keys
    fn build_tree(&mut self) {
        // Only rebuild if needed
        if self.root.children.is_empty() {
            let keys = self.keys.read().unwrap();

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
                self.root.insert_key(&parts, key.clone());
            }
        }
    }

    /// Draw the tree UI
    fn draw_tree(&mut self, ui: &mut egui::Ui) {
        ui.heading("File Explorer");

        ui.separator();

        // Add refresh button
        if ui.button("🔄 Refresh").clicked() {
            // Clear the tree to force rebuild
            self.root.children.clear();

            // Fetch fresh keys
            let ctx = crate::app::context::context();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = ctx.list_keys().await;
            });
        }

        ui.separator();

        // Draw the tree
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Sort children: directories first, then files
            let mut sorted_children: Vec<_> = self.root.children.iter_mut().collect();
            sorted_children.sort_by(|(_, a), (_, b)| {
                match (a.is_dir(), b.is_dir()) {
                    (true, false) => std::cmp::Ordering::Less,    // Directories come before files
                    (false, true) => std::cmp::Ordering::Greater, // Files come after directories
                    _ => a.name.cmp(&b.name),                     // Sort alphabetically within each group
                }
            });

            // Draw the sorted children
            for (_, child) in sorted_children {
                child.ui(ui, 0);
            }
        });
    }
}
