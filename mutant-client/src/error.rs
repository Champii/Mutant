use mutant_protocol::Response;
pub use thiserror::Error;
use wasm_bindgen::JsValue;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("WebSocket connection failed: {0}")]
    ConnectionError(String),

    #[error("WebSocket communication error: {0}")]
    WebSocketError(String),

    #[error("URL parsing error: {0}")]
    UrlParseError(#[from] url::ParseError),

    #[error("JSON serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("JSON deserialization error: {0}")]
    DeserializationError(serde_json::Error),

    #[error("Received message is not valid text: {0}")]
    NonTextMessageReceived(String),

    #[error("Received unexpected response from server: {0:?}")]
    UnexpectedResponse(Response),

    #[error("Server returned an error: {0}")]
    ServerError(String),

    #[error("Task ID not found in response when expected")]
    TaskIdMissing,

    #[error("Client is not connected or connection closed")]
    NotConnected,

    #[error("Failed to interact with JavaScript API: {0}")]
    JsError(String),

    #[error("Internal client error: {0}")]
    InternalError(String),

    #[error("Task with ID {0} not found")]
    TaskNotFound(mutant_protocol::TaskId),

    #[error("Channel closed unexpectedly")]
    ChannelClosed,

    #[error("Daemon returned an error: {0}")]
    DaemonError(String),

    #[error("Multipart upload failed: {0}")]
    MultipartError(String),
}

// Helper to convert JsValue to ClientError
impl From<JsValue> for ClientError {
    fn from(value: JsValue) -> Self {
        ClientError::JsError(format!("{:?}", value))
    }
}
