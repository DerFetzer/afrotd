use afrotd::{
    parser::RulesParser,
    rule::{ArticleNr, Rule, RuleInterpretation},
};
use clap::{Parser, ValueEnum};
use eyre::eyre;
use genanki_rs::{Deck, Field, Model, Note, Template};
use maud::{Markup, html};
use roman_numerals::ToRoman;
use std::path::PathBuf;

const CSS: &str = include_str!("../../res/anki.css");

#[derive(Debug, Copy, Clone, Default, ValueEnum)]
enum DeckType {
    RulesTemplates,
    #[default]
    Interpretations,
}

#[derive(Debug, Clone, Parser)]
struct Cli {
    rules_path: PathBuf,
    #[arg(short, long)]
    rules_url: String,
    #[arg(short, long)]
    year: u16,
    #[arg(short, long)]
    output_path: Option<PathBuf>,
    #[arg(short, long)]
    deck_type: Option<DeckType>,
}

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    let (model_id, deck_id) = get_model_and_deck_ids(cli.year, cli.deck_type.unwrap_or_default())?;

    let deck = match cli.deck_type.unwrap_or_default() {
        DeckType::RulesTemplates => create_rules_template_deck(&cli, model_id, deck_id)?,
        DeckType::Interpretations => create_interpretations_deck(&cli, model_id, deck_id)?,
    };

    deck.write_to_file(
        &cli.output_path
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(format!(
                "AFVD_{}_{}.apkg",
                cli.year,
                match cli.deck_type.unwrap_or_default() {
                    DeckType::RulesTemplates => "Regeln",
                    DeckType::Interpretations => "Interpretationen",
                }
            )),
    )?;

    Ok(())
}

fn create_interpretations_deck(cli: &Cli, model_id: i64, deck_id: i64) -> eyre::Result<Deck> {
    let interpretations: Vec<_> = RulesParser::parse_interpretations(&cli.rules_path)?
        .into_values()
        .flatten()
        .collect();
    let mut deck = Deck::new(
        deck_id,
        &format!(
            "American Football in Deutschland: Interpretationen ({})",
            cli.year
        ),
        &format!(
            "Interpretationen aus dem AFVD American Football Regelwerk von {}",
            cli.year
        ),
    );

    let model = Model::new(
        model_id,
        "Interpretation",
        vec![
            Field::new("InterpretationNr"),
            Field::new("Interpretation"),
            Field::new("Regelung"),
        ],
        vec![
            Template::new("Interpretation")
                .qfmt("{{Interpretation}}")
                .afmt("{{Interpretation}}<hr id=answer>{{Regelung}}"),
        ],
    )
    .css(CSS);

    for interpretation in interpretations {
        deck.add_note(
            Note::new(
                model.clone(),
                vec![
                    &interpretation.get_title(),
                    &render_interpretation(&interpretation).into_string(),
                    &render_ruling(&interpretation, cli).into_string(),
                ],
            )?
            .tags(article_nr_to_tags(interpretation.article_nr)),
        );
    }

    Ok(deck)
}

fn create_rules_template_deck(cli: &Cli, model_id: i64, deck_id: i64) -> eyre::Result<Deck> {
    let interpretations: Vec<_> = RulesParser::parse(&cli.rules_path)?.into_values().collect();
    let mut deck = Deck::new(
        deck_id,
        &format!("American Football in Deutschland: Regeln ({})", cli.year),
        &format!(
            "Regeln aus dem AFVD American Football Regelwerk von {}",
            cli.year
        ),
    );

    let model = Model::new(
        model_id,
        "Regel",
        vec![
            Field::new("Rule"),
            Field::new("Text"),
            Field::new("Back Extra"),
        ],
        vec![
            Template::new("Regel")
                .qfmt("{{cloze:Text}}")
                .afmt("{{cloze:Text}}<br>{{Back Extra}}"),
        ],
    )
    .model_type(genanki_rs::ModelType::Cloze)
    .css(CSS);

    for rule in interpretations {
        deck.add_note(
            Note::new(
                model.clone(),
                vec![
                    &rule.to_title(),
                    &render_rule(&rule).into_string(),
                    &render_back_extra(&rule, cli).into_string(),
                ],
            )?
            .tags(article_nr_to_tags(rule.article_nr)),
        );
    }

    Ok(deck)
}

fn render_rule(rule: &Rule) -> Markup {
    html! {
        div.header {
            (rule.to_title())
        }
        div.rule {
            (rule.render_text())
        }
    }
}

fn render_back_extra(rule: &Rule, cli: &Cli) -> Markup {
    html! {
        div.link {
            a href=(format!("{}#{}", cli.rules_url, rule.article_nr.to_pdf_destination()))
              target="_blank" rel="noreferrer noopener" {
                "Offizielles Regelwerk"
            }
        }
    }
}

fn render_interpretation(interpretation: &RuleInterpretation) -> Markup {
    html! {
        div.header {
            "A.R. " (interpretation.article_nr) "." (interpretation.index.to_roman())
        }
        div.situation {
            (interpretation.text)
        }
    }
}

fn render_ruling(interpretation: &RuleInterpretation, cli: &Cli) -> Markup {
    html! {
        div.ruling {
            "Regelung:"
        }
        div.ruling-text {
            (interpretation.ruling)
        }
        div.link {
            a href=(format!("{}#{}", cli.rules_url, interpretation.article_nr.to_pdf_destination()))
              target="_blank" rel="noreferrer noopener" {
                "Offizielles Regelwerk"
            }
        }
    }
}

fn article_nr_to_tags(article_nr: ArticleNr) -> [String; 2] {
    [
        format!("AmericanFootball::Regel{}", article_nr.0),
        format!(
            "AmericanFootball::Abschnitt{}.{}",
            article_nr.0, article_nr.1
        ),
    ]
}

fn get_model_and_deck_ids(year: u16, deck_type: DeckType) -> eyre::Result<(i64, i64)> {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const BASE_ID: i64 = 63754;
    let deck_type_offset = match deck_type {
        DeckType::RulesTemplates => 2,
        DeckType::Interpretations => 0,
    };
    let id_base_deck = (BASE_ID + deck_type_offset) * 10_000_000_000_000;
    let id_base_model = (BASE_ID + deck_type_offset + 1) * 10_000_000_000_000;

    if let [major, minor, patch] = &VERSION.split('.').collect::<Vec<_>>().as_slice() {
        let major: i64 = major.parse()?;
        let minor: i64 = minor.parse()?;
        let patch: i64 = patch.parse()?;
        let common_id = year as i64 + patch * 10_000 + minor * 10_000_000 + major * 10_000_000_000;
        Ok((common_id + id_base_deck, common_id + id_base_model))
    } else {
        Err(eyre!("Could not parse crate version"))
    }
}
