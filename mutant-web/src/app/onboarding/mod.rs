use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use web_time::{SystemTime, UNIX_EPOCH};

use crate::app::context::context;


pub mod wizard;
pub mod colony_modal;

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
    // Flag to pause monitoring during critical operations
    pub pause_monitoring: bool,
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
            auto_generate_private_key: true,
            auto_generate_colony_mnemonic: true,
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
            pause_monitoring: false,
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
        self.pause_monitoring = true; // Pause monitoring during credential submission

        log::info!("Starting credential submission");

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
                    log::info!("Credentials set successfully, proceeding to colony initialization");
                    self.current_step = OnboardingStep::ColonyInitialization;
                    self.colony_initialization_started = false; // Reset flag for new step
                    self.is_loading = false;
                    self.pause_monitoring = false; // Resume monitoring
                    Ok(())
                } else {
                    let error = response.error.unwrap_or_else(|| "Unknown error".to_string());
                    self.error_message = Some(error.clone());
                    self.is_loading = false;
                    self.pause_monitoring = false; // Resume monitoring
                    Err(error)
                }
            }
            Err(e) => {
                let error = format!("Failed to set credentials: {}", e);
                self.error_message = Some(error.clone());
                self.is_loading = false;
                self.pause_monitoring = false; // Resume monitoring
                Err(error)
            }
        }
    }

    /// Initialize colony - simplified to just trigger daemon initialization
    pub async fn initialize_colony(&mut self) -> Result<(), String> {
        self.is_loading = true;
        self.error_message = None;
        self.colony_initialization_progress = 0.1;
        self.colony_initialization_status = Some("Starting colony initialization...".to_string());

        log::info!("Starting colony initialization");
        let ctx = context();

        match ctx.initialize_colony().await {
            Ok(response) => {
                if response.success {
                    log::info!("Colony initialization request sent successfully, now reloading credentials");

                    // Reload credentials to activate the new colony setup
                    match self.reload_credentials().await {
                        Ok(_) => {
                            log::info!("Credentials reloaded successfully");

                            // Mark onboarding as complete - the colony modal will show the real daemon progress
                            self.current_step = OnboardingStep::Completed;
                            self.needs_onboarding = false;
                            self.colony_initialization_progress = 1.0;
                            self.colony_initialization_status = Some("Onboarding completed! Colony initialization continuing...".to_string());
                            self.is_loading = false;

                            log::info!("Onboarding completed successfully - daemon will handle colony initialization");
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
                    log::info!("Credential reload initiated successfully, waiting for daemon to process...");

                    // Wait a moment for the daemon to process the reload
                    let promise = js_sys::Promise::new(&mut |resolve, _| {
                        web_sys::window()
                            .unwrap()
                            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 3000)
                            .unwrap();
                    });
                    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

                    // Force a WebSocket reconnection to ensure we receive colony progress events
                    log::info!("Forcing WebSocket reconnection after credential reload...");
                    if let Err(e) = crate::app::client_manager::reconnect().await {
                        log::warn!("Failed to reconnect WebSocket after credential reload: {}", e);
                        // Don't fail the operation, just log the warning
                    } else {
                        log::info!("WebSocket reconnection completed successfully");
                    }

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
                        log::info!("Reconnection attempt initiated, will be verified in next monitoring cycle");
                        // Don't recursively call check_connection_status - let the monitoring loop handle it
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

/// Global colony initialization state (separate from onboarding)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColonyInitState {
    pub is_initializing: bool,
    pub progress: f32,
    pub status: Option<String>,
    pub error: Option<String>,
}

impl Default for ColonyInitState {
    fn default() -> Self {
        Self {
            is_initializing: false,
            progress: 0.0,
            status: None,
            error: None,
        }
    }
}

// Global onboarding state
lazy_static::lazy_static! {
    static ref ONBOARDING_STATE: Arc<RwLock<OnboardingState>> = Arc::new(RwLock::new(OnboardingState::default()));
    static ref COLONY_INIT_STATE: Arc<RwLock<ColonyInitState>> = Arc::new(RwLock::new(ColonyInitState::default()));
}

/// Get the global onboarding state
pub fn onboarding_state() -> Arc<RwLock<OnboardingState>> {
    ONBOARDING_STATE.clone()
}

/// Get the global colony initialization state
pub fn colony_init_state() -> Arc<RwLock<ColonyInitState>> {
    COLONY_INIT_STATE.clone()
}

/// Check if colony initialization is currently active (anywhere, not just onboarding)
pub fn is_colony_initializing() -> bool {
    // Only check the colony initialization state to avoid recursion
    // The colony progress events are handled separately in colony_window
    if let Ok(state) = COLONY_INIT_STATE.try_read() {
        state.is_initializing
    } else {
        false
    }
}

/// Start colony initialization tracking
pub fn start_colony_initialization(status: Option<String>) {
    if let Ok(mut state) = COLONY_INIT_STATE.try_write() {
        state.is_initializing = true;
        state.progress = 0.0;
        state.status = status;
        state.error = None;
        log::info!("Colony initialization started globally");
    }
}

/// Update colony initialization progress
pub fn update_colony_initialization_progress(progress: f32, status: Option<String>) {
    if let Ok(mut state) = COLONY_INIT_STATE.try_write() {
        state.progress = progress;
        if let Some(status) = status {
            state.status = Some(status);
        }
        log::info!("Colony initialization progress updated: {:.1}%", progress * 100.0);
    }
}

/// Complete colony initialization
pub fn complete_colony_initialization() {
    if let Ok(mut state) = COLONY_INIT_STATE.try_write() {
        state.is_initializing = false;
        state.progress = 1.0;
        state.status = Some("Colony initialization completed!".to_string());
        state.error = None;
        log::info!("Colony initialization completed globally");
    }
}

/// Set colony initialization error
pub fn set_colony_initialization_error(error: String) {
    if let Ok(mut state) = COLONY_INIT_STATE.try_write() {
        state.is_initializing = false;
        state.error = Some(error.clone());
        log::error!("Colony initialization error: {}", error);
    }
}

/// Reset colony initialization state (for testing)
pub fn reset_colony_initialization_state() {
    if let Ok(mut state) = COLONY_INIT_STATE.try_write() {
        *state = ColonyInitState::default();
        log::info!("Colony initialization state reset");
    }
}

/// Handle colony progress events from the daemon
pub fn handle_daemon_colony_progress(event: mutant_protocol::ColonyEvent, operation_id: Option<String>) {
    use mutant_protocol::ColonyEvent;

    log::debug!("Handling daemon colony progress event: {:?} (operation_id: {:?})", event, operation_id);

    // Map colony events to progress and status
    let (progress, status, is_initializing, is_error) = match &event {
        // Initialization events
        ColonyEvent::InitializationStarted => {
            // When daemon starts colony initialization, ensure our state reflects this
            start_colony_initialization(Some("Daemon colony initialization started...".to_string()));
            (0.1, Some("Daemon colony initialization started...".to_string()), true, false)
        },
        ColonyEvent::DataStoreInitStarted => {
            (0.2, Some("Initializing data store...".to_string()), true, false)
        },
        ColonyEvent::DataStoreInitCompleted => {
            (0.3, Some("Data store initialized".to_string()), true, false)
        },
        ColonyEvent::KeyStoreInitStarted => {
            (0.4, Some("Initializing key store...".to_string()), true, false)
        },
        ColonyEvent::KeyStoreInitCompleted => {
            (0.5, Some("Key store initialized".to_string()), true, false)
        },
        ColonyEvent::KeyDerivationStarted => {
            (0.6, Some("Deriving colony keys...".to_string()), true, false)
        },
        ColonyEvent::KeyDerivationCompleted { pod_address } => {
            (0.7, Some(format!("Keys derived for pod: {}", pod_address)), true, false)
        },
        ColonyEvent::GraphInitStarted => {
            (0.8, Some("Initializing graph database...".to_string()), true, false)
        },
        ColonyEvent::GraphInitCompleted => {
            (0.85, Some("Graph database initialized".to_string()), true, false)
        },
        ColonyEvent::UserPodCheckStarted => {
            (0.9, Some("Checking user pod...".to_string()), true, false)
        },
        ColonyEvent::UserPodCreationStarted => {
            (0.95, Some("Creating user pod...".to_string()), true, false)
        },
        ColonyEvent::UserPodCreationCompleted { pod_address } => {
            (0.98, Some(format!("User pod created: {}", pod_address)), true, false)
        },
        ColonyEvent::UserPodCheckCompleted => {
            (0.99, Some("User pod check completed".to_string()), true, false)
        },
        ColonyEvent::InitializationCompleted => {
            // Only complete if this is from the daemon's actual work, not onboarding
            if operation_id.as_ref().map_or(true, |id| id != "onboarding_init") {
                complete_colony_initialization();
                (1.0, Some("Colony initialization completed!".to_string()), false, false)
            } else {
                // This is from onboarding completion, don't mark as fully complete
                log::debug!("Ignoring onboarding completion event, waiting for daemon completion");
                return;
            }
        },

        // Error events
        ColonyEvent::OperationFailed { operation: _, error: _ } => {
            (0.0, None, false, true)
        },

        // Progress events
        ColonyEvent::Progress { operation, message, current, total } => {
            let progress = if let (Some(current), Some(total)) = (current, total) {
                if *total > 0 {
                    (*current as f32) / (*total as f32)
                } else {
                    0.0
                }
            } else {
                0.5 // Default progress for operations without specific progress
            };
            let status = format!("{}: {}", operation, message);
            (progress, Some(status), true, false)
        },

        // Other events that indicate ongoing operations
        ColonyEvent::PodRefreshStarted { .. } |
        ColonyEvent::ContactSyncStarted { .. } |
        ColonyEvent::IndexingStarted { .. } |
        ColonyEvent::SearchStarted { .. } => {
            // These are not part of initial colony initialization, so don't affect the modal
            return;
        },

        // Completion events for non-initialization operations
        ColonyEvent::PodRefreshCompleted { .. } |
        ColonyEvent::ContactSyncCompleted { .. } |
        ColonyEvent::IndexingCompleted { .. } |
        ColonyEvent::SearchCompleted { .. } |
        ColonyEvent::OperationCompleted { .. } => {
            // These are not part of initial colony initialization, so don't affect the modal
            return;
        },

        // Default case for any other events
        _ => {
            log::debug!("Unhandled colony event for progress tracking: {:?}", event);
            return;
        }
    };

    // Update the global colony initialization state
    if let Ok(mut state) = COLONY_INIT_STATE.try_write() {
        if is_error {
            if let ColonyEvent::OperationFailed { operation, error } = &event {
                state.is_initializing = false;
                state.error = Some(format!("{}: {}", operation, error));
                log::error!("Colony initialization failed: {}: {}", operation, error);
            }
        } else {
            state.is_initializing = is_initializing;
            state.progress = progress;
            if let Some(status) = status {
                state.status = Some(status);
            }
            state.error = None;

            log::info!("Colony initialization progress: {:.1}% - {}",
                progress * 100.0,
                state.status.as_deref().unwrap_or("Unknown status"));
        }
    }
}

/// Check if onboarding is currently active
pub fn is_onboarding_active() -> bool {
    log::debug!("is_onboarding_active() called");
    log::debug!("Attempting to read ONBOARDING_STATE...");

    // Use try_read to avoid blocking the UI thread - this is critical!
    if let Ok(state) = ONBOARDING_STATE.try_read() {
        log::debug!("Successfully got ONBOARDING_STATE read lock");
        let is_active = state.needs_onboarding && state.current_step != OnboardingStep::Completed;
        log::info!("is_onboarding_active: needs_onboarding={}, current_step={:?}, is_active={}",
            state.needs_onboarding, state.current_step, is_active);
        is_active
    } else {
        log::debug!("Could not get onboarding state lock in is_onboarding_active (busy), assuming not active");
        // If we can't get the lock, assume onboarding is not active to avoid blocking the UI
        // This is safer than blocking the entire UI thread
        false
    }
}

/// Reset onboarding state for clean initialization
pub fn reset_onboarding_state() {
    if let Ok(mut state) = ONBOARDING_STATE.try_write() {
        *state = OnboardingState::default();
        log::info!("Onboarding state reset for clean initialization");
    } else {
        log::error!("Failed to get onboarding state lock for reset (busy)");
    }
}

/// Initialize onboarding by checking status and starting connection monitoring
pub async fn init_onboarding() {
    log::info!("init_onboarding called");

    // First reset the onboarding state for clean initialization
    reset_onboarding_state();

    if let Ok(mut state) = ONBOARDING_STATE.try_write() {
        log::info!("Got onboarding state lock, calling check_onboarding_status");
        match state.check_onboarding_status().await {
            Ok(_) => {
                log::info!("Onboarding status check completed successfully");

                // Temporarily disable connection monitoring to avoid conflicts
                // TODO: Re-enable once the deadlock issues are fully resolved
                // if state.needs_onboarding {
                //     start_connection_monitoring();
                // }
                log::info!("Connection monitoring disabled to prevent deadlocks during onboarding");
            },
            Err(e) => log::error!("Onboarding status check failed: {}", e),
        }
    } else {
        log::error!("Failed to get onboarding state lock (busy)");
    }
}

/// Start periodic connection monitoring during onboarding
fn start_connection_monitoring() {
    log::info!("Starting periodic connection monitoring for onboarding");

    wasm_bindgen_futures::spawn_local(async move {
        loop {
            // Check if onboarding is still active and monitoring is not paused
            let (should_continue, is_paused) = {
                if let Ok(state) = ONBOARDING_STATE.read() {
                    let should_continue = state.needs_onboarding && state.current_step != OnboardingStep::Completed;
                    (should_continue, state.pause_monitoring)
                } else {
                    (false, false)
                }
            };

            if !should_continue {
                log::info!("Stopping connection monitoring - onboarding completed or not needed");
                break;
            }

            if is_paused {
                log::debug!("Connection monitoring paused, skipping this cycle");
                // Wait and continue to next iteration
                let promise = js_sys::Promise::new(&mut |resolve, _| {
                    web_sys::window()
                        .unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 1000)
                        .unwrap();
                });
                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                continue;
            }

            // Perform connection check with non-blocking approach
            let should_check = {
                if let Ok(state) = ONBOARDING_STATE.read() {
                    let time_check = if let Some(last_check) = state.last_connection_check {
                        match last_check.elapsed() {
                            Ok(elapsed) => elapsed.as_secs() >= 10, // Check every 10 seconds
                            Err(_) => true,
                        }
                    } else {
                        true
                    };
                    time_check && !state.connection_check_in_progress
                } else {
                    false
                }
            };

            if should_check {
                // Try to get write lock with timeout to avoid deadlocks
                if let Ok(mut state) = ONBOARDING_STATE.try_write() {
                    let _ = state.check_connection_status().await;
                } else {
                    log::debug!("Connection monitoring: Could not acquire write lock, skipping this check");
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
