use super::*;
use iced::widget::text_editor::{Action, Edit};

fn app() -> (tempfile::TempDir, Comet) {
    let dir = tempfile::tempdir().unwrap();
    let context = AppContext::new(dir.path().into());
    let (mut app, _) = Comet::new(context);
    assert!(app.error.is_none(), "{:?}", app.error);
    let _ = app.update(Message::New);
    assert!(app.selected.is_some());
    let _ = app.update(Message::Edit(Action::SelectAll));
    (dir, app)
}
#[tokio::test]
async fn markdown_round_trips_without_normalizing_structure() {
    let (_dir, mut app) = app();
    let source="# Café\r\n\r\n- [ ] **Task**\r\n\r\n| A | B |\r\n| - | - |\r\n| 1 | 2 |\r\n\r\n[[Other note]] #projects/native\r\n";
    app.editor = text_editor::Content::with_text(source);
    app.dirty = true;
    app.save().unwrap();
    let note = commands::notes::load_note(
        app.context.clone().unwrap(),
        app.selected.as_ref().unwrap().id.clone(),
    )
    .unwrap();
    assert_eq!(note.markdown, source);
}
#[tokio::test]
async fn navigation_flushes_edits_and_preserves_the_previous_note() {
    let (_dir, mut app) = app();
    let first = app.selected.as_ref().unwrap().id.clone();
    let _ = app.update(Message::Edit(Action::Edit(Edit::Paste(
        String::from("First note").into(),
    ))));
    let _ = app.update(Message::New);
    let second = app.selected.as_ref().unwrap().id.clone();
    assert_ne!(first, second);
    let _ = app.update(Message::Select(first));
    assert_eq!(app.editor.text(), "First note");
}

#[tokio::test]
async fn undo_survives_autosave_and_persists_the_reverted_markdown() {
    let (_dir, mut app) = app();
    let source = "**café**\r\n- [X] Keep spacing  \r\n";
    let _ = app.update(Message::Insert(source.into()));
    app.save().unwrap();
    let _ = app.update(Message::Edit(Action::Move(
        text_editor::Motion::DocumentEnd,
    )));
    let _ = app.update(Message::Insert("more".into()));
    app.edited_at -= Duration::from_secs(1);
    let _ = app.update(Message::Tick);
    assert!(!app.dirty);
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor.text(), source);
    assert!(app.dirty);
    app.save().unwrap();
    let note = commands::notes::load_note(
        app.context.clone().unwrap(),
        app.selected.as_ref().unwrap().id.clone(),
    )
    .unwrap();
    assert_eq!(note.markdown, source);
    let _ = app.update(Message::Redo);
    assert_eq!(app.editor.text(), format!("{source}more"));
    assert!(app.error.is_none());
}

#[tokio::test]
async fn undo_history_stays_with_each_note_when_switching() {
    let (_dir, mut app) = app();
    let first = app.selected.as_ref().unwrap().id.clone();
    let original = app.editor.text();
    let _ = app.update(Message::Insert("First".into()));
    let _ = app.update(Message::New);
    let second = app.selected.as_ref().unwrap().id.clone();
    let _ = app.update(Message::Edit(Action::SelectAll));
    let _ = app.update(Message::Insert("Second".into()));
    let _ = app.update(Message::Select(first.clone()));
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor.text(), original);
    let _ = app.update(Message::Select(second));
    assert_eq!(app.editor.text(), "Second");
    let _ = app.update(Message::Undo);
    assert_ne!(app.editor.text(), "Second");
    let _ = app.update(Message::Select(first));
    let _ = app.update(Message::Redo);
    assert_eq!(app.editor.text(), "First");
    assert!(app.error.is_none());
}

#[tokio::test]
async fn undo_updates_recoverable_drafts_and_clears_them_at_the_saved_version() {
    let (dir, mut app) = app();
    let original = app.editor.text();
    let _ = app.update(Message::Insert("First".into()));
    let _ = app.update(Message::Insert("Second".into()));
    let _ = app.update(Message::Undo);
    let (recovered, _) = Comet::new(AppContext::new(dir.path().into()));
    assert_eq!(recovered.editor.text(), "First");
    drop(recovered);
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor.text(), original);
    assert!(!app.dirty);
    let (reopened, _) = Comet::new(AppContext::new(dir.path().into()));
    assert_eq!(reopened.editor.text(), original);
    assert!(!reopened.dirty);
}

#[tokio::test]
async fn a_failed_draft_write_does_not_drop_the_undo_transaction() {
    let (_dir, mut app) = app();
    let original = app.editor.text();
    let root = crate::db::active_account_dir(app.context.as_ref().unwrap()).unwrap();
    let temporary = root.join("native-draft.json.tmp");
    std::fs::create_dir(&temporary).unwrap();
    let _ = app.update(Message::Insert("unsaved".into()));
    assert!(app.error.is_some());
    assert_eq!(app.editor.text(), "unsaved");
    std::fs::remove_dir(temporary).unwrap();
    let _ = app.update(Message::DismissError);
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor.text(), original);
    assert!(!app.dirty);
    let _ = app.update(Message::Redo);
    assert_eq!(app.editor.text(), "unsaved");
    assert!(app.error.is_none());
}

#[tokio::test]
async fn undo_respects_locks_dialogs_and_account_reset() {
    let (_dir, mut app) = app();
    let _ = app.update(Message::Insert("Keep".into()));
    let _ = app.update(Message::Action(NoteAction::Readonly));
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor.text(), "Keep");
    let _ = app.update(Message::Action(NoteAction::Readonly));
    app.dialog.show(Dialog::Settings);
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor.text(), "Keep");
    app.dialog.close();
    app.bootstrap().unwrap();
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor.text(), "Keep");
}
#[tokio::test]
async fn remote_changes_block_save_and_navigation_without_losing_the_draft() {
    let (_dir, mut app) = app();
    let id = app.selected.as_ref().unwrap().id.clone();
    let _ = app.update(Message::Edit(Action::Edit(Edit::Paste(
        String::from("My unsaved idea").into(),
    ))));
    commands::notes::save_note(
        app.context.clone().unwrap(),
        SaveNoteInput {
            id: id.clone(),
            markdown: "Changed elsewhere".into(),
            wikilink_resolutions: None,
        },
    )
    .unwrap();
    let _ = app.update(Message::New);
    assert_eq!(app.selected.as_ref().unwrap().id, id);
    assert_eq!(app.editor.text(), "My unsaved idea");
    assert!(app.dirty && app.error.is_some());
    let _ = app.update(Message::SaveCopy);
    assert_ne!(app.selected.as_ref().unwrap().id, id);
    assert_eq!(app.editor.text(), "My unsaved idea");
    assert_eq!(
        commands::notes::load_note(app.context.clone().unwrap(), id)
            .unwrap()
            .markdown,
        "Changed elsewhere"
    );
}
#[tokio::test]
async fn read_only_notes_reject_editor_changes() {
    let (_dir, mut app) = app();
    let _ = app.update(Message::Action(NoteAction::Readonly));
    let before = app.editor.text();
    let _ = app.update(Message::Insert("should not appear".into()));
    assert_eq!(app.editor.text(), before);
    assert!(!app.dirty);
}
#[test]
fn native_controls_emit_real_navigation_messages() {
    let app = fixtures::notebook();
    let mut ui = iced_test::Simulator::with_size(
        iced::Settings {
            default_font: iced_m3::fonts::REGULAR,
            ..Default::default()
        },
        iced::Size::new(1440.0, 900.0),
        app.view(),
    );
    ui.click("Pinned").unwrap();
    assert!(ui
        .into_messages()
        .any(|m| matches!(m, Message::Filter(NoteFilterInput::Pinned))));
}

#[tokio::test]
async fn an_interrupted_edit_is_recovered_after_restart() {
    let (dir, mut app) = app();
    let _ = app.update(Message::Insert("A recoverable thought".into()));
    let id = app.selected.as_ref().unwrap().id.clone();
    drop(app);
    let (mut recovered, _) = Comet::new(AppContext::new(dir.path().into()));
    assert_eq!(recovered.selected.as_ref().unwrap().id, id);
    assert_eq!(recovered.editor.text(), "A recoverable thought");
    assert!(recovered.dirty);
    recovered.save().unwrap();
    drop(recovered);
    let (reopened, _) = Comet::new(AppContext::new(dir.path().into()));
    assert_eq!(reopened.editor.text(), "A recoverable thought");
    assert!(!reopened.dirty);
}

#[test]
fn compact_settings_keeps_done_visible() {
    let mut app = fixtures::notebook();
    app.dialog.show(Dialog::Settings);
    let mut ui = iced_test::Simulator::with_size(
        iced::Settings {
            default_font: iced_m3::fonts::REGULAR,
            ..Default::default()
        },
        iced::Size::new(640.0, 480.0),
        app.view(),
    );
    let _ = ui.snapshot(&app.theme());
    let bounds = ui.find("Done").unwrap().bounds();
    assert!(
        bounds.y >= 0.0 && bounds.y + bounds.height <= 480.0,
        "{bounds:?}"
    );
    ui.click("Done").unwrap();
    assert!(ui
        .into_messages()
        .any(|m| matches!(m, Message::CloseDialog)));
}

#[tokio::test]
async fn task_click_changes_only_the_markdown_checkbox_and_preserves_cursor() {
    let (_dir, mut app) = app();
    let source = "# Café\r\n\r\n- [ ] **Ship** the editor\r\n- [X] Keep spacing  \r\n";
    app.editor = text_editor::Content::with_text(source);
    let offset = source.find("[ ]").unwrap() + 1;
    markdown_editor::place_cursor(&mut app.editor, source.find("editor").unwrap(), false);
    let cursor = app.editor.cursor();
    let _ = app.update(Message::ToggleTask(offset));
    assert_eq!(app.editor.text(), source.replacen("[ ]", "[x]", 1));
    assert_eq!(app.editor.cursor(), cursor);
    assert!(app.dirty);
    app.save().unwrap();
    let _ = app.update(Message::ToggleTask(offset));
    assert_eq!(app.editor.text(), source);
    let _ = app.update(Message::Action(NoteAction::Readonly));
    let _ = app.update(Message::ToggleTask(offset));
    assert_eq!(app.editor.text(), source);
}

#[test]
fn material_task_checkbox_emits_the_source_edit() {
    let content = text_editor::Content::with_text("- [ ] Ship it");
    let mut ui = iced_test::Simulator::with_size(
        iced::Settings {
            default_font: iced_m3::fonts::REGULAR,
            fonts: vec![iced_m3::fonts::ROBOTO.into()],
            ..Default::default()
        },
        iced::Size::new(640.0, 480.0),
        markdown_editor::editor(
            &content,
            "test",
            17.0,
            fixtures::notebook().theme(),
            true,
            None,
        ),
    );
    let bounds = ui
        .find(iced_test::selector::id("markdown-editor"))
        .unwrap()
        .bounds();
    ui.point_at((bounds.x + 10.0, bounds.y + 12.0));
    ui.simulate(iced_test::simulator::click());
    assert!(ui
        .into_messages()
        .any(|message| matches!(message, Message::ToggleTask(3))));
}

#[test]
fn rendered_text_clicks_resolve_to_source_positions() {
    let mut content = text_editor::Content::with_text("**foo** and *bar*");
    let mut ui = iced_test::Simulator::with_size(
        iced::Settings {
            default_font: iced_m3::fonts::REGULAR,
            fonts: vec![iced_m3::fonts::ROBOTO.into()],
            ..Default::default()
        },
        iced::Size::new(640.0, 480.0),
        markdown_editor::editor(
            &content,
            "test",
            17.0,
            fixtures::notebook().theme(),
            true,
            None,
        ),
    );
    let bounds = ui
        .find(iced_test::selector::id("markdown-editor"))
        .unwrap()
        .bounds();
    ui.point_at((bounds.x + 14.0, bounds.y + 10.0));
    ui.simulate(iced_test::simulator::click());
    let offset = ui
        .into_messages()
        .find_map(|message| match message {
            Message::EditorCursor(offset, _) => Some(offset),
            _ => None,
        })
        .unwrap();
    assert!((2..=5).contains(&offset), "source offset: {offset}");
    markdown_editor::place_cursor(&mut content, offset, false);
    let visible = markdown_editor::document::Document::parse(&content.text(), Some(offset..offset));
    assert_eq!(visible.lines[0].text, "**foo** and bar");
    content.perform(Action::Edit(Edit::Insert('!')));
    assert_eq!(
        content.text(),
        format!(
            "{}!{}",
            &"**foo** and *bar*"[..offset],
            &"**foo** and *bar*"[offset..]
        )
    );
}

#[test]
fn source_cursor_uses_utf8_bytes_and_clears_old_selections() {
    let source = "Café 👩‍💻\r\n**second**\r\n";
    let mut content = text_editor::Content::with_text(source);
    let offset = source.find("second").unwrap() + 2;
    markdown_editor::place_cursor(&mut content, offset, false);
    assert_eq!(
        markdown_editor::source_offset(&content, content.cursor().position),
        offset
    );
    markdown_editor::place_cursor(&mut content, offset + 3, true);
    assert_eq!(content.selection().as_deref(), Some("con"));
    markdown_editor::place_cursor(&mut content, offset, false);
    assert!(content.selection().is_none());
    assert_eq!(content.text(), source);
}

#[test]
fn wrapped_lines_map_clicks_back_to_source_at_different_widths() {
    let source = "**Starting here** café writing more words across wrapped lines. ".repeat(10);
    let hit = |width| {
        let content = text_editor::Content::with_text(&source);
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings {
                default_font: iced_m3::fonts::REGULAR,
                fonts: vec![iced_m3::fonts::ROBOTO.into()],
                ..Default::default()
            },
            iced::Size::new(width, 480.0),
            markdown_editor::editor(
                &content,
                "test",
                17.0,
                fixtures::notebook().theme(),
                true,
                None,
            ),
        );
        let bounds = ui
            .find(iced_test::selector::id("markdown-editor"))
            .unwrap()
            .bounds();
        ui.point_at((bounds.x + 10.0, bounds.y + 60.0));
        ui.simulate(iced_test::simulator::click());
        ui.into_messages()
            .find_map(|message| match message {
                Message::EditorCursor(offset, _) => Some(offset),
                _ => None,
            })
            .unwrap()
    };
    let narrow = hit(180.0);
    let wide = hit(420.0);
    assert!(narrow > 0 && wide > narrow, "{narrow}, {wide}");
    assert!(source.is_char_boundary(narrow) && source.is_char_boundary(wide));
}

#[test]
fn find_selects_unicode_without_changing_markdown() {
    let source = "Café 👩‍💻\r\n**A café idea**\r\n";
    let mut app = Comet::default();
    app.editor = text_editor::Content::with_text(source);
    markdown_editor::place_cursor(&mut app.editor, "Café ".len(), false);
    let _ = app.update(Message::Find("café".into()));
    let _ = app.update(Message::FindNext);
    assert_eq!(app.editor.selection().as_deref(), Some("café"));
    assert_eq!(app.editor.text(), source);
}
