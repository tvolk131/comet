//! The native desktop UI. Markdown is the editor's canonical value.
mod draft;
mod edit_history;
pub mod fixtures;
mod fonts;
mod markdown_editor;
mod scroll_input;
#[cfg(test)]
mod tests;
mod theme;
mod update;
mod view;

use crate::{
    commands,
    domain::{
        accounts::model::AccountSummary,
        notes::model::{
            ContextualTagNode, ContextualTagsInput, ExportModeInput, ExportNotesInput, LoadedNote,
            NoteConflictInfo, NoteFilterInput, NoteHistoryInfo, NoteQueryInput, NoteSortDirection,
            NoteSortField, NoteSummary, RenameTagInput, ResolveNoteConflictAction, SaveNoteInput,
            SetNoteReadonlyInput,
        },
        relay::model::Relay,
        sync::model::SyncState,
    },
    runtime::{AppContext, AppEvent},
};
use iced::{keyboard, widget::text_editor, window, Subscription, Task};
use iced_m3::{Element, Theme};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Notes,
    Editor,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Navigation,
    Settings,
    Palette,
    Publish,
    History,
    Delete,
    Conflict,
    RenameTag,
}

/// Keep the last dialog mounted so the host can finish its exit animation.
#[derive(Debug, Default)]
pub struct DialogState {
    content: Option<Dialog>,
    open: bool,
}

impl DialogState {
    pub fn show(&mut self, dialog: Dialog) {
        self.content = Some(dialog);
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NoteAction {
    Pin,
    Archive,
    Trash,
    Restore,
    Readonly,
    Duplicate,
    Delete,
}
#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    WindowOpened(window::Id),
    WindowScaleChanged(f32),
    FetchAttachment(String),
    OpenUrl(String),
    Select(String),
    New,
    Filter(NoteFilterInput),
    Tag(String),
    Search(String),
    More,
    Edit(text_editor::Action),
    Undo,
    Redo,
    EditorCursor(usize, bool),
    ToggleTask(usize),
    Insert(String),
    Format(&'static str, &'static str),
    Save,
    ShowPanel(Panel),
    Show(Dialog),
    CloseDialog,
    Action(NoteAction),
    Dark(bool),
    EditorAnimations(bool),
    FontSize(f32),
    Find(String),
    ToggleFind,
    FindNext,
    RelayUrl(String),
    PublishRelayUrl(String),
    BlossomUrl(String),
    AccessKey(String),
    SaveSync,
    SyncEnabled(bool),
    Unlock,
    RemoveRelay(String, String),
    PreferRelay(String),
    PublishTitle(String),
    PublishTags(String),
    Publish(bool),
    Unpublish,
    AsyncFinished(Result<String, String>),
    SwitchAccount(String),
    ImportKey(String),
    ImportAccount,
    AccountName(String),
    RenameAccount,
    StoreInKeychain,
    ImportMarkdown,
    Export,
    AttachImage,
    Imported(Result<Option<String>, String>),
    AccountChanged(Result<(), String>),
    Attachment(Result<Option<String>, String>),
    HistoryLoaded(Result<NoteHistoryInfo, String>),
    RestoreHistory(usize),
    ConflictLoaded(Result<Option<NoteConflictInfo>, String>),
    Resolve(usize),
    SaveCopy,
    RenameTag(String),
    ConfirmRenameTag,
    DismissError,
    CloseWindow(window::Id),
    Key(keyboard::Key, keyboard::Modifiers),
}

#[allow(clippy::struct_excessive_bools)] // Independent view preferences and UI flags.
pub struct Comet {
    pub(crate) context: Option<AppContext>,
    pub(crate) events: Option<tokio::sync::broadcast::Receiver<AppEvent>>,
    pub notes: Vec<NoteSummary>,
    pub tags: Vec<ContextualTagNode>,
    pub selected: Option<LoadedNote>,
    pub editor: text_editor::Content,
    edits: edit_history::Histories,
    pub(crate) attachment_dir: Option<std::path::PathBuf>,
    pub filter: NoteFilterInput,
    pub search: String,
    pub active_tag: Option<String>,
    pub total: usize,
    pub has_more: bool,
    pub panel: Panel,
    pub dialog: DialogState,
    pub dark: bool,
    pub reduced_motion: bool,
    pub editor_animations: bool,
    pub font_size: f32,
    pub window_scale_factor: f32,
    pub dirty: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub busy: bool,
    pub sync_state: SyncState,
    pub sync_enabled: bool,
    pub relays: Vec<Relay>,
    pub relay_url: String,
    pub publish_relay_url: String,
    pub blossom_url: String,
    pub access_key: String,
    pub accounts: Vec<AccountSummary>,
    pub account_name: String,
    pub import_key: String,
    pub publish_title: String,
    pub publish_tags: String,
    pub find: Option<String>,
    pub history: Option<NoteHistoryInfo>,
    pub conflict: Option<NoteConflictInfo>,
    pub tag_name: String,
    pub(crate) edited_at: Instant,
    pub(crate) search_changed: Option<Instant>,
}

impl Default for Comet {
    fn default() -> Self {
        Self {
            context: None,
            events: None,
            notes: vec![],
            tags: vec![],
            selected: None,
            editor: text_editor::Content::new(),
            edits: edit_history::Histories::default(),
            attachment_dir: None,
            filter: NoteFilterInput::All,
            search: String::new(),
            active_tag: None,
            total: 0,
            has_more: false,
            panel: Panel::Notes,
            dialog: DialogState::default(),
            dark: false,
            reduced_motion: false,
            editor_animations: true,
            font_size: 17.0,
            window_scale_factor: 1.0,
            dirty: false,
            error: None,
            notice: None,
            busy: false,
            sync_state: SyncState::Disconnected,
            sync_enabled: false,
            relays: vec![],
            relay_url: String::new(),
            publish_relay_url: String::new(),
            blossom_url: String::new(),
            access_key: String::new(),
            accounts: vec![],
            account_name: String::new(),
            import_key: String::new(),
            publish_title: String::new(),
            publish_tags: String::new(),
            find: None,
            history: None,
            conflict: None,
            tag_name: String::new(),
            edited_at: Instant::now(),
            search_changed: None,
        }
    }
}

impl Comet {
    pub fn new(context: AppContext) -> (Self, Task<Message>) {
        let mut app = Self {
            events: Some(context.subscribe()),
            context: Some(context.clone()),
            reduced_motion: theme::system_reduced_motion(),
            ..Self::default()
        };
        if let Err(error) = app.bootstrap() {
            app.error = Some(error.to_string());
        }
        app.load_preferences();
        if let Err(error) = app.restore_draft() {
            app.error = Some(error.to_string());
        }
        let task = Task::perform(
            async move {
                crate::adapters::nostr::sync_manager::auto_start(&context).await;
            },
            |()| Message::Tick,
        );
        (app, task)
    }
    pub fn theme(&self) -> Theme {
        theme::theme(self.dark).reduced_motion(self.reduced_motion)
    }
    fn editor_theme(&self) -> Theme {
        theme::theme(self.dark).reduced_motion(self.reduced_motion || !self.editor_animations)
    }
    pub fn view(&self) -> Element<'_, Message> {
        view::view(self)
    }
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick),
            window::close_requests().map(Message::CloseWindow),
            window::events().filter_map(|(id, event)| match event {
                window::Event::Opened { .. } => Some(Message::WindowOpened(id)),
                window::Event::Rescaled(scale) => Some(Message::WindowScaleChanged(scale)),
                _ => None,
            }),
            keyboard::listen().filter_map(|event| match event {
                keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    Some(Message::Key(key, modifiers))
                }
                _ => None,
            }),
        ])
    }
    pub(crate) fn set_note(&mut self, note: LoadedNote) {
        self.edits.open(&note.id, &note.markdown);
        self.editor = text_editor::Content::with_text(&note.markdown);
        self.publish_title.clone_from(&note.title);
        self.publish_tags = note.tags.join(", ");
        self.attachment_dir = self
            .context
            .as_ref()
            .and_then(|ctx| crate::db::active_account_attachments_dir(ctx).ok());
        self.selected = Some(note);
        self.dirty = false;
        self.panel = Panel::Editor;
        self.history = None;
        self.conflict = None;
    }
    fn bootstrap(&mut self) -> Result<(), crate::error::AppError> {
        let Some(context) = self.context.clone() else {
            return Ok(());
        };
        crate::db::init_database(&context)?;
        let data = commands::notes::bootstrap(context.clone())?;
        self.notes = data.initial_notes.notes;
        self.total = data.initial_notes.total_count;
        self.has_more = data.initial_notes.has_more;
        self.tags = data.initial_tags.roots;
        self.refresh_settings()?;
        self.selected = None;
        self.edits = edit_history::Histories::default();
        self.editor = text_editor::Content::new();
        if let Some(id) = data
            .selected_note_id
            .or_else(|| self.notes.first().map(|n| n.id.clone()))
        {
            self.set_note(commands::notes::load_note(context, id)?);
        }
        Ok(())
    }
    fn refresh(&mut self, append: bool) -> Result<(), crate::error::AppError> {
        let Some(context) = self.context.clone() else {
            return Ok(());
        };
        let result = commands::notes::query_notes(
            context.clone(),
            NoteQueryInput {
                note_filter: self.filter,
                search_query: self.search.clone(),
                active_tag_path: self.active_tag.clone(),
                limit: if append { 40 } else { self.notes.len().max(40) },
                offset: if append { self.notes.len() } else { 0 },
                sort_field: NoteSortField::ModifiedAt,
                sort_direction: NoteSortDirection::Newest,
            },
        )?;
        if append {
            self.notes.extend(result.notes);
        } else {
            self.notes = result.notes;
        }
        self.total = result.total_count;
        self.has_more = result.has_more;
        self.tags = commands::notes::contextual_tags(
            context,
            ContextualTagsInput {
                note_filter: self.filter,
            },
        )?
        .roots;
        Ok(())
    }
    fn refresh_settings(&mut self) -> Result<(), crate::error::AppError> {
        let Some(context) = self.context.clone() else {
            return Ok(());
        };
        self.accounts = commands::accounts::list_accounts(context.clone())?;
        self.account_name = self
            .accounts
            .iter()
            .find(|a| a.is_active)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        self.relays = commands::sync::list_relays(context.clone())?;
        self.blossom_url = commands::blob::get_blossom_url(context.clone())?.unwrap_or_default();
        self.sync_enabled = commands::sync::is_sync_enabled(context)?;
        Ok(())
    }
    fn load_preferences(&mut self) {
        let Some(ctx) = &self.context else {
            return;
        };
        let path = ctx.config_dir.join("native-ui.json");
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(p) => {
                    self.dark = p.dark;
                    self.editor_animations = p.editor_animations;
                    self.font_size = p.font_size.clamp(12.0, 28.0);
                }
                Err(e) => self.error = Some(format!("Cannot read appearance settings: {e}")),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.error = Some(format!("Cannot read appearance settings: {e}")),
        }
    }
    fn save_preferences(&self) -> Result<(), crate::error::AppError> {
        if let Some(ctx) = &self.context {
            let bytes = serde_json::to_vec_pretty(&Preferences {
                dark: self.dark,
                editor_animations: self.editor_animations,
                font_size: self.font_size,
            })?;
            std::fs::write(ctx.config_dir.join("native-ui.json.tmp"), bytes)?;
            std::fs::rename(
                ctx.config_dir.join("native-ui.json.tmp"),
                ctx.config_dir.join("native-ui.json"),
            )?;
        }
        Ok(())
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Preferences {
    dark: bool,
    font_size: f32,
    #[serde(default = "editor_animations_default")]
    editor_animations: bool,
}

fn editor_animations_default() -> bool {
    true
}

pub fn run(context: AppContext) -> iced::Result {
    iced::application(
        move || Comet::new(context.clone()),
        Comet::update,
        Comet::view,
    )
    .title(|app: &Comet| {
        app.selected
            .as_ref()
            .map_or_else(|| "Comet".into(), |n| format!("{} — Comet", n.title))
    })
    .theme(Comet::theme)
    .subscription(Comet::subscription)
    .default_font(iced_m3::fonts::REGULAR)
    .font(iced_m3::fonts::ROBOTO)
    .window(window::Settings {
        size: iced::Size::new(1280.0, 800.0),
        min_size: Some(iced::Size::new(600.0, 480.0)),
        exit_on_close_request: false,
        ..Default::default()
    })
    .run()
}
