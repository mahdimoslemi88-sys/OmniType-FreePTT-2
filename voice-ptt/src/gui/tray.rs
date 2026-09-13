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
/// * `quit_flag` — set when the user asks to quit (GUI watches it to close).
pub fn spawn(
    hotkey_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
    overlay_toggle: std::sync::Arc<std::sync::atomic::AtomicBool>,
    quit_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    let show_hide = MenuItem::new("Show / Hide overlay", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let show_id = show_hide.id().clone();
    let quit_id = quit.id().clone();

    let menu = Menu::new();
    menu.append(&show_hide)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;

    let icon = tray_icon::Icon::from_rgba(circle_icon_rgba(32), 32, 32)?;

    let _tray: Box<TrayIcon> = Box::new(
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Voice PTT — Push-to-Talk voice typing")
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

/// Generates a simple 32×32 RGBA icon: teal circle with a darker ring.
/// Avoids an image-parsing dependency for one static asset.
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
}
