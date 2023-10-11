use std::{env, path::PathBuf};

use log::{info, LevelFilter};
use source_cmd_parser::{
    error::SourceCmdResult,
    log_parser::SourceCmdBuilder,
    model::{ChatMessage, ChatResponse},
};

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
        .add_commandd(eval)
        .owner("***REMOVED***")
        .build()?;

    parser.run().await?;

    Ok(())
}

async fn eval(chat_message: ChatMessage) -> SourceCmdResult<Option<ChatResponse>> {
    let message = chat_message.raw_message;

    if message.trim().parse::<f64>().is_ok() {
        return Ok(None);
    }

    match meval::eval_str(message.replace('x', "*")) {
        Ok(response) => {
            info!("Eval: {} = {}", message, response);
            Ok(Some(ChatResponse::new(response.to_string())))
        }
        Err(_) => Ok(None),
    }
}
