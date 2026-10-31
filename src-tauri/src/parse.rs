//! Parsing of model replies for the outgoing modes.
//!
//! `direct` / `rp` are parsed strictly (`<en>` is required). `grammar` is the whole reply.
//! Nothing here copies or shows anything; the caller decides what to do with the result.

use std::sync::OnceLock;

use regex::Regex;

/// The reply did not have the expected shape. Carries the raw reply so the UI can show it
/// dimmed (the raw text is never logged unless `debugLogChat` is on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unexpected {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tagged {
    /// `en` is non-empty and trimmed; `back` is `None` when the model left it out.
    Text { en: String, back: Option<String> },
    /// `<en></en>` was empty: the model refused. `reason` is `<back>` (Russian).
    Declined { reason: String },
}

const DECLINED_FALLBACK: &str = "The model declined to write this message";

fn en_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<\s*en\s*>(.*?)<\s*/\s*en\s*>").expect("en regex"))
}

fn back_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // A missing closing tag is tolerated for `<back>`: it is only displayed.
    RE.get_or_init(|| {
        Regex::new(r"(?is)<\s*back\s*>(.*?)(?:<\s*/\s*back\s*>|$)").expect("back regex")
    })
}

/// Strict parser for `direct` and `rp`.
pub fn parse_tagged(reply: &str) -> Result<Tagged, Unexpected> {
    let Some(en_caps) = en_regex().captures(reply) else {
        return Err(Unexpected {
            raw: reply.trim().to_owned(),
        });
    };
    let en = strip_quotes(en_caps[1].trim());
    let back = back_regex()
        .captures(reply)
        .map(|c| c[1].trim().to_owned())
        .filter(|b| !b.is_empty());

    if en.is_empty() {
        return Ok(Tagged::Declined {
            reason: back.unwrap_or_else(|| DECLINED_FALLBACK.to_owned()),
        });
    }
    Ok(Tagged::Text { en, back })
}

/// `grammar`: the whole reply is the corrected message. An empty reply is unexpected.
pub fn parse_plain(reply: &str) -> Result<String, Unexpected> {
    let text = strip_quotes(reply.trim());
    if text.is_empty() {
        return Err(Unexpected {
            raw: reply.trim().to_owned(),
        });
    }
    Ok(text)
}

/// Remove one layer (or more) of quotes the model wrapped around the whole message.
/// Straight quotes are only removed when they do not also appear inside, so a line such as
/// `"Hail," he said, "and well met."` is left alone.
fn strip_quotes(text: &str) -> String {
    let mut cur = text.trim().to_owned();
    loop {
        let mut chars = cur.chars();
        let (Some(first), Some(last)) = (chars.next(), chars.next_back()) else {
            return cur;
        };
        let inner: String = cur
            .chars()
            .skip(1)
            .take(cur.chars().count().saturating_sub(2))
            .collect();
        let wrapped = match (first, last) {
            ('"', '"') => !inner.contains('"'),
            ('\'', '\'') => !inner.contains('\''),
            ('“', '”') | ('«', '»') | ('„', '“') => true,
            _ => false,
        };
        if !wrapped {
            return cur;
        }
        cur = inner.trim().to_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(en: &str, back: Option<&str>) -> Tagged {
        Tagged::Text {
            en: en.into(),
            back: back.map(Into::into),
        }
    }

    #[test]
    fn parses_en_and_back() {
        let r = parse_tagged("<en>Well met, friend.</en>\n<back>Рад встрече, друг.</back>");
        assert_eq!(r, Ok(text("Well met, friend.", Some("Рад встрече, друг."))));
    }

    #[test]
    fn tags_are_case_insensitive_and_whitespace_tolerant() {
        let r = parse_tagged("<EN> Hello </En>\n< back >Привет</ BACK >");
        assert_eq!(r, Ok(text("Hello", Some("Привет"))));
    }

    #[test]
    fn missing_en_is_an_error_and_keeps_the_raw_reply() {
        let r = parse_tagged("  Sure! Here is your message: Hello  ");
        assert_eq!(
            r,
            Err(Unexpected {
                raw: "Sure! Here is your message: Hello".into()
            })
        );
        // Only <back> present is still an error.
        assert!(parse_tagged("<back>Привет</back>").is_err());
    }

    #[test]
    fn unclosed_en_is_an_error() {
        // Typical for a reply cut off by max_tokens.
        assert!(parse_tagged("<en>Well met, friend. I have come").is_err());
    }

    #[test]
    fn empty_en_means_declined_with_back_as_reason() {
        let r = parse_tagged("<en></en><back>Не могу это написать.</back>");
        assert_eq!(
            r,
            Ok(Tagged::Declined {
                reason: "Не могу это написать.".into()
            })
        );
        let r = parse_tagged("<en>  </en>");
        assert_eq!(
            r,
            Ok(Tagged::Declined {
                reason: DECLINED_FALLBACK.into()
            })
        );
    }

    #[test]
    fn missing_back_alone_is_fine() {
        assert_eq!(parse_tagged("<en>Hello there.</en>"), Ok(text("Hello there.", None)));
        assert_eq!(
            parse_tagged("<en>Hello there.</en><back>  </back>"),
            Ok(text("Hello there.", None))
        );
    }

    #[test]
    fn unclosed_back_is_tolerated() {
        assert_eq!(
            parse_tagged("<en>Hi.</en><back>Привет."),
            Ok(text("Hi.", Some("Привет.")))
        );
    }

    #[test]
    fn multiline_en_is_kept_for_the_sanitizer() {
        let r = parse_tagged("<en>\nHello\nthere.\n</en>");
        assert_eq!(r, Ok(text("Hello\nthere.", None)));
    }

    #[test]
    fn strips_surrounding_quotes_only() {
        assert_eq!(
            parse_tagged("<en>\"Hello there.\"</en>"),
            Ok(text("Hello there.", None))
        );
        assert_eq!(
            parse_tagged("<en>“Hello there.”</en>"),
            Ok(text("Hello there.", None))
        );
        assert_eq!(
            parse_tagged("<en>\"Hail,\" he said, \"well met.\"</en>"),
            Ok(text("\"Hail,\" he said, \"well met.\"", None))
        );
        assert_eq!(parse_tagged("<en>'Tis cold.</en>"), Ok(text("'Tis cold.", None)));
    }

    #[test]
    fn plain_parser_trims_and_strips_quotes() {
        assert_eq!(parse_plain("  He go to town. \n"), Ok("He go to town.".into()));
        assert_eq!(parse_plain("\"He goes to town.\""), Ok("He goes to town.".into()));
    }

    #[test]
    fn plain_parser_rejects_empty() {
        assert!(parse_plain("   ").is_err());
        assert!(parse_plain("\"\"").is_err());
    }
}
