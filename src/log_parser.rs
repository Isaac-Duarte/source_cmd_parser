use std::{
    collections::HashMap,
    fs::File,
    future::Future,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
    task::Poll,
    time::Duration,
};

use enigo::KeyboardControllable;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, Stream, StreamExt,
};
use log::{debug, error, info};
use notify::{EventKind, RecommendedWatcher, Watcher};
use rayon::ThreadPoolBuilder;
use tokio::{sync::Mutex, time};

use crate::{
    error::{SourceCmdError, SourceCmdResult},
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
    ) -> Pin<Box<dyn Future<Output = Result<Option<ChatResponse>, E>> + Send>>;
}

/// Implementation of `SourceCmdFn` for any function type `F` that meets
/// the specified constraints.
///
/// This allows for any appropriate function to be treated as a `SourceCmdFn`
/// without needing to explicitly wrap it or implement the trait separately.
impl<F, Fut, T, E> SourceCmdFn<T, E> for F
where
    F: Fn(ChatMessage, T) -> Fut + Sync + Send + 'static,
    Fut: Future<Output = Result<Option<ChatResponse>, E>> + Send + 'static,
    T: Clone + Send + Sync + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    fn call(
        &self,
        message: ChatMessage,
        state: T,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ChatResponse>, E>> + Send>> {
        Box::pin(self(message, state))
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
    fn async_watcher(
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
    ) -> SourceCmdResult<Option<ChatResponse>>
    where
        T: Clone + Sync + Send + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        // If a timeout is specified, wrap the command in a timeout future
        let response = if let Some(time_out) = config.time_out {
            let timeout = time::timeout(
                time_out,
                command.call(message.clone(), config.shared_state.clone()),
            )
            .await;

            if let Ok(response) = timeout {
                response
            } else {
                error!("Command timed out: {}", message.raw_message);

                return Ok(None);
            }
        } else {
            command
                .call(message.clone(), config.shared_state.clone())
                .await
        };

        match response {
            Ok(response) => {
                // Modify chat resposne if the user is the owner to add a delay
                if config
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner == &message.user_name)
                    && response.is_some()
                {
                    Ok(Some(ChatResponse {
                        message: response.unwrap().message,
                        delay_on_enter: Some(config.chat_delay),
                    }))
                } else {
                    Ok(response)
                }
            }
            Err(err) => {
                error!(
                    "Error whil executing command {}: {:?}",
                    message.command, err
                );
                Ok(None)
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
    #[cfg(target_os = "linux")]
    async fn run_sequence(
        config: &Config<T>,
        enigo: &mut enigo::Enigo,
        chat_response: &ChatResponse,
    ) -> SourceCmdResult<()> {
        // Function to send a chat message
        async fn send_message(
            enigo: &mut enigo::Enigo,
            message: &str,
            chat_response: &ChatResponse,
            chat_key: enigo::Key,
        ) {
            enigo.key_down(chat_key);
            time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(chat_key);

            enigo.key_sequence(message);

            if let Some(delay) = chat_response.delay_on_enter {
                time::sleep(delay).await;
            }

            enigo.key_down(enigo::Key::Return);
            time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(enigo::Key::Return);
        }

        let message = chat_response.message.as_str();

        if message.len() <= config.max_entry_length {
            send_message(enigo, message, chat_response, config.chat_key).await;

            return Ok(());
        }

        let words = message.split_whitespace();

        let mut current_chunk = String::new();

        for word in words {
            if current_chunk.len() + word.len() + 1 > config.max_entry_length {
                // +1 for space
                send_message(enigo, &current_chunk, chat_response, config.chat_key).await;

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
            send_message(enigo, &current_chunk, chat_response, config.chat_key).await;

            time::sleep(config.chat_delay).await;
        }

        Ok(())
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
    #[cfg(target_os = "windows")]
    async fn run_sequence(
        config: &Config<T>,
        enigo: &mut enigo::Enigo,
        chat_response: &ChatResponse,
    ) -> SourceCmdResult<()> {
        use clipboard_win::set_clipboard_string;

        // Function to send a chat message
        async fn send_message(
            enigo: &mut enigo::Enigo,
            message: &str,
            chat_response: &ChatResponse,
            chat_key: enigo::Key,
        ) -> SourceCmdResult<()> {
            enigo.key_down(chat_key);
            tokio::time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(chat_key);

            // Set Clipboard
            set_clipboard_string(message)
                .map_err(|err| SourceCmdError::ClipboardError(err.to_string()))?;

            // Ctrl + V
            enigo.key_down(enigo::Key::Control);
            enigo.key_down(enigo::Key::Layout('v'));
            tokio::time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(enigo::Key::Control);
            enigo.key_up(enigo::Key::Layout('v'));

            if let Some(delay) = chat_response.delay_on_enter {
                time::sleep(delay).await;
            }

            enigo.key_down(enigo::Key::Return);
            tokio::time::sleep(time::Duration::from_millis(20)).await;
            enigo.key_up(enigo::Key::Return);

            Ok(())
        }

        let message = chat_response.message.as_str();

        if message.len() <= config.max_entry_length {
            send_message(enigo, message, chat_response, config.chat_key).await;

            return Ok(());
        }

        let words = message.split_whitespace();

        let mut current_chunk = String::new();

        for word in words {
            if current_chunk.len() + word.len() + 1 > config.max_entry_length {
                // +1 for space
                send_message(
                    enigo,
                    &current_chunk,
                    chat_response,
                    config.chat_key,
                )
                .await;

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
            send_message(
                enigo,
                &current_chunk,
                chat_response,
                config.chat_key,
            )
            .await;

            time::sleep(config.chat_delay).await;
        }

        Ok(())
    }

    fn handle_execution_response(
        config: &Config<T>,
        response: Option<ChatResponse>,
        message: &ChatMessage,
    ) -> Option<ChatResponse> {
        response.map(|mut response| {
            // If the response has an owner, add a delay before executing
            if config
                .owner
                .as_ref()
                .is_some_and(|owner| owner == &message.user_name)
            {
                response.delay_on_enter = Some(config.chat_delay);
            }

            response
        })
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
        SourceCmdLogParser::<T, E>::read_new_lines(
            &self.config.file_path,
            &mut self.last_position,
        )?;

        // On windows we are going to do manual polling
        #[cfg(target_os = "linux")]
        self.watcher
            .watch(parent, notify::RecursiveMode::NonRecursive)?;

        info!("Creating thread pool with {} threads", self.config.threads);
        let pool = ThreadPoolBuilder::new()
            .num_threads(self.config.threads)
            .build()?;

        while let Some(messages) = self.next().await {
            for message in messages? {
                // This will execute the desingated command, and commands with no prefix
                for cmd_arg in [&message.command, ""].iter() {
                    if let Some(commands) = self.get_commands(cmd_arg) {
                        for command in commands {
                            // Clone items to be moved into the thread
                            let cloned_config = self.config.clone();
                            let cloned_message = message.clone();
                            let cloned_enigo: Arc<Mutex<enigo::Enigo>> = self.enigo.clone();
                            let cloned_command = command.clone();

                            pool.spawn(move || {
                                let runtime = tokio::runtime::Runtime::new().unwrap();

                                let response: SourceCmdResult = runtime.block_on(async {
                                    let response = Self::execute_command(
                                        &cloned_config,
                                        cloned_command.as_ref(),
                                        &cloned_message,
                                    )
                                    .await?;

                                    let response = Self::handle_execution_response(
                                        &cloned_config,
                                        response,
                                        &cloned_message,
                                    );

                                    if let Some(response) = response {
                                        let mut enigo = cloned_enigo.lock().await;
                                        
                                        Self::run_sequence(&cloned_config, &mut enigo, &response)
                                            .await?;
                                    }

                                    Ok(())
                                });

                                if let Err(err) = response {
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

    fn read_new_lines(filename: &Path, last_position: &mut u64) -> SourceCmdResult<Vec<String>> {
        debug!("Reading new lines from: {:?}", filename);
        let mut lines = Vec::new();
        let file = File::open(filename)?;
        let mut reader = BufReader::new(file);

        reader.seek(SeekFrom::Start(*last_position))?;

        for line in reader.by_ref().lines().flatten() {
            lines.push(line);
        }

        *last_position = reader.stream_position()?;
        Ok(lines)
    }
}

/// Attempts to poll for the next set of chat messages from the watched file.
///
/// This method polls the underlying file watcher for any changes and attempts to read and
/// parse chat messages when a file modification event occurs for the monitored file.
///
/// # Parameters
/// - `self`: A pinned mutable reference to the current instance of `SourceCmdLogParser`.
/// - `cx`: A mutable reference to the current task context.
///
/// # Returns
/// - `Poll::Ready(Some(Ok(messages)))`: When valid chat messages are parsed from a file modification.
/// - `Poll::Ready(Some(Err(_)))`: When there's an error while reading or parsing the file.
/// - `Poll::Ready(None)`: When the stream ends, i.e., no more messages are expected.
/// - `Poll::Pending`: When there's no new data available yet, but the stream hasn't ended.
impl<T, E> Stream for SourceCmdLogParser<T, E>
where
    T: Unpin + Clone + Sync + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = SourceCmdResult<Vec<ChatMessage>>;

    #[cfg(target_os = "windows")]
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let cmd_parser = self.get_mut();

        // Check if the stop flag is set
        if let Some(stop_flag) = &cmd_parser.config.stop_flag {
            if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                return Poll::Ready(None);
            }
        }

        // Check the file for new data at regular intervals
        if cmd_parser.config.timer.poll_tick(cx).is_ready() {
            debug!("Polling file for new data");
            // Attempt to read new lines from the file
            match SourceCmdLogParser::<T, E>::read_new_lines(
                &cmd_parser.config.file_path,
                &mut cmd_parser.last_position,
            ) {
                Ok(lines) => {
                    let messages = lines
                        .iter()
                        .filter_map(|line| cmd_parser.parse_log.parse_command(line))
                        .collect::<Vec<_>>();

                    if !messages.is_empty() {
                        return Poll::Ready(Some(Ok(messages)));
                    }
                }
                Err(e) => {
                    error!("Error reading file: {:?}", e);
                    // You may choose to return an error or continue polling
                    return Poll::Ready(Some(Err(e.into())));
                }
            }
        }

        // If there are no new messages, return Poll::Pending to be polled again
        cx.waker().wake_by_ref();
        Poll::Pending
    }

    #[cfg(target_os = "linux")]
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let cmd_parser = self.get_mut();

        // Check if the stop flag is set
        if let Some(stop_flag) = &cmd_parser.config.stop_flag {
            if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                return Poll::Ready(None);
            }
        }

        match Pin::new(&mut cmd_parser.rx).poll_next(cx) {
            Poll::Ready(Some(event_result)) => {
                match event_result {
                    Ok(event) => {
                        // Validate event is a file write and is the file we're watching
                        if !EventKind::is_modify(&event.kind)
                            || event.paths[0] != cmd_parser.config.file_path
                        {
                            return Poll::Ready(Some(Ok(vec![])));
                        }

                        let lines = SourceCmdLogParser::<T, E>::read_new_lines(
                            &cmd_parser.config.file_path,
                            &mut cmd_parser.last_position,
                        )?;

                        let messages = lines
                            .iter()
                            .filter_map(|line| cmd_parser.parse_log.parse_command(line))
                            .collect::<Vec<_>>();

                        Poll::Ready(Some(Ok(messages)))
                    }
                    Err(e) => {
                        error!("Error: {:?}", e);
                        // Handle error
                        Poll::Pending
                    }
                }
            }
            Poll::Ready(None) => {
                Poll::Ready(None) // Stream ended
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

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
    threads: usize,
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
            threads: num_cpus::get(),
        }
    }

    pub fn file_path(mut self, file_path: Box<dyn AsRef<Path>>) -> Self {
        let path_buf = file_path.as_ref().as_ref();
        self.file_path = Some(path_buf.to_path_buf());

        self
    }

    pub fn time_out(mut self, time_out: Duration) -> Self {
        self.time_out = Some(time_out);
        self
    }

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

    pub fn add_global_command<F: SourceCmdFn<T, E> + 'static>(mut self, function: F) -> Self {
        self.commands
            .entry("".to_string())
            .or_default()
            .push(Arc::new(function));

        self
    }

    pub fn owner(mut self, owner: &str) -> Self {
        self.owner = Some(owner.to_string());
        self
    }

    pub fn state(mut self, state: T) -> Self {
        self.state = Some(state);
        self
    }

    pub fn set_parser(mut self, parse_log: Box<dyn ParseLog>) -> Self {
        self.parse_log = Some(parse_log);
        self
    }

    pub fn max_entry_length(mut self, max_entry_length: usize) -> Self {
        self.max_entry_length = max_entry_length;
        self
    }

    pub fn stop_flag(mut self, stop_flag: Arc<AtomicBool>) -> Self {
        self.stop_flag = Some(stop_flag);
        self
    }

    pub fn chat_key(mut self, chat_key: enigo::Key) -> Self {
        self.chat_key = chat_key;
        self
    }

    pub fn chat_delay(mut self, chat_delay: Duration) -> Self {
        self.chat_delay = chat_delay;
        self
    }

    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    pub fn build(self) -> SourceCmdResult<SourceCmdLogParser<T, E>> {
        if let (Some(file_path), Some(state), Some(parse_log)) =
            (self.file_path, self.state, self.parse_log)
        {
            #[cfg(target_os = "linux")]
            let (watcher, rx) = SourceCmdLogParser::<T, E>::async_watcher()?;

            Ok(SourceCmdLogParser {
                commands: self.commands,
                #[cfg(target_os = "linux")]
                watcher,
                #[cfg(target_os = "linux")]
                rx,
                enigo: {
                    let mut enigo = enigo::Enigo::new();

                    #[cfg(target_os = "linux")]
                    enigo.set_delay(0);
                    Arc::new(Mutex::new(enigo))
                },
                last_position: 0,
                config: Config {
                    file_path,
                    time_out: self.time_out,
                    owner: self.owner,
                    shared_state: state,
                    max_entry_length: self.max_entry_length,
                    chat_delay: self.chat_delay,
                    stop_flag: self.stop_flag,
                    chat_key: self.chat_key,
                    threads: self.threads,
                },
                parse_log,
                #[cfg(target_os = "windows")]
                timer: time::interval(Duration::from_millis(100)),
            })
        } else {
            Err(SourceCmdError::MissingFieldS(
                "file_path, state, parse_log".to_string(),
            ))
        }
    }
}
