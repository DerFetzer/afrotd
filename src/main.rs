use axum::{
    extract::{Path, State},
    headers::ContentType,
    http::StatusCode,
    routing::get,
    Router, TypedHeader,
};
use chrono::prelude::*;
use chrono_tz::{Europe::Berlin, Tz};
use clap::Parser;
use eyre::eyre;
use indexmap::IndexMap;
use maud::{html, Markup, Render, DOCTYPE};
use rand::{seq::SliceRandom, thread_rng, Rng};
use rand_pcg::Pcg64;
use rand_seeder::Seeder;
use rss::{ChannelBuilder, ItemBuilder};
use rule::{ArticleNr, Rule};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tokio::time;

mod parser;
mod rule;

const PUB_URL: &str = "https://ruleoftheday.de";

#[derive(Debug, Clone, Parser)]
struct Cli {
    rules_path: PathBuf,
    #[arg(short, long)]
    exclude_rule: Vec<ArticleNr>,
    #[arg(short, long)]
    start_date: NaiveDate,
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
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    let current_date = get_current_date();
    if cli.start_date > current_date {
        return Err(eyre!("Start date is later than current date!"));
    }

    let mut rules = parser::RulesParser::parse(&cli.rules_path)?;
    for article_nr in &cli.exclude_rule {
        rules.shift_remove(article_nr);
    }
    dbg!(rules.len());

    let rule_order = {
        let mut rng: Pcg64 = Seeder::from(&cli.start_date).make_rng();
        let mut rule_order: Vec<_> = (0..(rules.len())).collect();
        rule_order.shuffle(&mut rng);
        rule_order
    };

    let current_rule = get_rule(cli.start_date, current_date, &rules, &rule_order).clone();
    let current_rule_markup = html! { (current_rule) };

    let state = Arc::new(AppState {
        rules,
        start_date: cli.start_date,
        rule_order,
        dynamic_state: RwLock::new(DynamicState {
            current_date,
            current_rule_markup,
            rss: build_rss(&current_rule),
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
                dynamic_state.current_rule_markup = html! { (rule) };
            }
        }
    });

    let app = Router::new()
        .route("/", get(get_current_rule))
        .route("/all", get(get_all_rules))
        .route("/random", get(get_random_rule))
        .route("/rule/:article_nr", get(get_single_rule))
        .route("/rss.xml", get(rss))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
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
            .title(format!("{} {}", rule.article_nr, rule.title))
            .link(format!(
                "{}/{}",
                PUB_URL,
                rule.article_nr.to_path_parameter()
            ))
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
        head {
            (css())
            title { "Rule of the Day" }
        }
        body {
            .columns .is-flex-direction-column style="height:100vh" {
                header.column .is-narrow {
                    section.hero .is-info {
                        .hero-body {
                            p.title ."is-2" { strong { a href="/" { "Rule of the Day 🏈 🦓" } } }
                            p.subtitle ."is-4" { "Deine tägliche Dosis Regelwissen für " strong { "American Football" } " in Deutschland" }
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
                                "Sie wurden aus dem "
                                a href="https://afsvd.de/content/files/2023/01/Football_Regelbuch_2023.pdf" { "offiziellen Regelwerk des AFSVD" } " extrahiert. "
                                "Fehler können nicht ausgeschlossen werden."
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
