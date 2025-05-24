use futures_util::sink::SinkExt;
use tokio::sync::mpsc;
use warp::ws::{Message, WebSocket};

use crate::error::Error as DaemonError;
use mutant_protocol::{Response, GetDataResponse};

/// Helper function to send responses - uses binary protocol for data chunks
pub(crate) async fn send_response(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    response: Response,
) -> Result<(), DaemonError> {
    match response {
        Response::GetData(data_response) => {
            // Send binary data efficiently using a custom binary protocol
            send_binary_data_response(sender, data_response).await
        }
        _ => {
            // Send regular JSON responses
            let json = serde_json::to_string(&response).map_err(DaemonError::SerdeJson)?;
            sender
                .send(Message::text(json))
                .await
                .map_err(DaemonError::WebSocket)?;
            Ok(())
        }
    }
}

/// Send binary data using an efficient binary protocol
/// Format: [MAGIC_BYTES(4)][TASK_ID(8)][CHUNK_INDEX(8)][TOTAL_CHUNKS(8)][IS_LAST(1)][DATA_LEN(8)][DATA]
async fn send_binary_data_response(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    data_response: GetDataResponse,
) -> Result<(), DaemonError> {
    const MAGIC: &[u8; 4] = b"MTNT"; // Magic bytes to identify our binary protocol

    let mut binary_message = Vec::new();

    // Header: MAGIC(4) + TASK_ID(8) + CHUNK_INDEX(8) + TOTAL_CHUNKS(8) + IS_LAST(1) + DATA_LEN(8)
    binary_message.extend_from_slice(MAGIC);
    binary_message.extend_from_slice(&data_response.task_id.to_le_bytes());
    binary_message.extend_from_slice(&(data_response.chunk_index as u64).to_le_bytes());
    binary_message.extend_from_slice(&(data_response.total_chunks as u64).to_le_bytes());
    binary_message.push(if data_response.is_last { 1 } else { 0 });
    binary_message.extend_from_slice(&(data_response.data.len() as u64).to_le_bytes());

    // Data payload
    binary_message.extend_from_slice(&data_response.data);

    log::info!("Sending binary data chunk: task_id={}, chunk={}/{}, data_len={}, total_msg_len={}",
               data_response.task_id, data_response.chunk_index + 1, data_response.total_chunks,
               data_response.data.len(), binary_message.len());

    // Send as binary WebSocket message
    sender
        .send(Message::binary(binary_message))
        .await
        .map_err(DaemonError::WebSocket)?;
    Ok(())
}

/// Type alias for the update channel sender
pub(crate) type UpdateSender = mpsc::UnboundedSender<Response>;
