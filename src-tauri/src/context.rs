//! Per-character ring buffer of recent chat (in memory only). It feeds `<recent_chat>`.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

pub const CAPACITY: usize = 10;
const MAX_LINE_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub direction: Direction,
    pub text: String,
}

#[derive(Default)]
pub struct ChatContext {
    by_character: Mutex<HashMap<String, VecDeque<Entry>>>,
}

impl ChatContext {
    pub fn push(&self, character: &str, direction: Direction, text: &str) {
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text: String = flat.chars().take(MAX_LINE_CHARS).collect();
        if text.is_empty() {
            return;
        }
        let mut map = self.by_character.lock().unwrap_or_else(|e| e.into_inner());
        let buf = map.entry(character.to_owned()).or_default();
        if buf.len() == CAPACITY {
            buf.pop_front();
        }
        buf.push_back(Entry { direction, text });
    }

    /// Oldest first.
    pub fn recent(&self, character: &str) -> Vec<Entry> {
        let map = self.by_character.lock().unwrap_or_else(|e| e.into_inner());
        map.get(character)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_last_ten_per_character() {
        let c = ChatContext::default();
        for i in 0..12 {
            c.push("a", Direction::In, &format!("line {i}"));
        }
        c.push("b", Direction::Out, "other");
        let a = c.recent("a");
        assert_eq!(a.len(), CAPACITY);
        assert_eq!(a[0].text, "line 2");
        assert_eq!(a[9].text, "line 11");
        assert_eq!(c.recent("b").len(), 1);
        assert!(c.recent("nobody").is_empty());
    }

    #[test]
    fn flattens_lines_and_caps_length() {
        let c = ChatContext::default();
        c.push("a", Direction::In, "one\n  two\r\nthree");
        c.push("a", Direction::In, &"x".repeat(1000));
        c.push("a", Direction::In, "   ");
        let r = c.recent("a");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].text, "one two three");
        assert_eq!(r[1].text.chars().count(), MAX_LINE_CHARS);
    }
}
