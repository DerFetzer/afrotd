use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Router,
};
use axum_extra::{headers::ContentType, TypedHeader};
use chrono::prelude::*;
use chrono_tz::{Europe::Berlin, Tz};
use clap::{Args, Parser};
use eyre::eyre;
use indexmap::IndexMap;
use maud::{html, Markup, Render, DOCTYPE};
use rand::{seq::SliceRandom, thread_rng, Rng};
use rand_pcg::Pcg64;
use rand_seeder::Seeder;
use rss::{ChannelBuilder, ItemBuilder};
use serenity::{all::GatewayIntents, builder::CreateMessage, Client};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, RwLock},
};
use tokio::time;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use veil::Redact;

use discord::{build_discord_message, DiscordEventHandler};
use rule::{ArticleNr, Rule};

mod discord;
mod parser;
mod rule;

const PUB_URL: &str = "https://ruleoftheday.de";
const RSS_SVG: &str = "/res/rss.svg";
const OPENGRAPH_PNG: &str = "/res/opengraph.png";

#[derive(Debug, Clone, Parser)]
struct Cli {
    rules_path: PathBuf,
    #[arg(short, long)]
    exclude_rule: Vec<ArticleNr>,
    #[arg(short, long)]
    start_date: NaiveDate,
    #[command(flatten)]
    discord_args: DiscordArgs,
}

#[derive(Redact, Clone, Args)]
#[group(required = false, multiple = true, requires_all = ["discord_token", "discord_post_hour", "discord_channel_id"])]
struct DiscordArgs {
    #[redact(fixed = 10)]
    #[arg(long)]
    discord_token: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..23))]
    discord_post_hour: Option<u8>,
    #[arg(long)]
    discord_channel_id: Option<u64>,
}

struct AppState {
    rules: IndexMap<ArticleNr, Rule>,
    start_date: NaiveDate,
    rule_order: Vec<usize>,
    dynamic_state: RwLock<DynamicState>,
}

struct DynamicState {
    current_date: NaiveDate,
    current_rule_markup: Markup,
    rss: String,
    discord_message: CreateMessage,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // axum logs rejections from built-in extractors with the `axum::rejection`
                // target, at `TRACE` level. `axum::rejection=trace` enables showing those events
                "afrotd=info,tower_http=debug,axum::rejection=trace".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let cli = Cli::parse();

    info!("{cli:?}");

    let current_date = get_current_date();
    if cli.start_date > current_date {
        return Err(eyre!("Start date is later than current date!"));
    }

    let mut rules = parser::RulesParser::parse(&cli.rules_path)?;
    info!("Parsed {} rules", rules.len());
    for article_nr in &cli.exclude_rule {
        rules.shift_remove(article_nr);
    }
    info!("{} rules after exclusion", rules.len());

    let rule_order = {
        let mut rng: Pcg64 = Seeder::from(&cli.start_date).make_rng();
        let mut rule_order: Vec<_> = (0..(rules.len())).collect();
        rule_order.shuffle(&mut rng);
        rule_order
    };
    debug!("Rule order: {:?}", rule_order);

    let current_rule = get_rule(cli.start_date, current_date, &rules, &rule_order).clone();
    info!("Current rule: {}", current_rule.article_nr);
    let current_rule_markup = html! { (current_rule) };

    let state = Arc::new(AppState {
        rules,
        start_date: cli.start_date,
        rule_order,
        dynamic_state: RwLock::new(DynamicState {
            current_date,
            current_rule_markup,
            rss: build_rss(&current_rule),
            discord_message: build_discord_message(&current_rule),
        }),
    });

    // Task for updating the state when the date changes
    let task_state = state.clone();
    tokio::spawn(async move {
        let state = task_state;
        let mut interval = time::interval(time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let current_date = get_current_date();
            if current_date != state.dynamic_state.read().unwrap().current_date {
                let mut dynamic_state = state.dynamic_state.write().unwrap();
                dynamic_state.current_date = current_date;
                let rule = get_rule(
                    state.start_date,
                    dynamic_state.current_date,
                    &state.rules,
                    &state.rule_order,
                );
                info!("New rule: {}", rule.article_nr);
                dynamic_state.rss = build_rss(rule);
                dynamic_state.discord_message = build_discord_message(rule);
                dynamic_state.current_rule_markup = html! { (rule) };
            }
        }
    });

    // Discord task
    if let (Some(discord_token), Some(discord_post_hour), Some(discord_channel_id)) = (
        cli.discord_args.discord_token,
        cli.discord_args.discord_post_hour,
        cli.discord_args.discord_channel_id,
    ) {
        info!("Init discord bot");

        let task_state = state.clone();
        let intents = GatewayIntents::GUILDS;

        let mut client = Client::builder(&discord_token, intents)
            .event_handler(DiscordEventHandler {
                is_loop_running: AtomicBool::new(false),
                app_state: task_state,
                discord_post_hour,
                discord_channel_id,
            })
            .await?;

        tokio::spawn(async move {
            match client.start().await {
                Ok(_) => warn!("Discord client exìted!"),
                Err(err) => error!("Could not start discord client: {err}"),
            }
        });
    }

    let app = Router::new()
        .route("/", get(get_current_rule))
        .route("/all", get(get_all_rules))
        .route("/random", get(get_random_rule))
        .route("/rule/:article_nr", get(get_single_rule))
        .route("/rss.xml", get(rss))
        .route("/health", get(|| async { "OK" }))
        .nest_service("/res", ServeDir::new("res"))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
    Ok(())
}

fn get_rule<'a>(
    start_date: NaiveDate,
    current_date: NaiveDate,
    rules: &'a IndexMap<ArticleNr, Rule>,
    rule_order: &'a [usize],
) -> &'a Rule {
    let days_since_start = (current_date - start_date).num_days();
    assert!(days_since_start >= 0);
    &rules[rule_order[days_since_start as usize % rules.len()]]
}

fn get_current_date() -> NaiveDate {
    get_current_datetime().date_naive()
}

fn get_current_datetime() -> DateTime<Tz> {
    Utc::now().with_timezone(&Berlin)
}

fn build_rss(rule: &Rule) -> String {
    let now = get_current_datetime().to_rfc2822();
    ChannelBuilder::default()
        .title("Rule of the Day")
        .link(PUB_URL)
        .description("Deine tägliche Dosis Regelwissen für American Football in Deutschland")
        .language("de".to_string())
        .last_build_date(now.clone())
        .items(vec![ItemBuilder::default()
            .title(rule.to_title())
            .link(rule.to_url(PUB_URL))
            .description(rule.to_description())
            .build()])
        .build()
        .to_string()
}

fn css() -> Markup {
    html! {
        link rel="stylesheet" type="text/css"
            href="https://cdn.jsdelivr.net/npm/bulma@0.9.4/css/bulma.min.css";
    }
}

fn insert_content_to_site(content: &dyn Render) -> Markup {
    html! {
        (DOCTYPE)
        html lang="de" {
            head {
                (css())
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Rule of the Day" }
                meta name="description" property="description" content="Deine tägliche Dosis Regelwissen für American Football in Deutschland";
                meta property="og:title" content="Rule of the Day";
                meta property="og:description" content="Deine tägliche Dosis Regelwissen für American Football in Deutschland";
                meta property="og:url" content=(PUB_URL);
                meta property="og:image" content=(OPENGRAPH_PNG);
                meta property="og:locale" content="de_DE";
                style {
                    "summary {
                        cursor:pointer;
                        margin: 12px 0 6px;
                    }"
                }
            }
            body {
                .columns .is-flex-direction-column style="height:100vh" {
                    header.column .is-narrow {
                        section.hero .is-info {
                            .hero-body {
                                p { a href="/rss.xml" { img src=(RSS_SVG) height="32" width="32" alt="RSS Feed"; } }
                                p.title ."is-2" { strong { a href="/" { "Rule of the Day 🏈 🦓" } } }
                                p.subtitle ."is-4" {
                                    "Deine tägliche Dosis Regelwissen für " strong { "American Football" } " in Deutschland "
                                }
                            }
                        }
                    }
                    .column {
                        (content)
                    }
                    footer.column .is-narrow {
                        footer.footer {
                            .content .has-text-centered {
                                p {
                                    strong { "Rule of the Day" } " von " a href="https://github.com/DerFetzer" { "DerFetzer" } "."
                                }
                                p {
                                    "Der " a href="https://github.com/DerFetzer/afrotd"{ "Quellcode" } " steht unter der "
                                    a href="https://opensource.org/licenses/mit-license.php" { "MIT" } " Lizenz."
                                }
                                p .has-text-grey {
                                    strong { "Disclaimer: "} "Diese Seite soll eine Möglichkeit bieten, "
                                    "sich regelmäßig mit den Regeln im American Football in Deutschland zu beschäftigen."
                                    br;
                                    "Sie wurden unter freundlicher Genehmigung des " a href="https://afvd.de" { "AFVD" } " aus dem "
                                    a href="https://afsvd.de/content/files/2024/12/Football_Regelbuch_2025.pdf" { "offiziellen Regelwerk des AFSVD" } " extrahiert. "
                                    "Fehler können nicht ausgeschlossen werden."
                                    br;
                                    "Die Verarbeitung der Inhalte auf dieser Website ist nur nach schriftlicher Genehmigung des AFVD zulässig."
                                }
                                p .has-text-grey-light {
                                    a .has-text-grey-light href="https://legal.matthias-fetzer.de/"
                                        target="_blank" rel="noreferrer noopener" { "Impressum" }
                                    " • "
                                    a .has-text-grey-light href="https://legal.matthias-fetzer.de/privacy.html"
                                        target="_blank" rel="noreferrer noopener" { "Datenschutz" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn get_random_rule(State(state): State<Arc<AppState>>) -> Markup {
    let mut rng = thread_rng();
    let rules = &state.rules;
    let rule_index = rng.gen_range(0..rules.len() - 1);
    let rule = &rules[rule_index];
    insert_content_to_site(&html! {
        .container {
            .block { (rule) }
        }
    })
}

async fn get_current_rule(State(state): State<Arc<AppState>>) -> Markup {
    let rule = &state.dynamic_state.read().unwrap().current_rule_markup;
    insert_content_to_site(&html! {
        .container {
            .block { (rule) }
        }
    })
}

async fn get_single_rule(
    State(state): State<Arc<AppState>>,
    Path(article_nr): Path<String>,
) -> Result<Markup, StatusCode> {
    let article_nr =
        ArticleNr::from_path_paramter(article_nr).map_err(|_| StatusCode::BAD_REQUEST)?;
    let rules = &state.rules;
    let rule = rules.get(&article_nr).ok_or(StatusCode::NOT_FOUND)?;
    Ok(insert_content_to_site(&html! {
        .container {
            .block { (rule) }
        }
    }))
}

async fn get_all_rules(State(state): State<Arc<AppState>>) -> Markup {
    let rules = &state.rules;
    insert_content_to_site(&html! {
        .container {
            @for (_article, rule) in rules.iter() {
                .block { (rule) }
            }
        }
    })
}

async fn rss(State(state): State<Arc<AppState>>) -> (TypedHeader<ContentType>, String) {
    (
        TypedHeader("application/rss+xml".parse().unwrap()),
        state.dynamic_state.read().unwrap().rss.clone(),
    )
}
