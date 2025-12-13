use rustybuzz::{
    ttf_parser::{Face, GlyphId, Rect},
    UnicodeBuffer,
};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{
    attributes::AttributesList,
    color::Color,
    font_system::FontSystem,
    glyph_cache::CacheKeyFlags,
    layouting::{LayoutedGlyph, LayoutedLine},
};

/// A shaped glyph.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// The starting position of the glyph in the source text.
    pub start_index: usize,
    /// The ending position of the glyph in the source text.
    pub end_index: usize,
    /// The horizontal advance after rendering the glyph.
    pub x_advance: f32,
    /// The vertical advance after rendering the glyph.
    pub y_advance: f32,
    /// The horizontal offset of the glyph from its origin.
    pub x_offset: f32,
    /// The vertical offset of the glyph from its origin.
    pub y_offset: f32,
    /// The height of the glyph.
    pub height: f32,
    /// The vertical origin of the glyph, it is used to estimate the descent.
    pub y_origin: f32,
    /// The vertical maximum reach of the glyph.
    pub y_reach: f32,
    /// The identifier for the font used to render the glyph.
    pub font_id: fontdb::ID,
    /// The identifier for the glyph within the font.
    pub glyph_id: u16,
    /// An optional color for the glyph.
    pub color: Option<Color>,
    /// Flags used for cache key generation.
    pub cache_key_flags: CacheKeyFlags,
}

impl ShapedGlyph {
    fn as_layouted_glyph(
        &self,
        font_size: f32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        level: unicode_bidi::Level,
    ) -> LayoutedGlyph {
        LayoutedGlyph {
            start_index: self.start_index,
            end_index: self.end_index,
            font_size,
            font_id: self.font_id,
            glyph_id: self.glyph_id,
            x,
            y,
            width,
            height,
            level,
            y_origin: self.y_origin,
            y_reach: self.y_reach,
            x_offset: self.x_offset,
            y_offset: self.y_offset,
            color: self.color,
            cache_key_flags: self.cache_key_flags,
            physical_x_offset: None,
            physical_y_offset: None,
            cache_key: None,
        }
    }
}

/// A shaped word (for word wrapping).
#[derive(Clone, Debug)]
pub struct ShapedWord {
    /// Indicates if the word consists of blank (whitespace) characters.
    pub is_blank: bool,
    /// A vector of shaped glyphs that make up the word.
    pub shaped_glyphs: Vec<ShapedGlyph>,
    /// The total horizontal advance after rendering the word.
    pub total_x_advance: f32,
    /// The total vertical advance after rendering the word.
    pub total_y_advance: f32,
}

impl ShapedWord {
    pub fn new(
        font_system: &mut FontSystem,
        text: &str,
        attributes_list: &AttributesList,
        word_range: std::ops::Range<usize>,
        level: unicode_bidi::Level,
        is_blank: bool,
    ) -> Self {
        let word = &text[word_range.clone()];

        let mut shaped_glyphs = Vec::new();
        let is_span_right_to_left = level.is_rtl();

        let mut start_index = word_range.start;
        let mut default_attributes = attributes_list.default_attributes;

        for (grapheme_cluster_index, _) in word.grapheme_indices(true) {
            let start_grapheme_cluster_index = word_range.start + grapheme_cluster_index;
            let grapheme_cluster_attributes = attributes_list.get_span(start_grapheme_cluster_index);

            if !default_attributes.is_compatible_with(&grapheme_cluster_attributes) {
                shape_word(
                    font_system,
                    &mut shaped_glyphs,
                    text,
                    attributes_list,
                    start_index,
                    start_grapheme_cluster_index,
                    is_span_right_to_left,
                );

                start_index = start_grapheme_cluster_index;
                default_attributes = *grapheme_cluster_attributes;
            }
        }

        if start_index < word_range.end {
            shape_word(
                font_system,
                &mut shaped_glyphs,
                text,
                attributes_list,
                start_index,
                word_range.end,
                is_span_right_to_left,
            )
        }

        let total_x_advance: f32 = shaped_glyphs.iter().map(|glyph| glyph.x_advance).sum();
        let total_y_advance: f32 = shaped_glyphs.iter().map(|glyph| glyph.y_advance).sum();

        Self {
            is_blank,
            shaped_glyphs,
            total_x_advance,
            total_y_advance,
        }
    }
}

fn shape_word(
    font_system: &mut FontSystem,
    shaped_glyphs: &mut Vec<ShapedGlyph>,
    text: &str,
    attributes_list: &AttributesList,
    start_index: usize,
    end_index: usize,
    is_span_right_to_left: bool,
) {
    let mut scripts = Vec::new();

    for character in text[start_index..end_index].chars() {
        match character.script() {
            Script::Common | Script::Inherited | Script::Latin | Script::Unknown => (),
            script => {
                if !scripts.contains(&script) {
                    scripts.push(script);
                }
            }
        }
    }

    let attributes = attributes_list.get_span(start_index);
    let font_match_keys = font_system.get_font_matches(*attributes);

    let mut selected_font = None;
    for font_match_key in font_match_keys.iter() {
        // Check if the font matches any of the families
        let font_family_name = font_system.database.family_name(&attributes.family);
        let face_contains_family = if let Some(face) = font_system.database.face(font_match_key.id) {
            face.families.iter().any(|(name, _)| name == font_family_name)
        } else {
            false
        };

        if face_contains_family {
            if let Some(font) = font_system.get_font(font_match_key.id) {
                selected_font = Some(font);
            }
        }
    }

    // If no exact family match, fall back to the first available font
    if selected_font.is_none() {
        for font_match_key in font_match_keys.iter() {
            if let Some(font) = font_system.get_font(font_match_key.id) {
                selected_font = Some(font);
                break;
            }
        }
    }

    let font = selected_font.expect("No suitable font found for text rendering");

    let word_text = &text[start_index..end_index];

    let font_face = font.face.borrow_dependent();
    let font_scale = font_face.units_per_em() as f32;
    let ascent = font_face.ascender() as f32 / font_scale;
    let descent = -font_face.descender() as f32 / font_scale;

    let mut buffer = UnicodeBuffer::new();
    buffer.set_direction(if is_span_right_to_left {
        rustybuzz::Direction::RightToLeft
    } else {
        rustybuzz::Direction::LeftToRight
    });
    buffer.push_str(word_text);
    buffer.guess_segment_properties();

    let is_buffer_right_to_left = matches!(buffer.direction(), rustybuzz::Direction::RightToLeft);
    assert_eq!(is_buffer_right_to_left, is_span_right_to_left);

    let shape_plan = font_system
        .shape_plan_cache
        .get_from_font_and_buffer_info(&font, &buffer);
    let glyph_buffer = rustybuzz::shape_with_plan(font_face, shape_plan, buffer);
    let glyph_infos = glyph_buffer.glyph_infos();
    let glyph_positions = glyph_buffer.glyph_positions();

    let mut missing_glyphs = Vec::new();
    shaped_glyphs.reserve(glyph_infos.len());
    let glyph_start_index = shaped_glyphs.len();

    for (info, glyph_position) in glyph_infos.iter().zip(glyph_positions.iter()) {
        let x_advance = glyph_position.x_advance as f32 / font_scale;
        let y_advance = glyph_position.y_advance as f32 / font_scale;
        let x_offset = glyph_position.x_offset as f32 / font_scale;
        let y_offset = glyph_position.y_offset as f32 / font_scale;

        let glyph_bounding_box = match font_face.glyph_bounding_box(GlyphId(info.glyph_id as u16)) {
            Some(bounding_box) => bounding_box,
            None => Rect {
                x_min: 0,
                y_min: 0,
                x_max: 0,
                y_max: 0, // TODO
            },
        };
        let glyph_height = glyph_bounding_box.height() as f32 / font_scale;
        let glyph_y_origin = glyph_bounding_box.y_min as f32 / font_scale;
        let glyph_y_reach = glyph_bounding_box.y_max as f32 / font_scale;

        let glyph_index = start_index + info.cluster as usize;

        if info.glyph_id == 0 {
            missing_glyphs.push(glyph_index);
        }

        let attributes = attributes_list.get_span(glyph_index);
        shaped_glyphs.push(ShapedGlyph {
            start_index: glyph_index,
            end_index, // Set later
            x_advance,
            y_advance,
            x_offset,
            y_offset,
            height: glyph_height,
            y_origin: glyph_y_origin,
            y_reach: glyph_y_reach,
            font_id: font.id,
            glyph_id: info.glyph_id.try_into().unwrap(),
            color: attributes.color,
            cache_key_flags: attributes.cache_key_flags,
        });
    }

    // Adjust end of glyphs
    if is_buffer_right_to_left {
        for index in glyph_start_index + 1..shaped_glyphs.len() {
            let next_start = shaped_glyphs[index - 1].start_index;
            let next_end = shaped_glyphs[index - 1].end_index;
            let previous_glyph = &mut shaped_glyphs[index];

            if previous_glyph.start_index == next_start {
                previous_glyph.end_index = next_end;
            } else {
                previous_glyph.end_index = next_start;
            }
        }
    } else {
        for index in (glyph_start_index + 1..shaped_glyphs.len()).rev() {
            let next_start = shaped_glyphs[index].start_index;
            let next_end = shaped_glyphs[index].end_index;
            let previous_glyph = &mut shaped_glyphs[index - 1];

            if previous_glyph.start_index == next_start {
                previous_glyph.end_index = next_end;
            } else {
                previous_glyph.end_index = next_start;
            }
        }
    }

    while !missing_glyphs.is_empty() {
        log::warn!("There are missing glyphs: {missing_glyphs:?}");
    }
}

/// Get the character associated with a glyph ID.
fn get_character_from_glyph_id(face: &Face, glyph_id: GlyphId) -> Option<char> {
    let mut gid_character = None;
    // Iterate over all character to glyph mappings
    for subtable in face.tables().cmap?.subtables {
        if gid_character.is_some() {
            break;
        }

        subtable.codepoints(|codepoint| {
            if let Some(gid) = subtable.glyph_index(codepoint) {
                if gid == glyph_id {
                    gid_character = Some(std::char::from_u32(codepoint).unwrap());
                }
            }
        });
    }

    gid_character
}

/// A shaped span (for bidirectional processing).
#[derive(Clone, Debug)]
pub struct ShapedSpan {
    pub level: unicode_bidi::Level,
    pub shaped_words: Vec<ShapedWord>,
}

impl ShapedSpan {
    pub fn new(
        font_system: &mut FontSystem,
        text: &str,
        attributes_list: &AttributesList,
        span_range: std::ops::Range<usize>,
        line_is_right_to_left: bool,
        level: unicode_bidi::Level,
    ) -> Self {
        let text_span = &text[span_range.start..span_range.end];

        let mut shaped_words = Vec::new();
        let mut start_word = 0;

        for (linebreak_end_index, _) in unicode_linebreak::linebreaks(text_span) {
            let mut linebreak_start_index = linebreak_end_index;

            for (character_index, character) in text_span[start_word..linebreak_end_index].char_indices().rev() {
                // TODO(ghovax): Not all whitespace characters are linebreakable, e.g. 00A0 (No-break space)
                // https://www.unicode.org/reports/tr14/#GL
                // https://www.unicode.org/Public/UCD/latest/ucd/PropList.txt
                if character.is_whitespace() {
                    linebreak_start_index = start_word + character_index;
                } else {
                    break;
                }
            }

            if start_word < linebreak_start_index {
                let is_blank = false;

                shaped_words.push(ShapedWord::new(
                    font_system,
                    text,
                    attributes_list,
                    (span_range.start + start_word)..(span_range.start + linebreak_start_index),
                    level,
                    is_blank,
                ));
            }
            if linebreak_start_index < linebreak_end_index {
                let is_blank = true;

                for (character_index, character) in text_span[linebreak_start_index..linebreak_end_index].char_indices()
                {
                    shaped_words.push(ShapedWord::new(
                        font_system,
                        text,
                        attributes_list,
                        (span_range.start + linebreak_start_index + character_index)
                            ..(span_range.start + linebreak_start_index + character_index + character.len_utf8()),
                        level,
                        is_blank,
                    ));
                }
            }

            start_word = linebreak_end_index;
        }

        // Reverse glyphs in RTL lines
        if line_is_right_to_left {
            for word in &mut shaped_words {
                word.shaped_glyphs.reverse();
            }
        }

        // Reverse words in spans that do not match line direction
        if line_is_right_to_left != level.is_rtl() {
            shaped_words.reverse();
        }

        ShapedSpan { level, shaped_words }
    }
}

/// A shaped line.
#[derive(Clone, Debug)]
pub struct ShapedLine {
    pub is_right_to_left: bool,
    pub shaped_spans: Vec<ShapedSpan>,
}

impl ShapedLine {
    pub fn new(font_system: &mut FontSystem, text: &str, attributes_list: &AttributesList) -> Option<Self> {
        let mut shaped_spans = Vec::new();

        let bidirectional_info = unicode_bidi::BidiInfo::new(text, None);
        let is_right_to_left = if bidirectional_info.paragraphs.is_empty() {
            false
        } else {
            bidirectional_info.paragraphs.first().unwrap().level.is_rtl()
        };

        for paragraph_info in bidirectional_info.paragraphs.iter() {
            let line_is_right_to_left = paragraph_info.level.is_rtl();
            if line_is_right_to_left != is_right_to_left {
                return None;
            }

            let line_range = paragraph_info.range.clone();
            let levels = Self::adjust_levels(&unicode_bidi::Paragraph::new(&bidirectional_info, paragraph_info));

            // Find consecutive level word_texts. We use this to create `ShapedSpan`s
            // Each span is a set of characters with equal levels
            let mut start_index = line_range.start;
            let mut word_text_level = levels.get(start_index)?;
            shaped_spans.reserve(line_range.end - start_index + 1);

            for (index, level) in levels.iter().enumerate().take(line_range.end).skip(start_index + 1) {
                if level != word_text_level {
                    // End of the previous word_text, start of a new one
                    shaped_spans.push(ShapedSpan::new(
                        font_system,
                        text,
                        attributes_list,
                        start_index..index,
                        line_is_right_to_left,
                        *word_text_level,
                    ));

                    start_index = index;
                    word_text_level = level;
                }
            }

            shaped_spans.push(ShapedSpan::new(
                font_system,
                text,
                attributes_list,
                start_index..line_range.end,
                line_is_right_to_left,
                *word_text_level,
            ));
        }

        Some(Self {
            is_right_to_left: false,
            shaped_spans,
        })
    }

    /// A modified version of first part of `unicode_bidi::bidi_info::visual_word_text`
    fn adjust_levels(paragraph: &unicode_bidi::Paragraph) -> Vec<unicode_bidi::Level> {
        use unicode_bidi::BidiClass::*;

        let text = paragraph.info.text;
        let levels = &paragraph.info.levels;
        let original_classes = &paragraph.info.original_classes;

        let mut levels = levels.clone();
        let line_classes = &original_classes[..];
        let line_levels = &mut levels[..];

        // Reset some whitespace chars to paragraph level
        // <http://www.unicode.org/reports/tr9/#L1>
        let mut reset_from: Option<usize> = Some(0);
        let mut reset_to: Option<usize> = None;

        for (character_index, character) in text.char_indices() {
            match line_classes[character_index] {
                // Ignored by X9
                RLE | LRE | RLO | LRO | PDF | BN => {}
                // Segment separator, paragraph separator
                B | S => {
                    assert_eq!(reset_to, None);
                    reset_to = Some(character_index + character.len_utf8());

                    if reset_from.is_none() {
                        reset_from = Some(character_index);
                    }
                }
                // Whitespace, isolate formatting
                WS | FSI | LRI | RLI | PDI => {
                    if reset_from.is_none() {
                        reset_from = Some(character_index);
                    }
                }
                _ => {
                    reset_from = None;
                }
            }

            if let (Some(from), Some(to)) = (reset_from, reset_to) {
                for level in &mut line_levels[from..to] {
                    *level = paragraph.para.level;
                }

                reset_from = None;
                reset_to = None;
            }
        }

        if let Some(from) = reset_from {
            for level in &mut line_levels[from..] {
                *level = paragraph.para.level;
            }
        }

        levels
    }

    // TODO(ghovax): Does not yet handle right to left layouts
    pub fn layout(&self, font_size: f32) -> LayoutedLine {
        // Initialize variables
        let mut line_width: f32 = 0.0;
        let mut maximum_y_reach: f32 = 0.0;
        let mut minimum_y_origin: f32 = 0.0;

        // Collect all glyphs from spans
        let mut layouted_glyphs = Vec::new();
        let mut x_cursor = 0.0;
        let mut y_cursor = 0.0;

        for span in &self.shaped_spans {
            for word in &span.shaped_words {
                for glyph in &word.shaped_glyphs {
                    let x_advance = font_size * glyph.x_advance;
                    let y_advance = font_size * glyph.y_advance;
                    let height = glyph.height * font_size;

                    // Push glyph to layout
                    layouted_glyphs
                        .push(glyph.as_layouted_glyph(font_size, x_cursor, y_cursor, x_advance, height, span.level));

                    if !self.is_right_to_left {
                        x_cursor += x_advance;
                        line_width += x_advance;
                    }
                    y_cursor += y_advance;

                    maximum_y_reach = maximum_y_reach.max(glyph.y_reach * font_size);
                    minimum_y_origin = minimum_y_origin.min(glyph.y_origin * font_size);
                }
            }
        }

        // Create a single layouted line
        LayoutedLine {
            width: line_width,
            maximum_y_reach,
            minimum_y_origin,
            layouted_glyphs,
        }
    }
}
