use std::{env, path::PathBuf, sync::Arc};

use chatgpt::{prelude::ChatGPT, types::CompletionResponse};
use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use log::{info, LevelFilter};
use source_cmd_parser::{
    builder::SourceCmdBuilder,
    error::SourceCmdError,
    keyboard::Keyboard,
    model::ChatMessage,
    parsers::CSSLogParser,
};
use tokio::sync::RwLock;

lazy_static! {
    static ref GPT_CLIENT: ChatGPT =
        ChatGPT::new(env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"))
            .expect("Unable to create GPT Client");
}
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct State {
    pub last_message_time: Option<DateTime<Utc>>,
    pub last_chat_message: Option<String>,
    pub personality: String,
}

#[tokio::main]
async fn main() -> Result<(), SourceCmdError> {
    pretty_env_logger::formatted_timed_builder()
        .filter_level(LevelFilter::Debug)
        .init();
    info!("Starting Source Cmd Parser. Version: {}", VERSION);

    let mut parser = SourceCmdBuilder::new()
        .file_path(Box::new(PathBuf::from(
            "/mnt/games/SteamLibrary/steamapps/common/Counter-Strike Source/cstrike/log.txt",
        )))
        .state(Arc::new(RwLock::new(State::default())))
        .set_parser(Box::new(CSSLogParser::new()))
        .add_command(".explain", explain)
        .add_command(".dad_joke", dad_joke)
        .owner("username")
        .build()?;

    parser.run().await?;

    Ok(())
}

type StateType = Arc<RwLock<State>>;
type ErrorType = SourceCmdError;
type KeyboardType = Keyboard<StateType, ErrorType>;

async fn explain(
    chat_message: ChatMessage,
    state: StateType,
    mut keyboard: KeyboardType,
) -> Result<(), ErrorType> {
    info!("Explain: {}", chat_message.message);

    {
        let mut state = state.write().await;

        if let Some(last_message_time) = state.last_message_time {
            // Greater than 10 seconds
            if last_message_time + chrono::Duration::seconds(30) > Utc::now() {
                info!("Skipping explain. Last message was less than 30 seconds ago.");
                return Ok(());
            }
        }
        state.last_message_time = Some(chat_message.time_stamp);
    }

    let response: CompletionResponse = GPT_CLIENT
        .send_message(format!(
            "Please response in 120 characters or less. Can you response as if you were {}. The prompt is: \"{}\"",
            state.read().await.personality,
            chat_message.message
        ))
        .await
        .map_err(|e| SourceCmdError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    let mut chat_response = "[AI]: ".to_string();

    chat_response.push_str(response.message_choices[0].message.content.as_str());

    // Limit chat repsonse to 120 charactes
    // chat_response = chat_response.chars().take(120).collect();

    keyboard.simulate(chat_response).await?;
    Ok(())
}

async fn dad_joke(
    _: ChatMessage,
    _: StateType,
    mut keyboard: KeyboardType,
) -> Result<(), ErrorType> {
    let response: CompletionResponse = GPT_CLIENT
        .send_message("Please tell me a data joke".to_string())
        .await
        .map_err(|e| SourceCmdError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    let chat_response = response.message_choices[0].message.content.clone();
    keyboard.simulate(chat_response).await?;
    Ok(())
}
