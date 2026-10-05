//! Floating assistant orb: egui::Painter rendering + Win32 placement and dragging.

use std::f32::consts::{PI, TAU};
use std::time::Duration;

use eframe::egui::{
    epaint::PathShape, Color32, CursorIcon, Id, Painter, Pos2, Rect, Sense, Shape, Stroke, Vec2,
};

pub use super::orb_animation::OrbMode;
use super::orb_animation::{
    ease_out_cubic, lerp_color, max_reachable_scale, smoothstep, OrbAnimation, BASE_DIAMETER,
};
use super::orb_palette::OrbPalette;

const GLOW_EXTENT: f32 = 0.55;
/// Upper bound on `shake_offset`, as a fraction of the orb radius.
/// `shake_offset` multiplies by this constant; it used to repeat the number
/// `0.22` inline, so editing one of the two would have silently desynced the
/// window from the drawing.
const SHAKE_EXTENT: f32 = 0.22;
const GLOW_LAYERS: usize = 12;
/// Smallest radius the orb will answer a click on, in points — a floor for
/// when the scale spring has the orb nearly collapsed.
///
/// It is *one* floor for the pointer test **and** for the Win32 window region,
/// because there is only one target: the number the window claims from the
/// desktop and the number egui listens on have to be the same circle, or the
/// difference between them is a ring of pixels the window swallows and nothing
/// answers (see [`interaction_radius_pt`]).
const MIN_INTERACTION_RADIUS: f32 = 24.0;
const COMPLETE_HOLD_SECS: f32 = 0.9;
const ERROR_SHAKE_SECS: f32 = 0.55;

/// How far the orb's painted edge stays from the edge of the monitor's work
/// area while it is being dragged, in points.
///
/// The window is a fixed transparent square around the orb, so the only thing
/// that decides how close the orb *looks* to the screen edge is where its
/// centre is allowed to go. Holding the centre a whole canvas-half away (which
/// is what this used to do) is what made the orb look marooned in the middle
/// of the desktop; a small visible margin is what a user expects from a
/// desktop companion.
const EDGE_MARGIN_PT: f32 = 6.0;

/// How far out the orb can paint, as a multiple of `r` — the radius *after*
/// breathing. One constant per thing the painter actually draws, each read
/// straight off the code that draws it, so the host window is sized from the
/// drawing rather than from a factor chosen to look about right.
mod reach {
    /// `paint`: `r = radius * (1 + breath * breath_amp)`. `breath()` is
    /// `sin()`, so it is in `[-1, 1]`, and `breath_amp` peaks at
    /// `0.04 + 0.06 * level` while Recording with `level == 1`.
    pub(super) const BREATH: f32 = 1.10;

    /// `paint_glow`: layers at `r * (1 + GLOW_EXTENT * t)`, `t` up to 1.
    /// Filled circles, so there is no stroke to add.
    pub(super) const GLOW: f32 = 1.0 + super::GLOW_EXTENT;

    /// `paint_recording`: the expanding wave tops out at
    /// `r * (1.03 + 0.42)`, stroked with `r * 0.03`, and `paint` is called with
    /// that mode's own `breath_amp`, so `r` already carries the breathing.
    pub(super) const RECORDING_WAVE: f32 = 1.03 + 0.42 + 0.03 / 2.0;

    /// `paint_processing`: the orbit particles sit at `r * 1.36` and are
    /// `r * 0.03 * (0.7 + 0.5 * t)` across, so their outer edge is that plus
    /// their own radius.
    pub(super) const PROCESSING_PARTICLES: f32 = 1.36 + 0.03 * 1.2;

    /// `paint_complete`: the success burst is the furthest anything ever gets —
    /// ten particles at `r * (1.05 + 0.55)`, each `r * 0.045` across. The
    /// expanding ring behind them only reaches `r * (1.02 + 0.5) + r * 0.06/2`.
    pub(super) const COMPLETE_PARTICLES: f32 = 1.05 + 0.55 + 0.045;

    const fn max2(a: f32, b: f32) -> f32 {
        if a > b {
            a
        } else {
            b
        }
    }

    /// The furthest any single term reaches: the radius the window has to hold.
    pub(super) const ART: f32 = max2(
        max2(GLOW, RECORDING_WAVE),
        max2(PROCESSING_PARTICLES, COMPLETE_PARTICLES),
    );

    /// `shake_offset` slides the whole orb sideways by up to this × radius, so
    /// the painted circle travels as well.
    pub(super) const SHAKE: f32 = super::SHAKE_EXTENT;

    /// The two facts about the terms above that used to be tests, checked at
    /// compile time instead: the success burst really is the furthest thing the
    /// painter draws, and `ART` really is the maximum of the four terms.
    ///
    /// A test would only notice these on the next run; a build failure notices
    /// them while the drawing is still being edited, which is the moment the
    /// numbers are actually in question.
    const _: () = {
        assert!(
            ART == COMPLETE_PARTICLES,
            "the success burst is no longer the furthest drawn term: the window has to be re-derived"
        );
        assert!(COMPLETE_PARTICLES > GLOW);
        assert!(COMPLETE_PARTICLES > PROCESSING_PARTICLES);
        assert!(COMPLETE_PARTICLES > RECORDING_WAVE);
        // How much room each of the others has left, so the margins are visible
        // rather than implied.
        assert!(ART - GLOW > 0.09);
        assert!(ART - PROCESSING_PARTICLES > 0.24);
        assert!(ART - RECORDING_WAVE > 0.17);
    };
}

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
    /// True until `home` has been brought inside the work area with a real
    /// `pixels_per_point` (see [`Orb::new`] and [`Orb::clamp_home`]).
    home_needs_clamp: bool,
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
    /// Whether the primary button was down last frame, so a **press** can be
    /// told from a hold. A press that never becomes a click is exactly what a
    /// dead ring looks like from the outside, and `clicked` alone never sees
    /// it, so the edge is detected here to be able to write it down.
    press_down: bool,
    /// Pixels-per-point and the pointer position, captured each frame for
    /// [`Self::log_pointer`].
    ///
    /// Held as fields rather than passed in because the diagnostic is called
    /// from inside the `Area` closure, where both values already exist but the
    /// closure cannot borrow them out of `self` alongside `ui`. The position is
    /// in **egui points**, the same space as the orb's `center` — see the note
    /// on `log_pointer` for why the Win32 screen cursor cannot be used there.
    last_ppp: f32,
    last_pointer_pos: Option<Pos2>,
    /// The user's chosen size, as a multiplier on the built-in one.
    ///
    /// A **multiplier on the whole orb**, including its click target and the
    /// canvas it is painted into — not a separate size for the drawing. Those
    /// have to move together, because the click region is derived from the
    /// painted circle: a larger drawing with the old region would be an orb you
    /// cannot reliably click, and a smaller region than the drawing would eat
    /// clicks belonging to the desktop behind it.
    user_scale: f32,
}

impl Orb {
    /// `window_title` must match the overlay viewport title (used to find the HWND).
    /// `saved_center` = `(orb_position_x, orb_position_y)` from settings, in
    /// physical pixels.
    ///
    /// A restored position is **not** clamped here: the edge margin is in points
    /// and `pixels_per_point` is not known until the first frame, so clamping
    /// now compared pixels against points — 1.25x too permissive on this
    /// machine. [`Self::clamp_home`] does it as soon as `ppp` is real.
    pub fn new(window_title: &str, saved_center: Option<(i32, i32)>) -> Self {
        let home = match saved_center {
            Some((x, y)) => Pos2::new(x as f32, y as f32),
            None => win::primary_screen_center(),
        };
        Self {
            anim: OrbAnimation::new(home),
            window: win::OrbWindow::new(window_title),
            home,
            // Reconciled with the work area on the first frame, once ppp is real.
            home_needs_clamp: saved_center.is_some(),
            drag: None,
        last_ppp: 1.0,
        last_pointer_pos: None,
            user_scale: 1.0,
            shown_mode: OrbMode::Idle,
            complete_hold: 0.0,
            error_shake: 0.0,
            last_time: None,
            time: 0.0,
            audio_level: None,
            sent_side_pt: None,
            press_down: false,
        }
    }

    /// Initial window side: the fixed canvas from
    /// [`Orb::max_canvas_points_for`], so the very first frame already has the
    /// final size and the window is never resized afterwards.
    ///
    /// Sized for [`MAX_USER_SCALE`] rather than the user's current setting,
    /// because the window is created **once**: sizing it for the setting as it
    /// stands today would clip the orb the moment the user enlarged it, and
    /// growing the window later would strand the pixels the old rect covered.
    ///
    /// The window is fully transparent and its click region is shaped to the
    /// orb each frame, so the extra margin costs nothing the user can see or
    /// click through.
    pub fn initial_side_points() -> f32 {
        Self::max_canvas_points_for(MAX_USER_SCALE)
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

    /// Whether a drag is in progress. The idle policy treats this as a blocker
    /// rather than inferring it from a mode, because a drag and an `Idle` orb
    /// look identical from the state channel.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Moves the orb's resting centre to `to`, in physical screen pixels.
    ///
    /// Returns whether the orb actually got there. The orb is clamped into the
    /// work area before it moves, so a target that was computed for a monitor
    /// which has since changed ends up somewhere legal but not where it was
    /// asked to go — and the policy needs to be told that, because a "return"
    /// that silently landed elsewhere is worse than one that did not happen.
    ///
    /// `home_needs_clamp` is cleared here rather than deferred to the next
    /// frame: this is a whole position change, not the first-frame reconcile,
    /// and leaving the flag set would let `clamp_home` move the orb a second
    /// time with a possibly different `ppp`.
    pub fn move_home_to(&mut self, to: Pos2, ppp: f32) -> bool {
        let clamped = win::clamp_center(to, keep_out_px(self.draw_scale(), ppp));
        let reached = (clamped - to).length() <= 1.0;
        if !reached {
            tracing::info!(
                requested = %format_args!("({:.0}, {:.0})", to.x, to.y),
                clamped_to = %format_args!("({:.0}, {:.0})", clamped.x, clamped.y),
                "orb could not reach the requested waiting spot"
            );
        }
        self.home = clamped;
        self.anim.snap_position(clamped);
        self.home_needs_clamp = false;
        reached
    }

    /// Moves the orb to `to` as a **visible glide** rather than a jump.
    ///
    /// The idle return used to call [`Self::move_home_to`], which calls
    /// `snap_position` and puts the orb at its destination inside a single
    /// frame. From the user's side the orb was simply not there any more for a
    /// moment and then was, somewhere else — which reads as the app glitching
    /// rather than as the app tidying up.
    ///
    /// So the destination is published as the spring's *target* and the existing
    /// per-frame integration carries it there. No new easing, no timer, no
    /// second animation: the mechanism that already smooths the orb's hover and
    /// scale excursion is the one that moves it, which is also why the canvas
    /// has to stay large enough for the excursion (it already is).
    ///
    /// `reached` is reported the same way as the instant move — whether the
    /// spot was reachable — because "it is still gliding" is not a failure, and
    /// the policy's job is to know the *destination* was valid.
    ///
    /// The spring's overshoot is why the canvas carries margin: a plain ease
    /// would be simpler, but a spring that visibly overshoots into the corner
    /// and settles back is what makes the motion read as a deliberate trip
    /// rather than a glitch.
    pub fn glide_home_to(&mut self, to: Pos2, ppp: f32) -> bool {
        let clamped = win::clamp_center(to, keep_out_px(self.draw_scale(), ppp));
        let reached = (clamped - to).length() <= 1.0;
        if !reached {
            tracing::info!(
                requested = %format_args!("({:.0}, {:.0})", to.x, to.y),
                clamped_to = %format_args!("({:.0}, {:.0})", clamped.x, clamped.y),
                "orb could not reach the requested waiting spot"
            );
        }
        self.home = clamped;
        // Target only: `current_position` is left where it is and the animation
        // closes the gap over the next frames.
        self.anim.set_target_position(clamped);
        self.home_needs_clamp = false;
        reached
    }

    /// Whether a glide is still in progress, for callers that need the window to
    /// stay awake until the orb has arrived.
    pub fn is_gliding(&self) -> bool {
        (self.anim.target_position - self.anim.current_position).length() >= 0.35
    }

    /// The scale every piece of orb geometry is drawn and clicked at: the
    /// animation's own scale, times the user's size preference.
    ///
    /// **One accessor on purpose.** The radius, the click target, the window
    /// region, the edge margin and the clamp all have to agree, and each of them
    /// used to read `anim.current_scale` directly. A user-size setting threaded
    /// into seven places is seven chances to grow the drawing and forget the
    /// click region, which produces the one failure this code must never have:
    /// an orb that is visible but cannot be clicked.
    pub fn draw_scale(&self) -> f32 {
        self.anim.current_scale * self.user_scale
    }

    /// Sets the user's size preference, clamped by the caller.
    pub fn set_user_scale(&mut self, scale: f32) {
        self.user_scale = scale.clamp(0.6, 2.0);
    }

    pub fn user_scale(&self) -> f32 {
        self.user_scale
    }

    /// The canvas this orb needs, at the given user scale.
    ///
    /// A parameter rather than reading `self.user_scale`, because the window is
    /// created once at startup and must be sized for the **largest** orb the
    /// user can select — not the one currently selected. Sizing it for the
    /// current one would clip a later enlargement, and resizing a layered
    /// window strands the pixels the old rect covered.
    pub fn max_canvas_points_for(user_scale: f32) -> f32 {
        painted_reach_pt(max_reachable_scale() * user_scale, true) * 2.0
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
                let radius = BASE_DIAMETER * 0.5 * self.draw_scale();
                let center = screen.center() + self.shake_offset(radius);

                // The pointer target is the orb's own painted circle, not a
                // fraction of it: `radius * 1.1` was a fudge factor that had
                // nothing to do with what `paint` draws, so the window region
                // (drawn from the geometry) was 27.9 pt wider than this and the
                // difference was a dead ring around the orb — clicks the window
                // took from the desktop and threw away.
                let hit_radius = interaction_radius_pt(self.draw_scale(), mode.shakes());
                let hit_rect = Rect::from_center_size(center, Vec2::splat(hit_radius * 2.0));
                let response =
                    ui.interact(hit_rect, Id::new("omnitype_orb"), Sense::click_and_drag());

                // Measured here rather than in `handle_pointer` because this is
                // where the two numbers the measurement needs already exist, and
                // threading them through would make the signature lie about what
                // the function is about. The press edge (not the click) is what
                // makes a swallowed press visible at all: `clicked` alone never
                // fires for it, which is why "there is still a ring" could not be
                // confirmed or refuted from anything the program recorded.
                let pointer_down = ui.input(|i| i.pointer.primary_down());
                self.last_ppp = ppp;
                self.last_pointer_pos = ui.input(|i| i.pointer.hover_pos());
                if pointer_down && !self.press_down {
                    self.log_pointer(center, mode, hit_radius, "press");
                }
                self.press_down = pointer_down;
                if response.clicked() {
                    self.log_pointer(center, mode, hit_radius, "click");
                }

                self.handle_pointer(ui, &response, mode, ppp, &mut out);

                let draggable = mode.hoverable();
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

        if self.home_needs_clamp {
            self.home_needs_clamp = false;
            self.clamp_home(ppp);
        }
        // The OS window is created ONCE, at the largest canvas the orb can ever
        // paint into, and never resized. Resizing a transparent always-on-top
        // window leaves the pixels the old rect had covered on screen: dictation
        // grew the window 203 -> 298 px and going idle shrank it back, and every
        // cycle stranded a full-width band above the orb (measured: 298x28 px,
        // centred on the orb, in the exact rows the larger window used to own).
        // The orb animates inside a fixed, fully transparent canvas; only a drag
        // moves the window.
        let side_pt = Self::max_canvas_points();
        let side_px = (side_pt * ppp).ceil() as i32;
        // The window is a square and the orb is a circle in the middle of it,
        // so every click outside the orb's painted reach is a click this window
        // takes from whatever is underneath. `interaction_radius_pt` is that
        // reach — and the same number sizes the pointer target, so the region
        // and the hit test cannot drift apart. See `ClickRegion`.
        let region_radius_px =
            (interaction_radius_pt(self.draw_scale(), mode.shakes()) * ppp).ceil() as i32;
        self.window
            .place(self.anim.current_position, side_px, region_radius_px, ppp);
        // phase 1: only resize when the canvas actually changed.
        if self.sent_side_pt != Some(side_pt) {
            self.sent_side_pt = Some(side_pt);
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::InnerSize(
                eframe::egui::vec2(side_pt, side_pt),
            ));
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
        let draggable = mode.hoverable();

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
                    // The cursor delta is already in physical pixels, so this is
                    // where points become pixels — once, in `keep_out_px`.
                    let c = win::clamp_center(
                        drag.center_start + delta,
                        keep_out_px(self.draw_scale(), ppp),
                    );
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

    /// Writes one line describing where the pointer is relative to the orb.
    ///
    /// Everything a hand test needs in a single row: the radius in **both**
    /// units, the region it had to beat, and whether this press was inside
    /// it. A press logged with `inside = true` and no matching `click` is a
    /// received-but-unrecognised gesture; a press with `inside = false` never
    /// reached the orb at all. The two look identical on screen and only this
    /// line tells them apart.
    ///
    /// **Both operands are egui points, on purpose.** This used to subtract
    /// `GetCursorPos` — absolute *physical screen* pixels — from `center`, which
    /// is a point offset inside the window. The two are different coordinate
    /// spaces, so the difference was meaningless, and the symptom was a log
    /// full of confident nonsense: every single event read `inside = false` with
    /// a radius near 1500 pt on a 73 pt target, which looks exactly like "the
    /// click never arrived" and sent the last investigation after the wrong
    /// subsystem entirely. `i.pointer.hover_pos()` is already in the same space
    /// as `center`, so the subtraction is now a real distance.
    fn log_pointer(
        &self,
        center: Pos2,
        mode: OrbMode,
        hit_radius: f32,
        kind: &'static str,
    ) {
        let ppp = self.last_ppp;
        let Some(pos) = self.last_pointer_pos else {
            return;
        };
        let radius_pt = (pos - center).length();
        let radius_px = radius_pt * ppp;
        let region_px = hit_radius * ppp;
        tracing::info!(
            kind,
            mode = ?mode,
            x = pos.x.round() as i32,
            y = pos.y.round() as i32,
            radius_px = radius_px.round() as i32,
            radius_pt = (radius_pt * 100.0).round() / 100.0,
            region_px = region_px.round() as i32,
            inside = radius_pt <= hit_radius,
            "orb pointer",
        );
    }

    /// Brings `home` inside the work area, keeping the orb's **painted** edge
    /// `EDGE_MARGIN_PT` away from it.
    ///
    /// The orb can only be dragged in [`OrbMode::Idle`] and
    /// [`OrbMode::Error`], so the reach used here is the one it has while
    /// draggable — which is what lets it sit close to the edge instead of a
    /// whole transparent canvas away. Growing afterwards (dictation takes the
    /// orb to 1.7x) is safe precisely because the window is centred on the orb
    /// and holds the largest reach, so a bigger orb never runs out of canvas; it
    /// only comes nearer the screen edge, and springs back in when the orb
    /// shrinks.
    fn clamp_home(&mut self, ppp: f32) {
        let clamped = win::clamp_center(self.home, keep_out_px(self.draw_scale(), ppp));
        if clamped != self.home {
            tracing::info!(
                from = %format_args!("({}, {})", self.home.x, self.home.y),
                to = %format_args!("({:.0}, {:.0})", clamped.x, clamped.y),
                "orb pulled back inside the work area"
            );
            self.home = clamped;
            self.anim.snap_position(clamped);
        }
    }

    /// Fixed window side in points: twice the furthest the orb can paint, for
    /// the largest scale the spring can reach.
    ///
    /// Sizing the window to this once, instead of tracking the animated scale,
    /// is what keeps it from ever being resized. A resized transparent
    /// always-on-top window strands the pixels its old rect covered, which is
    /// the ghost-aura artifact measured in `docs/GUI-WINDOW-ARTIFACT-REPORT.md`.
    pub fn max_canvas_points() -> f32 {
        Self::max_canvas_points_for(1.0)
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
        Vec2::new((self.time * 42.0).sin() * radius * reach::SHAKE * k, 0.0)
    }

    fn voice_level(&self) -> f32 {
        if let Some(level) = self.audio_level {
            return level.clamp(0.0, 1.0);
        }
        let t = self.time;
        (0.5 + 0.28 * (t * 7.3).sin()
            + 0.14 * (t * 11.9 + 1.3).sin()
            + 0.08 * (t * 3.1 + 0.7).sin())
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
        painter.circle_filled(
            c + offset,
            layer_r,
            lerp_color(pal.rim, pal.core, smoothstep(t)),
        );
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
            Stroke::new(
                width * (0.35 + 0.65 * fade),
                with_alpha(color, fade.powf(1.3)),
            ),
        );
    }
}

// ── Win32 ─────────────────────────────────────────────────────────────

mod win {
    use std::ffi::c_void;

    use eframe::egui::Pos2;
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SetWindowPos, HWND_TOPMOST, SM_CXSCREEN,
        SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        SWP_NOACTIVATE, SWP_NOOWNERZORDER,
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

        /// Points the window at `raw`.
        ///
        /// The overlay hands the handle over on **every** frame, so this has to
        /// be a no-op when the handle has not changed. It used to clear `last`
        /// unconditionally, which threw away the cached rect ~60 times a second
        /// and made [`Self::place`] call `SetWindowPos` on every frame — the
        /// dedup it exists to provide was unreachable. A genuinely new window
        /// owns no cached rect and still gets placed once; a window that has
        /// gone away (`raw == 0`) also drops it, so the handle can be resolved
        /// again from scratch.
        pub fn set_raw(&mut self, raw: isize) {
            if self.raw == raw {
                return;
            }
            if raw != 0 {
                #[cfg(windows)]
                crate::gui::window_shape::enable_true_transparency(raw);
            }
            self.raw = raw;
            self.last = None;
        }

        /// The rect [`Self::place`] last committed, for tests.
        #[cfg(test)]
        pub fn last_rect(&self) -> Option<(i32, i32, i32, i32)> {
            self.last
        }

        /// Pretends [`Self::place`] committed `rect`, so a test can check what
        /// survives a [`Self::set_raw`] without owning a real window.
        #[cfg(test)]
        pub fn seed_last_for_test(&mut self, rect: (i32, i32, i32, i32)) {
            self.last = Some(rect);
        }

        fn hwnd(&mut self) -> Option<HWND> {
            if self.raw == 0 {
                let main =
                    crate::gui::window_shape::MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
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
            let ppp = if ppp.is_finite() && ppp > 0.0 {
                ppp
            } else {
                1.0
            };
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

    /// Keeps a window of half-size `half` (physical px) inside the **work area**
    /// of the monitor `p` is on, falling back to the virtual desktop.
    ///
    /// The work area rather than the whole monitor is what a user means by "the
    /// edge of the screen": `SM_CXVIRTUALSCREEN` includes the taskbar, so
    /// clamping to it let the orb be dragged under the taskbar and out of
    /// reach. `MonitorFromPoint` also makes the answer per-monitor, so a second
    /// display with a different resolution or a taskbar on another edge gets its
    /// own bounds.
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
        let (mut left, mut top, mut right, mut bottom) =
            (vx as f32, vy as f32, (vx + vw) as f32, (vy + vh) as f32);
        if let Some(work) = work_area_at(p) {
            (left, top, right, bottom) = work;
        }
        let clamp = |v: f32, lo: f32, hi: f32| if lo <= hi { v.clamp(lo, hi) } else { v };
        Pos2::new(
            clamp(p.x, left + half, right - half),
            clamp(p.y, top + half, bottom - half),
        )
    }

    /// Usable desktop of the monitor nearest to `p`, physical pixels.
    ///
    /// `None` when the monitor cannot be resolved, which sends the caller back
    /// to the virtual desktop rather than to an unbounded position.
    fn work_area_at(p: Pos2) -> Option<(f32, f32, f32, f32)> {
        #[cfg(windows)]
        {
            let pt = POINT {
                x: p.x.round() as i32,
                y: p.y.round() as i32,
            };
            let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
            if mon.is_invalid() {
                return None;
            }
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if unsafe { GetMonitorInfoW(mon, &mut info) }.as_bool() {
                let rc = info.rcWork;
                // A work area the monitor cannot honour (a degenerate one would
                // make the clamp an empty interval) falls back to the caller.
                if rc.right > rc.left && rc.bottom > rc.top {
                    return Some((
                        rc.left as f32,
                        rc.top as f32,
                        rc.right as f32,
                        rc.bottom as f32,
                    ));
                }
            }
        }
        None
    }
}

/// The one radius, in points, that decides how big the orb is to a click.
///
/// Both the egui pointer rectangle and the Win32 window region come from here,
/// and that is the whole point of the function existing:
///
/// * The window region is what stops the transparent square from stealing
///   clicks from the desktop behind it. It *also* clips rendering, so it cannot
///   be made smaller than the painted orb to match a smaller click target —
///   that would shear the glow and the success burst with a hard circular edge.
/// * The egui rect is what turns a click into a press/release. Anything the
///   region claims that this rect does not cover is a click the window eats and
///   drops.
///
/// They were derived separately (`radius * 1.1` here, `painted_reach_pt` there),
/// which left a dead ring 27.9 pt wide at idle and 35.5 pt while recording —
/// widest exactly when the orb is easiest to miss. Deriving both from the
/// painted geometry makes the ring zero by construction rather than by tuning.
///
/// `with_shake` is true only in [`OrbMode::Error`], which is the only mode that
/// shakes; charging the other four for a translation they cannot have would grow
/// an invisible but live ring around a still orb.
///
/// Free-standing, and free of [`Orb`], so the five modes can be checked without
/// building a window.
fn interaction_radius_pt(scale: f32, with_shake: bool) -> f32 {
    // The ceiling is the canvas, and the canvas is sized for the largest orb the
    // *user* can choose. A clamp against the un-scaled canvas would silently cap
    // every enlarged orb back to the default size's click target, so the orb
    // would grow visually while its clickable area did not.
    painted_reach_pt(scale, with_shake)
        .max(MIN_INTERACTION_RADIUS)
        .min(Orb::max_canvas_points_for(MAX_USER_SCALE) * 0.5)
}

/// The largest size multiplier a user can select.
///
/// Named rather than inlined so the ceiling the click radius is clamped against
/// and the clamp on the setting itself cannot drift apart.
pub const MAX_USER_SCALE: f32 = 2.0;

/// How far out from the orb's centre it can paint, in points, at `scale`.
///
/// `with_shake` is a separate term because the shake is a *translation* of the
/// whole orb rather than a bigger circle: it can only be active in
/// [`OrbMode::Error`], so a caller that knows the orb is not shaking leaves it
/// out instead of paying for it in every mode.
///
/// Free-standing so the ceiling it encodes can be tested without building an
/// [`Orb`]: see `tests::the_window_holds_every_pixel_every_mode_can_paint`.
/// Every term is the ceiling of something [`Orb::paint`] actually draws, and the
/// sum is what both the window and the click region have to contain.
fn painted_reach_pt(scale: f32, with_shake: bool) -> f32 {
    let radius = BASE_DIAMETER * 0.5 * scale;
    let art = radius * reach::BREATH * reach::ART;
    if with_shake {
        art + radius * reach::SHAKE
    } else {
        art
    }
}

/// How far the orb's centre has to stay from the edge of the work area, in
/// physical pixels: its painted reach at `scale` plus the visible margin.
///
/// The drag and the restored position both go through this, so they cannot
/// disagree — which they used to, because the drag used the canvas half in
/// pixels and the restore used the same number in points.
fn keep_out_px(scale: f32, ppp: f32) -> f32 {
    let ppp = if ppp.is_finite() && ppp > 0.0 {
        ppp
    } else {
        1.0
    };
    (painted_reach_pt(scale, true) + EDGE_MARGIN_PT) * ppp
}

/// The orb's reach while it is at rest, in physical pixels: painted extent plus
/// the edge margin.
///
/// Exposed so the idle policy's waiting spot is computed from *this* number
/// rather than a second guess at it. A spot derived from a smaller reach than
/// the clamp uses would be pushed away by the clamp on every single attempt,
/// which would present as a return that never works.
pub fn idle_half_reach_px(ppp: f32) -> f32 {
    keep_out_px(1.0, ppp)
}

/// The orb's click target, for `window_shape`'s click-through tests.
///
/// The region the orb asks Win32 for and the number egui hit-tests on are both
/// this, or one of the two guarantees (no cropped glow / no stolen clicks) is
/// silently lost. Exposing it here lets that be a test instead of an assumption.
#[cfg(test)]
pub(crate) fn interaction_radius_pt_for_test(scale: f32, with_shake: bool) -> f32 {
    interaction_radius_pt(scale, with_shake)
}

#[cfg(test)]
mod tests {
    /// The pointer diagnostic used to subtract the Win32 cursor — absolute
    /// physical screen pixels — from the orb's `center`, which is a point
    /// offset inside the window. Different coordinate spaces, so the "distance"
    /// was meaningless, and the log said `inside = false` with a radius of
    /// ~1500 pt against a 73 pt target on *every* event. That reads exactly like
    /// "the click never arrived", and it is what sent the last investigation
    /// looking at the window region instead of at the state machine.
    ///
    /// The property to pin is the one that was broken: a press on the orb must
    /// measure as inside its own hit radius. Both operands are points here, so
    /// the distance is real.
    #[test]
    fn a_press_on_the_orb_measures_as_inside_its_own_hit_radius() {
        let center = Pos2::new(148.0, 61.0);
        // A press at the orb's centre, and one on its painted edge, are both
        // inside; a press far away in the same canvas is not.
        for (label, offset, expect_inside) in [
            ("centre", Vec2::ZERO, true),
            ("on the painted edge", Vec2::new(70.0, 0.0), true),
            ("far outside", Vec2::new(900.0, 0.0), false),
        ] {
            let hit_radius = 73.0f32;
            let pos = center + offset;
            let radius_pt = (pos - center).length();
            assert_eq!(
                radius_pt <= hit_radius,
                expect_inside,
                "{label}: {radius_pt} pt against a {hit_radius} pt target"
            );
        }
    }

    /// The same comparison in the units the log prints, since that is what a
    /// reader is checking. At 1.25x scaling the 73 pt target is 91 physical
    /// pixels; a press at the centre must report a radius of zero, not a
    /// screen-coordinate difference in the hundreds.
    #[test]
    fn the_logged_radius_is_a_real_distance_not_a_screen_offset() {
        let ppp = 1.25f32;
        let hit_radius = 73.0f32;
        let center = Pos2::new(148.0, 61.0);
        let pos = Pos2::new(151.0, 61.0); // 3 pt from centre
        let radius_px = (pos - center).length() * ppp;
        assert!(
            (radius_px - 3.75).abs() < 0.01,
            "3 pt at 1.25x is 3.75 px, got {radius_px}"
        );
        assert!(
            radius_px <= hit_radius * ppp,
            "a press 3 pt from the centre is inside the target"
        );
    }

    use super::*;
    use crate::gui::orb_animation::{max_reachable_scale, HOVER_SCALE_BOOST};

    /// `SetWindowRgn` clips *rendering* as well as hit-testing, so a region (and
    /// a window) smaller than the painted orb would not merely shrink the click
    /// target — it would shear the glow, or the success burst, off with a hard
    /// circular edge. That failure is silent on screen, so the ceiling is pinned
    /// here instead of being re-derived by eye.
    ///
    /// The measured defect, as a test: no click the window claims can die
    /// between that window and the orb.
    ///
    /// The pointer rectangle and the Win32 region used to be derived
    /// independently — `radius * 1.1` for the pointer, `painted_reach_pt` for
    /// the region — and the difference was a ring 27.9 pt wide at idle and
    /// 35.5 pt while recording (B0 §6-1): the window took the click, egui never
    /// saw it, and neither did the app underneath. Both now come from
    /// [`interaction_radius_pt`], so the property to pin is that this number is
    /// never *inside* the painted circle.
    #[test]
    fn no_click_dies_between_the_window_region_and_the_orb() {
        let mut scale = 0.2f32;
        while scale <= max_reachable_scale() {
            for mode in OrbMode::ALL {
                let hit = interaction_radius_pt(scale, mode.shakes());
                let painted = painted_reach_pt(scale, mode.shakes());
                // The floor may push the target past the drawing — a collapsed
                // orb is still clickable. It may never pull it inside the
                // drawing, because that is the dead ring again.
                assert!(
                    hit >= painted - 0.001,
                    "{mode:?} at scale {scale}: clicks die between {painted} pt \
                     and {hit} pt"
                );
                assert!(hit <= Orb::max_canvas_points() * 0.5 + 0.001);
            }
            scale += 0.01;
        }
    }

    /// The sizes themselves, as a table: what `radius * 1.1` used to claim,
    /// against what the window now claims, for every mode and for hover.
    ///
    /// The right-hand column is spelled out rather than recomputed, so a change
    /// to the drawing is a visible diff here instead of a silently larger click
    /// target in a release nobody diffs. The left-hand column is the number B0
    /// §6-1 measured the dead ring against.
    #[test]
    fn the_click_target_grew_from_a_guess_to_the_painted_circle() {
        let hover = 1.0 + HOVER_SCALE_BOOST;
        let cases: [(&str, f32, bool); 6] = [
            ("Idle", 1.0, false),
            ("Idle + hover", hover, false),
            ("Recording", OrbMode::Recording.target_scale(), false),
            ("Processing", OrbMode::Processing.target_scale(), false),
            ("Complete", OrbMode::Complete.target_scale(), false),
            ("Error", 1.0, true),
        ];
        let rows: Vec<String> = cases
            .iter()
            .map(|(name, scale, shake)| {
                let old = (BASE_DIAMETER * 0.5 * scale * 1.1).max(MIN_INTERACTION_RADIUS);
                format!(
                    "{name}: {old:.1} pt -> {:.1} pt",
                    interaction_radius_pt(*scale, *shake)
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                "Idle: 33.0 pt -> 54.3 pt",
                "Idle + hover: 35.6 pt -> 58.6 pt",
                "Recording: 55.0 pt -> 90.5 pt",
                "Processing: 49.5 pt -> 81.4 pt",
                "Complete: 49.5 pt -> 81.4 pt",
                "Error: 33.0 pt -> 60.9 pt",
            ],
            "the click target table changed"
        );
    }

    /// The floor has to stay a floor: a scale the spring reaches mid-flight
    /// (tapping push-to-talk quickly stacks overshoot on overshoot) must not
    /// leave the orb unclickable, and it must not be larger than the window can
    /// hold.
    #[test]
    fn a_collapsed_orb_is_still_clickable_and_still_inside_the_canvas() {
        let collapsed = interaction_radius_pt(0.2, false);
        assert_eq!(collapsed, MIN_INTERACTION_RADIUS);
        assert!(collapsed > painted_reach_pt(0.2, false));
        // The ceiling is the canvas **for the largest orb the user can choose**.
        //
        // It used to be the un-scaled canvas, which silently capped every
        // enlarged orb's click target back to the default size: the orb would
        // grow on screen and its clickable area would not, which is the one
        // combination this code must never produce. The window is created once
        // at [`MAX_USER_SCALE`], so that canvas is the one that actually exists.
        assert!(collapsed <= Orb::max_canvas_points_for(MAX_USER_SCALE) * 0.5);
        // A nonsense scale cannot produce a nonsense target either.
        assert_eq!(
            interaction_radius_pt(f32::NAN, false),
            MIN_INTERACTION_RADIUS
        );
        assert_eq!(
            interaction_radius_pt(f32::INFINITY, false),
            Orb::max_canvas_points_for(MAX_USER_SCALE) * 0.5
        );
    }

    /// The property the user-size setting rests on: **a bigger orb is a bigger
    /// click target.**
    ///
    /// Not an obvious consequence, and the failure it guards is the one that
    /// makes the feature worse than useless — a large orb that swallows clicks
    /// only in its painted middle, or (the bug this replaced) a large orb whose
    /// click target stayed at the default size so its edges did nothing.
    #[test]
    fn a_larger_orb_also_answers_a_larger_click() {
        let small = interaction_radius_pt(1.0, false);
        let large = interaction_radius_pt(MAX_USER_SCALE, false);
        assert!(
            large > small,
            "the click target must follow the drawing, or the enlarged orb's edges are dead"
        );
        // And the enlarged target must still fit the window it is drawn in.
        assert!(large <= Orb::max_canvas_points_for(MAX_USER_SCALE) * 0.5);
    }

    /// The window is created once, so it has to be big enough for the largest
    /// orb the user can select — not the one selected today. Sizing it for
    /// today's would clip a later enlargement, and growing a layered window
    /// later strands the pixels its old rect covered.
    #[test]
    fn the_window_is_sized_for_the_largest_orb_not_the_current_one() {
        assert!(
            Orb::initial_side_points() >= Orb::max_canvas_points_for(MAX_USER_SCALE),
            "the canvas must hold the largest selectable orb"
        );
        assert!(
            Orb::initial_side_points() > Orb::max_canvas_points(),
            "and it must be larger than the default-size orb, or enlarging would clip"
        );
    }

    /// Only `Error` shakes, so only `Error` may be charged for the excursion.
    /// If the other four were, each would grow an invisible-but-live ring —
    /// clicks that work on a part of the screen nothing was ever drawn on.
    ///
    /// The expectations are written out per mode instead of being derived from
    /// `shakes()`, because a table computed from the same predicate it is
    /// supposed to check cannot fail: that is how the first version of this
    /// test passed with `shakes()` returning `true` for everything (canary C20).
    #[test]
    fn only_the_shaking_mode_pays_for_the_shake() {
        let hover = 1.0 + HOVER_SCALE_BOOST;
        // (`name`, `mode`, `scale`, `expected`) — the expectation is spelled out
        // per mode rather than derived from `shakes()`, because a table computed
        // from the same predicate it is meant to check cannot fail. That is how
        // the first version of this test stayed green while `shakes()` returned
        // `true` for every mode (canary C20).
        let cases: [(&str, OrbMode, f32, f32); 6] = [
            ("Idle", OrbMode::Idle, 1.0, painted_reach_pt(1.0, false)),
            (
                "Idle + hover",
                OrbMode::Idle,
                hover,
                painted_reach_pt(hover, false),
            ),
            (
                "Recording",
                OrbMode::Recording,
                OrbMode::Recording.target_scale(),
                painted_reach_pt(OrbMode::Recording.target_scale(), false),
            ),
            (
                "Processing",
                OrbMode::Processing,
                OrbMode::Processing.target_scale(),
                painted_reach_pt(OrbMode::Processing.target_scale(), false),
            ),
            (
                "Complete",
                OrbMode::Complete,
                OrbMode::Complete.target_scale(),
                painted_reach_pt(OrbMode::Complete.target_scale(), false),
            ),
            ("Error", OrbMode::Error, 1.0, painted_reach_pt(1.0, true)),
        ];
        for (name, mode, scale, want) in cases {
            let got = interaction_radius_pt(scale, mode.shakes());
            assert!(
                (got - want).abs() < 0.001,
                "{name}: {got} pt, and the expected answer for \
                 {} is {want} pt",
                if mode.shakes() {
                    "a shaking orb"
                } else {
                    "a still orb"
                },
            );
        }
        // The two answers have to actually differ, or the table above would pass
        // even if the shake term were quietly dropped everywhere.
        let excursion = painted_reach_pt(1.0, true) - painted_reach_pt(1.0, false);
        assert!(
            (excursion - BASE_DIAMETER * 0.5 * reach::SHAKE).abs() < 0.001,
            "the shake term is worth {excursion} pt, not one radius x SHAKE"
        );
    }

    /// `SetWindowRgn` clips *rendering* as well as hit-testing, so a region (and
    /// a window) smaller than the painted orb would not merely shrink the click
    /// target — it would shear the glow, or the success burst, off with a hard
    /// circular edge. That failure is silent on screen, so the ceiling is pinned
    /// here instead of being re-derived by eye.
    ///
    /// Swept across every mode and the whole range the scale spring can reach,
    /// because the mode only decides *which* term is the furthest one, and
    /// `reach::ART` has to be the maximum over all of them.
    #[test]
    fn the_window_holds_every_pixel_every_mode_can_paint() {
        let canvas_half = Orb::max_canvas_points() * 0.5;
        let top = max_reachable_scale();
        for mode in OrbMode::ALL {
            for shaking in [false, true] {
                // Only Error shakes, and a shake is a translation of the whole
                // orb: a mode that cannot shake must not be charged for it.
                let shaken = shaking && mode == OrbMode::Error;
                let mut scale = 0.2;
                while scale <= top {
                    let reach = painted_reach_pt(scale, shaken);
                    assert!(
                        reach <= canvas_half + 0.001,
                        "{mode:?} at scale {scale} paints to {reach} pt, \
                         but the window half is only {canvas_half} pt"
                    );
                    scale += 0.01;
                }
            }
        }
    }

    /// The whole point of deriving the canvas: it is *exactly* twice the worst
    /// painted reach, with no factor and no padding left over.
    ///
    /// A slack factor is how the old `CANVAS_FACTOR = 1.90` plus
    /// `CANVAS_PADDING = 24` came to be right by accident — `1.90 * diameter`
    /// alone was too small, and the 48 pt of padding hid it. A test that only
    /// checks the canvas is big enough cannot see that; this one fails if anyone
    /// re-introduces a round number.
    #[test]
    fn the_canvas_is_exactly_the_painted_reach_and_no_more() {
        let side = Orb::max_canvas_points();
        let needed = painted_reach_pt(max_reachable_scale(), true) * 2.0;
        assert!(
            (side - needed).abs() < 0.001,
            "canvas is {side} pt but the worst painted reach needs {needed} pt"
        );
        // Sanity on the size itself, so a change to the drawing that makes the
        // orb much bigger or smaller shows up as a visible diff.
        assert!(
            (side - 237.0).abs() < 8.0,
            "canvas moved to {side} pt (it was 238 pt before the derivation)"
        );
    }

    /// Which drawing term the window is actually sized for, as a number.
    ///
    /// The *fact* that it is the success burst is asserted at compile time (see
    /// the `const` block in `reach`); this is here so the arithmetic shows up in
    /// test output and a change to any term is a visible diff rather than a
    /// silent resizing of the window.
    #[test]
    fn the_window_is_sized_for_the_success_burst() {
        assert_eq!(reach::ART, reach::COMPLETE_PARTICLES);
        assert!(
            (reach::ART - 1.645).abs() < 0.001,
            "the furthest drawn term is now {}, not the 1.645 the window was derived from",
            reach::ART
        );
    }

    /// Dragging has to keep the orb's *painted* edge on screen, which is a very
    /// different number from half the transparent canvas: this is the whole
    /// difference between an orb that sits next to the edge and one that floats
    /// in the middle of the desktop.
    #[test]
    fn dragging_keeps_the_orb_near_the_edge_instead_of_marooning_it() {
        let keep_out = painted_reach_pt(1.0, true) + EDGE_MARGIN_PT;
        let old_keep_out = 238.0 * 0.5; // what the canvas half used to demand
                                        // The margin on top of the painted edge is exactly the visible margin,
                                        // and nothing else: this is the number that says the orb is no longer
                                        // being held a whole transparent canvas away from the edge.
        assert!((keep_out - painted_reach_pt(1.0, true) - EDGE_MARGIN_PT).abs() < 0.001);
        assert!(
            keep_out < old_keep_out * 0.65,
            "idle keep-out is {keep_out} pt; the old canvas half was {old_keep_out} pt"
        );
    }

    /// Growing after being parked at the edge must not crop the orb, and the
    /// reason is structural rather than another clamp: the window is centred on
    /// the orb and holds the worst reach, so the orb cannot outgrow its canvas.
    #[test]
    fn growing_after_being_parked_at_the_edge_still_fits_the_window() {
        let canvas_half = Orb::max_canvas_points() * 0.5;
        let parked_keep_out = painted_reach_pt(1.0, true) + EDGE_MARGIN_PT;
        for mode in OrbMode::ALL {
            let grown = painted_reach_pt(mode.target_scale(), mode == OrbMode::Error);
            // It still fits the window that is centred on it.
            assert!(
                grown <= canvas_half + 0.001,
                "{mode:?} paints to {grown} pt in a {canvas_half} pt window"
            );
            // So the only consequence of parking at the edge is that the orb
            // comes visually closer to it, by exactly the growth in reach.
            let encroachment = grown - parked_keep_out;
            assert!(
                encroachment < canvas_half,
                "{mode:?} would reach {encroachment} pt past the work-area edge"
            );
        }
    }

    /// The restored position and the drag have to agree, or the orb jumps the
    /// first time it is touched. Both go through the same keep-out, so this is a
    /// statement about the single number they share.
    #[test]
    fn the_restored_home_and_the_drag_use_the_same_keep_out() {
        // At 100% the keep-out is the painted reach plus the margin, in points
        // that happen to be pixels.
        let reach = painted_reach_pt(1.0, true);
        assert!((keep_out_px(1.0, 1.0) - (reach + EDGE_MARGIN_PT)).abs() < 0.001);
        // The same expression at 125%: the margin has to scale with the display
        // or the orb ends up proportionally closer to the edge than intended.
        assert!((keep_out_px(1.0, 1.25) - 1.25 * (reach + EDGE_MARGIN_PT)).abs() < 0.001);
        // A nonsensical scale factor falls back to 1.0 rather than collapsing the
        // margin to nothing (which would let the orb be dragged off-screen).
        assert_eq!(keep_out_px(1.0, 0.0), keep_out_px(1.0, 1.0));
        assert_eq!(keep_out_px(1.0, f32::NAN), keep_out_px(1.0, 1.0));
    }

    #[test]
    fn the_keep_out_scales_with_the_display() {
        // 1.25x is this machine. A 200% display has to double the margin too, or
        // the orb ends up proportionally closer to the edge than intended.
        assert!((keep_out_px(1.0, 2.0) - 2.0 * keep_out_px(1.0, 1.0)).abs() < 0.001);
    }

    mod win {
        use super::super::win::OrbWindow;

        /// The reported bug: the overlay calls `set_hwnd` on every frame, and
        /// `set_raw` used to clear the cached rect every time, so `place` issued
        /// a `SetWindowPos` on every single frame.
        ///
        /// The cache is what makes moving the orb cheap, and the alternative is
        /// not merely wasteful on this app: a moving transparent window strands
        /// the pixels of its old rect, which is the artifact measured in
        /// `docs/GUI-WINDOW-ARTIFACT-REPORT.md`.
        #[test]
        fn resending_the_same_handle_keeps_the_cached_rect() {
            let mut w = OrbWindow::new("OmniType");
            // A genuinely new handle owns no cached rect and must be placed once.
            w.seed_last_for_test((10, 20, 300, 300));
            w.set_raw(0x1234);
            assert_eq!(w.last_rect(), None);

            // The same handle again — which is every frame — must not throw the
            // rect away.
            w.seed_last_for_test((10, 20, 300, 300));
            w.set_raw(0x1234);
            assert_eq!(
                w.last_rect(),
                Some((10, 20, 300, 300)),
                "the same handle every frame must not discard the rect"
            );
        }

        /// A window that has gone away has to give the rect up, so the handle can
        /// be resolved from scratch and the new one placed.
        #[test]
        fn losing_the_handle_clears_the_cached_rect() {
            let mut w = OrbWindow::new("OmniType");
            w.set_raw(0x1234);
            w.seed_last_for_test((1, 2, 3, 4));
            w.set_raw(0);
            assert_eq!(w.last_rect(), None);
            // ...and a zero handle is idempotent rather than a slow leak of state.
            w.set_raw(0);
            assert_eq!(w.last_rect(), None);
        }
    }
}
