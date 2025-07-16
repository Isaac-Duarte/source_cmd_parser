use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use tokio::sync::Mutex;

use crate::{
    error::{SourceCmdError, SourceCmdResult},
    log_parser::{ParseLog, SourceCmdFn, SourceCmdLogParser},
    model::Config,
};

/// Builder for creating and configuring a `SourceCmdLogParser`.
///
/// This builder provides a fluent interface for setting up all the necessary
/// components of a source command parser, including file paths, commands,
/// timeouts, and other configuration options.
///
/// # Example
///
/// ```rust
/// use source_cmd_parser::SourceCmdLogParser;
/// use std::time::Duration;
///
/// let parser = SourceCmdLogParser::builder()
///     .file_path(Box::new("game.log"))
///     .time_out(Duration::from_secs(5))
///     .add_command("!hello", |msg, state, mut kb| async move {
///         kb.simulate("Hello there!".to_string()).await?;
///         Ok(())
///     })
///     .build()?;
/// ```
pub struct SourceCmdBuilder<T, E> {
    file_path: Option<PathBuf>,
    time_out: Option<Duration>,
    commands: HashMap<String, Vec<Arc<dyn SourceCmdFn<T, E>>>>,
    owner: Option<String>,
    state: Option<T>,
    parse_log: Option<Box<dyn ParseLog>>,
    max_entry_length: usize,
    chat_delay: Duration,
    stop_flag: Option<Arc<AtomicBool>>,
    chat_key: enigo::Key,
}

impl<T: Clone + Send + Sync + 'static, E: std::error::Error + Send + Sync + 'static> Default
    for SourceCmdBuilder<T, E>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Send + Sync + 'static, E: std::error::Error + Send + Sync + 'static>
    SourceCmdBuilder<T, E>
{
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            file_path: None,
            time_out: None,
            commands: HashMap::new(),
            owner: None,
            state: None,
            parse_log: None,
            max_entry_length: 128,
            chat_delay: Duration::from_millis(600),
            stop_flag: None,
            chat_key: enigo::Key::Layout('y'),
        }
    }

    /// Sets the file path to monitor for log changes.
    ///
    /// # Arguments
    /// * `file_path` - Path to the log file to monitor
    pub fn file_path(mut self, file_path: Box<dyn AsRef<Path>>) -> Self {
        let path_buf = file_path.as_ref().as_ref();
        self.file_path = Some(path_buf.to_path_buf());
        self
    }

    /// Sets the timeout for command execution.
    ///
    /// # Arguments
    /// * `time_out` - Maximum duration a command can run before being cancelled
    pub fn time_out(mut self, time_out: Duration) -> Self {
        self.time_out = Some(time_out);
        self
    }

    /// Adds a command handler for a specific command string.
    ///
    /// # Arguments
    /// * `command` - The command string to match (e.g., "!hello")
    /// * `function` - The function to execute when the command is received
    pub fn add_command<F: SourceCmdFn<T, E> + 'static>(
        mut self,
        command: &str,
        function: F,
    ) -> Self {
        self.commands
            .entry(command.to_string())
            .or_default()
            .push(Arc::new(function));
        self
    }

    /// Adds a global command handler that executes for all messages.
    ///
    /// # Arguments
    /// * `function` - The function to execute for every message
    pub fn add_global_command<F: SourceCmdFn<T, E> + 'static>(mut self, function: F) -> Self {
        self.commands
            .entry("".to_string())
            .or_default()
            .push(Arc::new(function));
        self
    }

    /// Sets the owner username for privileged commands.
    ///
    /// # Arguments
    /// * `owner` - Username that should have special privileges
    pub fn owner(mut self, owner: &str) -> Self {
        self.owner = Some(owner.to_string());
        self
    }

    /// Sets the shared state that will be passed to command functions.
    ///
    /// # Arguments
    /// * `state` - The shared state object
    pub fn state(mut self, state: T) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the log parser implementation.
    ///
    /// # Arguments
    /// * `parse_log` - The parser that will extract chat messages from log lines
    pub fn set_parser(mut self, parse_log: Box<dyn ParseLog>) -> Self {
        self.parse_log = Some(parse_log);
        self
    }

    /// Sets the maximum length of a single chat message.
    ///
    /// Messages longer than this will be split into multiple chunks.
    ///
    /// # Arguments
    /// * `max_entry_length` - Maximum characters per message chunk
    pub fn max_entry_length(mut self, max_entry_length: usize) -> Self {
        self.max_entry_length = max_entry_length;
        self
    }

    /// Sets the stop flag for graceful shutdown.
    ///
    /// # Arguments
    /// * `stop_flag` - Atomic boolean that can be set to stop the parser
    pub fn stop_flag(mut self, stop_flag: Arc<AtomicBool>) -> Self {
        self.stop_flag = Some(stop_flag);
        self
    }

    /// Sets the key used to open the chat window.
    ///
    /// # Arguments
    /// * `chat_key` - The key that opens the chat input (e.g., 'y' for most Source games)
    pub fn chat_key(mut self, chat_key: enigo::Key) -> Self {
        self.chat_key = chat_key;
        self
    }

    /// Sets the delay between message chunks and owner message delays.
    ///
    /// # Arguments
    /// * `chat_delay` - Time to wait between message chunks
    pub fn chat_delay(mut self, chat_delay: Duration) -> Self {
        self.chat_delay = chat_delay;
        self
    }

    /// Builds the `SourceCmdLogParser` with the configured options.
    ///
    /// # Returns
    /// A configured `SourceCmdLogParser` ready to run
    ///
    /// # Errors
    /// Returns an error if required fields (file_path, state, parse_log) are missing
    pub fn build(self) -> SourceCmdResult<SourceCmdLogParser<T, E>> {
        if let (Some(file_path), Some(state), Some(parse_log)) =
            (self.file_path, self.state, self.parse_log)
        {
            #[cfg(target_os = "linux")]
            let (watcher, rx) = SourceCmdLogParser::<T, E>::async_watcher()?;

            let enigo = {
                let mut enigo = enigo::Enigo::new();
                #[cfg(target_os = "linux")]
                enigo.set_delay(0);
                Arc::new(Mutex::new(enigo))
            };

            let config = Config {
                file_path,
                time_out: self.time_out,
                owner: self.owner,
                shared_state: state,
                max_entry_length: self.max_entry_length,
                chat_delay: self.chat_delay,
                stop_flag: self.stop_flag,
                chat_key: self.chat_key,
            };

            Ok(SourceCmdLogParser::new(
                self.commands,
                #[cfg(target_os = "linux")]
                watcher,
                #[cfg(target_os = "linux")]
                rx,
                enigo,
                config,
                parse_log,
                #[cfg(target_os = "windows")]
                time::interval(Duration::from_millis(100)),
            ))
        } else {
            Err(SourceCmdError::MissingFieldS(
                "file_path, state, parse_log".to_string(),
            ))
        }
    }
}
