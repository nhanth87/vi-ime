// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Emoji shortcode + emoticon expansion ("😀 Emoji (shortcode)").
//!
//! Two recognized forms (both must END with a boundary the engine already
//! commits on — space/enter/punctuation — so expansion happens at word end,
//! never mid-token):
//!   - `:shortcode:`  — colon-delimited, e.g. `:smile:` → 😄, `:heart:` → ❤️
//!   - emoticons      — `:)` `:(` `:D` `:P` `;)` `<3` etc.
//!
//! Pure lookup; the interception state machine lives in `fast_engine.rs`.
//! Kept table-small on purpose — this is a convenience, not a full emoji
//! keyboard (that belongs in a picker, not the typing hot path).

/// Look up a `:shortcode:` (WITHOUT the surrounding colons) → emoji.
pub fn shortcode(name: &str) -> Option<&'static str> {
    Some(match name {
        "smile" => "😄",
        "grin" => "😁",
        "joy" => "😂",
        "laughing" | "lol" => "😆",
        "wink" => "😉",
        "blush" => "😊",
        "heart" => "❤️",
        "broken_heart" => "💔",
        "thumbsup" | "+1" => "👍",
        "thumbsdown" | "-1" => "👎",
        "ok_hand" => "👌",
        "clap" => "👏",
        "pray" | "thanks" => "🙏",
        "fire" => "🔥",
        "star" => "⭐",
        "sparkles" => "✨",
        "tada" | "party" => "🎉",
        "rocket" => "🚀",
        "eyes" => "👀",
        "thinking" => "🤔",
        "cry" => "😢",
        "sob" => "😭",
        "sad" => "😞",
        "angry" => "😠",
        "cool" | "sunglasses" => "😎",
        "kiss" => "😘",
        "love" | "heart_eyes" => "😍",
        "sweat_smile" => "😅",
        "wave" => "👋",
        "muscle" => "💪",
        "100" => "💯",
        "check" => "✅",
        "cross" | "x" => "❌",
        "warning" => "⚠️",
        "bug" => "🐛",
        "coffee" => "☕",
        "beer" => "🍺",
        "cake" => "🎂",
        "poop" => "💩",
        "ghost" => "👻",
        "skull" => "💀",
        "sun" => "☀️",
        "moon" => "🌙",
        "vn" | "vietnam" => "🇻🇳",
        _ => return None,
    })
}

/// Look up an emoticon (the WHOLE token, e.g. ":)") → emoji. Case-sensitive
/// where it matters (`:D` vs `:d` — only the classic uppercase forms map).
pub fn emoticon(token: &str) -> Option<&'static str> {
    Some(match token {
        ":)" | ":-)" => "🙂",
        ":D" | ":-D" => "😄",
        ":(" | ":-(" => "🙁",
        ":'(" => "😢",
        ":P" | ":-P" | ":p" | ":-p" => "😛",
        ";)" | ";-)" => "😉",
        ":o" | ":O" | ":-o" | ":-O" => "😮",
        ":|" | ":-|" => "😐",
        ":*" | ":-*" => "😘",
        "<3" => "❤️",
        "</3" => "💔",
        "XD" | "xD" => "😆",
        ":3" => "😺",
        "^^" | "^_^" => "😊",
        "T_T" | "T.T" => "😭",
        ">:(" => "😠",
        _ => return None,
    })
}

/// Every emoticon token the table knows — used for prefix matching so the
/// capture state machine keeps buffering only while a longer match is possible.
const EMOTICONS: &[&str] = &[
    ":)", ":-)", ":D", ":-D", ":(", ":-(", ":'(", ":P", ":-P", ":p", ":-p",
    ";)", ";-)", ":o", ":O", ":-o", ":-O", ":|", ":-|", ":*", ":-*",
    "<3", "</3", ":3", "^^", "^_^", ">:(",
];

/// Can `s` still grow into some emoticon (is it a strict prefix of one)?
/// `":"` → true (many), `":)"` → false (already complete, no longer). Used to
/// decide whether an in-progress emoticon capture should keep going.
pub fn emoticon_prefix(s: &str) -> bool {
    EMOTICONS.iter().any(|e| e.len() > s.len() && e.starts_with(s))
}

/// Which ASCII chars may START an emoji capture. All are word-boundary
/// punctuation in the engine (never the first key of a Vietnamese syllable),
/// so beginning a capture here can't steal a keystroke from Vietnamese typing.
pub fn is_starter(ch: char) -> bool {
    matches!(ch, ':' | ';' | '<' | '^')
}

/// A char valid inside a `:shortcode:` name (between the colons).
pub fn is_shortcode_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-')
}

// Manual test (policy: no automation tests): with emoji ON, in a text field
// type `:)` then space → 🙂. Type `:smile:` → 😄. Type `:notareal:` → stays
// literal `:notareal:`. With emoji OFF, `:)` stays `:)`. In LibreOffice
// (evdev native keymap) emoji cannot be injected — see fast_engine note.
