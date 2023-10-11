use std::{env, path::PathBuf};

use chatgpt::{prelude::ChatGPT, types::CompletionResponse};
use lazy_static::lazy_static;
use log::{info, LevelFilter};
use source_cmd_parser::{
    error::SourceCmdResult,
    log_parser::SourceCmdBuilder,
    model::{ChatMessage, ChatResponse},
};

lazy_static! {
    static ref GPT_CLIENT: ChatGPT =
        ChatGPT::new(env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"))
            .expect("Unable to create GPT Client");
}
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> SourceCmdResult<()> {
    pretty_env_logger::formatted_timed_builder()
        .filter_level(LevelFilter::Debug)
        .init();
    info!("Starting Source Cmd Parser. Version: {}", VERSION);

    let mut parser = SourceCmdBuilder::new()
        .file_path(Box::new(PathBuf::from(
            "/mnt/games/SteamLibrary/steamapps/common/Counter-Strike Source/cstrike/log.txt",
        )))
        .add_command(".explain", explain)
        .add_command(".dad_joke", dad_joke)
        .owner("***REMOVED***")
        .build()?;

    parser.run().await?;

    Ok(())
}

async fn explain(chat_message: ChatMessage) -> SourceCmdResult<Option<ChatResponse>> {
    info!("Explain: {}", chat_message.message);

    let response: CompletionResponse = GPT_CLIENT
        .send_message(format!(
            "Please response in 200 characters or less. The prompt is: \"{}\"",
            chat_message.message
        ))
        .await
        .unwrap();

    let mut chat_response = "[AI]: ".to_string();

    chat_response.push_str(response.message_choices[0].message.content.as_str());

    Ok(Some(ChatResponse::new(chat_response)))
}

async fn dad_joke(_: ChatMessage) -> SourceCmdResult<Option<ChatResponse>> {
    let response: CompletionResponse = GPT_CLIENT
        .send_message("Please tell me a data joke".to_string())
        .await
        .unwrap();

    let chat_response = response.message_choices[0].message.content.clone();
    Ok(Some(ChatResponse::new(chat_response)))
}
