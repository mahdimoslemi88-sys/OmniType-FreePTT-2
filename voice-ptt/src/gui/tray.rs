//! System tray icon + menu via `tray-icon`.
//!
//! The `TrayIcon` is intentionally leaked for the process lifetime: dropping
//! it removes the icon, and our only exit paths are "user clicked Quit" or
//! "window closed", both of which end the process anyway.

use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::gui::flags::{DashboardFlags, Toggle};
use crate::gui::tray_warning::{OpenTarget, TrayWarning, BASE_TOOLTIP};
use crate::hotkey::HotkeyEvent;

/// Builds the tray and spawns the event-polling thread.
///
/// * `hotkey_tx` — forwarded `Quit` events reach the state machine.
/// * `flags` — which panel the user asked for; the GUI takes each request and
///   clears it. One struct rather than six parameters, because six of the same
///   type in a row can be swapped without the compiler noticing.
/// * `update_state` — shared update state for checking/downloading releases.
/// * `warning` — `None` when the startup diagnosis was clean, in which case the
///   tray looks exactly as it always did. `Some` adds a badge to the icon and
///   one menu item that opens the report; the decisions about *what* they say
///   are [`TrayWarning`]'s, not this function's.
pub fn spawn(
    hotkey_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
    flags: DashboardFlags,
    update_state: crate::updates::SharedUpdateState,
    warning: Option<TrayWarning>,
) -> Result<()> {
    // Tray labels follow the skill's section-8 contract. Routine config and
    // dictionary editing happens in the unified dashboard — no external editors.
    let show_hide = MenuItem::new("Open OmniType (باز کردن پنجره)", true, None);
    let history_gui = MenuItem::new("Transcription History (تاریخچه گفتار)", true, None);
    let engine_gui = MenuItem::new("Active Engine — Models (مدیریت مدل‌ها)", true, None);
    let dict_gui = MenuItem::new("Dictionary (واژگان)", true, None);
    let settings_gui = MenuItem::new("Settings (تنظیمات)", true, None);
    let update_gui = MenuItem::new("Check for Updates (بررسی و دانلود به‌روزرسانی)", true, None);
    let quit = MenuItem::new("Exit completely (خروج کامل)", true, None);

    let show_id = show_hide.id().clone();
    let history_gui_id = history_gui.id().clone();
    let engine_gui_id = engine_gui.id().clone();
    let dict_gui_id = dict_gui.id().clone();
    let settings_gui_id = settings_gui.id().clone();
    let update_gui_id = update_gui.id().clone();
    let quit_id = quit.id().clone();

    let menu = Menu::new();
    // The warning is the first thing offered and the only thing that changes
    // when the diagnosis is not clean: a badge on the icon, a tooltip that says
    // so, and a top-of-menu item that opens the report. Kept out of the `else`
    // chain below because it is a different shape (optional item, plus a file
    // to open rather than a flag to raise).
    let warning_item = match &warning {
        Some(w) => {
            let item = MenuItem::new(w.menu_label(), true, None);
            menu.append(&item)?;
            menu.append(&PredefinedMenuItem::separator())?;
            Some(item)
        }
        None => None,
    };
    let warning_id = warning_item.as_ref().map(|i| i.id().clone());

    menu.append(&show_hide)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&engine_gui)?;
    menu.append(&dict_gui)?;
    menu.append(&settings_gui)?;
    menu.append(&history_gui)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&update_gui)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;

    let (icon_rgba, icon_w, icon_h) = app_icon_rgba();
    let (icon_rgba, tooltip) = match &warning {
        Some(w) => (w.icon(icon_rgba, icon_w, icon_h), w.tooltip()),
        None => (icon_rgba, BASE_TOOLTIP.to_string()),
    };
    let icon = tray_icon::Icon::from_rgba(icon_rgba, icon_w, icon_h)?;

    let _tray: Box<TrayIcon> = Box::new(
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .build()?,
    );
    // Leak so the tray survives for the whole process (see module docs).
    Box::leak(_tray);

    // Event polling thread (MenuEvent receiver is global).
    let update_state_tray = update_state.clone();
    std::thread::Builder::new()
        .name("tray-events".into())
        .spawn(move || {
            let receiver = MenuEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if warning_id.as_ref() == Some(&event.id) {
                    // One warning, one destination — chosen by the fact that
                    // is wrong, not by how loud the badge is. Raising a flag
                    // shows the window and switches the tab (see
                    // `OverlayApp::open_dashboard`); the other arm opens a
                    // file, because a report is not a panel of ours.
                    if let Some(w) = &warning {
                        match w.target() {
                            OpenTarget::EnginePanel => flags.raise(Toggle::Engine),
                            OpenTarget::Report => {
                                crate::updates::open_path_in_default_app(w.report())
                            }
                        }
                    }
                } else if event.id == show_id {
                    flags.raise(Toggle::Overlay);
                } else if event.id == history_gui_id {
                    flags.raise(Toggle::History);
                } else if event.id == engine_gui_id {
                    flags.raise(Toggle::Engine);
                } else if event.id == dict_gui_id {
                    flags.raise(Toggle::Dictionary);
                } else if event.id == settings_gui_id {
                    flags.raise(Toggle::Settings);
                } else if event.id == update_gui_id {
                    // Open settings where the update card is visible
                    flags.raise(Toggle::Settings);
                    // Check if an update URL is ready to open immediately
                    let url_to_open = {
                        if let Ok(st) = update_state_tray.read() {
                            if let crate::updates::UpdateState::Available(ref info) = *st {
                                Some(
                                    info.installer_url
                                        .clone()
                                        .unwrap_or_else(|| info.release_url.clone()),
                                )
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    if let Some(url) = url_to_open {
                        crate::updates::open_url_in_browser(&url);
                    } else {
                        let st = update_state_tray.clone();
                        tokio::spawn(async move {
                            crate::updates::perform_check(&st, env!("CARGO_PKG_VERSION")).await;
                        });
                    }
                } else if event.id == quit_id {
                    flags.raise(Toggle::Quit);
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
        assert!(
            non_transparent > 100,
            "expected non-transparent pixels, found {non_transparent}"
        );
    }
}
