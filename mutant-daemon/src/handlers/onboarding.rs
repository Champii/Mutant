use std::sync::Arc;
use std::env;
use std::fs;
use std::path::Path;
use xdg::BaseDirectories;

use crate::error::Error as DaemonError;
use mutant_lib::MutAnt;
use mutant_protocol::{
    OnboardingStatusRequest, OnboardingStatusResponse,
    SetCredentialsRequest, SetCredentialsResponse,
    InitializeColonyRequest, InitializeColonyResponse,
    Response, ColonyEvent, ColonyProgressResponse,
};

use super::common::UpdateSender;
use super::colony::init_colony_manager;

/// Check if onboarding is needed by examining missing credentials and config
pub(crate) async fn handle_onboarding_status(
    _req: OnboardingStatusRequest,
    update_tx: UpdateSender,
    _mutant: Arc<MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling OnboardingStatus request");

    // Check for missing private key
    let missing_private_key = env::var("PRIVATE_KEY").is_err() || env::var("PRIVATE_KEY").unwrap_or_default().is_empty();
    
    // Check for missing colony mnemonic
    let missing_colony_mnemonic = env::var("COLONY_MNEMONIC").is_err() || env::var("COLONY_MNEMONIC").unwrap_or_default().is_empty();
    
    // Check for missing config directory
    let missing_config_dir = match BaseDirectories::with_prefix("mutant") {
        Ok(xdg_dirs) => {
            let config_dir = xdg_dirs.get_config_home();
            !config_dir.exists()
        }
        Err(_) => true,
    };

    let needs_onboarding = missing_private_key || missing_colony_mnemonic || missing_config_dir;

    let response = OnboardingStatusResponse {
        needs_onboarding,
        missing_private_key,
        missing_colony_mnemonic,
        missing_config_dir,
    };

    log::info!("Onboarding status: needs_onboarding={}, missing_private_key={}, missing_colony_mnemonic={}, missing_config_dir={}", 
        needs_onboarding, missing_private_key, missing_colony_mnemonic, missing_config_dir);

    let _ = update_tx.send(Response::OnboardingStatus(response));
    Ok(())
}

/// Set credentials by creating/updating .env file
pub(crate) async fn handle_set_credentials(
    req: SetCredentialsRequest,
    update_tx: UpdateSender,
    _mutant: Arc<MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling SetCredentials request");

    // Validate credentials
    if let Some(ref private_key) = req.private_key {
        if private_key.is_empty() {
            let response = SetCredentialsResponse {
                success: false,
                error: Some("Private key cannot be empty".to_string()),
            };
            let _ = update_tx.send(Response::SetCredentials(response));
            return Ok(());
        }

        // Basic validation - should be hex string
        if !private_key.chars().all(|c| c.is_ascii_hexdigit()) {
            let response = SetCredentialsResponse {
                success: false,
                error: Some("Private key must be a valid hex string".to_string()),
            };
            let _ = update_tx.send(Response::SetCredentials(response));
            return Ok(());
        }

        // Check length (64 characters for 32 bytes)
        if private_key.len() != 64 {
            let response = SetCredentialsResponse {
                success: false,
                error: Some("Private key must be 64 characters long".to_string()),
            };
            let _ = update_tx.send(Response::SetCredentials(response));
            return Ok(());
        }
    }

    // Create .env file in the current working directory
    let env_path = Path::new(".env");
    
    // Read existing .env file if it exists
    let mut env_content = if env_path.exists() {
        fs::read_to_string(env_path)
            .map_err(|e| DaemonError::IoError(format!("Failed to read .env file: {}", e)))?
    } else {
        String::new()
    };

    // Update or add private key
    if let Some(private_key) = req.private_key {
        env_content = update_env_var(&env_content, "PRIVATE_KEY", &private_key);
    }

    // Update or add colony mnemonic
    if let Some(colony_mnemonic) = req.colony_mnemonic {
        env_content = update_env_var(&env_content, "COLONY_MNEMONIC", &colony_mnemonic);
    }

    // Write the updated .env file
    fs::write(env_path, env_content)
        .map_err(|e| DaemonError::IoError(format!("Failed to write .env file: {}", e)))?;

    // Set file permissions to be readable only by owner
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(env_path)
            .map_err(|e| DaemonError::IoError(format!("Failed to get .env file metadata: {}", e)))?
            .permissions();
        perms.set_mode(0o600); // rw-------
        fs::set_permissions(env_path, perms)
            .map_err(|e| DaemonError::IoError(format!("Failed to set .env file permissions: {}", e)))?;
    }

    // Create config directory if it doesn't exist
    if let Ok(xdg_dirs) = BaseDirectories::with_prefix("mutant") {
        let config_dir = xdg_dirs.get_config_home();
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)
                .map_err(|e| DaemonError::IoError(format!("Failed to create config directory: {}", e)))?;
        }
    }

    log::info!("Successfully updated credentials in .env file");

    let response = SetCredentialsResponse {
        success: true,
        error: None,
    };

    let _ = update_tx.send(Response::SetCredentials(response));
    Ok(())
}

/// Initialize colony after credentials are set
pub(crate) async fn handle_initialize_colony(
    _req: InitializeColonyRequest,
    update_tx: UpdateSender,
    mutant: Arc<MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling InitializeColony request");

    // Send progress update
    let progress_response = ColonyProgressResponse {
        event: ColonyEvent::InitializationStarted,
        operation_id: Some("onboarding_init".to_string()),
    };
    let _ = update_tx.send(Response::ColonyProgress(progress_response));

    // Get wallet from MutAnt
    let wallet = match mutant.get_wallet().await {
        Ok(wallet) => wallet,
        Err(e) => {
            log::error!("Failed to get wallet for colony initialization: {}", e);
            let response = InitializeColonyResponse {
                success: false,
                pod_address: None,
                error: Some(format!("Failed to get wallet: {}", e)),
            };
            let _ = update_tx.send(Response::InitializeColony(response));
            return Ok(());
        }
    };

    // Get private key from environment (should be set by set_credentials)
    let private_key = env::var("PRIVATE_KEY").ok();

    // Initialize colony manager
    match init_colony_manager(wallet, mutant_lib::config::NetworkChoice::Mainnet, private_key).await {
        Ok(_) => {
            log::info!("Colony manager initialized successfully during onboarding");
            
            // Send completion progress
            let progress_response = ColonyProgressResponse {
                event: ColonyEvent::InitializationCompleted,
                operation_id: Some("onboarding_init".to_string()),
            };
            let _ = update_tx.send(Response::ColonyProgress(progress_response));

            let response = InitializeColonyResponse {
                success: true,
                pod_address: None, // TODO: Get actual pod address from colony manager
                error: None,
            };
            let _ = update_tx.send(Response::InitializeColony(response));
        }
        Err(e) => {
            log::error!("Failed to initialize colony manager during onboarding: {}", e);
            
            // Send error progress
            let progress_response = ColonyProgressResponse {
                event: ColonyEvent::OperationFailed {
                    operation: "Colony Initialization".to_string(),
                    error: format!("Colony initialization failed: {}", e)
                },
                operation_id: Some("onboarding_init".to_string()),
            };
            let _ = update_tx.send(Response::ColonyProgress(progress_response));

            let response = InitializeColonyResponse {
                success: false,
                pod_address: None,
                error: Some(format!("Colony initialization failed: {}", e)),
            };
            let _ = update_tx.send(Response::InitializeColony(response));
        }
    }

    Ok(())
}

/// Helper function to update or add an environment variable in .env content
fn update_env_var(content: &str, var_name: &str, var_value: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let var_line = format!("{}=\"{}\"", var_name, var_value);
    
    // Find existing variable and replace it
    let mut found = false;
    for line in &mut lines {
        if line.starts_with(&format!("{}=", var_name)) {
            *line = var_line.clone();
            found = true;
            break;
        }
    }
    
    // If not found, add it
    if !found {
        if !lines.is_empty() && !lines.last().unwrap().is_empty() {
            lines.push(String::new()); // Add empty line before new variable
        }
        lines.push(var_line);
    }
    
    lines.join("\n")
}
