use std::{marker::PhantomData, sync::Arc};

use clipboard_rs::ClipboardContext;
use tokio::sync::Mutex;

use crate::{
    error::SourceCmdResult,
    log_parser::SourceCmdLogParser,
    model::{ChatMessage, ChatResponse, Config},
};

/// Provides keyboard simulation functionality for sending chat messages.
///
/// This struct wraps the enigo library and provides a high-level interface
/// for simulating keyboard input to send chat messages in games.
pub struct Keyboard<State, E: std::error::Error + Send + Sync + 'static> {
    chat_message: ChatMessage,
    enigo: Arc<Mutex<enigo::Enigo>>,
    config: Config<State>,
    _er: PhantomData<E>,
    clipboard_ctx: Arc<ClipboardContext>,
}

impl<State, E: std::error::Error + Send + Sync + 'static> Keyboard<State, E> {
    /// Creates a new Keyboard instance.
    ///
    /// # Arguments
    /// * `chat_message` - The original chat message that triggered the command
    /// * `enigo` - Shared enigo instance for keyboard simulation
    /// * `config` - Parser configuration
    /// * `clipboard_ctx` - Shared clipboard context
    pub fn new(
        chat_message: ChatMessage,
        enigo: Arc<Mutex<enigo::Enigo>>,
        config: Config<State>,
        clipboard_ctx: Arc<ClipboardContext>,
    ) -> Self {
        Self {
            chat_message,
            enigo,
            config,
            _er: PhantomData,
            clipboard_ctx,
        }
    }

    /// Simulates typing and sending a chat message.
    ///
    /// This method handles:
    /// - Copying the message to clipboard
    /// - Opening the chat window
    /// - Pasting the message
    /// - Sending the message
    /// - Applying delays for owner messages
    /// - Splitting long messages into chunks
    ///
    /// # Arguments
    /// * `message` - The message to send
    ///
    /// # Returns
    /// Result indicating success or failure
    pub async fn simulate(&mut self, message: String) -> SourceCmdResult<()> {
        let mut response = ChatResponse::new(message);

        // Apply delay for owner messages
        if self
            .config
            .owner
            .as_ref()
            .is_some_and(|owner| self.chat_message.user_name.contains(owner))
        {
            response.delay_on_enter = Some(self.config.chat_delay);
        }

        SourceCmdLogParser::<_, E>::run_sequence(
            &self.config,
            self.enigo.clone(),
            response,
            self.clipboard_ctx.clone(),
        )
        .await?;

        tokio::time::sleep(self.config.chat_delay).await;

        Ok(())
    }

    /// Gets the original chat message that triggered the command.
    pub fn get_chat_message(&self) -> &ChatMessage {
        &self.chat_message
    }

    /// Gets the username of the person who sent the original message.
    pub fn get_username(&self) -> &str {
        &self.chat_message.user_name
    }

    /// Gets the command that was executed.
    pub fn get_command(&self) -> &str {
        &self.chat_message.command
    }

    /// Gets the raw message content (without the command).
    pub fn get_message(&self) -> &str {
        &self.chat_message.message
    }

    /// Gets the full raw message as it appeared in the log.
    pub fn get_raw_message(&self) -> &str {
        &self.chat_message.raw_message
    }

    /// Checks if the message was sent by the configured owner.
    pub fn is_from_owner(&self) -> bool {
        self.config
            .owner
            .as_ref()
            .is_some_and(|owner| self.chat_message.user_name.contains(owner))
    }
}
