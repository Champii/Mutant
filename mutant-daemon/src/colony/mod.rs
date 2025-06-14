use std::sync::Arc;
use tokio::sync::RwLock;
use colonylib::{KeyStore, DataStore, Graph, PodManager};
use serde_json::Value;
use autonomi::{Client, Wallet};
use mutant_lib::config::NetworkChoice;
use crate::error::Error as DaemonError;
use autonomi::client::key_derivation::DerivationIndex;

/// Default testnet private key used for development and testing
#[allow(dead_code)]
const DEV_TESTNET_PRIVATE_KEY_HEX: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

/// Colony integration manager for MutAnt daemon
///
/// This module provides indexing and search capabilities for public content
/// stored on the Autonomi network using ColonyLib's RDF-based metadata system.
pub struct ColonyManager {
    key_store: Arc<RwLock<KeyStore>>,
    data_store: Arc<RwLock<DataStore>>,
    graph: Arc<RwLock<Graph>>,
    wallet: Wallet,
    network_choice: NetworkChoice,
    initialized: bool,
    /// The user's pod public address derived from the main secret key
    pod_public_address: String,
}

#[allow(dead_code)]
fn index(i: u64) -> DerivationIndex {

    let mut bytes = [0u8; 32];

    bytes[..8].copy_from_slice(&i.to_ne_bytes());

    DerivationIndex::from_bytes(bytes)

}

impl ColonyManager {


    /// Initialize the colony manager with default configuration
    ///
    /// This will create or load existing colony data from the standard data directory.
    /// The manager can operate in public-only mode for search operations.
    /// Returns None if no private key is provided and no COLONY_MNEMONIC is set.
    pub async fn new(wallet: Wallet, network_choice: NetworkChoice, private_key_hex: Option<String>) -> Result<Option<Self>, DaemonError> {
        // Reload .env file to get the updated credentials
        if let Err(e) = dotenvy::dotenv() {
            log::debug!("No .env file found or failed to load during colony manager creation: {}", e);
        } else {
            log::info!("Reloaded environment variables from .env file for colony manager creation");
        }

        // Check if we have a private key or colony mnemonic
        let has_colony_mnemonic = std::env::var("COLONY_MNEMONIC").is_ok();

        if private_key_hex.is_none() && !has_colony_mnemonic {
            log::info!("No private key provided and no COLONY_MNEMONIC environment variable set, skipping colony initialization");
            return Ok(None);
        }

        // Initialize data store (creates directories if needed)
        let data_store = DataStore::create()
            .map_err(|e| DaemonError::ColonyError(format!("Failed to create data store: {}", e)))?;

        // Initialize or load key store
        let key_store_path = data_store.get_keystore_path();
        let mut key_store = if key_store_path.exists() {
            log::info!("Loading existing colony key store");
            let mut file = std::fs::File::open(&key_store_path)
                .map_err(|e| DaemonError::ColonyError(format!("Failed to open key store: {}", e)))?;
            KeyStore::from_file(&mut file, "password")
                .map_err(|e| DaemonError::ColonyError(format!("Failed to load key store: {}", e)))?
        } else {
            log::info!("Creating new colony key store from environment mnemonic");
            // Get mnemonic from environment variable - we already checked it exists above
            let mnemonic = std::env::var("COLONY_MNEMONIC")
                .map_err(|_| DaemonError::ColonyError("COLONY_MNEMONIC environment variable not set".to_string()))?;
            KeyStore::from_mnemonic(&mnemonic)
                .map_err(|e| DaemonError::ColonyError(format!("Failed to create key store: {}", e)))?
        };

        // Set the wallet key using the provided private key or fall back to default
        let wallet_key = match private_key_hex {
            Some(key) => {
                log::info!("Using provided private key for colony wallet");
                // Ensure the key has 0x prefix
                if key.starts_with("0x") {
                    key
                } else {
                    format!("0x{}", key)
                }
            }
            None => {
                log::warn!("No private key provided for colony manager");
                return Err(DaemonError::ColonyError("Private key required for colony manager initialization".to_string()));
            }
        };

        key_store.set_wallet_key(wallet_key.clone())
            .map_err(|e| DaemonError::ColonyError(format!("Failed to set wallet key: {}", e)))?;

        // Derive the pod public address from the wallet key
        let pod_public_address = key_store.get_address_at_index(1).unwrap();
        log::info!("Derived pod public address: {}", pod_public_address);

        // Initialize graph database
        let graph_path = data_store.get_graph_path();
        let graph = Graph::open(&graph_path)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to open graph database: {}", e)))?;

        Ok(Some(ColonyManager {
            key_store: Arc::new(RwLock::new(key_store)),
            data_store: Arc::new(RwLock::new(data_store)),
            graph: Arc::new(RwLock::new(graph)),
            wallet,
            network_choice,
            initialized: true,
            pod_public_address,
        }))
    }

    /// Initialize the colony manager with progress updates
    /// Returns None if no private key is provided and no COLONY_MNEMONIC is set.
    pub async fn new_with_progress(
        wallet: Wallet,
        network_choice: NetworkChoice,
        private_key_hex: Option<String>,
        update_tx: tokio::sync::mpsc::UnboundedSender<mutant_protocol::Response>
    ) -> Result<Option<Self>, DaemonError> {
        // Reload .env file to get the updated credentials
        if let Err(e) = dotenvy::dotenv() {
            log::debug!("No .env file found or failed to load during colony manager creation with progress: {}", e);
        } else {
            log::info!("Reloaded environment variables from .env file for colony manager creation with progress");
        }

        // Check if we have a private key or colony mnemonic
        let has_colony_mnemonic = std::env::var("COLONY_MNEMONIC").is_ok();

        if private_key_hex.is_none() && !has_colony_mnemonic {
            log::info!("No private key provided and no COLONY_MNEMONIC environment variable set, skipping colony initialization");
            return Ok(None);
        }

        // Helper function to send progress events
        let send_progress = |event: mutant_protocol::ColonyEvent| {
            let response = mutant_protocol::Response::ColonyProgress(mutant_protocol::ColonyProgressResponse {
                event,
                operation_id: None,
            });
            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send colony progress event: {}", e);
            }
        };

        // Send progress event: data store initialization started
        send_progress(mutant_protocol::ColonyEvent::DataStoreInitStarted);

        // Initialize data store (creates directories if needed)
        let data_store = DataStore::create()
            .map_err(|e| DaemonError::ColonyError(format!("Failed to create data store: {}", e)))?;

        // Send progress event: data store initialization completed
        send_progress(mutant_protocol::ColonyEvent::DataStoreInitCompleted);

        // Send progress event: key store initialization started
        send_progress(mutant_protocol::ColonyEvent::KeyStoreInitStarted);

        // Initialize or load key store
        let key_store_path = data_store.get_keystore_path();
        let mut key_store = if key_store_path.exists() {
            log::info!("Loading existing colony key store");
            let mut file = std::fs::File::open(&key_store_path)
                .map_err(|e| DaemonError::ColonyError(format!("Failed to open key store: {}", e)))?;
            KeyStore::from_file(&mut file, "password")
                .map_err(|e| DaemonError::ColonyError(format!("Failed to load key store: {}", e)))?
        } else {
            log::info!("Creating new colony key store from environment mnemonic");
            // Get mnemonic from environment variable - we already checked it exists above
            let mnemonic = std::env::var("COLONY_MNEMONIC")
                .map_err(|_| DaemonError::ColonyError("COLONY_MNEMONIC environment variable not set".to_string()))?;
            KeyStore::from_mnemonic(&mnemonic)
                .map_err(|e| DaemonError::ColonyError(format!("Failed to create key store: {}", e)))?
        };

        // Send progress event: key store initialization completed
        send_progress(mutant_protocol::ColonyEvent::KeyStoreInitCompleted);

        // Send progress event: key derivation started
        send_progress(mutant_protocol::ColonyEvent::KeyDerivationStarted);

        // Set the wallet key using the provided private key or fall back to default
        let wallet_key = match private_key_hex {
            Some(key) => {
                log::info!("Using provided private key for colony wallet");
                // Ensure the key has 0x prefix
                if key.starts_with("0x") {
                    key
                } else {
                    format!("0x{}", key)
                }
            }
            None => {
                log::warn!("No private key provided for colony manager");
                return Err(DaemonError::ColonyError("Private key required for colony manager initialization".to_string()));
            }
        };

        key_store.set_wallet_key(wallet_key.clone())
            .map_err(|e| DaemonError::ColonyError(format!("Failed to set wallet key: {}", e)))?;

        // Derive the pod public address from the wallet key
        let pod_public_address = key_store.get_address_at_index(1).unwrap();
        log::info!("Derived pod public address: {}", pod_public_address);

        // Send progress event: key derivation completed
        send_progress(mutant_protocol::ColonyEvent::KeyDerivationCompleted {
            pod_address: pod_public_address.clone()
        });

        // Send progress event: graph initialization started
        send_progress(mutant_protocol::ColonyEvent::GraphInitStarted);

        // Initialize graph database
        let graph_path = data_store.get_graph_path();
        let graph = Graph::open(&graph_path)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to open graph database: {}", e)))?;

        // Send progress event: graph initialization completed
        send_progress(mutant_protocol::ColonyEvent::GraphInitCompleted);

        Ok(Some(ColonyManager {
            key_store: Arc::new(RwLock::new(key_store)),
            data_store: Arc::new(RwLock::new(data_store)),
            graph: Arc::new(RwLock::new(graph)),
            wallet,
            network_choice,
            initialized: true,
            pod_public_address,
        }))
    }

    /// Get a client configured for the correct network
    async fn get_client(&self) -> Result<Client, DaemonError> {
        match self.network_choice {
            NetworkChoice::Mainnet => Client::init().await
                .map_err(|e| DaemonError::ColonyError(format!("Failed to initialize mainnet client: {}", e))),
            NetworkChoice::Devnet => Client::init_local().await
                .map_err(|e| DaemonError::ColonyError(format!("Failed to initialize devnet client: {}", e))),
            NetworkChoice::Alphanet => {
                // For alphanet, we need to use init_with_config with specific configuration
                // For now, fall back to mainnet init - this should be improved
                Client::init().await
                    .map_err(|e| DaemonError::ColonyError(format!("Failed to initialize alphanet client: {}", e)))
            }
        }
    }

    /// Search for content using SPARQL queries
    /// 
    /// This method accepts various query formats:
    /// - Simple text search: `"search term"`
    /// - Structured queries: `{"type": "text", "text": "search term", "limit": 10}`
    /// - Type-based search: `{"type": "by_type", "type_uri": "http://schema.org/VideoObject"}`
    /// - Advanced search: `{"type": "advanced", "text": "term", "type": "uri", "limit": 5}`
    pub async fn search(&self, query: Value) -> Result<Value, DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }
        
        log::debug!("Colony search request: {}", query);
        
        // For search operations, we need a temporary PodManager
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        let results = pod_manager.search(query).await
            .map_err(|e| DaemonError::ColonyError(format!("Search failed: {}", e)))?;
        
        log::debug!("Colony search results: {}", results);
        Ok(results)
    }
    
    /// Auto-generate metadata for a public upload and add it to the user's pod
    ///
    /// This is called automatically when a public file is uploaded successfully.
    /// It generates appropriate Schema.org metadata based on the file information.
    pub async fn auto_index_public_upload(
        &self,
        user_key: &str,
        filename: Option<String>,
        file_size: u64,
        public_address: String,
    ) -> Result<(), DaemonError> {
        if !self.initialized {
            log::warn!("Colony manager not initialized, skipping auto-indexing for key: {}", user_key);
            return Ok(()); // Don't fail the upload if colony isn't available
        }

        log::info!("Auto-indexing public upload: key={}, filename={:?}, size={}, public_address={}",
                   user_key, filename, file_size, public_address);

        // Generate metadata based on file information
        let metadata = self.generate_file_metadata(user_key, filename.as_deref(), file_size, &public_address);

        // Use existing index_content method
        match self.index_content(user_key, metadata, Some(public_address)).await {
            Ok(_) => {
                log::info!("Successfully auto-indexed public upload for key: {}", user_key);
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to auto-index public upload for key {}: {}", user_key, e);
                // Don't fail the upload if colony indexing fails
                Ok(())
            }
        }
    }

    /// Generate Schema.org metadata for a file upload
    fn generate_file_metadata(&self, user_key: &str, filename: Option<&str>, file_size: u64, public_address: &str) -> Value {
        let file_name = filename.unwrap_or(user_key);

        // Determine content type based on file extension
        let (schema_type, encoding_format) = self.determine_content_type(file_name);

        // Use the pod address instead of user_key for author and description
        let pod_address = &self.pod_public_address;

        // Use proper Schema.org format with schema: prefix like the example
        serde_json::json!({
            "@context": {"schema": "http://schema.org/"},
            "@type": schema_type,
            "@id": format!("ant://{}", public_address),
            "schema:name": file_name,
            "schema:description": format!("File uploaded by {}", pod_address),
            "schema:contentSize": file_size,
            "schema:encodingFormat": encoding_format,
            "schema:author": pod_address,
            "schema:dateCreated": chrono::Utc::now().to_rfc3339(),
            "schema:url": public_address
        })
    }

    /// Determine Schema.org content type and encoding format based on filename
    fn determine_content_type(&self, filename: &str) -> (&'static str, &'static str) {
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            // Video formats
            "mp4" => ("schema:VideoObject", "video/mp4"),
            "avi" => ("schema:VideoObject", "video/avi"),
            "mkv" => ("schema:VideoObject", "video/mkv"),
            "mov" => ("schema:VideoObject", "video/mov"),
            "wmv" => ("schema:VideoObject", "video/wmv"),
            "flv" => ("schema:VideoObject", "video/flv"),
            "webm" => ("schema:VideoObject", "video/webm"),

            // Audio formats
            "mp3" => ("schema:AudioObject", "audio/mp3"),
            "wav" => ("schema:AudioObject", "audio/wav"),
            "flac" => ("schema:AudioObject", "audio/flac"),
            "aac" => ("schema:AudioObject", "audio/aac"),
            "ogg" => ("schema:AudioObject", "audio/ogg"),
            "m4a" => ("schema:AudioObject", "audio/m4a"),

            // Image formats
            "jpg" | "jpeg" => ("schema:ImageObject", "image/jpeg"),
            "png" => ("schema:ImageObject", "image/png"),
            "gif" => ("schema:ImageObject", "image/gif"),
            "bmp" => ("schema:ImageObject", "image/bmp"),
            "svg" => ("schema:ImageObject", "image/svg+xml"),
            "webp" => ("schema:ImageObject", "image/webp"),

            // Document formats
            "pdf" => ("schema:DigitalDocument", "application/pdf"),
            "doc" | "docx" => ("schema:DigitalDocument", "application/msword"),
            "txt" => ("schema:TextDigitalDocument", "text/plain"),
            "html" | "htm" => ("schema:WebPage", "text/html"),
            "json" => ("schema:Dataset", "application/json"),
            "xml" => ("schema:Dataset", "application/xml"),
            "csv" => ("schema:Dataset", "text/csv"),

            // Archive formats
            "zip" => ("schema:DataDownload", "application/zip"),
            "rar" => ("schema:DataDownload", "application/rar"),
            "7z" => ("schema:DataDownload", "application/7z"),
            "tar" => ("schema:DataDownload", "application/tar"),
            "gz" => ("schema:DataDownload", "application/gzip"),

            // Default for unknown types
            _ => ("schema:MediaObject", "application/octet-stream")
        }
    }

    /// Index content by creating metadata pods
    ///
    /// This creates a new pod containing RDF metadata about the specified content.
    /// The metadata should be in JSON-LD format following Schema.org conventions.
    pub async fn index_content(
        &self,
        user_key: &str,
        metadata: Value,
        public_address: Option<String>,
    ) -> Result<(bool, Option<String>), DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::info!("Indexing content: key={}, public_address={:?}", user_key, public_address);

        // Initialize client for pod operations
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        // Get or create the user's main pod
        let user_pod_address = self.get_or_create_user_pod(&mut pod_manager, user_key).await?;

        // Add the content metadata to the user's pod
        let subject_id = public_address.unwrap_or_else(|| format!("file_{}", user_key.replace(".", "_")));
        let metadata_str = serde_json::to_string(&metadata)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to serialize metadata: {}", e)))?;

        log::debug!("Generated metadata JSON: {}", metadata_str);
        log::debug!("Subject ID: {}", subject_id);
        log::debug!("User pod address: {}", user_pod_address);

        pod_manager.put_subject_data(&user_pod_address, &subject_id, &metadata_str).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to add metadata to pod (pod_address: {}, subject_id: {}, metadata_len: {}): {}",
                user_pod_address, subject_id, metadata_str.len(), e)))?;

        // Upload the updated pod to the network
        pod_manager.upload_all().await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to upload pod: {}", e)))?;

        log::info!("Successfully indexed content in pod: {}", user_pod_address);
        Ok((true, Some(user_pod_address.to_string())))
    }

    /// Ensure the user's main pod exists, creating it if necessary
    pub async fn ensure_user_pod_exists(&self) -> Result<String, DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::info!("Ensuring user pod exists: {}", self.pod_public_address);
        log::debug!("Starting ensure_user_pod_exists method");

        // Initialize client for pod operations
        log::debug!("Getting client for pod operations");
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        log::debug!("Acquiring locks for data_store, key_store, and graph");
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        log::debug!("Creating PodManager");
        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;
        log::debug!("PodManager created successfully");

        // First, check if pod exists locally
        log::debug!("Checking if pod exists locally: {}", self.pod_public_address);
        log::debug!("About to call pod_manager.get_subject_data()");

        // Add timeout to prevent hanging indefinitely
        let get_data_future = pod_manager.get_subject_data(&self.pod_public_address);
        match tokio::time::timeout(tokio::time::Duration::from_secs(30), get_data_future).await {
            Ok(result) => match result {
                Ok(data) => {
                log::debug!("get_subject_data() returned Ok, data length: {}", data.len());
                // Parse the JSON response to check if there are actual results
                match serde_json::from_str::<serde_json::Value>(&data) {
                    Ok(json) => {
                        if let Some(results) = json.get("results")
                            .and_then(|r| r.get("bindings"))
                            .and_then(|b| b.as_array()) {
                            if !results.is_empty() {
                                // Pod exists locally with actual data
                                log::info!("Pod already exists locally with data: {}", self.pod_public_address);
                                return Ok(self.pod_public_address.clone());
                            } else {
                                // Empty results, pod doesn't exist locally
                                log::debug!("Pod query returned empty results locally: {}", self.pod_public_address);
                            }
                        } else {
                            // Malformed response, treat as not found
                            log::debug!("Pod query returned malformed response locally: {}", self.pod_public_address);
                        }
                    }
                    Err(e) => {
                        // Failed to parse JSON, treat as not found
                        log::debug!("Failed to parse pod query response locally: {}", e);
                    }
                }
                    // Pod doesn't exist locally, try to sync from network
                    log::info!("Pod not found locally, syncing from network: {}", self.pod_public_address);
                }
                Err(e) => {
                    // Pod doesn't exist locally, try to sync from network
                    log::info!("Pod not found locally (error), syncing from network: {}", self.pod_public_address);
                    log::debug!("get_subject_data() error: {}", e);
                }
            }
            Err(_) => {
                // Timeout occurred
                log::warn!("Timeout occurred while checking if pod exists locally: {}", self.pod_public_address);
                log::info!("Pod check timed out, syncing from network: {}", self.pod_public_address);
            }
        }

        // Sync from network to get latest pods
        log::debug!("Starting refresh_cache() to sync from network");
        pod_manager.refresh_ref(10).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to refresh cache from network: {}", e)))?;
        log::debug!("Completed refresh_cache() successfully");

        // Check again if pod exists locally after sync
        log::debug!("Checking if pod exists locally after sync: {}", self.pod_public_address);
        match pod_manager.get_subject_data(&self.pod_public_address).await {
            Ok(data) => {
                // Parse the JSON response to check if there are actual results
                match serde_json::from_str::<serde_json::Value>(&data) {
                    Ok(json) => {
                        if let Some(results) = json.get("results")
                            .and_then(|r| r.get("bindings"))
                            .and_then(|b| b.as_array()) {
                            if !results.is_empty() {
                                // Pod now exists locally after sync with actual data
                                log::info!("Pod found locally after sync with data: {}", self.pod_public_address);
                                return Ok(self.pod_public_address.clone());
                            } else {
                                // Empty results, pod still doesn't exist locally
                                log::debug!("Pod query returned empty results after sync: {}", self.pod_public_address);
                            }
                        } else {
                            // Malformed response, treat as not found
                            log::debug!("Pod query returned malformed response after sync: {}", self.pod_public_address);
                        }
                    }
                    Err(e) => {
                        // Failed to parse JSON, treat as not found
                        log::debug!("Failed to parse pod query response after sync: {}", e);
                    }
                }
                // Pod still doesn't exist, need to create it
                log::info!("Pod not found after sync, creating new pod: {}", self.pod_public_address);
            }
            Err(_) => {
                // Pod still doesn't exist, need to create it
                log::info!("Pod not found after sync (error), creating new pod: {}", self.pod_public_address);
            }
        }

        // Create the pod since it doesn't exist on the network
        let (pod_address, _scratchpad_address) = pod_manager.add_pod("main").await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod: {}", e)))?;

        // Verify the created pod address matches our expected address
        if pod_address != self.pod_public_address {
            log::warn!("Created pod address {} doesn't match expected address {}",
                      pod_address, self.pod_public_address);
        }

        // Add basic metadata about the pod itself
        let pod_metadata = serde_json::json!({
            "type": "UserPod",
            "name": format!("Content Pod for {}", self.pod_public_address),
            "description": format!("Metadata pod containing all public content for user {}", self.pod_public_address),
            "owner": self.pod_public_address,
            "created": chrono::Utc::now().to_rfc3339()
        });

        let pod_metadata_str = serde_json::to_string(&pod_metadata)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to serialize pod metadata: {}", e)))?;

        let pod_subject_id = format!("pod_{}", pod_address);
        log::debug!("Adding pod metadata: pod_address={}, subject_id={}, metadata_len={}",
                   pod_address, pod_subject_id, pod_metadata_str.len());

        pod_manager.put_subject_data(&pod_address, &pod_subject_id, &pod_metadata_str).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to add pod metadata (pod_address: {}, subject_id: {}, metadata_len: {}): {}",
                pod_address, pod_subject_id, pod_metadata_str.len(), e)))?;

        // Upload the pod to the network
        pod_manager.upload_all().await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to upload pod: {}", e)))?;

        log::info!("Successfully created and uploaded user pod: {}", pod_address);
        Ok(pod_address)
    }

    /// Ensure the user's main pod exists with progress updates
    pub async fn ensure_user_pod_exists_with_progress(
        &self,
        update_tx: tokio::sync::mpsc::UnboundedSender<mutant_protocol::Response>
    ) -> Result<String, DaemonError> {
        // Helper function to send progress events
        let send_progress = |event: mutant_protocol::ColonyEvent| {
            let response = mutant_protocol::Response::ColonyProgress(mutant_protocol::ColonyProgressResponse {
                event,
                operation_id: None,
            });
            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send colony progress event: {}", e);
            }
        };

        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::info!("Ensuring user pod exists: {}", self.pod_public_address);

        // Send progress event: user pod verification started
        send_progress(mutant_protocol::ColonyEvent::UserPodVerificationStarted {
            pod_address: self.pod_public_address.clone()
        });

        // Initialize client for pod operations
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        // First, check if pod exists locally
        match pod_manager.get_subject_data(&self.pod_public_address).await {
            Ok(data) => {
                // Parse the JSON response to check if there are actual results
                match serde_json::from_str::<serde_json::Value>(&data) {
                    Ok(json) => {
                        if let Some(results) = json.get("results")
                            .and_then(|r| r.get("bindings"))
                            .and_then(|b| b.as_array()) {
                            if !results.is_empty() {
                                // Pod exists locally with actual data
                                log::info!("Pod already exists locally with data: {}", self.pod_public_address);

                                // Send progress event: user pod verification completed
                                send_progress(mutant_protocol::ColonyEvent::UserPodVerificationCompleted {
                                    pod_address: self.pod_public_address.clone(),
                                    exists: true
                                });

                                return Ok(self.pod_public_address.clone());
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }

        // Pod doesn't exist locally, try to sync from network
        log::info!("Pod not found locally, syncing from network: {}", self.pod_public_address);

        // Send progress event: pod refresh started
        send_progress(mutant_protocol::ColonyEvent::PodRefreshStarted { total_pods: 1 });

        pod_manager.refresh_ref(10).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to refresh cache from network: {}", e)))?;

        // Send progress event: pod refresh completed
        send_progress(mutant_protocol::ColonyEvent::PodRefreshCompleted { refreshed_pods: 1 });

        // Check again if pod exists locally after sync
        match pod_manager.get_subject_data(&self.pod_public_address).await {
            Ok(data) => {
                match serde_json::from_str::<serde_json::Value>(&data) {
                    Ok(json) => {
                        if let Some(results) = json.get("results")
                            .and_then(|r| r.get("bindings"))
                            .and_then(|b| b.as_array()) {
                            if !results.is_empty() {
                                // Pod now exists locally after sync with actual data
                                log::info!("Pod found locally after sync with data: {}", self.pod_public_address);

                                // Send progress event: user pod verification completed
                                send_progress(mutant_protocol::ColonyEvent::UserPodVerificationCompleted {
                                    pod_address: self.pod_public_address.clone(),
                                    exists: true
                                });

                                return Ok(self.pod_public_address.clone());
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }

        // Pod still doesn't exist, need to create it
        log::info!("Pod not found after sync, creating new pod: {}", self.pod_public_address);

        // Send progress event: user pod creation started
        send_progress(mutant_protocol::ColonyEvent::UserPodCreationStarted);

        // Create the pod since it doesn't exist on the network
        let (pod_address, _scratchpad_address) = pod_manager.add_pod("main").await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod: {}", e)))?;

        // Verify the created pod address matches our expected address
        if pod_address != self.pod_public_address {
            log::warn!("Created pod address {} doesn't match expected address {}",
                      pod_address, self.pod_public_address);
        }

        // Add basic metadata about the pod itself
        let pod_metadata = serde_json::json!({
            "type": "UserPod",
            "name": format!("Content Pod for {}", self.pod_public_address),
            "description": format!("Metadata pod containing all public content for user {}", self.pod_public_address),
            "owner": self.pod_public_address,
            "created": chrono::Utc::now().to_rfc3339()
        });

        let pod_metadata_str = serde_json::to_string(&pod_metadata)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to serialize pod metadata: {}", e)))?;

        let pod_subject_id = format!("pod_{}", pod_address);

        pod_manager.put_subject_data(&pod_address, &pod_subject_id, &pod_metadata_str).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to add pod metadata: {}", e)))?;

        // Upload the pod to the network
        pod_manager.upload_all().await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to upload pod: {}", e)))?;

        // Send progress event: user pod creation completed
        send_progress(mutant_protocol::ColonyEvent::UserPodCreationCompleted {
            pod_address: pod_address.clone()
        });

        // Send progress event: user pod verification completed
        send_progress(mutant_protocol::ColonyEvent::UserPodVerificationCompleted {
            pod_address: pod_address.clone(),
            exists: true
        });

        log::info!("Successfully created and uploaded user pod: {}", pod_address);
        Ok(pod_address)
    }

    /// Get or create a user's main pod for storing their content metadata
    async fn get_or_create_user_pod(
        &self,
        pod_manager: &mut PodManager<'_>,
        _user_key: &str,
    ) -> Result<String, DaemonError> {
        // Reuse the existing pod_manager to avoid deadlock
        // First, check if pod exists locally
        log::debug!("Checking if pod exists locally: {}", self.pod_public_address);

        // Add timeout to prevent hanging indefinitely
        let get_data_future = pod_manager.get_subject_data(&self.pod_public_address);
        match tokio::time::timeout(tokio::time::Duration::from_secs(30), get_data_future).await {
            Ok(result) => match result {
                Ok(data) => {
                    log::debug!("get_subject_data() returned Ok, data length: {}", data.len());
                    // Parse the JSON response to check if there are actual results
                    match serde_json::from_str::<serde_json::Value>(&data) {
                        Ok(json) => {
                            if let Some(results) = json.get("results")
                                .and_then(|r| r.get("bindings"))
                                .and_then(|b| b.as_array()) {
                                if !results.is_empty() {
                                    // Pod exists locally with actual data
                                    log::info!("Pod already exists locally with data: {}", self.pod_public_address);
                                    return Ok(self.pod_public_address.clone());
                                } else {
                                    // Empty results, pod doesn't exist locally
                                    log::debug!("Pod query returned empty results locally: {}", self.pod_public_address);
                                }
                            } else {
                                // Malformed response, treat as not found
                                log::debug!("Pod query returned malformed response locally: {}", self.pod_public_address);
                            }
                        }
                        Err(e) => {
                            // Failed to parse JSON, treat as not found
                            log::debug!("Failed to parse pod query response locally: {}", e);
                        }
                    }
                    // Pod doesn't exist locally, try to sync from network
                    log::info!("Pod not found locally, syncing from network: {}", self.pod_public_address);
                }
                Err(e) => {
                    // Pod doesn't exist locally, try to sync from network
                    log::info!("Pod not found locally (error), syncing from network: {}", self.pod_public_address);
                    log::debug!("get_subject_data() error: {}", e);
                }
            }
            Err(_) => {
                // Timeout occurred
                log::warn!("Timeout occurred while checking if pod exists locally: {}", self.pod_public_address);
                log::info!("Pod check timed out, syncing from network: {}", self.pod_public_address);
            }
        }

        // Sync from network to get latest pods
        log::debug!("Starting refresh_ref() to sync from network");
        pod_manager.refresh_ref(10).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to refresh cache from network: {}", e)))?;
        log::debug!("Completed refresh_ref() successfully");

        // Check again if pod exists locally after sync
        log::debug!("Checking if pod exists locally after sync: {}", self.pod_public_address);
        match pod_manager.get_subject_data(&self.pod_public_address).await {
            Ok(data) => {
                // Parse the JSON response to check if there are actual results
                match serde_json::from_str::<serde_json::Value>(&data) {
                    Ok(json) => {
                        if let Some(results) = json.get("results")
                            .and_then(|r| r.get("bindings"))
                            .and_then(|b| b.as_array()) {
                            if !results.is_empty() {
                                // Pod now exists locally after sync with actual data
                                log::info!("Pod found locally after sync with data: {}", self.pod_public_address);
                                return Ok(self.pod_public_address.clone());
                            } else {
                                // Empty results, pod still doesn't exist locally
                                log::debug!("Pod query returned empty results after sync: {}", self.pod_public_address);
                            }
                        } else {
                            // Malformed response, treat as not found
                            log::debug!("Pod query returned malformed response after sync: {}", self.pod_public_address);
                        }
                    }
                    Err(e) => {
                        // Failed to parse JSON, treat as not found
                        log::debug!("Failed to parse pod query response after sync: {}", e);
                    }
                }
                // Pod still doesn't exist, need to create it
                log::info!("Pod not found after sync, creating new pod: {}", self.pod_public_address);
            }
            Err(_) => {
                // Pod still doesn't exist, need to create it
                log::info!("Pod not found after sync (error), creating new pod: {}", self.pod_public_address);
            }
        }

        // Create the pod since it doesn't exist on the network
        let (pod_address, _scratchpad_address) = pod_manager.add_pod("main").await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod: {}", e)))?;

        // Verify the created pod address matches our expected address
        if pod_address != self.pod_public_address {
            log::warn!("Created pod address {} doesn't match expected address {}",
                      pod_address, self.pod_public_address);
        }

        // Add basic metadata about the pod itself
        let pod_metadata = serde_json::json!({
            "type": "UserPod",
            "name": format!("Content Pod for {}", self.pod_public_address),
            "description": format!("Metadata pod containing all public content for user {}", self.pod_public_address),
            "owner": self.pod_public_address,
            "created": chrono::Utc::now().to_rfc3339()
        });

        let pod_metadata_str = serde_json::to_string(&pod_metadata)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to serialize pod metadata: {}", e)))?;

        let pod_subject_id = format!("pod_{}", pod_address);
        log::debug!("Adding pod metadata: pod_address={}, subject_id={}, metadata_len={}",
                   pod_address, pod_subject_id, pod_metadata_str.len());

        pod_manager.put_subject_data(&pod_address, &pod_subject_id, &pod_metadata_str).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to add pod metadata (pod_address: {}, subject_id: {}, metadata_len: {}): {}",
                pod_address, pod_subject_id, pod_metadata_str.len(), e)))?;

        // Note: We don't call upload_all() here because the caller (index_content) will do it
        log::info!("Successfully created user pod: {}", pod_address);
        Ok(pod_address)
    }
    
    /// Get metadata for a specific address
    /// 
    /// This searches the local graph database for metadata about the specified address.
    pub async fn get_metadata(&self, address: &str) -> Result<Value, DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }
        
        log::debug!("Getting metadata for address: {}", address);
        
        // For metadata retrieval, we need a temporary PodManager
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        let metadata_json = pod_manager.get_subject_data(address).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to get metadata: {}", e)))?;
        
        let metadata: Value = serde_json::from_str(&metadata_json)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to parse metadata JSON: {}", e)))?;
        
        Ok(metadata)
    }

    /// Get the user's pod public address
    #[allow(dead_code)]
    pub fn get_pod_address(&self) -> &str {
        &self.pod_public_address
    }

    /// Add a contact (pod address) to sync with
    #[allow(dead_code)]
    pub async fn add_contact(&self, pod_address: &str, contact_name: Option<String>) -> Result<(), DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::info!("Adding contact pod: {} (name: {:?})", pod_address, contact_name);

        // Ensure our own pod exists before trying to add references to it
        self.ensure_user_pod_exists().await?;

        // Initialize client for pod operations
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        // Use our pod public address instead of "main"
        pod_manager.add_pod_ref(&self.pod_public_address, pod_address)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to add pod reference: {}", e)))?;

        log::debug!("Added pod reference: {} to our pod: {} at depth 1", pod_address, self.pod_public_address);

        // Now download and sync the contact's pod using refresh_ref
        pod_manager.refresh_ref(10).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to sync contact pod: {}", e)))?;

        pod_manager.upload_all().await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to upload pod: {}", e)))?;

        log::info!("Successfully added and synced contact pod: {}", pod_address);
        Ok(())
    }

    /// Add a contact (pod address) to sync with - with progress updates
    pub async fn add_contact_with_progress(
        &self,
        pod_address: &str,
        contact_name: Option<String>,
        update_tx: tokio::sync::mpsc::UnboundedSender<mutant_protocol::Response>
    ) -> Result<(), DaemonError> {
        // Helper function to send progress events
        let send_progress = |event: mutant_protocol::ColonyEvent| {
            let response = mutant_protocol::Response::ColonyProgress(mutant_protocol::ColonyProgressResponse {
                event,
                operation_id: None,
            });
            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send colony progress event: {}", e);
            }
        };

        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::info!("Adding contact pod: {} (name: {:?})", pod_address, contact_name);

        // Send progress event: contact verification started
        send_progress(mutant_protocol::ColonyEvent::ContactVerificationStarted {
            pod_address: pod_address.to_string()
        });

        // Ensure our own pod exists before trying to add references to it
        self.ensure_user_pod_exists_with_progress(update_tx.clone()).await?;

        // Initialize client for pod operations
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        // Send progress event: contact verification completed (assuming it exists if we got this far)
        send_progress(mutant_protocol::ColonyEvent::ContactVerificationCompleted {
            pod_address: pod_address.to_string(),
            exists: true
        });

        // Send progress event: contact pod reference addition started
        send_progress(mutant_protocol::ColonyEvent::ContactRefAdditionStarted {
            pod_address: pod_address.to_string()
        });

        // Use our pod public address instead of "main"
        pod_manager.add_pod_ref(&self.pod_public_address, pod_address)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to add pod reference: {}", e)))?;

        log::debug!("Added pod reference: {} to our pod: {} at depth 1", pod_address, self.pod_public_address);

        // Send progress event: contact pod reference addition completed
        send_progress(mutant_protocol::ColonyEvent::ContactRefAdditionCompleted {
            pod_address: pod_address.to_string()
        });

        // Send progress event: contact pod download started
        send_progress(mutant_protocol::ColonyEvent::ContactPodDownloadStarted {
            pod_address: pod_address.to_string()
        });

        // Now download and sync the contact's pod using refresh_ref
        pod_manager.refresh_ref(10).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to sync contact pod: {}", e)))?;

        // Send progress event: contact pod download completed
        send_progress(mutant_protocol::ColonyEvent::ContactPodDownloadCompleted {
            pod_address: pod_address.to_string()
        });

        // Send progress event: contact pod upload started
        send_progress(mutant_protocol::ColonyEvent::ContactPodUploadStarted);

        pod_manager.upload_all().await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to upload pod: {}", e)))?;

        // Send progress event: contact pod upload completed
        send_progress(mutant_protocol::ColonyEvent::ContactPodUploadCompleted);

        log::info!("Successfully added and synced contact pod: {}", pod_address);
        Ok(())
    }

    /// List all available content from synced pods
    pub async fn list_all_content(&self) -> Result<Value, DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::debug!("Listing all content from synced pods");

        // Use a broad text search to get all content, then filter in the parsing stage
        // Since ColonyLib doesn't support custom SPARQL queries, we'll use text search
        let search_query = serde_json::json!({
            "type": "text",
            "text": "",
            "limit": 1000
        });

        self.search(search_query).await
    }

    /// Sync all contact pods to get latest content
    #[allow(dead_code)]
    pub async fn sync_all_contacts(&self) -> Result<usize, DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::info!("Syncing all contact pods");

        // Initialize client for pod operations
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        // Refresh cache to get latest data from all known pods
        pod_manager.refresh_ref(10).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to refresh cache: {}", e)))?;

        // Get count of synced pods from key store
        let pod_count = pod_manager.key_store.get_pointers().len();

        log::info!("Successfully synced {} pods", pod_count);
        Ok(pod_count)
    }

    /// Sync all contact pods to get latest content - with progress updates
    pub async fn sync_all_contacts_with_progress(
        &self,
        update_tx: tokio::sync::mpsc::UnboundedSender<mutant_protocol::Response>
    ) -> Result<usize, DaemonError> {
        // Helper function to send progress events
        let send_progress = |event: mutant_protocol::ColonyEvent| {
            let response = mutant_protocol::Response::ColonyProgress(mutant_protocol::ColonyProgressResponse {
                event,
                operation_id: None,
            });
            if let Err(e) = update_tx.send(response) {
                log::error!("Failed to send colony progress event: {}", e);
            }
        };

        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::info!("Syncing all contact pods");

        // Get contacts list first for progress tracking
        let contacts = self.get_contacts().await?;
        let total_contacts = contacts.len();

        log::debug!("sync_all_contacts_with_progress: Found {} contacts to sync", total_contacts);

        // Initialize client for pod operations
        let client = self.get_client().await?;

        // Get locks and hold them for the duration of the operation
        let mut data_store = self.data_store.write().await;
        let mut key_store = self.key_store.write().await;
        let mut graph = self.graph.write().await;

        let mut pod_manager = PodManager::new(
            client,
            &self.wallet,
            &mut *data_store,
            &mut *key_store,
            &mut *graph,
        ).await
        .map_err(|e| DaemonError::ColonyError(format!("Failed to create pod manager: {}", e)))?;

        // Send progress event: pod refresh started
        log::debug!("Sending PodRefreshStarted event for {} pods", total_contacts);
        send_progress(mutant_protocol::ColonyEvent::PodRefreshStarted {
            total_pods: total_contacts
        });

        // Send individual contact sync events
        for (index, contact) in contacts.iter().enumerate() {
            log::debug!("Sending ContactSyncStarted event for contact {} ({}/{})", contact, index + 1, total_contacts);
            send_progress(mutant_protocol::ColonyEvent::ContactSyncStarted {
                pod_address: contact.clone(),
                contact_index: index + 1,
                total_contacts
            });

            // Note: The actual sync happens in refresh_ref which syncs all contacts at once
            // So we'll send completion events after the refresh_ref call
        }

        // Refresh cache to get latest data from all known pods
        log::debug!("Starting refresh_ref operation");
        pod_manager.refresh_ref(10).await
            .map_err(|e| DaemonError::ColonyError(format!("Failed to refresh cache: {}", e)))?;
        log::debug!("Completed refresh_ref operation");

        // Send individual contact sync completion events
        for (index, contact) in contacts.iter().enumerate() {
            log::debug!("Sending ContactSyncCompleted event for contact {} ({}/{})", contact, index + 1, total_contacts);
            send_progress(mutant_protocol::ColonyEvent::ContactSyncCompleted {
                pod_address: contact.clone(),
                contact_index: index + 1,
                total_contacts
            });
        }

        // Send progress event: pod refresh completed
        log::debug!("Sending PodRefreshCompleted event for {} pods", total_contacts);
        send_progress(mutant_protocol::ColonyEvent::PodRefreshCompleted {
            refreshed_pods: total_contacts
        });

        // Get count of synced pods from key store
        let pod_count = pod_manager.key_store.get_pointers().len();

        log::info!("Successfully synced {} pods", pod_count);
        Ok(pod_count)
    }

    /// Get the user's own contact information that can be shared with friends
    pub async fn get_user_contact_info(&self) -> Result<(String, String, Option<String>), DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::debug!("Getting user contact information");

        // Use the pod public address as the primary contact method
        let pod_address = self.pod_public_address.clone();
        let contact_type = "pod".to_string();

        // For display name, use a truncated version of the pod address
        let display_name = if pod_address.len() > 16 {
            Some(format!("{}...{}", &pod_address[0..8], &pod_address[pod_address.len()-8..]))
        } else {
            Some(pod_address.clone())
        };

        log::debug!("User contact info: pod_address={}, type={}, display_name={:?}",
                   pod_address, contact_type, display_name);

        Ok((pod_address, contact_type, display_name))
    }

    /// Get the list of contacts (pod references) for this user
    pub async fn get_contacts(&self) -> Result<Vec<String>, DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }

        log::debug!("Getting contacts list using content discovery approach");

        // Use the same approach as content discovery - search for all content and extract unique pod addresses
        // This is the proven working method that the UI uses to organize content by contacts
        let search_query = serde_json::json!({
            "type": "text",
            "text": "",
            "limit": 1000
        });

        let search_results = self.search(search_query).await?;
        log::debug!("Content search results for contact extraction: {}", search_results);

        // Also log the structure to understand the format
        if let Some(results) = search_results.get("results") {
            if let Some(bindings) = results.get("bindings") {
                if let Some(bindings_array) = bindings.as_array() {
                    log::debug!("Search found {} bindings", bindings_array.len());
                    for (i, binding) in bindings_array.iter().take(3).enumerate() {
                        log::debug!("Binding {}: {}", i, binding);
                    }
                } else {
                    log::debug!("No bindings array found in search results");
                }
            } else {
                log::debug!("No bindings found in search results");
            }
        } else {
            log::debug!("No results found in search response");
        }

        // Extract unique pod addresses from the search results
        let contacts = self.extract_contacts_from_content(&search_results)?;

        log::debug!("Found {} contacts from content discovery", contacts.len());
        log::debug!("Contacts: {:?}", contacts);

        Ok(contacts)
    }

    /// Extract unique contact pod addresses from content search results
    fn extract_contacts_from_content(&self, search_results: &Value) -> Result<Vec<String>, DaemonError> {
        let mut contacts = Vec::new();

        // The search results have a direct "pods_found" field that lists all pod addresses
        if let Some(pods_found) = search_results.get("pods_found") {
            if let Some(pods_array) = pods_found.as_array() {
                log::debug!("Found pods_found array with {} entries", pods_array.len());

                for pod_entry in pods_array {
                    if let Some(pod_uri) = pod_entry.as_str() {
                        // Remove the "ant://" prefix to get the pod address
                        let pod_address = if pod_uri.starts_with("ant://") {
                            pod_uri.replace("ant://", "")
                        } else {
                            pod_uri.to_string()
                        };

                        // Don't include our own pod address
                        if pod_address != self.pod_public_address {
                            log::debug!("Adding contact pod: {}", pod_address);
                            contacts.push(pod_address);
                        } else {
                            log::debug!("Skipping own pod address: {}", pod_address);
                        }
                    }
                }
            } else {
                log::debug!("pods_found is not an array");
            }
        } else {
            log::debug!("No pods_found field in search results");
        }

        log::debug!("Extracted {} unique contact pods from content", contacts.len());
        Ok(contacts)
    }

    /// Refresh the local cache from the network
    /// 
    /// This downloads new pods and updates the local graph database.
    pub async fn refresh_cache(&self) -> Result<(), DaemonError> {
        if !self.initialized {
            return Err(DaemonError::ColonyError("Colony manager not initialized".to_string()));
        }
        
        log::info!("Refreshing colony cache from network");
        
        // TODO: Implement cache refresh
        // This would call pod_manager.refresh_cache() and pod_manager.refresh_ref()
        
        Ok(())
    }
    
    /// Save the key store to disk
    pub async fn save_key_store(&self) -> Result<(), DaemonError> {
        let data_store = self.data_store.read().await;
        let key_store = self.key_store.read().await;
        let key_store_path = data_store.get_keystore_path();
        
        let mut file = std::fs::File::create(&key_store_path)
            .map_err(|e| DaemonError::ColonyError(format!("Failed to create key store file: {}", e)))?;
        
        key_store.to_file(&mut file, "mutant_colony_default")
            .map_err(|e| DaemonError::ColonyError(format!("Failed to save key store: {}", e)))?;
        
        log::info!("Colony key store saved to {:?}", key_store_path);
        Ok(())
    }
}



impl Drop for ColonyManager {
    fn drop(&mut self) {
        if self.initialized {
            // Attempt to save the key store on drop
            if let Err(e) = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(self.save_key_store())
            }) {
                log::warn!("Failed to save colony key store on drop: {}", e);
            }
        }
    }
}
