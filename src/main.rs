use gtk4::DrawingArea;
use gtk4::{cairo, glib, prelude::*, Button, GestureClick, Orientation, ScrolledWindow};
use skia_safe::image::CachingHint;
use skia_safe::{Paint, Path, Rect, Surface};
use std::cell::RefCell;
use std::rc::Rc;
use text::attributes::Attributes;
use text::{font_system::FontSystem, line_buffer::LineBuffer, swash_cache::SwashCache};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::EnvFilter;

use unicode_segmentation::UnicodeSegmentation as _;

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
        mouse_position: (f64, f64),
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

            if mouse_position.1 <= line_vertical_position
                && mouse_position.1 > line_vertical_position - line_height as f64
            {
                for (glyph_index, glyph) in layouted_line.layouted_glyphs.iter().enumerate() {
                    if glyph.contains_horizontal_position(mouse_position.0 as f32) {
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

pub struct ProgramConfiguration {}

fn main() -> glib::ExitCode {
    // Initialize the logging handler
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let application = gtk4::Application::builder()
        .application_id("com.github.ghovax.editex")
        .build();

    application.connect_activate(|application| {
        let window = gtk4::ApplicationWindow::new(application);

        let (default_window_width, default_window_height) = (800, 600);
        window.set_default_size(default_window_width, default_window_height);
        let scale_factor = window.scale_factor();
        log::debug!("The scale factor is {}", scale_factor);

        let attributes = Attributes::new();
        // NOTE(ghovax): This could be loaded from an actual document.
        #[allow(clippy::useless_vec)]
        let document_elements = vec![
            DocumentElement::Line {
                anchor_point: (85.0, 190.0),
                spans: vec![
                    ("pop".to_string(), attributes),
                    ("old ".to_string(), attributes.italic()),
                    ("example text ßåß√Ï√ÅÏ".to_string(), attributes.bold()),
                ],
            },
            DocumentElement::Line {
                anchor_point: (885.0, 990.0),
                spans: vec![
                    ("pop".to_string(), attributes),
                    ("old ".to_string(), attributes.italic()),
                    ("example text ßåß√Ï√ÅÏ".to_string(), attributes.bold()),
                ],
            },
        ];
        // NOTE(ghovax): This could be loaded from a configuration file.
        let default_font_size = 32.0;
        let font_size = Rc::new(RefCell::new(default_font_size * scale_factor as f32));
        let mut font_system = FontSystem::new();
        let mut rasterizer_cache = SwashCache::new();
        let editing_cursor = Rc::new(RefCell::new(EditingCursor::default()));

        let layouted_lines: Rc<RefCell<Vec<LineBuffer>>> = Rc::new(RefCell::new(Vec::new()));

        let single_click_left_mouse_button_gesture = GestureClick::new();
        single_click_left_mouse_button_gesture
            .set_button(gtk4::gdk::ffi::GDK_BUTTON_PRIMARY as u32);

        {
            let layouted_lines = Rc::clone(&layouted_lines);
            let editing_cursor = Rc::clone(&editing_cursor);

            single_click_left_mouse_button_gesture.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk4::EventSequenceState::Claimed);
                let mouse_position = (x * scale_factor as f64, y * scale_factor as f64);
                log::trace!(
                    "The primary mouse button was pressed at {:?} in the drawing area",
                    mouse_position
                );

                let layouted_lines = layouted_lines.borrow();
                let mut editing_cursor = editing_cursor.borrow_mut();

                let editing_cursor_replacement =
                    EditingCursor::from_mouse_position(&layouted_lines, mouse_position);
                log::trace!(
                    "The editing cursor was replaced with {:?}",
                    editing_cursor_replacement
                );
                let _ = std::mem::replace(&mut *editing_cursor, editing_cursor_replacement);
            });
        }

        let vertical_box = gtk4::Box::new(Orientation::Vertical, 0);

        // Create a Box to act as a Toolbar
        let toolbar = gtk4::Box::new(Orientation::Horizontal, 5);
        toolbar.set_margin_top(5);
        toolbar.set_margin_bottom(5);
        toolbar.set_margin_start(5);
        toolbar.set_margin_end(5);

        let default_toolbar_height = 35;
        toolbar.set_height_request(default_toolbar_height);

        // Create toolbar buttons
        for (button_icon_path, button_action) in [
            (
                "add_40dp_FILL0_wght400_GRAD0_opsz40",
                Box::new(|| {
                    log::trace!("Pressed the add button");
                }) as Box<dyn Fn()>,
            ),
            (
                "add_link_40dp_FILL0_wght400_GRAD0_opsz40",
                Box::new(|| {
                    log::trace!("Pressed the add link button");
                }) as Box<dyn Fn()>,
            ),
        ] {
            let button = Button::builder().build();
            let button_icon = gtk4::Image::new();
            button_icon.set_from_file(Some(format!("src/{button_icon_path}.png").as_str()));
            button.set_child(Some(&button_icon));
            button.connect_clicked(move |_button| button_action());
            toolbar.append(&button);
        }

        vertical_box.append(&toolbar);

        let drawing_area = DrawingArea::new();
        drawing_area.set_content_width(1432); // TODO
        drawing_area.set_content_height(1412);
        drawing_area.add_controller(single_click_left_mouse_button_gesture);

        let scrolled_window = ScrolledWindow::new();
        scrolled_window.set_hexpand(true);
        scrolled_window.set_vexpand(true);
        scrolled_window.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
        scrolled_window.set_child(Some(&drawing_area));

        vertical_box.append(&scrolled_window);

        #[allow(deprecated)]
        let surface = Surface::new_raster_n32_premul((
            default_window_width * scale_factor,
            default_window_height * scale_factor,
        ))
        .unwrap();
        log::debug!(
            "The surface was initialized with a size of {:?}",
            (surface.as_ref().width(), surface.as_ref().height())
        );
        let surface_reference = Rc::new(RefCell::new(surface));

        {
            let layouted_lines = Rc::clone(&layouted_lines);
            let font_size = Rc::clone(&font_size);

            drawing_area.set_draw_func(move |_widget, cairo_context, width, height| {
                let mut surface = surface_reference.borrow_mut();

                if surface.as_ref().width() != width * scale_factor
                    || surface.as_ref().height() != height * scale_factor
                {
                    #[allow(deprecated)]
                    let surface_replacement = Surface::new_raster_n32_premul((
                        width * scale_factor,
                        height * scale_factor,
                    ))
                    .unwrap();
                    let _ = std::mem::replace(&mut *surface, surface_replacement);
                    log::trace!("The surface was resized to {:?}", (width, height));
                }

                // Do all the drawing operations
                let canvas = surface.canvas();
                canvas.clear(skia_safe::Color::WHITE);

                let mut painting_options = Paint::default();
                let mut draw_filled_rectangle = |x, y, width, height, color: text::color::Color| {
                    painting_options.set_color(skia_safe::Color::from_argb(
                        color.a(),
                        color.r(),
                        color.g(),
                        color.b(),
                    ));
                    canvas.draw_rect(
                        Rect::from_xywh(x as f32, y as f32, width as f32, height as f32),
                        &painting_options,
                    );
                };
                let mut layouted_line_buffers = Vec::new();
                let font_size = font_size.borrow();

                for document_element in document_elements.iter() {
                    match document_element {
                        DocumentElement::Line {
                            anchor_point,
                            spans,
                        } => {
                            let default_attributes = Attributes::new();
                            let mut line_buffer =
                                LineBuffer::from_rich_text(spans, default_attributes);
                            let layouted_line =
                                line_buffer.as_mut_layouted_line(&mut font_system, *font_size);

                            for glyph in layouted_line.layouted_glyphs.iter_mut() {
                                glyph.layout_physically(*anchor_point, 1.0);

                                let glyph_color = match glyph.color {
                                    Some(color) => color,
                                    None => text::color::Color::rgba(0, 0, 0, 255),
                                };

                                rasterizer_cache.with_pixels(
                                    &mut font_system,
                                    glyph.cache_key.unwrap(),
                                    glyph_color,
                                    |x, y, color| {
                                        draw_filled_rectangle(
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

                let editing_cursor = editing_cursor.borrow();
                let calculate_cursor_position = || {
                    for (line_index, line_buffer) in layouted_line_buffers.iter().enumerate() {
                        if editing_cursor.line_index == line_index {
                            let layouted_line = line_buffer.layouted_line.as_ref().unwrap();

                            for (glyph_index, glyph) in
                                layouted_line.layouted_glyphs.iter().enumerate()
                            {
                                if editing_cursor.line_index == glyph.start_index {
                                    return Some((glyph_index, 0.0));
                                } else if editing_cursor.line_index > glyph.start_index
                                    && editing_cursor.line_index < glyph.end_index
                                {
                                    // Guess the horizontal offset based on the characters
                                    let mut before = 0;
                                    let mut total = 0;

                                    let cluster =
                                        &line_buffer.text[glyph.start_index..glyph.end_index];
                                    for (i, _) in cluster.grapheme_indices(true) {
                                        if glyph.start_index + i < editing_cursor.line_index {
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
                                    if editing_cursor.line_index == glyph.end_index {
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
                    layouted_line_buffers.iter().zip(document_elements.iter())
                {
                    let layouted_line = line_buffer.layouted_line.as_ref().unwrap();

                    for glyph in layouted_line.layouted_glyphs.iter() {
                        let overlay_rectangle = Rect::from_xywh(
                            glyph.physical_x_offset.unwrap() as f32,
                            glyph.physical_y_offset.unwrap() as f32 - glyph.y_origin * *font_size,
                            glyph.width,
                            glyph.height,
                        );

                        let mut glyph_outline_path = Path::new();
                        let (x, y) = (overlay_rectangle.x(), overlay_rectangle.y());
                        glyph_outline_path.move_to((x, y));
                        glyph_outline_path.line_to((x + overlay_rectangle.width(), y));
                        glyph_outline_path.line_to((
                            x + overlay_rectangle.width(),
                            y - overlay_rectangle.height(),
                        ));
                        glyph_outline_path.line_to((x, y - overlay_rectangle.height()));
                        glyph_outline_path.close();

                        let mut painting_options = Paint::default();
                        painting_options.set_color(skia_safe::Color::from_argb(128, 0, 0, 255));
                        painting_options.set_stroke_width(1.0);
                        painting_options.set_stroke(true);

                        canvas.draw_path(&glyph_outline_path, &painting_options);
                    }

                    match document_element {
                        DocumentElement::Line { anchor_point, .. } => {
                            let mut painting_options = Paint::default();
                            painting_options.set_color(skia_safe::Color::from_argb(128, 255, 0, 0));
                            painting_options.set_stroke_width(1.0);
                            painting_options.set_stroke(true);

                            let x_origin = layouted_line
                                .layouted_glyphs
                                .first()
                                .unwrap()
                                .physical_x_offset
                                .unwrap() as f32;
                            let last_glyph = layouted_line.layouted_glyphs.last().unwrap();
                            let x_reach =
                                last_glyph.physical_x_offset.unwrap() as f32 + last_glyph.width;

                            let mut line_top_path = Path::new();
                            line_top_path.move_to((
                                x_origin,
                                anchor_point.1 - layouted_line.maximum_y_reach,
                            ));
                            line_top_path
                                .line_to((x_reach, anchor_point.1 - layouted_line.maximum_y_reach));

                            canvas.draw_path(&line_top_path, &painting_options);

                            let mut line_bottom_path = Path::new();
                            line_bottom_path.move_to((
                                x_origin,
                                anchor_point.1 - layouted_line.minimum_y_origin,
                            ));
                            line_bottom_path.line_to((
                                x_reach,
                                anchor_point.1 - layouted_line.minimum_y_origin,
                            ));

                            canvas.draw_path(&line_bottom_path, &painting_options);
                        }
                    }
                }

                let mut layouted_lines = layouted_lines.borrow_mut();
                let _ = std::mem::replace(&mut *layouted_lines, layouted_line_buffers);

                // Copy Skia surface to Cairo context
                let image_snapshot = surface.image_snapshot();
                let image_info = image_snapshot.image_info();
                let mut pixmap = vec![0; (image_info.width() * image_info.height() * 4) as usize];
                image_snapshot.read_pixels(
                    image_info,
                    pixmap.as_mut_slice(),
                    image_info.min_row_bytes(),
                    (0, 0),
                    CachingHint::Allow,
                );

                let surface = cairo::ImageSurface::create_for_data(
                    pixmap,
                    cairo::Format::ARgb32,
                    image_info.width(),
                    image_info.height(),
                    image_info.min_row_bytes() as i32,
                )
                .unwrap();

                cairo_context.scale(1.0 / scale_factor as f64, 1.0 / scale_factor as f64);
                cairo_context
                    .set_source_surface(&surface, 0.0, 0.0)
                    .unwrap();
                cairo_context.paint().unwrap();
            });
        }

        // Add the DrawingArea to the ScrolledWindow
        scrolled_window.set_child(Some(&drawing_area));

        // Add the ScrolledWindow to the main window
        window.set_child(Some(&vertical_box));

        window.present();
    });

    application.run()
}
