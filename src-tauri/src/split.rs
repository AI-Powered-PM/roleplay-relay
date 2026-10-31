//! WoW chat limit and the splitter hook.
//!
//! Milestone 4 replaces `split_for_chat` with the real 255-character splitter. Until then the
//! whole text is one part, so everything that already calls this (the clipboard write, the
//! Output guard's "more than 4 parts" rule) keeps working unchanged once the splitter lands.

/// WoW chat messages are limited to 255 characters.
pub const CHAT_LIMIT: usize = 255;

/// TODO(M4): sentence/word splitting with the `(n/m)` marker. Part 1 goes to the clipboard.
pub fn split_for_chat(text: &str) -> Vec<String> {
    vec![text.to_owned()]
}
