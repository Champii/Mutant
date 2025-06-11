use std::sync::Arc;

use futures_util::{
    sink::SinkExt,
    stream::StreamExt,
};
use tokio::sync::mpsc;
use warp::ws::WebSocket;
use mutant_lib::MutAnt;
use mutant_protocol::{ErrorResponse, Request, Response};

use super::common::send_response;
use super::dispatcher::handle_request;
use super::{TaskMap, ActiveKeysMap};

pub async fn handle_ws(ws: WebSocket, mutant: Arc<MutAnt>, tasks: TaskMap, active_keys: ActiveKeysMap) {
    let (mut ws_sender, mut ws_receiver) = ws.split();
    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<Response>();

    log::info!("WebSocket client connected");

    // Task to listen for updates from spawned tasks and send them to the client
    let update_forwarder = tokio::spawn(async move {
        while let Some(response) = update_rx.recv().await {
            if let Err(e) = send_response(&mut ws_sender, response).await {
                log::error!("Failed to send task update via WebSocket: {}", e);
                // If sending fails, the client might be disconnected, so we stop.
                break;
            }
        }
        // Ensure the sender is closed if the loop exits
        let _ = ws_sender.close().await;
        log::debug!("Update forwarder task finished.");
    });

    while let Some(result) = ws_receiver.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                // Improved error logging for WebSocket receive errors
                if e.to_string().contains("Space limit exceeded") || e.to_string().contains("too long") {
                    log::error!("WebSocket receive error: {}. This indicates the message size exceeds the configured limit.", e);

                    // Extract the message size from the error if possible
                    let size_info = if let Some(size_start) = e.to_string().find("Message too long: ") {
                        let size_start = size_start + "Message too long: ".len();
                        if let Some(size_end) = e.to_string()[size_start..].find(" >") {
                            let size = &e.to_string()[size_start..size_start+size_end];
                            format!("Your message was {} bytes. ", size)
                        } else {
                            "".to_string()
                        }
                    } else {
                        "".to_string()
                    };

                    // Try to send an error response before disconnecting
                    let _ = update_tx.send(Response::Error(ErrorResponse {
                        error: format!(
                            "Message size limit exceeded. {}Please try uploading a smaller file or contact the administrator to increase the limit. Error: {}",
                            size_info, e
                        ),
                        original_request: None,
                    }));
                } else {
                    log::trace!("WebSocket receive error: {}", e);
                }
                // Don't need to send error here, forwarder handles closure
                break;
            }
        };

        if msg.is_close() {
            log::info!("WebSocket client disconnected explicitly");
            break;
        }

        if let Ok(text) = msg.to_str() {
            let text_len = text.len();
            if text_len > 1024 * 1024 { // Log large messages (>1MB)
                log::info!("Received large text message: {} bytes", text_len);
            }

            let original_request = text.to_string(); // Keep for error reporting
            match serde_json::from_str::<Request>(text) {
                Ok(request) => {
                    if let Err(e) = handle_request(
                        request,
                        original_request.as_str(),
                        update_tx.clone(), // Pass the update channel sender
                        mutant.clone(),
                        tasks.clone(),
                        active_keys.clone(),
                    )
                    .await
                    {
                        log::error!("Error handling request: {}", e);
                        let _ = update_tx.send(Response::Error(ErrorResponse {
                            error: e.to_string(),
                            original_request: Some(original_request),
                        }));
                    }
                }
                Err(e) => {
                    log::warn!("Failed to deserialize request: {}", e);
                    let _ = update_tx.send(Response::Error(ErrorResponse {
                        error: format!("Invalid JSON request: {}", e),
                        original_request: Some(original_request),
                    }));
                }
            }
        } else if msg.is_binary() {
            log::warn!("Received binary message of size {} bytes, ignoring.", msg.as_bytes().len());
            let _ = update_tx.send(Response::Error(ErrorResponse {
                error: "Binary messages are not supported".to_string(),
                original_request: None,
            }));
        } else if msg.is_ping() {
            log::trace!("Received Ping");
        } else if msg.is_pong() {
            log::trace!("Received Pong");
        }
    }

    // Ensure the forwarder task is cleaned up when the receive loop ends
    update_forwarder.abort();
    log::debug!("WebSocket connection handler finished.");
}
