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
///
/// **Idempotent**: text that already carries Arabic presentation forms has
/// already been through here — the reshaper is the only thing in the program
/// that produces them — so it is returned untouched. Reordering an
/// already-reordered line is a second reversal, which is how a helper that
/// formats its argument plus a call site that had *also* formatted it turned a
/// readable Persian sentence into the backwards box the user red-circled in the
/// dictionary and profiles tabs. A double call is now a no-op rather than a
/// corruption.
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

    // Already shaped once: shaping again would reshape nothing and reorder the
    // visual line back into reading order, which renders reversed.
    if has_presentation_forms(trimmed) {
        return trimmed.to_string();
    }

    shape_once(trimmed)
}

/// One pass of reshape + reorder, with no idempotence guard.
///
/// Split out so the guard can be tested against the thing it guards: the defect
/// that shipped was a *second* pass over text that had already been through the
/// first one, and [`the_unguarded_second_pass_reads_backwards`] shows exactly
/// what that pass does — which is the only way to say the red-circled boxes were
/// caused by it rather than merely coincident with it.
fn shape_once(text: &str) -> String {
    let reshaped = ArabicReshaper::default().reshape(text);
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
/// Whether `text` already carries Arabic **presentation forms** — the
/// U+FB50–U+FDFF and U+FE70–U+FEFF blocks the reshaper writes.
///
/// This is the one observable difference between "text as the user's dictation
/// produced it" and "text this module has already laid out for egui", and it is
/// what [`format_persian_display`] uses to stay idempotent.
fn has_presentation_forms(text: &str) -> bool {
    text.chars()
        .any(|c| ('\u{FB50}'..='\u{FDFF}').contains(&c) || ('\u{FE70}'..='\u{FEFF}').contains(&c))
}

/// A `TextEdit` layouter that reshapes Persian text so letters connect
/// properly. egui has no built-in Arabic shaping, so a raw `TextEdit` shows
/// every Persian letter disconnected ("ک ل م ه" instead of "کلمه") **and in
/// reading order left-to-right**, which for Persian is backwards. This lays the
/// buffer's text out through `format_persian_display`, while the editable buffer
/// itself keeps the original keystrokes.
///
/// **Wrapping happens on the raw text, one finished line at a time.**
/// `format_persian_display` returns *visual* order, so letting egui wrap that
/// string splits it at word boundaries of a reversed sentence and the words land
/// on the wrong lines — a long dictation read as nonsense even though every
/// glyph was correct. Each line is therefore wrapped while it is still in
/// reading order and shaped only once it is final; egui's own wrapping stays in
/// place as a backstop for the case where the measurement below is a hair short.
pub fn persian_text_edit_layouter(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let font_id = egui::FontSelection::Default.resolve(ui.style());
    let color = ui.visuals().text_color();
    let valign = ui.layout().vertical_align();
    let format = egui::text::TextFormat {
        font_id,
        color,
        valign,
        ..Default::default()
    };

    let mut layout_job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping::wrap_at_width(wrap_width),
        ..Default::default()
    };
    for (index, line) in wrap_lines(ui, text, wrap_width).iter().enumerate() {
        if index > 0 {
            layout_job.append("\n", 0.0, format.clone());
        }
        layout_job.append(&format_persian_display(line), 0.0, format.clone());
    }
    ui.fonts(|f| f.layout_job(layout_job))
}

/// The lines egui will draw, in **reading order**, before any shaping.
///
/// A greedy word wrap over the raw text, measuring each word as it will be
/// drawn (shaped, so connected letters and their ligatures are counted). One
/// measurement per word rather than one per candidate line: a line's width is
/// the sum of its words plus the spaces between them, and this runs once per
/// frame for the box the user is editing.
fn wrap_lines(ui: &egui::Ui, text: &str, wrap_width: f32) -> Vec<String> {
    let font_id = egui::FontSelection::Default.resolve(ui.style());
    let color = ui.visuals().text_color();
    let width_of = |s: &str| -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        ui.fonts(|f| {
            f.layout_no_wrap(format_persian_display(s), font_id.clone(), color)
                .size()
                .x
        })
    };

    let mut lines: Vec<String> = Vec::new();
    for logical in text.split('\n') {
        if !wrap_width.is_finite() || wrap_width <= 0.0 {
            lines.push(logical.to_string());
            continue;
        }
        let space = width_of(" ");
        let mut current = String::new();
        let mut current_width = 0.0_f32;
        for word in logical.split(' ').filter(|w| !w.is_empty()) {
            let word_width = width_of(word);
            let candidate = if current.is_empty() {
                word_width
            } else {
                current_width + space + word_width
            };
            if !current.is_empty() && candidate > wrap_width {
                lines.push(std::mem::take(&mut current));
                current_width = word_width;
            } else {
                current_width = candidate;
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        // An empty logical line is a line, not nothing: dropping it here would
        // collapse the blank lines a user typed on purpose.
        lines.push(current);
    }
    lines
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

    // ── the shaping boundary ────────────────────────────────────────────

    /// Formatting an already-formatted string must change nothing.
    ///
    /// The defects this pins were both live in the shipped build: the dictionary
    /// tab's quick-fix warnings and the profiles tab's binding warnings called
    /// `format_persian_display` and handed the result to `callout`, which formats
    /// too. The second pass reshapes nothing (the letters are already in
    /// presentation forms) and reorders the *visual* line back into reading
    /// order — so the sentence rendered backwards, with the Latin token
    /// (`code.exe`) at the wrong end.
    #[test]
    fn formatting_twice_is_the_same_as_formatting_once() {
        let samples = [
            "نام برنامه خالی است؛ مثل code.exe یا مسیر کامل آن را بنویسید",
            "قوانین دیکشنری قبل از تایپ نهایی روی متن خروجی اعمال می‌شوند.",
            "مقصد: Untitled* - Typora",
            "حساسیت VAD باید بین ۰ و ۱ باشد",
            "سلام",
            "code.exe",
            "",
        ];
        for sample in samples {
            let once = format_persian_display(sample);
            assert_eq!(
                format_persian_display(&once),
                once,
                "a second shaping pass changed {sample:?}"
            );
        }
    }

    /// What the *unguarded* second pass did, which is what the user saw.
    ///
    /// This is the defect itself, not a description of it. Measured on the
    /// three-word sample below, the unguarded second pass produces
    /// `ﺍﻭﻝ ﻭﺳﻂ ﺁﺧﺮ`: the sentence is back in **reading order** — so an LTR
    /// renderer draws its first letter at the visual left, which for Persian is
    /// backwards — and the letters are **isolated** again, because they were
    /// handed to the reshaper as presentation forms. Reversed and unreadable is
    /// what the user called it; that is what this pins.
    #[test]
    fn the_unguarded_second_pass_reads_backwards() {
        let raw = "اول وسط آخر";
        let once = shape_once(raw);
        let twice = shape_once(&once);
        assert_ne!(twice, once, "a second pass is not a no-op");
        assert!(
            twice.starts_with('\u{FE8D}'),
            "the sentence's first letter (isolated alef) is drawn at the visual \
             left, i.e. the line reads backwards: {twice}"
        );
    }

    /// The reordering is what makes Persian readable on an LTR renderer, so a
    /// pass-through must not be mistaken for "shaping does nothing": one pass
    /// really does move the first word to the visual right end.
    #[test]
    fn one_pass_still_reorders_the_line() {
        let raw = "اول وسط آخر";
        let shaped = format_persian_display(raw);
        assert_ne!(shaped, raw, "Persian must be reordered for LTR drawing");
        assert_eq!(
            shaped.chars().count(),
            raw.chars().count(),
            "shaping is one character in, one character out"
        );
        assert!(
            shaped.ends_with('\u{FE8D}'),
            "the first letter of the sentence (an isolated alef, U+FE8D) must \
             end up at the visual right: {shaped}"
        );
    }

    /// The box the user edits must be laid out through the shaper, and as one
    /// finished line at a time: a raw `TextEdit` shows Persian letters
    /// disconnected and in reading order left-to-right, which is backwards.
    #[test]
    fn the_layouter_shapes_a_multiline_buffer_line_by_line() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let raw = "خط اول سلام\nخط دوم دنیا";
                let galley = persian_text_edit_layouter(ui, raw, 400.0);
                assert_eq!(
                    galley.text(),
                    &format!(
                        "{}\n{}",
                        format_persian_display("خط اول سلام"),
                        format_persian_display("خط دوم دنیا")
                    ),
                    "each line must be shaped on its own"
                );
            });
        });
    }

    /// Wrapping a **shaped** line would cut a reversed sentence at word
    /// boundaries, putting the last words of the source on the first line. The
    /// layouter wraps the raw text instead, so the reading order survives the
    /// line breaks.
    #[test]
    fn wrapping_keeps_the_reading_order_of_the_lines() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // The width of exactly two words decides where the break falls,
                // measured through the same layouter the widget uses.
                let two = persian_text_edit_layouter(ui, "کلمه کلمه", f32::INFINITY)
                    .size()
                    .x;
                let galley = persian_text_edit_layouter(ui, "کلمه کلمه کلمه کلمه", two + 0.5);
                let lines: Vec<&str> = galley.text().split('\n').collect();
                assert_eq!(lines.len(), 2, "two words per line, at most: {lines:?}");
                assert_eq!(lines[0], format_persian_display("کلمه کلمه"));
                assert_eq!(lines[1], format_persian_display("کلمه کلمه"));
            });
        });
    }

    /// A word longer than the box is left whole rather than cut: a URL or a
    /// long identifier has no word boundary to break at, and breaking it would
    /// hide characters the user is trying to read.
    #[test]
    fn a_word_longer_than_the_box_is_not_cut() {
        let word = "کلمهٔبسیاربسیارطولانی";
        let lines = {
            let ctx = egui::Context::default();
            let mut out = Vec::new();
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    out = wrap_lines(ui, word, 4.0);
                });
            });
            out
        };
        assert_eq!(lines, vec![word.to_string()]);
    }
}
