//! OmniType orb palette. Colors only, no logic.
//! No purple, violet, magenta, blue, red or yellow anywhere in here.

use eframe::egui::Color32;

/// One complete look for a single orb state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbPalette {
    /// Lit center of the body (gradient end, toward the light source).
    pub core: Color32,
    /// Outer edge of the body (gradient start).
    pub rim: Color32,
    /// Soft halo around the orb.
    pub glow: Color32,
    /// Rings, waves, particles.
    pub accent: Color32,
    /// Eyes.
    pub eye: Color32,
    /// Specular glass highlight.
    pub highlight: Color32,
    /// Halo intensity multiplier (0..~1).
    pub glow_strength: f32,
}

// Base tones
pub const PEARL_WHITE: Color32 = Color32::from_rgb(255, 255, 255);
pub const SOFT_SILVER: Color32 = Color32::from_rgb(220, 230, 240);
pub const GRAPHITE: Color32 = Color32::from_rgb(38, 44, 52);

pub const CORAL: Color32 = Color32::from_rgb(255, 120, 95);
pub const PEACH: Color32 = Color32::from_rgb(255, 180, 150);
pub const CORAL_INK: Color32 = Color32::from_rgb(96, 40, 28);
pub const CORAL_SHINE: Color32 = Color32::from_rgb(255, 236, 226);

pub const CYAN: Color32 = Color32::from_rgb(34, 211, 238);
pub const ICE: Color32 = Color32::from_rgb(165, 243, 252);
pub const CYAN_INK: Color32 = Color32::from_rgb(8, 70, 84);
pub const CYAN_SHINE: Color32 = Color32::from_rgb(236, 254, 255);

pub const MINT: Color32 = Color32::from_rgb(52, 211, 153);
pub const JADE: Color32 = Color32::from_rgb(16, 185, 129);
pub const JADE_INK: Color32 = Color32::from_rgb(6, 78, 59);
pub const MINT_SHINE: Color32 = Color32::from_rgb(220, 252, 236);

/// Idle: pearl white core, soft silver glow.
pub const IDLE: OrbPalette = OrbPalette {
    core: PEARL_WHITE,
    rim: SOFT_SILVER,
    glow: SOFT_SILVER,
    accent: SOFT_SILVER,
    eye: GRAPHITE,
    highlight: PEARL_WHITE,
    glow_strength: 0.45,
};

/// Recording / listening: warm coral, peach glow.
pub const RECORDING: OrbPalette = OrbPalette {
    core: PEACH,
    rim: CORAL,
    glow: PEACH,
    accent: CORAL,
    eye: CORAL_INK,
    highlight: CORAL_SHINE,
    glow_strength: 0.95,
};

/// Processing: cyan with ice highlights.
pub const PROCESSING: OrbPalette = OrbPalette {
    core: ICE,
    rim: CYAN,
    glow: CYAN,
    accent: ICE,
    eye: CYAN_INK,
    highlight: CYAN_SHINE,
    glow_strength: 0.8,
};

/// Complete / typing: mint fresh with jade depth.
pub const COMPLETE: OrbPalette = OrbPalette {
    core: MINT,
    rim: JADE,
    glow: MINT,
    accent: MINT,
    eye: JADE_INK,
    highlight: MINT_SHINE,
    glow_strength: 0.9,
};
