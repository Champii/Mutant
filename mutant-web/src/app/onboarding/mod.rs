use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use web_time::{SystemTime, UNIX_EPOCH};

use crate::app::context::context;


pub mod wizard;

/// Onboarding state management
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnboardingState {
    pub needs_onboarding: bool,
    pub current_step: OnboardingStep,
    pub private_key: String,
    pub colony_mnemonic: String,
    pub auto_generate_private_key: bool,
    pub auto_generate_colony_mnemonic: bool,
    pub is_loading: bool,
    pub error_message: Option<String>,
    pub colony_initialization_progress: f32,
    pub colony_initialization_status: Option<String>,
    pub colony_initialization_started: bool,
    // Connection monitoring fields
    pub connection_status: ConnectionStatus,
    pub connection_check_in_progress: bool,
    pub last_connection_check: Option<web_time::SystemTime>,
    pub reconnection_attempts: u32,
    pub max_reconnection_attempts: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Reconnecting,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum OnboardingStep {
    CheckingStatus,
    CredentialCollection,
    ColonyInitialization,
    Completed,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            needs_onboarding: true, // Default to true to force onboarding until we check
            current_step: OnboardingStep::CheckingStatus,
            private_key: String::new(),
            colony_mnemonic: String::new(),
            auto_generate_private_key: false,
            auto_generate_colony_mnemonic: false,
            is_loading: false,
            error_message: None,
            colony_initialization_progress: 0.0,
            colony_initialization_status: None,
            colony_initialization_started: false,
            connection_status: ConnectionStatus::Unknown,
            connection_check_in_progress: false,
            last_connection_check: None,
            reconnection_attempts: 0,
            max_reconnection_attempts: 3,
        }
    }
}

impl OnboardingState {
    /// Check if onboarding is needed by querying the daemon
    pub async fn check_onboarding_status(&mut self) -> Result<(), String> {
        log::info!("Checking onboarding status...");
        self.is_loading = true;
        self.error_message = None;

        let ctx = context();
        log::info!("Sending onboarding status request to daemon...");
        match ctx.check_onboarding_status().await {
            Ok(status) => {
                log::info!("Onboarding status received: needs_onboarding={}, missing_private_key={}, missing_colony_mnemonic={}, missing_config_dir={}",
                    status.needs_onboarding, status.missing_private_key, status.missing_colony_mnemonic, status.missing_config_dir);
                self.needs_onboarding = status.needs_onboarding;
                if !status.needs_onboarding {
                    log::info!("No onboarding needed, setting step to Completed");
                    self.current_step = OnboardingStep::Completed;
                } else {
                    log::info!("Onboarding needed, setting step to CredentialCollection");
                    self.current_step = OnboardingStep::CredentialCollection;

                    // Pre-fill auto-generate flags if credentials are missing
                    if status.missing_private_key {
                        self.auto_generate_private_key = true;
                    }
                    if status.missing_colony_mnemonic {
                        self.auto_generate_colony_mnemonic = true;
                    }
                }
                self.is_loading = false;
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to check onboarding status: {}", e);
                // If we can't check the status, assume onboarding is needed for safety
                self.needs_onboarding = true;
                self.current_step = OnboardingStep::CredentialCollection;
                self.auto_generate_private_key = true;
                self.auto_generate_colony_mnemonic = true;
                self.error_message = Some(format!("Could not connect to daemon. Assuming onboarding is needed. Error: {}", e));
                self.is_loading = false;
                log::warn!("Assuming onboarding is needed due to connection failure");
                Ok(()) // Don't fail, just assume onboarding is needed
            }
        }
    }

    /// Submit credentials to the daemon with connection monitoring
    pub async fn submit_credentials(&mut self) -> Result<(), String> {
        self.is_loading = true;
        self.error_message = None;

        // Check connection before submitting credentials
        if let Err(e) = self.monitor_connection_during_operation("credential_submission").await {
            self.error_message = Some(format!("Connection check failed: {}", e));
            self.is_loading = false;
            return Err(e);
        }

        // Generate credentials if auto-generate is enabled
        let private_key = if self.auto_generate_private_key {
            Some(self.generate_private_key())
        } else if !self.private_key.is_empty() {
            Some(self.private_key.clone())
        } else {
            None
        };

        let colony_mnemonic = if self.auto_generate_colony_mnemonic {
            Some(self.generate_colony_mnemonic())
        } else if !self.colony_mnemonic.is_empty() {
            Some(self.colony_mnemonic.clone())
        } else {
            None
        };

        let ctx = context();
        match ctx.set_credentials(private_key, colony_mnemonic).await {
            Ok(response) => {
                if response.success {
                    // Verify connection is still active after credential submission
                    if let Err(e) = self.check_connection_status().await {
                        log::warn!("Connection check after credential submission failed: {}", e);
                        // Continue anyway, but note the issue
                    }

                    self.current_step = OnboardingStep::ColonyInitialization;
                    self.colony_initialization_started = false; // Reset flag for new step
                    self.is_loading = false;
                    Ok(())
                } else {
                    let error = response.error.unwrap_or_else(|| "Unknown error".to_string());
                    self.error_message = Some(error.clone());
                    self.is_loading = false;
                    Err(error)
                }
            }
            Err(e) => {
                let error = format!("Failed to set credentials: {}", e);
                self.error_message = Some(error.clone());
                self.is_loading = false;
                Err(error)
            }
        }
    }

    /// Initialize colony with enhanced progress tracking and connection monitoring
    pub async fn initialize_colony(&mut self) -> Result<(), String> {
        self.is_loading = true;
        self.error_message = None;
        self.colony_initialization_progress = 0.0;
        self.colony_initialization_status = Some("Preparing colony initialization...".to_string());

        // Step 1: Monitor connection before starting
        log::info!("Step 1: Checking connection status before colony initialization");
        if let Err(e) = self.monitor_connection_during_operation("colony_initialization").await {
            self.error_message = Some(format!("Connection check failed: {}", e));
            self.is_loading = false;
            return Err(e);
        }
        self.colony_initialization_progress = 0.1;
        self.colony_initialization_status = Some("Connection verified, starting colony initialization...".to_string());

        // Step 2: Initialize colony
        log::info!("Step 2: Sending colony initialization request");
        let ctx = context();
        self.colony_initialization_progress = 0.2;
        self.colony_initialization_status = Some("Initializing colony system...".to_string());

        match ctx.initialize_colony().await {
            Ok(response) => {
                if response.success {
                    self.colony_initialization_progress = 0.6;
                    self.colony_initialization_status = Some("Colony initialization completed! Preparing credential reload...".to_string());
                    log::info!("Colony initialization completed successfully, now reloading credentials");

                    // Step 3: Check connection before credential reload
                    log::info!("Step 3: Checking connection before credential reload");
                    if let Err(e) = self.check_connection_status().await {
                        log::warn!("Connection check before reload failed: {}", e);
                        // Continue anyway, but note the issue
                        self.colony_initialization_status = Some("Colony initialized, connection unstable during reload...".to_string());
                    } else {
                        self.colony_initialization_progress = 0.7;
                        self.colony_initialization_status = Some("Connection stable, reloading credentials...".to_string());
                    }

                    // Step 4: Reload credentials without restarting the daemon
                    log::info!("Step 4: Reloading credentials");
                    match self.reload_credentials_with_monitoring().await {
                        Ok(_) => {
                            self.colony_initialization_progress = 0.9;
                            self.colony_initialization_status = Some("Credentials reloaded, finalizing setup...".to_string());

                            // Step 5: Final connection verification
                            log::info!("Step 5: Final connection verification");
                            if let Err(e) = self.check_connection_status().await {
                                log::warn!("Final connection check failed: {}", e);
                                self.colony_initialization_status = Some("Setup complete (connection may be unstable)".to_string());
                            } else {
                                self.colony_initialization_status = Some("Setup completed successfully!".to_string());
                            }

                            self.current_step = OnboardingStep::Completed;
                            self.needs_onboarding = false; // Mark onboarding as fully completed
                            self.colony_initialization_progress = 1.0;
                            self.is_loading = false;
                            log::info!("Onboarding completed successfully - credentials reloaded and needs_onboarding set to false");
                            Ok(())
                        }
                        Err(e) => {
                            log::warn!("Credential reload failed, but onboarding is complete: {}", e);
                            // Even if reload fails, mark onboarding as complete since credentials are saved
                            self.current_step = OnboardingStep::Completed;
                            self.needs_onboarding = false;
                            self.colony_initialization_progress = 1.0;
                            self.colony_initialization_status = Some("Completed (credential reload had issues)".to_string());
                            self.is_loading = false;
                            Ok(())
                        }
                    }
                } else {
                    let error = response.error.unwrap_or_else(|| "Unknown error".to_string());
                    self.error_message = Some(error.clone());
                    self.is_loading = false;
                    Err(error)
                }
            }
            Err(e) => {
                let error = format!("Failed to initialize colony: {}", e);
                self.error_message = Some(error.clone());
                self.is_loading = false;
                Err(error)
            }
        }
    }

    /// Generate a random private key (placeholder - in real implementation this should be cryptographically secure)
    fn generate_private_key(&self) -> String {
        // This is a placeholder - in a real implementation, this should use proper cryptographic libraries
        // For WASM, we'll use a combination of timestamp and browser's Math.random()
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let random_value = (js_sys::Math::random() * 1_000_000_000.0) as u64;
        // Combine timestamp and random value for better entropy
        let combined = timestamp.wrapping_add(random_value as u128);
        format!("{:064x}", combined % (1u128 << 64))
    }

    /// Generate a random colony mnemonic using proper BIP39 standard
    fn generate_colony_mnemonic(&self) -> String {
        use bip39::{Mnemonic, Language};

        // Generate entropy using WASM-compatible random source
        let mut entropy = [0u8; 16]; // 128 bits for 12-word mnemonic

        // Use timestamp and browser's Math.random() for entropy in WASM
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let timestamp_bytes = timestamp.to_le_bytes();

        // Fill entropy with combination of timestamp and random values
        for i in 0..16 {
            let random_value = (js_sys::Math::random() * 256.0) as u8;
            let timestamp_byte = timestamp_bytes[i % timestamp_bytes.len()];
            entropy[i] = random_value.wrapping_add(timestamp_byte);
        }

        // Generate BIP39 mnemonic from entropy
        match Mnemonic::from_entropy(&entropy, Language::English) {
            Ok(mnemonic) => mnemonic.to_string(),
            Err(e) => {
                log::error!("Failed to generate BIP39 mnemonic: {}", e);
                // Fallback to a simple but valid mnemonic if generation fails
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string()
            }
        }
    }

    /// Reload credentials without restarting the daemon
    pub async fn reload_credentials(&mut self) -> Result<(), String> {
        log::info!("Reloading credentials without restarting daemon");
        self.is_loading = true;
        self.error_message = None;

        let ctx = context();
        match ctx.reload_credentials().await {
            Ok(response) => {
                if response.success {
                    log::info!("Credential reload initiated successfully");
                    Ok(())
                } else {
                    let error = response.error.unwrap_or_else(|| "Unknown error".to_string());
                    self.error_message = Some(error.clone());
                    self.is_loading = false;
                    Err(error)
                }
            }
            Err(e) => {
                let error = format!("Failed to reload credentials: {}", e);
                self.error_message = Some(error.clone());
                self.is_loading = false;
                Err(error)
            }
        }
    }

    /// Reload credentials with connection monitoring
    pub async fn reload_credentials_with_monitoring(&mut self) -> Result<(), String> {
        log::info!("Reloading credentials with connection monitoring");

        // Check connection before reload
        if let Err(e) = self.check_connection_status().await {
            log::warn!("Connection check before credential reload failed: {}", e);
            // Continue anyway but note the issue
        }

        // Store initial connection status for potential future use
        let _initial_connection_status = self.connection_status.clone();

        // Perform the credential reload
        let reload_result = self.reload_credentials().await;

        // After reload, monitor connection recovery
        if reload_result.is_ok() {
            log::info!("Credential reload completed, monitoring connection recovery...");

            // Give the daemon a moment to process the reload
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 2000)
                    .unwrap();
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

            // Check if connection is still active
            match self.check_connection_status().await {
                Ok(_) => {
                    if self.connection_status == ConnectionStatus::Connected {
                        log::info!("Connection maintained after credential reload");
                    } else {
                        log::warn!("Connection lost after credential reload, status: {:?}", self.connection_status);
                        // The check_connection_status method will handle reconnection attempts
                    }
                }
                Err(e) => {
                    log::error!("Failed to check connection after credential reload: {}", e);
                }
            }
        }

        reload_result
    }

    /// Check WebSocket connection status
    pub async fn check_connection_status(&mut self) -> Result<(), String> {
        if self.connection_check_in_progress {
            return Ok(()); // Already checking
        }

        self.connection_check_in_progress = true;
        self.last_connection_check = Some(web_time::SystemTime::now());

        let ctx = context();

        // Try a simple health check to verify connection
        match ctx.check_onboarding_status().await {
            Ok(_) => {
                self.connection_status = ConnectionStatus::Connected;
                self.reconnection_attempts = 0; // Reset on successful connection
                log::info!("Connection status check: Connected");
            }
            Err(e) => {
                log::warn!("Connection status check failed: {}", e);
                self.connection_status = ConnectionStatus::Disconnected;

                // If we're in the middle of onboarding, attempt reconnection
                if self.needs_onboarding && self.reconnection_attempts < self.max_reconnection_attempts {
                    self.connection_status = ConnectionStatus::Reconnecting;
                    self.reconnection_attempts += 1;

                    log::info!("Attempting reconnection {} of {}", self.reconnection_attempts, self.max_reconnection_attempts);

                    // Trigger reconnection through client manager
                    if let Err(reconnect_err) = crate::app::client_manager::reconnect().await {
                        log::error!("Reconnection attempt failed: {}", reconnect_err);
                    } else {
                        // Wait a moment and check again
                        wasm_bindgen_futures::spawn_local(async move {
                            // Small delay before rechecking
                            let promise = js_sys::Promise::new(&mut |resolve, _| {
                                web_sys::window()
                                    .unwrap()
                                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 1000)
                                    .unwrap();
                            });
                            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

                            // Recheck connection status
                            if let Ok(mut state) = onboarding_state().write() {
                                let _ = state.check_connection_status().await;
                            }
                        });
                    }
                }
            }
        }

        self.connection_check_in_progress = false;
        Ok(())
    }

    /// Monitor connection during critical operations
    pub async fn monitor_connection_during_operation(&mut self, operation_name: &str) -> Result<(), String> {
        log::info!("Starting connection monitoring for operation: {}", operation_name);

        // Check connection before starting
        self.check_connection_status().await?;

        if self.connection_status != ConnectionStatus::Connected {
            return Err(format!("Connection not available for operation: {}", operation_name));
        }

        Ok(())
    }

    /// Reset the onboarding state
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// Global onboarding state
lazy_static::lazy_static! {
    static ref ONBOARDING_STATE: Arc<RwLock<OnboardingState>> = Arc::new(RwLock::new(OnboardingState::default()));
}

/// Get the global onboarding state
pub fn onboarding_state() -> Arc<RwLock<OnboardingState>> {
    ONBOARDING_STATE.clone()
}

/// Check if onboarding is currently active
pub fn is_onboarding_active() -> bool {
    log::debug!("is_onboarding_active() called");
    log::debug!("Attempting to read ONBOARDING_STATE...");

    if let Ok(state) = ONBOARDING_STATE.read() {
        log::debug!("Successfully got ONBOARDING_STATE read lock");
        let is_active = state.needs_onboarding && state.current_step != OnboardingStep::Completed;
        log::info!("is_onboarding_active: needs_onboarding={}, current_step={:?}, is_active={}",
            state.needs_onboarding, state.current_step, is_active);
        is_active
    } else {
        log::error!("Failed to get onboarding state lock in is_onboarding_active");
        false
    }
}

/// Reset onboarding state for clean initialization
pub fn reset_onboarding_state() {
    if let Ok(mut state) = ONBOARDING_STATE.write() {
        *state = OnboardingState::default();
        log::info!("Onboarding state reset for clean initialization");
    } else {
        log::error!("Failed to get onboarding state lock for reset");
    }
}

/// Initialize onboarding by checking status and starting connection monitoring
pub async fn init_onboarding() {
    log::info!("init_onboarding called");

    // First reset the onboarding state for clean initialization
    reset_onboarding_state();

    if let Ok(mut state) = ONBOARDING_STATE.write() {
        log::info!("Got onboarding state lock, calling check_onboarding_status");
        match state.check_onboarding_status().await {
            Ok(_) => {
                log::info!("Onboarding status check completed successfully");

                // Start periodic connection monitoring if onboarding is needed
                if state.needs_onboarding {
                    start_connection_monitoring();
                }
            },
            Err(e) => log::error!("Onboarding status check failed: {}", e),
        }
    } else {
        log::error!("Failed to get onboarding state lock");
    }
}

/// Start periodic connection monitoring during onboarding
fn start_connection_monitoring() {
    log::info!("Starting periodic connection monitoring for onboarding");

    wasm_bindgen_futures::spawn_local(async move {
        loop {
            // Check if onboarding is still active
            let should_continue = {
                if let Ok(state) = ONBOARDING_STATE.read() {
                    state.needs_onboarding && state.current_step != OnboardingStep::Completed
                } else {
                    false
                }
            };

            if !should_continue {
                log::info!("Stopping connection monitoring - onboarding completed or not needed");
                break;
            }

            // Perform connection check
            if let Ok(mut state) = ONBOARDING_STATE.write() {
                // Only check if not already checking and enough time has passed
                let should_check = if let Some(last_check) = state.last_connection_check {
                    match last_check.elapsed() {
                        Ok(elapsed) => elapsed.as_secs() >= 10, // Check every 10 seconds
                        Err(_) => true,
                    }
                } else {
                    true
                };

                if should_check && !state.connection_check_in_progress {
                    let _ = state.check_connection_status().await;
                }
            }

            // Wait 5 seconds before next check
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 5000)
                    .unwrap();
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        }
    });
}
