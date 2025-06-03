use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use xdg::BaseDirectories;

use mutant_lib::{config::NetworkChoice, MutAnt};
use tokio::sync::{RwLock, OnceCell};
use warp::Filter;

use crate::error::Error;
use crate::handlers;
use crate::handlers::TaskMap;
use crate::wallet;

use std::path::PathBuf;
use tokio::signal;

// Thread-safe singleton to track public-only mode
pub static PUBLIC_ONLY_MODE: OnceCell<bool> = OnceCell::const_new();

/// Helper function to initialize MutAnt based on network choice and private key
/// Returns (MutAnt instance, is_public_only, actual_private_key_used)
async fn init_mutant(network_choice: NetworkChoice, private_key: Option<String>) -> Result<(MutAnt, bool, Option<String>), Error> {
    let mut is_public_only = private_key.is_none();
    let mut actual_private_key = private_key.clone();

    let mutant = match (network_choice, private_key) {
        // Full access with private key
        (NetworkChoice::Devnet, _) => {
            log::info!("Running in testnet mode - creating new random wallet");
            is_public_only = false;

            // Use the new testnet mode that creates a random wallet and transfers funds
            match MutAnt::init_testnet().await {
                Ok((mutant, public_address, new_private_key)) => {
                    log::info!("🔑 NEW TESTNET WALLET CREATED");
                    log::info!("📍 Public Address: {}", public_address);
                    log::info!("💰 Transferred funds to new wallet");
                    log::info!("🚀 Using new wallet for all operations");
                    actual_private_key = Some(new_private_key);
                    Ok(mutant)
                }
                Err(e) => {
                    log::warn!("Failed to create testnet wallet: {}. Falling back to master key.", e);
                    MutAnt::init_local().await
                }
            }
        }
        (NetworkChoice::Alphanet, Some(key)) => {
            log::info!("Running in alphanet mode");
            MutAnt::init_alphanet(&key).await
        }
        (NetworkChoice::Mainnet, Some(key)) => {
            log::info!("Running in mainnet mode");
            MutAnt::init(&key).await
        }

        // Public-only mode (no private key)
        (NetworkChoice::Alphanet, None) => {
            log::info!("Running in alphanet public-only mode");
            MutAnt::init_public_alphanet().await
        }
        (NetworkChoice::Mainnet, None) => {
            log::info!("Running in mainnet public-only mode");
            MutAnt::init_public().await
        }
    }.map_err(Error::MutAnt)?;

    Ok((mutant, is_public_only, actual_private_key))
}

#[derive(serde::Deserialize, serde::Serialize)]
struct NetworkConfig {
    public_key: String,
}

#[derive(serde::Deserialize, serde::Serialize, Default)]
pub struct Config {
    mainnet: Option<NetworkConfig>,
    devnet: Option<NetworkConfig>,
    alphanet: Option<NetworkConfig>,
}

impl Config {
    pub fn load() -> Result<Self, Error> {
        let xdg_dirs = BaseDirectories::with_prefix("mutant")?;
        let config_path = xdg_dirs.get_config_file("config.json");

        if let Ok(content) = std::fs::read_to_string(&config_path) {
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> Result<(), Error> {
        let xdg_dirs = BaseDirectories::with_prefix("mutant")?;
        let config_path = xdg_dirs.get_config_file("config.json");

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }

    fn get_key_file_path(public_key_hex: &str) -> Result<PathBuf, Error> {
        let base_data_dir = BaseDirectories::new()?.get_data_home();
        Ok(base_data_dir
            .join("autonomi")
            .join("client")
            .join("wallets")
            .join(format!("{}", public_key_hex)))
    }

    pub fn get_private_key(
        &self,
        network_choice: NetworkChoice,
    ) -> Result<Option<(String, String)>, Error> {
        let public_key_hex = match network_choice {
            NetworkChoice::Mainnet => self.mainnet.as_ref().map(|c| c.public_key.as_str()),
            NetworkChoice::Devnet => self.devnet.as_ref().map(|c| c.public_key.as_str()),
            NetworkChoice::Alphanet => self.alphanet.as_ref().map(|c| c.public_key.as_str()),
        };

        if let Some(pk_hex) = public_key_hex {
            let key_file_path = Self::get_key_file_path(pk_hex)?;
            if key_file_path.exists() {
                let private_key_content = std::fs::read_to_string(key_file_path)?;
                Ok(Some((
                    private_key_content.trim().to_string(),
                    pk_hex.to_string(),
                )))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    pub fn set_public_key(&mut self, network_choice: NetworkChoice, public_key: String) {
        let config = NetworkConfig { public_key };
        match network_choice {
            NetworkChoice::Mainnet => self.mainnet = Some(config),
            NetworkChoice::Devnet => self.devnet = Some(config),
            NetworkChoice::Alphanet => self.alphanet = Some(config),
        }
    }
}

pub struct AppOptions {
    pub local: bool,
    pub alphanet: bool,
    pub ignore_ctrl_c: bool,
    pub bind_address: String,
}

pub async fn run(options: AppOptions) -> Result<(), Error> {
    // check if the daemon is running by checking the /tmp/mutant-daemon.lock file
    if std::fs::File::open("/tmp/mutant-daemon.lock").is_ok() {
        println!("Mutant Daemon is already running");
        return Ok(());
    }

    // Create the lock file. It will be held until the process exits.
    let _lock_file =
        lockfile::Lockfile::create("/tmp/mutant-daemon.lock").map_err(Error::Lockfile)?;

    // put the pid of the daemon in the lock file
    // Note: The lockfile crate typically manages this implicitly through the file lock.
    // Writing the PID might be redundant or could be handled differently.
    std::fs::write("/tmp/mutant-daemon.lock", std::process::id().to_string())?;

    log::info!("Starting Mutant Daemon...");

    let network_choice = if options.local {
        NetworkChoice::Devnet
    } else if options.alphanet {
        NetworkChoice::Alphanet
    } else {
        NetworkChoice::Mainnet
    };

    let mut config = Config::load()?;

    // Try to get a private key from config
    let private_key = match config.get_private_key(network_choice)? {
        Some((key_from_file, pk_hex)) => {
            log::info!(
                "Loaded private key from file for network {:?}: {}",
                network_choice,
                pk_hex
            );
            Some(key_from_file)
        }
        None => {
            // Try to scan for wallets
            match wallet::scan_and_select_wallet().await {
                Ok((selected_private_key, pk_hex)) => {
                    // Save the selected wallet for future use
                    config.set_public_key(network_choice, pk_hex.clone());
                    config.save()?;
                    log::info!(
                        "Saved newly selected public key {} to config for network {:?}",
                        pk_hex,
                        network_choice
                    );
                    Some(selected_private_key)
                }
                Err(e) => {
                    // No wallet found, initialize in public-only mode
                    log::warn!("No wallet found: {}. Initializing in public-only mode.", e);
                    log::info!("Only public downloads (mutant get -p) will be available.");
                    None
                }
            }
        }
    };

    // Initialize MutAnt with the appropriate mode
    let (mutant, is_public_only, actual_private_key) = init_mutant(network_choice, private_key.clone()).await?;
    let mutant = Arc::new(mutant);

    // Set the public-only mode flag
    PUBLIC_ONLY_MODE.set(is_public_only).expect("PUBLIC_ONLY_MODE should only be set once");

    log::info!("MutAnt initialized successfully{}",
        if is_public_only { " in public-only mode" } else { "" });

    // Initialize Colony Manager for search and indexing
    match mutant.get_wallet().await {
        Ok(wallet) => {
            if let Err(e) = crate::handlers::colony::init_colony_manager(wallet, network_choice, actual_private_key.clone()).await {
                log::warn!("Failed to initialize colony manager: {}. Search functionality will be unavailable.", e);
            } else {
                log::info!("Colony manager initialized successfully.");
            }
        }
        Err(e) => {
            log::warn!("Failed to get wallet for colony manager: {}. Search functionality will be unavailable.", e);
        }
    }

    // Initialize Task Management
    let tasks: TaskMap = Arc::new(RwLock::new(HashMap::new()));
    log::info!("Task manager initialized.");

    // Initialize Active Keys Management
    let active_keys: handlers::ActiveKeysMap = Arc::new(RwLock::new(HashMap::new()));
    log::info!("Active keys manager initialized.");

    // Define WebSocket route using the actual handler with increased message size limit
    // Set max message size to 2GB (2 * 1024 * 1024 * 1024)
    // This is the maximum size for WebSocket messages
    let max_message_size = 2 * 1024 * 1024 * 1024;

    // Set max frame size to 2GB (2 * 1024 * 1024 * 1024)
    // This needs to be large enough to handle the largest expected frame
    let max_frame_size = 2 * 1024 * 1024 * 1024;

    log::info!("Configuring WebSocket with max_message_size={} bytes, max_frame_size={} bytes",
               max_message_size, max_frame_size);

    // Clone mutant for use in multiple routes
    let mutant_for_ws = mutant.clone();
    let mutant_for_video = mutant.clone();

    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || mutant_for_ws.clone()))
        .and(warp::any().map(move || tasks.clone()))
        .and(warp::any().map(move || active_keys.clone()))
        .map(
            move |ws: warp::ws::Ws, mutant_instance: Arc<MutAnt>, task_map: TaskMap, active_keys_map: handlers::ActiveKeysMap| {
                // Configure WebSocket with increased message size limit and frame size
                ws.max_message_size(max_message_size)
                  .max_frame_size(max_frame_size)
                  .on_upgrade(move |socket| handlers::handle_ws(socket, mutant_instance, task_map, active_keys_map))
            },
        );

    // Add video streaming endpoint (WebSocket)
    let video_stream_route = warp::path("video_stream")
        .and(warp::path::param::<String>()) // filename parameter
        .and(warp::ws())
        .and(warp::any().map(move || mutant_for_video.clone()))
        .map(
            move |filename: String, ws: warp::ws::Ws, mutant_instance: Arc<MutAnt>| {
                ws.max_message_size(max_message_size)
                  .max_frame_size(max_frame_size)
                  .on_upgrade(move |socket| handlers::handle_video_stream(socket, mutant_instance, filename))
            },
        );

    // Add HTTP video endpoint for range requests (true streaming)
    let mutant_for_http_video = mutant.clone();
    let video_http_route = warp::path("video")
        .and(warp::path::param::<String>()) // filename parameter
        .and(warp::get())
        .and(warp::header::optional::<String>("range"))
        .and(warp::any().map(move || mutant_for_http_video.clone()))
        .and_then(handlers::handle_http_video);

    // Parse the bind address from options
    let addr: SocketAddr = options.bind_address.parse()
        .map_err(|e| Error::Internal(format!("Invalid bind address '{}': {}", options.bind_address, e)))?;
    log::info!("WebSocket server listening on {}", addr);

    let ignore_ctrl_c = options.ignore_ctrl_c;

    // Create a shutdown signal receiver
    let shutdown_signal = async move {
        let ctrl_c = signal::ctrl_c();
        #[cfg(unix)]
        let sigterm = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };
        #[cfg(not(unix))]
        let sigterm = std::future::pending::<()>(); // No SIGTERM on non-Unix

        if ignore_ctrl_c {
            log::debug!("Ignoring Ctrl+C signal.");
            sigterm.await;
        } else {
            tokio::select! {
                _ = ctrl_c => { log::info!("Received Ctrl+C, shutting down."); },
                _ = sigterm => { log::info!("Received SIGTERM, shutting down."); },
            }
        }

    };

    // Combine routes
    let routes = ws_route.or(video_stream_route).or(video_http_route);

    // Start the server with graceful shutdown
    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, shutdown_signal);

    log::info!("Starting server task...");
    // Await the server task. The lock file will be released when the process exits.
    tokio::task::spawn(server).await.map_err(Error::JoinError)?;
    log::info!("Server task finished.");

    // The lock file (_lock_file) is released automatically when the process exits
    // because the file descriptor associated with the lock is closed.
    Ok(())
}
