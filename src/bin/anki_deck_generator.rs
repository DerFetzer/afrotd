use afrotd::{parser::RulesParser, rule::RuleInterpretation};
use clap::Parser;
use eyre::eyre;
use genanki_rs::{Deck, Field, Model, Note, Template};
use maud::{html, Markup};
use roman_numerals::ToRoman;
use std::path::PathBuf;

const CSS: &str = include_str!("../../res/anki.css");

#[derive(Debug, Clone, Parser)]
struct Cli {
    rules_path: PathBuf,
    #[arg(short, long)]
    rules_url: String,
    #[arg(short, long)]
    year: u16,
    #[arg(short, long)]
    output_path: Option<PathBuf>,
}

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    let interpretations: Vec<_> = RulesParser::parse_interpretations(&cli.rules_path)?
        .into_values()
        .flatten()
        .collect();

    let (model_id, deck_id) = get_model_and_deck_ids(cli.year)?;

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
        vec![Template::new("Interpretation")
            .qfmt("{{Interpretation}}")
            .afmt("{{Interpretation}}<hr id=answer>{{Regelung}}")],
    )
    .css(CSS);

    for interpretation in interpretations {
        deck.add_note(Note::new(
            model.clone(),
            vec![
                &interpretation.get_title(),
                &render_interpretation(&interpretation).into_string(),
                &render_ruling(&interpretation, &cli).into_string(),
            ],
        )?);
    }

    deck.write_to_file(
        &cli.output_path
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(format!("AFVD_{}_Interpretationen.apkg", cli.year)),
    )?;

    Ok(())
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

fn get_model_and_deck_ids(year: u16) -> eyre::Result<(i64, i64)> {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const ID_BASE_DECK: i64 = 63754 * 10_000_000_000_000;
    const ID_BASE_MODEL: i64 = 63755 * 10_000_000_000_000;

    if let [major, minor, patch] = &VERSION.split('.').collect::<Vec<_>>().as_slice() {
        let major: i64 = major.parse()?;
        let minor: i64 = minor.parse()?;
        let patch: i64 = patch.parse()?;
        let common_id = year as i64 + patch * 10_000 + minor * 10_000_000 + major * 10_000_000_000;
        Ok((common_id + ID_BASE_DECK, common_id + ID_BASE_MODEL))
    } else {
        Err(eyre!("Could not parse crate version"))
    }
}
