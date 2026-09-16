//! A small event-loop driver for the production Comet view and update function.
//! Each event batch is dispatched, its messages applied, and the widget tree rebuilt
//! with the same cache before the next batch. No synthetic cursor assignments or
//! post-boot source replacements are used by the interaction scenarios.
use super::snapshot::{assert_snapshot, backend, image_path};
use comet_lib::ui::{fixtures, Comet, Message};
use iced::{
    advanced::{
        clipboard,
        renderer::{self, Headless},
        widget::{self, Operation},
        Clipboard,
    },
    event,
    keyboard::{self, key::Named, Modifiers},
    mouse, window, Event, Point, Rectangle, Renderer, Size,
};
use iced_test::{
    futures::futures::{executor::block_on, StreamExt},
    runtime::{user_interface::Cache, UserInterface},
    selector::{self, Bounded},
    Selector,
};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
};

#[derive(Default)]
struct MemoryClipboard(Option<String>);
impl Clipboard for MemoryClipboard {
    fn read(&self, _: clipboard::Kind) -> Option<String> {
        self.0.clone()
    }
    fn write(&mut self, _: clipboard::Kind, contents: String) {
        self.0 = Some(contents);
    }
}

pub struct EditorDriver {
    app: Comet,
    renderer: Renderer,
    cache: Option<Cache>,
    size: Size<u16>,
    pointer: mouse::Cursor,
    clipboard: MemoryClipboard,
}

impl EditorDriver {
    pub fn new(source: &str, size: (u16, u16)) -> Self {
        let mut app = fixtures::notebook();
        app.editor = iced::widget::text_editor::Content::with_text(source);
        app.selected.as_mut().unwrap().markdown = source.into();
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(std::borrow::Cow::Borrowed(iced_m3::fonts::ROBOTO));
        let renderer = iced_test::futures::futures::executor::block_on(
            <Renderer as Headless>::new(iced_m3::fonts::REGULAR, 16.into(), Some(backend())),
        )
        .expect("requested offscreen renderer must be available (no silent fallback)");
        assert_eq!(renderer.name(), backend());
        Self {
            app,
            renderer,
            cache: Some(Cache::default()),
            size: Size::new(size.0, size.1),
            pointer: mouse::Cursor::Unavailable,
            clipboard: MemoryClipboard::default(),
        }
    }

    pub fn readonly(mut self) -> Self {
        self.app.selected.as_mut().unwrap().readonly = true;
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.app.font_size = size;
        self
    }

    pub fn dark(mut self) -> Self {
        self.app.dark = true;
        self
    }

    /// Compare a stable part of the actual rendered editor across input events.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn region(&mut self, area: Rectangle) -> Vec<u8> {
        let frame = self.capture();
        let mut pixels = Vec::new();
        for y in (area.y * 2.0) as u32..((area.y + area.height) * 2.0) as u32 {
            for x in (area.x * 2.0) as u32..((area.x + area.width) * 2.0) as u32 {
                let offset = ((y * frame.width + x) * 4) as usize;
                pixels.extend_from_slice(&frame.rgba[offset..offset + 4]);
            }
        }
        pixels
    }

    /// Visible ink bounds, excluding the plain editor background and faint edges.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn ink_bounds(&mut self, area: Rectangle) -> Rectangle {
        let pixels = self.region(area);
        let width = (area.width * 2.0) as usize;
        let background = self.app.theme().colors.surface.into_rgba8();
        let mut left = usize::MAX;
        let mut top = usize::MAX;
        let mut right = 0;
        let mut bottom = 0;
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            if (0..3).any(|channel| pixel[channel].abs_diff(background[channel]) > 80) {
                left = left.min(index % width);
                right = right.max(index % width);
                top = top.min(index / width);
                bottom = bottom.max(index / width);
            }
        }
        assert!(
            left <= right && top <= bottom,
            "Expected visible ink in {area:?}"
        );
        Rectangle::new(
            Point::new(area.x + left as f32 / 2.0, area.y + top as f32 / 2.0),
            Size::new(
                (right - left + 1) as f32 / 2.0,
                (bottom - top + 1) as f32 / 2.0,
            ),
        )
    }

    fn logical_size(&self) -> Size {
        Size::new(f32::from(self.size.width), f32::from(self.size.height))
    }

    /// Redraw between OS events, as the desktop runtime does. This is essential
    /// for layout-dependent input and active/focused Material control states.
    fn event(&mut self, event: Event) {
        self.redraw();
        self.events(&[event]);
        self.redraw();
    }

    fn events(&mut self, events: &[Event]) {
        let mut messages = Vec::new();
        let mut ui = UserInterface::build(
            self.app.view(),
            self.logical_size(),
            self.cache.take().unwrap(),
            &mut self.renderer,
        );
        let (_, statuses) = ui.update(
            events,
            self.pointer,
            &mut self.renderer,
            &mut self.clipboard,
            &mut messages,
        );
        self.cache = Some(ui.into_cache());
        // Match the application's keyboard::listen subscription: it receives
        // unhandled keys, while editor navigation is consumed by the widget.
        for (event, status) in events.iter().zip(statuses) {
            if status == event::Status::Ignored {
                if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
                    messages.push(Message::Key(key.clone(), *modifiers));
                }
            }
        }
        self.apply(messages);
    }

    fn apply(&mut self, messages: Vec<Message>) {
        for message in messages {
            let task = self.app.update(message);
            if let Some(mut actions) = iced_test::runtime::task::into_stream(task) {
                while let Some(action) = block_on(actions.next()) {
                    match action {
                        iced_test::runtime::Action::Widget(mut operation) => {
                            let mut ui = UserInterface::build(
                                self.app.view(),
                                self.logical_size(),
                                self.cache.take().unwrap(),
                                &mut self.renderer,
                            );
                            ui.operate(&self.renderer, operation.as_mut());
                            self.cache = Some(ui.into_cache());
                            assert!(matches!(
                                operation.finish(),
                                widget::operation::Outcome::None
                            ));
                        }
                        _ => panic!("Interaction fixtures must not launch external work"),
                    }
                }
            }
            assert!(self.app.error.is_none(), "{:?}", self.app.error);
        }
    }

    fn redraw(&mut self) {
        self.redraw_at(iced::time::Instant::now());
    }

    fn redraw_at(&mut self, now: iced::time::Instant) {
        let theme = self.app.theme().reduced_motion(true);
        let mut messages = Vec::new();
        let mut ui = UserInterface::build(
            self.app.view(),
            self.logical_size(),
            self.cache.take().unwrap(),
            &mut self.renderer,
        );
        ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            self.pointer,
            &mut self.renderer,
            &mut self.clipboard,
            &mut messages,
        );
        ui.draw(
            &mut self.renderer,
            &theme,
            &renderer::Style {
                text_color: theme.colors.on_surface,
            },
            self.pointer,
        );
        self.cache = Some(ui.into_cache());
        assert!(messages.is_empty(), "A redraw must not mutate the document");
    }

    pub fn bounds(&mut self) -> Rectangle {
        self.find_bounds(selector::id("markdown-editor"))
    }

    fn find_bounds<S>(&mut self, selector: S) -> Rectangle
    where
        S: Selector + Send + 'static,
        S::Output: Bounded + Clone + Send + 'static,
    {
        let mut ui = UserInterface::build(
            self.app.view(),
            self.logical_size(),
            self.cache.take().unwrap(),
            &mut self.renderer,
        );
        let mut operation = selector.find();
        ui.operate(
            &self.renderer,
            &mut widget::operation::black_box(&mut operation),
        );
        self.cache = Some(ui.into_cache());
        match operation.finish() {
            widget::operation::Outcome::Some(Some(target)) => target.bounds(),
            _ => panic!("The target must be visible in the production view"),
        }
    }

    fn move_pointer(&mut self, point: Point) {
        self.pointer = mouse::Cursor::Available(point);
        self.event(Event::Mouse(mouse::Event::CursorMoved { position: point }));
    }

    pub fn click(&mut self, x: f32, y: f32) {
        self.press(x, y);
        self.release();
    }

    pub fn click_control(&mut self, label: &'static str) {
        self.press_control(label);
        self.release();
    }

    pub fn press_control<S>(&mut self, selector: S)
    where
        S: Selector + Send + 'static,
        S::Output: Bounded + Clone + Send + 'static,
    {
        let bounds = self.find_bounds(selector);
        self.move_pointer(bounds.center());
        self.event(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        )));
    }

    pub fn shift_click(&mut self, x: f32, y: f32) {
        self.event(Event::Keyboard(keyboard::Event::ModifiersChanged(
            Modifiers::SHIFT,
        )));
        self.click(x, y);
        self.event(Event::Keyboard(keyboard::Event::ModifiersChanged(
            Modifiers::empty(),
        )));
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        let bounds = self.bounds();
        self.move_pointer(Point::new(bounds.x + x, bounds.y + y));
    }

    pub fn press(&mut self, x: f32, y: f32) {
        self.move_to(x, y);
        self.event(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        )));
    }

    pub fn release(&mut self) {
        self.event(Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        )));
    }

    pub fn double_click(&mut self, x: f32, y: f32) {
        let bounds = self.bounds();
        self.move_pointer(Point::new(bounds.x + x, bounds.y + y));
        // Deliver the consecutive button events in one OS batch. Rendering
        // between clicks could exceed iced's 300 ms threshold on a busy runner.
        self.events(&[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
        self.redraw();
    }

    pub fn blur(&mut self) {
        // The empty footer is outside the editor and has no click action.
        self.move_pointer(Point::new(
            f32::from(self.size.width) / 2.0,
            f32::from(self.size.height) - 8.0,
        ));
        self.event(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        )));
        self.event(Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        )));
    }

    pub fn wheel(&mut self, lines: f32) {
        let bounds = self.bounds();
        self.move_pointer(bounds.center());
        self.event(Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: lines },
        }));
    }

    pub fn drag(&mut self, from: (f32, f32), to: (f32, f32)) {
        let bounds = self.bounds();
        self.move_pointer(Point::new(bounds.x + from.0, bounds.y + from.1));
        self.event(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        )));
        self.move_pointer(Point::new(bounds.x + to.0, bounds.y + to.1));
        self.event(Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        )));
    }

    pub fn key(&mut self, key: Named) {
        self.modified_key(key.into(), Modifiers::empty(), None);
    }
    pub fn shift_key(&mut self, key: Named) {
        self.modified_key(key.into(), Modifiers::SHIFT, None);
    }

    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            if character == '\n' {
                self.key(Named::Enter);
                continue;
            }
            let text = character.to_string();
            self.modified_key(
                if character == ' ' {
                    keyboard::Key::Named(Named::Space)
                } else {
                    keyboard::Key::Character(text.clone().into())
                },
                Modifiers::empty(),
                Some(text),
            );
        }
    }

    pub fn shortcut(&mut self, key: &str) {
        let command = if cfg!(target_os = "macos") {
            Modifiers::LOGO
        } else {
            Modifiers::CTRL
        };
        self.modified_key(keyboard::Key::Character(key.into()), command, None);
    }

    pub fn redo(&mut self) {
        let command = if cfg!(target_os = "macos") {
            Modifiers::LOGO
        } else {
            Modifiers::CTRL
        };
        self.modified_key(
            keyboard::Key::Character("z".into()),
            command | Modifiers::SHIFT,
            None,
        );
    }

    fn modified_key(&mut self, key: keyboard::Key, modifiers: Modifiers, text: Option<String>) {
        self.event(Event::Keyboard(keyboard::Event::ModifiersChanged(
            modifiers,
        )));
        self.event(Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text: text.map(Into::into),
        }));
        self.event(Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
        }));
        self.event(Event::Keyboard(keyboard::Event::ModifiersChanged(
            Modifiers::empty(),
        )));
    }

    pub fn resize(&mut self, size: (u16, u16)) {
        self.size = Size::new(size.0, size.1);
        self.redraw();
    }

    pub fn cursor(&self) -> (usize, usize) {
        let position = self.app.editor.cursor().position;
        (position.line, position.column)
    }

    pub fn assert_editor_focus(&mut self, name: &str, expected: bool) {
        let mut ui = UserInterface::build(
            self.app.view(),
            self.logical_size(),
            self.cache.take().unwrap(),
            &mut self.renderer,
        );
        let mut operation =
            widget::operation::focusable::is_focused(widget::Id::new("markdown-editor"));
        ui.operate(
            &self.renderer,
            &mut widget::operation::black_box(&mut operation),
        );
        self.cache = Some(ui.into_cache());
        let widget::operation::Outcome::Some(focused) = operation.finish() else {
            panic!("The production editor must be present");
        };
        if focused != expected {
            self.capture().write(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/screenshot-failures")
                    .join(name),
            );
        }
        assert_eq!(focused, expected, "{name}: editor keyboard focus");
    }

    pub fn check(
        &mut self,
        name: &str,
        source: &str,
        cursor: (usize, usize),
        selection: Option<&str>,
    ) {
        let capture = self.capture();
        let actual_source = self.app.editor.text();
        let actual_selection = self.app.editor.selection();
        if actual_source != source
            || self.cursor() != cursor
            || actual_selection.as_deref() != selection
        {
            capture.write(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/screenshot-failures")
                    .join(name),
            );
        }
        assert_eq!(actual_source, source, "{name}: Markdown source");
        assert_eq!(
            self.cursor(),
            cursor,
            "{name}: source cursor (UTF-8 byte column)"
        );
        assert_eq!(
            actual_selection.as_deref(),
            selection,
            "{name}: selected Markdown"
        );
        assert_snapshot(name, |path| capture.matches(path));
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Rounded, clamped pixel coordinates in a u16 viewport.
    fn capture(&mut self) -> Capture {
        let bounds = self.bounds();
        self.redraw();
        // Capture the settled Material state, including nested themed controls.
        // Advancing the redraw timestamp avoids wall-clock sleeps and animation
        // timing differences between fast local runs and slower CI machines.
        self.redraw_at(iced::time::Instant::now() + std::time::Duration::from_secs(2));
        let size = Size::new(
            u32::from(self.size.width) * 2,
            u32::from(self.size.height) * 2,
        );
        let pixels = self
            .renderer
            .screenshot(size, 2.0, self.app.theme().colors.surface);
        // Crop from the text's left edge through the pane's right edge, retaining
        // the cursor, selections and the scrollbar outside the text margin.
        let x = ((bounds.x * 2.0).round().max(0.0) as u32).min(size.width);
        let y = ((bounds.y * 2.0).round().max(0.0) as u32).min(size.height);
        let width = size.width - x;
        let height = ((bounds.height * 2.0).round().max(0.0) as u32).min(size.height - y);
        assert!(
            width > 0 && height > 0,
            "The editor must have a visible viewport"
        );
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let start = ((row * size.width + x) * 4) as usize;
            rgba.extend_from_slice(&pixels[start..start + (width * 4) as usize]);
        }
        Capture {
            rgba,
            width,
            height,
        }
    }
}

struct Capture {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}
impl Capture {
    fn write(&self, prefix: &Path) {
        let path = image_path(prefix);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut encoder = png::Encoder::new(
            BufWriter::new(File::create(path).unwrap()),
            self.width,
            self.height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&self.rgba)
            .unwrap();
    }
    fn matches(&self, prefix: &Path) -> bool {
        let path = image_path(prefix);
        if !path.exists() {
            self.write(prefix);
            return true;
        }
        let mut decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()))
            .read_info()
            .unwrap();
        let mut rgba = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut rgba).unwrap();
        info.width == self.width
            && info.height == self.height
            && info.color_type == png::ColorType::Rgba
            && rgba[..info.buffer_size()] == self.rgba
    }
}
