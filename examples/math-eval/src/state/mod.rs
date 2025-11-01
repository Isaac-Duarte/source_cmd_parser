pub mod cooldowns;
pub mod model;

pub use cooldowns::{CooldownCheck, Cooldowns};
pub use model::{State, StateHandle, StateHandleExt, StateValue};

use std::sync::Arc;

use tokio::sync::RwLock;

pub fn new_handle_with_defaults() -> StateHandle {
    Arc::new(RwLock::new(State::with_defaults()))
}
