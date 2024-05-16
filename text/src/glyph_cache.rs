// SPDX-License-Identifier: MIT OR Apache-2.0

bitflags::bitflags! {
    /// Flags that change rendering.
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    #[repr(transparent)]
    pub struct CacheKeyFlags: u32 {
        /// Skew by 14 degrees to synthesize italic.
        const FAKE_ITALIC = 1;
    }
}

/// Key for building a glyph cache.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CacheKey {
    /// Font ID.
    pub font_id: fontdb::ID,
    /// Glyph ID.
    pub glyph_id: u16,
    /// `f32` bits of font size.
    pub font_size_bits: u32,
    /// Binning of fractional X offset.
    pub x_bin: SubpixelBin,
    /// Binning of fractional Y offset.
    pub y_bin: SubpixelBin,
    /// Flags that alter the rendering.
    pub flags: CacheKeyFlags,
}

impl CacheKey {
    pub fn new(
        font_id: fontdb::ID,
        glyph_id: u16,
        font_size: f32,
        position: (f32, f32),
        flags: CacheKeyFlags,
    ) -> (Self, i32, i32) {
        let (x, x_bin) = SubpixelBin::new(position.0);
        let (y, y_bin) = SubpixelBin::new(position.1);
        (
            Self {
                font_id,
                glyph_id,
                font_size_bits: font_size.to_bits(),
                x_bin,
                y_bin,
                flags,
            },
            x,
            y,
        )
    }
}

/// Binning of subpixel position for cache optimization.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SubpixelBin {
    Zero,
    One,
    Two,
    Three,
}

impl SubpixelBin {
    pub fn new(position: f32) -> (i32, Self) {
        let (fraction, truncation) = libm::modff(position);
        let truncation = truncation as i32;

        if position.is_sign_negative() {
            if fraction > -0.125 {
                (truncation, Self::Zero)
            } else if fraction > -0.375 {
                (truncation - 1, Self::Three)
            } else if fraction > -0.625 {
                (truncation - 1, Self::Two)
            } else if fraction > -0.875 {
                (truncation - 1, Self::One)
            } else {
                (truncation - 1, Self::Zero)
            }
        } else {
            #[allow(clippy::collapsible_else_if)]
            if fraction < 0.125 {
                (truncation, Self::Zero)
            } else if fraction < 0.375 {
                (truncation, Self::One)
            } else if fraction < 0.625 {
                (truncation, Self::Two)
            } else if fraction < 0.875 {
                (truncation, Self::Three)
            } else {
                (truncation + 1, Self::Zero)
            }
        }
    }

    pub fn as_float(&self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::One => 0.25,
            Self::Two => 0.5,
            Self::Three => 0.75,
        }
    }
}
