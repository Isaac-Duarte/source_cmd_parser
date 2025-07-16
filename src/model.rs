use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use chrono::{DateTime, Utc};

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

#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub message: String,
    pub delay_on_enter: Option<Duration>,
}

impl ChatResponse {
    pub fn new(message: String) -> Self {
        Self {
            message,
            delay_on_enter: None,
        }
    }

    pub fn with_delay(message: String, delay_on_enter: Duration) -> Self {
        Self {
            message,
            delay_on_enter: Some(delay_on_enter),
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

    /// This is the max length of a chat message that will be sent in one chunk
    pub(crate) max_entry_length: usize,

    /// This is the delay between each chunk of a long message, or when
    /// the message is sent by the owner
    pub(crate) chat_delay: Duration,

    /// This is the stop flag that will be used to stop the parser
    pub(crate) stop_flag: Option<Arc<AtomicBool>>,

    /// This is the key that will be used to send chat messages
    pub(crate) chat_key: enigo::Key,
}
