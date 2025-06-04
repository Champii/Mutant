use eframe::egui::{self, Color32, RichText, Ui, Stroke};
use log;
use mutant_protocol::StatsResponse;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use wasm_bindgen_futures;

use super::Window;
use super::theme::{self, MutantColors};

#[derive(Clone, Serialize, Deserialize)]
pub struct StatsWindow {
    #[serde(skip)]
    stats_cache: Arc<RwLock<Option<StatsResponse>>>,
    loading: bool,
    error_message: Option<String>,
    refresh_requested: bool,
}

impl Default for StatsWindow {
    fn default() -> Self {
        Self {
            stats_cache: crate::app::context::context().get_stats_cache(),
            loading: false,
            error_message: None,
            refresh_requested: true, // Request initial load
        }
    }
}

impl Window for StatsWindow {
    fn name(&self) -> String {
        "MutAnt Stats".to_string()
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        // Handle refresh request
        if self.refresh_requested && !self.loading {
            self.refresh_stats();
        }

        self.draw_header(ui);
        ui.separator();

        if let Some(error) = &self.error_message {
            self.draw_error(ui, error);
        } else if self.loading {
            self.draw_loading(ui);
        } else if let Some(stats) = self.stats_cache.read().unwrap().as_ref() {
            self.draw_stats(ui, stats);
        } else {
            self.draw_no_data(ui);
        }
    }
}

impl StatsWindow {
    pub fn new() -> Self {
        let mut window = Self::default();
        window.refresh_stats();
        window
    }

    fn draw_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Modern title with accent color
            ui.label(
                RichText::new("Storage Statistics")
                    .size(18.0)
                    .strong()
                    .color(MutantColors::TEXT_PRIMARY)
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Modern styled refresh button
                if ui.add(theme::styled_button("Refresh", MutantColors::ACCENT_BLUE)).clicked() {
                    self.refresh_stats();
                }
            });
        });
    }

    fn draw_error(&self, ui: &mut Ui, error: &str) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);

            // Modern error display with themed colors
            ui.label(
                RichText::new("Connection Error")
                    .size(16.0)
                    .strong()
                    .color(MutantColors::ERROR)
            );
            ui.add_space(8.0);

            // Error message in a subtle frame
            egui::Frame::new()
                .fill(MutantColors::SURFACE)
                .stroke(Stroke::new(1.0, MutantColors::ERROR))
                .corner_radius(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(error)
                            .size(13.0)
                            .color(MutantColors::TEXT_SECONDARY)
                    );
                });

            ui.add_space(12.0);
            ui.label(
                RichText::new("Try refreshing or check if the daemon is running.")
                    .size(12.0)
                    .color(MutantColors::TEXT_MUTED)
            );
        });
    }

    fn draw_loading(&self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            // Modern loading display
            ui.label(
                RichText::new("Loading Statistics...")
                    .size(16.0)
                    .color(MutantColors::TEXT_SECONDARY)
            );
            ui.add_space(12.0);

            // Themed spinner
            ui.add(egui::Spinner::new().size(24.0).color(MutantColors::ACCENT_BLUE));
        });
    }

    fn draw_no_data(&self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            ui.label(
                RichText::new("No Statistics Available")
                    .size(16.0)
                    .color(MutantColors::TEXT_SECONDARY)
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Click refresh to load statistics.")
                    .size(13.0)
                    .color(MutantColors::TEXT_MUTED)
            );
        });
    }

    fn draw_stats(&self, ui: &mut Ui, stats: &StatsResponse) {
        ui.add_space(10.0);

        // Overview section
        self.draw_overview_section(ui, stats);

        ui.add_space(15.0);

        // Pads breakdown section
        self.draw_pads_section(ui, stats);

        ui.add_space(15.0);

        // Storage efficiency section
        self.draw_efficiency_section(ui, stats);
    }

    fn draw_overview_section(&self, ui: &mut Ui, stats: &StatsResponse) {
        // Modern section with themed styling
        egui::Frame::new()
            .fill(MutantColors::SURFACE)
            .stroke(Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
            .corner_radius(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // Section header with accent color
                    ui.label(
                        RichText::new("Overview")
                            .size(15.0)
                            .strong()
                            .color(MutantColors::ACCENT_ORANGE)
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        self.draw_modern_stat_card(ui, "Total Keys", &stats.total_keys.to_string(), MutantColors::ACCENT_BLUE);
                        ui.add_space(12.0);
                        self.draw_modern_stat_card(ui, "Total Pads", &stats.total_pads.to_string(), MutantColors::ACCENT_PURPLE);
                    });
                });
            });
    }

    fn draw_pads_section(&self, ui: &mut Ui, stats: &StatsResponse) {
        egui::Frame::new()
            .fill(MutantColors::SURFACE)
            .stroke(Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
            .corner_radius(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // Section header
                    ui.label(
                        RichText::new("Pads Breakdown")
                            .size(15.0)
                            .strong()
                            .color(MutantColors::ACCENT_ORANGE)
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(12.0);

                    // Calculate percentages
                    let total_pads = stats.total_pads as f32;
                    let occupied_pct = if total_pads > 0.0 { (stats.occupied_pads as f32 / total_pads) * 100.0 } else { 0.0 };
                    let free_pct = if total_pads > 0.0 { (stats.free_pads as f32 / total_pads) * 100.0 } else { 0.0 };
                    let pending_pct = if total_pads > 0.0 { (stats.pending_verify_pads as f32 / total_pads) * 100.0 } else { 0.0 };

                    ui.horizontal(|ui| {
                        self.draw_modern_stat_card_with_percentage(
                            ui,
                            "Occupied",
                            &stats.occupied_pads.to_string(),
                            occupied_pct,
                            MutantColors::ACCENT_GREEN
                        );
                        ui.add_space(12.0);
                        self.draw_modern_stat_card_with_percentage(
                            ui,
                            "Free",
                            &stats.free_pads.to_string(),
                            free_pct,
                            MutantColors::TEXT_SECONDARY
                        );
                        ui.add_space(12.0);
                        self.draw_modern_stat_card_with_percentage(
                            ui,
                            "Pending",
                            &stats.pending_verify_pads.to_string(),
                            pending_pct,
                            MutantColors::WARNING
                        );
                    });

                    ui.add_space(16.0);

                    // Modern usage bar
                    self.draw_modern_usage_bar(ui, stats);
                });
            });
    }

    fn draw_efficiency_section(&self, ui: &mut Ui, stats: &StatsResponse) {
        egui::Frame::new()
            .fill(MutantColors::SURFACE)
            .stroke(Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
            .corner_radius(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // Section header
                    ui.label(
                        RichText::new("Storage Efficiency")
                            .size(15.0)
                            .strong()
                            .color(MutantColors::ACCENT_ORANGE)
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(12.0);

                    let total_pads = stats.total_pads as f32;
                    let utilization = if total_pads > 0.0 {
                        (stats.occupied_pads as f32 / total_pads) * 100.0
                    } else {
                        0.0
                    };

                    let avg_pads_per_key = if stats.total_keys > 0 {
                        stats.occupied_pads as f32 / stats.total_keys as f32
                    } else {
                        0.0
                    };

                    ui.horizontal(|ui| {
                        self.draw_modern_stat_card(
                            ui,
                            "Utilization",
                            &format!("{:.1}%", utilization),
                            if utilization > 80.0 { MutantColors::ERROR }
                            else if utilization > 60.0 { MutantColors::WARNING }
                            else { MutantColors::ACCENT_GREEN }
                        );
                        ui.add_space(12.0);
                        self.draw_modern_stat_card(
                            ui,
                            "Avg Pads/Key",
                            &format!("{:.1}", avg_pads_per_key),
                            MutantColors::ACCENT_CYAN
                        );
                    });
                });
            });
    }

    fn draw_modern_stat_card(&self, ui: &mut Ui, label: &str, value: &str, accent_color: Color32) {
        let available_width = ui.available_width();
        let card_width = (available_width - 12.0) / 2.0; // Account for spacing

        ui.allocate_ui_with_layout(
            egui::vec2(card_width, 70.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                egui::Frame::new()
                    .fill(MutantColors::BACKGROUND_LIGHT)
                    .stroke(Stroke::new(1.0, MutantColors::BORDER_LIGHT))
                    .corner_radius(6.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(label)
                                    .size(11.0)
                                    .color(MutantColors::TEXT_MUTED)
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(value)
                                    .size(18.0)
                                    .strong()
                                    .color(accent_color)
                            );
                        });
                    });
            }
        );
    }

    fn draw_modern_stat_card_with_percentage(&self, ui: &mut Ui, label: &str, value: &str, percentage: f32, accent_color: Color32) {
        let available_width = ui.available_width();
        let card_width = (available_width - 24.0) / 3.0; // Account for spacing, 3 cards

        ui.allocate_ui_with_layout(
            egui::vec2(card_width, 75.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                egui::Frame::new()
                    .fill(MutantColors::BACKGROUND_LIGHT)
                    .stroke(Stroke::new(1.0, MutantColors::BORDER_LIGHT))
                    .corner_radius(6.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(label)
                                    .size(11.0)
                                    .color(MutantColors::TEXT_MUTED)
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(value)
                                    .size(16.0)
                                    .strong()
                                    .color(accent_color)
                            );
                            ui.label(
                                RichText::new(format!("{:.1}%", percentage))
                                    .size(10.0)
                                    .color(MutantColors::TEXT_SECONDARY)
                            );
                        });
                    });
            }
        );
    }

    fn draw_modern_usage_bar(&self, ui: &mut Ui, stats: &StatsResponse) {
        ui.label(
            RichText::new("Usage Distribution")
                .size(12.0)
                .color(MutantColors::TEXT_MUTED)
        );
        ui.add_space(8.0);

        let total_pads = stats.total_pads as f32;
        if total_pads > 0.0 {
            let occupied_ratio = stats.occupied_pads as f32 / total_pads;
            let free_ratio = stats.free_pads as f32 / total_pads;
            let pending_ratio = stats.pending_verify_pads as f32 / total_pads;

            // Modern progress bar with rounded corners and subtle styling
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());

            // Background
            ui.painter().rect_filled(
                rect,
                8.0,
                MutantColors::BACKGROUND_DARK
            );

            let mut current_x = rect.min.x;

            // Draw occupied section
            if occupied_ratio > 0.0 {
                let width = rect.width() * occupied_ratio;
                let occupied_rect = egui::Rect::from_min_size(
                    egui::pos2(current_x, rect.min.y),
                    egui::vec2(width, rect.height())
                );
                ui.painter().rect_filled(occupied_rect, 8.0, MutantColors::ACCENT_GREEN);
                current_x += width;
            }

            // Draw pending section
            if pending_ratio > 0.0 {
                let width = rect.width() * pending_ratio;
                let pending_rect = egui::Rect::from_min_size(
                    egui::pos2(current_x, rect.min.y),
                    egui::vec2(width, rect.height())
                );
                ui.painter().rect_filled(pending_rect, 8.0, MutantColors::WARNING);
                current_x += width;
            }

            // Draw free section
            if free_ratio > 0.0 {
                let width = rect.width() * free_ratio;
                let free_rect = egui::Rect::from_min_size(
                    egui::pos2(current_x, rect.min.y),
                    egui::vec2(width, rect.height())
                );
                ui.painter().rect_filled(free_rect, 8.0, MutantColors::TEXT_SECONDARY);
            }

            // Subtle border
            ui.painter().rect_stroke(
                rect,
                8.0,
                Stroke::new(1.0, MutantColors::BORDER_LIGHT),
                egui::epaint::StrokeKind::Outside
            );
        }
    }

    fn refresh_stats(&mut self) {
        if self.loading {
            return; // Already loading
        }

        self.loading = true;
        self.error_message = None;
        self.refresh_requested = false;

        // Get the context and trigger real stats fetching
        let context = crate::app::context::context();
        let stats_cache = self.stats_cache.clone();

        // Spawn async task to fetch real stats from daemon
        wasm_bindgen_futures::spawn_local(async move {
            match context.get_stats().await {
                Some(stats) => {
                    // Stats are already updated in cache by context.get_stats()
                    log::info!("Successfully fetched real stats from daemon: {:?}", stats);
                },
                None => {
                    log::error!("Failed to fetch stats from daemon");
                    // Clear the cache on error
                    let mut cache = stats_cache.write().unwrap();
                    *cache = None;
                }
            }
        });

        self.loading = false;
    }
}
