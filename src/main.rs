use std::{num::NonZeroU32, rc::Rc};

use cosmic_text::{Attrs, Buffer, Cursor, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap};
use tiny_skia::{Paint, PixmapMut, Transform};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter};
use unicode_segmentation::UnicodeSegmentation;
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    keyboard::Key,
    window::WindowBuilder,
};

#[derive(Debug, Clone)]
enum UserEvent {
    RequestRedraw,
    InsertText(String),
}

// NOTE(ghovax): This could be serialized, but I need to find a way to serialize `Attrs`.
enum DocumentElement {
    Line {
        anchor_point: PhysicalPosition<f32>,
        spans: Vec<(String, Attrs<'static>)>,
    },
}

fn main() {
    // Initialize the logging handler
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    // Create the window and graphics context to draw to
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build().unwrap();
    let event_loop_proxy = event_loop.create_proxy();
    let window = Rc::new(WindowBuilder::new().build(&event_loop).unwrap());
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    // Get the font handling loaded-up
    let mut font_system = FontSystem::new();
    let mut swash_cache = SwashCache::new();

    // NOTE(ghovax): These parameters could be configured.
    let mut display_scale = window.scale_factor() as f32;
    let metrics = Metrics::new(32.0, 32.0);

    // NOTE(ghovax): This could be loaded from an actual document.
    let attributes = Attrs::new().family(Family::Name("CMU Serif")).weight(Weight::MEDIUM);
    let mut document = vec![
        DocumentElement::Line {
            anchor_point: PhysicalPosition::new(85.0, 120.0),
            spans: vec![
                ("B".to_string(), attributes),
                ("old ".to_string(), attributes.style(Style::Italic)),
                ("example text".to_string(), attributes.weight(Weight::BOLD)),
            ],
        },
        DocumentElement::Line {
            anchor_point: PhysicalPosition::new(35.0, 180.0),
            spans: vec![
                ("B".to_string(), attributes),
                ("old ".to_string(), attributes.style(Style::Italic)),
                ("example text".to_string(), attributes.weight(Weight::BOLD)),
            ],
        },
    ];

    // Create all the line buffers from the respective document elements
    // NOTE(ghovax): This is not generalizeable to actual full documents with mixed content.
    let mut line_buffers = Vec::new();
    for document_element in document.iter() {
        match document_element {
            DocumentElement::Line { spans, .. } => {
                let mut line_buffer = Buffer::new_empty(metrics.scale(display_scale));
                line_buffer.set_size(
                    &mut font_system,
                    window.inner_size().width as f32,
                    window.inner_size().height as f32,
                );
                line_buffer.set_rich_text(
                    &mut font_system,
                    spans.iter().map(|span| (span.0.as_str(), span.1)),
                    attributes,
                    Shaping::Advanced,
                );
                line_buffers.push(line_buffer);
            }
        }
    }

    // TODO(ghovax): Figure out how to position the cursor correctly at each line
    let mut cursor = Cursor::default();
    let mut cursor_line_index = 0;
    let cursor_color = cosmic_text::Color::rgba(0, 0, 0, 255);

    let mut mouse_position = PhysicalPosition::new(0.0, 0.0);
    let mut mouse_left_button_state = ElementState::Released;

    event_loop
        .run(|event, event_loop_window_target| {
            event_loop_window_target.set_control_flow(ControlFlow::Wait);

            #[allow(clippy::single_match, clippy::collapsible_match)]
            match event {
                Event::UserEvent(user_event) => match user_event {
                    UserEvent::RequestRedraw => {
                        let (width, height) = {
                            let size = window.inner_size();
                            (size.width, size.height)
                        };

                        surface
                            .resize(NonZeroU32::new(width).unwrap(), NonZeroU32::new(height).unwrap())
                            .unwrap();

                        let mut surface_buffer = surface.buffer_mut().unwrap();
                        let surface_buffer_data = unsafe {
                            std::slice::from_raw_parts_mut(
                                surface_buffer.as_mut_ptr() as *mut u8,
                                surface_buffer.len() * 4,
                            )
                        };
                        let mut surface_pixel_map = PixmapMut::from_bytes(surface_buffer_data, width, height).unwrap();
                        surface_pixel_map.fill(tiny_skia::Color::WHITE);

                        // TODO: For each line buffer, should I set `line_buffer.set_size(&mut font_system, width as f32, height as f32)`?

                        let mut painting_options = Paint::default();
                        let mut paint_rectangle = |x, y, width, height, color: cosmic_text::Color| {
                            // NOTE: Due to `softbuffer`` and `tiny_skia` having incompatible internal color representations we swap
                            // the red and blue channels here
                            painting_options.set_color_rgba8(color.b(), color.g(), color.r(), color.a());
                            surface_pixel_map.fill_rect(
                                tiny_skia::Rect::from_xywh(x as f32, y as f32, width as f32, height as f32).unwrap(),
                                &painting_options,
                                Transform::identity(),
                                None,
                            );
                        };

                        for (line_buffer, document_element) in line_buffers.iter_mut().zip(document.iter()) {
                            let anchor_point = match document_element {
                                DocumentElement::Line { anchor_point, .. } => anchor_point.clone(),
                            };
                            line_buffer.set_wrap(&mut font_system, Wrap::None);
                            line_buffer.line_layout(&mut font_system, 0).unwrap();

                            let line_height = line_buffer.metrics().line_height;
                            for layout_run in line_buffer.layout_runs() {
                                let cursor_glyph_position = |cursor: &Cursor| -> Option<(usize, f32)> {
                                    let line_index = layout_run.line_i;

                                    if cursor.line == line_index {
                                        for (glyph_index, glyph) in layout_run.glyphs.iter().enumerate() {
                                            if cursor.index == glyph.start {
                                                return Some((glyph_index, 0.0));
                                            } else if cursor.index > glyph.start && cursor.index < glyph.end {
                                                // Guess the horizontal offset based on the characters
                                                let mut before = 0;
                                                let mut total = 0;

                                                let cluster = &layout_run.text[glyph.start..glyph.end];
                                                for (i, _) in cluster.grapheme_indices(true) {
                                                    if glyph.start + i < cursor.index {
                                                        before += 1;
                                                    }
                                                    total += 1;
                                                }

                                                let offset = glyph.w * (before as f32) / (total as f32);
                                                return Some((glyph_index, offset));
                                            }
                                        }
                                        match layout_run.glyphs.last() {
                                            Some(glyph) => {
                                                if cursor.index == glyph.end {
                                                    return Some((layout_run.glyphs.len(), 0.0));
                                                }
                                            }
                                            None => {
                                                return Some((0, 0.0));
                                            }
                                        }
                                    }

                                    None
                                };

                                // Draw the cursor
                                if let Some((cursor_glyph_index, cursor_glyph_offset)) = cursor_glyph_position(&cursor)
                                {
                                    let x = match layout_run.glyphs.get(cursor_glyph_index) {
                                        Some(glyph) => {
                                            // Start of detected glyph
                                            if glyph.level.is_rtl() {
                                                (glyph.x + glyph.w - cursor_glyph_offset) as i32
                                            } else {
                                                (glyph.x + cursor_glyph_offset) as i32
                                            }
                                        }
                                        None => match layout_run.glyphs.last() {
                                            Some(glyph) => {
                                                // End of last glyph
                                                if glyph.level.is_rtl() {
                                                    glyph.x as i32
                                                } else {
                                                    (glyph.x + glyph.w) as i32
                                                }
                                            }
                                            None => {
                                                // Start of empty line
                                                0
                                            }
                                        },
                                    };

                                    let cursor_width = match layout_run.glyphs.get(cursor_glyph_index) {
                                        Some(glyph) => {
                                            // Start of detected glyph
                                            if glyph.level.is_rtl() {
                                                (glyph.w - cursor_glyph_offset) as i32
                                            } else {
                                                glyph.w as i32
                                            }
                                        }
                                        None => match layout_run.glyphs.last() {
                                            Some(glyph) => {
                                                // End of last glyph
                                                if glyph.level.is_rtl() {
                                                    0
                                                } else {
                                                    glyph.w as i32
                                                }
                                            }
                                            None => {
                                                // Start of empty line
                                                0
                                            }
                                        },
                                    };
                                    paint_rectangle(
                                        x + anchor_point.x as i32,
                                        (layout_run.line_top + anchor_point.y) as i32,
                                        2,
                                        line_height as u32,
                                        cursor_color,
                                    );
                                }

                                for glyph in layout_run.glyphs.iter() {
                                    let physical_glyph = glyph.physical((anchor_point.x, anchor_point.y), 1.0);

                                    let glyph_color = match glyph.color_opt {
                                        Some(color) => color,
                                        None => cosmic_text::Color::rgba(0, 0, 0, 255),
                                    };

                                    swash_cache.with_pixels(
                                        &mut font_system,
                                        physical_glyph.cache_key,
                                        glyph_color,
                                        |x, y, color| {
                                            paint_rectangle(
                                                physical_glyph.x + x,
                                                layout_run.line_y as i32 + physical_glyph.y + y,
                                                1,
                                                1,
                                                color,
                                            );
                                        },
                                    );
                                }
                            }
                        }

                        surface_buffer.present().unwrap();
                    }
                    UserEvent::InsertText(text) => {
                        for character in text.chars() {
                            if character.is_control() && !['\t', '\n', '\r', '\u{92}'].contains(&character) {
                                // Filter out special chars (except for tab)
                                log::debug!("Refusing to insert control character {:?}", character);
                            } else if ['\n', '\r'].contains(&character) {
                                log::debug!("Received enter input, still have to implement the functionality");
                            } else {
                                let cursor_line_buffer = line_buffers.get_mut(cursor_line_index).unwrap();
                                let cursor_line_spans = match document.get_mut(cursor_line_index).unwrap() {
                                    DocumentElement::Line { spans, .. } => spans,
                                };

                                // Find for which elements of the span the cursor index belongs to
                                // For example, in the span `[("B", attributes), ("old ", attributes.style(Style::Italic))]`,
                                // the cursor index 1 would belong to the span `[("old ", attributes.style(Style::Italic))]`, as would the indices 2, 3 and 4
                                // and the cursor index 0 would belong to the span `[("B", attributes)]`.
                                let mut cursor_span_index = None;
                                let mut index_in_span = None;
                                {
                                    let mut total_characters_counter = 0;
                                    'outer: for (span_index, span) in cursor_line_spans.iter().enumerate() {
                                        for (character_index, _character) in span.0.chars().enumerate() {
                                            if total_characters_counter == cursor.index {
                                                cursor_span_index = Some(span_index);
                                                index_in_span = Some(character_index);
                                                break 'outer;
                                            }
                                            total_characters_counter += 1;
                                        }
                                    }
                                }

                                let cursor_span_index = cursor_span_index.unwrap_or(cursor_line_spans.len() - 1); // TODO
                                let index_in_span =
                                    index_in_span.unwrap_or(cursor_line_spans.get(cursor_span_index).unwrap().0.len());

                                cursor_line_spans
                                    .get_mut(cursor_span_index)
                                    .unwrap()
                                    .0
                                    .insert(index_in_span, character);
                                cursor_line_buffer.set_rich_text(
                                    &mut font_system,
                                    cursor_line_spans.iter().map(|span| (span.0.as_str(), span.1)),
                                    attributes,
                                    Shaping::Advanced,
                                );
                                cursor.index += 1;
                            }
                        }
                    }
                },
                Event::WindowEvent { window_id, event } => match event {
                    WindowEvent::CloseRequested => {
                        event_loop_window_target.exit();
                    }
                    WindowEvent::KeyboardInput {
                        event: winit::event::KeyEvent { logical_key, state, .. },
                        ..
                    } => {
                        if state.is_pressed() {
                            match logical_key {
                                Key::Named(key) => {
                                    if let Some(text) = key.to_text() {
                                        event_loop_proxy
                                            .send_event(UserEvent::InsertText(text.to_string()))
                                            .unwrap();
                                        event_loop_proxy.send_event(UserEvent::RequestRedraw).unwrap();
                                    }
                                }
                                Key::Character(text) => {
                                    event_loop_proxy
                                        .send_event(UserEvent::InsertText(text.to_string()))
                                        .unwrap();
                                    event_loop_proxy.send_event(UserEvent::RequestRedraw).unwrap();
                                }
                                _ => {}
                            }
                        }
                    }
                    WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        log::info!("Updated scale factor for the window with ID {window_id:?}");

                        display_scale = scale_factor as f32;
                        for line_buffer in line_buffers.iter_mut() {
                            line_buffer.set_metrics(&mut font_system, metrics.scale(display_scale));
                        }

                        event_loop_proxy.send_event(UserEvent::RequestRedraw).unwrap();
                    }
                    WindowEvent::Resized(size) => {
                        event_loop_proxy.send_event(UserEvent::RequestRedraw).unwrap();
                    }
                    WindowEvent::CursorMoved { device_id: _, position } => {
                        mouse_position = position;
                    }
                    WindowEvent::MouseInput {
                        device_id: _,
                        state,
                        button,
                    } => {
                        if button == MouseButton::Left {
                            if state == ElementState::Pressed && mouse_left_button_state == ElementState::Released {
                                for ((line_index, line_buffer), document_element) in
                                    line_buffers.iter().enumerate().zip(document.iter())
                                {
                                    let anchor_point = match document_element {
                                        DocumentElement::Line { anchor_point, .. } => anchor_point,
                                    };
                                    if let Some(updated_cursor) = line_buffer.hit(
                                        mouse_position.x as f32 - anchor_point.x,
                                        mouse_position.y as f32 - anchor_point.y,
                                    ) {
                                        if updated_cursor != cursor {
                                            cursor = updated_cursor;
                                            cursor_line_index = line_index;

                                            event_loop_proxy.send_event(UserEvent::RequestRedraw).unwrap();
                                            break;
                                        }
                                    }
                                }
                            }

                            mouse_left_button_state = state;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        })
        .unwrap();
}
