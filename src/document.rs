use std::{num::NonZeroU32, rc::Rc};

use text::{
    attributes::Attributes, color::Color, font_system::FontSystem, line_buffer::LineBuffer,
    swash_cache::SwashCache,
};
use tiny_skia::{BlendMode, LineJoin, Paint, PathBuilder, PixmapMut, Rect, Stroke, Transform};
use unicode_segmentation::UnicodeSegmentation as _;
use winit::{dpi::PhysicalPosition, window::Window};

// NOTE(ghovax): This could be serialized, but I need to find a way to serialize `Attrs`.
pub enum DocumentElement {
    Line {
        anchor_point: (f32, f32),
        spans: Vec<(String, Attributes)>,
    },
}

#[derive(Default, Debug)]
pub struct EditingCursor {
    line_index: usize,
    glyph_index_in_line: usize,
}

impl EditingCursor {
    pub fn from_mouse_position(
        physically_layouted_line_buffers: &[LineBuffer],
        mouse_position: &PhysicalPosition<f64>,
    ) -> Self {
        let mut selected_line_index = 0;
        let mut selected_glyph_index_in_line = 0;

        'outer_loop: for (line_index, line_buffer) in
            physically_layouted_line_buffers.iter().enumerate()
        {
            if line_buffer.layouted_line.is_none() {
                log::warn!("The `LineBuffer` at index {} is not layouted yet when trying to get the cursor position", line_index);
                continue;
            }

            let layouted_line = line_buffer.layouted_line.as_ref().unwrap();
            let line_height = layouted_line.maximum_y_reach + layouted_line.minimum_y_origin;

            let line_vertical_position = layouted_line
                .layouted_glyphs
                .first()
                .unwrap()
                .physical_y_offset
                .unwrap() as f64;

            if mouse_position.y <= line_vertical_position
                && mouse_position.y > line_vertical_position - line_height as f64
            {
                for (glyph_index, glyph) in layouted_line.layouted_glyphs.iter().enumerate() {
                    if glyph.contains_horizontal_position(mouse_position.x as f32) {
                        selected_line_index = line_index;
                        selected_glyph_index_in_line = glyph_index;
                        break 'outer_loop;
                    }
                }
            }
        }

        Self {
            line_index: selected_line_index,
            glyph_index_in_line: selected_glyph_index_in_line,
        }
    }
}

pub struct Document {
    pub elements: Vec<DocumentElement>,
    pub font_system: FontSystem,
    pub font_size: f32,
    pub swash_cache: SwashCache,
    pub physically_layouted_line_buffers: Option<Vec<LineBuffer>>,
    pub editing_cursor: EditingCursor,
}

impl Document {
    pub fn new(
        window: &Rc<Window>,
        document_elements: Vec<DocumentElement>,
        font_size: f32,
    ) -> Self {
        let display_scale = window.scale_factor() as f32;

        Self {
            elements: document_elements,
            font_system: FontSystem::new(),
            font_size: font_size * display_scale,
            swash_cache: SwashCache::new(),
            physically_layouted_line_buffers: None,
            editing_cursor: EditingCursor::default(),
        }
    }

    pub fn position_cursor(&mut self, mouse_position: &PhysicalPosition<f64>) {
        self.editing_cursor = EditingCursor::from_mouse_position(
            self.physically_layouted_line_buffers.as_ref().unwrap(),
            mouse_position,
        );
    }

    pub fn set_font_size(&mut self, font_size: f32, scale_factor: f32) {
        self.font_size = font_size * scale_factor;
    }

    pub fn draw_to_surface(
        &mut self,
        window: &Rc<winit::window::Window>,
        surface: &mut softbuffer::Surface<Rc<winit::window::Window>, Rc<winit::window::Window>>,
    ) {
        let (width, height) = {
            let size = window.inner_size();
            (size.width, size.height)
        };

        surface
            .resize(
                NonZeroU32::new(width).unwrap(),
                NonZeroU32::new(height).unwrap(),
            )
            .unwrap();

        let mut surface_buffer = surface.buffer_mut().unwrap();
        let surface_buffer_data = unsafe {
            std::slice::from_raw_parts_mut(
                surface_buffer.as_mut_ptr() as *mut u8,
                surface_buffer.len() * 4,
            )
        };
        let mut surface_pixel_map =
            PixmapMut::from_bytes(surface_buffer_data, width, height).unwrap();
        surface_pixel_map.fill(tiny_skia::Color::WHITE);

        let mut painting_options = Paint::default();
        let mut fill_paint_rectangle = |x, y, width, height, color: Color| {
            // NOTE(ghovax): Due to `softbuffer`` and `tiny_skia` having incompatible internal color
            // representations we swap the red and blue channels here
            painting_options.set_color_rgba8(color.b(), color.g(), color.r(), color.a());
            surface_pixel_map.fill_rect(
                tiny_skia::Rect::from_xywh(x as f32, y as f32, width as f32, height as f32)
                    .unwrap(),
                &painting_options,
                Transform::identity(),
                None,
            );
        };
        let mut layouted_line_buffers = Vec::new();

        for document_element in self.elements.iter() {
            match document_element {
                DocumentElement::Line {
                    anchor_point,
                    spans,
                } => {
                    let default_attributes = Attributes::new();
                    let mut line_buffer = LineBuffer::from_rich_text(spans, default_attributes);
                    let layouted_line =
                        line_buffer.as_mut_layouted_line(&mut self.font_system, self.font_size);

                    for glyph in layouted_line.layouted_glyphs.iter_mut() {
                        glyph.layout_physically(*anchor_point, 1.0);

                        let glyph_color = match glyph.color {
                            Some(color) => color,
                            None => Color::rgba(0, 0, 0, 255),
                        };

                        self.swash_cache.with_pixels(
                            &mut self.font_system,
                            glyph.cache_key.unwrap(),
                            glyph_color,
                            |x, y, color| {
                                fill_paint_rectangle(
                                    glyph.physical_x_offset.unwrap() + x,
                                    glyph.physical_y_offset.unwrap() + y,
                                    1,
                                    1,
                                    color,
                                );
                            },
                        );
                    }

                    layouted_line_buffers.push(line_buffer);
                }
            }
        }

        let calculate_cursor_position = || {
            for (line_index, line_buffer) in layouted_line_buffers.iter().enumerate() {
                if self.editing_cursor.line_index == line_index {
                    let layouted_line = line_buffer.layouted_line.as_ref().unwrap();

                    for (glyph_index, glyph) in layouted_line.layouted_glyphs.iter().enumerate() {
                        if self.editing_cursor.line_index == glyph.start_index {
                            return Some((glyph_index, 0.0));
                        } else if self.editing_cursor.line_index > glyph.start_index
                            && self.editing_cursor.line_index < glyph.end_index
                        {
                            // Guess the horizontal offset based on the characters
                            let mut before = 0;
                            let mut total = 0;

                            let cluster = &line_buffer.text[glyph.start_index..glyph.end_index];
                            for (i, _) in cluster.grapheme_indices(true) {
                                if glyph.start_index + i < self.editing_cursor.line_index {
                                    before += 1;
                                }
                                total += 1;
                            }

                            let offset = glyph.width * (before as f32) / (total as f32);
                            return Some((glyph_index, offset));
                        }
                    }
                    match layouted_line.layouted_glyphs.last() {
                        Some(glyph) => {
                            if self.editing_cursor.line_index == glyph.end_index {
                                return Some((layouted_line.layouted_glyphs.len(), 0.0));
                            }
                        }
                        None => {
                            return Some((0, 0.0));
                        }
                    }
                }
            }

            None
        };

        if let Some((cursor_glyph_index, cursor_glyph_horizontal_offset)) =
            calculate_cursor_position()
        {}

        // Draw the hitboxes of the glyphs after they've been laid out and the line boundaries
        for (line_buffer, document_element) in
            layouted_line_buffers.iter().zip(self.elements.iter())
        {
            let layouted_line = line_buffer.layouted_line.as_ref().unwrap();

            for glyph in layouted_line.layouted_glyphs.iter() {
                let overlay_rectangle = Rect::from_xywh(
                    glyph.physical_x_offset.unwrap() as f32,
                    glyph.physical_y_offset.unwrap() as f32 - glyph.y_origin * self.font_size,
                    glyph.width,
                    glyph.height,
                )
                .unwrap();

                let mut path = PathBuilder::new();
                let (x, y) = (overlay_rectangle.x(), overlay_rectangle.y());
                path.move_to(x, y);
                path.line_to(x + overlay_rectangle.width(), y);
                path.line_to(
                    x + overlay_rectangle.width(),
                    y - overlay_rectangle.height(),
                );
                path.line_to(x, y - overlay_rectangle.height());
                path.close();
                let path = path.finish().unwrap();

                let mut paint = Paint::default();
                paint.set_color_rgba8(0, 0, 255, 128);

                let mut stroke = Stroke::default();
                stroke.width = 2.0;

                surface_pixel_map.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            match document_element {
                DocumentElement::Line { anchor_point, .. } => {
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(255, 0, 0, 128);

                    let mut stroke = Stroke::default();
                    stroke.width = 2.0;

                    let x_origin = layouted_line
                        .layouted_glyphs
                        .first()
                        .unwrap()
                        .physical_x_offset
                        .unwrap() as f32;
                    let last_glyph = layouted_line.layouted_glyphs.last().unwrap();
                    let x_reach = last_glyph.physical_x_offset.unwrap() as f32 + last_glyph.width;

                    let mut line_top_path = PathBuilder::new();
                    line_top_path
                        .move_to(x_origin, -layouted_line.maximum_y_reach + anchor_point.1);
                    line_top_path.line_to(x_reach, -layouted_line.maximum_y_reach + anchor_point.1);
                    let line_top_path = line_top_path.finish().unwrap();

                    surface_pixel_map.stroke_path(
                        &line_top_path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        None,
                    );

                    let mut line_bottom_path = PathBuilder::new();
                    line_bottom_path
                        .move_to(x_origin, -layouted_line.minimum_y_origin + anchor_point.1);
                    line_bottom_path
                        .line_to(x_reach, -layouted_line.minimum_y_origin + anchor_point.1);
                    let line_bottom_path = line_bottom_path.finish().unwrap();

                    surface_pixel_map.stroke_path(
                        &line_bottom_path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }

        self.physically_layouted_line_buffers = Some(layouted_line_buffers);

        surface_buffer.present().unwrap();
    }
}
