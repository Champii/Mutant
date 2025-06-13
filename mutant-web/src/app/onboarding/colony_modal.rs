use eframe::egui;
use egui::{FontId, Vec2};

use crate::app::theme::MutantColors;

/// Colony initialization progress modal
pub struct ColonyInitModal {
    animation_time: f32,
}

impl Default for ColonyInitModal {
    fn default() -> Self {
        Self {
            animation_time: 0.0,
        }
    }
}

impl ColonyInitModal {
    /// Show the colony initialization modal if colony initialization is in progress
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        log::info!("ColonyInitModal::show() called");

        // Use the existing colony progress tracking system from colony_window
        let should_show = crate::app::colony_window::is_colony_operation_in_progress();

        // Also check the progress to see what we have
        let (progress, status) = crate::app::colony_window::get_colony_operation_progress();

        log::info!("Colony init modal: should_show={}, progress={}, status={:?}", should_show, progress, status);

        if !should_show {
            log::info!("Colony init modal: not showing (should_show=false)");
            return false;
        }

        log::info!("Colony init modal: SHOWING MODAL!");

        // Update animation
        self.animation_time += ctx.input(|i| i.stable_dt);

        // Show modal as a window overlay
        let mut open = true;
        egui::Window::new("Colony Initialization")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(egui::Frame::new()
                .fill(MutantColors::BACKGROUND_DARK)
                .stroke(egui::Stroke::new(2.0, MutantColors::ACCENT_ORANGE))
                .corner_radius(12.0)
                .inner_margin(20.0))
            .show(ctx, |ui| {
                self.draw_content(ui);
            });

        true
    }

    fn draw_content(&mut self, ui: &mut egui::Ui) {
        // Get progress from the existing colony progress system
        let (progress, status) = crate::app::colony_window::get_colony_operation_progress();

        ui.vertical_centered(|ui| {
            // Title
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("🌐 Initializing Colony")
                    .font(FontId::proportional(24.0))
                    .color(MutantColors::ACCENT_ORANGE)
            );

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Setting up your decentralized metadata system...")
                    .font(FontId::proportional(14.0))
                    .color(MutantColors::TEXT_SECONDARY)
            );

            ui.add_space(20.0);

            // Progress bar
            let progress_bar = egui::ProgressBar::new(progress)
                .fill(MutantColors::ACCENT_ORANGE)
                .animate(true);
            ui.add(progress_bar);

            ui.add_space(10.0);

            // Progress percentage
            ui.label(
                egui::RichText::new(format!("{:.0}%", progress * 100.0))
                    .font(FontId::proportional(12.0))
                    .color(MutantColors::TEXT_SECONDARY)
            );

            ui.add_space(10.0);

            // Status message from colony progress system
            if let Some(status) = status {
                ui.label(
                    egui::RichText::new(status)
                        .font(FontId::proportional(14.0))
                        .color(MutantColors::TEXT_SECONDARY)
                );
            }

            ui.add_space(10.0);
        });
    }
}
