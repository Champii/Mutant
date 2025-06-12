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

    /// Submit credentials to the daemon
    pub async fn submit_credentials(&mut self) -> Result<(), String> {
        self.is_loading = true;
        self.error_message = None;

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
                    self.current_step = OnboardingStep::ColonyInitialization;
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

    /// Initialize colony
    pub async fn initialize_colony(&mut self) -> Result<(), String> {
        self.is_loading = true;
        self.error_message = None;
        self.colony_initialization_progress = 0.0;
        self.colony_initialization_status = Some("Starting colony initialization...".to_string());

        let ctx = context();
        match ctx.initialize_colony().await {
            Ok(response) => {
                if response.success {
                    self.current_step = OnboardingStep::Completed;
                    self.colony_initialization_progress = 1.0;
                    self.colony_initialization_status = Some("Colony initialization completed!".to_string());
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

    /// Generate a random colony mnemonic (placeholder)
    fn generate_colony_mnemonic(&self) -> String {
        // This is a placeholder - in a real implementation, this should generate a proper BIP39 mnemonic
        let words = [
            "abandon", "ability", "able", "about", "above", "absent", "absorb", "abstract",
            "absurd", "abuse", "access", "accident", "account", "accuse", "achieve", "acid",
        ];
        // Use timestamp and random values for word selection in WASM
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        (0..12)
            .map(|i| {
                let random_offset = (js_sys::Math::random() * words.len() as f64) as usize;
                let index = (timestamp as usize + i + random_offset) % words.len();
                words[index]
            })
            .collect::<Vec<_>>()
            .join(" ")
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
        log::debug!("is_onboarding_active: needs_onboarding={}, current_step={:?}, is_active={}",
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

/// Initialize onboarding by checking status
pub async fn init_onboarding() {
    log::info!("init_onboarding called");

    // First reset the onboarding state for clean initialization
    reset_onboarding_state();

    if let Ok(mut state) = ONBOARDING_STATE.write() {
        log::info!("Got onboarding state lock, calling check_onboarding_status");
        match state.check_onboarding_status().await {
            Ok(_) => log::info!("Onboarding status check completed successfully"),
            Err(e) => log::error!("Onboarding status check failed: {}", e),
        }
    } else {
        log::error!("Failed to get onboarding state lock");
    }
}
