use std::collections::HashMap;

use swash::{
    scale::{
        image::{Content, Image},
        Render, ScaleContext, Source, StrikeWith,
    },
    zeno::{Angle, Format, Transform, Vector},
    CacheKey,
};

/// Cache for rasterizing with the swash scaler.
// TODO(ghovax): `#[derive(Debug, Clone)]` does not work.
pub struct SwashCache {
    context: ScaleContext,
    pub image_cache: HashMap<CacheKey, Option<Image>>,
    pub outline_command_cache: HashMap<CacheKey, Option<Vec<swash::zeno::Command>>>,
}

impl SwashCache {
    /// Create a new swash cache.
    pub fn new() -> Self {
        Self {
            context: ScaleContext::new(),
            image_cache: HashMap::default(),
            outline_command_cache: HashMap::default(),
        }
    }

    /// Create a swash `Image`` from a cache key, without caching the results.
    pub fn get_image_uncached(&mut self, font_system: &mut FontSystem, cache_key: CacheKey) -> Option<Image> {
        swash_image_from_cache_key(font_system, &mut self.context, cache_key)
    }

    /// Create a swash Image from a cache key, caching results
    pub fn get_image(&mut self, font_system: &mut FontSystem, cache_key: CacheKey) -> &Option<Image> {
        self.image_cache
            .entry(cache_key)
            .or_insert_with(|| swash_image_from_cache_key(font_system, &mut self.context, cache_key))
    }

    pub fn get_outline_commands(
        &mut self,
        font_system: &mut FontSystem,
        cache_key: CacheKey,
    ) -> Option<&[swash::zeno::Command]> {
        self.outline_command_cache
            .entry(cache_key)
            .or_insert_with(|| swash_outline_commands_from_cache_key(font_system, &mut self.context, cache_key))
            .as_deref()
    }

    /// Enumerate pixels in an `Image`, use `with_image` for better performance.
    pub fn with_pixels<F: FnMut(i32, i32, Color)>(
        &mut self,
        font_system: &mut FontSystem,
        cache_key: CacheKey,
        base_color: Color,
        mut drawing_function: F,
    ) {
        if let Some(image) = self.get_image(font_system, cache_key) {
            let x = image.placement.left;
            let y = -image.placement.top;

            match image.content {
                Content::Mask => {
                    let mut index = 0;
                    for offset_y in 0..image.placement.height as i32 {
                        for offset_x in 0..image.placement.width as i32 {
                            // TODO: Blend base alpha?
                            drawing_function(
                                x + offset_x,
                                y + offset_y,
                                Color(((image.data[index] as u32) << 24) | base_color.0 & 0xFF_FF_FF),
                            );
                            index += 1;
                        }
                    }
                }
                Content::Color => {
                    let mut index = 0;
                    for offset_y in 0..image.placement.height as i32 {
                        for offset_x in 0..image.placement.width as i32 {
                            // TODO: Blend base alpha?
                            drawing_function(
                                x + offset_x,
                                y + offset_y,
                                Color::rgba(image.data[index], image.data[index + 1], image.data[index + 2], image.data[index + 3]),
                            );
                            index += 4;
                        }
                    }
                }
                Content::SubpixelMask => {
                    log::warn!("SubpixelMask isn't yet implemented");
                }
            }
        }
    }
}

fn swash_image_from_cache_key(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
) -> Option<Image> {
    let font = match font_system.get_font(cache_key.font_id) {
        Some(some) => some,
        None => {
            log::warn!("Unable to find the font with the ID {:?}", cache_key.font_id);
            return None;
        }
    };

    // Build the scaler for the font
    let mut scaler = context
        .builder(font.as_swash())
        .size(f32::from_bits(cache_key.font_size_bits))
        .hint(true)
        .build();

    // Compute the fractional offset
    // NOTE(ghovax): You'll likely want to quantize this in a real renderer.
    let offset = Vector::new(cache_key.x_bin.as_float(), cache_key.y_bin.as_float());

    // Select our source order
    Render::new(&[
        // Color outline with the first palette
        Source::ColorOutline(0),
        // Color bitmap with best fit selection mode
        Source::ColorBitmap(StrikeWith::BestFit),
        // Standard scalable outline
        Source::Outline,
    ])
    // Select a subpixel format
    .format(Format::Alpha)
    // Apply the fractional offset
    .offset(offset)
    // TODO(ghovax): I might want to add more features here.
    .transform(if cache_key.flags.contains(CacheKeyFlags::FAKE_ITALIC) {
        Some(Transform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0)))
    } else {
        None
    })
    // Render the image
    .render(&mut scaler, cache_key.glyph_id)
}

fn swash_outline_commands_from_cache_key(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
) -> Option<Vec<swash::zeno::Command>> {
    use swash::zeno::PathData as _;

    let font = match font_system.get_font(cache_key.font_id) {
        Some(some) => some,
        None => {
            log::warn!("Unable to find the font with the ID {:?}", cache_key.font_id);
            return None;
        }
    };

    // Build the scaler
    let mut scaler = context
        .builder(font.as_swash())
        .size(f32::from_bits(cache_key.font_size_bits))
        .build();

    // Scale the outline
    let outline = scaler
        .scale_outline(cache_key.glyph_id)
        .or_else(|| scaler.scale_color_outline(cache_key.glyph_id))?;

    // Get the path information of the outline
    let path = outline.path();

    // Return the commands
    Some(path.commands().collect())
}
