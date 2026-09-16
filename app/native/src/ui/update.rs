use super::edit_history::{Kind, Snapshot};
use super::{
    commands, keyboard, text_editor, window, AppEvent, Comet, Dialog, Duration, ExportModeInput,
    ExportNotesInput, Instant, Message, NoteAction, NoteFilterInput, Panel, RenameTagInput,
    ResolveNoteConflictAction, SaveNoteInput, SetNoteReadonlyInput, Task,
};
use crate::{
    domain::relay::model::{PublishNoteInput, PublishShortNoteInput},
    error::AppError,
};
use iced::widget::text_editor::{Action, Edit, Motion};

impl Comet {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Record the outer user action once: a checkbox toggle includes an
        // internal selection, replacement and cursor restore in one transaction.
        let kind = match &message {
            Message::Edit(Action::Edit(Edit::Insert(_))) => Some(Kind::Typing),
            Message::Edit(Action::Edit(Edit::Backspace)) => Some(Kind::Backspace),
            Message::Edit(Action::Edit(Edit::Delete)) => Some(Kind::Delete),
            Message::Edit(Action::Edit(_))
            | Message::Insert(_)
            | Message::Format(..)
            | Message::ToggleTask(_)
            | Message::RestoreHistory(_)
            | Message::Attachment(Ok(Some(_))) => Some(Kind::Atomic),
            Message::Key(keyboard::Key::Character(key), modifiers)
                if modifiers.command() && matches!(key.to_lowercase().as_str(), "b" | "i") =>
            {
                Some(Kind::Atomic)
            }
            _ => None,
        };
        let before = kind.and_then(|kind| {
            self.selected
                .as_ref()
                .map(|note| (note.id.clone(), kind, Snapshot::capture(&self.editor)))
        });
        if matches!(
            &message,
            Message::EditorCursor(..)
                | Message::FindNext
                | Message::Show(_)
                | Message::ShowPanel(_)
        ) || matches!(&message, Message::Edit(action) if !action.is_edit() && !matches!(action, Action::Scroll { .. }))
        {
            self.edits.break_group();
        }
        let result = self.try_update(message);
        if let Some((id, kind, before)) = before {
            if self.selected.as_ref().is_some_and(|note| note.id == id) {
                self.edits.record(
                    &before,
                    &Snapshot::capture(&self.editor),
                    kind,
                    Instant::now(),
                );
            }
        }
        match result {
            Ok(task) => task,
            Err(error) => {
                self.error = Some(error.to_string());
                Task::none()
            }
        }
    }

    /// A save failure aborts navigation and keeps the editor buffer intact.
    pub(crate) fn save(&mut self) -> Result<(), AppError> {
        if !self.dirty {
            return Ok(());
        }
        let (Some(context), Some(note)) = (self.context.clone(), self.selected.as_ref()) else {
            return Ok(());
        };
        self.write_draft()?;
        let response = commands::notes::save_note_checked(
            context,
            SaveNoteInput {
                id: note.id.clone(),
                markdown: self.editor.text(),
                wikilink_resolutions: None,
            },
            &note.markdown,
        )?;
        self.selected = Some(response.note);
        self.dirty = false;
        self.clear_draft()?;
        self.refresh(false)?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Central Elm message reducer; effects use the typed command layer.
    fn try_update(&mut self, message: Message) -> Result<Task<Message>, AppError> {
        if self.busy
            && !matches!(
                &message,
                Message::Tick
                    | Message::AsyncFinished(_)
                    | Message::AccountChanged(_)
                    | Message::Imported(_)
                    | Message::Attachment(_)
                    | Message::HistoryLoaded(_)
                    | Message::ConflictLoaded(_)
                    | Message::DismissError
                    | Message::CloseWindow(_)
                    | Message::WindowOpened(_)
                    | Message::WindowScaleChanged(_)
            )
        {
            return Ok(Task::none());
        }
        match message {
            Message::WindowOpened(id) => {
                return Ok(window::scale_factor(id).map(Message::WindowScaleChanged));
            }
            Message::WindowScaleChanged(scale) => {
                if scale.is_finite() && scale > 0.0 {
                    self.window_scale_factor = scale;
                }
            }
            Message::FetchAttachment(hash) => {
                if let Some(ctx) = self.context.clone() {
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            commands::blob::fetch_blob(ctx, hash)
                                .await
                                .map(|status| match status {
                                    commands::blob::BlobFetchStatus::Downloaded => {
                                        "Attachment downloaded".into()
                                    }
                                    commands::blob::BlobFetchStatus::NeedsUnlock => {
                                        "Unlock your account to download attachments".into()
                                    }
                                    commands::blob::BlobFetchStatus::Missing => {
                                        "Attachment is unavailable on the configured server".into()
                                    }
                                })
                                .map_err(|e| e.to_string())
                        },
                        Message::AsyncFinished,
                    ));
                }
            }
            Message::OpenUrl(url) => {
                let parsed = url::Url::parse(&url).map_err(|e| AppError::custom(e.to_string()))?;
                if matches!(parsed.scheme(), "http" | "https" | "mailto") {
                    open::that_detached(url)?;
                }
            }
            Message::Tick => {
                if self.dirty
                    && self.edited_at.elapsed() >= Duration::from_millis(650)
                    && self.error.is_none()
                {
                    self.save()?;
                }
                if self
                    .search_changed
                    .is_some_and(|t| t.elapsed() >= Duration::from_millis(180))
                {
                    self.search_changed = None;
                    self.refresh(false)?;
                }
                let mut changes = vec![];
                if let Some(events) = &mut self.events {
                    loop {
                        match events.try_recv() {
                            Ok(event) => changes.push(event),
                            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                                changes.push(AppEvent::TagsChanged);
                            }
                            Err(_) => break,
                        }
                    }
                }
                for event in changes {
                    match event {
                        AppEvent::SyncStatus(state) => self.sync_state = state,
                        AppEvent::RemoteChange(change) => {
                            self.refresh(false)?;
                            if self
                                .selected
                                .as_ref()
                                .is_some_and(|n| n.id == change.note_id)
                            {
                                if self.dirty {
                                    self.error = Some("A remote update arrived while you were editing. Save a copy to keep your draft.".into());
                                } else if let Some(ctx) = self.context.clone() {
                                    match commands::notes::load_note(ctx, change.note_id) {
                                        Ok(note) => self.set_note(note),
                                        Err(_) if change.action == "delete" => {
                                            self.selected = None;
                                            self.panel = Panel::Notes;
                                        }
                                        Err(e) => return Err(e),
                                    }
                                }
                            }
                        }
                        AppEvent::TagsChanged => {
                            self.refresh(false)?;
                        }
                        AppEvent::SyncProgress => {}
                    }
                }
            }
            Message::Select(id) => {
                if self.busy {
                    return Ok(Task::none());
                }
                self.save()?;
                if let Some(context) = self.context.clone() {
                    self.set_note(commands::notes::load_note(context, id)?);
                }
                self.dialog.close();
            }
            Message::New => {
                if self.busy {
                    return Ok(Task::none());
                }
                self.save()?;
                if let Some(ctx) = self.context.clone() {
                    self.set_note(commands::notes::create_note(
                        ctx,
                        self.active_tag.iter().cloned().collect(),
                        None,
                    )?);
                    self.editor.perform(Action::Move(Motion::DocumentEnd));
                    self.filter = NoteFilterInput::All;
                    self.search.clear();
                    self.refresh(false)?;
                }
                self.dialog.close();
            }
            Message::Filter(filter) => {
                self.save()?;
                self.filter = filter;
                self.active_tag = None;
                self.search.clear();
                self.refresh(false)?;
                self.panel = Panel::Notes;
                self.dialog.close();
            }
            Message::Tag(tag) => {
                self.save()?;
                self.active_tag = Some(tag);
                self.search.clear();
                self.refresh(false)?;
                self.panel = Panel::Notes;
                self.dialog.close();
            }
            Message::Search(value) => {
                self.search = value;
                self.search_changed = Some(Instant::now());
            }
            Message::More => self.refresh(true)?,
            Message::Edit(action) => {
                if self.busy
                    || (action.is_edit()
                        && self
                            .selected
                            .as_ref()
                            .is_none_or(|n| n.readonly || n.deleted_at.is_some()))
                {
                    return Ok(Task::none());
                }
                let changed = action.is_edit();
                match action {
                    Action::SelectWord => {
                        super::markdown_editor::select_at_cursor(&mut self.editor, false);
                    }
                    Action::SelectLine => {
                        super::markdown_editor::select_at_cursor(&mut self.editor, true);
                    }
                    action => self.editor.perform(action),
                }
                if changed {
                    self.mark_edited()?;
                }
            }
            Message::Undo | Message::Redo => {
                if !self.dialog.is_open()
                    && self
                        .selected
                        .as_ref()
                        .is_some_and(|note| !note.readonly && note.deleted_at.is_none())
                {
                    if let Some(snapshot) = self.edits.travel(
                        Snapshot::capture(&self.editor),
                        matches!(message, Message::Redo),
                    ) {
                        snapshot.restore(&mut self.editor);
                        self.mark_edited()?;
                    }
                }
            }
            Message::EditorCursor(offset, select) => {
                super::markdown_editor::place_cursor(&mut self.editor, offset, select);
            }
            Message::ToggleTask(offset) => {
                let source = self.editor.text();
                if offset > 0
                    && source.as_bytes().get(offset - 1) == Some(&b'[')
                    && source.as_bytes().get(offset + 1) == Some(&b']')
                    && matches!(source.as_bytes().get(offset), Some(b' ' | b'x' | b'X'))
                    && self
                        .selected
                        .as_ref()
                        .is_some_and(|n| !n.readonly && n.deleted_at.is_none())
                {
                    let cursor = self.editor.cursor();
                    let replacement = if source.as_bytes()[offset] == b' ' {
                        "x"
                    } else {
                        " "
                    };
                    super::markdown_editor::place_cursor(&mut self.editor, offset, false);
                    super::markdown_editor::place_cursor(&mut self.editor, offset + 1, true);
                    let result = self.try_update(Message::Insert(replacement.into()));
                    self.editor.perform(Action::Move(Motion::DocumentStart));
                    self.editor.move_to(cursor);
                    return result;
                }
            }
            Message::Insert(value) => {
                return self.try_update(Message::Edit(Action::Edit(Edit::Paste(value.into()))))
            }
            Message::Format(before, after) => {
                let selection = self.editor.selection().unwrap_or_default();
                let task =
                    self.try_update(Message::Insert(format!("{before}{selection}{after}")))?;
                // Toolbar buttons temporarily take keyboard focus. Return it
                // to the document so typing and undo work immediately.
                return Ok(task.chain(iced::widget::operation::focus("markdown-editor")));
            }
            Message::Save => {
                self.save()?;
                self.notice = Some("Saved".into());
            }
            Message::ShowPanel(panel) => self.panel = panel,
            Message::Show(dialog) => {
                self.dialog.show(dialog);
                if dialog == Dialog::Settings {
                    self.refresh_settings()?;
                }
                if dialog == Dialog::RenameTag {
                    self.tag_name = self.active_tag.clone().unwrap_or_default();
                }
                if dialog == Dialog::History || dialog == Dialog::Conflict {
                    if let (Some(ctx), Some(note)) = (self.context.clone(), self.selected.as_ref())
                    {
                        let id = note.id.clone();
                        if dialog == Dialog::History {
                            return Ok(Task::perform(
                                async move {
                                    commands::notes::get_note_history(ctx, id)
                                        .map_err(|e| e.to_string())
                                },
                                Message::HistoryLoaded,
                            ));
                        }
                        return Ok(Task::perform(
                            async move {
                                commands::notes::get_note_conflict(ctx, id)
                                    .await
                                    .map_err(|e| e.to_string())
                            },
                            Message::ConflictLoaded,
                        ));
                    }
                }
            }
            Message::CloseDialog => {
                if !self.busy {
                    self.dialog.close();
                    self.import_key.clear();
                    self.access_key.clear();
                }
            }
            Message::Action(action) => {
                if self.busy {
                    return Ok(Task::none());
                }
                self.save()?;
                let (Some(ctx), Some(note)) = (self.context.clone(), self.selected.clone()) else {
                    return Ok(Task::none());
                };
                let changed = match action {
                    NoteAction::Pin => {
                        if note.pinned_at.is_some() {
                            commands::notes::unpin_note(ctx, note.id)?
                        } else {
                            commands::notes::pin_note(ctx, note.id)?
                        }
                    }
                    NoteAction::Archive => {
                        if note.archived_at.is_some() {
                            commands::notes::restore_note(ctx, note.id)?
                        } else {
                            commands::notes::archive_note(ctx, note.id)?
                        }
                    }
                    NoteAction::Trash => commands::notes::trash_note(ctx, note.id)?,
                    NoteAction::Restore => commands::notes::restore_from_trash(ctx, note.id)?,
                    NoteAction::Readonly => commands::notes::set_note_readonly(
                        ctx,
                        SetNoteReadonlyInput {
                            note_id: note.id,
                            readonly: !note.readonly,
                        },
                    )?,
                    NoteAction::Duplicate => commands::notes::duplicate_note(ctx, note.id)?,
                    NoteAction::Delete => {
                        commands::notes::delete_note_permanently(ctx, note.id)?;
                        self.selected = None;
                        self.panel = Panel::Notes;
                        self.dialog.close();
                        self.refresh(false)?;
                        return Ok(Task::none());
                    }
                };
                self.set_note(changed);
                self.refresh(false)?;
                self.dialog.close();
            }
            Message::Dark(dark) => {
                self.dark = dark;
                self.save_preferences()?;
            }
            Message::FontSize(size) => {
                self.font_size = size;
                self.save_preferences()?;
            }
            Message::EditorAnimations(enabled) => {
                self.editor_animations = enabled;
                self.save_preferences()?;
            }
            Message::ToggleFind => {
                self.find = if self.find.is_some() {
                    None
                } else {
                    Some(String::new())
                };
            }
            Message::Find(value) => self.find = Some(value),
            Message::FindNext => self.find_next(),
            Message::RelayUrl(s) => self.relay_url = s,
            Message::PublishRelayUrl(s) => self.publish_relay_url = s,
            Message::BlossomUrl(s) => self.blossom_url = s,
            Message::AccessKey(s) => self.access_key = s,
            Message::SaveSync => {
                if let Some(ctx) = self.context.clone() {
                    if !self.relay_url.trim().is_empty() {
                        commands::sync::set_sync_relay(ctx.clone(), self.relay_url.clone())?;
                        self.relay_url.clear();
                    }
                    if !self.publish_relay_url.trim().is_empty() {
                        commands::sync::add_publish_relay(
                            ctx.clone(),
                            self.publish_relay_url.clone(),
                        )?;
                        self.publish_relay_url.clear();
                    }
                    if self.blossom_url.trim().is_empty() {
                        commands::blob::remove_blossom_url(ctx.clone())?;
                    } else {
                        commands::blob::set_blossom_url(ctx.clone(), self.blossom_url.clone())?;
                    }
                    if !self.access_key.trim().is_empty() {
                        commands::sync::set_access_key(ctx, self.access_key.clone())?;
                        self.access_key.clear();
                    }
                    self.refresh_settings()?;
                    self.notice = Some("Connection settings saved".into());
                }
            }
            Message::SyncEnabled(enabled) => {
                if let Some(ctx) = self.context.clone() {
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            commands::sync::set_sync_enabled(ctx, enabled)
                                .await
                                .map(|()| "Sync settings saved".into())
                                .map_err(|e| e.to_string())
                        },
                        Message::AsyncFinished,
                    ));
                }
            }
            Message::Unlock => {
                if let Some(ctx) = self.context.clone() {
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            commands::sync::unlock_current_account(ctx)
                                .await
                                .map(|()| "Account unlocked".into())
                                .map_err(|e| e.to_string())
                        },
                        Message::AsyncFinished,
                    ));
                }
            }
            Message::RemoveRelay(url, kind) => {
                if let Some(ctx) = self.context.clone() {
                    if kind == "sync" {
                        commands::sync::remove_sync_relay(ctx, Some(url))?;
                    } else {
                        commands::sync::remove_relay(ctx, url, kind)?;
                    }
                    self.refresh_settings()?;
                }
            }
            Message::PreferRelay(url) => {
                if let Some(ctx) = self.context.clone() {
                    commands::sync::set_preferred_sync_relay(ctx, url)?;
                    self.refresh_settings()?;
                }
            }
            Message::PublishTitle(s) => self.publish_title = s,
            Message::PublishTags(s) => self.publish_tags = s,
            Message::Publish(short) => {
                self.save()?;
                if let (Some(ctx), Some(note)) = (self.context.clone(), &self.selected) {
                    let id = note.id.clone();
                    let title = self.publish_title.clone();
                    let tags = self
                        .publish_tags
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            let result = if short {
                                commands::sync::publish_short_note(
                                    ctx,
                                    PublishShortNoteInput { note_id: id, tags },
                                )
                                .await
                            } else {
                                commands::sync::publish_note(
                                    ctx,
                                    PublishNoteInput {
                                        note_id: id,
                                        title,
                                        tags,
                                        image: None,
                                    },
                                )
                                .await
                            };
                            result
                                .map(|r| {
                                    format!(
                                        "Published to {} of {} relays",
                                        r.success_count, r.relay_count
                                    )
                                })
                                .map_err(|e| e.to_string())
                        },
                        Message::AsyncFinished,
                    ));
                }
            }
            Message::Unpublish => {
                if let (Some(ctx), Some(note)) = (self.context.clone(), &self.selected) {
                    let id = note.id.clone();
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            commands::sync::delete_published_note(ctx, id)
                                .await
                                .map(|r| format!("Deletion sent to {} relays", r.success_count))
                                .map_err(|e| e.to_string())
                        },
                        Message::AsyncFinished,
                    ));
                }
            }
            Message::AsyncFinished(result) => {
                self.busy = false;
                match result {
                    Ok(notice) => {
                        self.notice = Some(notice);
                        self.refresh_settings()?;
                        self.refresh(false)?;
                        if !self.dirty {
                            if let (Some(ctx), Some(note)) =
                                (self.context.clone(), self.selected.as_ref())
                            {
                                let note = commands::notes::load_note(ctx, note.id.clone())?;
                                self.set_note(note);
                            }
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Message::SwitchAccount(key) => {
                self.save()?;
                if let Some(ctx) = self.context.clone() {
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            commands::accounts::switch_account(ctx, key)
                                .await
                                .map(|_| ())
                                .map_err(|e| e.to_string())
                        },
                        Message::AccountChanged,
                    ));
                }
            }
            Message::ImportKey(key) => self.import_key = key,
            Message::ImportAccount => {
                self.save()?;
                if let Some(ctx) = self.context.clone() {
                    let key = std::mem::take(&mut self.import_key);
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            commands::accounts::add_account(ctx, key, true)
                                .await
                                .map(|_| ())
                                .map_err(|e| e.to_string())
                        },
                        Message::AccountChanged,
                    ));
                }
            }
            Message::AccountName(name) => self.account_name = name,
            Message::RenameAccount => {
                if let (Some(ctx), Some(account)) = (
                    self.context.clone(),
                    self.accounts.iter().find(|a| a.is_active),
                ) {
                    commands::accounts::rename_account(
                        ctx,
                        account.public_key.clone(),
                        self.account_name.clone(),
                    )?;
                    self.refresh_settings()?;
                }
            }
            Message::StoreInKeychain => {
                if let Some(ctx) = self.context.clone() {
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            commands::accounts::move_secret_to_keychain(ctx)
                                .await
                                .map(|_| "Key stored in your OS keychain".into())
                                .map_err(|e| e.to_string())
                        },
                        Message::AsyncFinished,
                    ));
                }
            }
            Message::ImportMarkdown => {
                self.save()?;
                self.busy = true;
                return Ok(Task::perform(
                    async {
                        match rfd::AsyncFileDialog::new()
                            .add_filter("Markdown", &["md", "markdown", "txt"])
                            .pick_file()
                            .await
                        {
                            Some(file) => String::from_utf8(file.read().await)
                                .map(Some)
                                .map_err(|e| e.to_string()),
                            None => Ok(None),
                        }
                    },
                    Message::Imported,
                ));
            }
            Message::AccountChanged(result) => {
                self.busy = false;
                match result {
                    Ok(()) => {
                        self.filter = NoteFilterInput::All;
                        self.search.clear();
                        self.active_tag = None;
                        self.bootstrap()?;
                        self.restore_draft()?;
                        self.dialog.close();
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Message::Imported(result) => {
                self.busy = false;
                match result {
                    Ok(Some(markdown)) => {
                        if let Some(ctx) = self.context.clone() {
                            self.save()?;
                            self.set_note(commands::notes::create_note(
                                ctx,
                                vec![],
                                Some(markdown),
                            )?);
                            self.refresh(false)?;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.error = Some(e),
                }
            }
            Message::Export => {
                self.save()?;
                if let Some(ctx) = self.context.clone() {
                    self.busy = true;
                    let filter = self.filter;
                    let tag = self.active_tag.clone();
                    return Ok(Task::perform(
                        async move {
                            let Some(dir) = rfd::AsyncFileDialog::new().pick_folder().await else {
                                return Ok("Export cancelled".into());
                            };
                            commands::notes::export_notes(
                                ctx,
                                ExportNotesInput {
                                    export_mode: if tag.is_some() {
                                        ExportModeInput::Tag
                                    } else {
                                        ExportModeInput::NoteFilter
                                    },
                                    note_filter: Some(filter),
                                    tag_path: tag,
                                    preserve_tags: true,
                                    export_dir: dir.path().to_string_lossy().into_owned(),
                                },
                            )
                            .map(|n| format!("Exported {n} notes"))
                            .map_err(|e| e.to_string())
                        },
                        Message::AsyncFinished,
                    ));
                }
            }
            Message::AttachImage => {
                if let Some(ctx) = self.context.clone() {
                    self.busy = true;
                    return Ok(Task::perform(
                        async move {
                            let Some(file) = rfd::AsyncFileDialog::new()
                                .add_filter("Image", &["png", "jpg", "jpeg", "gif", "webp"])
                                .pick_file()
                                .await
                            else {
                                return Ok(None);
                            };
                            commands::app::import_image(
                                ctx,
                                file.path().to_string_lossy().into_owned(),
                            )
                            .map(|image| Some(format!("![image]({})", image.uri)))
                            .map_err(|e| e.to_string())
                        },
                        Message::Attachment,
                    ));
                }
            }
            Message::Attachment(result) => {
                self.busy = false;
                match result {
                    Ok(Some(value)) => return self.try_update(Message::Insert(value)),
                    Ok(None) => {}
                    Err(e) => self.error = Some(e),
                }
            }
            Message::HistoryLoaded(result) => match result {
                Ok(history) => {
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|n| n.id == history.note_id)
                    {
                        self.history = Some(history);
                    }
                }
                Err(e) => self.error = Some(e),
            },
            Message::RestoreHistory(index) => {
                let markdown = self
                    .history
                    .as_ref()
                    .and_then(|h| h.snapshots.get(index))
                    .and_then(|s| s.markdown.clone());
                if let Some(markdown) = markdown {
                    self.editor = text_editor::Content::with_text(&markdown);
                    self.dirty = true;
                    self.save()?;
                    self.dialog.close();
                }
            }
            Message::ConflictLoaded(result) => match result {
                Ok(conflict) => {
                    if conflict
                        .as_ref()
                        .is_none_or(|c| self.selected.as_ref().is_some_and(|n| n.id == c.note_id))
                    {
                        self.conflict = conflict;
                    }
                }
                Err(e) => self.error = Some(e),
            },
            Message::Resolve(index) => {
                if let (Some(ctx), Some(conflict)) = (self.context.clone(), self.conflict.as_ref())
                {
                    if let Some(snapshot) = conflict.snapshots.get(index) {
                        let note_id = conflict.note_id.clone();
                        let snapshot = snapshot.clone();
                        self.busy = true;
                        return Ok(Task::perform(
                            async move {
                                commands::notes::resolve_note_conflict(
                                    ctx,
                                    note_id,
                                    if snapshot.deleted_at.is_some() {
                                        ResolveNoteConflictAction::KeepDeleted
                                    } else {
                                        ResolveNoteConflictAction::Restore
                                    },
                                    snapshot.markdown,
                                    Some(snapshot.snapshot_id),
                                    Some(snapshot.wikilink_resolutions),
                                )
                                .await
                                .map(|()| "Conflict resolved".into())
                                .map_err(|e| e.to_string())
                            },
                            Message::AsyncFinished,
                        ));
                    }
                }
            }
            Message::SaveCopy => {
                if let Some(ctx) = self.context.clone() {
                    self.set_note(commands::notes::create_note(
                        ctx,
                        vec![],
                        Some(self.editor.text()),
                    )?);
                    self.clear_draft()?;
                    self.error = None;
                    self.refresh(false)?;
                }
            }
            Message::RenameTag(name) => self.tag_name = name,
            Message::ConfirmRenameTag => {
                if let (Some(ctx), Some(from)) = (self.context.clone(), self.active_tag.clone()) {
                    self.save()?;
                    commands::notes::rename_tag(
                        ctx,
                        RenameTagInput {
                            from_path: from,
                            to_path: self.tag_name.clone(),
                        },
                    )?;
                    self.active_tag = Some(self.tag_name.clone());
                    self.refresh(false)?;
                    self.dialog.close();
                }
            }
            Message::DismissError => self.error = None,
            Message::CloseWindow(id) => {
                if self.busy {
                    self.error = Some("Please wait for the current operation to finish.".into());
                } else {
                    self.save()?;
                    return Ok(window::close(id));
                }
            }
            Message::Key(key, modifiers) => {
                if self.dialog.is_open() {
                    return Ok(Task::none());
                }
                if modifiers.command() {
                    if let keyboard::Key::Character(c) = key {
                        return self.try_update(match c.to_lowercase().as_str() {
                            "n" => Message::New,
                            "s" => Message::Save,
                            "k" | "o" => Message::Show(Dialog::Palette),
                            "f" => Message::ToggleFind,
                            "," => Message::Show(Dialog::Settings),
                            "b" => Message::Format("**", "**"),
                            "i" => Message::Format("*", "*"),
                            _ => return Ok(Task::none()),
                        });
                    }
                }
            }
        }
        Ok(Task::none())
    }
    fn find_next(&mut self) {
        self.edits.break_group();
        let Some(query) = self.find.as_ref().filter(|q| !q.is_empty()) else {
            return;
        };
        let text = self.editor.text();
        let start =
            super::markdown_editor::source_offset(&self.editor, self.editor.cursor().position);
        let Some(found) = text[start..]
            .find(query)
            .map(|i| i + start)
            .or_else(|| text[..start].find(query))
        else {
            self.notice = Some("No matches in this note".into());
            return;
        };
        let end = found + query.len();
        super::markdown_editor::place_cursor(&mut self.editor, found, false);
        super::markdown_editor::place_cursor(&mut self.editor, end, true);
    }

    fn mark_edited(&mut self) -> Result<(), AppError> {
        self.dirty = self
            .selected
            .as_ref()
            .is_some_and(|note| note.markdown != self.editor.text());
        self.edited_at = Instant::now();
        self.notice = None;
        if self.dirty {
            self.write_draft()
        } else {
            self.clear_draft()
        }
    }
}
