//! The chat-limit splitter. The limit comes from the active game profile.
//!
//! Parts are filled greedily by sentence, then by word. A word is only cut when it is longer
//! than a whole part. Lengths are Unicode scalar values, and every part (marker included) is at
//! most `limit`.

/// How parts are labelled. Comes from the `splitMarker` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// `(1/3)` appended to every part.
    Numbered,
    /// `…` appended to every part but the last.
    Ellipsis,
    None,
}

impl Marker {
    /// Unknown values fall back to the default, like `Settings::sanitized`.
    pub fn from_setting(s: &str) -> Self {
        match s {
            "…" => Marker::Ellipsis,
            "none" => Marker::None,
            _ => Marker::Numbered,
        }
    }
}

fn len(s: &str) -> usize {
    s.chars().count()
}

fn digits(n: usize) -> usize {
    n.to_string().len()
}

/// Split `text` into parts of at most `limit` characters. Text that already fits is returned as
/// one part, unmarked.
pub fn split(text: &str, marker: Marker, limit: usize) -> Vec<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if len(&text) <= limit {
        return vec![text];
    }
    match marker {
        Marker::None => pack(&text, limit),
        Marker::Ellipsis => {
            let mut parts = pack(&text, limit - 1);
            let last = parts.len() - 1;
            for p in &mut parts[..last] {
                p.push('…');
            }
            parts
        }
        Marker::Numbered => {
            // The marker " (k/k)" takes more room as k gains digits, so find a part count
            // that is stable: reserve for k parts and check we really need no more than k.
            let mut k = 2;
            loop {
                let reserve = 1 + 3 + 2 * digits(k); // " (" + k + "/" + k + ")"
                let parts = pack(&text, limit - reserve);
                if parts.len() <= k {
                    let m = parts.len();
                    return parts
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| format!("{p} ({}/{m})", i + 1))
                        .collect();
                }
                k = parts.len();
            }
        }
    }
}

/// Greedy fill by sentences, falling back to words for a sentence that does not fit.
fn pack(text: &str, budget: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for sentence in sentences(text) {
        if cur.is_empty() && len(&sentence) <= budget {
            cur = sentence;
        } else if !cur.is_empty() && len(&cur) + 1 + len(&sentence) <= budget {
            cur.push(' ');
            cur.push_str(&sentence);
        } else if len(&sentence) <= budget {
            parts.push(std::mem::replace(&mut cur, sentence));
        } else {
            // A sentence longer than a part: flush, then fill by words. The last piece stays
            // open so the next sentence can join it.
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            let mut pieces = pack_words(&sentence, budget);
            cur = pieces.pop().unwrap_or_default();
            parts.extend(pieces);
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

fn pack_words(text: &str, budget: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if len(word) > budget {
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            let chars: Vec<char> = word.chars().collect();
            let mut chunks: Vec<String> = chars.chunks(budget).map(|c| c.iter().collect()).collect();
            cur = chunks.pop().unwrap_or_default();
            parts.extend(chunks);
        } else if cur.is_empty() {
            cur = word.to_owned();
        } else if len(&cur) + 1 + len(word) <= budget {
            cur.push(' ');
            cur.push_str(word);
        } else {
            parts.push(std::mem::replace(&mut cur, word.to_owned()));
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// A sentence ends at a word that ends in `. ! ? …`, optionally followed by closing quotes,
/// brackets or `*` (so `*bows.*` ends a sentence).
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        cur.push(word);
        let core = word.trim_end_matches(['"', '\'', ')', '*', '”', '’']);
        if core.ends_with(['.', '!', '?', '…']) {
            out.push(cur.join(" "));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.join(" "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = crate::game::WOW.chat_limit;

    fn all_fit(parts: &[String]) {
        for p in parts {
            assert!(len(p) <= LIMIT, "part of {} chars: {p}", len(p));
        }
    }

    fn sentence(i: usize) -> String {
        format!("Sentence number {i} is here for the test.")
    }

    #[test]
    fn short_text_is_one_unmarked_part() {
        for m in [Marker::Numbered, Marker::Ellipsis, Marker::None] {
            assert_eq!(split("Well met.", m, LIMIT), vec!["Well met."]);
        }
    }

    #[test]
    fn exactly_255_is_one_part() {
        let t = "a".repeat(255);
        assert_eq!(split(&t, Marker::Numbered, LIMIT), vec![t]);
    }

    #[test]
    fn exactly_256_is_two_parts() {
        let t = format!("{} {}", "a".repeat(127), "b".repeat(128)); // 256 chars
        assert_eq!(len(&t), 256);
        let parts = split(&t, Marker::Numbered, LIMIT);
        assert_eq!(parts.len(), 2);
        all_fit(&parts);
        assert!(parts[0].ends_with("(1/2)") && parts[1].ends_with("(2/2)"));
    }

    #[test]
    fn splits_on_sentence_boundaries() {
        let text: String = (0..14).map(sentence).collect::<Vec<_>>().join(" ");
        let parts = split(&text, Marker::None, LIMIT);
        assert!(parts.len() > 1);
        all_fit(&parts);
        for p in &parts {
            assert!(p.ends_with('.'), "{p}");
            assert!(p.starts_with("Sentence"), "{p}");
        }
    }

    #[test]
    fn six_hundred_chars_make_at_most_three_parts() {
        let mut text = String::new();
        let mut i = 0;
        while len(&text) < 600 {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&sentence(i));
            i += 1;
        }
        let parts = split(&text, Marker::Numbered, LIMIT);
        assert!(parts.len() <= 3, "{} parts", parts.len());
        all_fit(&parts);
        for p in &parts {
            assert!(p.contains(". (") , "ends on a sentence: {p}");
        }
    }

    #[test]
    fn long_sentence_falls_back_to_words() {
        let text = "word ".repeat(100).trim().to_owned() + ".";
        let parts = split(&text, Marker::None, LIMIT);
        all_fit(&parts);
        assert!(parts.len() >= 2);
        for p in &parts {
            assert!(!p.starts_with(' ') && !p.ends_with(' '));
            for w in p.split(' ') {
                assert!(w == "word" || w == "word.", "cut a word: {w}");
            }
        }
    }

    #[test]
    fn one_huge_word_is_cut_only_because_it_must() {
        let text = "x".repeat(600);
        let parts = split(&text, Marker::Numbered, LIMIT);
        all_fit(&parts);
        assert_eq!(parts.len(), 3);
        let joined: String = parts
            .iter()
            .map(|p| p.rsplit_once(" (").unwrap().0)
            .collect();
        assert_eq!(joined, text);
    }

    #[test]
    fn many_short_sentences() {
        let text = "Hi. ".repeat(200).trim().to_owned();
        let parts = split(&text, Marker::Numbered, LIMIT);
        all_fit(&parts);
        assert!(parts.len() >= 4);
        let m = parts.len();
        assert!(parts.last().unwrap().ends_with(&format!("({m}/{m})")));
    }

    #[test]
    fn emote_text_ends_sentences_at_the_closing_star() {
        let emote = "*bows deeply and raises both hands to the sky.*";
        let text = format!("{emote} {emote} {emote} {emote} {emote} {emote}");
        let parts = split(&text, Marker::None, LIMIT);
        all_fit(&parts);
        assert!(parts.len() >= 2);
        for p in &parts {
            assert!(p.starts_with('*') && p.ends_with('*'), "{p}");
        }
    }

    #[test]
    fn cyrillic_is_counted_in_characters_not_bytes() {
        let text = "Привет, друг. ".repeat(30).trim().to_owned();
        assert!(text.len() > 255 && len(&text) > 255);
        let parts = split(&text, Marker::None, LIMIT);
        all_fit(&parts);
        // 255 Cyrillic chars would be 510 bytes: a byte-based splitter would cut far too early.
        assert!(parts.iter().any(|p| len(p) > 200));
        // 255 Cyrillic chars in one go stay one part.
        assert_eq!(split(&"я".repeat(255), Marker::Numbered, LIMIT).len(), 1);
    }

    #[test]
    fn ellipsis_marker_is_on_all_but_the_last_part() {
        let text: String = (0..14).map(sentence).collect::<Vec<_>>().join(" ");
        let parts = split(&text, Marker::Ellipsis, LIMIT);
        all_fit(&parts);
        let last = parts.len() - 1;
        for (i, p) in parts.iter().enumerate() {
            assert_eq!(p.ends_with('…'), i != last, "{p}");
        }
    }

    #[test]
    fn numbered_marker_counts_up_and_total_matches() {
        let text: String = (0..30).map(sentence).collect::<Vec<_>>().join(" ");
        let parts = split(&text, Marker::Numbered, LIMIT);
        all_fit(&parts);
        let m = parts.len();
        assert!(m >= 5);
        for (i, p) in parts.iter().enumerate() {
            assert!(p.ends_with(&format!(" ({}/{m})", i + 1)), "{p}");
        }
    }

    #[test]
    fn marker_width_with_two_digit_totals_still_fits() {
        // Enough parts to reach (10/10)+; parts must stay within the limit.
        let text = "x".repeat(255 * 12);
        let parts = split(&text, Marker::Numbered, LIMIT);
        assert!(parts.len() >= 10);
        all_fit(&parts);
    }

    #[test]
    fn limit_is_a_parameter_not_a_constant() {
        let text: String = (0..6).map(sentence).collect::<Vec<_>>().join(" ");
        for limit in [60, 100, 400] {
            for m in [Marker::Numbered, Marker::Ellipsis, Marker::None] {
                for p in split(&text, m, limit) {
                    assert!(len(&p) <= limit, "limit {limit}: {p}");
                }
            }
        }
        assert_eq!(split(&text, Marker::None, 400).len(), 1);
        assert!(split(&text, Marker::None, 100).len() > 2);
    }

    #[test]
    fn marker_settings_parse() {
        assert_eq!(Marker::from_setting("(n/m)"), Marker::Numbered);
        assert_eq!(Marker::from_setting("…"), Marker::Ellipsis);
        assert_eq!(Marker::from_setting("none"), Marker::None);
        assert_eq!(Marker::from_setting("??"), Marker::Numbered);
    }

    #[test]
    fn nothing_is_lost_or_reordered_without_a_marker() {
        let text: String = (0..20).map(sentence).collect::<Vec<_>>().join(" ");
        assert_eq!(split(&text, Marker::None, LIMIT).join(" "), text);
    }
}
