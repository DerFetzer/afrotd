use std::sync::RwLock;

use indexmap::IndexMap;
use jiff::{Timestamp, Zoned, civil::Date, tz::TimeZone};
use maud::Markup;
use rule::{ArticleNr, Rule};
use serenity::all::CreateMessage;
use shadow_rs::shadow;

pub mod discord;
pub mod parser;
pub mod rule;

shadow!(build);

pub const PUB_URL: &str = "https://ruleoftheday.de";
pub const RSS_SVG: &str = "/res/rss.svg";
pub const OPENGRAPH_PNG: &str = "/res/opengraph.png";
pub const RULE_BOOK_URL: &str =
    "https://afsvd.de/content/files/2025/12/Football_Regelbuch_2026-1.pdf";

pub struct AppState {
    pub rules: IndexMap<ArticleNr, Rule>,
    pub start_date: Date,
    pub rule_order: Vec<usize>,
    pub dynamic_state: RwLock<DynamicState>,
}

pub struct DynamicState {
    pub current_date: Date,
    pub current_rule_markup: Markup,
    pub rss: String,
    pub discord_message: CreateMessage,
}

pub fn get_current_datetime() -> Zoned {
    Timestamp::now().to_zoned(TimeZone::get("Europe/Berlin").expect("Could not get timezone"))
}
