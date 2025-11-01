use std::{
    collections::HashMap,
    fs,
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
///     .cfg_file_path("cfg/scp.cfg")
///     .exec_bind_key(enigo::Key::Layout('p'))
///     .chat_delay(Duration::from_millis(500))
///     .add_command("!hello", |msg, state, mut kb| async move {
///         kb.simulate("Hello there!".to_string()).await?;
///         Ok(())
///     })
///     .build()?;
/// ```
pub struct SourceCmdBuilder<T, E> {
    file_path: Option<PathBuf>,
    cfg_file_path: Option<PathBuf>,
    time_out: Option<Duration>,
    commands: HashMap<String, Vec<Arc<dyn SourceCmdFn<T, E>>>>,
    owner: Option<String>,
    state: Option<T>,
    parse_log: Option<Box<dyn ParseLog>>,
    stop_flag: Option<Arc<AtomicBool>>,
    exec_bind_key: enigo::Key,
    chat_delay: Duration,
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
            cfg_file_path: None,
            time_out: None,
            commands: HashMap::new(),
            owner: None,
            state: None,
            parse_log: None,
            stop_flag: None,
            exec_bind_key: enigo::Key::Layout('p'), // Default to 'p' key
            chat_delay: Duration::from_millis(500), // Default 500ms delay
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

    /// Sets the cfg file that will receive generated chat commands.
    pub fn cfg_file_path<P: AsRef<Path>>(mut self, cfg_file_path: P) -> Self {
        self.cfg_file_path = Some(cfg_file_path.as_ref().to_path_buf());
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

    /// Sets the stop flag for graceful shutdown.
    ///
    /// # Arguments
    /// * `stop_flag` - Atomic boolean that can be set to stop the parser
    pub fn stop_flag(mut self, stop_flag: Arc<AtomicBool>) -> Self {
        self.stop_flag = Some(stop_flag);
        self
    }

    /// Sets the key that will be pressed to execute the cfg file.
    ///
    /// # Arguments
    /// * `exec_bind_key` - The key bound to `exec scp.cfg` in the game
    pub fn exec_bind_key(mut self, exec_bind_key: enigo::Key) -> Self {
        self.exec_bind_key = exec_bind_key;
        self
    }

    /// Sets the delay for owner messages before executing commands.
    ///
    /// # Arguments
    /// * `chat_delay` - Time to wait before executing owner commands
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
    /// Returns an error if required fields (file_path, cfg_file_path, state, parse_log) are missing
    pub fn build(self) -> SourceCmdResult<SourceCmdLogParser<T, E>> {
        if let (Some(file_path), Some(cfg_file_path), Some(state), Some(parse_log)) = (
            self.file_path,
            self.cfg_file_path,
            self.state,
            self.parse_log,
        ) {
            #[cfg(target_os = "linux")]
            let (watcher, rx) = SourceCmdLogParser::<T, E>::async_watcher()?;

            if let Some(parent) = cfg_file_path.parent() {
                fs::create_dir_all(parent)?;
            } else {
                return Err(SourceCmdError::UnableToGetParentDirectory(
                    cfg_file_path.clone(),
                ));
            }

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
                stop_flag: self.stop_flag,
                cfg_file_path: cfg_file_path.clone(),
                cfg_write_lock: Arc::new(Mutex::new(())),
                exec_bind_key: self.exec_bind_key,
                enigo,
                chat_delay: self.chat_delay,
            };

            Ok(SourceCmdLogParser::new(
                self.commands,
                #[cfg(target_os = "linux")]
                watcher,
                #[cfg(target_os = "linux")]
                rx,
                config,
                parse_log,
                #[cfg(target_os = "windows")]
                time::interval(Duration::from_millis(100)),
            ))
        } else {
            Err(SourceCmdError::MissingFieldS(
                "file_path, cfg_file_path, state, parse_log".to_string(),
            ))
        }
    }
}
