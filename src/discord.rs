use crate::rule::Rule;
use crate::{get_current_datetime, AppState};
use crate::{OPENGRAPH_PNG, PUB_URL};

use chrono::{prelude::*, NaiveDate};
use serenity::builder::{CreateEmbed, CreateMessage};
use serenity::{
    all::{ChannelId, GuildId, Ready},
    client::{Context, EventHandler},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::time;
use tracing::{error, info};

pub struct DiscordEventHandler {
    pub is_loop_running: AtomicBool,
    pub app_state: Arc<AppState>,
    pub discord_post_hour: u8,
    pub discord_channel_id: u64,
}

#[serenity::async_trait]
impl EventHandler for DiscordEventHandler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("{} is connected", ready.user.name);
    }

    async fn cache_ready(&self, ctx: Context, _guilds: Vec<GuildId>) {
        info!("Cache built successfully!");

        if !self.is_loop_running.load(Ordering::Relaxed) {
            let discord_post_hour = self.discord_post_hour;
            let discord_channel_id = self.discord_channel_id;
            let app_state = self.app_state.clone();
            tokio::spawn(async move {
                let mut interval = time::interval(time::Duration::from_secs(30));
                let mut last_send_date = NaiveDate::MIN;
                loop {
                    interval.tick().await;
                    let current_date_time = get_current_datetime();

                    if current_date_time.hour() as u8 == discord_post_hour
                        && current_date_time.minute() == 0
                        && current_date_time.date_naive() - last_send_date
                            > chrono::TimeDelta::zero()
                    {
                        info!("Send message");

                        let message = {
                            let dynamic_state = app_state.dynamic_state.read().unwrap();
                            dynamic_state.discord_message.clone()
                        };
                        if let Err(err) = ChannelId::new(discord_channel_id)
                            .send_message(&ctx, message)
                            .await
                        {
                            error!("Could not send message: {err}");
                        }
                        last_send_date = current_date_time.date_naive();
                    }
                }
            });

            // Now that the loop is running, we set the bool to true
            self.is_loop_running.swap(true, Ordering::Relaxed);
        }
    }
}

pub fn build_discord_message(rule: &Rule) -> CreateMessage {
    let embed = CreateEmbed::new()
        .title(rule.to_title())
        .url(rule.to_url(PUB_URL))
        .image(format!("{PUB_URL}{OPENGRAPH_PNG}"))
        .description(rule.to_description());
    CreateMessage::new().embed(embed)
}
