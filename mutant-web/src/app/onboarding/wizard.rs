use eframe::egui;
use egui::{Align2, FontId, Vec2};

use crate::app::theme::{self, MutantColors};
use super::{onboarding_state, OnboardingStep, OnboardingState};

/// Full-screen onboarding wizard modal
pub struct OnboardingWizard {
    animation_time: f32,
}

impl Default for OnboardingWizard {
    fn default() -> Self {
        Self {
            animation_time: 0.0,
        }
    }
}

impl OnboardingWizard {
    /// Show the onboarding wizard as a full-screen modal
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let state = onboarding_state();
        let should_show = if let Ok(state) = state.read() {
            let should_show = state.needs_onboarding && state.current_step != OnboardingStep::Completed;
            log::debug!("Onboarding wizard: needs_onboarding={}, current_step={:?}, should_show={}",
                state.needs_onboarding, state.current_step, should_show);
            should_show
        } else {
            log::error!("Failed to get onboarding state lock in wizard show");
            false
        };

        if !should_show {
            log::debug!("Onboarding wizard: not showing");
            return false;
        }

        log::debug!("Onboarding wizard: showing modal");

        // Update animation
        self.animation_time += ctx.input(|i| i.stable_dt);

        // Content area using a central panel instead of window to avoid issues
        egui::CentralPanel::default()
            .frame(egui::Frame::new()
                .fill(MutantColors::BACKGROUND_DARK)
                .stroke(egui::Stroke::new(2.0, MutantColors::ACCENT_ORANGE))
                .corner_radius(12.0)
                .inner_margin(20.0))
            .show(ctx, |ui| {
                // Draw content based on current step
                if let Ok(mut state) = state.write() {
                    self.draw_content(ui, &mut state);
                } else {
                    ui.label("Loading onboarding...");
                }
            });

        true
    }

    fn draw_content(&mut self, ui: &mut egui::Ui, state: &mut OnboardingState) {
        ui.vertical_centered(|ui| {
            // Title
            ui.add_space(20.0);
            ui.label(
                egui::RichText::new("🚀 Welcome to MutAnt")
                    .font(FontId::proportional(32.0))
                    .color(MutantColors::ACCENT_ORANGE)
            );
            
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Let's get you set up with decentralized storage")
                    .font(FontId::proportional(16.0))
                    .color(MutantColors::TEXT_SECONDARY)
            );

            ui.add_space(30.0);

            // Step indicator
            self.draw_step_indicator(ui, &state.current_step);

            ui.add_space(30.0);

            // Step content
            match state.current_step {
                OnboardingStep::CheckingStatus => {
                    self.draw_checking_status(ui, state);
                }
                OnboardingStep::CredentialCollection => {
                    self.draw_credential_collection(ui, state);
                }
                OnboardingStep::ColonyInitialization => {
                    self.draw_colony_initialization(ui, state);
                }
                OnboardingStep::Completed => {
                    self.draw_completion(ui, state);
                }
            }
        });
    }

    fn draw_step_indicator(&self, ui: &mut egui::Ui, current_step: &OnboardingStep) {
        ui.horizontal(|ui| {
            let steps = [
                ("1", "Credentials", OnboardingStep::CredentialCollection),
                ("2", "Initialize", OnboardingStep::ColonyInitialization),
                ("3", "Complete", OnboardingStep::Completed),
            ];

            for (i, (number, label, step)) in steps.iter().enumerate() {
                if i > 0 {
                    // Draw connector line
                    let _line_color = if matches!(current_step, OnboardingStep::ColonyInitialization | OnboardingStep::Completed) && i == 1 {
                        MutantColors::ACCENT_ORANGE
                    } else if matches!(current_step, OnboardingStep::Completed) && i == 2 {
                        MutantColors::ACCENT_ORANGE
                    } else {
                        MutantColors::BORDER_MEDIUM
                    };

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                }

                // Step circle
                let is_current = std::mem::discriminant(current_step) == std::mem::discriminant(step);
                let is_completed = match (current_step, step) {
                    (OnboardingStep::ColonyInitialization, OnboardingStep::CredentialCollection) => true,
                    (OnboardingStep::Completed, OnboardingStep::CredentialCollection | OnboardingStep::ColonyInitialization) => true,
                    _ => false,
                };

                let circle_color = if is_completed {
                    MutantColors::SUCCESS
                } else if is_current {
                    MutantColors::ACCENT_ORANGE
                } else {
                    MutantColors::BORDER_MEDIUM
                };

                ui.vertical(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(40.0, 40.0),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(30.0, 30.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 15.0, circle_color);
                            ui.painter().text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                if is_completed { "✓" } else { number },
                                FontId::proportional(14.0),
                                MutantColors::TEXT_PRIMARY,
                            );
                        }
                    );
                    
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new(*label)
                            .font(FontId::proportional(12.0))
                            .color(if is_current || is_completed { MutantColors::TEXT_PRIMARY } else { MutantColors::TEXT_SECONDARY })
                    );
                });
            }
        });
    }

    fn draw_checking_status(&self, ui: &mut egui::Ui, _state: &mut OnboardingState) {
        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            
            // Spinner
            ui.spinner();
            ui.add_space(20.0);
            
            ui.label(
                egui::RichText::new("Checking system status...")
                    .font(FontId::proportional(18.0))
                    .color(MutantColors::TEXT_PRIMARY)
            );
        });
    }

    fn draw_credential_collection(&self, ui: &mut egui::Ui, state: &mut OnboardingState) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Set up your credentials")
                    .font(FontId::proportional(20.0))
                    .color(MutantColors::TEXT_PRIMARY)
            );
            
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("MutAnt needs a private key for network access and a colony mnemonic for metadata management.")
                    .font(FontId::proportional(14.0))
                    .color(MutantColors::TEXT_SECONDARY)
            );

            ui.add_space(20.0);

            // Private Key Section
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("🔑 Private Key")
                                .font(FontId::proportional(16.0))
                                .color(MutantColors::ACCENT_BLUE)
                        );
                    });
                    
                    ui.add_space(10.0);
                    
                    ui.checkbox(&mut state.auto_generate_private_key, "Auto-generate a new private key");
                    
                    if !state.auto_generate_private_key {
                        ui.add_space(5.0);
                        ui.label("Enter your private key (64 hex characters):");
                        ui.text_edit_singleline(&mut state.private_key);
                    }
                });
            });

            ui.add_space(15.0);

            // Colony Mnemonic Section
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("🌐 Colony Mnemonic")
                                .font(FontId::proportional(16.0))
                                .color(MutantColors::ACCENT_GREEN)
                        );
                    });
                    
                    ui.add_space(10.0);
                    
                    ui.checkbox(&mut state.auto_generate_colony_mnemonic, "Auto-generate a new colony mnemonic");
                    
                    if !state.auto_generate_colony_mnemonic {
                        ui.add_space(5.0);
                        ui.label("Enter your colony mnemonic (12 words):");
                        ui.text_edit_multiline(&mut state.colony_mnemonic);
                    }
                });
            });

            ui.add_space(20.0);

            // Error message
            if let Some(error) = &state.error_message {
                ui.colored_label(MutantColors::ERROR, error);
                ui.add_space(10.0);
            }

            // Action buttons
            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("Continue")).clicked() && !state.is_loading {
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Ok(mut state) = onboarding_state().write() {
                            let _ = state.submit_credentials().await;
                        }
                    });
                }

                if state.is_loading {
                    ui.spinner();
                }
            });
        });
    }

    fn draw_colony_initialization(&self, ui: &mut egui::Ui, state: &mut OnboardingState) {
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Initializing Colony")
                    .font(FontId::proportional(20.0))
                    .color(MutantColors::TEXT_PRIMARY)
            );
            
            ui.add_space(20.0);
            
            // Progress bar
            let progress = egui::ProgressBar::new(state.colony_initialization_progress)
                .fill(MutantColors::ACCENT_ORANGE)
                .animate(state.is_loading);
            ui.add(progress);
            
            ui.add_space(10.0);
            
            if let Some(status) = &state.colony_initialization_status {
                ui.label(
                    egui::RichText::new(status)
                        .font(FontId::proportional(14.0))
                        .color(MutantColors::TEXT_SECONDARY)
                );
            }

            // Error message
            if let Some(error) = &state.error_message {
                ui.add_space(10.0);
                ui.colored_label(MutantColors::ERROR, error);
            }

            // Auto-start initialization
            if !state.is_loading && state.colony_initialization_progress == 0.0 {
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(mut state) = onboarding_state().write() {
                        let _ = state.initialize_colony().await;
                    }
                });
            }
        });
    }

    fn draw_completion(&self, ui: &mut egui::Ui, state: &mut OnboardingState) {
        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            
            ui.label(
                egui::RichText::new("🎉 Setup Complete!")
                    .font(FontId::proportional(24.0))
                    .color(MutantColors::SUCCESS)
            );
            
            ui.add_space(20.0);
            
            ui.label(
                egui::RichText::new("MutAnt is now ready to use. You can start uploading and downloading files!")
                    .font(FontId::proportional(16.0))
                    .color(MutantColors::TEXT_PRIMARY)
            );

            ui.add_space(30.0);

            if ui.add(theme::success_button("Get Started")).clicked() {
                // Mark onboarding as completed
                state.needs_onboarding = false;
            }
        });
    }
}
