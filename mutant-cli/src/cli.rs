use clap::Parser;
use clap::ValueEnum;
use mutant_protocol::StorageMode;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    #[command(about = "Store a value associated with a key")]
    Put {
        key: String,
        file: String,
        #[arg(short, long)]
        public: bool,
        #[arg(value_enum, short, long, default_value_t = StorageModeCli::Heaviest)]
        mode: StorageModeCli,
        #[arg(short, long)]
        background: bool,
        #[arg(short, long)]
        no_verify: bool,
    },
    #[command(about = "Retrieve a value associated with a key")]
    Get {
        key: String,
        destination_path: String,
        #[arg(short, long)]
        background: bool,
        #[arg(short, long)]
        public: bool,
    },
    #[command(about = "Remove a key-value pair")]
    Rm { key: String },
    #[command(about = "List stored keys")]
    Ls {
        #[arg(long, help = "Show fetch history")]
        history: bool,
    },
    #[command(about = "Show storage statistics")]
    Stats,
    #[command(about = "Manage background tasks")]
    Tasks {
        #[command(subcommand)]
        command: TasksCommands,
    },
    #[command(about = "Manage the daemon")]
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    #[command(about = "Synchronize local index cache with remote storage")]
    Sync {
        #[arg(short, long)]
        push_force: bool,
        #[arg(short, long)]
        background: bool,
    },
    #[command(
        about = "Perform a get check on scratchpads that should have been created but failed at some point. Removes the pads that are not found."
    )]
    Purge {
        #[arg(short, long)]
        aggressive: bool,
        #[arg(short, long)]
        background: bool,
    },
    #[command(about = "Import scratchpad private key from a file")]
    Import { file_path: String },
    #[command(about = "Export all scratchpad private key to a file")]
    Export { destination_path: String },
    #[command(
        about = "Perform a health check on scratchpads that should have been created but cannot be retrieved. Recycles the pads that are not found."
    )]
    HealthCheck {
        key_name: String,
        #[arg(short, long)]
        background: bool,
        #[arg(short, long)]
        recycle: bool,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum StorageModeCli {
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

impl From<StorageModeCli> for StorageMode {
    fn from(mode: StorageModeCli) -> Self {
        match mode {
            StorageModeCli::Lightest => StorageMode::Lightest,
            StorageModeCli::Light => StorageMode::Light,
            StorageModeCli::Medium => StorageMode::Medium,
            StorageModeCli::Heavy => StorageMode::Heavy,
            StorageModeCli::Heaviest => StorageMode::Heaviest,
        }
    }
}

#[derive(clap::Subcommand)]
pub enum TasksCommands {
    #[command(about = "List all background tasks")]
    List,
    #[command(about = "Get the status of a background task")]
    Get { task_id: String },
    #[command(about = "Stop a background task")]
    Stop { task_id: String },
}

#[derive(clap::Subcommand)]
pub enum DaemonCommands {
    #[command(about = "Start the daemon")]
    Start,
    #[command(about = "Stop the daemon")]
    Stop,
    #[command(about = "Restart the daemon")]
    Restart,
    #[command(about = "Get the status of the daemon")]
    Status,
    #[command(about = "Get the logs of the daemon")]
    Logs,
}
