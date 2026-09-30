//! Parse user-facing hotkey strings like `"CapsLock"`, `"Ctrl+Alt+S"`, or
//! `"Shift+F5"` into a form the polling thread can evaluate with
//! `GetAsyncKeyState`.
//!
//! Grammar (case-insensitive, `+` separated, whitespace tolerated):
//! ```text
//! binding := [modifier '+']* key
//! modifier := "ctrl" | "control" | "alt" | "menu" | "shift"
//! key      := a single logical key name (see parse_key) or a single character
//! ```
//! At most one non-modifier key is allowed; modifiers may appear in any order
//! but duplicates are rejected.

use std::fmt;

/// A parsed hotkey binding: a main key plus optional modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
    /// The main (non-modifier) key.
    pub key: Key,
    /// Modifiers that must all be held. Order is irrelevant.
    pub modifiers: Vec<Modifier>,
}

/// A logical key that can be bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    CapsLock,
    Space,
    Tab,
    Enter,
    Escape,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    LeftMouse,
    RightMouse,
    /// A single character, e.g. 'a'..'z', '0'..'9', or punctuation.
    Char(char),
    /// A function key F1..F24.
    Fn(u8),
    /// A numpad digit 0..9.
    Numpad(u8),
}

/// A keyboard modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
}

/// Why a hotkey string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyParseError {
    /// The string was empty or only whitespace.
    Empty,
    /// More than one non-modifier key was found.
    TooManyKeys,
    /// No key (only modifiers) was found.
    NoKey,
    /// A token matched neither a modifier nor a known key name.
    UnknownToken(String),
    /// A modifier was repeated, e.g. `"Ctrl+Ctrl+S"`.
    DuplicateModifier,
}

impl fmt::Display for HotkeyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "hotkey string is empty"),
            Self::TooManyKeys => write!(f, "hotkey has more than one key"),
            Self::NoKey => write!(f, "hotkey has only modifiers and no key"),
            Self::UnknownToken(t) => write!(f, "unknown key name: {t}"),
            Self::DuplicateModifier => write!(f, "hotkey repeats a modifier"),
        }
    }
}

impl std::error::Error for HotkeyParseError {}

impl HotkeyBinding {
    /// Parses a binding string. Returns `Ok` only for well-formed bindings.
    pub fn parse(s: &str) -> Result<Self, HotkeyParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(HotkeyParseError::Empty);
        }

        // Split on '+' and also tolerate pure whitespace separation, since
        // users type both "Ctrl+Alt+S" and "Ctrl Alt S".
        let tokens: Vec<&str> = s
            .split('+')
            .flat_map(|part| part.split_whitespace())
            .collect();
        if tokens.is_empty() {
            return Err(HotkeyParseError::Empty);
        }

        let mut modifiers = Vec::new();
        let mut key = None;

        for token in tokens {
            let lower = token.to_lowercase();
            let modifier = match lower.as_str() {
                "ctrl" | "control" => Some(Modifier::Ctrl),
                "alt" | "menu" => Some(Modifier::Alt),
                "shift" => Some(Modifier::Shift),
                _ => None,
            };
            if let Some(m) = modifier {
                if modifiers.contains(&m) {
                    return Err(HotkeyParseError::DuplicateModifier);
                }
                modifiers.push(m);
                continue;
            }

            // Non-modifier token: there can be only one.
            if key.is_some() {
                return Err(HotkeyParseError::TooManyKeys);
            }
            key = Some(parse_key(&lower)?);
        }

        let key = key.ok_or(HotkeyParseError::NoKey)?;
        Ok(Self { key, modifiers })
    }

    /// True if this binding is the push-to-talk default (CapsLock alone).
    pub fn is_plain_caps_lock(&self) -> bool {
        self.key == Key::CapsLock && self.modifiers.is_empty()
    }

    /// Config string for this binding: `CapsLock`, `Ctrl+Alt+S`, `Shift+F5`.
    ///
    /// This is exactly what the key-capture UI writes back into `config.toml`,
    /// and it always parses back into an equal binding.
    pub fn to_token_string(&self) -> String {
        // Canonical modifier order (Ctrl, Alt, Shift) so `Shift+Ctrl+S` and
        // `Ctrl+Shift+S` are stored as the same string.
        let mut parts: Vec<String> = [Modifier::Ctrl, Modifier::Alt, Modifier::Shift]
            .into_iter()
            .filter(|m| self.modifiers.contains(m))
            .map(|m| {
                match m {
                    Modifier::Ctrl => "Ctrl",
                    Modifier::Alt => "Alt",
                    Modifier::Shift => "Shift",
                }
                .to_string()
            })
            .collect();
        parts.push(self.key.to_token());
        parts.join("+")
    }

    /// Lowercase canonical string, useful for config round-tripping and tests.
    pub fn to_canonical_string(&self) -> String {
        self.to_token_string().to_lowercase()
    }
}

impl Key {
    /// User-facing token for this key: `CapsLock`, `F5`, `S`, `Numpad5`.
    ///
    /// Inverse of the vocabulary [`HotkeyBinding::parse`] accepts; every token
    /// produced here parses back to the same key.
    pub fn to_token(&self) -> String {
        match self {
            Key::Char(c) => c.to_ascii_uppercase().to_string(),
            Key::Fn(n) => format!("F{n}"),
            Key::Numpad(n) => format!("Numpad{n}"),
            other => named_key_token(*other).to_string(),
        }
    }
}

/// Parses a single key token (already lowercased).
fn parse_key(lower: &str) -> Result<Key, HotkeyParseError> {
    // A single ASCII character maps to Char (covers "a".."z", "0".."9", and
    // punctuation like "," "." "/"). Digits are normalized to `Char` too —
    // the VK code layer treats them identically via `char_to_vk`.
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Ok(Key::Char(c));
    }

    let key = match lower {
        "capslock" | "caps" | "capital" => Key::CapsLock,
        "space" | "spacebar" => Key::Space,
        "tab" => Key::Tab,
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "backspace" | "back" => Key::Backspace,
        "del" | "delete" => Key::Delete,
        "ins" | "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" | "prior" => Key::PageUp,
        "pagedown" | "pgdn" | "next" => Key::PageDown,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "scrolllock" | "scroll" => Key::ScrollLock,
        "numlock" => Key::NumLock,
        "printscreen" | "prtsc" | "print" => Key::PrintScreen,
        "pause" | "break" => Key::Pause,
        "mouse1" | "lmb" | "leftmouse" | "leftclick" => Key::LeftMouse,
        "mouse2" | "rmb" | "rightmouse" | "rightclick" => Key::RightMouse,
        "f1" => Key::Fn(1),
        "f2" => Key::Fn(2),
        "f3" => Key::Fn(3),
        "f4" => Key::Fn(4),
        "f5" => Key::Fn(5),
        "f6" => Key::Fn(6),
        "f7" => Key::Fn(7),
        "f8" => Key::Fn(8),
        "f9" => Key::Fn(9),
        "f10" => Key::Fn(10),
        "f11" => Key::Fn(11),
        "f12" => Key::Fn(12),
        "f13" => Key::Fn(13),
        "f14" => Key::Fn(14),
        "f15" => Key::Fn(15),
        "f16" => Key::Fn(16),
        "f17" => Key::Fn(17),
        "f18" => Key::Fn(18),
        "f19" => Key::Fn(19),
        "f20" => Key::Fn(20),
        "f21" => Key::Fn(21),
        "f22" => Key::Fn(22),
        "f23" => Key::Fn(23),
        "f24" => Key::Fn(24),
        "numpad0" | "num0" => Key::Numpad(0),
        "numpad1" | "num1" => Key::Numpad(1),
        "numpad2" | "num2" => Key::Numpad(2),
        "numpad3" | "num3" => Key::Numpad(3),
        "numpad4" | "num4" => Key::Numpad(4),
        "numpad5" | "num5" => Key::Numpad(5),
        "numpad6" | "num6" => Key::Numpad(6),
        "numpad7" | "num7" => Key::Numpad(7),
        "numpad8" | "num8" => Key::Numpad(8),
        "numpad9" | "num9" => Key::Numpad(9),
        _ => return Err(HotkeyParseError::UnknownToken(lower.to_string())),
    };
    Ok(key)
}

/// Canonical token of a key that has a fixed name (character and numbered
/// keys are handled by [`Key::to_token`]).
fn named_key_token(key: Key) -> &'static str {
    match key {
        Key::CapsLock => "CapsLock",
        Key::Space => "Space",
        Key::Tab => "Tab",
        Key::Enter => "Enter",
        Key::Escape => "Escape",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Insert => "Insert",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::Up => "Up",
        Key::Down => "Down",
        Key::ScrollLock => "ScrollLock",
        Key::NumLock => "NumLock",
        Key::PrintScreen => "PrintScreen",
        Key::Pause => "Pause",
        Key::LeftMouse => "Mouse1",
        Key::RightMouse => "Mouse2",
        // Unreachable through `Key::to_token`, which formats these itself.
        Key::Char(_) | Key::Fn(_) | Key::Numpad(_) => "?",
    }
}

// ---------------------------------------------------------------------------
// Win32 virtual-key mapping
// ---------------------------------------------------------------------------

/// Virtual-key code alias (opaque on non-Windows so the crate still compiles
/// for tests on other platforms).
#[cfg(windows)]
pub type VkCode = windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

#[cfg(not(windows))]
pub type VkCode = u16;

/// Converts a logical key to its Win32 virtual-key code.
///
/// Returns `None` for keys with no direct VK equivalent (should not happen for
/// any binding produced by `HotkeyBinding::parse`, but the caller treats
/// `None` as "unbindable" rather than panicking).
#[cfg(windows)]
pub fn key_to_vk(key: Key) -> Option<VkCode> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    Some(match key {
        Key::CapsLock => VK_CAPITAL,
        Key::Space => VK_SPACE,
        Key::Tab => VK_TAB,
        Key::Enter => VK_RETURN,
        Key::Escape => VK_ESCAPE,
        Key::Backspace => VK_BACK,
        Key::Delete => VK_DELETE,
        Key::Insert => VK_INSERT,
        Key::Home => VK_HOME,
        Key::End => VK_END,
        Key::PageUp => VK_PRIOR,
        Key::PageDown => VK_NEXT,
        Key::Left => VK_LEFT,
        Key::Right => VK_RIGHT,
        Key::Up => VK_UP,
        Key::Down => VK_DOWN,
        Key::ScrollLock => VK_SCROLL,
        Key::NumLock => VK_NUMLOCK,
        Key::PrintScreen => VK_SNAPSHOT,
        Key::Pause => VK_PAUSE,
        Key::LeftMouse => VK_LBUTTON,
        Key::RightMouse => VK_RBUTTON,
        Key::Char(c) => char_to_vk(c)?,
        Key::Fn(n) => match n {
            1 => VK_F1,
            2 => VK_F2,
            3 => VK_F3,
            4 => VK_F4,
            5 => VK_F5,
            6 => VK_F6,
            7 => VK_F7,
            8 => VK_F8,
            9 => VK_F9,
            10 => VK_F10,
            11 => VK_F11,
            12 => VK_F12,
            13 => VK_F13,
            14 => VK_F14,
            15 => VK_F15,
            16 => VK_F16,
            17 => VK_F17,
            18 => VK_F18,
            19 => VK_F19,
            20 => VK_F20,
            21 => VK_F21,
            22 => VK_F22,
            23 => VK_F23,
            24 => VK_F24,
            _ => return None,
        },
        Key::Numpad(n) => match n {
            0 => VK_NUMPAD0,
            1 => VK_NUMPAD1,
            2 => VK_NUMPAD2,
            3 => VK_NUMPAD3,
            4 => VK_NUMPAD4,
            5 => VK_NUMPAD5,
            6 => VK_NUMPAD6,
            7 => VK_NUMPAD7,
            8 => VK_NUMPAD8,
            9 => VK_NUMPAD9,
            _ => return None,
        },
    })
}

/// Maps a single ASCII character to its virtual-key code (letters use the
/// physical key, independent of the current keyboard layout).
#[cfg(windows)]
fn char_to_vk(c: char) -> Option<VkCode> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let upper = c.to_ascii_uppercase();
    Some(match upper {
        'A' => VK_A,
        'B' => VK_B,
        'C' => VK_C,
        'D' => VK_D,
        'E' => VK_E,
        'F' => VK_F,
        'G' => VK_G,
        'H' => VK_H,
        'I' => VK_I,
        'J' => VK_J,
        'K' => VK_K,
        'L' => VK_L,
        'M' => VK_M,
        'N' => VK_N,
        'O' => VK_O,
        'P' => VK_P,
        'Q' => VK_Q,
        'R' => VK_R,
        'S' => VK_S,
        'T' => VK_T,
        'U' => VK_U,
        'V' => VK_V,
        'W' => VK_W,
        'X' => VK_X,
        'Y' => VK_Y,
        'Z' => VK_Z,
        '0' => VK_0,
        '1' => VK_1,
        '2' => VK_2,
        '3' => VK_3,
        '4' => VK_4,
        '5' => VK_5,
        '6' => VK_6,
        '7' => VK_7,
        '8' => VK_8,
        '9' => VK_9,
        _ => return None,
    })
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn key_to_vk(_key: Key) -> Option<VkCode> {
    None
}

/// VK code of a key, as a plain `u16`.
///
/// A thin wrapper over [`key_to_vk`] so the hotkey poll loop can compare
/// against `GetAsyncKeyState` results without importing the Win32 `VIRTUAL_KEY`
/// newtype. `None` is preserved rather than defaulted: an unbindable key is a
/// real state that the caller must handle, not something to paper over.
pub(crate) fn key_vk_code(key: Key) -> Option<u16> {
    #[cfg(windows)]
    {
        key_to_vk(key).map(|vk| vk.0)
    }
    #[cfg(not(windows))]
    {
        let _ = key;
        None
    }
}

/// VK code of a modifier, as a plain `u16`.
pub(crate) fn modifier_vk_code(m: Modifier) -> u16 {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_MENU, VK_SHIFT};
        match m {
            Modifier::Ctrl => VK_CONTROL.0,
            Modifier::Alt => VK_MENU.0,
            Modifier::Shift => VK_SHIFT.0,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = m;
        0
    }
}

/// Converts a Win32 virtual-key code back into a logical key.
///
/// Inverse of [`key_to_vk`]; used by the key-capture path to name the key the
/// user actually pressed. Returns `None` for keys a binding cannot express
/// (OEM punctuation, browser/media keys, unused VK slots).
#[cfg(windows)]
pub fn vk_to_key(vk: u16) -> Option<Key> {
    // Plain numbers with the VK each arm stands for: the mapping is stable
    // Win32 ABI, and spelling it out keeps the inverse of `key_to_vk`
    // auditable at a glance.
    match vk {
        0x01 => Some(Key::LeftMouse),   // VK_LBUTTON
        0x02 => Some(Key::RightMouse),  // VK_RBUTTON
        0x08 => Some(Key::Backspace),   // VK_BACK
        0x09 => Some(Key::Tab),         // VK_TAB
        0x0D => Some(Key::Enter),       // VK_RETURN
        0x13 => Some(Key::Pause),       // VK_PAUSE
        0x14 => Some(Key::CapsLock),    // VK_CAPITAL
        0x1B => Some(Key::Escape),      // VK_ESCAPE
        0x20 => Some(Key::Space),       // VK_SPACE
        0x21 => Some(Key::PageUp),      // VK_PRIOR
        0x22 => Some(Key::PageDown),    // VK_NEXT
        0x23 => Some(Key::End),         // VK_END
        0x24 => Some(Key::Home),        // VK_HOME
        0x25 => Some(Key::Left),        // VK_LEFT
        0x26 => Some(Key::Up),          // VK_UP
        0x27 => Some(Key::Right),       // VK_RIGHT
        0x28 => Some(Key::Down),        // VK_DOWN
        0x2C => Some(Key::PrintScreen), // VK_SNAPSHOT
        0x2D => Some(Key::Insert),      // VK_INSERT
        0x2E => Some(Key::Delete),      // VK_DELETE
        0x90 => Some(Key::NumLock),     // VK_NUMLOCK
        0x91 => Some(Key::ScrollLock),  // VK_SCROLL
        // VK_0..VK_9
        0x30..=0x39 => Some(Key::Char((b'0' + (vk as u8 - 0x30)) as char)),
        // VK_A..VK_Z — lowercase, matching what `HotkeyBinding::parse` stores
        // for a bare letter token.
        0x41..=0x5A => Some(Key::Char((b'a' + (vk as u8 - 0x41)) as char)),
        // VK_NUMPAD0..VK_NUMPAD9
        0x60..=0x69 => Some(Key::Numpad((vk - 0x60) as u8)),
        // VK_F1..VK_F24
        0x70..=0x87 => Some(Key::Fn((vk - 0x6F) as u8)),
        _ => None,
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn vk_to_key(_vk: u16) -> Option<Key> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use Modifier::{Alt, Ctrl, Shift};

    #[test]
    fn parse_plain_key() {
        let b = HotkeyBinding::parse("CapsLock").unwrap();
        assert_eq!(b.key, Key::CapsLock);
        assert!(b.modifiers.is_empty());
        assert!(b.is_plain_caps_lock());
    }

    #[test]
    fn parse_with_modifiers() {
        let b = HotkeyBinding::parse("Ctrl+Alt+S").unwrap();
        assert_eq!(b.key, Key::Char('s'));
        assert_eq!(b.modifiers, vec![Ctrl, Alt]);
    }

    #[test]
    fn parse_is_case_insensitive_and_tolerates_spaces() {
        let b = HotkeyBinding::parse("  Shift  F5 ").unwrap();
        assert_eq!(b.key, Key::Fn(5));
        assert_eq!(b.modifiers, vec![Shift]);
    }

    #[test]
    fn parse_alternative_modifier_names() {
        let b = HotkeyBinding::parse("Control+Menu+Q").unwrap();
        assert_eq!(b.key, Key::Char('q'));
        assert_eq!(b.modifiers, vec![Ctrl, Alt]);
    }

    #[test]
    fn rejects_two_keys() {
        assert_eq!(
            HotkeyBinding::parse("A+B"),
            Err(HotkeyParseError::TooManyKeys)
        );
    }

    #[test]
    fn rejects_modifiers_only() {
        assert_eq!(
            HotkeyBinding::parse("Ctrl+Alt"),
            Err(HotkeyParseError::NoKey)
        );
    }

    #[test]
    fn rejects_unknown_token() {
        assert_eq!(
            HotkeyBinding::parse("Ctrl+Nonsense"),
            Err(HotkeyParseError::UnknownToken("nonsense".into()))
        );
    }

    #[test]
    fn rejects_duplicate_modifier() {
        assert_eq!(
            HotkeyBinding::parse("Ctrl+Ctrl+S"),
            Err(HotkeyParseError::DuplicateModifier)
        );
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(HotkeyBinding::parse(""), Err(HotkeyParseError::Empty));
        assert_eq!(HotkeyBinding::parse("   "), Err(HotkeyParseError::Empty));
    }

    #[test]
    fn parses_numpad_and_mouse() {
        assert_eq!(HotkeyBinding::parse("Numpad5").unwrap().key, Key::Numpad(5));
        assert_eq!(HotkeyBinding::parse("Mouse1").unwrap().key, Key::LeftMouse);
    }

    #[cfg(windows)]
    #[test]
    fn vk_codes_for_common_keys() {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        assert_eq!(key_to_vk(Key::CapsLock), Some(VK_CAPITAL));
        assert_eq!(key_to_vk(Key::Char('s')), Some(VK_S));
        assert_eq!(key_to_vk(Key::Fn(5)), Some(VK_F5));
        assert_eq!(key_to_vk(Key::Space), Some(VK_SPACE));
    }

    /// Everything the capture path can produce must survive a trip through
    /// `config.toml` (token → parse → equal binding).
    #[test]
    fn tokens_round_trip_through_parse() {
        for token in [
            "CapsLock",
            "Space",
            "Tab",
            "Enter",
            "Escape",
            "Backspace",
            "Delete",
            "Insert",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Left",
            "Right",
            "Up",
            "Down",
            "ScrollLock",
            "NumLock",
            "PrintScreen",
            "Pause",
            "Mouse1",
            "Mouse2",
            "F1",
            "F24",
            "Numpad0",
            "Numpad9",
            "A",
            "Z",
            "0",
            "9",
        ] {
            let parsed = HotkeyBinding::parse(token).unwrap_or_else(|e| panic!("{token}: {e}"));
            let again = HotkeyBinding::parse(&parsed.to_token_string())
                .unwrap_or_else(|e| panic!("{} re-parse: {e}", parsed.to_token_string()));
            assert_eq!(again, parsed, "token {token} did not round-trip");
        }
    }

    #[test]
    fn token_string_orders_modifiers_and_keeps_the_key() {
        assert_eq!(
            HotkeyBinding::parse("shift+control+f5")
                .unwrap()
                .to_token_string(),
            "Ctrl+Shift+F5"
        );
        assert_eq!(
            HotkeyBinding::parse("CapsLock").unwrap().to_token_string(),
            "CapsLock"
        );
    }

    #[cfg(windows)]
    #[test]
    fn vk_to_key_inverts_key_to_vk() {
        let keys = [
            Key::CapsLock,
            Key::Space,
            Key::Tab,
            Key::Enter,
            Key::Escape,
            Key::Backspace,
            Key::Delete,
            Key::Insert,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::ScrollLock,
            Key::NumLock,
            Key::PrintScreen,
            Key::Pause,
            Key::LeftMouse,
            Key::RightMouse,
            Key::Char('a'),
            Key::Char('z'),
            Key::Char('0'),
            Key::Char('9'),
            Key::Fn(1),
            Key::Fn(24),
            Key::Numpad(0),
            Key::Numpad(9),
        ];
        for key in keys {
            let vk = key_to_vk(key).unwrap_or_else(|| panic!("no VK for {key:?}"));
            assert_eq!(vk_to_key(vk.0), Some(key), "round-trip failed for {key:?}");
        }
        // Keys a binding cannot express must stay rejected.
        assert_eq!(vk_to_key(0x00), None);
        assert_eq!(vk_to_key(0xFE), None);
    }
}
