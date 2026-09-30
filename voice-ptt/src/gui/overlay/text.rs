//! Text shaping and caption formatting for the overlay.
//!
//! Persian is cursive and right-to-left; egui lays out left-to-right with no
//! script awareness, so every user-visible string passes through
//! [`format_persian_display`] first. Centralising that here is what lets the
//! panels treat a label as an opaque `&str`.

use ar_reshaper::ArabicReshaper;
use eframe::egui;
use egui_phosphor::regular as ic;
use unicode_bidi::BidiInfo;

/// Reshapes Persian/Arabic cursive text and reorders visually for LTR renderers like egui.
pub fn format_persian_display(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Fast check: if text contains any Arabic/Persian/Presentation-form characters
    let has_persian = trimmed.chars().any(|c| {
        ('\u{0600}'..='\u{06FF}').contains(&c)
            || ('\u{FB50}'..='\u{FDFF}').contains(&c)
            || ('\u{FE70}'..='\u{FEFF}').contains(&c)
    });

    if !has_persian {
        return trimmed.to_string();
    }

    let reshaped = ArabicReshaper::default().reshape(trimmed);
    let bidi_info = BidiInfo::new(&reshaped, None);
    let mut visual = String::new();
    for para in &bidi_info.paragraphs {
        let mut line = bidi_info
            .reorder_line(para, para.range.clone())
            .into_owned();
        // Rule L4 (mirroring): inside an RTL paragraph, mirrored characters
        // such as ( ) [ ] { } < > must be visually swapped. unicode-bidi
        // deliberately leaves this to the engine; without it, parentheses in
        // Persian text render as `)(` — reversed.
        if para.level.is_rtl() {
            mirror_chars(&mut line);
        }
        if !visual.is_empty() {
            visual.push(' ');
        }
        visual.push_str(&line);
    }
    visual
}
/// Replaces each mirrored-punctuation character with its mirror image
/// (Unicode Bidi Rule L4). Only applied to RTL paragraphs.
pub(crate) fn mirror_chars(s: &mut String) {
    let mirrored: Vec<char> = s
        .chars()
        .map(|c| match c {
            '(' => ')',
            ')' => '(',
            '[' => ']',
            ']' => '[',
            '{' => '}',
            '}' => '{',
            '<' => '>',
            '>' => '<',
            '‹' => '›',
            '›' => '‹',
            '«' => '»',
            '»' => '«',
            '⁅' => '⁆',
            '⁆' => '⁅',
            '⁽' => '⁾',
            '⁾' => '⁽',
            _ => c,
        })
        .collect();
    *s = mirrored.into_iter().collect();
}
/// A `TextEdit` layouter that reshapes Persian text so letters connect
/// properly. egui has no built-in Arabic shaping, so a raw `TextEdit` shows
/// every Persian letter disconnected ("ک ل م ه" instead of "کلمه"). This lays
/// the buffer's text out through `format_persian_display`, while the editable
/// buffer itself keeps the original keystrokes.
pub fn persian_text_edit_layouter(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let shaped = format_persian_display(text);
    let valign = ui.layout().vertical_align();
    let mut layout_job = egui::text::LayoutJob::single_section(
        shaped,
        egui::text::TextFormat {
            font_id: egui::FontSelection::Default.resolve(ui.style()),
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    layout_job.wrap = egui::text::TextWrapping::wrap_at_width(wrap_width);
    layout_job.sections[0].format.valign = valign;
    ui.fonts(|f| f.layout_job(layout_job))
}
/// Converts the latest egui key press into a hotkey binding token such as
/// "CapsLock", "Ctrl+Alt+S", or "Shift+F5". Returns  until a real
/// (non-modifier) key goes down.
///
/// egui 0.28 has no , so the physical key is detected from the
/// raw  stream by its logical-key value, which winit reports for
/// CapsLock even though egui does not model it.
pub(crate) fn egui_key_to_hotkey_token(ctx: &egui::Context) -> Option<String> {
    let modifiers = ctx.input(|i| i.modifiers);
    let key = ctx.input(|i| {
        i.events.iter().find_map(|ev| match ev {
            egui::Event::Key {
                key, pressed: true, ..
            } => Some(*key),
            _ => None,
        })
    })?;

    // egui only reports modifier keys here when they are pressed alone; we
    // wait for the actual key the user binds.
    match key {
        egui::Key::A
        | egui::Key::B
        | egui::Key::C
        | egui::Key::D
        | egui::Key::E
        | egui::Key::F
        | egui::Key::G
        | egui::Key::H
        | egui::Key::I
        | egui::Key::J
        | egui::Key::K
        | egui::Key::L
        | egui::Key::M
        | egui::Key::N
        | egui::Key::O
        | egui::Key::P
        | egui::Key::Q
        | egui::Key::R
        | egui::Key::S
        | egui::Key::T
        | egui::Key::U
        | egui::Key::V
        | egui::Key::W
        | egui::Key::X
        | egui::Key::Y
        | egui::Key::Z => {}
        egui::Key::Space
        | egui::Key::Tab
        | egui::Key::Enter
        | egui::Key::Escape
        | egui::Key::Backspace
        | egui::Key::Delete
        | egui::Key::Insert
        | egui::Key::Home
        | egui::Key::End
        | egui::Key::PageUp
        | egui::Key::PageDown
        | egui::Key::ArrowUp
        | egui::Key::ArrowDown
        | egui::Key::ArrowLeft
        | egui::Key::ArrowRight
        | egui::Key::F1
        | egui::Key::F2
        | egui::Key::F3
        | egui::Key::F4
        | egui::Key::F5
        | egui::Key::F6
        | egui::Key::F7
        | egui::Key::F8
        | egui::Key::F9
        | egui::Key::F10
        | egui::Key::F11
        | egui::Key::F12 => {}
        _ => return None,
    }

    let mut parts: Vec<String> = Vec::new();
    if modifiers.ctrl {
        parts.push("Ctrl".into());
    }
    if modifiers.alt {
        parts.push("Alt".into());
    }
    if modifiers.shift {
        parts.push("Shift".into());
    }
    parts.push(format!("{:?}", key));
    Some(parts.join("+"))
}
/// Wraps the (already shaped) transcript into a compact multi-line caption
/// with a live countdown footer (`⏱ Ns`). Only the preview is truncated —
/// the full raw text is what click-to-copy puts on the clipboard.
///
/// Wrapping is done on the *raw* source text at word boundaries (never
/// mid-word), then each completed line is shaped. Splits on the raw text so
/// that `format_persian_display` (reshaping + bidi) is applied per finished
/// line; splitting the already-shaped string breaks the ligatures.
pub(crate) fn toast_caption(raw: &str, remaining_secs: u64) -> String {
    const CHARS_PER_LINE: usize = 44;
    const MAX_LINES: usize = 3;

    // Greedy word-wrap: accumulate words while the line stays within budget.
    // A single word longer than the budget is emitted whole rather than cut.
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in raw.split_whitespace() {
        let extra = if current.is_empty() { 0 } else { 1 };
        if current.chars().count() + extra + word.chars().count() > CHARS_PER_LINE
            && !current.is_empty()
        {
            lines.push(std::mem::take(&mut current));
            if lines.len() == MAX_LINES {
                break;
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() && lines.len() < MAX_LINES {
        lines.push(current);
    }

    // Truncation marker when the transcript needed more than MAX_LINES.
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format_persian_display(line));
    }
    if raw.split_whitespace().count() > 0 && lines.len() == MAX_LINES {
        // Greedy wrap stops after 3 lines; detect overflow by comparing the
        // characters the lines cover against the whole source.
        let covered: usize = lines.iter().map(|l| l.chars().count()).sum::<usize>() + MAX_LINES - 1;
        if covered < raw.chars().count() {
            out.push_str(&format_persian_display(" …"));
        }
    }
    out.push('\n');
    out.push_str(&format!("{} {}s", ic::TIMER, remaining_secs));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_caption_wraps_and_appends_countdown() {
        // Short text: body line + footer, no blank spacer row (compact card).
        let short = toast_caption("Hello", 10);
        assert_eq!(short.lines().count(), 2);
        assert_eq!(short, format!("Hello\n{} 10s", ic::TIMER));

        // Long single-word text: wraps without ever cutting mid-word. One
        // 200-char word fills one line per toast_caption call, so the caption
        // is body + footer (the word is a single unbreakable token).
        let long_text = "x".repeat(200);
        let long = toast_caption(&long_text, 7);
        let body_lines = long.lines().count() - 1; // minus footer
        assert_eq!(body_lines, 1);
        assert!(!long.contains('…'));
        assert!(long.ends_with(" 7s"));

        // Many words across the 44-char budget wrap at word boundaries and
        // cap at 3 lines; the truncation marker appears when the source needs
        // a 4th line.
        let words = "aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp qq rr ss tt uu vv ww xx yy zz 11 22 33 44 55 66 77 88 99 q1 q2 q3 q4 q5 q6 q7 q8 q9 q0 z1 z2 z3 z4 z5 z6 z7 z8 z9 z0 y1 y2 y3 y4 y5 y6 y7 y8 y9 y0";
        let capped = toast_caption(words, 7);
        let body_lines = capped.lines().count() - 1;
        assert_eq!(body_lines, 3);
        assert!(capped.contains('…'), "no truncation marker");
        assert!(capped.ends_with(" 7s"));
    }

    #[test]
    fn test_toast_caption_never_splits_words() {
        // A 44-char budget must fit "aa bb cc dd ee ff gg hh ii jj kk ll"
        // (12 words × 2 + 11 spaces = 35) on one line, never breaking inside
        // a word.
        let words = "aa bb cc dd ee ff gg hh ii jj kk ll";
        let caption = toast_caption(words, 5);
        let first_line = caption.lines().next().unwrap();
        assert!(first_line.contains("ll"), "line 1 = {first_line}");
        assert!(!first_line.contains("lm"), "word split across lines");
        assert!(caption.ends_with(" 5s"));
    }

    #[test]
    fn test_toast_caption_keeps_line_order() {
        // First word must stay on the FIRST line, not the last — the old
        // char-chopping implementation reversed the visual order for RTL.
        // The output is shaped, so compare against the shaped expectation.
        let caption = toast_caption("اول وسط آخر", 3);
        let first_line = caption.lines().next().unwrap();
        assert_eq!(first_line, format_persian_display("اول وسط آخر"));
        assert!(caption.ends_with(" 3s"));
    }
}
