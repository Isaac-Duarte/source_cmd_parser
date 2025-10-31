use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    /// We'll probably just ignore the timestamp from the log file
    pub time_stamp: DateTime<Utc>,
    pub user_name: String,
    pub message: String,
    pub command: String,
    pub raw_message: String,
}

impl ChatMessage {
    pub fn new(user_name: String, message: String, command: String, raw_message: String) -> Self {
        Self {
            time_stamp: Utc::now(),
            user_name,
            message,
            command,
            raw_message,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config<T> {
    /// This is the path to the file that will be monitored
    pub(crate) file_path: PathBuf,

    /// This is the timeout for commands
    pub(crate) time_out: Option<Duration>,

    /// This is the user name of the owner of the commands
    pub(crate) owner: Option<String>,

    /// This is the shared state that will be passed to the command functions
    pub(crate) shared_state: T,

    /// This is the stop flag that will be used to stop the parser
    pub(crate) stop_flag: Option<Arc<AtomicBool>>,

    /// Location where the generated cfg file will be written
    pub(crate) cfg_file_path: PathBuf,

    /// Lock to ensure only one command writes to the cfg file at a time
    pub(crate) cfg_write_lock: Arc<Mutex<()>>,

    /// The key that will trigger the exec command after writing the cfg
    pub(crate) exec_bind_key: enigo::Key,

    /// Shared enigo instance for pressing the bind key
    pub(crate) enigo: Arc<Mutex<enigo::Enigo>>,
}
