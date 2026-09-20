//! System tray icon + menu via `tray-icon`.
//!
//! The `TrayIcon` is intentionally leaked for the process lifetime: dropping
//! it removes the icon, and our only exit paths are "user clicked Quit" or
//! "window closed", both of which end the process anyway.

use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::hotkey::HotkeyEvent;

/// Commands the tray can produce toward the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleOverlay,
    Quit,
}

/// Builds the tray and spawns the event-polling thread.
///
/// * `hotkey_tx` — forwarded `Quit` events reach the state machine.
/// * `overlay_toggle` — set when the user asks to show/hide the overlay.
/// * `dict_toggle` — set when the user asks to open the dictionary manager GUI.
/// * `engine_toggle` — set when the user asks to open the AI model/engine manager GUI.
/// * `history_toggle` — set when the user asks to open the transcript history GUI.
/// * `quit_flag` — set when the user asks to quit (GUI watches it to close).
/// * `dict_path` — path to dictionary.toml for opening in default editor.
/// * `config_path` — path to config.toml for opening in default editor.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    hotkey_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
    overlay_toggle: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dict_toggle: std::sync::Arc<std::sync::atomic::AtomicBool>,
    engine_toggle: std::sync::Arc<std::sync::atomic::AtomicBool>,
    history_toggle: std::sync::Arc<std::sync::atomic::AtomicBool>,
    quit_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dict_path: std::path::PathBuf,
    config_path: std::path::PathBuf,
) -> Result<()> {
    let show_hide = MenuItem::new("Show / Hide overlay", true, None);
    let history_gui = MenuItem::new("Transcription History (تاریخچه گفتار و کپی)", true, None);
    let engine_gui = MenuItem::new("AI Models & Engines (مدیریت مدل‌ها)", true, None);
    let dict_gui = MenuItem::new("Dictionary Manager (واژگان)", true, None);
    let config_file = MenuItem::new("Open config.toml (تنظیمات)", true, None);
    let dict_file = MenuItem::new("Open dictionary.toml (فایل دیکشنری)", true, None);
    let quit = MenuItem::new("Quit", true, None);

    let show_id = show_hide.id().clone();
    let history_gui_id = history_gui.id().clone();
    let engine_gui_id = engine_gui.id().clone();
    let dict_gui_id = dict_gui.id().clone();
    let config_file_id = config_file.id().clone();
    let dict_file_id = dict_file.id().clone();
    let quit_id = quit.id().clone();

    let menu = Menu::new();
    menu.append(&show_hide)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&history_gui)?;
    menu.append(&engine_gui)?;
    menu.append(&dict_gui)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&config_file)?;
    menu.append(&dict_file)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;

    let (icon_rgba, icon_w, icon_h) = app_icon_rgba();
    let icon = tray_icon::Icon::from_rgba(icon_rgba, icon_w, icon_h)?;

    let _tray: Box<TrayIcon> = Box::new(
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("OmniType — AI Voice Typing & Industrial Speech Routing")
            .with_icon(icon)
            .build()?,
    );
    // Leak so the tray survives for the whole process (see module docs).
    Box::leak(_tray);

    // Event polling thread (MenuEvent receiver is global).
    std::thread::Builder::new()
        .name("tray-events".into())
        .spawn(move || {
            let receiver = MenuEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if event.id == show_id {
                    overlay_toggle.store(true, std::sync::atomic::Ordering::Relaxed);
                } else if event.id == history_gui_id {
                    history_toggle.store(true, std::sync::atomic::Ordering::Relaxed);
                } else if event.id == engine_gui_id {
                    engine_toggle.store(true, std::sync::atomic::Ordering::Relaxed);
                } else if event.id == dict_gui_id {
                    dict_toggle.store(true, std::sync::atomic::Ordering::Relaxed);
                } else if event.id == config_file_id {
                    #[cfg(windows)]
                    {
                        let target = config_path.to_str().unwrap_or("config.toml");
                        let _ = std::process::Command::new("cmd")
                            .args(["/C", "start", "", target])
                            .spawn();
                    }
                } else if event.id == dict_file_id {
                    #[cfg(windows)]
                    {
                        let target = dict_path.to_str().unwrap_or("dictionary.toml");
                        let _ = std::process::Command::new("cmd")
                            .args(["/C", "start", "", target])
                            .spawn();
                    }
                } else if event.id == quit_id {
                    quit_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = hotkey_tx.send(HotkeyEvent::Quit);
                    break;
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to spawn tray thread: {e}"))?;

    Ok(())
}

const ICON_BYTES: &[u8] = include_bytes!("../../assets/icon.ico");

/// Parses a 32-bpp uncompressed Windows ICO file into top-to-bottom RGBA bytes.
pub fn parse_ico_32bpp(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if data.len() < 6 {
        return None;
    }
    let res = u16::from_le_bytes([data[0], data[1]]);
    let typ = u16::from_le_bytes([data[2], data[3]]);
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    if res != 0 || typ != 1 || count == 0 {
        return None;
    }

    for i in 0..count {
        let entry_start = 6 + i * 16;
        if entry_start + 16 > data.len() {
            break;
        }
        let w_byte = data[entry_start];
        let h_byte = data[entry_start + 1];
        let bpp = u16::from_le_bytes([data[entry_start + 6], data[entry_start + 7]]);
        let offset = u32::from_le_bytes([
            data[entry_start + 12],
            data[entry_start + 13],
            data[entry_start + 14],
            data[entry_start + 15],
        ]) as usize;

        let w = if w_byte == 0 { 256 } else { w_byte as u32 };
        let h = if h_byte == 0 { 256 } else { h_byte as u32 };

        if bpp == 32 && offset + 40 <= data.len() {
            let hdr_size = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            let pix_offset = offset + hdr_size;
            let needed = (w * h * 4) as usize;
            if pix_offset + needed <= data.len() {
                let mut rgba = vec![0u8; needed];
                for y in 0..h {
                    let src_y = h - 1 - y;
                    for x in 0..w {
                        let src_idx = pix_offset + ((src_y * w + x) * 4) as usize;
                        let dst_idx = ((y * w + x) * 4) as usize;
                        let b = data[src_idx];
                        let g = data[src_idx + 1];
                        let r = data[src_idx + 2];
                        let a = data[src_idx + 3];
                        rgba[dst_idx] = r;
                        rgba[dst_idx + 1] = g;
                        rgba[dst_idx + 2] = b;
                        rgba[dst_idx + 3] = a;
                    }
                }
                return Some((rgba, w, h));
            }
        }
    }
    None
}

/// Returns the application icon RGBA buffer and dimensions.
/// Uses the embedded `assets/icon.ico` if available, otherwise falls back to a generated teal circle.
pub fn app_icon_rgba() -> (Vec<u8>, u32, u32) {
    if let Some((rgba, w, h)) = parse_ico_32bpp(ICON_BYTES) {
        (rgba, w, h)
    } else {
        (circle_icon_rgba(32), 32, 32)
    }
}

/// Generates a simple 32×32 RGBA icon: teal circle with a darker ring.
/// Fallback when icon parsing is not available.
fn circle_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 / 2.0 - 1.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= radius {
                let inner = radius * 0.72;
                if dist <= inner {
                    rgba.extend_from_slice(&[38, 198, 178, 255]); // teal fill
                } else {
                    rgba.extend_from_slice(&[20, 120, 110, 255]); // ring
                }
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]); // transparent
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_rgba_has_right_size_and_some_opaque_pixels() {
        let rgba = circle_icon_rgba(32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let opaque = rgba.chunks(4).filter(|p| p[3] == 255).count();
        assert!(opaque > 200, "expected a visible circle, got {opaque} px");
    }

    #[test]
    fn test_embedded_icon_parses_properly() {
        let (rgba, w, h) = app_icon_rgba();
        assert_eq!(w, 32);
        assert_eq!(h, 32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let non_transparent = rgba.chunks(4).filter(|p| p[3] > 0).count();
        assert!(non_transparent > 100, "expected non-transparent pixels, found {non_transparent}");
    }
}

