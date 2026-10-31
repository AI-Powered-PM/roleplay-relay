//! Output guard for everything the player will send from their own account
//! (direct, rp, grammar, quick mode): parse -> sanitize -> block rules.
//!
//! Sanitizing is silent. A block means nothing is copied unless the player clicks
//! "Copy anyway" in the compose window.

use std::sync::OnceLock;

use regex::Regex;

use crate::languages::{is_non_latin_letter, Language};
use crate::parse::{self, Tagged, Unexpected};
use crate::prompts::Mode;
use crate::split::{self, Marker};

/// Most chat messages one reply may turn into.
pub const MAX_PARTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Block {
    #[error("Blocked: the reply contains a link or web address")]
    Url,
    /// The active user language is still in the English result.
    #[error("Result still has {language} text")]
    Leftover { language: &'static str },
    /// Letters of some non-Latin script (Cyrillic, Greek, CJK, Arabic, ...).
    #[error("Result has non-English letters")]
    NonLatin,
    #[error("Blocked: the reply looks like AI or translation text, not a chat line")]
    MetaText,
    #[error("Blocked: the reply is too long (more than 4 chat messages)")]
    TooManyParts,
}

/// What a model reply turned into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Processed {
    /// Sanitized and passed every rule.
    Ok { en: String, back: Option<String> },
    /// The model refused (`<en></en>`); `reason` is its explanation in the user language.
    Declined { reason: String },
    /// Sanitized text that failed a rule. Not copied.
    Blocked {
        en: String,
        back: Option<String>,
        block: Block,
    },
}

impl Processed {
    /// The part of the reply that is written in the user's language (back-translation or the
    /// refusal reason). It is only displayed, never sent.
    pub fn user_language_text(&self) -> Option<&str> {
        match self {
            Processed::Ok { back, .. } | Processed::Blocked { back, .. } => back.as_deref(),
            Processed::Declined { reason } => Some(reason),
        }
    }

    /// Drop foreign-script letters from [`Self::user_language_text`].
    pub fn strip_foreign_script(&mut self, lang: &Language) {
        match self {
            Processed::Ok { back, .. } | Processed::Blocked { back, .. } => {
                if let Some(b) = back {
                    *b = lang.strip_foreign_script(b);
                }
            }
            Processed::Declined { reason } => *reason = lang.strip_foreign_script(reason),
        }
    }
}

/// Parse a raw reply for `mode`, sanitize it and apply the block rules.
/// `Err` means the reply had no usable text at all (nothing is copied, raw reply is returned).
pub fn process_reply(
    mode: Mode,
    reply: &str,
    marker: Marker,
    lang: &Language,
) -> Result<Processed, Unexpected> {
    let (en, back) = match mode {
        Mode::Grammar => (parse::parse_plain(reply)?, None),
        _ => match parse::parse_tagged(reply)? {
            Tagged::Text { en, back } => (en, back),
            Tagged::Declined { reason } => return Ok(Processed::Declined { reason }),
        },
    };

    let en = sanitize(&en);
    if en.is_empty() {
        // Only emoji / control characters were left: nothing worth sending.
        return Err(Unexpected {
            raw: reply.trim().to_owned(),
        });
    }
    match check(&en, marker, lang) {
        Ok(()) => Ok(Processed::Ok { en, back }),
        Err(block) => Ok(Processed::Blocked { en, back, block }),
    }
}

/// Block rules on already-sanitized text, including the number of parts the splitter makes.
pub fn check(text: &str, marker: Marker, lang: &Language) -> Result<(), Block> {
    check_text(text, lang)?;
    check_parts(split::split(text, marker).len())
}

fn check_text(text: &str, lang: &Language) -> Result<(), Block> {
    if url_regex().is_match(text) {
        return Err(Block::Url);
    }
    // The language check comes first so the player sees "still has Chinese text" rather than
    // the generic message.
    if lang.has_leftover(text) {
        return Err(Block::Leftover {
            language: lang.english_name,
        });
    }
    if text.chars().any(is_non_latin_letter) {
        return Err(Block::NonLatin);
    }
    if is_meta(text) {
        return Err(Block::MetaText);
    }
    Ok(())
}

pub fn check_parts(count: usize) -> Result<(), Block> {
    if count > MAX_PARTS {
        Err(Block::TooManyParts)
    } else {
        Ok(())
    }
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)http|www\.|\b[\w-]+\.(?:com|net|org|gg|io|ru|xyz|shop)\b")
            .expect("url regex")
    })
}


const META_PHRASES: &[&str] = &[
    "as an ai",
    "language model",
    "i can't help with",
    "i cannot help with",
    "i'm not able to",
    "claude",
    "anthropic",
    "translation:",
    "<en>",
    "<back>",
];

fn is_meta(text: &str) -> bool {
    // Curly apostrophes would let "I can’t help with" slip past the list.
    let lower = text.to_lowercase().replace(['’', '‘'], "'");
    META_PHRASES.iter().any(|p| lower.contains(p))
}

/// Silent clean-up: one line, no control / invisible characters, no emoji, no `|`
/// (WoW chat escape sequences), single spaces.
pub fn sanitize(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            '|' => None,
            c if c.is_control() || is_invisible(c) || is_pictograph(c) => None,
            c => Some(c),
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}'   // zero-width space/joiners, direction marks
        | '\u{202A}'..='\u{202E}' // bidi embeddings and overrides
        | '\u{2060}'..='\u{2064}' // word joiner and invisible operators
        | '\u{2066}'..='\u{2069}' // bidi isolates
        | '\u{FEFF}'              // byte order mark / zero-width no-break space
        | '\u{FE00}'..='\u{FE0F}' // variation selectors (emoji presentation)
        | '\u{E0000}'..='\u{E007F}' // tag characters
    )
}

fn is_pictograph(c: char) -> bool {
    matches!(c,
        '\u{1F000}'..='\u{1FAFF}' // emoticons, symbols, flags, transport, supplemental
        | '\u{2300}'..='\u{23FF}' // technical symbols (watch, hourglass, ...)
        | '\u{2600}'..='\u{27BF}' // miscellaneous symbols and dingbats
        | '\u{2B00}'..='\u{2BFF}' // arrows and stars
        | '\u{20E3}'              // keycap
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ru() -> &'static Language {
        crate::languages::resolve("ru")
    }

    fn check(text: &str) -> Result<(), Block> {
        super::check(text, Marker::Numbered, ru())
    }

    fn check_in(code: &str, text: &str) -> Result<(), Block> {
        super::check(text, Marker::Numbered, crate::languages::resolve(code))
    }

    fn process_reply(mode: Mode, reply: &str) -> Result<Processed, Unexpected> {
        super::process_reply(mode, reply, Marker::Numbered, ru())
    }

    fn ok(en: &str) -> Processed {
        Processed::Ok {
            en: en.into(),
            back: None,
        }
    }

    // ---- sanitize ----

    #[test]
    fn newlines_and_tabs_become_one_space() {
        assert_eq!(sanitize("Hello\nthere,\r\n\tfriend."), "Hello there, friend.");
    }

    #[test]
    fn control_and_zero_width_characters_are_removed() {
        assert_eq!(sanitize("He\u{0007}llo\u{200B} wor\u{FEFF}ld\u{2060}!"), "Hello world!");
        assert_eq!(sanitize("a\u{200E}b\u{202E}c"), "abc");
    }

    #[test]
    fn emoji_and_pictographs_are_removed() {
        assert_eq!(sanitize("Well met 😀 friend ❤️ ⭐ ⌚ 🇺🇸!"), "Well met friend !");
    }

    #[test]
    fn pipes_are_removed_so_chat_escapes_cannot_form() {
        assert_eq!(
            sanitize("Look |cff00ff00|Hitem:19019|h[Thunderfury]|h|r now"),
            "Look cff00ff00Hitem:19019h[Thunderfury]hr now"
        );
        assert!(!sanitize("a | b || c").contains('|'));
    }

    #[test]
    fn repeated_spaces_collapse_and_ends_are_trimmed() {
        assert_eq!(sanitize("  a    b   "), "a b");
    }

    #[test]
    fn ordinary_punctuation_and_ellipsis_survive() {
        let s = "Thou art late… 'tis cold — very cold; yes? Yes!";
        assert_eq!(sanitize(s), s);
    }

    // ---- block rules ----

    #[test]
    fn urls_and_domains_are_blocked() {
        for s in [
            "visit cheapgold.com now",
            "go to http://x",
            "see www.gold",
            "buy at best-gold.GG today",
            "try gold.io",
            "gold.ru is good",
            "gold.xyz",
            "my.shop",
            "gold.net",
            "gold.org",
        ] {
            assert_eq!(check(s), Err(Block::Url), "{s}");
        }
    }

    #[test]
    fn normal_sentences_are_not_url_blocked() {
        assert_eq!(check("Well met. Come in, friend."), Ok(()));
        assert_eq!(check("I need 3.5 gold, no more."), Ok(()));
    }

    const RU_LEFTOVER: Block = Block::Leftover { language: "Russian" };

    #[test]
    fn leftover_cyrillic_is_blocked() {
        assert_eq!(check("Well met, друг"), Err(RU_LEFTOVER));
        assert_eq!(check("Ё"), Err(RU_LEFTOVER));
    }

    #[test]
    fn leftover_blocks_name_the_language_and_use_the_spec_wording() {
        let cases = [
            ("de", "Danke, ich bin bereit", "Result still has German text"),
            ("de", "Straße", "Result still has German text"),
            ("zh-CN", "Well met， friend", "Result still has Chinese text"),
            ("zh-CN", "你好", "Result still has Chinese text"),
            ("fr", "merci, je suis prêt", "Result still has French text"),
            ("es-MX", "¿Ready", "Result still has Spanish text"),
            ("es-MX", "mañana", "Result still has Spanish text"),
            ("es-MX", "hola amigo, gracias", "Result still has Spanish text"),
            ("ru", "привет", "Result still has Russian text"),
        ];
        for (code, text, message) in cases {
            let err = check_in(code, text).unwrap_err();
            assert_eq!(err.to_string(), message, "{code}: {text}");
        }
    }

    #[test]
    fn language_check_runs_before_the_generic_script_check() {
        // A zh-CN user sees the Chinese message; a ru user with Chinese text sees the generic one.
        assert!(matches!(check_in("zh-CN", "你好"), Err(Block::Leftover { .. })));
        assert_eq!(check_in("ru", "你好"), Err(Block::NonLatin));
        assert_eq!(check_in("de", "привет"), Err(Block::NonLatin));
        assert_eq!(check_in("fr", "Hello Ω"), Err(Block::NonLatin));
        assert_eq!(Block::NonLatin.to_string(), "Result has non-English letters");
    }

    #[test]
    fn accented_latin_letters_are_not_non_latin() {
        // Under ru the generic check must let Latin loanwords through.
        assert_eq!(check_in("ru", "A café and a naïve fiancé, señor Müller"), Ok(()));
    }

    #[test]
    fn real_rp_lines_pass_for_every_language() {
        let lines = [
            "May the Earth Mother guide your steps, stranger.",
            "nods slowly and settles beside the fire, eyes on the flames.",
            "smiles faintly. \"The wind speaks of rain.\"",
            "Will you share a meal with us before the hunt?",
        ];
        for lang in crate::languages::all() {
            for line in lines {
                assert_eq!(check_in(lang.code, line), Ok(()), "{}: {line}", lang.code);
            }
        }
    }

    #[test]
    fn foreign_letters_in_the_back_translation_are_found_and_stripped() {
        let mut p = process_reply(
            Mode::Direct,
            "<en>The road is long.</en><back>Дорога長 длинна.</back>",
        )
        .unwrap();
        assert!(ru().has_foreign_script(p.user_language_text().unwrap()));
        p.strip_foreign_script(ru());
        assert_eq!(p.user_language_text(), Some("Дорога длинна."));
        // The English text is untouched, and a reply without a back-translation has nothing to check.
        assert!(matches!(&p, Processed::Ok { en, .. } if en == "The road is long."));
        let g = process_reply(Mode::Grammar, "The road is long.").unwrap();
        assert_eq!(g.user_language_text(), None);
    }

    #[test]
    fn meta_text_is_blocked_case_insensitively() {
        for s in [
            "As an AI I cannot",
            "I am a Language Model",
            "I can't help with that",
            "I can’t help with that",
            "I cannot help with that",
            "I'm not able to say it",
            "Claude says hi",
            "made by ANTHROPIC",
            "Translation: hello",
            "<en>hello",
            "hello<back>",
        ] {
            assert_eq!(check(s), Err(Block::MetaText), "{s}");
        }
    }

    #[test]
    fn more_than_four_parts_is_blocked() {
        assert_eq!(check_parts(1), Ok(()));
        assert_eq!(check_parts(4), Ok(()));
        assert_eq!(check_parts(5), Err(Block::TooManyParts));
    }

    #[test]
    fn long_replies_are_blocked_by_the_real_splitter() {
        let sentence = "This is one sentence of ordinary chat text. ";
        let four = sentence.repeat(20); // ~880 chars: 4 parts
        let five = sentence.repeat(26); // ~1140 chars: 5 parts
        assert_eq!(check(four.trim()), Ok(()));
        assert_eq!(check(five.trim()), Err(Block::TooManyParts));
        assert_eq!(
            super::check(five.trim(), Marker::None, ru()),
            Err(Block::TooManyParts)
        );
    }

    #[test]
    fn rule_order_is_url_then_language_then_script_then_meta() {
        assert_eq!(check("claude друг gold.com"), Err(Block::Url));
        assert_eq!(check("claude друг"), Err(RU_LEFTOVER));
        assert_eq!(check_in("de", "claude друг"), Err(Block::NonLatin));
    }

    #[test]
    fn every_block_has_a_message() {
        for b in [Block::Url, Block::MetaText, Block::TooManyParts] {
            assert!(b.to_string().starts_with("Blocked: "));
        }
    }

    // ---- full pipeline ----

    #[test]
    fn direct_reply_is_parsed_sanitized_and_passed() {
        let r = process_reply(
            Mode::Direct,
            "<en>Well met,\nfriend 😀</en><back>Рад встрече, друг.</back>",
        );
        assert_eq!(
            r,
            Ok(Processed::Ok {
                en: "Well met, friend".into(),
                back: Some("Рад встрече, друг.".into())
            })
        );
    }

    #[test]
    fn missing_en_is_an_error_not_a_copy() {
        let r = process_reply(Mode::Rp, "Sure! Well met, friend.");
        assert_eq!(
            r,
            Err(Unexpected {
                raw: "Sure! Well met, friend.".into()
            })
        );
    }

    #[test]
    fn empty_en_is_declined() {
        let r = process_reply(Mode::Direct, "<en></en><back>Не могу.</back>");
        assert_eq!(r, Ok(Processed::Declined { reason: "Не могу.".into() }));
    }

    #[test]
    fn url_in_a_reply_is_blocked_with_the_sanitized_text() {
        let r = process_reply(
            Mode::Rp,
            "<en>Visit cheap|gold.com\nfor gold</en><back>Заходи.</back>",
        );
        assert_eq!(
            r,
            Ok(Processed::Blocked {
                en: "Visit cheapgold.com for gold".into(),
                back: Some("Заходи.".into()),
                block: Block::Url
            })
        );
    }

    #[test]
    fn a_pipe_cannot_hide_a_url_from_the_check() {
        // Sanitizing runs first, so "gold|.com" becomes "gold.com" and is caught.
        let r = process_reply(Mode::Direct, "<en>see gold|.com</en>");
        assert!(matches!(
            r,
            Ok(Processed::Blocked { block: Block::Url, .. })
        ));
    }

    #[test]
    fn grammar_is_plain_text_without_back() {
        assert_eq!(
            process_reply(Mode::Grammar, "  \"He goes to town.\"  "),
            Ok(ok("He goes to town."))
        );
        assert!(matches!(
            process_reply(Mode::Grammar, "Translation: He goes"),
            Ok(Processed::Blocked { block: Block::MetaText, .. })
        ));
        assert!(process_reply(Mode::Grammar, "   ").is_err());
    }

    #[test]
    fn a_reply_of_only_emoji_is_unexpected() {
        assert!(process_reply(Mode::Direct, "<en>😀🎉</en>").is_err());
    }

    #[test]
    fn output_never_contains_pipe_newline_emoji_or_non_latin_when_ok() {
        let r = process_reply(Mode::Direct, "<en>a|b\nc 😀 d</en>").unwrap();
        let Processed::Ok { en, .. } = r else {
            panic!("expected ok, got {r:?}");
        };
        assert!(!en.contains(['|', '\n', '😀']));
        assert!(!en.chars().any(is_non_latin_letter));
    }
}
