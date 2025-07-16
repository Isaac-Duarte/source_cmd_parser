use std::{
    collections::HashMap,
    future::Future,
    io::{BufRead, Read, Seek},
    pin::Pin,
    sync::Arc,
};

use clipboard_rs::ClipboardContext;
use enigo::KeyboardControllable;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, Stream, StreamExt,
};
use log::{debug, error, info};
use notify::{RecommendedWatcher, Watcher};
use tokio::{
    sync::{Mutex, MutexGuard},
    time,
};

use crate::{
    builder::SourceCmdBuilder,
    error::SourceCmdResult,
    keyboard::Keyboard,
    model::{ChatMessage, ChatResponse, Config},
};

/// The `SourceCmdFn` trait provides a unified interface for functions
/// that accept a chat message and asynchronously produce a result with
/// an optional chat response.
///
/// This trait ensures thread safety by requiring the `Send`, `Sync`,
/// and `'static` lifetimes for its implementors which allows the function
/// to be async.
pub trait SourceCmdFn<T, E>: Send + Sync + 'static
where
    T: Clone + Send + Sync + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    /// Takes a `ChatMessage` and returns a future resolving to a result
    /// containing an optional chat response.
    ///
    /// # Parameters
    /// - `message`: The chat message to process.
    ///
    /// # Returns
    /// A pinned box containing a future. When this future is resolved, it yields
    /// a `SourceCmdResult` containing an optional `ChatResponse`.
    fn call(
        &self,
        message: ChatMessage,
        state: T,
        keyboard: Keyboard<T, E>,
    ) -> Pin<Box<dyn Future<Output = Result<(), E>> + Send>>;
}

/// Implementation of `SourceCmdFn` for any function type `F` that meets
/// the specified constraints.
///
/// This allows for any appropriate function to be treated as a `SourceCmdFn`
/// without needing to explicitly wrap it or implement the trait separately.
impl<F, Fut, T, E> SourceCmdFn<T, E> for F
where
    F: Fn(ChatMessage, T, Keyboard<T, E>) -> Fut + Sync + Send + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    T: Clone + Send + Sync + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    fn call(
        &self,
        message: ChatMessage,
        state: T,
        keyboard: Keyboard<T, E>,
    ) -> Pin<Box<dyn Future<Output = Result<(), E>> + Send>> {
        Box::pin(self(message, state, keyboard))
    }
}

/// The `Parse` trait provides a unified interface for users of this library
/// to implement their own chat message parsers. This is useful mainly
/// when implmenting a parser for a different game.
pub trait ParseLog {
    fn parse_command(&self, message: &str) -> Option<ChatMessage>;
}

/// `SourceCmdLogParser` is responsible for monitoring a file for changes,
/// parsing chat messages, and executing associated commands.
///
/// It holds the configuration and resources required to monitor a file for changes,
/// parse the incoming chat commands, and execute the associated actions.
pub struct SourceCmdLogParser<T, E> {
    /// This is a map of commands to their associated functions
    commands: HashMap<String, Vec<Arc<dyn SourceCmdFn<T, E>>>>,

    /// This is the file watcher that will be used to monitor the file
    #[cfg(target_os = "linux")]
    watcher: RecommendedWatcher,

    /// This is the receiver for the file watcher
    #[cfg(target_os = "linux")]
    rx: Receiver<notify::Result<notify::Event>>,

    /// This is the enigo instance that will be used to send key presses
    enigo: Arc<Mutex<enigo::Enigo>>,

    /// This is the last position in the file that was read
    last_position: u64,

    /// This is the parser that will be used to parse chat messages
    /// This isn't store din the config as I need to be able to clone it
    parse_log: Box<dyn ParseLog>,

    /// Config for the parser
    config: Config<T>,

    #[cfg(target_os = "windows")]
    /// This is the timer used to poll for file changes on windows
    timer: time::Interval,
}

impl<T, E> SourceCmdLogParser<T, E> {
    /// Creates a new SourceCmdLogParser with the given parameters
    pub fn new(
        commands: HashMap<String, Vec<Arc<dyn SourceCmdFn<T, E>>>>,
        #[cfg(target_os = "linux")] watcher: RecommendedWatcher,
        #[cfg(target_os = "linux")] rx: Receiver<notify::Result<notify::Event>>,
        enigo: Arc<Mutex<enigo::Enigo>>,
        config: Config<T>,
        parse_log: Box<dyn ParseLog>,
        #[cfg(target_os = "windows")] timer: time::Interval,
    ) -> Self {
        Self {
            commands,
            #[cfg(target_os = "linux")]
            watcher,
            #[cfg(target_os = "linux")]
            rx,
            enigo,
            last_position: 0,
            parse_log,
            config,
            #[cfg(target_os = "windows")]
            timer,
        }
    }

    pub fn builder() -> SourceCmdBuilder<T, E>
    where
        T: Clone + Send + Sync + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        SourceCmdBuilder::<T, E>::new()
    }

    /// Retrieves the commands associated with a given command string.
    ///
    /// # Parameters
    /// - `command`: The command string to look up.
    ///
    /// # Returns
    /// An optional reference to a vec of command functions.
    pub fn get_commands(&self, command: &str) -> Option<&Vec<Arc<dyn SourceCmdFn<T, E>>>> {
        self.commands.get(command)
    }

    /// Initializes and returns an asynchronous file watcher and an associated receiver.
    ///
    /// The file watcher is automatically configured with the best implementation for your platform.
    /// This method also sets up a channel with increased capacity for improved responsiveness.
    ///
    /// # Returns
    /// A result containing a tuple with the initialized file watcher and receiver.
    pub fn async_watcher(
    ) -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<notify::Event>>)> {
        // Increased channel capacity
        let (mut tx, rx) = channel(10);

        // Automatically select the best implementation for your platform.
        let watcher = RecommendedWatcher::new(
            move |res| {
                futures::executor::block_on(async {
                    tx.send(res).await.unwrap();
                })
            },
            notify::Config::default(),
        )?;

        Ok((watcher, rx))
    }

    /// Executes a given chat command asynchronously.
    ///
    /// # Parameters
    /// - `command`: The command function to be executed.
    /// - `message`: The chat message associated with the command.
    ///
    /// # Returns
    /// An asynchronous result containing an optional `ChatResponse`.
    pub async fn execute_command(
        config: &Config<T>,
        command: &dyn SourceCmdFn<T, E>,
        message: &ChatMessage,
        keyboard: Keyboard<T, E>,
    ) -> SourceCmdResult<()>
    where
        T: Clone + Sync + Send + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        // If a timeout is specified, wrap the command in a timeout future
        let response = if let Some(time_out) = config.time_out {
            let timeout = time::timeout(
                time_out,
                command.call(message.clone(), config.shared_state.clone(), keyboard),
            )
            .await;

            if let Ok(response) = timeout {
                response
            } else {
                error!("Command timed out: {}", message.raw_message);

                return Ok(());
            }
        } else {
            command
                .call(message.clone(), config.shared_state.clone(), keyboard)
                .await
        };

        match response {
            Ok(_response) => Ok(()),
            Err(err) => {
                error!(
                    "Error whil executing command {}: {:?}",
                    message.command, err
                );

                Ok(())
            }
        }
    }

    /// Runs a sequence of actions based on the provided `ChatResponse`.
    ///
    /// This method triggers the sequence of key presses based on the `chat_response` message,
    /// and, if provided, it waits for a specified delay before pressing the 'Enter' key.
    ///
    /// # Parameters
    /// - `chat_response`: The chat response containing the message sequence.
    ///
    /// # Returns
    /// A result indicating the success or failure of the operation.
    pub async fn run_sequence(
        config: &Config<T>,
        enigo: Arc<Mutex<enigo::Enigo>>,
        chat_response: ChatResponse,
        ctx: Arc<ClipboardContext>,
    ) -> SourceCmdResult<()> {
        use clipboard_rs::{Clipboard, ClipboardContext};

        // Function to send a chat message
        async fn send_message<'a>(
            enigo: &mut MutexGuard<'a, enigo::Enigo>,
            message: &str,
            chat_response: &ChatResponse,
            chat_key: enigo::Key,
            ctx: &ClipboardContext,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            // Copy message to clipboard
            #[cfg(target_os = "windows")]
            {
                ctx.set_text(message.to_string())
                    .map_err(|e| crate::error::SourceCmdError::ClipboardError(e.to_string()))?;
            }
            
            #[cfg(not(target_os = "windows"))]
            {
                ctx.set_text(message.to_string())
                    .map_err(|_| crate::error::SourceCmdError::IoError(std::io::Error::new(std::io::ErrorKind::Other, "Clipboard error")))?;
            }

            // Simulate pressing chat key (e.g., open chat window)
            enigo.key_down(chat_key);
            time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(chat_key);

            // Simulate Ctrl+V to paste the message
            enigo.key_down(enigo::Key::Control);
            enigo.key_down(enigo::Key::Layout('v'));
            tokio::time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(enigo::Key::Control);
            enigo.key_up(enigo::Key::Layout('v'));

            // Wait if there is any delay before pressing Enter
            if let Some(delay) = chat_response.delay_on_enter {
                time::sleep(delay).await;
            }

            // Simulate pressing Enter to send the message
            enigo.key_down(enigo::Key::Return);
            time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(enigo::Key::Return);
            
            Ok(())
        }

        let mut enigo = enigo.lock().await;

        let message = chat_response.message.as_str();

        if message.len() <= config.max_entry_length {
            if let Err(e) = send_message(&mut enigo, message, &chat_response, config.chat_key, &ctx).await {
                error!("Failed to send message: {}", e);
            }
            return Ok(());
        }

        let words = message.split_whitespace();
        let mut current_chunk = String::new();

        for word in words {
            if current_chunk.len() + word.len() + 1 > config.max_entry_length {
                // +1 for space
                if let Err(e) = send_message(
                    &mut enigo,
                    &current_chunk,
                    &chat_response,
                    config.chat_key,
                    &ctx,
                )
                .await {
                    error!("Failed to send message chunk: {}", e);
                }

                time::sleep(config.chat_delay).await;
                current_chunk.clear();
            }

            if !current_chunk.is_empty() {
                current_chunk.push(' ');
            }
            current_chunk.push_str(word);
        }

        // Send any remaining chunk
        if !current_chunk.is_empty() {
            if let Err(e) = send_message(
                &mut enigo,
                &current_chunk,
                &chat_response,
                config.chat_key,
                &ctx,
            )
            .await {
                error!("Failed to send final message chunk: {}", e);
            }

            time::sleep(config.chat_delay).await;
        }

        Ok(())
    }

    /// Starts monitoring the file for changes, parses chat messages,
    /// and executes the associated commands.
    ///
    /// # Returns
    /// A result indicating the success or failure of the operation.
    pub async fn run(&mut self) -> SourceCmdResult<()>
    where
        T: Unpin + Clone + Send + Sync + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        let parent = self.config.file_path.parent().unwrap();
        info!("Watching: {:?}", parent);

        // Initial read
        Self::read_new_lines(
            &self.config.file_path,
            &mut self.last_position,
        )?;

        // On windows we are going to do manual polling
        #[cfg(target_os = "linux")]
        self.watcher
            .watch(parent, notify::RecursiveMode::NonRecursive)?;

        info!("Using tokio tasks for command execution");

        let ctx = Arc::new(ClipboardContext::new().unwrap());

        while let Some(messages) = self.next().await {
            for message in messages? {
                // This will execute the designated command, and commands with no prefix
                for cmd_arg in [&message.command, ""].iter() {
                    if let Some(commands) = self.get_commands(cmd_arg) {
                        for command in commands {
                            // Clone items to be moved into the task
                            let cloned_config = self.config.clone();
                            let cloned_message = message.clone();
                            let cloned_enigo: Arc<Mutex<enigo::Enigo>> = self.enigo.clone();
                            let cloned_command = command.clone();
                            let cloned_clipboard_context = ctx.clone();

                            // Spawn async task instead of blocking thread
                            tokio::spawn(async move {
                                let keyboard = Keyboard::new(
                                    cloned_message.clone(),
                                    cloned_enigo,
                                    cloned_config.clone(),
                                    cloned_clipboard_context,
                                );

                                if let Err(err) = Self::execute_command(
                                    &cloned_config,
                                    cloned_command.as_ref(),
                                    &cloned_message,
                                    keyboard,
                                )
                                .await {
                                    error!("Error while executing command: {:?}", err);
                                }
                            });
                        }
                    }
                }
            }
        }

        info!("Done watching");

        Ok(())
    }

    /// Reads new lines from the monitored file since the last read position.
    ///
    /// # Arguments
    /// * `filename` - Path to the file to read
    /// * `last_position` - Mutable reference to the last read position
    ///
    /// # Returns
    /// Vector of new lines found in the file
    pub fn read_new_lines(filename: &std::path::Path, last_position: &mut u64) -> SourceCmdResult<Vec<String>> {
        debug!("Reading new lines from: {:?}", filename);
        let mut lines = Vec::new();
        let file = std::fs::File::open(filename)?;
        let mut reader = std::io::BufReader::new(file);

        reader.seek(std::io::SeekFrom::Start(*last_position))?;

        for line in reader.by_ref().lines().flatten() {
            lines.push(line);
        }

        *last_position = reader.stream_position()?;
        Ok(lines)
    }
}

/// Stream implementation for SourceCmdLogParser.
///
/// This allows the parser to be used as an async stream that yields
/// chat messages as they are detected in the monitored file.
impl<T, E> Stream for SourceCmdLogParser<T, E>
where
    T: Unpin + Clone + Sync + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = SourceCmdResult<Vec<ChatMessage>>;

    /// Windows implementation that polls the file at regular intervals.
    #[cfg(target_os = "windows")]
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let cmd_parser = self.get_mut();

        // Check if the stop flag is set
        if let Some(stop_flag) = &cmd_parser.config.stop_flag {
            if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                return std::task::Poll::Ready(None);
            }
        }

        // Check the file for new data at regular intervals
        if cmd_parser.timer.poll_tick(cx).is_ready() {
            debug!("Polling file for new data");
            // Attempt to read new lines from the file
            match Self::read_new_lines(
                &cmd_parser.config.file_path,
                &mut cmd_parser.last_position,
            ) {
                Ok(lines) => {
                    let messages = lines
                        .iter()
                        .filter_map(|line| cmd_parser.parse_log.parse_command(line))
                        .collect::<Vec<_>>();

                    if !messages.is_empty() {
                        return std::task::Poll::Ready(Some(Ok(messages)));
                    }
                }
                Err(e) => {
                    error!("Error reading file: {:?}", e);
                    return std::task::Poll::Ready(Some(Err(e)));
                }
            }
        }

        // If there are no new messages, return Poll::Pending to be polled again
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }

    /// Linux implementation that uses file system events for efficient monitoring.
    #[cfg(target_os = "linux")]
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use notify::EventKind;

        let cmd_parser = self.get_mut();

        // Check if the stop flag is set
        if let Some(stop_flag) = &cmd_parser.config.stop_flag {
            if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                return std::task::Poll::Ready(None);
            }
        }

        match Pin::new(&mut cmd_parser.rx).poll_next(cx) {
            std::task::Poll::Ready(Some(event_result)) => {
                match event_result {
                    Ok(event) => {
                        // Validate event is a file write and is the file we're watching
                        if !EventKind::is_modify(&event.kind)
                            || event.paths[0] != cmd_parser.config.file_path
                        {
                            return std::task::Poll::Ready(Some(Ok(vec![])));
                        }

                        let lines = Self::read_new_lines(
                            &cmd_parser.config.file_path,
                            &mut cmd_parser.last_position,
                        )?;

                        let messages = lines
                            .iter()
                            .filter_map(|line| cmd_parser.parse_log.parse_command(line))
                            .collect::<Vec<_>>();

                        std::task::Poll::Ready(Some(Ok(messages)))
                    }
                    Err(e) => {
                        error!("File watcher error: {:?}", e);
                        std::task::Poll::Pending
                    }
                }
            }
            std::task::Poll::Ready(None) => {
                std::task::Poll::Ready(None) // Stream ended
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}