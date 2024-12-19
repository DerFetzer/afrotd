use eyre::eyre;
use maud::{html, PreEscaped, Render};
use roman_numerals::ToRoman;
use std::{fmt::Display, str::FromStr};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Rule {
    pub article_nr: ArticleNr,
    pub title: String,
    pub text: String,
    pub interpretations: Vec<RuleInterpretation>,
}

impl Rule {
    #[allow(clippy::comparison_chain)]
    fn render_text(&self) -> maud::Markup {
        let mut current_indent = 0u8;
        let processed_lines = self.text.lines().map(|l| {
            let (new_indent, list_type) = if l.starts_with("\t\t\t") {
                (3, "a")
            } else if l.starts_with("\t\t") {
                (2, "1")
            } else if l.starts_with('\t') {
                (1, "a")
            } else {
                (0, "")
            };
            let penalty_index = l.find(" Strafe: ");

            let markup = html! {
                @if new_indent > current_indent {
                    (PreEscaped(format!("<ol type={list_type}>")))
                } @else if new_indent < current_indent {
                    @for _ in 0..(current_indent - new_indent){
                        (PreEscaped("</ol>"))
                    }
                }
                @if new_indent != 0 && !l.starts_with("\tAusnahmen") {
                    li { ({
                        let space_index = l.find(' ').expect("Missing space after enumeration index.");
                        if let Some(penalty_index) = penalty_index {
                            html! {
                                p { (&l[space_index..penalty_index + 1]) }
                                p { strong { (&l[penalty_index + 1..]) } }
                            }
                        } else {
                            html! {
                                ( &l[space_index..] )
                            }
                        }
                    }) }
                } @else if let Some(penalty_index) = penalty_index {
                    p { (l[..penalty_index + 1]) }
                    p { strong { (l[penalty_index + 1..]) } }
                } @else if l.starts_with("\tAusnahmen") {
                    p { strong { (l) } }
                } @else {
                    p { (l) }
                }
            };
            current_indent = new_indent;
            markup
        });
        html! {
            .content {
                @for line in processed_lines {
                    (line)
                }
            }
        }
    }

    pub fn to_description(&self) -> String {
        format!(
            "{}...",
            self.text
                .lines()
                .next()
                .unwrap()
                .chars()
                .take(50)
                .collect::<String>()
        )
    }

    pub fn to_title(&self) -> String {
        format!("{} {}", self.article_nr, self.title)
    }

    pub fn to_url(&self, base_url: &str) -> String {
        format!("{}/rule/{}", base_url, self.article_nr.to_path_parameter())
    }
}

impl Render for Rule {
    fn render(&self) -> maud::Markup {
        html! {
            article.message ."is-size-4" {
                div.message-header {
                    p { (self.article_nr) " " (self.title) }
                }
                div.message-body {
                    (self.render_text())
                    div.block {
                        a .button .is-medium
                            href=(format!("https://afsvd.de/content/files/2024/12/Football_Regelbuch_2025.pdf#{}", self.article_nr.to_pdf_destination()))
                            target="_blank" rel="noreferrer noopener" {
                            "Offizielles Regelwerk"
                        }
                    }
                    div.block {
                        @for interpretation in &self.interpretations {
                            (interpretation)
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RuleInterpretation {
    pub article_nr: ArticleNr,
    pub index: u8,
    pub text: String,
    pub ruling: String,
}

impl Render for RuleInterpretation {
    fn render(&self) -> maud::Markup {
        html! {
            article.message ."is-size-5" .is-info {
                div.message-header {
                    p { "A.R. " (self.article_nr) "." (self.index.to_roman()) }
                }
                div.message-body {
                    p { (self.text) }
                    p {
                        details {
                            summary { b { "Regelung" } }
                            p {
                                (self.ruling)
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ArticleNr(pub u8, pub u8, pub u8);

impl ArticleNr {
    pub fn from_path_paramter(path_parameter: String) -> eyre::Result<Self> {
        let article_nr_parts = path_parameter
            .splitn(3, '-')
            .map(|p| p.parse())
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| eyre!("Parts have to be integer"))?;
        if let [chapter, section, article] = &article_nr_parts[..] {
            Ok(ArticleNr(*chapter, *section, *article))
        } else {
            Err(eyre!("Parameter has to consist of three parts"))
        }
    }

    pub fn to_pdf_destination(self) -> String {
        if self.2 != 0 {
            format!("subsection.1.{}.{}.{}", self.0, self.1, self.2)
        } else {
            format!("section.1.{}.{}", self.0, self.1)
        }
    }

    pub fn to_path_parameter(self) -> String {
        format!("{}-{}-{}", self.0, self.1, self.2)
    }
}

impl Display for ArticleNr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl FromStr for ArticleNr {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.trim().trim_end_matches('.').split('.').collect();
        if let [chapter, section, article] = &parts[..] {
            Ok(Self(chapter.parse()?, section.parse()?, article.parse()?))
        } else {
            Err(eyre!("Invalid article number: {s}"))
        }
    }
}
