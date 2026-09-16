//! One recoverable editor draft per account, independent of sync snapshots.
use super::Comet;
use crate::{commands, error::AppError};
use iced::widget::text_editor;
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize)]
struct Draft {
    note_id: String,
    base_markdown: String,
    markdown: String,
}
impl Comet {
    fn draft_path(&self) -> Result<Option<PathBuf>, AppError> {
        self.context
            .as_ref()
            .map(|ctx| crate::db::active_account_dir(ctx).map(|p| p.join("native-draft.json")))
            .transpose()
    }
    pub(crate) fn write_draft(&self) -> Result<(), AppError> {
        let (Some(path), Some(note)) = (self.draft_path()?, &self.selected) else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(&Draft {
            note_id: note.id.clone(),
            base_markdown: note.markdown.clone(),
            markdown: self.editor.text(),
        })?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
    pub(crate) fn clear_draft(&self) -> Result<(), AppError> {
        if let Some(path) = self.draft_path()? {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    pub(crate) fn restore_draft(&mut self) -> Result<(), AppError> {
        let Some(path) = self.draft_path()? else {
            return Ok(());
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let draft: Draft = serde_json::from_slice(&bytes)?;
        let Some(ctx) = self.context.clone() else {
            return Ok(());
        };
        let mut note = commands::notes::load_note(ctx, draft.note_id)?;
        if note.markdown == draft.markdown {
            self.clear_draft()?;
            return Ok(());
        }
        // Keep the original base for the transactional stale-content check.
        note.markdown = draft.base_markdown;
        self.set_note(note);
        self.editor = text_editor::Content::with_text(&draft.markdown);
        self.dirty = true;
        self.notice = Some("Recovered your unsaved draft".into());
        Ok(())
    }
}
