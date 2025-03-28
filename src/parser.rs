use eyre::{Context, eyre};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use roman_numerals::FromRoman;
use std::{path::Path, process::Command};

use crate::rule::{ArticleNr, Rule, RuleInterpretation};

pub struct RulesParser;

impl RulesParser {
    pub fn parse(rules_path: &Path) -> eyre::Result<IndexMap<ArticleNr, Rule>> {
        let rules_text = Self::load_rules_text(rules_path)?;

        let mut rules = Self::extract_rules(&rules_text)?;
        let interpretations = Self::extract_interpretations(&rules_text)?;

        for (article_nr, article_interpretations) in interpretations {
            let rule = rules
                .get_mut(&article_nr)
                .ok_or_else(|| eyre!("Could not find rule {}", article_nr))?;
            rule.interpretations = article_interpretations;
        }

        Ok(rules)
    }

    pub fn parse_interpretations(
        rules_path: &Path,
    ) -> eyre::Result<IndexMap<ArticleNr, Vec<RuleInterpretation>>> {
        let rules_text = Self::load_rules_text(rules_path)?;

        Self::extract_interpretations(&rules_text)
    }

    fn load_rules_text(rules_path: &Path) -> eyre::Result<String> {
        let rules_text = Self::extract_text_from_pdf(rules_path)?;
        Ok(Self::preprocess_text(rules_text))
    }

    fn extract_text_from_pdf(rules_path: &Path) -> eyre::Result<String> {
        let pdftotext_output = Command::new("pdftotext")
            .args([
                "-x",
                "0",
                "-y",
                "40",
                "-W",
                "1000",
                "-H",
                "540",
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

        String::from_utf8(pdftotext_output.stdout).wrap_err("Stdout is no valid utf8")
    }

    fn preprocess_text(mut text: String) -> String {
        // Treat section 9.1. as rule
        text = text.replace(
            "Abschnitt 9.1 Persönliche Fouls\nAlle",
            "Artikel 9.1.0 Persönliche Fouls\nAlle",
        );
        text = text.replace("Regel 9\nVerhalten von Spielern und anderen", "");

        // Fixes for rules that are longer than one line
        text = text.replace("9.1.4 Targeting und Forcible Contact zum Kopf-/Halsbereich\nverteidigungsloser Spieler",
                                        "9.1.4 Targeting und Forcible Contact zum Kopf-/Halsbereich verteidigungsloser Spieler");
        text = text.replace(
            "6.1.3 Berühren, illegales Berühren und Recovern eines Free\nKicks",
            "6.1.3 Berühren, illegales Berühren und Recovern eines Free Kicks",
        );

        let re_new_page = Regex::new(r"-?\n\x0C").unwrap();
        let re_section = Regex::new(r"(?sm)^Abschnitt .*?Artikel").unwrap();
        let re_new_chapter = Regex::new(r"\n\x0C.*\n.*\n\n").unwrap();

        text = re_new_page.replace_all(&text, "").to_string();
        text = re_section.replace_all(&text, "\nArtikel").to_string();
        text = re_new_chapter.replace_all(&text, "").to_string();

        text
    }

    fn extract_rules(text: &str) -> eyre::Result<IndexMap<ArticleNr, Rule>> {
        let rules_start = text
            .find("Artikel 1.1.1")
            .ok_or(eyre!("Could not find 'Artikel 1.1.1' inside the pdf text"))?;
        let rules_end = text
            .find("Die Abkürzungen R, Ab, Art stehen für Regel,")
            .ok_or(eyre!(
                "Could not find 'Die Abkürzungen R, Ab, Art stehen für Regel,' inside the pdf text"
            ))?;

        let mut rules = IndexMap::new();

        let re_article_header =
            Regex::new(r"(?m)Artikel (?<article_nr>\d+\.\d+\.\d+) (?<title>.*)$").unwrap();

        let rules_part = &text[rules_start..rules_end];

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
                .find("Zusammenfassung der Strafen")
                .ok_or(eyre!("Could not find end of rules"))?
                + end_of_last_capture;
            let rule = Self::extract_rule_from_text(rules_part, last_capture, rules_end)?;
            rules.insert(rule.article_nr, rule);
        }
        Ok(rules)
    }

    fn extract_interpretations(
        text: &str,
    ) -> eyre::Result<IndexMap<ArticleNr, Vec<RuleInterpretation>>> {
        let re_rule = Regex::new(r"(?sm)^Regel .*?A\.R\.").unwrap();
        let re_section = Regex::new(r"(?sm)^Abschnitt .*?A\.R\.").unwrap();
        let re_article = Regex::new(r"(?sm)^Artikel .*?A\.R\.").unwrap();

        let mut text = re_rule.replace_all(text, "\nA.R.").to_string();
        text = re_section.replace_all(&text, "\nA.R.").to_string();
        text = re_section.replace_all(&text, "\nA.R.").to_string();
        text = re_article.replace_all(&text, "\nA.R.").to_string();

        let interpretations_start = text.find("\nA.R. 1.3.2.I ").ok_or(eyre!(
            "Could not find '\\nA.R. 1.3.2.I ' inside the pdf text"
        ))?;
        let interpretations_end = text
            .find("Teil IV")
            .ok_or(eyre!("Could not find 'Teil IV' inside the pdf text"))?;

        let mut interpretations: IndexMap<ArticleNr, Vec<RuleInterpretation>> = IndexMap::new();

        let re_interpretation = Regex::new(r"(?sm)^A\.R\. (?<ar_nr>\d+\.\d+\.\d+.)(?<index>[IVX]+) (?<situation>.*?)Regelung:(?<ruling>.*?)\nA\.R\. ").unwrap();

        let mut interpretations_text = text[interpretations_start..interpretations_end].to_string();
        interpretations_text.push_str("\nA.R. "); // Add interpretation header to match last one
        let mut current_position = 0;

        while let Some(captures) =
            re_interpretation.captures_at(&interpretations_text, current_position)
        {
            let article_nr = captures["ar_nr"].parse()?;
            interpretations
                .entry(article_nr)
                .or_default()
                .push(RuleInterpretation {
                    article_nr,
                    index: u8::from_roman(&captures["index"])
                        .ok_or_else(|| eyre!("Invalid Roman number."))?,
                    text: captures["situation"].trim().to_string(),
                    ruling: captures["ruling"].trim().to_string(),
                });
            current_position = captures.get(0).unwrap().end() - 5;
        }

        Ok(interpretations)
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

        let article_nr: ArticleNr = article_header["article_nr"].parse()?;
        let title = article_header["title"].parse()?;

        let article_text = &text[article_header.get(0).unwrap().start()..next_start];

        let text_start = article_text
            .find('\n')
            .ok_or(eyre!("No newline after header"))?;
        let mut text = article_text[text_start..].to_string();

        // Special handling for Clipping and Blocking in the back
        if (title == "Clipping" && article_nr.0 == 9) || title == "Blocken in den Rücken" {
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

#[cfg(test)]
mod tests {
    use super::*;

    use insta::{assert_snapshot, assert_yaml_snapshot};
    use maud::html;
    use std::path::PathBuf;
    use temp_testdir::TempDir;

    #[test]
    fn test_parse() {
        let temp = TempDir::default();

        let mut rules_path = PathBuf::from(temp.as_ref());
        rules_path.push("rules.pdf");

        let rules_response = reqwest::blocking::get(
            "https://afsvd.de/content/files/2024/12/Football_Regelbuch_2025-1.pdf",
        )
        .unwrap()
        .error_for_status()
        .unwrap();

        std::fs::write(&rules_path, rules_response.bytes().unwrap()).unwrap();

        let mut rules_text = RulesParser::extract_text_from_pdf(&rules_path).unwrap();
        assert_snapshot!("text_from_pdf", rules_text);

        rules_text = RulesParser::preprocess_text(rules_text);
        assert_snapshot!("preprocessed_text", rules_text);

        let rules = RulesParser::extract_rules(&rules_text).unwrap();
        for (article_nr, rule) in &rules {
            assert_yaml_snapshot!(format!("rule_{article_nr}"), rule);
        }

        for (article_nr, rule_html) in rules.values().map(|r| {
            (
                r.article_nr,
                tidier::format(html!((r)).into_string(), false, &Default::default()).unwrap(),
            )
        }) {
            assert_snapshot!(format!("rule_html_{article_nr}"), rule_html);
        }

        let interpretations = RulesParser::extract_interpretations(&rules_text).unwrap();
        for interpretation in interpretations.values().flatten() {
            assert_yaml_snapshot!(
                format!(
                    "interpretation_{}_{}",
                    interpretation.article_nr, interpretation.index
                ),
                interpretation
            );
        }

        for (article_nr, index, interpretation_html) in
            interpretations.values().flatten().map(|i| {
                (
                    i.article_nr,
                    i.index,
                    tidier::format(html!((i)).into_string(), false, &Default::default()).unwrap(),
                )
            })
        {
            assert_snapshot!(
                format!("interpretation_html_{article_nr}_{index}"),
                interpretation_html
            );
        }
    }
}
