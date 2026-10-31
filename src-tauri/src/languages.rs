//! The user's language: a static table with everything that differs per language.
//! Adding a language = one entry in `LANGUAGES` plus its sample strings; no other Rust
//! logic and no bundled prompt changes.

use serde::Serialize;

pub const DEFAULT_CODE: &str = "ru";

/// How an entry detects text left over in the user's language inside an English result.
enum Leftover {
    /// Any single character of the script (or its punctuation) is a hit.
    Chars(fn(char) -> bool),
    /// Latin-script languages: a special letter is a hit; plain words only count when at
    /// least two distinct ones show up (one alone is too common in English names and phrases).
    Latin {
        letters: &'static str,
        words: &'static [&'static str],
    },
}

pub struct Language {
    pub code: &'static str,
    /// Dropdown label: the language's own name.
    pub label: &'static str,
    /// Used in UI messages ("Result still has {english_name} text").
    pub english_name: &'static str,
    /// Goes into prompts as `{{user_language}}`.
    pub prompt_name: &'static str,
    /// Goes into prompts as `{{user_language_notes}}`.
    pub notes: &'static str,
    leftover: Leftover,
    /// Extra English lines (beyond the shared RP lines in the tests) that must not be flagged.
    #[cfg_attr(not(test), allow(dead_code))]
    pub ok_samples: &'static [&'static str],
    /// Strings that must be flagged as leftovers.
    #[cfg_attr(not(test), allow(dead_code))]
    pub leftover_samples: &'static [&'static str],
}

impl Language {
    /// True when `text` (an English result) still contains this language.
    pub fn has_leftover(&self, text: &str) -> bool {
        match &self.leftover {
            Leftover::Chars(is_hit) => text.chars().any(is_hit),
            Leftover::Latin { letters, words } => {
                if text.chars().any(|c| letters.contains(c)) {
                    return true;
                }
                let mut seen: Vec<&str> = Vec::new();
                let lower = text.to_lowercase();
                for token in lower.split(|c: char| !c.is_alphabetic()) {
                    if let Some(w) = words.iter().find(|w| **w == token) {
                        if !seen.contains(w) {
                            seen.push(w);
                        }
                    }
                }
                seen.len() >= 2
            }
        }
    }
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{3000}'..='\u{303F}'
        | '\u{FF00}'..='\u{FFEF}')
}

/// Dropdown order is the table order.
static LANGUAGES: [Language; 5] = [
    Language {
        code: "de",
        label: "Deutsch",
        english_name: "German",
        prompt_name: "German",
        notes: "Use natural informal German as players write in game chat: \"du\" for one person, \"ihr\" for several. Keep names of people, places and game terms in English.",
        leftover: Leftover::Latin {
            letters: "äöüÄÖÜß",
            words: &[
                "und", "ich", "nicht", "ist", "das", "der", "wir", "auch", "aber", "danke",
                "bitte", "hallo",
            ],
        },
        ok_samples: &[
            "The die is cast, friend.",
            "Danke is how Germans say thanks.",
        ],
        leftover_samples: &[
            "Danke, ich bin bereit",
            "Wir sind bereit, aber nicht alle",
            "Grüße, friend",
            "Strasse of Stein ß",
        ],
    },
    Language {
        code: "zh-CN",
        label: "简体中文",
        english_name: "Chinese",
        prompt_name: "Simplified Chinese",
        notes: "Use Simplified Chinese characters as used in mainland China, natural casual register. Keep names of people, places and game terms in English; do not transliterate them into Chinese characters.",
        leftover: Leftover::Chars(is_cjk),
        ok_samples: &["Meet me at Thunder Bluff at 8, friend."],
        leftover_samples: &["Well met, 朋友", "Well met， friend", "雷霆崖", "Meet me。"],
    },
    Language {
        code: "fr",
        label: "Français",
        english_name: "French",
        prompt_name: "French",
        notes: "Use natural informal French as players write in game chat: \"tu\" for one person, \"vous\" for several. Keep names of people, places and game terms in English.",
        leftover: Leftover::Latin {
            letters: "àâæçèéêëîïôœùûÿÀÂÆÇÈÉÊËÎÏÔŒÙÛŸ«»",
            words: &[
                "je", "tu", "nous", "vous", "avec", "merci", "bonjour", "salut", "oui", "mais",
                "pour", "suis",
            ],
        },
        ok_samples: &[
            "Pour the ale, friend.",
            "Oui is how the French say yes.",
        ],
        leftover_samples: &[
            "merci, je suis prêt",
            "Bonjour, nous voilà",
            "Salut mais pas vous",
            "Well met, «friend»",
        ],
    },
    Language {
        code: "es-MX",
        label: "Español",
        english_name: "Spanish",
        prompt_name: "Mexican Spanish",
        notes: "Use Latin American Spanish as spoken in Mexico: \"ustedes\" for plural \"you\" (never \"vosotros\"), Mexican everyday vocabulary, natural informal register. Keep names of people, places and game terms in English.",
        leftover: Leftover::Latin {
            letters: "¿¡ñÑáéíóúÁÉÍÓÚ",
            words: &[
                "que", "los", "las", "pero", "para", "estoy", "eres", "gracias", "hola", "usted",
                "ustedes",
            ],
        },
        ok_samples: &[
            "Pass the los of the pack, friend.",
            "Hola is how Mexicans say hello.",
        ],
        leftover_samples: &[
            "hola amigo, gracias",
            "¿Are you ready?",
            "Well met, señor",
            "Estoy aquí pero eres tú",
        ],
    },
    Language {
        code: "ru",
        label: "Русский",
        english_name: "Russian",
        prompt_name: "Russian",
        notes: "",
        leftover: Leftover::Chars(is_cyrillic),
        ok_samples: &["Well met, friend."],
        leftover_samples: &["Well met, друг", "Ё", "привет"],
    },
];

#[cfg_attr(not(test), allow(dead_code))]
pub fn all() -> &'static [Language] {
    &LANGUAGES
}

pub fn find(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code == code)
}

/// The language for a settings value; unknown codes fall back to Russian.
pub fn resolve(code: &str) -> &'static Language {
    find(code)
        .or_else(|| find(DEFAULT_CODE))
        .expect("default language is in the table")
}

/// What the Settings dropdown shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LanguageOption {
    pub code: &'static str,
    pub label: &'static str,
}

pub fn options() -> Vec<LanguageOption> {
    LANGUAGES
        .iter()
        .map(|l| LanguageOption {
            code: l.code,
            label: l.label,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real English RP chat lines, with emotes, that must never be flagged in any language.
    const RP_LINES: [&str; 20] = [
        "May the Earth Mother guide your steps, stranger.",
        "Well met, friend. What brings you to Thunder Bluff?",
        "nods slowly and settles beside the fire, eyes on the flames.",
        "The wind speaks of rain over the plains of Mulgore.",
        "Walk with the spirits, and the spirits will walk with you.",
        "smiles faintly. \"The elements are restless tonight.\"",
        "Cairne Bloodhoof leads our people with a steady hand.",
        "I have come a long way from the Barrens, and my feet are tired.",
        "raises a hand in greeting and bows his great horned head.",
        "Do not mistake my silence for weakness, traveler.",
        "An'she warms the grass, and Mu'sha watches over the night.",
        "Will you share a meal with us before the hunt?",
        "The kodo are calm today; the herd is well.",
        "laughs, a deep rumbling sound. \"You have courage, little one.\"",
        "Thrall is Warchief of the Horde, and we follow him gladly.",
        "Stay close to the fire, the night is cold and the road is long.",
        "Aye, I will tend your wounds. Sit down and be still.",
        "looks up at the stars and whispers a quiet prayer.",
        "What news from Orgrimmar? Is the Horde at peace, or at war?",
        "Honor your ancestors and the land will honor you in return.",
    ];

    #[test]
    fn table_order_and_labels_match_the_spec() {
        let codes: Vec<_> = all().iter().map(|l| l.code).collect();
        assert_eq!(codes, ["de", "zh-CN", "fr", "es-MX", "ru"]);
        let labels: Vec<_> = options().iter().map(|o| o.label).collect();
        assert_eq!(labels, ["Deutsch", "简体中文", "Français", "Español", "Русский"]);
    }

    #[test]
    fn names_match_the_spec() {
        let rows: Vec<_> = all()
            .iter()
            .map(|l| (l.english_name, l.prompt_name))
            .collect();
        assert_eq!(
            rows,
            [
                ("German", "German"),
                ("Chinese", "Simplified Chinese"),
                ("French", "French"),
                ("Spanish", "Mexican Spanish"),
                ("Russian", "Russian"),
            ]
        );
        assert_eq!(resolve("ru").notes, "");
        assert!(resolve("es-MX").notes.contains("never \"vosotros\""));
    }

    #[test]
    fn unknown_code_falls_back_to_russian() {
        assert_eq!(resolve("xx").code, "ru");
        assert_eq!(resolve("").code, "ru");
        assert_eq!(resolve("fr").code, "fr");
        assert!(find("xx").is_none());
    }

    #[test]
    fn real_rp_lines_are_never_flagged() {
        for lang in all() {
            for line in RP_LINES.iter().chain(lang.ok_samples) {
                assert!(
                    !lang.has_leftover(line),
                    "{}: false positive on {line:?}",
                    lang.code
                );
            }
        }
    }

    #[test]
    fn leftovers_are_caught() {
        for lang in all() {
            assert!(!lang.leftover_samples.is_empty(), "{}", lang.code);
            for s in lang.leftover_samples {
                assert!(lang.has_leftover(s), "{}: missed {s:?}", lang.code);
            }
        }
    }

    #[test]
    fn spec_examples() {
        let de = resolve("de");
        assert!(de.has_leftover("Danke, ich bin bereit"));
        assert!(de.has_leftover("Straße"));
        assert!(de.has_leftover("Müller says hi"));
        assert!(resolve("zh-CN").has_leftover("你好"));
        assert!(resolve("zh-CN").has_leftover("Hello，friend"));
        let fr = resolve("fr");
        assert!(fr.has_leftover("merci, je suis prêt"));
        let es = resolve("es-MX");
        assert!(es.has_leftover("¿"));
        assert!(es.has_leftover("ñ"));
        assert!(es.has_leftover("hola amigo, gracias"));
        assert!(resolve("ru").has_leftover("а"));
    }

    #[test]
    fn one_plain_word_alone_is_not_enough() {
        assert!(!resolve("de").has_leftover("Danke, friend"));
        assert!(!resolve("fr").has_leftover("Merci, friend"));
        assert!(!resolve("es-MX").has_leftover("Hola, friend"));
        // The same word twice is still one distinct word.
        assert!(!resolve("de").has_leftover("Danke, danke!"));
    }

    #[test]
    fn only_the_active_language_is_checked() {
        assert!(!resolve("ru").has_leftover("Danke, ich bin bereit"));
        assert!(!resolve("de").has_leftover("привет"));
        assert!(!resolve("fr").has_leftover("你好"));
    }

    #[test]
    fn words_match_whole_words_only() {
        // "undone" contains "und", "dash" contains nothing: substrings must not count.
        assert!(!resolve("de").has_leftover("The undone derby was auch..."));
        assert!(!resolve("es-MX").has_leftover("lasting queen paragon"));
    }
}
