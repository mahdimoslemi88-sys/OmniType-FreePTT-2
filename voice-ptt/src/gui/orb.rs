//! Floating assistant orb: egui::Painter rendering + Win32 placement and dragging.

use std::f32::consts::{PI, TAU};
use std::time::Duration;

use eframe::egui::{
    epaint::PathShape, Color32, CursorIcon, Id, Painter, Pos2, Rect, Sense, Shape, Stroke, Vec2,
};

pub use super::orb_animation::OrbMode;
use super::orb_animation::{ease_out_cubic, lerp_color, smoothstep, OrbAnimation, BASE_DIAMETER};
use super::orb_palette::OrbPalette;

/// Window side = orb diameter * factor + 2 * padding (generous room for glow/rings/overshoot).
const CANVAS_FACTOR: f32 = 1.90;
const CANVAS_PADDING: f32 = 24.0;
const GLOW_EXTENT: f32 = 0.55;
/// Upper bound on `shake_offset`, as a fraction of the orb radius.
const SHAKE_EXTENT: f32 = 0.22;
const GLOW_LAYERS: usize = 12;
const MIN_HIT_RADIUS: f32 = 24.0;
const COMPLETE_HOLD_SECS: f32 = 0.9;
const ERROR_SHAKE_SECS: f32 = 0.55;

/// What happened this frame; the overlay decides what to do with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrbOutput {
    /// Orb was clicked (not dragged).
    pub clicked: bool,
    /// Drag finished; new orb center in physical screen pixels. Persist this.
    pub moved_to: Option<(i32, i32)>,
}

#[derive(Clone, Copy)]
struct DragState {
    cursor_start: (i32, i32),
    center_start: Pos2,
}

pub struct Orb {
    anim: OrbAnimation,
    window: win::OrbWindow,
    /// Idle resting center, physical screen pixels.
    home: Pos2,
    canvas_scale: f32,
    drag: Option<DragState>,
    shown_mode: OrbMode,
    complete_hold: f32,
    error_shake: f32,
    last_time: Option<f64>,
    time: f32,
    audio_level: Option<f32>,
    /// Last `InnerSize` command sent to the viewport (physical points).
    /// phase 1: the same size used to be re-sent every frame, which made the
    /// wgpu surface reconfigure continuously (thousands of
    /// `wgpu_hal::vulkan ... present mode` warnings per session).
    sent_side_pt: Option<f32>,
}

impl Orb {
    /// `window_title` must match the overlay viewport title (used to find the HWND).
    /// `saved_center` = `(orb_position_x, orb_position_y)` from settings, if present.
    pub fn new(window_title: &str, saved_center: Option<(i32, i32)>) -> Self {
        let idle_half = Self::max_canvas_points() * 0.5;
        let home = match saved_center {
            Some((x, y)) => win::clamp_center(Pos2::new(x as f32, y as f32), idle_half),
            None => win::primary_screen_center(),
        };
        Self {
            anim: OrbAnimation::new(home),
            window: win::OrbWindow::new(window_title),
            home,
            canvas_scale: 1.0,
            drag: None,
            shown_mode: OrbMode::Idle,
            complete_hold: 0.0,
            error_shake: 0.0,
            last_time: None,
            time: 0.0,
            audio_level: None,
            sent_side_pt: None,
        }
    }

    /// Initial window side: the fixed canvas from [`Orb::max_canvas_points`],
    /// so the very first frame already has the final size and the window is
    /// never resized afterwards.
    pub fn initial_side_points() -> f32 {
        Self::max_canvas_points()
    }

    /// Optional: hand over an HWND you already own instead of title lookup.
    pub fn set_hwnd(&mut self, raw_hwnd: isize) {
        self.window.set_raw(raw_hwnd);
    }

    /// Optional: feed real mic RMS (0..1). `None` = tasteful simulated energy.
    pub fn set_audio_level(&mut self, level: Option<f32>) {
        self.audio_level = level;
    }

    pub fn home_position(&self) -> (i32, i32) {
        (self.home.x.round() as i32, self.home.y.round() as i32)
    }

    pub fn show(&mut self, ctx: &eframe::egui::Context, requested: OrbMode) -> OrbOutput {
        let now = ctx.input(|i| i.time);
        let dt = match self.last_time {
            Some(prev) => ((now - prev) as f32).clamp(0.0, 0.1),
            None => 0.0,
        };
        self.last_time = Some(now);
        self.time += dt;

        let mode = self.resolve_mode(requested, dt);
        if mode != self.shown_mode {
            self.on_mode_enter(mode);
            self.shown_mode = mode;
        }
        // Always keep orb anchored at user-placed home position (no jumping to screen center)
        if self.drag.is_none() {
            self.anim.set_target_position(self.home);
        }
        self.anim.set_mode(mode);
        self.anim.update(dt);
        self.error_shake = (self.error_shake - dt).max(0.0);

        let ppp = ctx.pixels_per_point();
        let mut out = OrbOutput::default();

        eframe::egui::Area::new(Id::new("omnitype_orb_floating_area"))
            .fixed_pos(Pos2::ZERO)
            .interactable(true)
            .show(ctx, |ui| {
                let screen = ui.ctx().screen_rect();
                let radius = BASE_DIAMETER * 0.5 * self.anim.current_scale;
                let center = screen.center() + self.shake_offset(radius);

                let hit_radius = (radius * 1.1).max(MIN_HIT_RADIUS);
                let hit_rect = Rect::from_center_size(center, Vec2::splat(hit_radius * 2.0));
                let response =
                    ui.interact(hit_rect, Id::new("omnitype_orb"), Sense::click_and_drag());

                self.handle_pointer(ui, &response, mode, ppp, &mut out);

                let draggable = matches!(mode, OrbMode::Idle | OrbMode::Error);
                let hovered = response.hovered() || self.drag.is_some();
                self.anim.set_hover(hovered && draggable);

                if self.drag.is_some() {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                } else if hovered {
                    ui.ctx().set_cursor_icon(if draggable {
                        CursorIcon::Grab
                    } else {
                        CursorIcon::PointingHand
                    });
                }

                self.paint(ui.painter(), center, radius, mode);
            });

        let canvas_scale = self.update_canvas_scale();
        // The OS window is created ONCE, at the largest canvas the orb can ever
        // reach, and never resized. Resizing a transparent always-on-top window
        // leaves the pixels the old rect had covered on screen: dictation grows
        // the window 203 -> 298 px and going idle shrinks it back, and every
        // cycle stranded a full-width band above the orb (measured: 298x28 px,
        // centred on the orb, in the exact rows the larger window used to own).
        // The orb keeps animating inside a fixed, fully transparent canvas; only
        // a drag moves the window.
        let side_pt = Self::max_canvas_points();
        let side_px = (side_pt * ppp).ceil() as i32;
        // The window is a square and the orb is a circle in the middle of it,
        // so every click outside the orb's painted reach is a click this window
        // takes from whatever is underneath. `painted_radius` is that reach; the
        // window region is clipped to it. See `ClickRegion`.
        let region_radius_px = (self.painted_radius_pt() * ppp).ceil() as i32;
        self.window
            .place(self.anim.current_position, side_px, region_radius_px, ppp);
        debug_assert!(side_pt >= Self::canvas_side_points(canvas_scale));
        // phase 1: only resize when the canvas actually changed.
        if self.sent_side_pt != Some(side_pt) {
            self.sent_side_pt = Some(side_pt);
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::InnerSize(eframe::egui::vec2(
                side_pt, side_pt,
            )));
        }

        ctx.request_repaint_after(self.repaint_interval(mode));
        out
    }

    // ── state handling ────────────────────────────────────────────────

    fn resolve_mode(&mut self, requested: OrbMode, dt: f32) -> OrbMode {
        match requested {
            OrbMode::Complete => {
                self.complete_hold = COMPLETE_HOLD_SECS;
                OrbMode::Complete
            }
            // Typing finished: let the success pulse finish before shrinking.
            OrbMode::Idle if self.complete_hold > 0.0 => {
                self.complete_hold -= dt;
                OrbMode::Complete
            }
            other => {
                self.complete_hold = 0.0;
                other
            }
        }
    }

    fn on_mode_enter(&mut self, mode: OrbMode) {
        // Desktop companion: stays in place at user-dragged home location across all modes
        match mode {
            OrbMode::Recording | OrbMode::Processing | OrbMode::Complete | OrbMode::Idle => {
                self.anim.set_target_position(self.home);
            }
            OrbMode::Error => {
                self.error_shake = ERROR_SHAKE_SECS;
                self.anim.set_target_position(self.home);
            }
        }
    }

    fn handle_pointer(
        &mut self,
        ui: &eframe::egui::Ui,
        response: &eframe::egui::Response,
        mode: OrbMode,
        ppp: f32,
        out: &mut OrbOutput,
    ) {
        let draggable = matches!(mode, OrbMode::Idle | OrbMode::Error);

        if draggable && self.drag.is_none() && response.drag_started() {
            if let Some(cursor) = win::cursor_position() {
                self.drag = Some(DragState {
                    cursor_start: cursor,
                    center_start: self.anim.current_position,
                });
            }
        }

        if let Some(drag) = self.drag {
            let still_down = ui.input(|i| i.pointer.primary_down());
            if draggable && still_down && !response.drag_stopped() {
                if let Some((cx, cy)) = win::cursor_position() {
                    let delta = Vec2::new(
                        (cx - drag.cursor_start.0) as f32,
                        (cy - drag.cursor_start.1) as f32,
                    );
                    let half = Self::canvas_side_points(self.canvas_scale) * ppp * 0.5;
                    let c = win::clamp_center(drag.center_start + delta, half);
                    self.home = c;
                    self.anim.snap_position(c);
                }
            } else {
                self.drag = None;
                out.moved_to = Some(self.home_position());
            }
        }

        if response.clicked() {
            out.clicked = true;
        }
    }

    fn update_canvas_scale(&mut self) -> f32 {
        let target = self.anim.target_scale;
        let current = self.anim.current_scale;
        // Grow the window immediately, shrink it only once the orb has settled.
        self.canvas_scale = if self.anim.scale_settled() {
            target
        } else {
            target.max(current).max(self.canvas_scale)
        };
        self.canvas_scale
    }

    fn canvas_side_points(scale: f32) -> f32 {
        BASE_DIAMETER * scale * CANVAS_FACTOR + CANVAS_PADDING * 2.0
    }

    /// How far out from its centre the orb can ever paint, in points.
    ///
    /// This is the radius the window's click region may be clipped to without
    /// ever clipping the orb itself, so it has to be the **worst** case, not the
    /// typical one. Each term is the ceiling of one thing [`Self::paint`] draws
    ///:
    ///
    /// * `radius * 1.10` — `paint` multiplies by `1 + breath * breath_amp`,
    ///   and `breath_amp` peaks at `0.04 + 0.06 * level` when Recording;
    /// * `* (1 + GLOW_EXTENT)` — `paint_glow` layers reach
    ///   `r * (1 + GLOW_EXTENT * t)` with `t` up to 1;
    /// * `radius * 1.18` — the rings sit at `r * 1.18`, which is inside the
    ///   glow term but is kept as an explicit floor so changing `GLOW_EXTENT`
    ///   cannot silently shrink the clickable area;
    /// * `+ radius * SHAKE_EXTENT` — `shake_offset` slides the whole orb
    ///   sideways, so the circle has to travel with it.
    ///
    /// Clamped to the canvas so the region never asks for more than the window
    /// has.
    fn painted_radius_pt(&self) -> f32 {
        region_radius_pt(self.anim.current_scale, Self::max_canvas_points() * 0.5)
    }

    /// Fixed window side in points: the canvas at the biggest scale the orb ever
    /// reaches (`Recording`). Sizing the window to this once, instead of tracking
    /// the animated scale, is what keeps the window from ever being resized.
    fn max_canvas_points() -> f32 {
        Self::canvas_side_points(OrbMode::Recording.target_scale())
    }

    fn repaint_interval(&self, mode: OrbMode) -> Duration {
        let busy = self.drag.is_some()
            || !self.anim.is_settled()
            || self.error_shake > 0.0
            || self.complete_hold > 0.0;
        let ms = if busy {
            16
        } else {
            match mode {
                OrbMode::Idle | OrbMode::Error => {
                    if self.anim.is_blinking() || self.anim.hover() > 0.0 {
                        33
                    } else {
                        80
                    }
                }
                OrbMode::Recording => 16,
                OrbMode::Processing => 33,
                OrbMode::Complete => 16,
            }
        };
        Duration::from_millis(ms)
    }

    fn shake_offset(&self, radius: f32) -> Vec2 {
        if self.error_shake <= 0.0 {
            return Vec2::ZERO;
        }
        let k = self.error_shake / ERROR_SHAKE_SECS;
        Vec2::new((self.time * 42.0).sin() * radius * 0.22 * k, 0.0)
    }

    fn voice_level(&self) -> f32 {
        if let Some(level) = self.audio_level {
            return level.clamp(0.0, 1.0);
        }
        let t = self.time;
        (0.5 + 0.28 * (t * 7.3).sin() + 0.14 * (t * 11.9 + 1.3).sin() + 0.08 * (t * 3.1 + 0.7).sin())
            .clamp(0.0, 1.0)
    }

    // ── rendering ─────────────────────────────────────────────────────

    fn paint(&self, painter: &Painter, center: Pos2, radius: f32, mode: OrbMode) {
        let pal = self.anim.palette();
        let breath = self.anim.breath();
        let level = self.voice_level();

        let breath_amp = match mode {
            OrbMode::Idle | OrbMode::Error => 0.035,
            OrbMode::Recording => 0.04 + 0.06 * level,
            OrbMode::Processing => 0.025,
            OrbMode::Complete => 0.03,
        };
        let r = radius * (1.0 + breath * breath_amp);

        let glow_boost = 1.0
            + 0.35 * self.anim.hover()
            + match mode {
                OrbMode::Recording => 0.25 * level,
                OrbMode::Complete => 0.4 * (1.0 - (self.anim.mode_time() / 0.8).min(1.0)),
                _ => 0.0,
            };
        paint_glow(
            painter,
            center,
            r,
            pal.glow,
            pal.glow_strength * glow_boost * (0.85 + 0.15 * breath),
        );

        match mode {
            OrbMode::Recording => self.paint_recording(painter, center, r, &pal, level),
            OrbMode::Processing => self.paint_processing(painter, center, r, &pal),
            OrbMode::Complete => self.paint_complete(painter, center, r, &pal),
            OrbMode::Idle | OrbMode::Error => {}
        }

        paint_body(painter, center, r, &pal);
        self.paint_eyes(painter, center, r, &pal, mode);
    }

    fn paint_eyes(&self, painter: &Painter, c: Pos2, r: f32, pal: &OrbPalette, mode: OrbMode) {
        let t = self.time;
        let look = match mode {
            OrbMode::Processing => {
                let s = self.anim.spin_phase() * 0.5;
                Vec2::new(s.cos() * 0.07, -0.06 + s.sin() * 0.03)
            }
            OrbMode::Recording => Vec2::new(0.0, -0.03 + 0.01 * (t * 2.0).sin()),
            OrbMode::Idle | OrbMode::Error => {
                Vec2::new((t * 0.37).sin() * 0.035, (t * 0.23).sin() * 0.02)
            }
            OrbMode::Complete => Vec2::new(0.0, -0.02),
        } * r;

        let spacing = r * 0.25;
        let w = (r * 0.13).max(1.6);
        let mut h_full = (r * 0.30).max(3.0);
        if mode == OrbMode::Error {
            h_full *= 0.6;
        }
        let base = c + Vec2::new(0.0, -0.02 * r) + look;
        let happy = mode == OrbMode::Complete && self.anim.mode_time() > 0.1;

        for side in [-1.0f32, 1.0] {
            let p = base + Vec2::new(side * spacing, 0.0);
            if happy {
                let hw = w * 1.1;
                let hh = h_full * 0.28;
                let a = p + Vec2::new(-hw, hh);
                let b = p + Vec2::new(0.0, -hh);
                let d = p + Vec2::new(hw, hh);
                let sw = w * 0.75;
                painter.add(Shape::Path(PathShape::line(
                    vec![a, b, d],
                    Stroke::new(sw, pal.eye),
                )));
                painter.circle_filled(a, sw * 0.5, pal.eye);
                painter.circle_filled(b, sw * 0.5, pal.eye);
                painter.circle_filled(d, sw * 0.5, pal.eye);
            } else {
                let h = (h_full * self.anim.eye_openness()).max(w * 0.6);
                painter.rect_filled(Rect::from_center_size(p, Vec2::new(w, h)), w * 0.5, pal.eye);
            }
        }
    }

    fn paint_recording(&self, painter: &Painter, c: Pos2, r: f32, pal: &OrbPalette, level: f32) {
        // Expanding warm waves
        let phase = self.anim.ripple_phase();
        for k in 0..3 {
            let p = (phase + k as f32 / 3.0) % 1.0;
            let rad = r * (1.03 + 0.42 * ease_out_cubic(p));
            let fade = (1.0 - p).powi(2);
            painter.circle_stroke(
                c,
                rad,
                Stroke::new(
                    (r * 0.03 * (1.0 - p)).max(0.6),
                    with_alpha(pal.accent, 0.5 * fade * (0.6 + 0.4 * level)),
                ),
            );
        }

        // Living energy ring
        let t = self.time;
        let ring_r = r * 1.10;
        let amp = r * (0.012 + 0.03 * level);
        let n = 72;
        let points: Vec<Pos2> = (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU;
                let wob = (a * 5.0 + t * 3.2).sin() * 0.6 + (a * 9.0 - t * 4.7).sin() * 0.4;
                c + Vec2::angled(a) * (ring_r + wob * amp)
            })
            .collect();
        painter.add(Shape::Path(PathShape::closed_line(
            points,
            Stroke::new((r * 0.022).max(1.0), with_alpha(pal.glow, 0.75)),
        )));

        // Travelling highlight
        paint_comet_arc(
            painter,
            c,
            ring_r,
            t * 1.8,
            1.1,
            18,
            (r * 0.035).max(1.2),
            pal.accent,
        );
    }

    fn paint_processing(&self, painter: &Painter, c: Pos2, r: f32, pal: &OrbPalette) {
        let spin = self.anim.spin_phase();
        let ring_r = r * 1.18;

        painter.circle_stroke(
            c,
            ring_r,
            Stroke::new((r * 0.03).max(1.0), with_alpha(pal.accent, 0.14)),
        );
        paint_comet_arc(
            painter,
            c,
            ring_r,
            spin,
            1.7 * PI,
            40,
            (r * 0.05).max(1.5),
            pal.rim,
        );
        paint_comet_arc(
            painter,
            c,
            r * 1.30,
            -spin * 0.6 + PI,
            0.45 * PI,
            14,
            (r * 0.025).max(1.0),
            pal.accent,
        );

        for i in 0..6 {
            let a = -spin * 0.75 + i as f32 * TAU / 6.0;
            let tw = (self.time * 3.0 + i as f32 * 1.1).sin() * 0.5 + 0.5;
            painter.circle_filled(
                c + Vec2::angled(a) * r * 1.36,
                (r * 0.03).max(1.0) * (0.7 + 0.5 * tw),
                with_alpha(pal.accent, 0.35 + 0.55 * tw),
            );
        }
    }

    fn paint_complete(&self, painter: &Painter, c: Pos2, r: f32, pal: &OrbPalette) {
        let p = (self.anim.mode_time() / 0.9).clamp(0.0, 1.0);
        if p >= 1.0 {
            return;
        }
        let e = ease_out_cubic(p);

        painter.circle_stroke(
            c,
            r * (1.02 + 0.5 * e),
            Stroke::new(
                (r * 0.06 * (1.0 - p)).max(0.5),
                with_alpha(pal.accent, 0.8 * (1.0 - p)),
            ),
        );

        for i in 0..10 {
            let a = i as f32 * TAU / 10.0 + 0.31;
            let d = r * (1.05 + 0.55 * e);
            let s = (r * 0.045).max(1.2) * (1.0 - 0.7 * p);
            let col = if i % 2 == 0 { pal.accent } else { pal.core };
            painter.circle_filled(c + Vec2::angled(a) * d, s, with_alpha(col, 1.0 - p));
        }
    }
}

// ── painter helpers ───────────────────────────────────────────────────

fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn paint_glow(painter: &Painter, c: Pos2, r: f32, color: Color32, strength: f32) {
    let strength = strength.clamp(0.0, 1.5);
    for i in (0..GLOW_LAYERS).rev() {
        let t = (i as f32 + 1.0) / GLOW_LAYERS as f32;
        let rad = r * (1.0 + GLOW_EXTENT * t);
        let a = strength * 0.10 * (1.0 - t + 0.08).powf(1.8);
        painter.circle_filled(c, rad, with_alpha(color, a));
    }
}

fn paint_body(painter: &Painter, c: Pos2, r: f32, pal: &OrbPalette) {
    // Offset radial gradient simulation: light from upper-left.
    let layers = ((r / 3.0) as usize).clamp(8, 24);
    let light = Vec2::new(-0.30, -0.36) * r;
    for i in 0..layers {
        let t = i as f32 / (layers - 1) as f32;
        let layer_r = r * (1.0 - 0.78 * t);
        let offset = light * (t * 0.85);
        painter.circle_filled(c + offset, layer_r, lerp_color(pal.rim, pal.core, smoothstep(t)));
    }

    // Glass edge
    let edge_w = (r * 0.035).max(0.8);
    painter.circle_stroke(
        c,
        r - edge_w * 0.5,
        Stroke::new(edge_w, with_alpha(Color32::WHITE, 0.35)),
    );

    // Specular highlights + bounce light
    painter.circle_filled(
        c + Vec2::new(-0.34, -0.42) * r,
        r * 0.20,
        with_alpha(pal.highlight, 0.35),
    );
    painter.circle_filled(
        c + Vec2::new(-0.38, -0.46) * r,
        r * 0.09,
        with_alpha(pal.highlight, 0.75),
    );
    painter.circle_filled(
        c + Vec2::new(0.18, 0.52) * r,
        r * 0.18,
        with_alpha(pal.highlight, 0.10),
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_comet_arc(
    painter: &Painter,
    c: Pos2,
    radius: f32,
    head: f32,
    sweep: f32,
    segments: usize,
    width: f32,
    color: Color32,
) {
    for i in 0..segments {
        let f0 = i as f32 / segments as f32;
        let f1 = (i + 1) as f32 / segments as f32;
        let a0 = head - f0 * sweep;
        let a1 = head - f1 * sweep;
        let fade = 1.0 - f0;
        painter.line_segment(
            [c + Vec2::angled(a0) * radius, c + Vec2::angled(a1) * radius],
            Stroke::new(width * (0.35 + 0.65 * fade), with_alpha(color, fade.powf(1.3))),
        );
    }
}

// ── Win32 ─────────────────────────────────────────────────────────────

mod win {
    use std::ffi::c_void;

    use eframe::egui::Pos2;
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SetWindowPos, HWND_TOPMOST, SM_CXSCREEN,
        SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
    };

    pub struct OrbWindow {
        /// phase 1: kept for the commented-out title lookup below; the handle is
        /// now resolved by the overlay from eframe's raw window handle.
        #[allow(dead_code)]
        title: Vec<u16>,
        raw: isize,
        last: Option<(i32, i32, i32, i32)>,
    }

    impl OrbWindow {
        pub fn new(title: &str) -> Self {
            Self {
                title: title.encode_utf16().chain(std::iter::once(0)).collect(),
                raw: 0,
                last: None,
            }
        }

        pub fn set_raw(&mut self, raw: isize) {
            if self.raw != raw && raw != 0 {
                #[cfg(windows)]
                crate::gui::window_shape::enable_true_transparency(raw);
            }
            self.raw = raw;
            self.last = None;
        }

        fn hwnd(&mut self) -> Option<HWND> {
            if self.raw == 0 {
                let main = crate::gui::window_shape::MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
                if main != 0 {
                    self.raw = main;
                    #[cfg(windows)]
                    crate::gui::window_shape::enable_true_transparency(self.raw);
                } else {
                    // phase 1: the title-based fallback is disabled. The main window
                    // is created with an empty title (`with_title("")`), so this
                    // lookup never found it — and `FindWindowW` matches *any*
                    // window, which is exactly how unrelated windows could get
                    // adopted. The overlay now registers the real HWND directly.
                    // (rollback: the original lookup follows, commented out.)
                    // let found = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(self.title.as_ptr())) };
                    // if let Ok(h) = found {
                    //     if !h.0.is_null() {
                    //         self.raw = h.0 as isize;
                    //         #[cfg(windows)]
                    //         crate::gui::window_shape::enable_true_transparency(self.raw);
                    //     }
                    // }
                }
            }
            (self.raw != 0).then_some(HWND(self.raw as *mut c_void))
        }

        /// Center the (square) window on `center`, physical pixels, and clip
        /// its click region to a circle of `region_radius_px`. No-op if neither
        /// changed.
        pub fn place(&mut self, center: Pos2, side_px: i32, region_radius_px: i32, ppp: f32) {
            let ppp = if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 };
            let side = side_px.max(1);
            let x = (center.x - side as f32 * 0.5).round() as i32;
            let y = (center.y - side as f32 * 0.5).round() as i32;
            let rect = (x, y, side, side);
            let moved = self.last != Some(rect);
            let Some(hwnd) = self.hwnd() else { return };
            if moved {
                let result = unsafe {
                    SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        x,
                        y,
                        side,
                        side,
                        SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                    )
                };
                if result.is_ok() {
                    self.last = Some(rect);
                    // phase 3.2: a moved transparent window keeps the pixels of
                    // its previous position unless it is erased — those
                    // leftovers are what looked like nested window frames
                    // piling up.
                    #[cfg(windows)]
                    crate::gui::window_shape::force_repaint(self.raw);
                } else {
                    self.raw = 0; // window recreated? resolve again next frame
                    self.last = None;
                    // phase 1: let the overlay re-resolve the real handle if
                    // this one is gone (the OS window can be recreated for the
                    // main viewport).
                    #[cfg(windows)]
                    crate::gui::window_shape::invalidate_main_hwnd();
                }
            }
            // Runs whether or not the window moved: the orb animates, so the
            // region grows and shrinks under a window that never does.
            //
            // `ClickRegion` is denominated in points, so the pixel radius is
            // divided back out here rather than smuggled in as a `ppp` of 1.0 —
            // that "harmless" shortcut is how a radius ends up 1.25x too small
            // on a scaled display and quietly crops the glow.
            #[cfg(windows)]
            crate::gui::window_shape::apply_click_region(
                hwnd.0 as isize,
                crate::gui::window_shape::ClickRegion::Circle {
                    radius_pt: region_radius_px as f32 / ppp,
                },
                [side as f32 / ppp, side as f32 / ppp],
                ppp,
            );
        }
    }

    pub fn cursor_position() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
    }

    pub fn primary_screen_center() -> Pos2 {
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        Pos2::new(w as f32 * 0.5, h as f32 * 0.5)
    }

    /// Keep a window of half-size `half` (physical px) inside the virtual desktop.
    pub fn clamp_center(p: Pos2, half: f32) -> Pos2 {
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        if vw <= 0 || vh <= 0 {
            return p;
        }
        let clamp = |v: f32, lo: f32, hi: f32| if lo <= hi { v.clamp(lo, hi) } else { v };
        Pos2::new(
            clamp(p.x, vx as f32 + half, (vx + vw) as f32 - half),
            clamp(p.y, vy as f32 + half, (vy + vh) as f32 - half),
        )
    }
}

/// How far out from the orb's centre the click region may be clipped, in points,
/// for an orb currently at `scale` and a canvas half-width of `canvas_half`.
///
/// Free-standing so the ceiling it encodes can be tested without building an
/// [`Orb`]: see `tests::region_never_crops_the_glow`. Every term is a ceiling of
/// something [`paint`] and [`paint_glow`] actually draw, and the sum is what the
/// region has to contain.
fn region_radius_pt(scale: f32, canvas_half: f32) -> f32 {
    let radius = BASE_DIAMETER * 0.5 * scale;
    let breathed = radius * 1.10; // `paint`: 1 + breath * breath_amp, amp <= 0.10
    let glowed = breathed * (1.0 + GLOW_EXTENT); // `paint_glow`: r * (1 + GLOW_EXTENT * t), t <= 1
    let rings = breathed * 1.18;
    let shaken = radius * SHAKE_EXTENT;
    let reach = glowed.max(rings) + shaken;
    reach.clamp(MIN_HIT_RADIUS, canvas_half)
}

#[cfg(test)]
mod tests {
    use super::super::orb_animation::SPRING_DAMPING_RATIO;
    use super::*;

    /// The largest scale the animation can actually reach.
    ///
    /// Not simply `Recording.target_scale()`: `OrbAnimation` integrates a
    /// spring, and a spring overshoots. The step response of a second-order
    /// system peaks at `exp(-pi*zeta / sqrt(1 - zeta^2))` above its target, so
    /// that is the number to sweep to — doubled, because repeated hotkey taps
    /// can stack on velocity that has not yet damped out.
    fn max_reachable_scale() -> f32 {
        let zeta = SPRING_DAMPING_RATIO;
        let overshoot = (-PI * zeta / (1.0f32 - zeta * zeta).sqrt()).exp();
        let biggest = OrbMode::Recording.target_scale();
        biggest * (1.0 + overshoot * 2.0)
    }

    /// `SetWindowRgn` clips *rendering* as well as hit-testing, so a region
    /// smaller than the painted orb would not merely shrink its click target —
    /// it would shear the glow off with a hard circular edge. This is the one
    /// failure mode of the region approach that is silent on screen, so the
    /// ceiling is pinned here instead of being re-derived by eye.
    #[test]
    fn region_never_crops_the_glow() {
        let canvas_half = Orb::max_canvas_points() * 0.5;
        let top = max_reachable_scale();
        let mut scale = 0.2;
        while scale <= top {
            let region = region_radius_pt(scale, canvas_half);

            // What `paint` + `paint_glow` + the shake can actually reach.
            let radius = BASE_DIAMETER * 0.5 * scale;
            let painted = radius * 1.10 * (1.0 + GLOW_EXTENT) + radius * SHAKE_EXTENT;

            assert!(
                region + 0.001 >= painted,
                "scale {scale}: region {region} < painted reach {painted}"
            );
            assert!(
                region <= canvas_half + 0.001,
                "scale {scale}: region {region} does not fit a canvas half of {canvas_half}"
            );
            scale += 0.01;
        }
    }

    /// `min(MAX_REACHABLE, canvas half)` is what the region actually becomes, so
    /// the window only has to be big enough for the *reachable* orb — a bigger
    /// window would just be more desktop to protect.
    #[test]
    fn the_window_is_big_enough_for_the_biggest_orb() {
        let top = max_reachable_scale();
        let canvas_half = Orb::max_canvas_points() * 0.5;
        let reach = region_radius_pt(top, canvas_half);
        assert!(
            reach <= canvas_half + 0.001,
            "the largest reachable orb ({top} scale) paints to {reach} pt but the window half is {canvas_half} pt"
        );
        // And the clamp is not what saved it: without the canvas the orb would
        // have needed more room than the window has.
        let unclamped = region_radius_pt(top, f32::INFINITY);
        assert!(
            unclamped <= canvas_half + 0.001,
            "the canvas is too small: {unclamped} pt of orb in a {canvas_half} pt window"
        );
    }
}
