use log::debug;
use regex::Regex;

use crate::{log_parser::ParseLog, model::ChatMessage};

const CSS_REGEX_STRING: &str =
    r"^(\d{2}\/\d{2}\/\d{4} - \d{2}:\d{2}:\d{2}): (\*DEAD\* )?([^|]+) :\s+(.+)$";
const CS2_REGEX_STRING: &str =
    r"^(\d{2}\/\d{2} \d{2}:\d{2}:\d{2})  (\[ALL\])? ([^\]]+)(?: \[DEAD\])?: (.+)$";

pub struct Cs2LogParser {
    regex: Regex,
}

impl Default for Cs2LogParser {
    fn default() -> Self {
        Self {
            regex: Regex::new(CS2_REGEX_STRING).unwrap(),
        }
    }
}

impl Cs2LogParser {
    pub fn new() -> Self {
        Self::default()
    }
}

pub struct CSSLogParser {
    regex: Regex,
}

impl Default for CSSLogParser {
    fn default() -> Self {
        Self {
            regex: Regex::new(CSS_REGEX_STRING).unwrap(),
        }
    }
}

impl CSSLogParser {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Default implementaion for parsing the logs for Counter-Strike: Source with con_timestamp 1.
impl ParseLog for CSSLogParser {
    fn parse_command(&self, raw_message: &str) -> Option<ChatMessage> {
        parse_using_regex(&self.regex, raw_message)
    }
}

/// Default implementaion for parsing the logs for Counter-Strike: 2 with -condebug enabled.
impl ParseLog for Cs2LogParser {
    fn parse_command(&self, raw_message: &str) -> Option<ChatMessage> {
        parse_using_regex(&self.regex, raw_message)
    }
}

/// Parses a raw message string into a `ChatMessage` struct.
///
/// # Parameters
/// - `raw_message`: The raw string message to be parsed.
///
/// # Returns
/// An optional `ChatMessage` containing the parsed message details.
/// Returns `None` if the parsing fails.
fn parse_using_regex(regex: &Regex, raw_message: &str) -> Option<ChatMessage> {
    let raw_message = raw_message.trim().to_string();

    if let Some(captures) = regex.captures(&raw_message) {
        let user_name = captures.get(3).unwrap().as_str().to_string();
        let message = captures.get(4).unwrap().as_str().to_string();
        let command = message.split_whitespace().next().unwrap().to_string();
        let raw_message = message.clone();

        let message = if message.starts_with(command.as_str()) {
            message[command.len()..].trim().to_string()
        } else {
            message
        };

        Some(ChatMessage::new(user_name, message, command, raw_message))
    } else {
        debug!("Failed to parse message: {}", raw_message);
        None
    }
}
