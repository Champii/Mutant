use std::sync::Arc;
use tokio::fs;

use crate::error::Error as DaemonError;
use super::{is_public_only_mode, PUBLIC_ONLY_ERROR_MSG};
use mutant_lib::storage::PadInfo;
use mutant_lib::MutAnt;
use mutant_protocol::{
    ErrorResponse, ExportRequest, ExportResponse, ExportResult, ImportRequest, ImportResponse, ImportResult,
    Response,
};

use super::common::UpdateSender;

pub(crate) async fn handle_import(
    req: ImportRequest,
    update_tx: UpdateSender,
    mutant: Arc<MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling Import request");

    // Check if we're in public-only mode
    if is_public_only_mode() {
        return update_tx
            .send(Response::Error(ErrorResponse {
                error: PUBLIC_ONLY_ERROR_MSG.to_string(),
                original_request: Some(serde_json::to_string(&req).unwrap_or_default()),
            }))
            .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)));
    }

    let file_path = req.file_path.clone();
    let pads_hex = fs::read(&file_path)
        .await
        .map_err(|e| DaemonError::IoError(format!("Failed to read file {}: {}", file_path, e)))?;

    let pads_hex: Vec<PadInfo> = serde_json::from_slice(&pads_hex)
        .map_err(|e| DaemonError::IoError(format!("Failed to parse pads hex: {}", e)))?;

    // Call the method which returns StorageStats directly
    let stats = mutant.import_raw_pads_private_key(pads_hex).await;
    log::info!("Imported file successfully: {:?}", stats);

    let response = Response::Import(ImportResponse {
        result: ImportResult {
            nb_keys_imported: 0, // TODO
        },
    });

    update_tx
        .send(response)
        .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)))?;

    Ok(())
}

pub(crate) async fn handle_export(
    req: ExportRequest,
    update_tx: UpdateSender,
    mutant: Arc<MutAnt>,
) -> Result<(), DaemonError> {
    log::debug!("Handling Export request");

    // Check if we're in public-only mode
    if is_public_only_mode() {
        return update_tx
            .send(Response::Error(ErrorResponse {
                error: PUBLIC_ONLY_ERROR_MSG.to_string(),
                original_request: Some(serde_json::to_string(&req).unwrap_or_default()),
            }))
            .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)));
    }

    let destination_path = req.destination_path.clone();

    let pads_hex = mutant.export_raw_pads_private_key().await?;

    let pads_hex = serde_json::to_vec(&pads_hex)
        .map_err(|e| DaemonError::IoError(format!("Failed to serialize pads hex: {}", e)))?;

    fs::write(&destination_path, pads_hex).await.map_err(|e| {
        DaemonError::IoError(format!("Failed to write file {}: {}", destination_path, e))
    })?;

    let response = Response::Export(ExportResponse {
        result: ExportResult {
            nb_keys_exported: 0, // TODO
        },
    });

    update_tx
        .send(response)
        .map_err(|e| DaemonError::Internal(format!("Update channel send error: {}", e)))?;

    Ok(())
}
