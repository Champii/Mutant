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
        log::debug!("OnboardingWizard::show() called");

        log::debug!("Getting onboarding state...");
        let state_lock = onboarding_state();
        log::debug!("Got onboarding state, attempting to read...");

        // Use try_read to avoid blocking and check if we should show
        let should_show = if let Ok(state) = state_lock.try_read() {
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
                // Draw content based on current step - use try_write to avoid deadlocks
                if let Ok(mut state) = state_lock.try_write() {
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
        // Work directly with state to ensure checkbox changes persist
        let mut local_private_key = state.private_key.clone();
        let mut local_colony_mnemonic = state.colony_mnemonic.clone();

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
                        ui.text_edit_singleline(&mut local_private_key);
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
                        ui.text_edit_multiline(&mut local_colony_mnemonic);
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
                let should_submit = ui.add(theme::primary_button("Continue")).clicked() && !state.is_loading;

                if should_submit {
                    // Use the state values that were modified by the UI
                    let private_key = local_private_key.clone();
                    let colony_mnemonic = local_colony_mnemonic.clone();
                    let auto_generate_private_key = state.auto_generate_private_key;
                    let auto_generate_colony_mnemonic = state.auto_generate_colony_mnemonic;

                    // Don't modify state here - do it in the async task to avoid deadlock
                    wasm_bindgen_futures::spawn_local(async move {
                        // Use a retry mechanism to avoid deadlocks
                        let mut attempts = 0;
                        let max_attempts = 5;

                        while attempts < max_attempts {
                            if let Ok(mut state) = onboarding_state().try_write() {
                                // Set loading state here to avoid UI deadlock
                                state.is_loading = true;
                                state.error_message = None;

                                // Set the captured values in state
                                state.private_key = private_key.clone();
                                state.colony_mnemonic = colony_mnemonic.clone();
                                state.auto_generate_private_key = auto_generate_private_key;
                                state.auto_generate_colony_mnemonic = auto_generate_colony_mnemonic;

                                match state.submit_credentials().await {
                                    Ok(_) => {
                                        log::info!("Credential submission completed successfully");
                                        break;
                                    }
                                    Err(e) => {
                                        log::error!("Credential submission failed: {}", e);
                                        state.is_loading = false;
                                        break;
                                    }
                                }
                            } else {
                                attempts += 1;
                                log::debug!("Could not acquire write lock for credential submission, attempt {}/{}", attempts, max_attempts);

                                // Small delay before retry
                                let promise = js_sys::Promise::new(&mut |resolve, _| {
                                    web_sys::window()
                                        .unwrap()
                                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 100)
                                        .unwrap();
                                });
                                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                            }
                        }

                        if attempts >= max_attempts {
                            log::error!("Failed to acquire write lock for credential submission after {} attempts", max_attempts);
                            // Reset loading state if we couldn't submit
                            if let Ok(mut state) = onboarding_state().try_write() {
                                state.is_loading = false;
                                state.error_message = Some("System busy, please try again".to_string());
                            }
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
        // Check if colony initialization is already in progress from daemon events
        let daemon_colony_in_progress = crate::app::colony_window::is_colony_operation_in_progress();

        // Check if we need to start initialization before drawing UI
        // Only start if daemon isn't already handling it and we haven't started yet
        let should_start_initialization = !state.is_loading
            && !state.colony_initialization_started
            && state.colony_initialization_progress == 0.0
            && !daemon_colony_in_progress;

        // Store the flag to defer the async task spawning until after UI rendering
        let mut spawn_initialization = false;

        if should_start_initialization {
            // Set flag immediately to prevent multiple spawns
            state.colony_initialization_started = true;
            spawn_initialization = true;
            log::info!("Marking colony initialization to start after UI rendering (daemon not active)");
        } else if daemon_colony_in_progress && !state.colony_initialization_started {
            // If daemon is handling colony initialization, mark our state as started to show progress
            state.colony_initialization_started = true;
            log::info!("Daemon colony initialization detected, syncing onboarding state");
        }

        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Initializing Colony")
                    .font(FontId::proportional(20.0))
                    .color(MutantColors::TEXT_PRIMARY)
            );

            ui.add_space(20.0);

            // Connection status indicator
            self.draw_connection_status(ui, state);

            ui.add_space(15.0);

            // Progress bar
            let progress = egui::ProgressBar::new(state.colony_initialization_progress)
                .fill(MutantColors::ACCENT_ORANGE)
                .animate(state.is_loading);
            ui.add(progress);

            ui.add_space(10.0);

            // Progress percentage
            ui.label(
                egui::RichText::new(format!("{:.0}%", state.colony_initialization_progress * 100.0))
                    .font(FontId::proportional(12.0))
                    .color(MutantColors::TEXT_SECONDARY)
            );

            ui.add_space(10.0);

            if let Some(status) = &state.colony_initialization_status {
                ui.label(
                    egui::RichText::new(status)
                        .font(FontId::proportional(14.0))
                        .color(MutantColors::TEXT_SECONDARY)
                );
            }

            // Reconnection status
            if state.reconnection_attempts > 0 {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!("Reconnection attempts: {}/{}",
                        state.reconnection_attempts, state.max_reconnection_attempts))
                        .font(FontId::proportional(12.0))
                        .color(MutantColors::WARNING)
                );
            }

            // Error message
            if let Some(error) = &state.error_message {
                ui.add_space(10.0);
                ui.colored_label(MutantColors::ERROR, error);

                // Retry button for connection errors
                if error.contains("connection") || error.contains("Connection") {
                    ui.add_space(10.0);
                    if ui.add(theme::secondary_button("Retry Connection")).clicked() {
                        // Reset the initialization flag to allow retry
                        if let Ok(mut state) = onboarding_state().try_write() {
                            state.colony_initialization_started = false;
                            state.error_message = None;
                            state.is_loading = false;
                            log::info!("Reset colony initialization for retry");
                        }
                    }
                }
            }
        });

        // Spawn the async task AFTER UI rendering is complete to avoid race conditions
        if spawn_initialization {
            log::info!("Starting colony initialization auto-start");

            // Only start global colony initialization tracking if daemon isn't already handling it
            // This prevents duplicate initialization calls that cause database locks
            if !crate::app::colony_window::is_colony_operation_in_progress() {
                super::start_colony_initialization(Some("Starting colony initialization...".to_string()));
            } else {
                log::info!("Daemon colony initialization already in progress, skipping duplicate start call");
            }

            wasm_bindgen_futures::spawn_local(async move {
                log::info!("Colony initialization async task started");

                // Try to get the lock once with a timeout
                let initialization_result = match onboarding_state().try_write() {
                    Ok(mut state) => {
                        log::info!("Got write lock for colony initialization");
                        state.initialize_colony().await
                    }
                    Err(_) => {
                        log::error!("Could not acquire write lock for colony initialization");
                        Err("System busy, please try again".to_string())
                    }
                };

                // Handle the result
                match initialization_result {
                    Ok(_) => {
                        log::info!("Colony initialization completed successfully");
                        // Only call complete if we're not already completed by daemon events
                        if !crate::app::colony_window::is_colony_operation_in_progress() {
                            super::complete_colony_initialization();
                        }
                    }
                    Err(e) => {
                        log::error!("Colony initialization failed: {}", e);
                        super::set_colony_initialization_error(e.clone());
                        // Reset the flag and set error message
                        if let Ok(mut state) = onboarding_state().try_write() {
                            state.colony_initialization_started = false;
                            state.error_message = Some(e);
                            state.is_loading = false;
                        }
                    }
                }
            });
        }
    }

    /// Draw connection status indicator
    fn draw_connection_status(&self, ui: &mut egui::Ui, state: &OnboardingState) {
        ui.horizontal(|ui| {
            // Connection status icon and text
            let (icon, color, text) = match state.connection_status {
                super::ConnectionStatus::Connected => ("🟢", MutantColors::SUCCESS, "Connected"),
                super::ConnectionStatus::Disconnected => ("🔴", MutantColors::ERROR, "Disconnected"),
                super::ConnectionStatus::Reconnecting => ("🟡", MutantColors::WARNING, "Reconnecting..."),
                super::ConnectionStatus::Unknown => ("⚪", MutantColors::TEXT_SECONDARY, "Checking..."),
            };

            ui.label(
                egui::RichText::new(format!("{} {}", icon, text))
                    .font(FontId::proportional(12.0))
                    .color(color)
            );

            // Show last check time if available
            if let Some(last_check) = state.last_connection_check {
                if let Ok(elapsed) = last_check.elapsed() {
                    let seconds_ago = elapsed.as_secs();
                    if seconds_ago < 60 {
                        ui.label(
                            egui::RichText::new(format!("({}s ago)", seconds_ago))
                                .font(FontId::proportional(10.0))
                                .color(MutantColors::TEXT_SECONDARY)
                        );
                    }
                }
            }
        });
    }

    fn draw_completion(&self, ui: &mut egui::Ui, _state: &OnboardingState) {
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
                // Mark onboarding as completed directly - no need for async task here
                if let Ok(mut state) = onboarding_state().try_write() {
                    state.needs_onboarding = false;
                    log::info!("Onboarding marked as completed by user");
                }
            }
        });
    }
}
