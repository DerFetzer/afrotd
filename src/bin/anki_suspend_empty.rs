use anki_bridge::prelude::*;
use clap::Parser;

#[derive(Debug, Clone, Parser)]
struct Cli {
    deck_name: String,
}

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    let anki = AnkiClient::default();

    let card_ids = anki.request(FindCardsRequest {
        query: format!("\"deck:{}\"", cli.deck_name),
    })?;

    let cards = anki.request(CardsInfoRequest { cards: card_ids })?;
    dbg!(&cards);

    let empty_cards: Vec<_> = cards
        .iter()
        .filter(|c| !c.fields.get("Text").unwrap().value.contains("{{c"))
        .map(|c| c.card_id)
        .collect();

    dbg!(&empty_cards);

    anki.request(SuspendRequest { cards: empty_cards })?;

    Ok(())
}
