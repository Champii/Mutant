use base64::DecodeError;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

// --- Event System Definitions ---

/// Callback type used during `get` operations to report progress and allow cancellation.
///
/// The callback receives `GetEvent` variants and returns a `Future` that resolves to:
/// - `Ok(true)`: Continue the operation.
/// - `Ok(false)`: Cancel the operation (results in `Error::OperationCancelled`).
/// - `Err(e)`: Propagate an error from the callback.
pub type GetCallback = Arc<
    dyn Fn(
            GetEvent,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<bool, Box<dyn std::error::Error + Send + Sync>>>
                    + Send
                    + Sync,
            >,
        > + Send
        + Sync,
>;

/// Callback type used during initialization (`init`) operations to report progress
/// and handle interactive prompts.
///
/// The callback receives `InitProgressEvent` variants and returns a `Future` that resolves to:
/// - `Ok(Some(true))`: User confirmed action (e.g., create remote index).
/// - `Ok(Some(false))`: User denied action.
/// - `Ok(None)`: Event acknowledged, no specific user action required.
/// - `Err(e)`: Propagate an error from the callback.
pub type InitCallback = Box<
    dyn Fn(
            InitProgressEvent,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<Option<bool>, Box<dyn std::error::Error + Send + Sync>>>
                    + Send
                    + Sync,
            >,
        > + Send
        + Sync,
>;

/// Callback type used during `purge` operations to report progress and allow cancellation.
///
/// The callback receives `PurgeEvent` variants and returns a `Future` that resolves to:
/// - `Ok(true)`: Continue the operation.
/// - `Ok(false)`: Cancel the operation (results in `Error::OperationCancelled`).
/// - `Err(e)`: Propagate an error from the callback.
pub type PurgeCallback = Arc<
    dyn Fn(
            PurgeEvent,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<bool, Box<dyn std::error::Error + Send + Sync>>>
                    + Send
                    + Sync,
            >,
        > + Send
        + Sync,
>;

/// Callback type used during `sync` operations to report progress and allow cancellation.
///
/// The callback receives `SyncEvent` variants and returns a `Future` that resolves to:
/// - `Ok(true)`: Continue the operation.
/// - `Ok(false)`: Cancel the operation (results in `Error::OperationCancelled`).
/// - `Err(e)`: Propagate an error from the callback.
pub type SyncCallback = Arc<
    dyn Fn(
            SyncEvent,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<bool, Box<dyn std::error::Error + Send + Sync>>>
                    + Send
                    + Sync,
            >,
        > + Send
        + Sync,
>;

/// Callback type used during `health_check` operations to report progress and allow cancellation.
///
/// The callback receives `HealthCheckEvent` variants and returns a `Future` that resolves to:
/// - `Ok(true)`: Continue the operation.
/// - `Ok(false)`: Cancel the operation (results in `Error::OperationCancelled`).
/// - `Err(e)`: Propagate an error from the callback.
pub type HealthCheckCallback = Arc<
    dyn Fn(
            HealthCheckEvent,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<bool, Box<dyn std::error::Error + Send + Sync>>>
                    + Send
                    + Sync,
            >,
        > + Send
        + Sync,
>;

/// Events emitted during a `get` operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GetEvent {
    /// Indicates the start of the `get` operation.
    Starting {
        /// Total number of pads (chunks) to be fetched.
        total_chunks: usize,
    },
    /// Indicates that a single pad (chunk) has been fetched.
    PadFetched,
    /// Contains data from a fetched pad (chunk).
    /// Only sent when streaming is enabled.
    PadData {
        /// The index of the chunk in the overall file.
        chunk_index: usize,
        /// The actual data from the pad.
        data: Vec<u8>,
    },
    /// Indicates that the `get` operation has completed successfully.
    Complete,
}

/// Events emitted during an `init` (initialization) operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InitProgressEvent {
    /// Indicates the start of the initialization process.
    Starting {
        /// An estimated total number of steps for the initialization.
        total_steps: u64,
    },

    /// Reports progress on a specific step during initialization.
    Step {
        /// The current step number.
        step: u64,
        /// A message describing the current step.
        message: String,
    },

    /// Indicates that user confirmation is required to create a remote index.
    /// The `InitCallback` should return `Ok(Some(true))` to proceed or `Ok(Some(false))` to skip.
    PromptCreateRemoteIndex,

    /// Indicates that the initialization process has failed.
    Failed {
        /// A message describing the failure.
        error_msg: String,
    },

    /// Indicates that the initialization process has completed successfully.
    Complete {
        /// A final message summarizing the outcome.
        message: String,
    },
}

/// Events emitted during a `purge` operation (storage cleanup/verification).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PurgeEvent {
    /// Indicates the start of the `purge` operation.
    Starting {
        /// Total number of pads to be processed.
        total_count: usize,
    },

    /// Indicates that a single pad has been processed (verified or marked for cleanup).
    PadProcessed,

    /// Indicates that the `purge` operation has completed.
    Complete {
        /// Number of pads successfully verified.
        verified_count: usize,
        /// Number of pads that failed verification or encountered errors.
        failed_count: usize,
    },
}

/// Events emitted during a `sync` operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncEvent {
    /// Indicates the remote index is being fetched.
    FetchingRemoteIndex,

    /// Indicates that the remote index is being merged with the local index.
    Merging,

    /// Indicates that the remote index is being pushed to the network.
    PushingRemoteIndex,

    /// Indicates that the remote index is being Verified.
    VerifyingRemoteIndex,

    /// Indicates that the `sync` operation has completed successfully.
    Complete,
}

/// Events emitted during a `health_check` operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthCheckEvent {
    /// Indicates the start of the `health_check` operation.
    Starting {
        /// Total number of keys to be checked.
        total_keys: usize,
    },

    /// Indicates that a key has been processed
    KeyProcessed,

    /// Indicates that the `health_check` operation has completed successfully.
    Complete {
        /// Number of keys marked for reupload
        nb_keys_updated: usize,
    },
}

// --- Task Management System Definitions ---

pub type TaskId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskType {
    Put,
    Get,
    Sync,
    Purge,
    HealthCheck,
    Rm,
    Mv,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Stopped,
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PutEvent {
    Starting {
        total_chunks: usize,
        initial_written_count: usize,
        initial_confirmed_count: usize,
        chunks_to_reserve: usize,
    },
    PadReserved,
    PadsWritten,
    PadsConfirmed,
    Complete,
}

pub type PutCallback = Arc<
    dyn Fn(
            PutEvent,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<bool, Box<dyn std::error::Error + Send + Sync>>>
                    + Send
                    + Sync,
            >,
        > + Send
        + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskProgress {
    Put(PutEvent),
    Get(GetEvent),
    Sync(SyncEvent),
    Purge(PurgeEvent),
    HealthCheck(HealthCheckEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskResult {
    Pending,
    Error(String),
    Result(TaskResultType),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskResultType {
    Put(PutResult),
    Get(GetResult),
    Sync(SyncResult),
    Purge(PurgeResult),
    HealthCheck(HealthCheckResult),
}

/// Represents the final result of a successful `put` operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PutResult {
    /// The public address of the key, if it's a public key
    pub public_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub progress: Option<TaskProgress>,
    pub result: TaskResult,
    pub key: Option<String>, // The key this task is operating on, if any
}

// --- Protocol Definitions (Requests & Responses) ---

// --- Incoming Requests ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PutSource {
    /// Path to a file on the daemon's filesystem
    FilePath(String),
    /// Direct byte data to upload
    Bytes(Vec<u8>),
    /// Streaming data - data will be sent in chunks via PutData requests
    Stream { total_size: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PutRequest {
    pub user_key: String,
    pub source: PutSource,
    /// Original filename (used for display purposes)
    pub filename: Option<String>,
    pub mode: StorageMode,
    pub public: bool,
    pub no_verify: bool,
}

/// Request containing a chunk of data for a streaming put operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PutDataRequest {
    /// The task ID associated with this data chunk.
    pub task_id: TaskId,
    /// The index of this chunk in the overall file.
    pub chunk_index: usize,
    /// The total number of chunks in the file.
    pub total_chunks: usize,
    /// The actual data for this chunk.
    pub data: Vec<u8>,
    /// Whether this is the last chunk of the file.
    pub is_last: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetRequest {
    pub user_key: String,
    pub destination_path: Option<String>, // Optional path where the fetched file should be saved on the daemon
    pub public: bool,
    pub stream_data: bool, // Whether to stream data back to the client
}

#[derive(Deserialize, Debug, PartialEq, Eq, Serialize, Clone)]
pub struct QueryTaskRequest {
    pub task_id: Uuid,
}

#[derive(Deserialize, Debug, PartialEq, Eq, Serialize, Clone)]
pub struct ListTasksRequest;

#[derive(Deserialize, Debug, PartialEq, Eq, Serialize, Clone)]
pub struct StopTaskRequest {
    pub task_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RmRequest {
    pub user_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MvRequest {
    pub old_key: String,
    pub new_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListKeysRequest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PurgeRequest {
    pub aggressive: bool,
}



#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Request {
    Put(PutRequest),
    PutData(PutDataRequest),
    Get(GetRequest),
    QueryTask(QueryTaskRequest),
    ListTasks(ListTasksRequest),
    StopTask(StopTaskRequest),
    Rm(RmRequest),
    Mv(MvRequest),
    ListKeys(ListKeysRequest),
    Stats(StatsRequest),
    Sync(SyncRequest),
    Purge(PurgeRequest),
    Import(ImportRequest),
    Export(ExportRequest),
    HealthCheck(HealthCheckRequest),
    // Colony integration requests
    Search(SearchRequest),
    IndexContent(IndexContentRequest),
    GetMetadata(GetMetadataRequest),
    AddContact(AddContactRequest),
    ListContent(ListContentRequest),
    SyncContacts(SyncContactsRequest),
}

// --- Outgoing Responses ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskCreatedResponse {
    pub task_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskUpdateResponse {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub progress: Option<TaskProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskResultResponse {
    pub task_id: Uuid,
    pub status: TaskStatus,
    pub result: TaskResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoppedResponse {
    pub task_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListEntry {
    pub task_id: Uuid,
    pub task_type: TaskType,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskListEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorResponse {
    pub error: String,
    pub original_request: Option<String>, // Optional original request string for context
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RmSuccessResponse {
    pub user_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MvSuccessResponse {
    pub old_key: String,
    pub new_key: String,
}

/// Detailed information about a single stored key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyDetails {
    pub key: String,
    pub total_size: usize,
    pub pad_count: usize,
    pub confirmed_pads: usize,
    pub is_public: bool,
    pub public_address: Option<String>, // hex representation
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListKeysResponse {
    pub keys: Vec<KeyDetails>,
}

// Add these structs
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StatsRequest {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StatsResponse {
    pub total_keys: u64,
    pub total_pads: u64,
    pub occupied_pads: u64,
    pub free_pads: u64,
    pub pending_verify_pads: u64,
}
// End of added structs

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SyncRequest {
    pub push_force: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SyncResponse {
    pub result: SyncResult,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SyncResult {
    pub nb_keys_added: usize,
    pub nb_keys_updated: usize,
    pub nb_free_pads_added: usize,
    pub nb_pending_pads_added: usize,
}

/// Represents the final result of a successful `get` operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetResult {
    /// Total size of the retrieved data in bytes.
    pub size: usize,
    /// Indicates if data was streamed back to the client.
    pub streamed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PurgeResponse {
    pub result: PurgeResult,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PurgeResult {
    pub nb_pads_purged: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ImportRequest {
    pub file_path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ImportResponse {
    pub result: ImportResult,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ImportResult {
    pub nb_keys_imported: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub destination_path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExportResponse {
    pub result: ExportResult,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub nb_keys_exported: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HealthCheckRequest {
    pub key_name: String,
    pub recycle: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HealthCheckResponse {
    pub result: HealthCheckResult,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HealthCheckResult {
    pub nb_keys_reset: usize,
    pub nb_keys_recycled: usize,
}

// Colony integration request types
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub query: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct IndexContentRequest {
    pub user_key: String,
    pub metadata: serde_json::Value,
    pub public_address: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct GetMetadataRequest {
    pub address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AddContactRequest {
    pub pod_address: String,
    pub contact_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ListContentRequest {
    // Empty for now - lists all content
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SyncContactsRequest {
    // Empty for now - syncs all contacts
}

// Colony integration response types
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SearchResponse {
    pub results: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct IndexContentResponse {
    pub success: bool,
    pub pod_address: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct GetMetadataResponse {
    pub metadata: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AddContactResponse {
    pub success: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ListContentResponse {
    pub content: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SyncContactsResponse {
    pub synced_count: usize,
}

/// Response containing a chunk of data from a get operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetDataResponse {
    /// The task ID associated with this data chunk.
    pub task_id: TaskId,
    /// The index of this chunk in the overall file.
    pub chunk_index: usize,
    /// The total number of chunks in the file.
    pub total_chunks: usize,
    /// The actual data for this chunk.
    pub data: Vec<u8>,
    /// Whether this is the last chunk of the file.
    pub is_last: bool,
}

/// Represents all possible responses the daemon can send to the client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Response {
    Error(ErrorResponse),
    TaskCreated(TaskCreatedResponse),
    TaskUpdate(TaskUpdateResponse),
    TaskResult(TaskResultResponse),
    TaskStopped(TaskStoppedResponse),
    TaskList(TaskListResponse),
    RmSuccess(RmSuccessResponse),
    MvSuccess(MvSuccessResponse),
    ListKeys(ListKeysResponse),
    Stats(StatsResponse),
    Import(ImportResponse),
    Export(ExportResponse),
    /// A chunk of data from a get operation.
    GetData(GetDataResponse),
    // Colony integration responses
    Search(SearchResponse),
    IndexContent(IndexContentResponse),
    GetMetadata(GetMetadataResponse),
    AddContact(AddContactResponse),
    ListContent(ListContentResponse),
    SyncContacts(SyncContactsResponse),
}

// Helper moved to where Response is used (client/server)
// // Helper to create an ErrorResponse
// impl Response {
//     pub fn error(msg: String, original_request: Option<String>) -> Self {
//         Response::Error(ErrorResponse {
//             error: msg,
//             original_request,
//         })
//     }
// }

// --- Protocol Error Definition ---

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // Separate deserialization for potentially better client-side error handling
    #[error("JSON deserialization error: {0}")]
    Deserialization(serde_json::Error),

    #[error("Base64 decoding error: {0}")]
    Base64Decode(#[from] DecodeError),

    #[error("Task not found: {0}")]
    TaskNotFound(Uuid),

    #[error("Invalid request format: {0}")]
    InvalidRequest(String),

    #[error("Internal server error: {0}")] // Generic for server-side issues
    InternalError(String),

    #[error("WebSocket error: {0}")] // Can be used by both client/server
    WebSocket(String),
}

pub const LIGHTEST_SCRATCHPAD_SIZE: usize = 512 * 1024;
pub const LIGHT_SCRATCHPAD_SIZE: usize = 1 * 1024 * 1024;
pub const MEDIUM_SCRATCHPAD_SIZE: usize = 2 * 1024 * 1024;
pub const HEAVY_SCRATCHPAD_SIZE: usize = 3 * 1024 * 1024;
pub const HEAVIEST_SCRATCHPAD_SIZE: usize = (4 * 1024 * 1024) - 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageMode {
    /// 0.5 MB per scratchpad
    Lightest,
    /// 1 MB per scratchpad
    Light,
    /// 2 MB per scratchpad
    Medium,
    /// 3 MB per scratchpad
    Heavy,
    /// 4 MB per scratchpad
    Heaviest,
}

impl StorageMode {
    pub fn scratchpad_size(&self) -> usize {
        match self {
            StorageMode::Lightest => LIGHTEST_SCRATCHPAD_SIZE,
            StorageMode::Light => LIGHT_SCRATCHPAD_SIZE,
            StorageMode::Medium => MEDIUM_SCRATCHPAD_SIZE,
            StorageMode::Heavy => HEAVY_SCRATCHPAD_SIZE,
            StorageMode::Heaviest => HEAVIEST_SCRATCHPAD_SIZE,
        }
    }
}
