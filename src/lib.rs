use std::sync::RwLock;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::{Europe::Berlin, Tz};
use indexmap::IndexMap;
use maud::Markup;
use rule::{ArticleNr, Rule};
use serenity::all::CreateMessage;

pub mod discord;
pub mod parser;
pub mod rule;

pub const PUB_URL: &str = "https://ruleoftheday.de";
pub const RSS_SVG: &str = "/res/rss.svg";
pub const OPENGRAPH_PNG: &str = "/res/opengraph.png";

pub struct AppState {
    pub rules: IndexMap<ArticleNr, Rule>,
    pub start_date: NaiveDate,
    pub rule_order: Vec<usize>,
    pub dynamic_state: RwLock<DynamicState>,
}

pub struct DynamicState {
    pub current_date: NaiveDate,
    pub current_rule_markup: Markup,
    pub rss: String,
    pub discord_message: CreateMessage,
}

pub fn get_current_date() -> NaiveDate {
    get_current_datetime().date_naive()
}

pub fn get_current_datetime() -> DateTime<Tz> {
    Utc::now().with_timezone(&Berlin)
}
