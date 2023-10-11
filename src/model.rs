use std::time::Duration;

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
