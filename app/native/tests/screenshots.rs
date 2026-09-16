//! Render the production widget tree with bundled Roboto on the requested backend.
//! Baselines are reviewed files; missing baselines fail unless explicitly updating.
use comet_lib::ui::{fixtures, Comet, Dialog, Message, Panel};
use iced::{mouse, Color, Event, Rectangle, Settings, Size};
use iced_m3::{fonts, Theme};
use iced_test::{selector, Simulator};
#[path = "support/snapshot.rs"]
mod baseline;

fn snapshot(name: &str, size: (u16, u16), app: &Comet) {
    snapshot_with_focus(name, size, app, false);
}

fn simulator(app: &Comet, size: (u16, u16)) -> Simulator<'_, Message, Theme> {
    baseline::backend(); // Reject misspelled or ambiguous backend requests.
    Simulator::with_size(
        Settings {
            default_font: fonts::REGULAR,
            fonts: vec![fonts::ROBOTO.into()],
            ..Default::default()
        },
        Size::new(f32::from(size.0), f32::from(size.1)),
        app.view(),
    )
}

fn snapshot_with_focus(name: &str, size: (u16, u16), app: &Comet, focus: bool) {
    let mut ui = simulator(app, size);
    if focus {
        // The fixture already owns the desired source cursor. Give the real
        // editor focus to exercise delimiter reveal in its production layout.
        ui.click(iced_test::selector::id("markdown-editor"))
            .unwrap();
    }
    let snapshot = ui
        .snapshot(&app.theme().reduced_motion(true))
        .expect("requested renderer must produce a screenshot");
    if let Ok(fab) = ui.find(selector::id("new-note-fab")) {
        assert_fab_alignment(
            &snapshot,
            fab.bounds(),
            app.theme().colors.on_primary_container,
        );
    }
    baseline::assert_snapshot(name, |path| snapshot.matches_image(path).unwrap());
}

/// Check visible ink, not just layout boxes: font ascenders can put a text
/// glyph below the label even when both widgets claim to be centered.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Positive, rounded screenshot coordinates.
fn assert_fab_alignment(
    snapshot: &iced_test::simulator::Snapshot,
    bounds: Rectangle,
    foreground: Color,
) {
    let temporary = tempfile::tempdir().unwrap();
    let prefix = temporary.path().join("fab-alignment");
    snapshot.matches_image(&prefix).unwrap();
    let file = std::fs::File::open(baseline::image_path(&prefix)).unwrap();
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    let x = (bounds.x * 2.0).round() as usize;
    let y = (bounds.y * 2.0).round() as usize;
    let width = (bounds.width * 2.0).round() as usize;
    let height = (bounds.height * 2.0).round() as usize;
    let foreground = foreground.into_rgba8();
    let center_twice = |columns: std::ops::Range<usize>| {
        let mut top = height;
        let mut bottom = 0;
        for row in 0..height {
            for column in columns.clone() {
                let offset = ((y + row) * info.width as usize + x + column) * 4;
                if (0..3)
                    .all(|channel| pixels[offset + channel].abs_diff(foreground[channel]) <= 20)
                {
                    top = top.min(row);
                    bottom = bottom.max(row);
                }
            }
        }
        assert!(top <= bottom, "The icon and label must both be visible");
        top + bottom
    };
    // The icon and label occupy separate horizontal regions of the extended
    // FAB. Both visible centers should agree to within one logical pixel.
    let icon = center_twice(24..96);
    let label = center_twice(96..width - 24);
    assert!(
        icon.abs_diff(label) <= 4,
        "FAB icon and label are misaligned: doubled pixel centers {icon} and {label}"
    );
    assert!(
        icon.abs_diff(height - 1) <= 4,
        "FAB icon must be vertically centered in the button"
    );
}

#[test]
fn compact_editor_640x480() {
    snapshot("compact-editor-640x480", (640, 480), &fixtures::notebook());
}
#[test]
fn medium_notebook_1024x768() {
    snapshot(
        "medium-notebook-1024x768",
        (1024, 768),
        &fixtures::notebook(),
    );
}
#[test]
fn wide_notebook_1440x900() {
    snapshot("wide-notebook-1440x900", (1440, 900), &fixtures::notebook());
}
#[test]
fn compact_notes_640x480() {
    let mut app = fixtures::notebook();
    app.panel = Panel::Notes;
    snapshot("compact-notes-640x480", (640, 480), &app);
}

#[test]
fn new_note_fab_stays_anchored_and_clear_of_the_final_note() {
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        let mut app = fixtures::notebook();
        app.panel = Panel::Notes;
        let template = app.notes[0].clone();
        app.notes = (0..30)
            .map(|i| {
                let mut note = template.clone();
                note.id = format!("scroll-{i}");
                note.title = format!("Note {i:02}");
                note.preview = if i == 29 {
                    "Final note preview".into()
                } else {
                    "An idea worth keeping.".into()
                };
                note.pinned_at = None;
                note
            })
            .collect();
        app.total = app.notes.len();
        let mut ui = simulator(&app, size);
        let _ = ui.snapshot(&app.theme().reduced_motion(true)).unwrap();
        let before = ui.find(selector::id("new-note-fab")).unwrap().bounds();
        let list = ui.find(selector::id("notes-list")).unwrap().bounds();
        assert!((before.y + before.height - (f32::from(size.1) - 20.0)).abs() < 0.01);
        assert!((before.x + before.width - (list.x + list.width - 20.0)).abs() < 0.01);
        ui.point_at(list.center());
        ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines {
                x: 0.0,
                y: -10_000.0,
            },
        })]);
        let image = ui.snapshot(&app.theme().reduced_motion(true)).unwrap();
        let after = ui.find(selector::id("new-note-fab")).unwrap().bounds();
        assert_eq!(after, before, "Scrolling must not move the action");
        let final_preview = ui.find("Final note preview").unwrap();
        let visible = final_preview
            .visible_bounds()
            .expect("The final note is visible");
        assert_eq!(
            visible.size(),
            final_preview.bounds().size(),
            "The final note must not be clipped"
        );
        assert!(visible.y + visible.height < after.y - 12.0);
        baseline::assert_snapshot(&format!("notes-scrolled-{}x{}", size.0, size.1), |path| {
            image.matches_image(path).unwrap()
        });
        ui.click(selector::id("new-note-fab")).unwrap();
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            matches!(messages.as_slice(), [Message::New]),
            "The FAB creates directly, without opening a menu or selecting a note"
        );
    }
}

#[test]
fn notes_overflow_exposes_import_and_export_at_every_width() {
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        let mut app = fixtures::notebook();
        app.panel = Panel::Notes;
        for label in ["Import Markdown", "Export notes"] {
            let mut ui = simulator(&app, size);
            let _ = ui.snapshot(&app.theme().reduced_motion(true)).unwrap();
            ui.click("More").unwrap();
            // Settle the menu reveal and trigger ripple before comparing pixels.
            let _ = ui.snapshot(&app.theme().reduced_motion(true)).unwrap();
            ui.simulate([Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now() + std::time::Duration::from_secs(2),
            ))]);
            let image = ui.snapshot(&app.theme().reduced_motion(true)).unwrap();
            ui.find("Import Markdown").unwrap();
            ui.find("Export notes").unwrap();
            if label == "Import Markdown" {
                baseline::assert_snapshot(&format!("notes-menu-{}x{}", size.0, size.1), |path| {
                    image.matches_image(path).unwrap()
                });
            }
            ui.click(label).unwrap();
            let messages: Vec<_> = ui.into_messages().collect();
            assert!(matches!(
                (label, messages.as_slice()),
                ("Import Markdown", [Message::ImportMarkdown])
                    | ("Export notes", [Message::Export])
            ));
        }
    }
}
#[test]
fn dark_editor_1440x900() {
    let mut app = fixtures::notebook();
    app.dark = true;
    snapshot("dark-editor-1440x900", (1440, 900), &app);
}
#[test]
fn compact_settings_640x480() {
    let mut app = fixtures::notebook();
    app.dialog.show(Dialog::Settings);
    snapshot("compact-settings-640x480", (640, 480), &app);
}
#[test]
fn empty_notebook_1024x768() {
    snapshot("empty-notebook-1024x768", (1024, 768), &Comet::default());
}

fn formatting_fixture() -> Comet {
    let mut app = fixtures::notebook();
    app.editor = iced::widget::text_editor::Content::with_text(
        "# One editable document\n\n**foo** shows its markers at the cursor.\n\nOther **bold words**, *emphasis*, and [links](https://example.com) stay rendered.\n\n- [ ] Click to complete a task\n- [x] Markdown stays intact\n",
    );
    app.editor.move_to(iced::advanced::text::editor::Cursor {
        position: iced::advanced::text::editor::Position { line: 2, column: 4 },
        selection: None,
    });
    app
}

#[test]
fn formatting_at_cursor_640x480() {
    snapshot_with_focus(
        "formatting-at-cursor-640x480",
        (640, 480),
        &formatting_fixture(),
        true,
    );
}

#[test]
fn formatting_at_cursor_1440x900() {
    snapshot_with_focus(
        "formatting-at-cursor-1440x900",
        (1440, 900),
        &formatting_fixture(),
        true,
    );
}

#[test]
fn markdown_blocks_1024x768() {
    let mut app = fixtures::notebook();
    app.editor = iced::widget::text_editor::Content::with_text(&format!(
        "# Markdown in place\n\n```rust\nlet idea = \"Keep writing\";\n```\n\n| Feature | Status |\n| --- | --- |\n| **Editing** | Ready |\n| Task boxes | Clickable |\n\n> Keep the original text.\n\n![Sketch](attachment://{}.png)\n", "a".repeat(64)
    ));
    snapshot("markdown-blocks-1024x768", (1024, 768), &app);
}

#[test]
fn sidebar_tags_use_filter_chips_and_preserve_hierarchy() {
    for dark in [false, true] {
        for selected in [false, true] {
            let mut app = fixtures::notebook();
            app.dark = dark;
            let mut child = app.tags[0].clone();
            child.path = "personal/journal".into();
            child.label = "journal".into();
            child.depth = 1;
            child.inclusive_note_count = 1;
            app.tags[0].children.push(child);
            if selected {
                app.active_tag = Some("personal/journal".into());
            }
            let name = format!(
                "sidebar-tags-{}-{}",
                if dark { "dark" } else { "light" },
                if selected { "selected" } else { "resting" }
            );
            snapshot(&name, (1440, 900), &app);
            let mut ui = simulator(&app, (1440, 900));
            let parent = ui.find(selector::id("tag-chip-personal")).unwrap().bounds();
            let child = ui
                .find(selector::id("tag-chip-personal/journal"))
                .unwrap()
                .bounds();
            assert!((parent.height - 32.0).abs() < 0.01);
            assert!((child.height - 32.0).abs() < 0.01);
            assert!(child.x > parent.x, "Nested tags retain indentation");
            ui.click(selector::id("tag-chip-personal/journal")).unwrap();
            let messages: Vec<_> = ui.into_messages().collect();
            if selected {
                assert!(matches!(messages.as_slice(), [Message::Filter(_)]));
            } else {
                assert!(
                    matches!(messages.as_slice(), [Message::Tag(path)] if path == "personal/journal")
                );
            }
        }
    }
}
