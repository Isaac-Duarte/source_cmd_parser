use std::marker::PhantomData;

use enigo::KeyboardControllable;
use tokio::{fs, io::AsyncWriteExt};

use crate::{
    error::SourceCmdResult,
    model::{ChatMessage, Config},
};

/// Writes chat commands to a cfg file and presses a bind key to execute them.
///
/// This struct writes chat messages as `say {message}` to the configured cfg file,
/// then automatically presses the configured bind key to trigger `exec scp.cfg`
/// in the game. Players should have a bind set up like: `bind p "exec scp.cfg"`
pub struct Keyboard<State, E: std::error::Error + Send + Sync + 'static> {
    chat_message: ChatMessage,
    config: Config<State>,
    _er: PhantomData<E>,
}

impl<State, E: std::error::Error + Send + Sync + 'static> Keyboard<State, E> {
    /// Creates a new Keyboard instance bound to the supplied config.
    ///
    /// # Arguments
    /// * `chat_message` - The original chat message that triggered the command
    /// * `config` - Parser configuration containing the cfg destination
    pub fn new(chat_message: ChatMessage, config: Config<State>) -> Self {
        Self {
            chat_message,
            config,
            _er: PhantomData,
        }
    }

    /// Writes the message to the configured cfg file as `say {message}`.
    ///
    /// After writing the file, this method will press the configured bind key
    /// to trigger the `exec scp.cfg` command in the game.
    pub async fn simulate(&mut self, message: String) -> SourceCmdResult<()> {
        let mut sanitised = message.replace(['\r', '\n'], " ").trim().to_string();

        if sanitised.is_empty() {
            return Ok(());
        }

          // Apply delay for owner messages
        if self
            .config
            .owner
            .as_ref()
            .is_some_and(|owner| self.chat_message.user_name.contains(owner))
        {
            tokio::time::sleep(self.config.chat_delay).await;
        }

        log::info!("WHAT?");

        // Prevent accidental brace balancing issues by escaping closing braces.
        sanitised = sanitised.replace('}', "\\}");

        let payload = format!("say {{{}}}\n", sanitised);
        let cfg_path = self.config.cfg_file_path.clone();
        let lock = self.config.cfg_write_lock.clone();

        let _guard = lock.lock().await;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&cfg_path)
            .await?;

        file.write_all(payload.as_bytes()).await?;
        file.flush().await?;

        // Press the bind key to execute the cfg
        let mut enigo = self.config.enigo.lock().await;
        enigo.key_down(self.config.exec_bind_key);
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        enigo.key_up(self.config.exec_bind_key);

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
