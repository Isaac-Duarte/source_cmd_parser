use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Cooldowns {
    users: HashMap<String, UserCooldown>,
}

#[derive(Default)]
struct UserCooldown {
    timestamps: Vec<Instant>,
}

pub enum CooldownCheck {
    Allowed,
    Limited { remaining: Duration },
}

impl Cooldowns {
    pub fn check_and_record(
        &mut self,
        user: &str,
        limit: usize,
        window: Duration,
    ) -> CooldownCheck {
        let cooldown = self
            .users
            .entry(user.to_owned())
            .or_insert_with(UserCooldown::default);

        cooldown
            .timestamps
            .retain(|timestamp| timestamp.elapsed() < window);

        if cooldown.timestamps.len() >= limit {
            let elapsed = cooldown
                .timestamps
                .first()
                .map(|ts| ts.elapsed())
                .unwrap_or_default();

            let remaining = window
                .checked_sub(elapsed)
                .unwrap_or_else(|| Duration::from_secs(0));

            CooldownCheck::Limited { remaining }
        } else {
            cooldown.timestamps.push(Instant::now());
            CooldownCheck::Allowed
        }
    }
}
