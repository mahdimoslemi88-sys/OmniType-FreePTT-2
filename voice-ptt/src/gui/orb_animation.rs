//! Orb motion model: scale spring, position easing, breathing pulse,
//! blink scheduling and palette cross-fades. No rendering in here.

use std::f32::consts::{PI, TAU};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{Color32, Pos2};

use super::orb_palette::{self, OrbPalette};

/// Idle orb diameter in logical points. `scale` values multiply this.
// Sized to 60px diameter idle per companion specifications
pub const BASE_DIAMETER: f32 = 60.0;

/// Hover enlarges the orb a little. Read by `gui::orb`, which has to know the
/// largest scale the window has to hold: hover is only offered in the modes
/// that are draggable, so it can stack with that mode's own target.
pub(crate) const HOVER_SCALE_BOOST: f32 = 0.08; // 60pt -> ~65pt on hover
const SPRING_STIFFNESS: f32 = 170.0;
/// Damping ratio of the scale spring. Read by `gui::orb`'s tests, which derive
/// the scale overshoot from it rather than assuming the orb never exceeds its
/// target — a spring always overshoots, and an overshoot past the canvas would
/// crop the glow.
pub(crate) const SPRING_DAMPING_RATIO: f32 = 0.62; // slight, soft overshoot
const POSITION_SMOOTHING: f32 = 5.5; // 1/s, exponential approach
const HOVER_SMOOTHING: f32 = 12.0;
const PALETTE_FADE_SECS: f32 = 0.35;
const BLINK_SECS: f32 = 0.16;
const BLINK_MIN_GAP: f32 = 2.8;
const BLINK_MAX_GAP: f32 = 6.5;
const MAX_SPRING_STEP: f32 = 1.0 / 120.0;

/// Visual mode of the orb (decoupled from the app's `AppState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbMode {
    Idle,
    Recording,
    Processing,
    Complete,
    Error,
}

impl OrbMode {
    /// Target scale relative to `BASE_DIAMETER`.
    // Idle: 60px (scale 1.0), Recording: 100px, Processing: 90px, Complete: 90px
    pub fn target_scale(self) -> f32 {
        match self {
            OrbMode::Idle | OrbMode::Error => 1.0,
            OrbMode::Recording => 100.0 / BASE_DIAMETER,
            OrbMode::Processing => 90.0 / BASE_DIAMETER,
            OrbMode::Complete => 90.0 / BASE_DIAMETER,
        }
    }

    /// Whether the hover enlargement is offered in this mode.
    ///
    /// Only the draggable modes get it (`Orb::show` gates `set_hover` on the
    /// same condition), which is why `largest_target_scale` applies the boost
    /// per mode: multiplying the biggest target by it would invent a
    /// `Recording`-sized orb that hover can never produce.
    pub fn hoverable(self) -> bool {
        matches!(self, OrbMode::Idle | OrbMode::Error)
    }

    /// Every mode, in the order the UI can show them.
    pub const ALL: [OrbMode; 5] = [
        OrbMode::Idle,
        OrbMode::Recording,
        OrbMode::Processing,
        OrbMode::Complete,
        OrbMode::Error,
    ];

    pub fn palette(self) -> OrbPalette {
        match self {
            OrbMode::Idle | OrbMode::Error => orb_palette::IDLE,
            OrbMode::Recording => orb_palette::RECORDING,
            OrbMode::Processing => orb_palette::PROCESSING,
            OrbMode::Complete => orb_palette::COMPLETE,
        }
    }

    /// Breathing speed in radians per second.
    fn breath_speed(self) -> f32 {
        match self {
            OrbMode::Idle | OrbMode::Error => TAU / 4.2,
            OrbMode::Recording => TAU / 1.6,
            OrbMode::Processing => TAU / 2.4,
            OrbMode::Complete => TAU / 2.0,
        }
    }
}

/// Largest scale any mode ever *aims* for, hover included.
pub fn largest_target_scale() -> f32 {
    OrbMode::ALL
        .iter()
        .map(|m| {
            let boost = if m.hoverable() {
                HOVER_SCALE_BOOST
            } else {
                0.0
            };
            m.target_scale() * (1.0 + boost)
        })
        .fold(0.0f32, f32::max)
}

/// Largest scale the scale spring can actually reach, in points of `scale`.
///
/// Not simply [`largest_target_scale`]: `OrbAnimation` integrates a spring, and
/// a spring overshoots. The step response of a second-order system peaks at
/// `exp(-pi*zeta / sqrt(1 - zeta^2))` above its target, so that is the number to
/// multiply by — doubled, because repeated hotkey taps can stack on velocity
/// that has not yet damped out.
///
/// This is the number the host window has to be sized for. It lives here, next
/// to the spring it describes, so the window and the animation cannot disagree
/// about how large the orb can get; the test in `gui::orb` checks that the
/// canvas derived from it actually holds the drawn orb.
pub fn max_reachable_scale() -> f32 {
    let zeta = SPRING_DAMPING_RATIO;
    let overshoot = (-PI * zeta / (1.0f32 - zeta * zeta).sqrt()).exp();
    largest_target_scale() * (1.0 + overshoot * 2.0)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
    }
}

pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| -> u8 {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

pub fn lerp_palette(a: &OrbPalette, b: &OrbPalette, t: f32) -> OrbPalette {
    OrbPalette {
        core: lerp_color(a.core, b.core, t),
        rim: lerp_color(a.rim, b.rim, t),
        glow: lerp_color(a.glow, b.glow, t),
        accent: lerp_color(a.accent, b.accent, t),
        eye: lerp_color(a.eye, b.eye, t),
        highlight: lerp_color(a.highlight, b.highlight, t),
        glow_strength: a.glow_strength + (b.glow_strength - a.glow_strength) * t.clamp(0.0, 1.0),
    }
}

pub struct OrbAnimation {
    /// Current scale relative to `BASE_DIAMETER`.
    pub current_scale: f32,
    pub target_scale: f32,
    scale_velocity: f32,
    /// Orb center in physical screen pixels.
    pub current_position: Pos2,
    pub target_position: Pos2,
    pub pulse_phase: f32,
    /// Seconds until the next blink starts.
    pub blink_timer: f32,
    blink_progress: Option<f32>,
    ripple_phase: f32,
    spin_phase: f32,
    hover: f32,
    hover_target: f32,
    mode: OrbMode,
    mode_time: f32,
    palette_from: OrbPalette,
    palette_to: OrbPalette,
    palette_t: f32,
    rng_state: u32,
}

impl OrbAnimation {
    pub fn new(position: Pos2) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0x9E37_79B9)
            | 1;
        let mut anim = Self {
            current_scale: 1.0,
            target_scale: 1.0,
            scale_velocity: 0.0,
            current_position: position,
            target_position: position,
            pulse_phase: 0.0,
            blink_timer: 0.0,
            blink_progress: None,
            ripple_phase: 0.0,
            spin_phase: 0.0,
            hover: 0.0,
            hover_target: 0.0,
            mode: OrbMode::Idle,
            mode_time: 0.0,
            palette_from: orb_palette::IDLE,
            palette_to: orb_palette::IDLE,
            palette_t: 1.0,
            rng_state: seed,
        };
        anim.blink_timer = anim.next_blink_gap();
        anim
    }

    pub fn set_mode(&mut self, mode: OrbMode) {
        if mode == self.mode {
            return;
        }
        self.palette_from = self.palette();
        self.palette_to = mode.palette();
        self.palette_t = 0.0;
        self.mode = mode;
        self.mode_time = 0.0;
        if mode == OrbMode::Complete {
            self.blink_progress = None;
        }
    }

    pub fn set_hover(&mut self, hovered: bool) {
        self.hover_target = if hovered { 1.0 } else { 0.0 };
    }

    pub fn set_target_position(&mut self, position: Pos2) {
        self.target_position = position;
    }

    /// Jump without easing (used while dragging).
    pub fn snap_position(&mut self, position: Pos2) {
        self.current_position = position;
        self.target_position = position;
    }

    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }

        // Hover
        let a = 1.0 - (-HOVER_SMOOTHING * dt).exp();
        self.hover += (self.hover_target - self.hover) * a;
        if (self.hover_target - self.hover).abs() < 0.005 {
            self.hover = self.hover_target;
        }

        // Scale target
        let hover_boost = match self.mode {
            OrbMode::Idle | OrbMode::Error => HOVER_SCALE_BOOST * self.hover,
            _ => 0.0,
        };
        self.target_scale = self.mode.target_scale() * (1.0 + hover_boost);

        // Scale spring (sub-stepped, stable at low idle frame rates)
        let omega = SPRING_STIFFNESS.sqrt();
        let damping = 2.0 * SPRING_DAMPING_RATIO * omega;
        let mut remaining = dt;
        while remaining > 0.0 {
            let h = remaining.min(MAX_SPRING_STEP);
            let accel = SPRING_STIFFNESS * (self.target_scale - self.current_scale)
                - damping * self.scale_velocity;
            self.scale_velocity += accel * h;
            self.current_scale += self.scale_velocity * h;
            remaining -= h;
        }
        if (self.current_scale - self.target_scale).abs() < 0.0015
            && self.scale_velocity.abs() < 0.003
        {
            self.current_scale = self.target_scale;
            self.scale_velocity = 0.0;
        }
        self.current_scale = self.current_scale.max(0.2);

        // Position
        let a = 1.0 - (-POSITION_SMOOTHING * dt).exp();
        self.current_position += (self.target_position - self.current_position) * a;
        if (self.target_position - self.current_position).length() < 0.35 {
            self.current_position = self.target_position;
        }

        // Phases
        self.pulse_phase = (self.pulse_phase + dt * self.mode.breath_speed()) % TAU;
        self.ripple_phase = (self.ripple_phase + dt * 0.7) % 1.0;
        self.spin_phase = (self.spin_phase + dt * 3.4) % TAU;

        // Blink
        if self.mode == OrbMode::Complete {
            self.blink_progress = None;
        } else if let Some(p) = self.blink_progress {
            let p = p + dt / BLINK_SECS;
            if p >= 1.0 {
                self.blink_progress = None;
                self.blink_timer = self.next_blink_gap();
            } else {
                self.blink_progress = Some(p);
            }
        } else {
            self.blink_timer -= dt;
            if self.blink_timer <= 0.0 {
                self.blink_progress = Some(0.0);
            }
        }

        // Palette cross-fade
        self.palette_t = (self.palette_t + dt / PALETTE_FADE_SECS).min(1.0);

        self.mode_time += dt;
    }

    pub fn mode(&self) -> OrbMode {
        self.mode
    }

    pub fn mode_time(&self) -> f32 {
        self.mode_time
    }

    /// -1..1 breathing wave.
    pub fn breath(&self) -> f32 {
        self.pulse_phase.sin()
    }

    /// 0..1, loops.
    pub fn ripple_phase(&self) -> f32 {
        self.ripple_phase
    }

    /// Radians, loops.
    pub fn spin_phase(&self) -> f32 {
        self.spin_phase
    }

    pub fn hover(&self) -> f32 {
        self.hover
    }

    /// 1 = open, ~0 = closed.
    pub fn eye_openness(&self) -> f32 {
        match self.blink_progress {
            Some(p) => (1.0 - (p * PI).sin()).max(0.08),
            None => 1.0,
        }
    }

    /// True while blinking or about to blink (used to raise idle frame rate briefly).
    pub fn is_blinking(&self) -> bool {
        self.blink_progress.is_some() || self.blink_timer < 0.12
    }

    pub fn palette(&self) -> OrbPalette {
        lerp_palette(
            &self.palette_from,
            &self.palette_to,
            ease_in_out_cubic(self.palette_t),
        )
    }

    pub fn scale_settled(&self) -> bool {
        (self.current_scale - self.target_scale).abs() < 0.002 && self.scale_velocity.abs() < 0.005
    }

    pub fn is_settled(&self) -> bool {
        self.scale_settled()
            && self.current_position == self.target_position
            && self.palette_t >= 1.0
            && self.hover == self.hover_target
    }

    fn next_rand(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn next_blink_gap(&mut self) -> f32 {
        BLINK_MIN_GAP + (BLINK_MAX_GAP - BLINK_MIN_GAP) * self.next_rand()
    }
}
