use std::{num::NonZeroU32, rc::Rc};

use document::{Document, DocumentElement, EditingCursor};
use text::{
    attributes::{Attributes, Style, Weight},
    color::Color,
    font_system::FontSystem,
    line_buffer::LineBuffer,
    swash_cache::SwashCache,
};
use tiny_skia::{Paint, PixmapMut, Transform};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    keyboard::Key,
    window::WindowBuilder,
};

mod document;

#[derive(Debug, Clone)]
enum UserEvent {
    RequestRedraw,
    InsertText(String),
}

fn main() {
    // Initialize the logging handler
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    // Create the window and graphics context to draw to
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
        .build()
        .unwrap();
    let event_loop_proxy = event_loop.create_proxy();
    let window = Rc::new(WindowBuilder::new().build(&event_loop).unwrap());
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    let default_attributes = Attributes::new();
    // NOTE(ghovax): There is definitely a better way to do this.
    let italic_attributes = {
        let mut attributes = Attributes::new();
        attributes.style = Style::Italic;
        attributes
    };
    let bold_attributes = {
        let mut attributes = Attributes::new();
        attributes.weight = Weight::BOLD;
        attributes
    };

    // NOTE(ghovax): This could be loaded from an actual document.
    let document_elements = vec![DocumentElement::Line {
        anchor_point: (85.0, 120.0),
        spans: vec![
            ("pop".to_string(), default_attributes),
            ("old ".to_string(), italic_attributes),
            ("example text ßåß√Ï√ÅÏ".to_string(), bold_attributes),
        ],
    }];

    // NOTE(ghovax): This could be loaded from a configuration file.
    let font_size = 32.0;
    let mut document = Document::new(&window, document_elements, font_size);

    // Create all the line buffers from the respective document elements

    // TODO(ghovax): Figure out how to position the cursor at each line.
    let mut mouse_position = PhysicalPosition::new(0.0, 0.0);
    let mut mouse_left_button_state = ElementState::Released;

    event_loop
        .run(|event, event_loop_window_target| {
            event_loop_window_target.set_control_flow(ControlFlow::Wait);

            #[allow(clippy::single_match, clippy::collapsible_match)]
            match event {
                Event::UserEvent(user_event) => match user_event {
                    UserEvent::RequestRedraw => {
                        document.draw_to_surface(&window, &mut surface);
                    }
                    UserEvent::InsertText(text) => {
                        for character in text.chars() {
                            if character.is_control()
                                && !['\t', '\n', '\r', '\u{92}'].contains(&character)
                            {
                                // Filter out special chars (except for tab)
                                log::debug!("Refusing to insert control character {:?}", character);
                            } else if ['\n', '\r'].contains(&character) {
                                // TODO
                            } else {
                                // TODO
                            }
                        }
                    }
                },
                Event::WindowEvent { window_id, event } => match event {
                    WindowEvent::CloseRequested => {
                        event_loop_window_target.exit();
                    }
                    WindowEvent::KeyboardInput {
                        event:
                            winit::event::KeyEvent {
                                logical_key, state, ..
                            },
                        ..
                    } => {
                        if state.is_pressed() {
                            match logical_key {
                                Key::Character(text) => {
                                    for event in [
                                        UserEvent::InsertText(text.to_string()), // First send the text
                                        UserEvent::RequestRedraw, // Then request a redraw
                                    ] {
                                        event_loop_proxy.send_event(event).unwrap();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        log::info!("Updated scale factor for the window with ID {window_id:?}");
                        document.set_font_size(font_size, scale_factor as f32);

                        event_loop_proxy
                            .send_event(UserEvent::RequestRedraw)
                            .unwrap();
                    }
                    WindowEvent::Resized(size) => {
                        event_loop_proxy
                            .send_event(UserEvent::RequestRedraw)
                            .unwrap();
                    }
                    WindowEvent::CursorMoved {
                        device_id: _,
                        position,
                    } => {
                        mouse_position = position;
                    }
                    WindowEvent::MouseInput {
                        device_id: _,
                        state,
                        button,
                    } => {
                        if button == MouseButton::Left {
                            if state == ElementState::Pressed
                                && mouse_left_button_state == ElementState::Released
                            {
                                // Position the cursor where the mouse clicked
                                document.position_cursor(&mouse_position);
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
