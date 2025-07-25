use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use log::{info, warn, LevelFilter};
use source_cmd_parser::{
    builder::SourceCmdBuilder,
    error::SourceCmdError,
    keyboard::Keyboard,
    model::ChatMessage,
    parsers::CSSLogParser,
};
use tokio::sync::RwLock;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct State {
    pub user_cooldowns: HashMap<String, UserCooldown>,
}

pub struct UserCooldown {
    pub timestamps: Vec<Instant>,
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
        .add_global_command(eval)
        .owner("username")
        .build()?;

    parser.run().await?;

    Ok(())
}

const COOLDOWN_DURATION: Duration = Duration::from_secs(120); // 2 minutes
const MESSAGE_LIMIT: usize = 5;

type StateType = Arc<RwLock<State>>;
type KeyboardType = Keyboard<StateType, SourceCmdError>;

async fn eval(
    chat_message: ChatMessage,
    state_lock: StateType,
    mut keyboard: KeyboardType,
) -> Result<(), SourceCmdError> {
    let message = chat_message.raw_message;

    if message.trim().parse::<f64>().is_ok() {
        return Ok(());
    }

    match meval::eval_str(&message.replace('x', "*")) {
        Ok(response) => {
            {
                let mut state = state_lock.write().await;

                let user_cooldown = state
                    .user_cooldowns
                    .entry(chat_message.user_name.clone())
                    .or_insert(UserCooldown {
                        timestamps: Vec::new(),
                    });

                // Remove outdated timestamps
                user_cooldown
                    .timestamps
                    .retain(|&timestamp| timestamp.elapsed() < COOLDOWN_DURATION);

                // Check cooldown status
                if user_cooldown.timestamps.len() >= MESSAGE_LIMIT {
                    warn!(
                        "Skipping eval. User {} has reached the message limit of {}. Time left till cooldown: {:?}",
                        chat_message.user_name, MESSAGE_LIMIT,
                        COOLDOWN_DURATION - user_cooldown.timestamps[0].elapsed());

                    return Ok(());
                }

                // If not in cooldown, add the new timestamp
                user_cooldown.timestamps.push(Instant::now());
            }

            info!("Eval: {} = {}", message, response);
            keyboard.simulate(response.to_string()).await?;
            Ok(())
        }
        Err(_) => Ok(()),
    }
}
