//! The document is rendered once, with native editing and a source-aware cursor.
//! iced's editor supplies keyboard, clipboard, selection and IME handling. Its
//! source layout is replaced by paragraphs whose glyphs map back to Markdown.
mod attachments;
pub(crate) mod document;
mod reveal;
mod scrollbar;
use super::Message;
use document::{physical_lines, Document, Line};
use iced::advanced::widget::operation::focusable::Focusable as _;
use iced::advanced::Renderer as _;
use iced::{
    advanced::{
        graphics::text::Paragraph,
        image::{self, Renderer as _},
        layout, renderer,
        text::{self, Paragraph as _, Renderer as _},
        widget::{self, Tree},
        Clipboard, Layout, Shell, Widget,
    },
    alignment, font, mouse,
    widget::{
        text_editor::{self, Action, Motion},
        themer,
    },
    Border, Color, Element, Event, Font, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
};
use scrollbar::Scrollbar;
use std::{ops::Range, path::Path};
use unicode_segmentation::UnicodeSegmentation;

const TEXT_MARGIN: f32 = 28.0;

pub(crate) fn source_offset(
    content: &text_editor::Content,
    position: text::editor::Position,
) -> usize {
    let source = content.text();
    let lines = physical_lines(&source);
    lines.get(position.line).map_or(source.len(), |line| {
        line.start + position.column.min(line.len())
    })
}

pub(crate) fn place_cursor(content: &mut text_editor::Content, offset: usize, select: bool) {
    let source = content.text();
    let offset = offset.min(source.len());
    let offset = (0..=offset)
        .rev()
        .find(|i| source.is_char_boundary(*i))
        .unwrap_or(0);
    let lines = physical_lines(&source);
    let (line, range) = lines
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.start <= offset)
        .unwrap();
    let old = content.cursor();
    let selection = select.then_some(old.selection.unwrap_or(old.position));
    // iced's move_to preserves an existing selection when selection is None.
    content.perform(Action::Move(Motion::DocumentStart));
    content.move_to(text::editor::Cursor {
        position: text::editor::Position {
            line,
            column: (offset - range.start).min(range.len()),
        },
        selection,
    });
}

pub(crate) fn select_at_cursor(content: &mut text_editor::Content, whole_line: bool) {
    let source = content.text();
    let cursor = content.cursor().position;
    let lines = physical_lines(&source);
    let Some(line) = lines.get(cursor.line) else {
        return;
    };
    let range = if whole_line {
        line.clone()
    } else {
        let text = &source[line.clone()];
        let column = cursor.column.min(text.len());
        text.split_word_bound_indices()
            .find(|(start, word)| *start <= column && column < start + word.len())
            .or_else(|| text.split_word_bound_indices().next_back())
            .map_or(line.clone(), |(start, word)| {
                line.start + start..line.start + start + word.len()
            })
    };
    // Native word/line selection stores an anchor and an implicit selection
    // kind. Use explicit source endpoints so delimiter reveal and highlighting
    // see the same selection that replacement and clipboard operations use.
    place_cursor(content, range.start, false);
    place_cursor(content, range.end, true);
}

pub fn editor<'a>(
    content: &'a text_editor::Content,
    note_id: &'a str,
    size: f32,
    theme: iced_m3::Theme,
    editable: bool,
    attachments: Option<&Path>,
) -> Element<'a, Message> {
    super::fonts::ensure_loaded();
    let document = Document::parse(&content.text(), None);
    let controls = document
        .lines
        .iter()
        .enumerate()
        .filter_map(|(line, data)| {
            if let Some((offset, checked)) = data.task {
                let check = iced_m3::checkbox(checked)
                    .on_toggle(move |_| Message::ToggleTask(offset))
                    .disabled(!editable);
                return Some((line, true, themer(Some(theme.clone()), check).into()));
            }
            if let Some((file, hash)) = data.image.as_deref().and_then(attachments::attachment) {
                if attachments.is_none_or(|root| !root.join(file).is_file()) {
                    let download = iced_m3::button("Download attachment")
                        .variant(iced_m3::ButtonVariant::Outlined)
                        .on_press(Message::FetchAttachment(hash.into()));
                    return Some((line, false, themer(Some(theme.clone()), download).into()));
                }
            }
            None
        })
        .collect();
    Element::new(MarkdownEditor {
        content,
        note_id,
        size,
        theme: theme.iced(),
        tag_background: theme.colors.primary_container,
        reveal_duration: theme.motion.short,
        input: iced::widget::text_editor(content)
            .id("markdown-editor")
            .on_action(|action| action)
            .font(iced_m3::fonts::REGULAR)
            .size(size)
            .padding(0)
            .height(Length::Fill)
            .into(),
        controls,
        attachments: attachments.map(Path::to_path_buf),
    })
}

struct MarkdownEditor<'a> {
    content: &'a text_editor::Content,
    note_id: &'a str,
    size: f32,
    theme: Theme,
    tag_background: Color,
    reveal_duration: std::time::Duration,
    input: Element<'a, Action>,
    controls: Vec<(usize, bool, Element<'a, Message>)>,
    attachments: Option<std::path::PathBuf>,
}

#[derive(Default)]
struct State {
    document: Document,
    lines: Vec<LayoutLine>,
    scroll: f32,
    height: f32,
    identity: String,
    cursor: Option<usize>,
    modifiers: iced::keyboard::Modifiers,
    preferred_x: Option<f32>,
    reveal: Option<Range<usize>>,
    pointer: Option<PointerGesture>,
    scrollbar_grab: Option<f32>,
    scrollbar_hovered: bool,
    reveal_motion: reveal::Motion,
    motion_layout: Option<(Size, f32)>,
    follow_reveal: bool,
}
struct PointerGesture {
    origin: Point,
    dragging: bool,
}
struct LayoutLine {
    paragraph: Paragraph,
    x: f32,
    y: f32,
    height: f32,
    image: Option<image::Handle>,
    image_size: Size,
    cells: Vec<LayoutCell>,
}
struct LayoutCell {
    data: Line,
    paragraph: Paragraph,
    x: f32,
    width: f32,
}

impl State {
    fn hit(&self, point: Point) -> usize {
        let y = point.y + self.scroll;
        let Some((i, line)) = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.height > 0.0)
            .min_by(|(_, a), (_, b)| {
                distance(y, a.y, a.height).total_cmp(&distance(y, b.y, b.height))
            })
        else {
            return 0;
        };
        if let Some(cell) = line.cells.iter().min_by(|a, b| {
            distance(point.x, a.x, a.width).total_cmp(&distance(point.x, b.x, b.width))
        }) {
            let index = cell
                .paragraph
                .hit_test(Point::new(
                    point.x - cell.x - 8.0,
                    (y - line.y - 6.0).max(0.0),
                ))
                .map_or(0, text::Hit::cursor);
            return cell.data.source_at(index);
        }
        let index = line
            .paragraph
            .hit_test(Point::new(point.x - line.x, (y - line.y).max(0.0)))
            .map_or(0, text::Hit::cursor);
        self.document.lines[i].source_at(index)
    }

    fn caret(&self, offset: usize) -> Rectangle {
        let index = self
            .document
            .lines
            .iter()
            .rposition(|line| line.source.start <= offset)
            .unwrap_or(0);
        let Some(line) = self.lines.get(index) else {
            return Rectangle::default();
        };
        if let Some(cell) = line
            .cells
            .iter()
            .find(|cell| cell.data.source.contains(&offset))
        {
            return caret_in(
                &cell.paragraph,
                &cell.data,
                offset,
                cell.x + 8.0,
                line.y + 6.0,
            );
        }
        caret_in(
            &line.paragraph,
            &self.document.lines[index],
            offset,
            line.x,
            line.y,
        )
    }

    fn link(&self, point: Point) -> Option<String> {
        let source = self.hit(point);
        let i = self
            .document
            .lines
            .iter()
            .position(|line| line.source.contains(&source))?;
        let mut byte = 0;
        let display = self.document.lines[i].display_at(source);
        for run in &self.document.lines[i].runs {
            byte += run.text.len();
            if display < byte {
                return run.style.link.as_ref().map(ToString::to_string);
            }
        }
        None
    }
}
fn distance(y: f32, start: f32, height: f32) -> f32 {
    (start - y).max(0.0) + (y - start - height).max(0.0)
}

impl Widget<Message, Theme, Renderer> for MarkdownEditor<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        std::iter::once(Tree::new(&self.input))
            .chain(self.controls.iter().map(|(_, _, check)| Tree::new(check)))
            .collect()
    }
    fn diff(&self, tree: &mut Tree) {
        if tree.children.is_empty() {
            tree.children.push(Tree::new(&self.input));
        }
        tree.children.truncate(self.controls.len() + 1);
        for (_, _, check) in self.controls.iter().skip(tree.children.len() - 1) {
            tree.children.push(Tree::new(check));
        }
        tree.children[0].diff(&self.input);
        for (i, (_, _, check)) in self.controls.iter().enumerate() {
            tree.children[i + 1].diff(check);
        }
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Text owns its margins; the viewport extends to the pane edge so the
        // scroll indicator does not inherit the document's horizontal inset.
        let content_limits = limits.shrink(Size::new(TEXT_MARGIN * 2.0, 0.0));
        let input = self
            .input
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &content_limits)
            .move_to(Point::new(TEXT_MARGIN, 0.0));
        let focused = tree.children[0]
            .state
            .downcast_ref::<text_editor::State<text::highlighter::PlainText>>()
            .is_focused();
        let state = tree.state.downcast_mut::<State>();
        let source = self.content.text();
        let cursor = self.content.cursor();
        let offset = source_offset(self.content, cursor.position);
        let selection = cursor
            .selection
            .map_or(offset, |p| source_offset(self.content, p));
        if state.identity != self.note_id {
            state.scroll = 0.0;
            state.scrollbar_grab = None;
            state.scrollbar_hovered = false;
            state.preferred_x = None;
            state.pointer = None;
            state.reveal_motion = reveal::Motion::default();
            state.follow_reveal = false;
            state.identity = self.note_id.into();
        }
        // Revealing or hiding syntax can move text horizontally, wrap lines or
        // replace an entire table row. Keep the projection used at mouse-down
        // until release so clicks and drag endpoints stay under the pointer.
        if state.pointer.is_none() {
            state.reveal = focused.then_some(offset.min(selection)..offset.max(selection));
        }
        if state.motion_layout != Some((limits.max(), self.size)) {
            state.reveal_motion.finish();
            state.motion_layout = Some((limits.max(), self.size));
        }
        state.document = state.reveal_motion.resolve(
            &source,
            Document::parse(&source, state.reveal.clone()),
            self.reveal_duration,
            |line| {
                let size = line_size(line, self.size);
                let paragraph = make_paragraph(
                    line,
                    size,
                    content_limits.max().width - line_indent(line) - 8.0,
                    &self.theme,
                );
                paragraph.buffer().layout_runs().nth(1).is_some()
            },
        );
        let mut y = 0.0;
        state.lines = state
            .document
            .lines
            .iter()
            .enumerate()
            .map(|(line_index, line)| {
                let x = line_indent(line);
                let size = line_size(line, self.size);
                let paragraph = make_paragraph(
                    line,
                    size,
                    content_limits.max().width - x - 8.0,
                    &self.theme,
                );
                let cell_width =
                    (content_limits.max().width - x - 8.0) / line.table_cells.len().max(1) as f32;
                let cells: Vec<_> = line
                    .table_cells
                    .iter()
                    .enumerate()
                    .map(|(i, data)| LayoutCell {
                        paragraph: make_paragraph(data, self.size, cell_width - 16.0, &self.theme),
                        data: data.clone(),
                        x: x + i as f32 * cell_width,
                        width: cell_width,
                    })
                    .collect();
                let image = line
                    .image
                    .as_deref()
                    .and_then(attachments::attachment)
                    .and_then(|(file, _)| self.attachments.as_ref().map(|dir| dir.join(file)))
                    .filter(|path| path.is_file())
                    .map(image::Handle::from_path);
                let image_size = image
                    .as_ref()
                    .and_then(|handle| renderer.measure_image(handle))
                    .map_or(Size::ZERO, |size| {
                        let scale = ((content_limits.max().width - x) / size.width as f32).min(1.0);
                        Size::new(size.width as f32 * scale, size.height as f32 * scale)
                    });
                let height = if line.code {
                    // Hidden fences remain part of the block's background.
                    // Measure their source in both states so reveal never moves
                    // code or the paragraphs following it, even when wrapped.
                    paragraph.min_height().max(size * 1.5)
                } else if line.hidden {
                    0.0
                } else if !cells.is_empty() {
                    cells
                        .iter()
                        .map(|cell| cell.paragraph.min_height())
                        .fold(self.size * 1.5, f32::max)
                        + 12.0
                } else if image_size.height > 0.0 {
                    image_size.height + 12.0
                } else {
                    // Empty source lines occupy a full text row, matching the
                    // caret and whitespace-only lines during editing.
                    paragraph.min_height().max(size * 1.5)
                        + if line.heading.is_some() { 8.0 } else { 0.0 }
                };
                let height = height
                    + if self
                        .controls
                        .iter()
                        .any(|(index, check, _)| *index == line_index && !check)
                    {
                        48.0
                    } else {
                        0.0
                    };
                let result = LayoutLine {
                    paragraph,
                    x,
                    y: y + line.offset_y(),
                    height,
                    image,
                    image_size,
                    cells,
                };
                y += height;
                result
            })
            .collect();
        state.height = y + self.size * 2.0;
        if focused && state.cursor != Some(offset) {
            state.follow_reveal = true;
        }
        if focused
            && state
                .pointer
                .as_ref()
                .is_none_or(|pointer| pointer.dragging)
            && (state.cursor != Some(offset) || state.follow_reveal)
        {
            let caret = state.caret(offset);
            if caret.y < state.scroll {
                state.scroll = caret.y;
            }
            if caret.y + caret.height > state.scroll + limits.max().height {
                state.scroll = caret.y + caret.height - limits.max().height;
            }
        }
        state.cursor = Some(offset);
        if !state.reveal_motion.active() {
            state.follow_reveal = false;
        }
        state.scroll = state
            .scroll
            .clamp(0.0, (state.height - limits.max().height).max(0.0));
        let mut children = vec![input];
        for (i, (line, is_check, check)) in self.controls.iter_mut().enumerate() {
            let line = &state.lines[*line];
            children.push(
                check
                    .as_widget_mut()
                    .layout(
                        &mut tree.children[i + 1],
                        renderer,
                        &layout::Limits::new(
                            Size::ZERO,
                            if *is_check {
                                Size::new(24.0, 24.0)
                            } else {
                                Size::new(content_limits.max().width, 40.0)
                            },
                        ),
                    )
                    .move_to(Point::new(
                        TEXT_MARGIN + if *is_check { line.x - 32.0 } else { line.x },
                        line.y - state.scroll
                            + if *is_check {
                                line.paragraph
                                    .buffer()
                                    .layout_runs()
                                    .next()
                                    .map_or(self.size * 0.75, |run| {
                                        run.line_top + run.line_height / 2.0
                                    })
                                    // Optical alignment with Roboto sentence text,
                                    // including descenders, scales with editor size.
                                    + self.size * 0.06
                                    - 12.0
                            } else {
                                line.paragraph.min_height().max(self.size * 1.5) + 4.0
                            },
                    )),
            );
        }
        layout::Node::with_children(limits.max(), children)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(iced::window::Event::RedrawRequested(now)) = event {
            let state = tree.state.downcast_mut::<State>();
            if state.reveal_motion.tick(
                *now,
                state.pointer.is_some() || state.scrollbar_grab.is_some(),
            ) {
                shell.invalidate_layout();
                shell.request_redraw();
            }
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Window(iced::window::Event::Unfocused)
                | Event::Keyboard(iced::keyboard::Event::KeyPressed { .. })
        ) && tree.state.downcast_mut::<State>().pointer.take().is_some()
        {
            tree.state.downcast_mut::<State>().cursor = None;
            shell.invalidate_layout();
            shell.request_redraw();
        }
        if tree.state.downcast_ref::<State>().reveal_motion.active()
            && tree.state.downcast_ref::<State>().pointer.is_none()
            && tree.state.downcast_ref::<State>().scrollbar_grab.is_none()
        {
            shell.request_redraw();
        }
        let state = tree.state.downcast_mut::<State>();
        let scrollbar = Scrollbar::new(layout.bounds(), state.height, state.scroll);
        let visible = layout.bounds().intersection(viewport);
        let over_viewport = visible.is_some_and(|bounds| cursor.is_over(bounds));
        let over_scrollbar = over_viewport
            && scrollbar
                .as_ref()
                .is_some_and(|bar| cursor.is_over(bar.track));
        if state.scrollbar_hovered != over_scrollbar {
            state.scrollbar_hovered = over_scrollbar;
            shell.request_redraw();
        }
        match event {
            Event::Window(iced::window::Event::Unfocused) => {
                state.scrollbar_grab = None;
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.scrollbar_grab.take().is_some() =>
            {
                shell.capture_event();
                shell.request_redraw();
                return;
            }
            Event::Mouse(mouse::Event::CursorMoved { position })
                if state.scrollbar_grab.is_some() =>
            {
                if let Some(bar) = scrollbar {
                    state.scroll = bar.scroll_to(position.y, state.scrollbar_grab.unwrap());
                }
                shell.capture_event();
                shell.invalidate_layout();
                shell.request_redraw();
                return;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if over_scrollbar => {
                state.follow_reveal = false;
                let bar = scrollbar.unwrap();
                let position = cursor.position().unwrap();
                // Keep the grabbed point under the pointer. A track click centers
                // the thumb there and can continue directly into a drag.
                let grab = if bar.thumb.contains(position) {
                    (position.y - bar.thumb.y) / bar.thumb.height
                } else {
                    0.5
                };
                state.scrollbar_grab = Some(grab);
                state.scroll = bar.scroll_to(position.y, grab);
                // Like a document checkbox, its scrollbar belongs to the editor.
                // Restore input focus after the root focus scope handles the press.
                tree.children[0]
                    .state
                    .downcast_mut::<text_editor::State<text::highlighter::PlainText>>()
                    .focus();
                shell.capture_event();
                shell.invalidate_layout();
                shell.request_redraw();
                return;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if over_viewport => {
                state.follow_reveal = false;
                // Handle raw deltas before the native source editor converts
                // pixels to integer lines. Trackpads stay smooth and 1:1; mouse
                // wheels move one rendered row per line, with no extra multiplier.
                let pixels = match delta {
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                    mouse::ScrollDelta::Lines { y, .. } => y * self.size * 1.5,
                };
                if pixels != 0.0 {
                    state.scroll = (state.scroll - pixels)
                        .clamp(0.0, (state.height - layout.bounds().height).max(0.0));
                    shell.capture_event();
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
                return;
            }
            _ => {}
        }
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) = event
        {
            let focused = tree.children[0]
                .state
                .downcast_ref::<text_editor::State<text::highlighter::PlainText>>()
                .is_focused();
            if focused && modifiers.command() && !modifiers.alt() {
                let command = key
                    .to_latin(*physical_key)
                    .map(|key| key.to_ascii_lowercase());
                let history = match command {
                    Some('z') => Some(if modifiers.shift() {
                        Message::Redo
                    } else {
                        Message::Undo
                    }),
                    Some('y') if !cfg!(target_os = "macos") => Some(Message::Redo),
                    _ => None,
                };
                if let Some(message) = history {
                    tree.state.downcast_mut::<State>().cursor = None;
                    tree.state.downcast_mut::<State>().preferred_x = None;
                    shell.publish(message);
                    shell.capture_event();
                    shell.invalidate_layout();
                    shell.request_redraw();
                    return;
                }
            }
        }
        let content_bounds = layout.child(0).bounds();
        let Some(clip) = content_bounds.intersection(viewport) else {
            return;
        };
        for (i, (_, is_check, check)) in self.controls.iter_mut().enumerate() {
            let bounds = layout.child(i + 1).bounds();
            if bounds.intersects(&clip) {
                // Capture is shared by sibling widgets. Only this control's
                // own result may trigger document focus or stop child dispatch.
                let mut messages = Vec::new();
                let mut local = Shell::new(&mut messages);
                check.as_widget_mut().update(
                    &mut tree.children[i + 1],
                    event,
                    layout.child(i + 1),
                    cursor,
                    renderer,
                    clipboard,
                    &mut local,
                    &clip,
                );
                let captured = local.is_event_captured();
                shell.merge(local, std::convert::identity);
                if captured {
                    if *is_check
                        && matches!(
                            event,
                            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                        )
                    {
                        // A task box belongs to the document. Keep typing at the
                        // existing caret after a pointer toggle; Tab can still
                        // focus the Material checkbox for keyboard activation.
                        check.as_widget_mut().operate(
                            &mut tree.children[i + 1],
                            layout.child(i + 1),
                            renderer,
                            &mut widget::operation::focusable::unfocus::<()>(),
                        );
                        tree.children[0]
                            .state
                            .downcast_mut::<text_editor::State<text::highlighter::PlainText>>()
                            .focus();
                        shell.invalidate_layout();
                        shell.request_redraw();
                    }
                    return;
                }
            }
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && cursor.is_over(clip)
        {
            // Double/triple clicks emit SelectWord/SelectLine instead of Click.
            // Freeze every document press, including those selection gestures.
            tree.state.downcast_mut::<State>().pointer = Some(PointerGesture {
                origin: cursor.position().unwrap()
                    - Vector::new(content_bounds.x, content_bounds.y),
                dragging: false,
            });
        }
        let mut actions = Vec::new();
        let mut local = Shell::new(&mut actions);
        self.input.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.child(0),
            cursor,
            renderer,
            clipboard,
            &mut local,
            &clip,
        );
        if local.is_event_captured() {
            shell.capture_event();
        }
        shell.request_redraw_at(local.redraw_request());
        let state = tree.state.downcast_mut::<State>();
        if let Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.modifiers = *modifiers;
        }
        if let iced::advanced::InputMethod::Enabled { cursor, .. } = local.input_method_mut() {
            let caret = state.caret(source_offset(self.content, self.content.cursor().position));
            *cursor = Rectangle {
                x: content_bounds.x + caret.x,
                y: content_bounds.y + caret.y - state.scroll,
                ..caret
            };
        }
        shell.request_input_method(local.input_method());
        drop(local);
        for action in actions {
            if !matches!(
                action,
                Action::Move(Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown)
                    | Action::Select(Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown)
                    | Action::Scroll { .. }
            ) {
                state.preferred_x = None;
            }
            match action {
                Action::Click(point)
                    if state.modifiers.command() && state.link(point).is_some() =>
                {
                    shell.publish(Message::OpenUrl(state.link(point).unwrap()))
                }
                Action::Click(point) => {
                    state.pointer = Some(PointerGesture {
                        origin: point,
                        dragging: false,
                    });
                    shell.publish(Message::EditorCursor(
                        state.hit(point),
                        state.modifiers.shift(),
                    ));
                }
                Action::Drag(point) => {
                    let Some(pointer) = &mut state.pointer else {
                        continue;
                    };
                    // Ordinary click jitter is not a selection gesture. Once a
                    // drag starts, returning to the anchor still updates it.
                    pointer.dragging |= point.distance(pointer.origin) >= 4.0;
                    if !pointer.dragging {
                        continue;
                    }
                    shell.publish(Message::EditorCursor(state.hit(point), true));
                }
                Action::Move(motion) | Action::Select(motion)
                    if matches!(
                        motion,
                        Motion::Up
                            | Motion::Down
                            | Motion::PageUp
                            | Motion::PageDown
                            | Motion::Home
                            | Motion::End
                    ) =>
                {
                    let select = matches!(action, Action::Select(_));
                    let caret =
                        state.caret(source_offset(self.content, self.content.cursor().position));
                    let x = if matches!(motion, Motion::Home | Motion::End) {
                        caret.x
                    } else {
                        *state.preferred_x.get_or_insert(caret.x)
                    };
                    let point = match motion {
                        Motion::Up => Point::new(x, caret.y - state.scroll - caret.height * 0.5),
                        Motion::Down => Point::new(x, caret.y - state.scroll + caret.height * 1.5),
                        Motion::PageUp => {
                            Point::new(x, caret.y - state.scroll - layout.bounds().height)
                        }
                        Motion::PageDown => {
                            Point::new(x, caret.y - state.scroll + layout.bounds().height)
                        }
                        Motion::Home => {
                            Point::new(0.0, caret.y - state.scroll + caret.height * 0.5)
                        }
                        _ => Point::new(
                            content_bounds.width,
                            caret.y - state.scroll + caret.height * 0.5,
                        ),
                    };
                    shell.publish(Message::EditorCursor(state.hit(point), select));
                }
                action => shell.publish(Message::Edit(action)),
            }
            shell.invalidate_layout();
            shell.request_redraw();
        }
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
            shell.invalidate_layout();
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let content_bounds = layout.child(0).bounds();
        let Some(clip) = content_bounds.intersection(viewport) else {
            return;
        };
        let state = tree.state.downcast_ref::<State>();
        let focused = tree.children[0]
            .state
            .downcast_ref::<text_editor::State<text::highlighter::PlainText>>()
            .is_focused();
        let selection = self.content.cursor().selection.map(|p| {
            let a = source_offset(self.content, p);
            let b = source_offset(self.content, self.content.cursor().position);
            a.min(b)..a.max(b)
        });
        renderer.with_layer(clip, |renderer| {
            for (i, line) in state.lines.iter().enumerate() {
                if line.height == 0.0 {
                    continue;
                }
                let data = &state.document.lines[i];
                let origin = Point::new(
                    content_bounds.x + line.x,
                    content_bounds.y + line.y - state.scroll,
                );
                if origin.y > clip.y + clip.height || origin.y + line.height < clip.y {
                    continue;
                }
                if data.code || data.quote {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(origin.x - 6.0, origin.y),
                                Size::new(
                                    if data.quote {
                                        3.0
                                    } else {
                                        content_bounds.width - line.x
                                    },
                                    line.height,
                                ),
                            ),
                            ..Default::default()
                        },
                        theme
                            .extended_palette()
                            .background
                            .strong
                            .color
                            .scale_alpha(
                                data.opacity()
                                    * if data.quote {
                                        1.0 - data.prefix_visibility
                                    } else {
                                        1.0
                                    },
                            ),
                    );
                }
                if data.hidden {
                    continue;
                }
                if data.rule {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(origin.x, origin.y + line.height / 2.0),
                                Size::new(content_bounds.width, 1.0),
                            ),
                            ..Default::default()
                        },
                        theme
                            .extended_palette()
                            .background
                            .strong
                            .color
                            .scale_alpha(data.opacity()),
                    );
                    continue;
                }
                if let Some(image) = &line.image {
                    if line.image_size.height > 0.0 {
                        renderer.draw_image(
                            image::Image::new(image.clone()).opacity(data.opacity()),
                            Rectangle::new(origin, line.image_size),
                            clip,
                        );
                        continue;
                    }
                }
                if !line.cells.is_empty() {
                    for cell in &line.cells {
                        let origin = Point::new(content_bounds.x + cell.x, origin.y);
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(origin, Size::new(cell.width, line.height)),
                                border: Border {
                                    width: 1.0,
                                    color: theme
                                        .extended_palette()
                                        .background
                                        .strong
                                        .color
                                        .scale_alpha(data.opacity()),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            if data.table_header {
                                theme
                                    .extended_palette()
                                    .background
                                    .weak
                                    .color
                                    .scale_alpha(data.opacity())
                            } else {
                                Color::TRANSPARENT
                            },
                        );
                        let origin = origin + Vector::new(8.0, 6.0);
                        draw_tags(
                            renderer,
                            &cell.paragraph,
                            &cell.data,
                            origin,
                            self.tag_background.scale_alpha(data.opacity()),
                        );
                        if let Some(selection) = &selection {
                            draw_selection(
                                renderer,
                                &cell.paragraph,
                                &cell.data,
                                selection,
                                origin,
                                theme
                                    .extended_palette()
                                    .primary
                                    .weak
                                    .color
                                    .scale_alpha(data.opacity()),
                            );
                        }
                        renderer.fill_paragraph(
                            &cell.paragraph,
                            origin,
                            theme.palette().text,
                            clip,
                        );
                    }
                    continue;
                }
                draw_tags(
                    renderer,
                    &line.paragraph,
                    data,
                    origin,
                    self.tag_background.scale_alpha(data.opacity()),
                );
                if let Some(selection) = &selection {
                    draw_selection(
                        renderer,
                        &line.paragraph,
                        data,
                        selection,
                        origin,
                        theme
                            .extended_palette()
                            .primary
                            .weak
                            .color
                            .scale_alpha(data.opacity()),
                    );
                }
                if data.task.is_none() {
                    if let Some(bullet) = &data.bullet {
                        renderer.fill_text(
                            text::Text {
                                content: bullet.clone(),
                                bounds: Size::new(28.0, line.height),
                                size: self.size.into(),
                                line_height: text::LineHeight::Relative(1.5),
                                font: iced_m3::fonts::REGULAR,
                                align_x: text::Alignment::Default,
                                align_y: alignment::Vertical::Top,
                                shaping: text::Shaping::Advanced,
                                wrapping: text::Wrapping::None,
                            },
                            Point::new(origin.x - 26.0, origin.y),
                            theme
                                .palette()
                                .text
                                .scale_alpha(data.opacity() * (1.0 - data.prefix_visibility)),
                            clip,
                        );
                    }
                }
                renderer.fill_paragraph(&line.paragraph, origin, theme.palette().text, clip);
                let mut span = 0;
                for run in &data.runs {
                    if run.style.strike || run.style.link.is_some() {
                        for bounds in line.paragraph.span_bounds(span) {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle::new(
                                        origin
                                            + Vector::new(
                                                bounds.x,
                                                bounds.y
                                                    + bounds.height
                                                        * if run.style.strike { 0.5 } else { 0.9 },
                                            ),
                                        Size::new(bounds.width, 1.0),
                                    ),
                                    ..Default::default()
                                },
                                theme
                                    .palette()
                                    .text
                                    .scale_alpha(data.opacity() * run.visibility),
                            );
                        }
                    }
                    span += 1;
                }
            }
            for (i, (_, _, check)) in self.controls.iter().enumerate() {
                check.as_widget().draw(
                    &tree.children[i + 1],
                    renderer,
                    theme,
                    defaults,
                    layout.child(i + 1),
                    cursor,
                    &clip,
                );
            }
            if focused {
                let offset = source_offset(self.content, self.content.cursor().position);
                let caret = state.caret(offset);
                let opacity = state
                    .document
                    .lines
                    .iter()
                    .rev()
                    .find(|line| line.source.start <= offset)
                    .map_or(1.0, Line::opacity);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: content_bounds.x + caret.x,
                            y: content_bounds.y + caret.y - state.scroll,
                            ..caret
                        },
                        ..Default::default()
                    },
                    theme.palette().primary.scale_alpha(opacity),
                );
            }
        });
        if let (Some(viewport), Some(bar)) = (
            layout.bounds().intersection(viewport),
            Scrollbar::new(layout.bounds(), state.height, state.scroll),
        ) {
            renderer.with_layer(viewport, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: bar.thumb,
                        border: Border::default().rounded(3),
                        ..Default::default()
                    },
                    theme
                        .palette()
                        .text
                        .scale_alpha(if state.scrollbar_grab.is_some() {
                            0.65
                        } else if state.scrollbar_hovered {
                            0.5
                        } else {
                            0.35
                        }),
                );
            });
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.scrollbar_grab.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if layout
            .bounds()
            .intersection(viewport)
            .is_some_and(|bounds| cursor.is_over(bounds))
            && Scrollbar::new(layout.bounds(), state.height, state.scroll)
                .is_some_and(|bar| cursor.is_over(bar.track))
        {
            return mouse::Interaction::Grab;
        }
        for (i, (_, _, check)) in self.controls.iter().enumerate() {
            let interaction = check.as_widget().mouse_interaction(
                &tree.children[i + 1],
                layout.child(i + 1),
                cursor,
                viewport,
                renderer,
            );
            if interaction != mouse::Interaction::None {
                return interaction;
            }
        }
        if cursor.is_over(layout.child(0).bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.input.as_widget_mut().operate(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            operation,
        );
        for (i, (_, _, check)) in self.controls.iter_mut().enumerate() {
            check.as_widget_mut().operate(
                &mut tree.children[i + 1],
                layout.child(i + 1),
                renderer,
                operation,
            );
        }
    }
}

fn draw_selection(
    renderer: &mut Renderer,
    paragraph: &Paragraph,
    data: &Line,
    selection: &Range<usize>,
    origin: Point,
    color: Color,
) {
    for run in paragraph.buffer().layout_runs() {
        let mut spans: Vec<_> = run
            .glyphs
            .iter()
            .filter(|glyph| {
                data.source_at(glyph.end) > selection.start
                    && data.source_at(glyph.start) < selection.end
            })
            .map(|glyph| glyph.x..glyph.x + glyph.w)
            .collect();
        spans.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut merged: Vec<Range<f32>> = Vec::new();
        for span in spans {
            if let Some(previous) = merged
                .last_mut()
                .filter(|previous| span.start <= previous.end + 0.01)
            {
                previous.end = previous.end.max(span.end);
            } else {
                merged.push(span);
            }
        }
        // One fill per contiguous visual span avoids antialiased seams between
        // selected glyphs while retaining disjoint selections in bidirectional text.
        for span in merged {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        origin + Vector::new(span.start, run.line_top),
                        Size::new(span.end - span.start, run.line_height),
                    ),
                    ..Default::default()
                },
                color,
            );
        }
    }
}

fn draw_tags(
    renderer: &mut Renderer,
    paragraph: &Paragraph,
    line: &Line,
    origin: Point,
    color: Color,
) {
    for (index, run) in line
        .runs
        .iter()
        .enumerate()
        .filter(|(_, run)| run.style.tag)
    {
        for bounds in paragraph.span_bounds(index) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        origin + Vector::new(bounds.x - 1.0, bounds.y + 3.0),
                        Size::new(bounds.width + 2.0, (bounds.height - 6.0).max(1.0)),
                    ),
                    border: Border {
                        radius: 4.0.into(),
                        ..Border::default()
                    },
                    ..renderer::Quad::default()
                },
                color.scale_alpha(run.visibility),
            );
        }
    }
}

fn line_indent(line: &Line) -> f32 {
    let rendered = (if line.bullet.is_some() || line.task.is_some() {
        32.0
    } else if line.quote {
        18.0
    } else {
        0.0
    }) + line.indent.min(12) as f32 * 16.0;
    // Task boxes retain their own clickable gutter while their list/quote
    // prefix is edited. Other prefixes replace the rendered gutter entirely.
    let editing = if line.task.is_some() { 32.0 } else { 0.0 };
    rendered + (editing - rendered) * line.prefix_visibility
}

fn line_size(line: &Line, size: f32) -> f32 {
    size * match line.heading {
        Some(1) => 1.85,
        Some(2) => 1.5,
        Some(3) => 1.25,
        _ => 1.0,
    }
}

fn make_paragraph(line: &Line, size: f32, width: f32, theme: &Theme) -> Paragraph {
    let spans: Vec<text::Span<'_, (), Font>> = line
        .runs
        .iter()
        .map(|run| {
            let mut font = iced_m3::fonts::REGULAR;
            // Delimiters are annotations, not emphasized content. In nested
            // formatting they must not introduce a new font face (and baseline)
            // that is absent from the rendered content, e.g. ***`code`***.
            if (run.style.bold && !run.style.muted) || line.heading.is_some() || line.table_header {
                font.weight = font::Weight::Bold;
            }
            if run.style.italic && !run.style.muted {
                font.style = font::Style::Italic;
            }
            if run.style.code {
                font = Font {
                    weight: font::Weight::Medium,
                    ..Font::with_name("Fira Mono")
                };
            }
            let color = if run.style.muted {
                theme.extended_palette().secondary.base.color
            } else if run.style.tag || run.style.link.is_some() {
                theme.palette().primary
            } else {
                theme.palette().text
            };
            let span = text::Span::new(run.text.as_str())
                .font(font)
                .color(color.scale_alpha(run.visibility * line.opacity()));
            if run.visibility < 1.0 && !line.fence {
                let full_size = if run.style.tag { size * 0.9 } else { size };
                span.size((full_size * run.visibility).max(0.01))
                    .line_height(text::LineHeight::Absolute((size * 1.5).into()))
            } else if run.style.tag {
                span.size(size * 0.9)
                    .line_height(text::LineHeight::Absolute((size * 1.5).into()))
            } else {
                span
            }
        })
        .collect();
    Paragraph::with_spans(text::Text {
        content: &spans,
        bounds: Size::new(width.max(1.0), f32::INFINITY),
        size: size.into(),
        line_height: text::LineHeight::Relative(1.5),
        font: iced_m3::fonts::REGULAR,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::WordOrGlyph,
    })
}

fn caret_in(paragraph: &Paragraph, line: &Line, offset: usize, x: f32, y: f32) -> Rectangle {
    let display = line.display_at(offset);
    let mut position = Rectangle::new(Point::new(x, y), Size::new(1.5, paragraph.size().0 * 1.5));
    for run in paragraph.buffer().layout_runs() {
        for glyph in run.glyphs {
            // Respect glyph direction, including mixed-direction paragraphs.
            if display >= glyph.start && display <= glyph.end {
                let part = &line.text[glyph.start..glyph.end];
                let total = part.graphemes(true).count().max(1);
                let before = part
                    .grapheme_indices(true)
                    .take_while(|(i, _)| glyph.start + i < display)
                    .count();
                let fraction = before as f32 / total as f32;
                position.x = x
                    + glyph.x
                    + glyph.w
                        * if glyph.level.is_rtl() {
                            1.0 - fraction
                        } else {
                            fraction
                        };
                position.y = y + run.line_top;
                position.height = run.line_height;
                return position;
            }
            position = Rectangle::new(
                Point::new(x + glyph.x + glyph.w, y + run.line_top),
                Size::new(1.5, run.line_height),
            );
        }
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revealing_emphasis_keeps_vertical_metrics_stable() {
        super::super::fonts::ensure_loaded();
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(std::borrow::Cow::Borrowed(iced_m3::fonts::ROBOTO));
        for (source, before, after) in [
            ("Some **bold** text", 4, 5),
            ("Some******bold******", 3, 4),
            ("Some *italic* text", 4, 5),
            ("Some `code` text", 4, 5),
        ] {
            let metrics = |offset| {
                let document = Document::parse(source, Some(offset..offset));
                let paragraph = make_paragraph(&document.lines[0], 17.0, 600.0, &Theme::Light);
                paragraph
                    .buffer()
                    .layout_runs()
                    .map(|run| (run.line_top, run.line_y, run.line_height))
                    .collect::<Vec<_>>()
            };
            assert_eq!(metrics(before), metrics(after), "{source}");
        }
    }

    #[test]
    fn nested_code_reveal_keeps_vertical_metrics_stable() {
        super::super::fonts::ensure_loaded();
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(std::borrow::Cow::Borrowed(iced_m3::fonts::ROBOTO));
        for source in [
            "Text **`foo`** end",
            "Text *`foo`* end",
            "Text ***`foo`*** end",
            "CRXJS + Vite + React + Tailwind, with **`lwk_wasm`**",
        ] {
            let start = source.find('*').unwrap();
            let end = source.rfind('*').unwrap() + 1;
            for size in [12.0, 17.0, 28.0] {
                let metrics = |cursor| {
                    let document = Document::parse(source, cursor);
                    let paragraph = make_paragraph(&document.lines[0], size, 1600.0, &Theme::Light);
                    paragraph
                        .buffer()
                        .layout_runs()
                        .map(|run| (run.line_top, run.line_y, run.line_height))
                        .collect::<Vec<_>>()
                };
                let hidden = metrics(None);
                for offset in start - 1..=end {
                    assert_eq!(
                        metrics(Some(offset..offset)),
                        hidden,
                        "{source}, size {size}, cursor {offset}"
                    );
                }
            }
        }
    }
}
