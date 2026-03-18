use std::{collections::HashMap, path::PathBuf, sync::LazyLock};

use anki_bridge::prelude::*;
use clap::Parser;
use regex::Regex;

#[derive(Debug, Clone, Parser)]
struct Cli {
    ref_deck_name: String,
    deck_name: String,
    #[clap(short, long)]
    unprocessed_path: Option<PathBuf>,
}

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    let anki = AnkiClient::default();
    let ref_notes = get_deck_notes(&anki, &cli.ref_deck_name, false)?;
    let notes = get_deck_notes(&anki, &cli.deck_name, true)?;

    if let Some(unprocessed_path) = &cli.unprocessed_path {
        std::fs::create_dir_all(unprocessed_path)?;
    }

    for note in &notes {
        let rule_name = &note.fields.get("Rule").unwrap().value;
        let rule_text = &note
            .fields
            .get("Text")
            .unwrap()
            .value
            .replace("type=a", "type=\"a\"");
        if let Some(ref_note) = find_rule_note(&ref_notes, rule_name) {
            println!("Found ref_note: {rule_name}");
            let ref_rule_text = &ref_note.fields.get("Text").unwrap().value;
            let ref_rule_text_wo_cloze =
                remove_clozes(ref_rule_text).replace("type=a", "type=\"a\"");
            if &ref_rule_text_wo_cloze == rule_text {
                println!("Texts are equal");
                update_rule_text(&anki, note, ref_rule_text)?;
                set_note_processed(&anki, note, ProcessedState::Indentical)?;
                continue;
            } else {
                let clozes = get_clozes(ref_rule_text);
                println!("Clozes: {:?}", clozes);

                if clozes.is_empty() {
                    set_note_processed(&anki, note, ProcessedState::NoClozes)?;
                    continue;
                }

                let mut minor_changes = true;
                let mut changes_to_apply: Vec<Change> = vec![];

                for (content, clozes) in &clozes {
                    println!("content: {content}");
                    let rule_text_cloze_matches: Vec<_> = rule_text
                        .match_indices(content)
                        .map(|(start, content)| TextMatch {
                            start,
                            content: content.to_string(),
                        })
                        .collect();
                    let ref_rule_text_cloze_content_matches: Vec<_> =
                        ref_rule_text.match_indices(content).collect();

                    let mut ref_rule_text_cloze_matches_consum =
                        ref_rule_text_cloze_content_matches.clone();
                    ref_rule_text_cloze_matches_consum.sort();
                    ref_rule_text_cloze_matches_consum.reverse();

                    let mut clozes = clozes.clone();
                    clozes.sort();
                    let mut option_clozes = vec![];

                    println!("clozes: {clozes:?}");
                    println!("consume: {ref_rule_text_cloze_matches_consum:?}");

                    for cloze in &clozes {
                        while let Some((start, _)) =
                            ref_rule_text_cloze_matches_consum.pop_if(|m| m.0 < cloze.start)
                        {
                            option_clozes.push(ClozeOption::None(start));
                        }
                        option_clozes.push(ClozeOption::Cloze(cloze.clone()));
                        ref_rule_text_cloze_matches_consum.pop().unwrap();
                    }
                    while let Some((start, _)) = ref_rule_text_cloze_matches_consum.pop() {
                        option_clozes.push(ClozeOption::None(start));
                    }

                    println!(
                        "ref_rule_text_cloze_content_matches: {ref_rule_text_cloze_content_matches:?}"
                    );
                    println!("option_clozes: {option_clozes:?}");
                    println!("consume: {ref_rule_text_cloze_matches_consum:?}");
                    assert_eq!(
                        ref_rule_text_cloze_content_matches.len(),
                        option_clozes.len()
                    );

                    if rule_text_cloze_matches.len() == option_clozes.len() {
                        println!("Same amount of instances in rule");
                        changes_to_apply.push(Change {
                            cloze_text: content.to_string(),
                            clozes: option_clozes,
                            text_matches: rule_text_cloze_matches,
                        });
                        continue;
                    } else {
                        println!(
                            "Different amount of instances in rule ->\n{rule_text_cloze_matches:?}\n{clozes:?}"
                        );
                    }
                    minor_changes = false;
                }
                if minor_changes {
                    println!("Only minor changes");
                    println!("{changes_to_apply:#?}");
                    println!();
                    let new_rule_text = apply_minor_changes_to_text(&changes_to_apply, rule_text);
                    println!("{rule_text}");
                    println!();
                    println!("{new_rule_text}");

                    update_rule_text(&anki, note, &new_rule_text)?;
                    set_note_processed(&anki, note, ProcessedState::MinorChanges)?;
                    continue;
                }
            }
            if let Some(unprocessed_path) = &cli.unprocessed_path {
                std::fs::write(
                    unprocessed_path.join(format!("{rule_name}.html")),
                    ref_rule_text,
                )?;
            }
        } else {
            println!("Could not find ref_note: {rule_name}");
        }
    }
    println!();
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ClozeOption {
    Cloze(Cloze),
    None(usize),
}

impl ClozeOption {
    fn get_start(&self) -> usize {
        match self {
            Self::Cloze(cloze) => cloze.start,
            Self::None(start) => *start,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Cloze {
    start: usize,
    id: u32,
    cloze: String,
    cloze_content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TextMatch {
    start: usize,
    content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Change {
    cloze_text: String,
    clozes: Vec<ClozeOption>,
    text_matches: Vec<TextMatch>,
}

fn get_deck_notes(
    anki: &AnkiClient,
    deck_name: &str,
    omit_processed: bool,
) -> eyre::Result<Vec<NotesInfoResponse>> {
    let note_ids = anki.request(FindNotesRequest {
        query: format!(
            "deck:\"{deck_name}\" {}",
            if omit_processed { "-tag:Processed" } else { "" }
        ),
    })?;
    Ok(anki.request(NotesInfoRequest { notes: note_ids.0 })?)
}

fn find_rule_note<'a>(
    notes: &'a [NotesInfoResponse],
    rule_name: &str,
) -> Option<&'a NotesInfoResponse> {
    notes.iter().find(|n| {
        n.fields
            .get("Rule")
            .map(|f| f.value == rule_name)
            .unwrap_or(false)
    })
}

#[derive(Clone, Copy)]
enum ProcessedState {
    Indentical,
    MinorChanges,
    NoClozes,
}

fn set_note_processed(
    anki: &AnkiClient,
    note: &NotesInfoResponse,
    state: ProcessedState,
) -> eyre::Result<()> {
    let mut new_tags = note.tags.clone();
    new_tags.push("Processed".to_string());
    new_tags.push(
        match state {
            ProcessedState::Indentical => "Identical",
            ProcessedState::MinorChanges => "MinorChanges",
            ProcessedState::NoClozes => "NoClozes",
        }
        .to_string(),
    );
    Ok(anki.request(UpdateNoteTagsRequest {
        note: note.note_id,
        tags: new_tags,
    })?)
}

fn update_rule_text(anki: &AnkiClient, note: &NotesInfoResponse, text: &str) -> eyre::Result<()> {
    let mut new_fields: HashMap<_, _> = note
        .fields
        .iter()
        .map(|(k, fr)| (k.clone(), fr.value.clone()))
        .collect();
    new_fields.insert("Text".to_string(), text.to_string());
    Ok(anki.request(UpdateNoteFieldsRequest {
        note: UpdateNoteFieldsEntry {
            id: note.note_id,
            fields: new_fields,
            audio: vec![],
            video: vec![],
            picture: vec![],
        },
    })?)
}

fn get_clozes(s: &str) -> HashMap<String, Vec<Cloze>> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{c([0-9])+::(.+?)\}\}").unwrap());
    let clozes: Vec<_> = RE
        .captures_iter(s)
        .map(|c| {
            (
                c.get(1).unwrap().as_str().parse::<u32>().unwrap(),
                c.get(2).unwrap().range(),
                c.extract(),
            )
        })
        .map(|(id, range, (cloze, [_, content]))| Cloze {
            id,
            start: range.start,
            cloze: cloze.to_string(),
            cloze_content: content.to_string(),
        })
        .collect();
    let mut clozes_map = HashMap::new();
    for cloze in clozes {
        clozes_map
            .entry(cloze.cloze_content.clone())
            .or_insert(vec![])
            .push(cloze);
    }
    clozes_map
}

fn remove_clozes(s: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{c[0-9]+::").unwrap());
    let s = s.replace("}}", "");
    RE.replace_all(&s, "").into_owned()
}

fn apply_minor_changes_to_text(changes: &[Change], text: &str) -> String {
    let mut s = String::new();
    assert!(
        changes
            .iter()
            .all(|c| c.clozes.len() == c.text_matches.len())
    );
    let mut clozes: Vec<_> = changes.iter().flat_map(|c| &c.clozes).collect();
    clozes.sort_by_key(|a| a.get_start());
    let mut text_matches: Vec<_> = changes.iter().flat_map(|c| &c.text_matches).collect();
    text_matches.sort();

    assert_eq!(clozes.len(), text_matches.len());

    println!("{clozes:?}");
    println!("{text_matches:?}");

    let mut current_position = 0;

    for (cloze, text_match) in clozes.iter().zip(text_matches.iter()) {
        println!("{cloze:?}");
        println!("{text_match:?}");
        println!();
        let ClozeOption::Cloze(cloze) = cloze else {
            continue;
        };
        s.push_str(&text[current_position..text_match.start]);
        let cloze_text = format!("{{{{c{}::{}}}}}", cloze.id, text_match.content);
        s.push_str(&cloze_text);
        current_position = text_match.start + text_match.content.len();
    }
    s.push_str(&text[current_position..]);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_clozes() {
        let input = r#"<div class="header">2.25.11 Postscrimmage Kick Spot</div><div class="rule"><div class="content"><p>Der Postscrimmage Kick Spot dient als Basic Spot, wenn die Postscrimmage Kick Durchführung zutrifft (Regel 10.2.3).</p><ol type="a"><li>Endet der Kick, außer in den nachfolgend aufgeführten Spezialfällen, im Spielfeld,ist der Postscrimmage Kick Spot der Punkt, {{c1::an dem der Kick endet.}}</li><li>Wenn der Kick in {{c3::Team B’s Endzone}}&nbsp;endet, ist der Postscrimmage Kick Spot {{c4::Team B’s 20-Meterlinie}}. Spezialfälle:</li><ol type="1"><li> Bleibt der Ball bei einem {{c5::erfolglosen Fieldgoalversuch}}&nbsp;{{c9::unberührt von Team B, nachdem er die neutrale Zone überquert hat}}, und wird {{c6::jenseits der neutralen Zone}} für dead erklärt, dann ist der Postscrimmage Kick Spot:</li><ol type="a"><li> der Previous Spot, wenn dieser sich auf oder außerhalb {{c7::Team B’s 20-Meterlinie}} befand (A.R. 10.2.3.V).</li><li> Team B’s 20-Meterlinie, wenn der Previous Spot sich {{c8::zwischen Team B’s 20-Meterlinie und dessen Goalline}} befand.</li></ol><li> Wenn Regel 6.3.11 zutrifft, ist der Postscrimmage Kick Spot Team B’s 20-Meterlinie.</li><li> Wenn Regel 6.5.1.b zutrifft, ist der Postscrimmage Kick Spot der Punkt, an dem der Receiver zuerst den Kick berührt.</li></ol></ol></div></div><div class="link"><a href="https://afsvd.de/content/files/2024/12/Football_Regelbuch_2025.pdf#subsection.1.2.25.11" target="_blank" rel="noreferrer noopener">Offizielles Regelwerk</a></div>"#;
        let exp = r#"<div class="header">2.25.11 Postscrimmage Kick Spot</div><div class="rule"><div class="content"><p>Der Postscrimmage Kick Spot dient als Basic Spot, wenn die Postscrimmage Kick Durchführung zutrifft (Regel 10.2.3).</p><ol type="a"><li>Endet der Kick, außer in den nachfolgend aufgeführten Spezialfällen, im Spielfeld,ist der Postscrimmage Kick Spot der Punkt, an dem der Kick endet.</li><li>Wenn der Kick in Team B’s Endzone&nbsp;endet, ist der Postscrimmage Kick Spot Team B’s 20-Meterlinie. Spezialfälle:</li><ol type="1"><li> Bleibt der Ball bei einem erfolglosen Fieldgoalversuch&nbsp;unberührt von Team B, nachdem er die neutrale Zone überquert hat, und wird jenseits der neutralen Zone für dead erklärt, dann ist der Postscrimmage Kick Spot:</li><ol type="a"><li> der Previous Spot, wenn dieser sich auf oder außerhalb Team B’s 20-Meterlinie befand (A.R. 10.2.3.V).</li><li> Team B’s 20-Meterlinie, wenn der Previous Spot sich zwischen Team B’s 20-Meterlinie und dessen Goalline befand.</li></ol><li> Wenn Regel 6.3.11 zutrifft, ist der Postscrimmage Kick Spot Team B’s 20-Meterlinie.</li><li> Wenn Regel 6.5.1.b zutrifft, ist der Postscrimmage Kick Spot der Punkt, an dem der Receiver zuerst den Kick berührt.</li></ol></ol></div></div><div class="link"><a href="https://afsvd.de/content/files/2024/12/Football_Regelbuch_2025.pdf#subsection.1.2.25.11" target="_blank" rel="noreferrer noopener">Offizielles Regelwerk</a></div>"#;

        assert_eq!(remove_clozes(input), exp);
    }
}
