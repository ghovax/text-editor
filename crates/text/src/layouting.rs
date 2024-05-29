use unicode_linebreak::BreakOpportunity;

use crate::{
    attributes::AttributesList,
    color::Color,
    font_system::FontSystem,
    glyph_cache::{CacheKey, CacheKeyFlags},
    shaping::{ShapedLine, ShapedSpan, ShapedWord},
};

/// A laid-out glyph together with all its configurations.
#[derive(Clone, Debug)]
pub struct LayoutedGlyph {
    /// Index of the glyph in the text.
    pub start_index: usize,
    /// End index of the glyph in the text.
    pub end_index: usize,
    /// Font size of the glyph.
    pub font_size: f32,
    /// Font id of the glyph.
    pub font_id: fontdb::ID,
    /// Font id of the glyph.
    pub glyph_id: u16,
    /// Horizontal offset of the hitbox.
    pub x: f32,
    /// Vertical offset of the hitbox.
    pub y: f32,
    /// Width of hitbox.
    pub width: f32,
    /// Height of hitbox.
    pub height: f32,
    /// Unicode BiDi embedding level, character is left-to-right if `level` is divisible by 2.
    pub level: unicode_bidi::Level,
    /// The vertical origin of the glyph, it is used to estimate the descent.
    pub y_origin: f32,
    /// The vertical maximum reach of the glyph.
    pub y_reach: f32,
    /// Horizontal offset in the line.
    ///
    /// This offset is useful when you are dealing with logical units and you do not care or
    /// cannot guarantee pixel grid alignment. For instance, when you want to use the glyphs
    /// for vectorial text, apply linear transformations to the layout, etc...
    pub x_offset: f32,
    /// Vertical offset in the line.
    ///
    /// This offset is useful when you are dealing with logical units and you do not care or
    /// cannot guarantee pixel grid alignment. For instance, when you want to use the glyphs
    /// for vectorial text, apply linear transformations to the layout, etc...
    pub y_offset: f32,
    /// Color of the glyph.
    pub color: Option<Color>,
    /// The flags needed for altering the rendering.
    pub cache_key_flags: CacheKeyFlags,
    /// Integer component of the horizontal offset in the line.
    pub physical_x_offset: Option<i32>,
    /// Integer component of the vertical offset in the line.
    pub physical_y_offset: Option<i32>,
    /// Cache key, see [CacheKey].
    pub cache_key: Option<CacheKey>,
}

impl LayoutedGlyph {
    pub fn layout_physically(&mut self, offset: (f32, f32), scale: f32) {
        // Account for the font size in the offsets calculation
        let x_offset = self.font_size * self.x_offset;
        let y_offset = self.font_size * self.y_offset;

        let (cache_key, x, y) = CacheKey::new(
            self.font_id,
            self.glyph_id,
            self.font_size * scale,
            (
                (self.x + x_offset) * scale + offset.0,
                libm::truncf((self.y - y_offset) * scale + offset.1), // Hinting in the vertical axis
            ),
            self.cache_key_flags,
        );

        self.cache_key = Some(cache_key);
        self.physical_x_offset = Some(x as i32);
        self.physical_y_offset = Some(y as i32);
    }

    pub fn contains_horizontal_position(&self, x: f32) -> bool {
        let x_offset = self.physical_x_offset.unwrap() as f32;

        x >= x_offset && x < x_offset + self.width
    }
}

/// A line of laid-out glyphs.
#[derive(Clone, Debug)]
pub struct LayoutedLine {
    /// Width of the line.
    pub width: f32,
    /// Maximum ascent of the glyphs in line.
    pub maximum_y_reach: f32,
    /// Maximum descent of the glyphs in line.
    pub minimum_y_origin: f32,
    /// Glyphs in line who have been positioned.
    pub layouted_glyphs: Vec<LayoutedGlyph>,
}
