use std::time::Duration;

use log::{info, warn};
use source_cmd_parser::{
    error::SourceCmdError,
    keyboard::Keyboard,
    model::ChatMessage,
};

use crate::state::{CooldownCheck, Cooldowns, StateHandle, StateHandleExt};

const COOLDOWN_DURATION: Duration = Duration::from_secs(120);
const MESSAGE_LIMIT: usize = 5;

pub async fn handle(
    chat_message: ChatMessage,
    state: StateHandle,
    mut keyboard: Keyboard<StateHandle, SourceCmdError>,
) -> Result<(), SourceCmdError> {
    let message = chat_message.raw_message;
    let user_name = chat_message.user_name.clone();

    if message.trim().parse::<f64>().is_ok() {
        return Ok(());
    }

    let evaluation_target = message.replace('x', "*");

    let cooldown_status = state
        .with_write(move |state| {
            let cooldowns = state.get_or_insert_with::<Cooldowns, _>(Cooldowns::default);
            cooldowns.check_and_record(&user_name, MESSAGE_LIMIT, COOLDOWN_DURATION)
        })
        .await;

    if let CooldownCheck::Limited { remaining } = cooldown_status {
        warn!(
            "Skipping eval. User {} has reached the limit of {} messages. Cooldown remaining: {:?}",
            chat_message.user_name, MESSAGE_LIMIT, remaining
        );
        return Ok(());
    }

    match meval::eval_str(&evaluation_target) {
        Ok(response) => {
            info!("Eval: {} = {}", message, response);
            keyboard.simulate(response.to_string()).await?;
            Ok(())
        }
        Err(_) => Ok(()),
    }
}
