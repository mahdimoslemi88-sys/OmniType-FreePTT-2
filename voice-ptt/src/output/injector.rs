//! Text injection via Win32 `SendInput`.
//!
//! The draft spec injected one `wVk` per character, which **cannot type
//! Persian** (virtual key codes only cover the active keyboard layout). The
//! correct approach for Unicode text is `KEYEVENTF_UNICODE` key-down/up
//! pairs carrying UTF-16 code units — layout-independent, works for any
//! script, and still reaches the focused window.
//!
//! Batch size note: `SendInput` is atomic per call (all events processed
//! together), so we send in blocks of 32 events for speed.

use anyhow::Result;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE,
};

/// How many INPUT structs to send per `SendInput` call.
const BATCH: usize = 32;

/// Injects `text` into the currently focused window as synthetic keystrokes.
pub fn inject_text(text: &str) -> Result<usize> {
    if text.is_empty() {
        return Ok(0);
    }

    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);

    for unit in text.encode_utf16() {
        // Down
        inputs.push(make_unicode_input(unit, false));
        // Up
        inputs.push(make_unicode_input(unit, true));

        if inputs.len() >= BATCH {
            flush(&mut inputs)?;
        }
    }
    if !inputs.is_empty() {
        flush(&mut inputs)?;
    }

    Ok(text.chars().count())
}

/// Injects a plain `\n` as the Enter key (keystroke, not a Unicode char).
pub fn press_enter() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_RETURN};
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_RETURN,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                ..Default::default()
            },
        },
    };
    let mut up = down;
    up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

    unsafe {
        let sent = SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        if sent != 2 {
            anyhow::bail!("SendInput failed for Enter: sent {sent}");
        }
    }
    Ok(())
}

fn make_unicode_input(code_unit: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn flush(inputs: &mut Vec<INPUT>) -> Result<()> {
    let sent = unsafe { SendInput(inputs.as_slice(), std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        anyhow::bail!(
            "SendInput delivered {sent} of {} events (another app may be blocking input)",
            inputs.len()
        );
    }
    inputs.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_units_are_counted_correctly() {
        // "سلام" = 4 chars, all BMP → 8 INPUT events (down+up each).
        let inputs: Vec<INPUT> = "سلام"
            .encode_utf16()
            .flat_map(|u| [make_unicode_input(u, false), make_unicode_input(u, true)])
            .collect();
        assert_eq!(inputs.len(), 8);

        // Emoji (surrogate pair) = 1 char but 2 UTF-16 units → 4 events.
        let inputs: Vec<INPUT> = "🚀"
            .encode_utf16()
            .flat_map(|u| [make_unicode_input(u, false), make_unicode_input(u, true)])
            .collect();
        assert_eq!(inputs.len(), 4);
    }

    /// End-to-end typing requires a desktop session and a focused window;
    /// here we only verify empty input is a no-op that never calls SendInput.
    #[test]
    fn empty_text_is_noop() {
        assert_eq!(inject_text("").unwrap(), 0);
    }
}
