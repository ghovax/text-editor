use gtk4::gdk::Key;
use gtk4::gio::{Cancellable, MemoryInputStream};
use gtk4::{cairo, glib, prelude::*, Button, EventControllerKey, GestureClick, Orientation, ScrolledWindow};
use gtk4::{DrawingArea, FileChooserAction, FileChooserDialog, ResponseType};
use serde::{Deserialize, Serialize};
use skia_safe::image::CachingHint;
use skia_safe::{Paint, Path, Rect, Surface};
use std::borrow::Borrow;
use std::cell::RefCell;
use std::fs::File;
use std::io::Read as _;
use std::rc::Rc;
use text::attributes::{Attributes, Style, Weight};
use text::{font_system::FontSystem, line_buffer::LineBuffer, swash_cache::SwashCache};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::EnvFilter;

use unicode_segmentation::UnicodeSegmentation as _;

// NOTE(ghovax): This could be serialized, but I need to find a way to serialize `Attributes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DocumentElement {
    #[serde(rename_all = "camelCase")]
    Page {
        size: (f32, f32),
        contents: Vec<DocumentElement>,
    },
    #[serde(rename_all = "camelCase", untagged)]
    Line {
        anchor_point: (f32, f32),
        spans: Vec<(String, Attributes)>,
    },
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditingCursor {
    pub line_index: usize,
    pub glyph_index_in_line: usize,
}

impl EditingCursor {
    pub fn from_mouse_position(physically_layouted_line_buffers: &[LineBuffer], mouse_position: (f64, f64)) -> Self {
        let mut selected_line_index = 0;
        let mut selected_glyph_index_in_line = 0;

        'outer_loop: for (line_index, line_buffer) in physically_layouted_line_buffers.iter().enumerate() {
            if line_buffer.layouted_line.is_none() {
                log::warn!(
                    "The `LineBuffer` at index {} is not layouted yet when trying to get the cursor position",
                    line_index
                );
                continue;
            }

            let layouted_line = line_buffer.layouted_line.as_ref().unwrap();

            // Skip empty lines (no glyphs)
            if layouted_line.layouted_glyphs.is_empty() {
                continue;
            }

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

fn main() -> glib::ExitCode {
    // Initialize the logging handler
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let application = gtk4::Application::builder()
        .application_id("com.github.ghovax.editex")
        .build();

    application.set_flags(gtk4::gio::ApplicationFlags::HANDLES_COMMAND_LINE);

    application.connect_command_line(|app, cmd_line| {
        let args = cmd_line.arguments();
        let file_path = if args.len() > 1 {
            // Convert OsString to PathBuf, skip first arg (program name)
            Some(std::path::PathBuf::from(&args[1]))
        } else {
            None
        };

        run_application_logic(app, file_path);
        0 // Return 0 for success
    });

    application.run()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProgramConfiguration {
    // Window configurations
    pub window_width: i32,
    pub window_height: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    // Layouting configurations
    pub top_margin: f32,
    pub bottom_margin: f32,
    pub left_margin: f32,
    pub right_margin: f32,
    // The actual contents of the document
    pub elements: Vec<DocumentElement>,
    // Settings about lines and text
    pub font_size: f32,
}

fn run_application_logic(application: &gtk4::Application, initial_file_path: Option<std::path::PathBuf>) {
    let window = Rc::new(gtk4::ApplicationWindow::new(application));

    // Loads the configuration file from `$HOME/.editex/config.json`, and if it doesn't exist
    // it creates a default one with hard-coded parameters.
    let configuration_file_path = std::env::var("HOME").unwrap() + "/.editex/config.json";
    let configuration: ProgramConfiguration = match std::fs::read_to_string(configuration_file_path.clone()) {
        Ok(configuration_file_content) => serde_json::from_str(&configuration_file_content).unwrap(),
        Err(_) => {
            log::info!("No configuration file found, creating a default one");
            // Make the directory
            if let Err(error) = std::fs::create_dir_all(std::env::var("HOME").unwrap() + "/.editex") {
                log::error!("Error creating the configuration directory: {}", error);
                return;
            }
            let default_configuration = ProgramConfiguration {
                window_width: 800,
                window_height: 600,
            };
            std::fs::write(
                configuration_file_path,
                serde_json::to_string_pretty(&default_configuration).unwrap(),
            )
            .unwrap();
            default_configuration
        }
    };

    let scale_factor = window.scale_factor();
    log::debug!("The scale factor for the current display is {}", scale_factor);

    let mut font_system = FontSystem::new();
    let mut rasterizer_cache = SwashCache::new();

    let document = Rc::new(RefCell::new(None::<Document>));
    let font_size = Rc::new(RefCell::new(0.0));
    let editing_cursor = Rc::new(RefCell::new(EditingCursor::default()));
    let layouted_lines = Rc::new(RefCell::new(Vec::new()));

    // Initialize the GUI components from the creation of a vertical stack
    let vertical_box = gtk4::Box::new(Orientation::Vertical, 0);

    // Create a Box to act as a Toolbar
    let toolbar = gtk4::Box::new(Orientation::Horizontal, 5);
    toolbar.set_height_request(35);
    toolbar.set_margin_top(5);
    toolbar.set_margin_bottom(5);
    toolbar.set_margin_start(5);
    toolbar.set_margin_end(5);

    // Create toolbar buttons
    let button = Button::builder().build();
    button.set_can_focus(false); // Prevent keyboard activation (Space/Enter)
    let button_icon = gtk4::Image::new();
    let image_bytes = include_bytes!("add_link_40dp_FILL0_wght400_GRAD0_opsz40.png").to_vec();
    let image_bytes_stream = MemoryInputStream::from_bytes(&glib::Bytes::from_owned(image_bytes));
    let pixel_buffer = gtk4::gdk_pixbuf::Pixbuf::from_stream(&image_bytes_stream, None::<&Cancellable>).unwrap();
    button_icon.set_from_pixbuf(Some(&pixel_buffer));
    button.set_child(Some(&button_icon));

    let drawing_area = Rc::new(DrawingArea::new()); // Just `Rc` because it has interior mutability

    {
        let window = Rc::clone(&window);
        let document = Rc::clone(&document);
        let font_size = Rc::clone(&font_size);
        let drawing_area = Rc::clone(&drawing_area);

        button.connect_clicked(move |_button| {
            log::trace!("Pressed the button to open a document");
            let dialog = Rc::new(FileChooserDialog::new(
                Some("Open document"),
                Some(window.as_ref()),
                FileChooserAction::Open,
                &[],
            ));

            // Create a horizontal box to hold the buttons with spacing
            let button_box = gtk4::Box::new(Orientation::Horizontal, 5);

            // Create custom Cancel button
            let cancel_button = Button::with_label("Cancel");
            cancel_button.set_can_focus(false); // Prevent keyboard activation
            {
                let dialog = Rc::clone(&dialog);

                cancel_button.connect_clicked(move |_| {
                    dialog.response(ResponseType::Cancel);
                });
            }

            // Create custom Open button
            let open_button = Button::with_label("Open file");
            open_button.set_can_focus(false); // Prevent keyboard activation
            {
                let dialog = Rc::clone(&dialog);
                open_button.connect_clicked(move |_| {
                    dialog.response(ResponseType::Accept);
                });
            }

            // Add buttons to the box
            button_box.append(&cancel_button);
            button_box.append(&open_button);

            // Add the custom button box to the dialog
            dialog.add_action_widget(&button_box, ResponseType::None);

            {
                let document = Rc::clone(&document);
                let font_size = Rc::clone(&font_size);
                let drawing_area = Rc::clone(&drawing_area);

                dialog.connect_response(move |dialog, response| {
                    if response == ResponseType::Accept {
                        if let Some(file) = dialog.file() {
                            if let Some(path) = file.path() {
                                let mut file_content = String::new();
                                if let Ok(mut file) = File::open(path) {
                                    file.read_to_string(&mut file_content).unwrap();

                                    let document_replacement: Document = serde_json::from_str(&file_content).unwrap();
                                    let font_size_replacement = document_replacement.font_size * scale_factor as f32;
                                    let mut document = document.borrow_mut();
                                    let _ = std::mem::replace(&mut *document, Some(document_replacement));
                                    log::info!("The document has been updated");

                                    let mut font_size = font_size.borrow_mut();
                                    let _ = std::mem::replace(&mut *font_size, font_size_replacement);

                                    drawing_area.queue_draw();
                                    // Give focus to the drawing area so it can receive keyboard input
                                    drawing_area.grab_focus();
                                }
                            }
                        }
                    }
                    dialog.close();
                });
            }

            dialog.show();
        });
    }

    toolbar.append(&button);

    vertical_box.append(&toolbar);

    let single_click_left_mouse_button_gesture = GestureClick::new();
    single_click_left_mouse_button_gesture.set_button(gtk4::gdk::ffi::GDK_BUTTON_PRIMARY as u32);

    {
        let layouted_lines = Rc::clone(&layouted_lines);
        let drawing_area = Rc::clone(&drawing_area);
        let editing_cursor = Rc::clone(&editing_cursor);

        single_click_left_mouse_button_gesture.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let mouse_position = (x * scale_factor as f64, y * scale_factor as f64);
            log::trace!(
                "The primary mouse button was pressed at {:?} in the drawing area",
                mouse_position
            );

            let layouted_lines = layouted_lines.borrow_mut();
            let mut editing_cursor = editing_cursor.borrow_mut();

            let editing_cursor_replacement = EditingCursor::from_mouse_position(&layouted_lines, mouse_position);
            log::trace!("The editing cursor was replaced with: {:?}", editing_cursor_replacement);
            let _ = std::mem::replace(&mut *editing_cursor, editing_cursor_replacement);

            drawing_area.queue_draw();
            log::trace!("The drawing area was queued to be redrawn");
        });
    }

    drawing_area.add_controller(single_click_left_mouse_button_gesture);

    let scrolled_window = ScrolledWindow::new();
    scrolled_window.set_hexpand(true); // TODO
    scrolled_window.set_vexpand(true);
    scrolled_window.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scrolled_window.set_child(Some(drawing_area.as_ref()));
    scrolled_window.set_size_request(configuration.window_width, configuration.window_height);

    vertical_box.append(&scrolled_window);

    #[allow(deprecated)]
    let surface = Surface::new_raster_n32_premul((
        configuration.window_width * scale_factor,
        configuration.window_height * scale_factor,
    ))
    .unwrap();
    log::debug!(
        "The surface was initialized with a size of {:?}",
        (surface.as_ref().width(), surface.as_ref().height())
    );
    let surface = Rc::new(RefCell::new(surface));

    // The setup of the drawing function
    {
        let layouted_lines = Rc::clone(&layouted_lines);
        let font_size = Rc::clone(&font_size);
        let editing_cursor = Rc::clone(&editing_cursor);
        let document = Rc::clone(&document);

        drawing_area.set_draw_func(move |_drawing_area, cairo_context, width, height| {
            let document = document.borrow_mut();
            if document.is_none() {
                log::trace!("The drawing area is being redrawn but there is no document");
                return;
            }
            let document = document.as_ref().unwrap();

            log::trace!("The drawing area is being redrawn");
            let mut surface = surface.borrow_mut();

            if surface.as_ref().width() != width * scale_factor || surface.as_ref().height() != height * scale_factor {
                #[allow(deprecated)]
                let surface_replacement =
                    Surface::new_raster_n32_premul((width * scale_factor, height * scale_factor)).unwrap();
                let _ = std::mem::replace(&mut *surface, surface_replacement);
                log::trace!("The surface was resized to {:?}", (width, height));
            }

            // Do all the drawing operations
            let canvas = surface.canvas();
            canvas.clear(skia_safe::Color::WHITE);

            let mut painting_options = Paint::default();
            let mut draw_filled_rectangle = |x, y, width, height, color: text::color::Color| {
                painting_options.set_color(skia_safe::Color::from_argb(color.a(), color.r(), color.g(), color.b()));
                canvas.draw_rect(
                    Rect::from_xywh(x as f32, y as f32, width as f32, height as f32),
                    &painting_options,
                );
            };
            let mut layouted_line_buffers = Vec::new();
            let font_size = font_size.borrow_mut();

            for document_element in document.elements.iter() {
                match document_element {
                    DocumentElement::Line { anchor_point, spans } => {
                        let default_attributes = Attributes::new();

                        let mut line_buffer = LineBuffer::from_rich_text(spans, default_attributes);
                        let layouted_line = line_buffer.as_mut_layouted_line(&mut font_system, *font_size);

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
                    DocumentElement::Page { size, contents } => {
                        // TODO
                    }
                }
            }

            // Draw the hitboxes of the glyphs after they've been laid out and the line boundaries
            for (line_buffer, document_element) in layouted_line_buffers.iter().zip(document.elements.iter()) {
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
                    glyph_outline_path.line_to((x + overlay_rectangle.width(), y - overlay_rectangle.height()));
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
                        // Only draw line bounds if there are glyphs (skip empty lines)
                        if !layouted_line.layouted_glyphs.is_empty() {
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
                            let x_reach = last_glyph.physical_x_offset.unwrap() as f32 + last_glyph.width;

                            let mut line_top_path = Path::new();
                            line_top_path.move_to((x_origin, anchor_point.1 - layouted_line.maximum_y_reach));
                            line_top_path.line_to((x_reach, anchor_point.1 - layouted_line.maximum_y_reach));

                            canvas.draw_path(&line_top_path, &painting_options);

                            let mut line_bottom_path = Path::new();
                            line_bottom_path.move_to((x_origin, anchor_point.1 - layouted_line.minimum_y_origin));
                            line_bottom_path.line_to((x_reach, anchor_point.1 - layouted_line.minimum_y_origin));

                            canvas.draw_path(&line_bottom_path, &painting_options);
                        }
                    }
                    DocumentElement::Page { size, contents } => {
                        // TODO
                    }
                }
            }

            let editing_cursor = editing_cursor.borrow_mut();

            for ((line_index, line_buffer), document_element) in
                layouted_line_buffers.iter().enumerate().zip(document.elements.iter())
            {
                match document_element {
                    DocumentElement::Line { anchor_point, .. } => {
                        let layouted_line = line_buffer.layouted_line.as_ref().unwrap();

                        // Calculate the cursor position from the cursor
                        let retrieve_cursor_position = || {
                            if editing_cursor.line_index == line_index {
                                for (glyph_index, glyph) in layouted_line.layouted_glyphs.iter().enumerate() {
                                    if editing_cursor.glyph_index_in_line == glyph.start_index {
                                        return Some((glyph_index, 0.0));
                                    } else if editing_cursor.glyph_index_in_line > glyph.start_index
                                        && editing_cursor.glyph_index_in_line < glyph.end_index
                                    {
                                        // Guess the horizontal offset based on the characters
                                        let mut before_glyphs_count = 0;
                                        let mut total_glyphs = 0;

                                        let cluster = &line_buffer.text[glyph.start_index..glyph.end_index];
                                        for (grapheme_index, _) in cluster.grapheme_indices(true) {
                                            if glyph.start_index + grapheme_index < editing_cursor.glyph_index_in_line {
                                                before_glyphs_count += 1;
                                            }
                                            total_glyphs += 1;
                                        }

                                        let offset = glyph.width * (before_glyphs_count as f32) / (total_glyphs as f32);
                                        return Some((glyph_index, offset));
                                    }
                                }

                                match layouted_line.layouted_glyphs.last() {
                                    Some(glyph) => {
                                        if editing_cursor.glyph_index_in_line == glyph.end_index {
                                            return Some((layouted_line.layouted_glyphs.len(), 0.0));
                                        }
                                    }
                                    None => {
                                        return Some((0, 0.0));
                                    }
                                }
                            }

                            None
                        };

                        if let Some((cursor_glyph_index, cursor_glyph_horizontal_offset)) = retrieve_cursor_position() {
                            let x = match layouted_line.layouted_glyphs.get(cursor_glyph_index) {
                                Some(glyph) => {
                                    // Start of detected glyph
                                    if glyph.level.is_rtl() {
                                        (glyph.x + glyph.width - cursor_glyph_horizontal_offset) as i32
                                    } else {
                                        (glyph.x + cursor_glyph_horizontal_offset) as i32
                                    }
                                }
                                None => match layouted_line.layouted_glyphs.last() {
                                    Some(glyph) => {
                                        // End of last glyph
                                        if glyph.level.is_rtl() {
                                            glyph.x as i32
                                        } else {
                                            (glyph.x + glyph.width) as i32
                                        }
                                    }
                                    None => {
                                        // Start of empty line
                                        0
                                    }
                                },
                            };

                            log::trace!(
                                "The cursor is being drawn at position {:?}",
                                (
                                    x + anchor_point.0 as i32,
                                    (anchor_point.1 - layouted_line.maximum_y_reach) as i32
                                )
                            );
                            let cursor_color = text::color::Color::rgba(0, 255, 0, 255);
                            draw_filled_rectangle(
                                x + anchor_point.0 as i32,
                                (anchor_point.1 - layouted_line.maximum_y_reach) as i32,
                                1,
                                (layouted_line.maximum_y_reach - layouted_line.minimum_y_origin).abs() as u32,
                                cursor_color,
                            );

                            break;
                        }
                    }
                    DocumentElement::Page { size, contents } => {
                        // TODO
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
            cairo_context.set_source_surface(&surface, 0.0, 0.0).unwrap();
            cairo_context.paint().unwrap();
        });
    }

    // Create a `EventControllerKey` to handle keyboard events
    let key_controller = EventControllerKey::new();

    {
        let editing_cursor = Rc::clone(&editing_cursor);
        let drawing_area = Rc::clone(&drawing_area);
        let layouted_lines = Rc::clone(&layouted_lines);
        let document = Rc::clone(&document);

        key_controller.connect_key_pressed(move |_controller, key, keycode, state| {
            log::trace!(
                "The user pressed the key: {} with keycode {} that is in the state {:?}",
                key,
                keycode,
                state
            );

            let mut editing_cursor = editing_cursor.borrow_mut();
            let mut layouted_lines = layouted_lines.borrow_mut();
            let mut document = document.borrow_mut();

            // Handle key events
            match key {
                Key::Left => {
                    // Move the cursor left
                    if editing_cursor.glyph_index_in_line > 0 {
                        editing_cursor.glyph_index_in_line -= 1;
                    }
                }
                Key::Right => {
                    // Move the cursor right
                    let line_buffer = layouted_lines.get(editing_cursor.line_index).unwrap();
                    if editing_cursor.glyph_index_in_line < line_buffer.text.len() {
                        editing_cursor.glyph_index_in_line += 1;
                    }
                }
                Key::Up => {
                    // Move the cursor up
                    if editing_cursor.line_index > 0 {
                        editing_cursor.line_index -= 1;
                    }
                }
                Key::Down => {
                    // Move the cursor down
                    if editing_cursor.line_index < layouted_lines.len() - 1 {
                        editing_cursor.line_index += 1;
                    }
                }
                Key::BackSpace => {
                    // Delete character before cursor
                    if editing_cursor.glyph_index_in_line > 0 {
                        let mut line_index_counter = 0;
                        for document_element in document.as_mut().unwrap().elements.iter_mut() {
                            if let DocumentElement::Line { spans, .. } = document_element {
                                if line_index_counter == editing_cursor.line_index {
                                    let mut total_characters_counter = 0;
                                    let mut cursor_span_index = None;
                                    let mut index_in_span = None;

                                    'outer: for (span_index, span) in spans.iter().enumerate() {
                                        for (character_index, _character) in span.0.chars().enumerate() {
                                            if total_characters_counter == editing_cursor.glyph_index_in_line - 1 {
                                                cursor_span_index = Some(span_index);
                                                index_in_span = Some(character_index);
                                                break 'outer;
                                            }
                                            total_characters_counter += 1;
                                        }
                                        // Check if cursor is at end of span
                                        if total_characters_counter == editing_cursor.glyph_index_in_line - 1 {
                                            cursor_span_index = Some(span_index);
                                            index_in_span = Some(span.0.len() - 1);
                                            break 'outer;
                                        }
                                    }

                                    if let (Some(cursor_span_index), Some(index_in_span)) = (cursor_span_index, index_in_span) {
                                        let span_text = &mut spans.get_mut(cursor_span_index).unwrap().0;
                                        span_text.remove(index_in_span);
                                        editing_cursor.glyph_index_in_line -= 1;
                                    }
                                }
                                line_index_counter += 1;
                            }
                        }
                    }
                }
                Key::Delete => {
                    // Delete character after cursor
                    let line_buffer = layouted_lines.get(editing_cursor.line_index).unwrap();
                    if editing_cursor.glyph_index_in_line < line_buffer.text.len() {
                        let mut line_index_counter = 0;
                        for document_element in document.as_mut().unwrap().elements.iter_mut() {
                            if let DocumentElement::Line { spans, .. } = document_element {
                                if line_index_counter == editing_cursor.line_index {
                                    let mut total_characters_counter = 0;
                                    let mut cursor_span_index = None;
                                    let mut index_in_span = None;

                                    'outer: for (span_index, span) in spans.iter().enumerate() {
                                        for (character_index, _character) in span.0.chars().enumerate() {
                                            if total_characters_counter == editing_cursor.glyph_index_in_line {
                                                cursor_span_index = Some(span_index);
                                                index_in_span = Some(character_index);
                                                break 'outer;
                                            }
                                            total_characters_counter += 1;
                                        }
                                    }

                                    if let (Some(cursor_span_index), Some(index_in_span)) = (cursor_span_index, index_in_span) {
                                        let span_text = &mut spans.get_mut(cursor_span_index).unwrap().0;
                                        span_text.remove(index_in_span);
                                    }
                                }
                                line_index_counter += 1;
                            }
                        }
                    }
                }
                _ => {}
            }

            if let Some(character) = key.to_unicode() {
                if character.is_control() && !['\t', '\n', '\r', '\u{92}'].contains(&character) {
                    // Filter out special chars (except for tab)
                    log::trace!("Refusing to insert control character {:?}", character);
                } else if ['\n', '\r'].contains(&character) {
                    // Handle Enter key - create a new line
                    let mut line_index_counter = 0;
                    let mut new_line_to_insert: Option<(usize, DocumentElement)> = None;
                    let font_size = document.as_ref().unwrap().font_size;

                    for document_element in document.as_mut().unwrap().elements.iter_mut() {
                        if let DocumentElement::Line { anchor_point, spans } = document_element {
                            if line_index_counter == editing_cursor.line_index {
                                // Find the cursor position within spans
                                let mut total_characters_counter = 0;
                                let mut cursor_span_index = None;
                                let mut index_in_span = None;

                                'outer: for (span_index, span) in spans.iter().enumerate() {
                                    for (character_index, _character) in span.0.chars().enumerate() {
                                        if total_characters_counter == editing_cursor.glyph_index_in_line {
                                            cursor_span_index = Some(span_index);
                                            index_in_span = Some(character_index);
                                            break 'outer;
                                        }
                                        total_characters_counter += 1;
                                    }
                                    // Check if cursor is at end of span
                                    if total_characters_counter == editing_cursor.glyph_index_in_line {
                                        cursor_span_index = Some(span_index);
                                        index_in_span = Some(span.0.len());
                                        break 'outer;
                                    }
                                }

                                // Handle edge cases for empty lines or cursor at start
                                if spans.is_empty() {
                                    // If there are no spans, create an empty one
                                    spans.push((String::new(), Attributes::new()));
                                }

                                let cursor_span_index = cursor_span_index.unwrap_or(0);
                                let index_in_span = index_in_span.unwrap_or_else(|| {
                                    spans.get(cursor_span_index).map(|s| s.0.len()).unwrap_or(0)
                                });

                                // Split the spans at cursor position
                                let mut new_line_spans = Vec::new();

                                // Add the part after cursor in the current span to new line
                                if index_in_span < spans[cursor_span_index].0.len() {
                                    let split_text = spans[cursor_span_index].0.split_off(index_in_span);
                                    if !split_text.is_empty() {
                                        new_line_spans.push((split_text, spans[cursor_span_index].1.clone()));
                                    }
                                }

                                // Move remaining spans to new line
                                while cursor_span_index + 1 < spans.len() {
                                    new_line_spans.push(spans.remove(cursor_span_index + 1));
                                }

                                // If new line has no spans, add an empty one with default attributes
                                if new_line_spans.is_empty() {
                                    new_line_spans.push((String::new(), Attributes::new()));
                                }

                                // Calculate new anchor point (offset by font size + some spacing)
                                let new_anchor_point = (anchor_point.0, anchor_point.1 + font_size * 1.5);

                                // Create new line element
                                new_line_to_insert = Some((
                                    line_index_counter + 1,
                                    DocumentElement::Line {
                                        anchor_point: new_anchor_point,
                                        spans: new_line_spans,
                                    }
                                ));

                                // Update cursor
                                editing_cursor.line_index += 1;
                                editing_cursor.glyph_index_in_line = 0;

                                break;
                            }
                            line_index_counter += 1;
                        }
                    }

                    // Insert the new line
                    if let Some((insert_index, new_line)) = new_line_to_insert {
                        document.as_mut().unwrap().elements.insert(insert_index, new_line);

                        // Update anchor points of all lines below the inserted line
                        let line_spacing = font_size * 1.5;
                        for i in (insert_index + 1)..document.as_ref().unwrap().elements.len() {
                            if let Some(DocumentElement::Line { anchor_point, .. }) = document.as_mut().unwrap().elements.get_mut(i) {
                                anchor_point.1 += line_spacing;
                            }
                        }
                    }
                } else {
                    let mut line_index_counter = 0;
                    for document_element in document.as_mut().unwrap().elements.iter_mut() {
                        if let DocumentElement::Line { spans, .. } = document_element {
                            if line_index_counter == editing_cursor.line_index {
                                // Find for which elements of the span the cursor index belongs to
                                // For example, in the span `[("B", attributes), ("old ", attributes.bold())]`,
                                // the cursor index 1 would belong to the span `[("old ", attributes.italic())]`, as would the indices 2, 3 and 4
                                // and the cursor index 0 would belong to the span `[("B", attributes)]`.
                                let mut cursor_span_index = None;
                                let mut index_in_span = None;
                                {
                                    let mut total_characters_counter = 0;
                                    'outer: for (span_index, span) in spans.iter().enumerate() {
                                        for (character_index, _character) in span.0.chars().enumerate() {
                                            if total_characters_counter == editing_cursor.glyph_index_in_line {
                                                cursor_span_index = Some(span_index);
                                                index_in_span = Some(character_index);
                                                break 'outer;
                                            }
                                            total_characters_counter += 1;
                                        }
                                    }
                                }

                                let cursor_span_index = cursor_span_index.unwrap_or(spans.len() - 1); // TODO
                                let index_in_span =
                                    index_in_span.unwrap_or(spans.get(cursor_span_index).unwrap().0.len());

                                spans
                                    .get_mut(cursor_span_index)
                                    .unwrap()
                                    .0
                                    .insert(index_in_span, character);

                                editing_cursor.glyph_index_in_line += 1;
                            }
                            line_index_counter += 1;
                        }
                    }
                }
            }

            drawing_area.queue_draw();
            glib::Propagation::Stop
        });
    }

    // Make the drawing area focusable so it can receive keyboard events
    drawing_area.set_focusable(true);
    drawing_area.set_can_focus(true);

    // Attach the key controller to the drawing area instead of the window
    // This ensures keyboard input goes to the text editing area, not UI buttons
    drawing_area.add_controller(key_controller);

    // Add the DrawingArea to the ScrolledWindow
    scrolled_window.set_child(Some(drawing_area.as_ref()));

    // Add the ScrolledWindow to the main window
    window.set_child(Some(&vertical_box));

    // Auto-load document if file path was provided as command-line argument
    if let Some(file_path) = initial_file_path {
        log::info!("Loading document from command-line argument: {:?}", file_path);

        if let Ok(mut file) = File::open(&file_path) {
            let mut file_content = String::new();
            if let Ok(_) = file.read_to_string(&mut file_content) {
                if let Ok(document_replacement) = serde_json::from_str::<Document>(&file_content) {
                    let font_size_replacement = document_replacement.font_size * scale_factor as f32;

                    let mut document_ref = document.borrow_mut();
                    *document_ref = Some(document_replacement);

                    let mut font_size_ref = font_size.borrow_mut();
                    *font_size_ref = font_size_replacement;

                    drawing_area.queue_draw();
                    drawing_area.grab_focus();

                    log::info!("Document loaded successfully from {:?}", file_path);
                } else {
                    log::error!("Failed to parse JSON from file: {:?}", file_path);
                }
            } else {
                log::error!("Failed to read file: {:?}", file_path);
            }
        } else {
            log::error!("Failed to open file: {:?}", file_path);
        }
    }

    window.present();
}
