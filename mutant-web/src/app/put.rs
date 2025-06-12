use std::sync::{Arc, RwLock};

use eframe::egui::{self, RichText};
use mutant_protocol::StorageMode;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;
use web_time::{Duration, SystemTime, UNIX_EPOCH};

use super::Window;
use super::components::file_picker::FilePicker;
use super::components::progress::detailed_progress;
use super::context;
use super::notifications;
use super::theme::{MutantColors, success_button};

#[derive(Clone, Serialize, Deserialize)]
pub enum PutStep {
    Configuration,
    Upload,
}

impl Default for PutStep {
    fn default() -> Self {
        PutStep::Configuration
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PutWindow {
    // Current step in the upload process
    current_step: Arc<RwLock<PutStep>>,

    // File picker for selecting files from daemon filesystem
    #[serde(skip)]
    file_picker: Arc<RwLock<Option<FilePicker>>>,

    // Selected file path from the file picker
    selected_file_path: Arc<RwLock<Option<String>>>,

    // File selection state (for display)
    selected_file: Arc<RwLock<Option<String>>>,
    file_size: Arc<RwLock<Option<u64>>>,

    // Key name input
    key_name: Arc<RwLock<String>>,

    // Configuration options
    public: Arc<RwLock<bool>>,
    storage_mode: Arc<RwLock<StorageMode>>,
    no_verify: Arc<RwLock<bool>>,

    // Two-phase progress tracking for public uploads
    // Phase 1: File data upload
    phase1_reservation_progress: Arc<RwLock<f32>>,
    phase1_upload_progress: Arc<RwLock<f32>>,
    phase1_confirmation_progress: Arc<RwLock<f32>>,
    phase1_total_chunks: Arc<RwLock<usize>>,
    phase1_complete: Arc<RwLock<bool>>,

    // Phase 2: Public index upload (only for public files)
    phase2_reservation_progress: Arc<RwLock<f32>>,
    phase2_upload_progress: Arc<RwLock<f32>>,
    phase2_confirmation_progress: Arc<RwLock<f32>>,
    phase2_total_chunks: Arc<RwLock<usize>>,
    phase2_complete: Arc<RwLock<bool>>,

    // Current phase tracking
    current_phase: Arc<RwLock<u8>>, // 1 for file data, 2 for public index

    // UI state for phase collapsing
    phase1_collapsed: Arc<RwLock<bool>>,

    // Legacy fields for backward compatibility (now used for current phase)
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
            current_step: Arc::new(RwLock::new(PutStep::Configuration)),
            file_picker: Arc::new(RwLock::new(None)),
            selected_file_path: Arc::new(RwLock::new(None)),
            selected_file: Arc::new(RwLock::new(None)),
            file_size: Arc::new(RwLock::new(None)),
            key_name: Arc::new(RwLock::new(String::new())),
            public: Arc::new(RwLock::new(false)), // Default to private (false means private)
            storage_mode: Arc::new(RwLock::new(StorageMode::Heaviest)),
            no_verify: Arc::new(RwLock::new(false)),

            // Phase 1: File data upload progress
            phase1_reservation_progress: Arc::new(RwLock::new(0.0)),
            phase1_upload_progress: Arc::new(RwLock::new(0.0)),
            phase1_confirmation_progress: Arc::new(RwLock::new(0.0)),
            phase1_total_chunks: Arc::new(RwLock::new(0)),
            phase1_complete: Arc::new(RwLock::new(false)),

            // Phase 2: Public index upload progress
            phase2_reservation_progress: Arc::new(RwLock::new(0.0)),
            phase2_upload_progress: Arc::new(RwLock::new(0.0)),
            phase2_confirmation_progress: Arc::new(RwLock::new(0.0)),
            phase2_total_chunks: Arc::new(RwLock::new(0)),
            phase2_complete: Arc::new(RwLock::new(false)),

            // Current phase tracking
            current_phase: Arc::new(RwLock::new(1)), // Start with phase 1

            // UI state for phase collapsing
            phase1_collapsed: Arc::new(RwLock::new(false)),

            // Legacy fields for backward compatibility (now used for current phase)
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
            window_id: generate_wasm_compatible_id(),
        }
    }
}

impl Window for PutWindow {
    fn name(&self) -> String {
        "MutAnt Upload".to_string()
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        // If we're uploading, check the progress before drawing the form
        if *self.is_uploading.read().unwrap() && !*self.upload_complete.read().unwrap() {
            self.check_progress();
        }

        // Draw the form with the updated progress values
        self.draw_upload_form(ui);

        // Request a repaint to ensure we update frequently
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }
}

impl PutWindow {
    pub fn new() -> Self {
        Self::default()
    }

    // Create a new PutWindow with pre-selected file info (for backward compatibility)
    pub fn new_with_file(filename: String, file_size: u64) -> Self {
        let window = Self::default();

        // Set the selected file info and skip to configuration step
        *window.selected_file.write().unwrap() = Some(filename.clone());
        *window.file_size.write().unwrap() = Some(file_size);
        *window.key_name.write().unwrap() = filename;
        *window.current_step.write().unwrap() = PutStep::Configuration;

        window
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

                    // Note: Phase detection is now handled in the real-time progress receiver
                    // This polling-based checker is kept for backward compatibility but phase logic moved
                }
            }
        }
    }

    fn draw_upload_form(&mut self, ui: &mut egui::Ui) {
        let current_step = self.current_step.read().unwrap().clone();
        let is_uploading = *self.is_uploading.read().unwrap();
        let upload_complete = *self.upload_complete.read().unwrap();

        match current_step {
            PutStep::Configuration => {
                self.draw_configuration_step(ui);
            }
            PutStep::Upload => {
                if upload_complete {
                    self.draw_upload_complete_step(ui);
                } else if is_uploading {
                    self.draw_upload_progress_step(ui);
                } else {
                    // This shouldn't happen, but fallback to configuration
                    *self.current_step.write().unwrap() = PutStep::Configuration;
                }
            }
        }
    }

    fn draw_configuration_step(&mut self, ui: &mut egui::Ui) {
        // Use the full available space without any margins or headers
        let available_rect = ui.available_rect_before_wrap();
        let content_height = available_rect.height();
        let content_width = available_rect.width();

        // Force the UI to expand to the full available rectangle
        ui.expand_to_include_rect(available_rect);

        // Use horizontal layout that fills the entire space
        ui.horizontal(|ui| {
                // Left side: File picker (50% width, full height)
                let left_width = content_width * 0.5;

                ui.allocate_ui_with_layout(
                    egui::Vec2::new(left_width, content_height - 15.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        // Initialize file picker if needed
                        if self.file_picker.read().unwrap().is_none() {
                            *self.file_picker.write().unwrap() = Some(FilePicker::new().with_files_only(true));
                        }

                        // Draw the file picker using ALL available space
                        if let Some(ref mut picker) = *self.file_picker.write().unwrap() {
                            if picker.draw(ui) {
                                // File was selected - get the full filesystem path for upload
                                if let Some(full_path) = picker.selected_file_full_path() {
                                    *self.selected_file_path.write().unwrap() = Some(full_path.clone());

                                    // Extract filename from path for display
                                    let filename = std::path::Path::new(&full_path)
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_else(|| full_path.clone());

                                    *self.selected_file.write().unwrap() = Some(filename.clone());
                                    *self.key_name.write().unwrap() = filename;
                                }
                            }
                        }
                    }
                );

                // Right side: Configuration (50% width, full height) - NO spacing between panels
                let right_width = content_width * 0.5;

                ui.allocate_ui_with_layout(
                    egui::Vec2::new(right_width, content_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        // Simple centering with equal top/bottom margins
                        let available_height = ui.available_height();
                        let margin = available_height * 0.1; // 15% margin top and bottom

                        ui.add_space(margin);

                        ui.vertical_centered(|ui| {
                            // Constrain the content width for better readability
                            let max_content_width = (right_width * 0.8).min(350.0);

                            ui.allocate_ui_with_layout(
                                egui::Vec2::new(max_content_width, available_height - (margin * 2.0)),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                        // Header for configuration with gradient-like styling - centered
                        ui.vertical_centered(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("📤").size(20.0).color(MutantColors::ACCENT_ORANGE));
                                ui.add_space(8.0);
                                ui.heading(RichText::new("Upload Configuration").size(18.0).color(MutantColors::TEXT_PRIMARY));
                            });
                        });
                        ui.add_space(12.0);

                        // Show selected file info with enhanced styling - centered
                        if let Some(filename) = &*self.selected_file.read().unwrap() {
                            ui.vertical_centered(|ui| {
                                ui.group(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.vertical_centered(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("📄").size(14.0).color(MutantColors::ACCENT_BLUE));
                                            ui.label(RichText::new("Selected File").size(13.0).color(MutantColors::TEXT_SECONDARY).strong());
                                        });
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(filename).size(14.0).color(MutantColors::ACCENT_BLUE).strong());

                                        if let Some(path) = &*self.selected_file_path.read().unwrap() {
                                            ui.label(RichText::new(format!("📁 {}", path)).size(11.0).color(MutantColors::TEXT_MUTED));
                                        }
                                    });
                                });
                            });
                            ui.add_space(12.0);
                        }

                        // Key name input with enhanced styling - centered
                        ui.vertical_centered(|ui| {
                            ui.group(|ui| {
                                ui.set_min_width(ui.available_width());
                                ui.vertical_centered(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("🔑").size(14.0).color(MutantColors::ACCENT_ORANGE));
                                        ui.label(RichText::new("Key Name").size(13.0).color(MutantColors::TEXT_PRIMARY).strong());
                                    });
                                    ui.add_space(6.0);
                                    let mut key_name = self.key_name.write().unwrap();
                                    ui.text_edit_singleline(&mut *key_name);
                                });
                            });
                        });

                        ui.add_space(8.0);

                        // Privacy setting - fixed height with group for visual consistency
                        ui.group(|ui| {
                            ui.set_min_width(ui.available_width());
                            ui.set_height(80.0); // Fixed height for consistency
                            ui.vertical_centered(|ui| {
                                ui.horizontal(|ui| {
                                    let is_public = *self.public.read().unwrap();
                                    let (icon, color) = if is_public {
                                        ("🌐", MutantColors::ACCENT_GREEN)
                                    } else {
                                        ("🔒", MutantColors::ACCENT_BLUE)
                                    };
                                    ui.label(RichText::new(icon).size(14.0).color(color));
                                    ui.label(RichText::new("Privacy Setting").size(13.0).color(MutantColors::TEXT_PRIMARY).strong());
                                });

                                // Center the buttons manually by calculating position
                                let available_width = ui.available_width();
                                let button_width = 80.0; // Approximate button width
                                let spacing = 6.0;
                                let total_width = button_width * 2.0 + spacing;
                                let start_x = (available_width - total_width) / 2.0;

                                ui.horizontal(|ui| {
                                    // Add space to center the buttons
                                    ui.add_space(start_x);

                                    let mut public = self.public.write().unwrap();

                                    // Private button (default)
                                    let private_button = if !*public {
                                        egui::Button::new(RichText::new("🔒 Private").color(MutantColors::TEXT_PRIMARY))
                                            .fill(MutantColors::ACCENT_BLUE)
                                            .stroke(egui::Stroke::new(2.0, MutantColors::ACCENT_BLUE))
                                    } else {
                                        egui::Button::new(RichText::new("🔒 Private").color(MutantColors::TEXT_SECONDARY))
                                            .fill(MutantColors::SURFACE)
                                            .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
                                    };

                                    if ui.add(private_button).clicked() {
                                        *public = false;
                                    }

                                    ui.add_space(spacing);

                                    // Public button
                                    let public_button = if *public {
                                        egui::Button::new(RichText::new("🌐 Public").color(MutantColors::TEXT_PRIMARY))
                                            .fill(MutantColors::ACCENT_GREEN)
                                            .stroke(egui::Stroke::new(2.0, MutantColors::ACCENT_GREEN))
                                    } else {
                                        egui::Button::new(RichText::new("🌐 Public").color(MutantColors::TEXT_SECONDARY))
                                            .fill(MutantColors::SURFACE)
                                            .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
                                    };

                                    if ui.add(public_button).clicked() {
                                        *public = true;
                                    }
                                });

                                let description = if *self.public.read().unwrap() {
                                    "File will be publicly accessible to anyone"
                                } else {
                                    "File will be private and encrypted"
                                };
                                ui.label(RichText::new(description).size(10.0).color(MutantColors::TEXT_MUTED));
                            });
                        });

                        ui.add_space(8.0);

                        // Advanced options (renamed from "Upload Options") - centered
                        ui.vertical_centered(|ui| {
                            ui.collapsing(RichText::new("⚙ Advanced").color(MutantColors::TEXT_PRIMARY), |ui| {
                                ui.add_space(2.0);

                                // Storage mode selection with enhanced styling - centered
                                ui.vertical_centered(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("💾").size(12.0).color(MutantColors::ACCENT_BLUE));
                                        ui.label(RichText::new("Storage Mode:").color(MutantColors::TEXT_PRIMARY));
                                        ui.add_space(8.0);

                                        let mut storage_mode = self.storage_mode.write().unwrap();
                                        egui::ComboBox::new(format!("mutant_put_storage_mode_{}", self.window_id), "")
                                            .selected_text(RichText::new(format!("{:?}", *storage_mode)).color(MutantColors::ACCENT_ORANGE))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut *storage_mode, StorageMode::Light,
                                                    RichText::new("Light").color(MutantColors::TEXT_PRIMARY));
                                                ui.selectable_value(&mut *storage_mode, StorageMode::Medium,
                                                    RichText::new("Medium").color(MutantColors::TEXT_PRIMARY));
                                                ui.selectable_value(&mut *storage_mode, StorageMode::Heavy,
                                                    RichText::new("Heavy").color(MutantColors::TEXT_PRIMARY));
                                                ui.selectable_value(&mut *storage_mode, StorageMode::Heaviest,
                                                    RichText::new("Heaviest").color(MutantColors::TEXT_PRIMARY));
                                            });
                                    });
                                });

                                ui.add_space(6.0);

                                // No verify checkbox with enhanced styling - centered
                                ui.vertical_centered(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("⚡").size(12.0).color(MutantColors::WARNING));
                                        let mut no_verify = self.no_verify.write().unwrap();
                                        ui.checkbox(&mut *no_verify, RichText::new("Skip Verification").color(MutantColors::TEXT_PRIMARY));
                                    });
                                    ui.label(RichText::new("Skip verification of uploaded data (faster but less safe)").size(10.0).color(MutantColors::TEXT_MUTED));
                                });
                            });
                        });

                        ui.add_space(12.0);

                        // Upload button with enhanced styling - already centered
                        let can_upload = !self.key_name.read().unwrap().is_empty() && self.selected_file_path.read().unwrap().is_some();

                        ui.vertical_centered(|ui| {
                            let upload_button = if can_upload {
                                egui::Button::new(RichText::new("🚀 Start Upload").size(16.0).strong().color(MutantColors::TEXT_PRIMARY))
                                    .fill(MutantColors::ACCENT_ORANGE)
                                    .stroke(egui::Stroke::new(2.0, MutantColors::ACCENT_ORANGE))
                                    .min_size([200.0, 40.0].into())
                            } else {
                                egui::Button::new(RichText::new("🚀 Start Upload").size(16.0).color(MutantColors::TEXT_MUTED))
                                    .fill(MutantColors::SURFACE)
                                    .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
                                    .min_size([200.0, 40.0].into())
                            };

                            if ui.add_enabled(can_upload, upload_button).clicked() {
                                self.start_path_upload();
                            }
                        });

                        if !can_upload {
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                if self.selected_file_path.read().unwrap().is_none() {
                                    ui.label(RichText::new("⚠ Please select a file to upload").size(12.0).color(MutantColors::WARNING));
                                } else {
                                    ui.label(RichText::new("⚠ Please enter a key name").size(12.0).color(MutantColors::WARNING));
                                }
                            });
                        }

                        // Show error message if any with enhanced styling - centered
                        if let Some(error) = &*self.error_message.read().unwrap() {
                            ui.add_space(12.0);
                            ui.vertical_centered(|ui| {
                                ui.group(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.vertical_centered(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("❌").size(14.0).color(MutantColors::ERROR));
                                            ui.label(RichText::new("Error").size(13.0).color(MutantColors::ERROR).strong());
                                        });
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(error).size(12.0).color(MutantColors::ERROR));
                                    });
                                });
                            });
                        }
                                }
                            );
                        });

                    }
                );
        });
    }

    fn draw_upload_progress_step(&mut self, ui: &mut egui::Ui) {
        // Force the UI to expand to the full available rectangle, similar to configuration step
        let available_rect = ui.available_rect_before_wrap();
        ui.expand_to_include_rect(available_rect);

        let available_width = available_rect.width();
        let available_height = available_rect.height();

        // Use the full available space without any inner margins
        ui.allocate_ui_with_layout(
            egui::Vec2::new(available_width, available_height),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                // Add vertical spacing to center content
                let content_height = 300.0; // Approximate height of our content
                let top_spacing = (available_height - content_height) / 2.0;
                if top_spacing > 0.0 {
                    ui.add_space(top_spacing);
                }

                // Constrain content width for better readability
                let max_content_width = (available_width * 0.8).min(500.0);

                ui.allocate_ui_with_layout(
                    egui::Vec2::new(max_content_width, content_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        // Header
                        ui.vertical_centered(|ui| {
                            ui.heading(
                                RichText::new("🚀 Uploading...")
                                    .size(20.0)
                                    .color(MutantColors::TEXT_PRIMARY),
                            );
                        });
                        ui.add_space(15.0);

                        // Show file being uploaded
                        if let Some(filename) = &*self.selected_file.read().unwrap() {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(format!("Uploading: {}", filename))
                                        .color(MutantColors::TEXT_SECONDARY),
                                );
                            });
                            ui.add_space(10.0);
                        }

                        // Show upload progress bars
                        let is_public = *self.public.read().unwrap();
                        let current_phase = *self.current_phase.read().unwrap();
                        let phase1_complete = *self.phase1_complete.read().unwrap();
                        let phase2_complete = *self.phase2_complete.read().unwrap();
                        let elapsed = *self.elapsed_time.read().unwrap();
                        let elapsed_str = format!("{:.1}s", elapsed.as_secs_f64());

                        // Progress bars with full width
                        ui.vertical(|ui| {
                            if is_public {
                                // Two-phase progress for public uploads

                                // Phase 1: File Data Upload
                                ui.group(|ui| {
                                    ui.vertical(|ui| {
                                        let phase1_icon = if phase1_complete { "✅" } else { "📤" };
                                        let phase1_color = if phase1_complete {
                                            MutantColors::ACCENT_GREEN
                                        } else if current_phase == 1 {
                                            MutantColors::ACCENT_ORANGE
                                        } else {
                                            MutantColors::TEXT_MUTED
                                        };

                                        // Phase 1 header with collapse button when complete
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{} Phase 1: Uploading file data", phase1_icon))
                                                    .color(phase1_color)
                                                    .strong()
                                            );

                                            // Add collapse button when phase 1 is complete
                                            if phase1_complete {
                                                let phase1_collapsed = *self.phase1_collapsed.read().unwrap();
                                                let collapse_icon = if phase1_collapsed { "▶" } else { "▼" };
                                                if ui.small_button(collapse_icon).clicked() {
                                                    *self.phase1_collapsed.write().unwrap() = !phase1_collapsed;
                                                }
                                            }
                                        });

                                        // Show phase 1 details only if not collapsed or not complete
                                        let phase1_collapsed = *self.phase1_collapsed.read().unwrap();
                                        if !phase1_collapsed || !phase1_complete {
                                            if phase1_complete {
                                                // Show completed Phase 1 progress
                                                let phase1_total = *self.phase1_total_chunks.read().unwrap();
                                                ui.label("Reserving pads:");
                                                ui.add_sized([max_content_width, 20.0], detailed_progress(1.0, phase1_total, phase1_total, elapsed_str.clone()));
                                                ui.label("Uploading pads:");
                                                ui.add_sized([max_content_width, 20.0], detailed_progress(1.0, phase1_total, phase1_total, elapsed_str.clone()));
                                                ui.label("Confirming pads:");
                                                ui.add_sized([max_content_width, 20.0], detailed_progress(1.0, phase1_total, phase1_total, elapsed_str.clone()));
                                            } else if current_phase == 1 {
                                                // Show current Phase 1 progress
                                                let total_chunks = *self.total_chunks.read().unwrap();
                                                let reservation_progress = *self.reservation_progress.read().unwrap();
                                                let upload_progress = *self.upload_progress.read().unwrap();
                                                let confirmation_progress = *self.confirmation_progress.read().unwrap();

                                                ui.label("Reserving pads:");
                                                ui.add_sized([max_content_width, 20.0], detailed_progress(reservation_progress, (reservation_progress * total_chunks as f32) as usize, total_chunks, elapsed_str.clone()));
                                                ui.label("Uploading pads:");
                                                ui.add_sized([max_content_width, 20.0], detailed_progress(upload_progress, (upload_progress * total_chunks as f32) as usize, total_chunks, elapsed_str.clone()));
                                                ui.label("Confirming pads:");
                                                ui.add_sized([max_content_width, 20.0], detailed_progress(confirmation_progress, (confirmation_progress * total_chunks as f32) as usize, total_chunks, elapsed_str.clone()));
                                            } else {
                                                // Phase 1 not started yet
                                                ui.label("Waiting to start...");
                                            }
                                        }
                                    });
                                });

                                ui.add_space(10.0);

                                // Phase 2: Public Index Upload
                                ui.group(|ui| {
                                    ui.vertical(|ui| {
                                        let phase2_icon = if phase2_complete { "✅" } else if current_phase == 2 { "🌐" } else { "⏳" };
                                        let phase2_color = if phase2_complete {
                                            MutantColors::ACCENT_GREEN
                                        } else if current_phase == 2 {
                                            MutantColors::ACCENT_BLUE
                                        } else {
                                            MutantColors::TEXT_MUTED
                                        };

                                        ui.label(
                                            RichText::new(format!("{} Phase 2: Uploading public index", phase2_icon))
                                                .color(phase2_color)
                                                .strong()
                                        );

                                        if phase2_complete {
                                            // Show completed Phase 2 progress
                                            let phase2_total = *self.phase2_total_chunks.read().unwrap();
                                            ui.label("Reserving pads:");
                                            ui.add_sized([max_content_width, 20.0], detailed_progress(1.0, phase2_total, phase2_total, elapsed_str.clone()));
                                            ui.label("Uploading pads:");
                                            ui.add_sized([max_content_width, 20.0], detailed_progress(1.0, phase2_total, phase2_total, elapsed_str.clone()));
                                            ui.label("Confirming pads:");
                                            ui.add_sized([max_content_width, 20.0], detailed_progress(1.0, phase2_total, phase2_total, elapsed_str.clone()));
                                        } else if current_phase == 2 {
                                            // Show current Phase 2 progress
                                            let total_chunks = *self.total_chunks.read().unwrap();
                                            let reservation_progress = *self.reservation_progress.read().unwrap();
                                            let upload_progress = *self.upload_progress.read().unwrap();
                                            let confirmation_progress = *self.confirmation_progress.read().unwrap();

                                            ui.label("Reserving pads:");
                                            ui.add_sized([max_content_width, 20.0], detailed_progress(reservation_progress, (reservation_progress * total_chunks as f32) as usize, total_chunks, elapsed_str.clone()));
                                            ui.label("Uploading pads:");
                                            ui.add_sized([max_content_width, 20.0], detailed_progress(upload_progress, (upload_progress * total_chunks as f32) as usize, total_chunks, elapsed_str.clone()));
                                            ui.label("Confirming pads:");
                                            ui.add_sized([max_content_width, 20.0], detailed_progress(confirmation_progress, (confirmation_progress * total_chunks as f32) as usize, total_chunks, elapsed_str.clone()));
                                        } else {
                                            // Phase 2 not started yet
                                            ui.label("Waiting for Phase 1 to complete...");
                                        }
                                    });
                                });
                            } else {
                                // Single-phase progress for private uploads
                                let total_chunks = *self.total_chunks.read().unwrap();
                                let reservation_progress = *self.reservation_progress.read().unwrap();
                                let upload_progress = *self.upload_progress.read().unwrap();
                                let confirmation_progress = *self.confirmation_progress.read().unwrap();

                                // Reservation progress
                                ui.label("Reserving pads:");
                                ui.add_sized(
                                    [max_content_width, 20.0],
                                    detailed_progress(
                                        reservation_progress,
                                        (reservation_progress * total_chunks as f32) as usize,
                                        total_chunks,
                                        elapsed_str.clone(),
                                    ),
                                );

                                ui.add_space(5.0);

                                // Upload progress
                                ui.label("Uploading pads:");
                                ui.add_sized(
                                    [max_content_width, 20.0],
                                    detailed_progress(
                                        upload_progress,
                                        (upload_progress * total_chunks as f32) as usize,
                                        total_chunks,
                                        elapsed_str.clone(),
                                    ),
                                );

                                ui.add_space(5.0);

                                // Confirmation progress
                                ui.label("Confirming pads:");
                                ui.add_sized(
                                    [max_content_width, 20.0],
                                    detailed_progress(
                                        confirmation_progress,
                                        (confirmation_progress * total_chunks as f32) as usize,
                                        total_chunks,
                                        elapsed_str,
                                    ),
                                );
                            }
                        });
                    },
                );
            },
        );
    }

    fn draw_upload_complete_step(&mut self, ui: &mut egui::Ui) {
        // Force the UI to expand to the full available rectangle, similar to configuration step
        let available_rect = ui.available_rect_before_wrap();
        ui.expand_to_include_rect(available_rect);

        let available_width = available_rect.width();
        let available_height = available_rect.height();

        // Use the full available space without any inner margins
        ui.allocate_ui_with_layout(
            egui::Vec2::new(available_width, available_height),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                // Add vertical spacing to center content
                let content_height = 250.0; // Approximate height of our content
                let top_spacing = (available_height - content_height) / 2.0;
                if top_spacing > 0.0 {
                    ui.add_space(top_spacing);
                }

                // Center everything using the same mechanism as the title
                ui.vertical_centered(|ui| {
                    // Header
                    ui.heading(
                        RichText::new("✅ Upload Complete!")
                            .size(20.0)
                            .color(MutantColors::ACCENT_GREEN),
                    );

                    ui.add_space(15.0);

                    ui.label(
                        RichText::new("Successfully uploaded:").color(MutantColors::TEXT_SECONDARY),
                    );
                    // ui.label(RichText::new(filename).color(MutantColors::ACCENT_BLUE));

                    if let Some(filename) = &*self.selected_file.read().unwrap() {
                        ui.label(RichText::new(filename).color(MutantColors::ACCENT_BLUE));
                    }

                    ui.label(
                        RichText::new(format!("Key: {}", self.key_name.read().unwrap()))
                            .color(MutantColors::TEXT_PRIMARY),
                    );

                    let elapsed = *self.elapsed_time.read().unwrap();
                    ui.label(
                        RichText::new(format!("Time: {:.1}s", elapsed.as_secs_f64()))
                            .color(MutantColors::TEXT_MUTED),
                    );

                    ui.add_space(10.0);

                    // Show public address if available - using the same centering as title
                    if let Some(public_addr) = &*self.public_address.read().unwrap() {
                        ui.vertical_centered(|ui| {
                            ui.group(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("Public Address:")
                                            .color(MutantColors::TEXT_SECONDARY),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(public_addr)
                                                .color(MutantColors::ACCENT_ORANGE),
                                        );
                                        if ui.button("📋 Copy").clicked() {
                                            ui.ctx().copy_text(public_addr.clone());
                                            notifications::info(
                                                "Public address copied to clipboard!".to_string(),
                                            );
                                        }
                                    });
                                });
                            });
                        });
                        ui.add_space(10.0);
                    }

                    // Action button
                    if ui.add(success_button("🔄 Upload Another")).clicked() {
                        self.reset();
                    }
                });
            },
        );
    }

    fn start_path_upload(&self) {
        let file_path = match &*self.selected_file_path.read().unwrap() {
            Some(path) => path.clone(),
            None => {
                *self.error_message.write().unwrap() = Some("No file selected".to_string());
                return;
            }
        };

        let key_name = self.key_name.read().unwrap().clone();
        if key_name.is_empty() {
            *self.error_message.write().unwrap() = Some("Key name cannot be empty".to_string());
            return;
        }

        // Clear any previous error
        *self.error_message.write().unwrap() = None;

        // Set upload state
        *self.is_uploading.write().unwrap() = true;
        *self.upload_complete.write().unwrap() = false;
        *self.start_time.write().unwrap() = Some(SystemTime::now());
        *self.current_step.write().unwrap() = PutStep::Upload;

        // Reset phase tracking
        *self.current_phase.write().unwrap() = 1; // Start with Phase 1
        *self.phase1_complete.write().unwrap() = false;
        *self.phase2_complete.write().unwrap() = false;
        *self.phase1_collapsed.write().unwrap() = false;

        // Reset all progress
        *self.reservation_progress.write().unwrap() = 0.0;
        *self.upload_progress.write().unwrap() = 0.0;
        *self.confirmation_progress.write().unwrap() = 0.0;

        // Reset Phase 1 progress
        *self.phase1_reservation_progress.write().unwrap() = 0.0;
        *self.phase1_upload_progress.write().unwrap() = 0.0;
        *self.phase1_confirmation_progress.write().unwrap() = 0.0;
        *self.phase1_total_chunks.write().unwrap() = 0;

        // Reset Phase 2 progress
        *self.phase2_reservation_progress.write().unwrap() = 0.0;
        *self.phase2_upload_progress.write().unwrap() = 0.0;
        *self.phase2_confirmation_progress.write().unwrap() = 0.0;
        *self.phase2_total_chunks.write().unwrap() = 0;

        // Start the upload using file path
        let ctx = context::context();
        let public = *self.public.read().unwrap();
        let storage_mode = self.storage_mode.read().unwrap().clone();
        let no_verify = *self.no_verify.read().unwrap();

        let current_put_id = self.current_put_id.clone();
        let is_uploading = self.is_uploading.clone();
        let error_message = self.error_message.clone();

        // Create progress tracking for this upload (not used anymore, but kept for compatibility)
        let (_progress_id, _progress) = ctx.create_progress(&key_name, &file_path);

        // Store progress references for UI updates
        let reservation_progress = self.reservation_progress.clone();
        let upload_progress = self.upload_progress.clone();
        let confirmation_progress = self.confirmation_progress.clone();
        let upload_complete = self.upload_complete.clone();
        let total_chunks = self.total_chunks.clone();
        let chunks_to_reserve = self.chunks_to_reserve.clone();
        let initial_written_count = self.initial_written_count.clone();
        let initial_confirmed_count = self.initial_confirmed_count.clone();

        // Store phase tracking references
        let current_phase = self.current_phase.clone();
        let phase1_complete = self.phase1_complete.clone();
        let phase2_complete = self.phase2_complete.clone();
        let phase1_reservation_progress = self.phase1_reservation_progress.clone();
        let phase1_upload_progress = self.phase1_upload_progress.clone();
        let phase1_confirmation_progress = self.phase1_confirmation_progress.clone();
        let phase1_total_chunks = self.phase1_total_chunks.clone();
        let phase2_reservation_progress = self.phase2_reservation_progress.clone();
        let phase2_upload_progress = self.phase2_upload_progress.clone();
        let phase2_confirmation_progress = self.phase2_confirmation_progress.clone();
        let phase2_total_chunks = self.phase2_total_chunks.clone();
        let public_address = self.public_address.clone();
        let key_name_clone = key_name.clone();

        spawn_local(async move {
            log::info!("Starting file path upload: {} -> {}", file_path, key_name);
            match ctx
                .put_file_path(&key_name, &file_path, public, storage_mode, no_verify)
                .await
            {
                Ok((put_id, mut progress_rx)) => {
                    log::info!(
                        "File path upload started successfully with put_id: {}",
                        put_id
                    );
                    *current_put_id.write().unwrap() = Some(put_id.clone());

                    // Start a progress monitoring task using the real progress receiver
                    let is_uploading_clone = is_uploading.clone();
                    let upload_complete_clone = upload_complete.clone();
                    let _error_message_clone = error_message.clone();

                    spawn_local(async move {
                        // Listen for real progress updates from the daemon
                        while let Some(progress_result) = progress_rx.recv().await {
                            match progress_result {
                                Ok(task_progress) => {
                                    log::debug!("Received progress update: {:?}", task_progress);

                                    // Handle different types of progress updates
                                    match task_progress {
                                        mutant_protocol::TaskProgress::Put(put_event) => {
                                            match put_event {
                                                mutant_protocol::PutEvent::Starting {
                                                    total_chunks: event_total_chunks,
                                                    initial_written_count:
                                                        event_initial_written_count,
                                                    initial_confirmed_count:
                                                        event_initial_confirmed_count,
                                                    chunks_to_reserve: event_chunks_to_reserve,
                                                } => {
                                                    *total_chunks.write().unwrap() =
                                                        event_total_chunks;
                                                    *chunks_to_reserve.write().unwrap() =
                                                        event_chunks_to_reserve;
                                                    *initial_written_count.write().unwrap() =
                                                        event_initial_written_count;
                                                    *initial_confirmed_count.write().unwrap() =
                                                        event_initial_confirmed_count;
                                                    log::info!(
                                                        "Put operation starting: {} total chunks, {} to reserve",
                                                        event_total_chunks,
                                                        event_chunks_to_reserve
                                                    );
                                                }
                                                mutant_protocol::PutEvent::PadReserved => {
                                                    // Update reservation progress
                                                    let chunks_to_reserve_val =
                                                        *chunks_to_reserve.read().unwrap();
                                                    if chunks_to_reserve_val > 0 {
                                                        let current_reserved =
                                                            *reservation_progress.read().unwrap()
                                                                * chunks_to_reserve_val as f32;
                                                        let new_progress = (current_reserved + 1.0)
                                                            / chunks_to_reserve_val as f32;
                                                        *reservation_progress.write().unwrap() =
                                                            new_progress.min(1.0);
                                                    }
                                                }
                                                mutant_protocol::PutEvent::PadsWritten => {
                                                    // Update upload progress
                                                    let total_chunks_val =
                                                        *total_chunks.read().unwrap();
                                                    if total_chunks_val > 0 {
                                                        let current_written =
                                                            *upload_progress.read().unwrap()
                                                                * total_chunks_val as f32;
                                                        let new_progress = (current_written + 1.0)
                                                            / total_chunks_val as f32;
                                                        *upload_progress.write().unwrap() =
                                                            new_progress.min(1.0);
                                                    }
                                                }
                                                mutant_protocol::PutEvent::PadsConfirmed => {
                                                    // Update confirmation progress
                                                    let total_chunks_val =
                                                        *total_chunks.read().unwrap();
                                                    if total_chunks_val > 0 {
                                                        let current_confirmed =
                                                            *confirmation_progress.read().unwrap()
                                                                * total_chunks_val as f32;
                                                        let new_progress = (current_confirmed
                                                            + 1.0)
                                                            / total_chunks_val as f32;
                                                        *confirmation_progress.write().unwrap() =
                                                            new_progress.min(1.0);
                                                    }
                                                }
                                                mutant_protocol::PutEvent::Complete => {
                                                    log::info!("Put operation completed successfully");

                                                    // For phase tracking, detect if this is the first completion (Phase 1)
                                                    let current_phase_val = *current_phase.read().unwrap();
                                                    let is_public = public;

                                                    if is_public && current_phase_val == 1 {
                                                        // This is the end of Phase 1 for a public upload
                                                        *phase1_complete.write().unwrap() = true;

                                                        // Store Phase 1 progress
                                                        *phase1_reservation_progress.write().unwrap() = *reservation_progress.read().unwrap();
                                                        *phase1_upload_progress.write().unwrap() = *upload_progress.read().unwrap();
                                                        *phase1_confirmation_progress.write().unwrap() = *confirmation_progress.read().unwrap();
                                                        *phase1_total_chunks.write().unwrap() = *total_chunks.read().unwrap();

                                                        // Move to Phase 2 for UI display
                                                        *current_phase.write().unwrap() = 2;

                                                        // Reset current progress for Phase 2 display
                                                        *reservation_progress.write().unwrap() = 0.0;
                                                        *upload_progress.write().unwrap() = 0.0;
                                                        *confirmation_progress.write().unwrap() = 0.0;
                                                        *total_chunks.write().unwrap() = 0;

                                                        log::info!("Phase 1 complete, transitioning to Phase 2 display");
                                                        // Continue listening for Phase 2 events - don't break yet
                                                    } else if is_public && current_phase_val == 2 {
                                                        // This is the end of Phase 2 for a public upload
                                                        *phase2_complete.write().unwrap() = true;

                                                        // Store Phase 2 progress
                                                        *phase2_reservation_progress.write().unwrap() = *reservation_progress.read().unwrap();
                                                        *phase2_upload_progress.write().unwrap() = *upload_progress.read().unwrap();
                                                        *phase2_confirmation_progress.write().unwrap() = *confirmation_progress.read().unwrap();
                                                        *phase2_total_chunks.write().unwrap() = *total_chunks.read().unwrap();

                                                        log::info!("Phase 2 complete, upload finished (auto-indexing happens in background)");

                                                        // Mark upload as complete - auto-indexing happens in background
                                                        *upload_complete_clone.write().unwrap() = true;
                                                        *is_uploading_clone.write().unwrap() = false;

                                                        // Refresh the keys list and get public address
                                                        spawn_local(async move {
                                                            let ctx = context::context();
                                                            let _ = ctx.list_keys().await;
                                                        });

                                                        // Break the loop since upload is complete
                                                        break;
                                                    } else {
                                                        // This is either:
                                                        // 1. A private upload completion (single phase)
                                                        // 2. The final completion for a public upload (after auto-indexing)
                                                        *upload_complete_clone.write().unwrap() = true;
                                                        *is_uploading_clone.write().unwrap() = false;

                                                        // Refresh the keys list and get public address
                                                        spawn_local(async move {
                                                            let ctx = context::context();
                                                            let _ = ctx.list_keys().await;

                                                            // For public uploads, fetch the public address
                                                            if is_public {
                                                                let keys = ctx.list_keys().await;
                                                                for key in keys {
                                                                    if key.key == key_name_clone && key.is_public {
                                                                        if let Some(addr) = key.public_address {
                                                                            *public_address.write().unwrap() = Some(addr);
                                                                            break;
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        });

                                                        log::info!("Upload completed successfully");
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        _ => {
                                            log::debug!(
                                                "Received non-put progress update: {:?}",
                                                task_progress
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::error!("Progress update error: {}", e);
                                    *is_uploading_clone.write().unwrap() = false;
                                    break;
                                }
                            }
                        }

                        log::info!("Progress monitoring task completed");

                        ctx.list_keys().await;
                    });
                }
                Err(e) => {
                    *is_uploading.write().unwrap() = false;
                    *error_message.write().unwrap() =
                        Some(format!("Failed to start upload: {}", e));
                }
            }
        });
    }

    fn complete_upload(&self) {
        // Mark upload as complete
        *self.is_uploading.write().unwrap() = false;
        *self.upload_complete.write().unwrap() = true;
        *self.current_step.write().unwrap() = PutStep::Upload;

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

    fn reset(&self) {
        *self.current_step.write().unwrap() = PutStep::Configuration;
        *self.file_picker.write().unwrap() = None;
        *self.selected_file_path.write().unwrap() = None;
        *self.selected_file.write().unwrap() = None;
        *self.file_size.write().unwrap() = None;
        *self.key_name.write().unwrap() = String::new();
        *self.public.write().unwrap() = false; // Reset to private
        *self.storage_mode.write().unwrap() = StorageMode::Heaviest;
        *self.no_verify.write().unwrap() = false;
        *self.is_uploading.write().unwrap() = false;
        *self.upload_complete.write().unwrap() = false;
        *self.public_address.write().unwrap() = None;
        *self.error_message.write().unwrap() = None;
        *self.current_put_id.write().unwrap() = None;

        // Reset phase tracking
        *self.current_phase.write().unwrap() = 1;
        *self.phase1_complete.write().unwrap() = false;
        *self.phase2_complete.write().unwrap() = false;
        *self.phase1_collapsed.write().unwrap() = false;

        // Reset Phase 1 progress
        *self.phase1_reservation_progress.write().unwrap() = 0.0;
        *self.phase1_upload_progress.write().unwrap() = 0.0;
        *self.phase1_confirmation_progress.write().unwrap() = 0.0;
        *self.phase1_total_chunks.write().unwrap() = 0;

        // Reset Phase 2 progress
        *self.phase2_reservation_progress.write().unwrap() = 0.0;
        *self.phase2_upload_progress.write().unwrap() = 0.0;
        *self.phase2_confirmation_progress.write().unwrap() = 0.0;
        *self.phase2_total_chunks.write().unwrap() = 0;

        // Reset current progress (legacy fields)
        *self.reservation_progress.write().unwrap() = 0.0;
        *self.upload_progress.write().unwrap() = 0.0;
        *self.confirmation_progress.write().unwrap() = 0.0;
        *self.total_chunks.write().unwrap() = 0;
        *self.chunks_to_reserve.write().unwrap() = 0;
        *self.initial_written_count.write().unwrap() = 0;
        *self.initial_confirmed_count.write().unwrap() = 0;
        *self.start_time.write().unwrap() = None;
        *self.elapsed_time.write().unwrap() = Duration::from_secs(0);
    }
}

/// Generate a WASM-compatible unique ID using timestamp and random values
fn generate_wasm_compatible_id() -> String {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let random_value = (js_sys::Math::random() * 1_000_000_000.0) as u64;
    format!("id_{}_{}", timestamp, random_value)
}
