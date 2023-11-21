use std::{
    collections::HashMap,
    fs::File,
    future::Future,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::Poll,
    time::Duration,
};

use enigo::KeyboardControllable;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, Stream, StreamExt,
};
use log::{debug, error, info};
use notify::{Config, EventKind, RecommendedWatcher, Watcher};
use tokio::{sync::RwLock, time};

use crate::{
    error::{SourceCmdError, SourceCmdResult},
    model::{ChatMessage, ChatResponse},
};

/// The `SourceCmdFn` trait provides a unified interface for functions
/// that accept a chat message and asynchronously produce a result with
/// an optional chat response.
///
/// This trait ensures thread safety by requiring the `Send`, `Sync`,
/// and `'static` lifetimes for its implementors which allows the function
/// to be async.
pub trait SourceCmdFn<T>: Send + Sync + 'static
where
    T: Send + Sync + 'static,
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
        state: Arc<RwLock<T>>,
    ) -> Pin<Box<dyn Future<Output = SourceCmdResult<Option<ChatResponse>>> + Send>>;
}

/// Implementation of `SourceCmdFn` for any function type `F` that meets
/// the specified constraints.
///
/// This allows for any appropriate function to be treated as a `SourceCmdFn`
/// without needing to explicitly wrap it or implement the trait separately.
impl<F, Fut, T> SourceCmdFn<T> for F
where
    F: Fn(ChatMessage, Arc<RwLock<T>>) -> Fut + Sync + Send + 'static,
    Fut: Future<Output = SourceCmdResult<Option<ChatResponse>>> + Send + 'static,
    T: Send + Sync + 'static,
{
    fn call(
        &self,
        message: ChatMessage,
        state: Arc<RwLock<T>>,
    ) -> Pin<Box<dyn Future<Output = SourceCmdResult<Option<ChatResponse>>> + Send>> {
        Box::pin(self(message, state))
    }
}

const REGEX_STRING: &str =
    r"^(\d{2}\/\d{2}\/\d{4} - \d{2}:\d{2}:\d{2}): (\*DEAD\* )?([^:]+) :  (.+)$";
const CS2_REGEX_STRING: &str = r"^(\d{2}\/\d{2} \d{2}:\d{2}:\d{2}) (\*DEAD\* )?([^:]+) : (.+)$";

/// `SourceCmdLogParser` is responsible for monitoring a file for changes,
/// parsing chat messages, and executing associated commands.
///
/// It holds the configuration and resources required to monitor a file for changes,
/// parse the incoming chat commands, and execute the associated actions.
pub struct SourceCmdLogParser<T> {
    file_path: PathBuf,
    time_out: Option<Duration>,
    commands: HashMap<String, Vec<Box<dyn SourceCmdFn<T>>>>,
    regex: regex::Regex,
    watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<notify::Event>>,
    enigo: enigo::Enigo,
    last_position: u64,
    owner: Option<String>,
    shared_state: Arc<RwLock<T>>,
}

impl<T> SourceCmdLogParser<T> {
    pub fn builder() -> SourceCmdBuilder<T>
    where
        T: Send + Sync + 'static,
    {
        SourceCmdBuilder::<T>::new()
    }

    /// Retrieves the commands associated with a given command string.
    ///
    /// # Parameters
    /// - `command`: The command string to look up.
    ///
    /// # Returns
    /// An optional reference to a vec of command functions.
    pub fn get_commands(&self, command: &str) -> Option<&Vec<Box<dyn SourceCmdFn<T>>>> {
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
            Config::default(),
        )?;

        Ok((watcher, rx))
    }

    /// Parses a raw message string into a `ChatMessage` struct.
    ///
    /// # Parameters
    /// - `raw_message`: The raw string message to be parsed.
    ///
    /// # Returns
    /// An optional `ChatMessage` containing the parsed message details.
    /// Returns `None` if the parsing fails.
    pub fn parse_command(&self, raw_message: &str) -> Option<ChatMessage> {
        let raw_message = if raw_message.contains("[Warden]") {
            raw_message
                .trim()
                .replace("[Warden]00BFFF ", "")
                .replace("000000", "")
                .replace("FFC0CB", " ")
        } else {
            raw_message.trim().to_string()
        };

        if let Some(captures) = self.regex.captures(&raw_message) {
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

    /// Executes a given chat command asynchronously.
    ///
    /// # Parameters
    /// - `command`: The command function to be executed.
    /// - `message`: The chat message associated with the command.
    ///
    /// # Returns
    /// An asynchronous result containing an optional `ChatResponse`.
    pub async fn execute_command(
        &self,
        command: &Box<dyn SourceCmdFn<T>>,
        message: &ChatMessage,
    ) -> SourceCmdResult<Option<ChatResponse>>
    where
        T: Send + Sync + 'static,
    {
        let response = command
            .call(message.clone(), self.shared_state.clone())
            .await;

        match response {
            Ok(response) => Ok(response),
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
    async fn run_sequence(&mut self, chat_response: &mut ChatResponse) -> SourceCmdResult<()> {
        // Function to send a chat message
        async fn send_message(
            enigo: &mut enigo::Enigo,
            message: &str,
            chat_response: &ChatResponse,
        ) {
            enigo.key_down(enigo::Key::Layout('Y'));
            enigo.key_up(enigo::Key::Layout('Y'));

            enigo.key_sequence(message);

            if let Some(delay) = chat_response.delay_on_enter {
                time::sleep(delay).await;
            }

            enigo.key_down(enigo::Key::Return);
            enigo.key_up(enigo::Key::Return);
        }

        let message = chat_response.message.as_str();

        if message.len() <= 120 {
            send_message(&mut self.enigo, message, chat_response).await;

            return Ok(());
        }

        let words = message.split_whitespace();

        let mut current_chunk = String::new();

        for word in words {
            if current_chunk.len() + word.len() + 1 > 120 {
                // +1 for space
                send_message(&mut self.enigo, &current_chunk, chat_response).await;

                time::sleep(Duration::from_millis(600)).await;

                current_chunk.clear();
            }

            if !current_chunk.is_empty() {
                current_chunk.push(' ');
            }
            current_chunk.push_str(word);
        }

        // Send any remaining chunk
        if !current_chunk.is_empty() {
            send_message(&mut self.enigo, &current_chunk, chat_response).await;

            time::sleep(Duration::from_millis(600)).await;
        }

        Ok(())
    }

    fn handle_execution_response(
        &self,
        response: Option<ChatResponse>,
        message: &ChatMessage,
    ) -> SourceCmdResult<Vec<ChatResponse>> {
        let mut responses = vec![];

        if let Some(mut response) = response {
            // If the response has an owner, add a delay before executing
            if self
                .owner
                .as_ref()
                .is_some_and(|owner| owner == &message.user_name)
            {
                response.delay_on_enter = Some(Duration::from_millis(700));
            }

            responses.push(response);
        }

        Ok(responses)
    }

    /// Starts monitoring the file for changes, parses chat messages,
    /// and executes the associated commands.
    ///
    /// # Returns
    /// A result indicating the success or failure of the operation.
    pub async fn run(&mut self) -> SourceCmdResult<()>
    where
        T: Send + Sync + 'static,
    {
        let parent = self.file_path.parent().unwrap();
        info!("Watching: {:?}", parent);

        // Initial read
        SourceCmdLogParser::<T>::read_new_lines(&self.file_path, &mut self.last_position)?;

        self.watcher
            .watch(parent, notify::RecursiveMode::NonRecursive)?;

        while let Some(messages) = self.next().await {
            let mut responses = Vec::new();

            for message in messages? {
                // This will execute the desingated command, and commands with no prefix
                for cmd_arg in [&message.command, ""].iter() {
                    if let Some(commands) = self.get_commands(cmd_arg) {
                        for command in commands {
                            responses.extend(self.handle_execution_response(
                                self.execute_command(command, &message).await?,
                                &message,
                            )?);
                        }
                    }
                }
            }

            for response in &mut responses {
                self.run_sequence(response).await?;
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

        for line in reader.by_ref().lines() {
            if let Ok(line) = line {
                lines.push(line);
            }
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
impl<T> Stream for SourceCmdLogParser<T> {
    type Item = SourceCmdResult<Vec<ChatMessage>>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let cmd_parser = self.get_mut();

        match Pin::new(&mut cmd_parser.rx).poll_next(cx) {
            Poll::Ready(Some(event_result)) => {
                match event_result {
                    Ok(event) => {
                        // Validate event is a file write and is the file we're watching
                        if !EventKind::is_modify(&event.kind)
                            || event.paths[0] != cmd_parser.file_path
                        {
                            return Poll::Ready(Some(Ok(vec![])));
                        }

                        let lines = match SourceCmdLogParser::<T>::read_new_lines(
                            &cmd_parser.file_path,
                            &mut cmd_parser.last_position,
                        ) {
                            Ok(lines) => lines,
                            Err(e) => return Poll::Ready(Some(Err(e))),
                        };

                        let messages = lines
                            .iter()
                            .filter_map(|line| cmd_parser.parse_command(line))
                            .collect::<Vec<_>>();

                        Poll::Ready(Some(Ok(messages)))
                    }
                    Err(e) => {
                        error!("Error: {:?}", e);
                        // You might want to handle this error more gracefully
                        Poll::Ready(Some(Err(e.into())))
                    }
                }
            }
            Poll::Ready(None) => {
                // Stream ended
                Poll::Ready(None)
            }
            Poll::Pending => {
                // Just return Poll::Pending without waking up the task again
                Poll::Pending
            }
        }
    }
}


pub struct SourceCmdBuilder<T> {
    file_path: Option<PathBuf>,
    time_out: Option<Duration>,
    commands: HashMap<String, Vec<Box<dyn SourceCmdFn<T>>>>,
    owner: Option<String>,
    state: Option<T>,
}

impl<T: Send + Sync + 'static> SourceCmdBuilder<T> {
    pub fn new() -> Self {
        Self {
            file_path: None,
            time_out: None,
            commands: HashMap::new(),
            owner: None,
            state: None,
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

    pub fn add_command<F: SourceCmdFn<T> + 'static>(mut self, command: &str, function: F) -> Self {
        self.commands
            .entry(command.to_string())
            .or_default()
            .push(Box::new(function));

        self
    }

    pub fn add_commandd<F: SourceCmdFn<T> + 'static>(mut self, function: F) -> Self {
        self.commands
            .entry("".to_string())
            .or_default()
            .push(Box::new(function));

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

    pub fn build(self) -> SourceCmdResult<SourceCmdLogParser<T>> {
        if let (Some(file_path), Some(state)) = (self.file_path, self.state) {
            let (watcher, rx) = SourceCmdLogParser::<T>::async_watcher()?;

            Ok(SourceCmdLogParser {
                file_path,
                time_out: self.time_out,
                commands: self.commands,
                regex: regex::Regex::new(REGEX_STRING)?,
                watcher,
                rx,
                enigo: enigo::Enigo::new(),
                last_position: 0,
                owner: self.owner,
                shared_state: Arc::new(RwLock::new(state)),
            })
        } else {
            Err(SourceCmdError::MissingFieldS(
                "file_path, directory_path, time_out, commands".to_string(),
            ))
        }
    }
}
