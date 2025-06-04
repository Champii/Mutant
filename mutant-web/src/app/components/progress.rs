use eframe::egui::{self, text::LayoutJob, ProgressBar, RichText};
use crate::app::theme::MutantColors;

pub fn progress(completion: f32, duration: String) -> ProgressBar {
    let mut text = format!("{:.1}% - {}", completion * 100.0, duration);

    if completion < 0.000001 {
        text = format!("0% - Waiting...");
    } else if completion >= 0.999999 {
        text = format!("100% - Complete");
    }

    let mut job = LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            valign: egui::Align::Center,
            ..Default::default()
        },
    );

    job.halign = egui::Align::Center;

    ProgressBar::new(completion as f32).animate(true).text(job)
}

// A more detailed progress bar that shows current/total items
pub fn detailed_progress(completion: f32, current: usize, total: usize, duration: String) -> ProgressBar {
    let mut text = format!("{}/{} - {:.1}% - {}", current, total, completion * 100.0, duration);

    if completion < 0.000001 {
        text = format!("0/{} - Waiting...", total);
    } else if completion >= 0.999999 {
        text = format!("{}/{} - Complete", total, total);
    }

    let mut job = LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            valign: egui::Align::Center,
            ..Default::default()
        },
    );

    job.halign = egui::Align::Center;

    ProgressBar::new(completion as f32).animate(true).text(job)
}

/// Modern styled progress bar for file transfers (Phase 1: Browser → Daemon)
pub fn file_transfer_progress(ui: &mut egui::Ui, completion: f32, bytes_transferred: u64, total_bytes: u64, filename: &str) {
    egui::Frame::new()
        .fill(MutantColors::SURFACE)
        .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // File icon
                ui.label(RichText::new("📁").size(16.0).color(MutantColors::ACCENT_BLUE));

                ui.vertical(|ui| {
                    // Filename
                    ui.label(RichText::new(filename).size(13.0).color(MutantColors::TEXT_PRIMARY));

                    // Progress bar
                    let progress_bar = ProgressBar::new(completion)
                        .fill(MutantColors::ACCENT_BLUE)
                        .animate(true);
                    ui.add(progress_bar);

                    // Transfer stats
                    let transferred_str = format_bytes(bytes_transferred);
                    let total_str = format_bytes(total_bytes);
                    let percentage = (completion * 100.0) as u32;

                    ui.label(RichText::new(format!("{} / {} ({}%)", transferred_str, total_str, percentage))
                        .size(11.0)
                        .color(MutantColors::TEXT_SECONDARY));
                });
            });
        });
}

/// Modern styled progress bar for network uploads (Phase 2: Daemon → Network)
pub fn network_upload_progress(
    ui: &mut egui::Ui,
    reservation_progress: f32, reserved_count: usize,
    upload_progress: f32, uploaded_count: usize,
    confirmation_progress: f32, confirmed_count: usize,
    total_chunks: usize,
    elapsed_time: String
) {
    egui::Frame::new()
        .fill(MutantColors::SURFACE)
        .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                // Header
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🌐").size(16.0).color(MutantColors::ACCENT_ORANGE));
                    ui.label(RichText::new("Uploading to Autonomi Network")
                        .size(14.0)
                        .strong()
                        .color(MutantColors::TEXT_PRIMARY));
                });

                ui.add_space(8.0);

                // Reservation phase
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Reserving:").size(12.0).color(MutantColors::TEXT_SECONDARY));
                    ui.add(ProgressBar::new(reservation_progress)
                        .fill(MutantColors::WARNING)
                        .animate(true));
                    ui.label(RichText::new(format!("{}/{}", reserved_count, total_chunks))
                        .size(11.0)
                        .color(MutantColors::TEXT_MUTED));
                });

                ui.add_space(4.0);

                // Upload phase
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Uploading:").size(12.0).color(MutantColors::TEXT_SECONDARY));
                    ui.add(ProgressBar::new(upload_progress)
                        .fill(MutantColors::ACCENT_ORANGE)
                        .animate(true));
                    ui.label(RichText::new(format!("{}/{}", uploaded_count, total_chunks))
                        .size(11.0)
                        .color(MutantColors::TEXT_MUTED));
                });

                ui.add_space(4.0);

                // Confirmation phase
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Confirming:").size(12.0).color(MutantColors::TEXT_SECONDARY));
                    ui.add(ProgressBar::new(confirmation_progress)
                        .fill(MutantColors::SUCCESS)
                        .animate(true));
                    ui.label(RichText::new(format!("{}/{}", confirmed_count, total_chunks))
                        .size(11.0)
                        .color(MutantColors::TEXT_MUTED));
                });

                ui.add_space(6.0);

                // Elapsed time
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⏱").size(12.0).color(MutantColors::ACCENT_CYAN));
                    ui.label(RichText::new(format!("Elapsed: {}", elapsed_time))
                        .size(11.0)
                        .color(MutantColors::TEXT_MUTED));
                });
            });
        });
}

/// Helper function to format bytes in human-readable format
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
