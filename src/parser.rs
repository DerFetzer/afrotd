use eyre::{eyre, Context};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use std::{path::Path, process::Command};

use crate::rule::{ArticleNr, Rule};

pub struct RulesParser;

impl RulesParser {
    pub fn parse(rules_path: &Path) -> eyre::Result<IndexMap<ArticleNr, Rule>> {
        let pdftotext_output = Command::new("pdftotext")
            .args([
                rules_path
                    .to_str()
                    .ok_or(eyre!("Invalid path to rules pdf"))?,
                "-",
            ])
            .output()
            .wrap_err("Could not run pdftotext")?;
        if !pdftotext_output.status.success() {
            return Err(eyre!(
                "pdftotext did not exit successfuly: {}",
                String::from_utf8(pdftotext_output.stderr)?
            ));
        }
        let mut rules_text =
            String::from_utf8(pdftotext_output.stdout).wrap_err("Stdout is no valid utf8")?;

        // Treat section 9.1. as rule
        rules_text = rules_text.replace(
            "Abschnitt 9.1. Persönliche Fouls\nAlle",
            "Artikel 9.1.0. Persönliche Fouls\nAlle",
        );
        rules_text = rules_text.replace("Regel 9\nVerhalten von Spielern und anderen", "");

        let re_new_page = Regex::new(r"\n\x0C.*\n\n.*\n\n").unwrap();
        let re_section = Regex::new(r"(?m)^Abschnitt .*$").unwrap();
        let re_new_chapter = Regex::new(r"\n\x0C.*\n.*\n\n").unwrap();

        rules_text = re_new_page.replace_all(&rules_text, "").to_string();
        rules_text = re_section.replace_all(&rules_text, "").to_string();
        rules_text = re_new_chapter.replace_all(&rules_text, "").to_string();

        let rules_start = rules_text
            .find("Artikel 1.1.1.")
            .ok_or(eyre!("Could not find 'Artikel 1.1.1.' inside the pdf text"))?;
        let rules_end = rules_text
            .find("Zusammenfassung der Strafen\nDie")
            .ok_or(eyre!(
                "Could not find 'Zusammenfassung der Strafen' inside the pdf text"
            ))?;

        let mut rules = IndexMap::new();

        let re_article_header =
            Regex::new(r"(?m)^Artikel (?<article_nr>\d+\.\d+\.\d+\.) (?<title>.*)$").unwrap();

        let rules_part = &rules_text[rules_start..rules_end];

        let captures: Vec<_> = re_article_header.captures_iter(rules_part).collect();

        let mut last_captures = None;

        for (article_header, next) in captures.iter().zip(captures.iter().skip(1)) {
            last_captures = Some(next);
            let rule = Self::extract_rule_from_text(
                rules_part,
                article_header,
                next.get(0).unwrap().start(),
            )?;
            rules.insert(rule.article_nr, rule);
        }
        if let Some(last_capture) = last_captures {
            let end_of_last_capture = last_capture.get(0).unwrap().end();
            let rules_end = rules_part[end_of_last_capture..]
                .find("\n\n")
                .ok_or(eyre!("Could not find end of rules"))?
                + end_of_last_capture;
            let rule = Self::extract_rule_from_text(rules_part, last_capture, rules_end)?;
            rules.insert(rule.article_nr, rule);
        }
        // dbg!(&rules);
        dbg!(rules.len());

        Ok(rules)
    }

    fn extract_rule_from_text(
        text: &str,
        article_header: &Captures,
        next_start: usize,
    ) -> eyre::Result<Rule> {
        static RE_NEWLINE_ALPHA_LISTING: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\n([a-z]\) )").unwrap());
        static RE_NEWLINE_NUM_LISTING: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\n([0-9]+\. )").unwrap());
        static RE_NEWLINE_INNER_ALPHA_LISTING: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\n([a-z]\. )").unwrap());
        static RE_NEWLINE_WITHOUT_TAB: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\n([^\t])").unwrap());

        static RE_EXCEPTIONS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n(Ausnahmen:\n)").unwrap());
        static RE_TRAILING_NUM: Lazy<Regex> = Lazy::new(|| Regex::new(r" \d+$").unwrap());

        let article_nr = article_header["article_nr"].parse()?;
        let title = article_header["title"].parse()?;

        let article_text = &text[article_header.get(0).unwrap().start()..next_start];

        let text_start = article_text
            .find('\n')
            .ok_or(eyre!("No newline after header"))?;
        let mut text = article_text[text_start..].to_string();

        // Special handling for Clipping and Blocking in the back
        if title == "Clipping" || title == "Blocken in den Rücken" {
            text = RE_NEWLINE_ALPHA_LISTING
                .replace_all(&text, "\n\t\t\t$1")
                .to_string();
        } else {
            text = RE_NEWLINE_ALPHA_LISTING
                .replace_all(&text, "\n\t$1")
                .to_string();
        }
        text = RE_NEWLINE_NUM_LISTING
            .replace_all(&text, "\n\t\t$1")
            .to_string();
        text = RE_NEWLINE_INNER_ALPHA_LISTING
            .replace_all(&text, "\n\t\t\t$1")
            .to_string();

        text = RE_EXCEPTIONS.replace_all(&text, "\n\t$1").to_string();

        text = RE_NEWLINE_WITHOUT_TAB
            .replace_all(&text, " $1")
            .trim()
            .to_string();

        text = RE_TRAILING_NUM.replace_all(&text, "").to_string();

        // Special replacements for Targeting rule
        text = text.replace("Anmerkung 1 Targeting", "\nAnmerkung 1\nTargeting");
        text = text.replace(
            "Anmerkung 2 Verteidigungslose",
            "\nAnmerkung 2\nVerteidigungslose",
        );

        if text.starts_with("a)") {
            text = format!("\t{}", text);
        }

        Ok(Rule {
            article_nr,
            title,
            text,
            interpretations: vec![],
        })
    }
}
