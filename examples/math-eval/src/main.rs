mod commands;
mod state;

use std::{env, path::PathBuf};

use log::{info, LevelFilter};
use source_cmd_parser::{builder::SourceCmdBuilder, error::SourceCmdError, Cs2LogParser};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), SourceCmdError> {
    pretty_env_logger::formatted_timed_builder()
        .filter_level(LevelFilter::Info)
        .init();
    info!("Starting Source Cmd Parser. Version: {}", VERSION);

    let state_handle = state::new_handle_with_defaults();

    let mut parser = SourceCmdBuilder::new()
        .file_path(Box::new(PathBuf::from(
            "/home/isaac/games/SteamLibrary/steamapps/common/Counter-Strike Global Offensive/game/csgo/console.log",
        )))
        .cfg_file_path(PathBuf::from(
            "/home/isaac/games/SteamLibrary/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg/scp.cfg",
        ))
        .exec_bind_key(source_cmd_parser::enigo::Key::Layout('p')) // Configure your bind key here
        .chat_delay(std::time::Duration::from_millis(200)) // 200ms delay for owner
        .state(state_handle)
        .set_parser(Box::new(Cs2LogParser::new()))
        .add_global_command(commands::eval)
        .owner("/home/fozie")
        .build()?;

    parser.run().await?;

    Ok(())
}
