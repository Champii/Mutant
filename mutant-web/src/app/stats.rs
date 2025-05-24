use eframe::egui::{self, Color32, RichText, Ui};
use log;
use mutant_protocol::StatsResponse;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use wasm_bindgen_futures::spawn_local;

use super::Window;

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
            ui.heading("📊 Storage Statistics");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Refresh button
                if ui.button("🔄 Refresh").clicked() {
                    self.refresh_stats();
                }
            });
        });
    }

    fn draw_error(&self, ui: &mut Ui, error: &str) {
        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            ui.label(RichText::new("⚠️ Error").size(24.0).color(Color32::RED));
            ui.add_space(10.0);
            ui.label(RichText::new(error).color(Color32::LIGHT_RED));
            ui.add_space(20.0);
            ui.label("Try refreshing or check if the daemon is running.");
        });
    }

    fn draw_loading(&self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            ui.label(RichText::new("⏳ Loading...").size(20.0));
            ui.add_space(10.0);
            ui.spinner();
        });
    }

    fn draw_no_data(&self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            ui.label(RichText::new("📊 No Data").size(20.0));
            ui.add_space(10.0);
            ui.label("Click refresh to load statistics.");
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
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("📋 Overview").size(16.0).strong());
                ui.separator();
                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    self.draw_stat_card(ui, "🔑 Total Keys", &stats.total_keys.to_string(), Color32::from_rgb(100, 150, 255));
                    ui.add_space(10.0);
                    self.draw_stat_card(ui, "📦 Total Pads", &stats.total_pads.to_string(), Color32::from_rgb(150, 100, 255));
                });
            });
        });
    }

    fn draw_pads_section(&self, ui: &mut Ui, stats: &StatsResponse) {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("📦 Pads Breakdown").size(16.0).strong());
                ui.separator();
                ui.add_space(5.0);

                // Calculate percentages
                let total_pads = stats.total_pads as f32;
                let occupied_pct = if total_pads > 0.0 { (stats.occupied_pads as f32 / total_pads) * 100.0 } else { 0.0 };
                let free_pct = if total_pads > 0.0 { (stats.free_pads as f32 / total_pads) * 100.0 } else { 0.0 };
                let pending_pct = if total_pads > 0.0 { (stats.pending_verify_pads as f32 / total_pads) * 100.0 } else { 0.0 };

                ui.horizontal(|ui| {
                    self.draw_stat_card_with_percentage(
                        ui,
                        "🟢 Occupied",
                        &stats.occupied_pads.to_string(),
                        occupied_pct,
                        Color32::from_rgb(100, 200, 100)
                    );
                    ui.add_space(10.0);
                    self.draw_stat_card_with_percentage(
                        ui,
                        "⚪ Free",
                        &stats.free_pads.to_string(),
                        free_pct,
                        Color32::from_rgb(200, 200, 200)
                    );
                    ui.add_space(10.0);
                    self.draw_stat_card_with_percentage(
                        ui,
                        "🟡 Pending",
                        &stats.pending_verify_pads.to_string(),
                        pending_pct,
                        Color32::from_rgb(255, 200, 100)
                    );
                });

                ui.add_space(10.0);

                // Visual progress bar for pad usage
                self.draw_usage_bar(ui, stats);
            });
        });
    }

    fn draw_efficiency_section(&self, ui: &mut Ui, stats: &StatsResponse) {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("📈 Storage Efficiency").size(16.0).strong());
                ui.separator();
                ui.add_space(5.0);

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
                    self.draw_stat_card(
                        ui,
                        "📊 Utilization",
                        &format!("{:.1}%", utilization),
                        if utilization > 80.0 { Color32::from_rgb(255, 100, 100) }
                        else if utilization > 60.0 { Color32::from_rgb(255, 200, 100) }
                        else { Color32::from_rgb(100, 200, 100) }
                    );
                    ui.add_space(10.0);
                    self.draw_stat_card(
                        ui,
                        "🔢 Avg Pads/Key",
                        &format!("{:.1}", avg_pads_per_key),
                        Color32::from_rgb(150, 150, 255)
                    );
                });
            });
        });
    }

    fn draw_stat_card(&self, ui: &mut Ui, label: &str, value: &str, color: Color32) {
        let available_width = ui.available_width();
        let card_width = (available_width - 20.0) / 2.0; // Account for spacing

        ui.allocate_ui_with_layout(
            egui::vec2(card_width, 80.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                egui::Frame::none()
                    .fill(color.gamma_multiply(0.1))
                    .stroke(egui::Stroke::new(1.0, color))
                    .rounding(8.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(label).size(12.0).color(Color32::GRAY));
                            ui.label(RichText::new(value).size(20.0).strong().color(color));
                        });
                    });
            }
        );
    }

    fn draw_stat_card_with_percentage(&self, ui: &mut Ui, label: &str, value: &str, percentage: f32, color: Color32) {
        let available_width = ui.available_width();
        let card_width = (available_width - 20.0) / 3.0; // Account for spacing, 3 cards

        ui.allocate_ui_with_layout(
            egui::vec2(card_width, 80.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                egui::Frame::none()
                    .fill(color.gamma_multiply(0.1))
                    .stroke(egui::Stroke::new(1.0, color))
                    .rounding(8.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(label).size(12.0).color(Color32::GRAY));
                            ui.label(RichText::new(value).size(18.0).strong().color(color));
                            ui.label(RichText::new(format!("{:.1}%", percentage)).size(10.0).color(Color32::GRAY));
                        });
                    });
            }
        );
    }

    fn draw_usage_bar(&self, ui: &mut Ui, stats: &StatsResponse) {
        ui.label(RichText::new("Pad Usage Distribution").size(12.0).color(Color32::GRAY));
        ui.add_space(5.0);

        let total_pads = stats.total_pads as f32;
        if total_pads > 0.0 {
            let occupied_ratio = stats.occupied_pads as f32 / total_pads;
            let free_ratio = stats.free_pads as f32 / total_pads;
            let pending_ratio = stats.pending_verify_pads as f32 / total_pads;

            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());

            let mut current_x = rect.min.x;

            // Draw occupied section
            if occupied_ratio > 0.0 {
                let width = rect.width() * occupied_ratio;
                let occupied_rect = egui::Rect::from_min_size(
                    egui::pos2(current_x, rect.min.y),
                    egui::vec2(width, rect.height())
                );
                ui.painter().rect_filled(occupied_rect, 4.0, Color32::from_rgb(100, 200, 100));
                current_x += width;
            }

            // Draw pending section
            if pending_ratio > 0.0 {
                let width = rect.width() * pending_ratio;
                let pending_rect = egui::Rect::from_min_size(
                    egui::pos2(current_x, rect.min.y),
                    egui::vec2(width, rect.height())
                );
                ui.painter().rect_filled(pending_rect, 4.0, Color32::from_rgb(255, 200, 100));
                current_x += width;
            }

            // Draw free section
            if free_ratio > 0.0 {
                let width = rect.width() * free_ratio;
                let free_rect = egui::Rect::from_min_size(
                    egui::pos2(current_x, rect.min.y),
                    egui::vec2(width, rect.height())
                );
                ui.painter().rect_filled(free_rect, 4.0, Color32::from_rgb(200, 200, 200));
            }

            // Draw border
            ui.painter().rect(rect, 4.0, Color32::TRANSPARENT, egui::Stroke::new(1.0, Color32::GRAY), egui::epaint::StrokeKind::Solid);
        }
    }

    fn refresh_stats(&mut self) {
        if self.loading {
            return; // Already loading
        }

        self.loading = true;
        self.error_message = None;
        self.refresh_requested = false;

        // For demonstration, show mock data immediately
        // In a real implementation, this would trigger an async call to fetch stats
        // and update the cache when the response arrives
        let mock_stats = StatsResponse {
            total_keys: 42,
            total_pads: 1000,
            occupied_pads: 650,
            free_pads: 300,
            pending_verify_pads: 50,
        };

        // Update the cache with mock data
        {
            let mut cache = self.stats_cache.write().unwrap();
            *cache = Some(mock_stats);
        }

        self.loading = false;
    }
}
