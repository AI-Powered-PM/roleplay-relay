//! Everything specific to one game. No other module hardcodes these values; they read
//! [`active`]. MVP: one profile, always active, no UI.

pub struct GameProfile {
    /// Short id, for logs.
    pub id: &'static str,
    /// For logs only (docs name the game by hand).
    pub display_name: &'static str,
    /// Longest chat message, in Unicode scalar values.
    pub chat_limit: usize,
    /// Most chat messages one reply may be split into (block if more).
    pub max_parts: usize,
    /// Characters removed from outgoing text (chat escape sequences).
    pub strip_chars: &'static [char],
}

/// World of Warcraft: 255-character chat lines; `|c` `|H` `|T` start escape sequences, and
/// malformed or fake ones can get a message rejected.
pub const WOW: GameProfile = GameProfile {
    id: "wow",
    display_name: "World of Warcraft",
    chat_limit: 255,
    max_parts: 4,
    strip_chars: &['|'],
};

pub fn active() -> &'static GameProfile {
    &WOW
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wow_profile_values() {
        let p = active();
        assert_eq!(p.id, "wow");
        assert_eq!(p.chat_limit, 255);
        assert_eq!(p.max_parts, 4);
        assert_eq!(p.strip_chars, ['|']);
    }
}
