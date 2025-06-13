use eframe::egui::{self, Image, RichText, Sense, TextureHandle, TextureOptions, Ui};
use egui_extras::syntax_highlighting::{self, CodeTheme};
use image;
use mime_guess::from_path;
// wasm_bindgen import removed as it's not used at top level
// JsCast and prelude removed as they were unused at top level. web_sys also removed.
use base64::Engine;
use crate::app::theme::MutantColors;
use log;
use std::sync::Arc;

/// Enum representing different types of files
#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    Text,
    Image,
    Code(String), // String represents the language
    Video,
    Other,
}

/// Struct to hold file content in different formats
#[derive(Clone)]
pub struct FileContent {
    pub file_type: FileType,
    pub raw_data: Vec<u8>, // For videos, this will be empty.
    pub editable_content: Option<String>,  // Separate field for edited content
    pub content_modified: bool,            // Flag to track if content was modified
    pub image_texture: Option<TextureHandle>,
    pub video_url: Option<String>, // WebSocket URL for video streaming
}

impl FileContent {
    pub fn new(raw_data: Vec<u8>, file_path: &str, websocket_url_for_video: Option<String>) -> Self {
        let detected_file_type = detect_file_type(
            if websocket_url_for_video.is_some() ||
               file_path.ends_with(".mp4") || file_path.ends_with(".webm") ||
               file_path.ends_with(".ogv") || file_path.ends_with(".mov")
            { &[] } else { &raw_data },
            file_path,
        );

        let mut final_video_url: Option<String> = None;
        let final_raw_data: Vec<u8>;

        if detected_file_type == FileType::Video {
            if let Some(provided_url) = websocket_url_for_video {
                final_video_url = Some(provided_url);
                log::info!("Using provided video_ws_url in FileContent: {:?}", final_video_url);
            } else {
                // Generate URL if not provided (fallback, though FileViewerTab should provide it)
                let filename = std::path::Path::new(file_path)
                    .file_name()
                    .map_or_else(|| "unknown_video".to_string(), |f| f.to_string_lossy().into_owned());
                let base_ws_url = crate::app::client_manager::get_daemon_ws_url();
                let video_stream_base_url = if base_ws_url.ends_with("/ws") {
                    base_ws_url[0..base_ws_url.len()-3].to_string()
                } else {
                    log::warn!("Base WebSocket URL for video does not end with /ws: {}", base_ws_url);
                    base_ws_url
                };
                final_video_url = Some(format!("{}/video_stream/{}", video_stream_base_url, filename));
                log::info!("FileContent generated video_ws_url: {:?}", final_video_url);
            }
            final_raw_data = Vec::new(); // Do not store raw_data for videos
        } else {
            final_raw_data = raw_data;
        }

        // Initialize with processed data and detected type
        let mut content = Self {
            file_type: detected_file_type, // Use the type detected at the start of this function
            raw_data: final_raw_data,
            editable_content: None,
            content_modified: false,
            image_texture: None,
            video_url: final_video_url, // Assign the determined video_url
        };

        // Process content based on file type
        content.process_content();

        content
    }

    fn process_content(&mut self) {
        match &self.file_type {
            FileType::Text => {
                // Try to convert raw data to text
                if let Ok(text) = String::from_utf8(self.raw_data.clone()) {
                    self.editable_content = Some(text); // Initialize editable content
                } else {
                    // If conversion fails, set as Other type
                    self.file_type = FileType::Other;
                }
            },
            FileType::Code(_) => {
                // Convert raw data to text for code display
                if let Ok(text) = String::from_utf8(self.raw_data.clone()) {
                    self.editable_content = Some(text); // Initialize editable content
                } else {
                    // If conversion fails, set as Other type
                    self.file_type = FileType::Other;
                }
            },
            FileType::Video => {
                // Video content is handled by video_url, no raw_data processing needed here.
            }
            _ => {
                // Other types are handled when rendering
            }
        }
    }

    /// Create a data URL for binary content (used for images)
    /// Note: This should not be used for videos anymore as they will be streamed.
    pub fn create_data_url(&self, mime_type: &str) -> String {
        let base64_data = base64::engine::general_purpose::STANDARD.encode(&self.raw_data);
        format!("data:{};base64,{}", mime_type, base64_data)
    }
}

/// Detect the type of file based on content and extension
pub fn detect_file_type(data: &[u8], file_path: &str) -> FileType {
    // First try to detect by MIME type from extension
    let mime = from_path(file_path).first_or_octet_stream();
    let mime_type = mime.essence_str();

    // Check for common MIME types
    if mime_type.starts_with("text/") {
        // For text files, check if it's code
        if let Some(lang) = get_language_from_extension(file_path) {
            return FileType::Code(lang);
        }
        return FileType::Text;
    } else if mime_type.starts_with("image/") {
        return FileType::Image;
    } else if mime_type.starts_with("video/") {
        return FileType::Video;
    }

    // If MIME detection fails, try to detect by content
    if let Ok(_) = String::from_utf8(data[0..std::cmp::min(1024, data.len())].to_vec()) {
        // If the first 1KB is valid UTF-8, it's probably text
        // Check if it's code
        if let Some(lang) = get_language_from_extension(file_path) {
            return FileType::Code(lang);
        }
        return FileType::Text;
    }

    // Check for image magic numbers
    if data.len() >= 4 {
        // PNG signature
        if &data[0..8] == &[137, 80, 78, 71, 13, 10, 26, 10] {
            return FileType::Image;
        }
        // JPEG signature
        if &data[0..3] == &[255, 216, 255] {
            return FileType::Image;
        }
        // GIF signature
        if &data[0..6] == &[71, 73, 70, 56, 57, 97] || &data[0..6] == &[71, 73, 70, 56, 55, 97] {
            return FileType::Image;
        }
    }

    // Default to Other if we can't determine the type
    FileType::Other
}

/// Get the programming language from a file extension
fn get_language_from_extension(file_path: &str) -> Option<String> {
    let extension = file_path.split('.').last()?;

    Some(extension.to_lowercase())
    //     "rs" => Some("rust".to_string()),
    //     "js" => Some("javascript".to_string()),
    //     "ts" => Some("typescript".to_string()),
    //     "py" => Some("python".to_string()),
    //     "java" => Some("java".to_string()),
    //     "c" => Some("c".to_string()),
    //     "cpp" | "cc" | "cxx" => Some("cpp".to_string()),
    //     "h" | "hpp" => Some("cpp".to_string()),
    //     "cs" => Some("csharp".to_string()),
    //     "go" => Some("go".to_string()),
    //     "rb" => Some("ruby".to_string()),
    //     "php" => Some("php".to_string()),
    //     "html" => Some("html".to_string()),
    //     "css" => Some("css".to_string()),
    //     "json" => Some("json".to_string()),
    //     "xml" => Some("xml".to_string()),
    //     "yaml" | "yml" => Some("yaml".to_string()),
    //     "md" => Some("markdown".to_string()),
    //     "sh" | "bash" => Some("bash".to_string()),
    //     "sql" => Some("sql".to_string()),
    //     "toml" => Some("toml".to_string()),
    //     _ => None,
    // }
}

// Get or create a MutAnt-themed code theme that respects the current context
fn get_code_theme(ui: &egui::Ui) -> Arc<CodeTheme> {
    // Create a custom dark theme that matches our MutAnt theme colors
    // We don't use static storage anymore to ensure it respects theme updates
    let theme = CodeTheme::dark(0.9); // Higher contrast for better readability

    // Store in context memory for this frame
    let theme_clone = theme.clone();
    theme_clone.store_in_memory(ui.ctx());
    Arc::new(theme)
}

/// Draw a text viewer with syntax highlighting
pub fn draw_text_viewer(ui: &mut Ui, file_content: &mut FileContent) {
    // For all files, use our optimized code editor with plain text syntax
    // The optimized version can handle large files efficiently
    draw_code_editor(ui, file_content, "txt");
}

/// Draw a code editor with syntax highlighting
fn draw_code_editor(ui: &mut Ui, file_content: &mut FileContent, language: &str) {
    let theme = get_code_theme(ui);

    // Get a mutable reference to the editable content
    let content = match file_content.editable_content.as_mut() {
        Some(content) => content,
        None => {
            ui.label("Error: No editable content available");
            return;
        }
    };

    // Don't override the style - let it inherit from the root theme

    // Use a more efficient scroll area that only renders visible content
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // Use a more efficient layouter with caching
            // This is the key to performance improvement
            let mut layouter = move |ui: &egui::Ui, string: &str, wrap_width: f32| {
                // Create a new layout job with syntax highlighting
                let mut job = syntax_highlighting::highlight(
                    ui.ctx(),
                    ui.style(),
                    &theme,
                    string,
                    language,
                );

                job.wrap.max_width = wrap_width;

                // Override the font to use the theme's monospace font size (12.0)
                // This ensures the code editor uses the same font size as the rest of the UI
                if let Some(monospace_font) = ui.style().text_styles.get(&egui::TextStyle::Monospace) {
                    // Update all text sections to use the correct font
                    for section in &mut job.sections {
                        section.format.font_id = monospace_font.clone();
                    }
                }

                // Return the layout job
                ui.fonts(|f| f.layout_job(job))
            };

            // Add the TextEdit with the optimized layouter
            // Use the same font size as the rest of the UI (12.0 for monospace)
            let response = ui.add(
                egui::TextEdit::multiline(content)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace) // This will use the theme's 12.0 size
                    .code_editor()
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .interactive(true) // Make it editable
            );

            // Check if the content has changed
            if response.changed() {
                file_content.content_modified = true;
            }
        });
}

/// Draw a code viewer with syntax highlighting
pub fn draw_code_viewer(ui: &mut Ui, file_content: &mut FileContent, language: &str) {
    // Display file type information
    ui.label(format!("Code file ({})", language));
    ui.separator();

    // Use our optimized code editor with the appropriate language
    // The optimized version can handle large files efficiently
    draw_code_editor(ui, file_content, language);
}

/// Draw an image viewer
pub fn draw_image_viewer(ui: &mut Ui, texture: &TextureHandle) {
    egui::ScrollArea::both().show(ui, |ui| {
        // Display the image
        ui.add(Image::new(texture).sense(Sense::click()));
    });
}

// Declare JS bindings
pub mod bindings {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_name = initVideoPlayer)] // Use the new universal video player
        pub fn init_video_player(
            video_element_id: String,
            websocket_url: String,
            x: f32,
            y: f32,
            width: f32,
            height: f32
        );

        #[wasm_bindgen(js_name = cleanupVideoPlayer)]
        pub fn cleanup_video_player(video_element_id: String);

        #[wasm_bindgen(js_name = updateVideoPlayerPosition)]
        pub fn update_video_player_position(
            video_element_id: String,
            x: f32,
            y: f32,
            width: f32,
            height: f32,
            is_visible: bool
        );

        #[wasm_bindgen(js_name = hideVideoPlayer)]
        pub fn hide_video_player(video_element_id: String);

        #[wasm_bindgen(js_name = showVideoPlayer)]
        pub fn show_video_player(video_element_id: String);

        #[wasm_bindgen(js_name = updateVideoPlayersZIndex)]
        pub fn update_video_players_z_index(visible_player_ids: String);

        // Keep legacy binding for backward compatibility
        #[wasm_bindgen(js_name = initMpegtsPlayer)]
        pub fn init_mpegts_player(
            video_element_id: String,
            websocket_url: String,
            x: f32,
            y: f32,
            width: f32,
            height: f32
        );

        #[wasm_bindgen(js_name = cleanupMpegtsPlayer)]
        pub fn cleanup_mpegts_player(video_element_id: String);
    }
}

/// Draw a video player using the appropriate technology based on video format
/// (mpegts.js for MPEG-TS/FLV, MediaSource Extensions for MP4)
pub fn draw_video_player(ui: &mut Ui, video_ws_url: &str, file_key: &str) {
    if video_ws_url.is_empty() {
        ui.colored_label(MutantColors::ERROR, "Video URL is missing.");
        return;
    }

    ui.label(RichText::new("Video Player").size(18.0).color(MutantColors::TEXT_PRIMARY));
    ui.separator();

    // Create a unique ID for the video element that mpegts.js will use or create.
    // The file_key ensures uniqueness across multiple video players.
    let video_element_id = format!("video_element_{}", file_key.replace(|c: char| !c.is_alphanumeric(), "_"));

    // Reserve space for the video. The actual <video> tag will be created by JS.
    // This space is where JS should place the video player.
    // The JS side will need to handle the positioning.
    let desired_size = egui::vec2(640.0, 480.0); // Or make this responsive
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

    // Use a unique egui ID for storing the initialization state.
    let initialized_flag_id = egui::Id::new(&video_element_id).with("mpegts_initialized");

    let is_initialized = ui.ctx().memory_mut(|mem| mem.data.get_temp::<bool>(initialized_flag_id).unwrap_or(false));

    if !is_initialized {
        log::info!(
            "Calling JS: initVideoPlayer('{}', '{}', x: {}, y: {}, w: {}, h: {})",
            video_element_id, video_ws_url, rect.min.x, rect.min.y, rect.width(), rect.height()
        );
        // Call the JavaScript function to initialize the appropriate video player.
        // The JS function will detect the video format and choose the right player.
        bindings::init_video_player(
            video_element_id.clone(),
            video_ws_url.to_string(),
            rect.min.x,
            rect.min.y,
            rect.width(),
            rect.height()
        );

        // Store the initialization state to prevent re-initialization.
        ui.ctx().memory_mut(|mem| mem.data.insert_temp(initialized_flag_id, true));
    } else {
        // Update position if the video player is already initialized
        // For now, assume the video is visible when being drawn
        bindings::update_video_player_position(
            video_element_id.clone(),
            rect.min.x,
            rect.min.y,
            rect.width(),
            rect.height(),
            true // visible
        );
    }

    // Optional: Add a placeholder or instructions if needed.
    // For example, you could use ui.label or add a frame with a specific background.
    // The JS will overlay the video on top of this area.
    // ui.add(egui::Label::new(format!("Video player area for ID: {}", video_element_id)));

    // TODO: Handle cleanup. When the component showing the video is removed,
    // we should call `bindings::cleanup_mpegts_player(video_element_id.clone());`
    // and remove the initialized_flag_id from memory. This typically happens in a Drop impl
    // or equivalent for the component holding the video player state.
    // For now, cleanup is manual or relies on page reload.
    // Example debug cleanup:
    // if ui.button("DEBUG: Cleanup Player").clicked() {
    //     bindings::cleanup_mpegts_player(video_element_id.clone());
    //     ui.ctx().memory_mut(|mem| mem.data.remove::<bool>(initialized_flag_id));
    // }
}

/// Draw an error message for unsupported file types
pub fn draw_unsupported_file(ui: &mut Ui) {
    use crate::app::theme::MutantColors;

    ui.colored_label(
        MutantColors::ERROR,
        "This file type is not supported for viewing. You can download the file instead."
    );
}

/// Load an image from raw data
pub fn load_image(ctx: &egui::Context, data: &[u8]) -> Option<TextureHandle> {
    // Try to load the image
    let image = match image::load_from_memory(data) {
        Ok(img) => img,
        Err(_) => return None,
    };

    // Convert to RGBA
    let image_rgba = image.to_rgba8();
    let size = [image_rgba.width() as _, image_rgba.height() as _];
    let image_data = egui::ColorImage::from_rgba_unmultiplied(size, &image_rgba);

    // Create texture
    Some(ctx.load_texture(
        "image_viewer_texture",
        image_data,
        TextureOptions::default(),
    ))
}
