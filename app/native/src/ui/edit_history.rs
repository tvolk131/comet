//! Bounded, per-note undo transactions over the canonical Markdown bytes.
//! Cursor-only navigation is a grouping boundary, never an undo step.
use iced::{advanced::text::editor::Cursor, widget::text_editor};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const MAX_NOTES: usize = 8;
const MAX_STEPS: usize = 200;
const MAX_PATCH_BYTES: usize = 8 * 1024 * 1024;
const TYPING_PAUSE: Duration = Duration::from_secs(1);

pub(super) struct Snapshot {
    pub text: String,
    pub cursor: Cursor,
}
impl Snapshot {
    pub fn capture(content: &text_editor::Content) -> Self {
        Self {
            text: content.text(),
            cursor: content.cursor(),
        }
    }
    pub fn restore(self, content: &mut text_editor::Content) {
        *content = text_editor::Content::with_text(&self.text);
        content.move_to(self.cursor);
    }
    fn edit_range(&self) -> std::ops::Range<usize> {
        let offset = |position: iced::advanced::text::editor::Position| {
            self.text
                .split_inclusive('\n')
                .take(position.line)
                .map(str::len)
                .sum::<usize>()
                + position.column
        };
        let cursor = offset(self.cursor.position);
        let anchor = self.cursor.selection.map_or(cursor, offset);
        cursor.min(anchor)..cursor.max(anchor)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Typing,
    Backspace,
    Delete,
    Atomic,
}

struct Change {
    start: usize,
    removed: String,
    inserted: String,
    before: Cursor,
    after: Cursor,
}
impl Change {
    fn between(before: &Snapshot, after: &Snapshot, kind: Kind) -> Self {
        let mut start = before
            .text
            .bytes()
            .zip(after.text.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        // Repeated characters make a textual diff ambiguous. Keep typing and
        // deletion patches at the caret so consecutive keystrokes still join.
        if kind != Kind::Atomic {
            start = start
                .min(before.edit_range().start)
                .min(after.edit_range().start);
        }
        while !before.text.is_char_boundary(start) || !after.text.is_char_boundary(start) {
            start -= 1;
        }
        let mut suffix = before.text[start..]
            .bytes()
            .rev()
            .zip(after.text[start..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        if kind != Kind::Atomic {
            suffix = suffix
                .min(before.text.len() - before.edit_range().end)
                .min(after.text.len() - after.edit_range().end);
        }
        while !before.text.is_char_boundary(before.text.len() - suffix)
            || !after.text.is_char_boundary(after.text.len() - suffix)
        {
            suffix -= 1;
        }
        Self {
            start,
            removed: before.text[start..before.text.len() - suffix].into(),
            inserted: after.text[start..after.text.len() - suffix].into(),
            before: before.cursor,
            after: after.cursor,
        }
    }
    fn merge(&mut self, next: &Self, kind: Kind) -> bool {
        if self.after != next.before || next.before.selection.is_some() {
            return false;
        }
        match kind {
            Kind::Typing
                if next.removed.is_empty() && next.start == self.start + self.inserted.len() =>
            {
                self.inserted.push_str(&next.inserted);
            }
            Kind::Backspace
                if self.inserted.is_empty()
                    && next.inserted.is_empty()
                    && next.start + next.removed.len() == self.start =>
            {
                self.start = next.start;
                self.removed.insert_str(0, &next.removed);
            }
            Kind::Delete
                if self.inserted.is_empty()
                    && next.inserted.is_empty()
                    && next.start == self.start =>
            {
                self.removed.push_str(&next.removed);
            }
            _ => return false,
        }
        self.after = next.after;
        true
    }
}

struct History {
    fingerprint: [u8; 32],
    undo: VecDeque<Change>,
    redo: Vec<Change>,
    group: Option<(Kind, Instant)>,
}
fn fingerprint(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}
impl History {
    fn new(text: &str) -> Self {
        Self {
            fingerprint: fingerprint(text),
            undo: VecDeque::new(),
            redo: Vec::new(),
            group: None,
        }
    }
    fn reconcile(&mut self, text: &str) {
        // A remote replacement or recovered draft starts a fresh history. Never
        // apply local patches to a different version of a note.
        if self.fingerprint != fingerprint(text) {
            *self = Self::new(text);
        }
    }
    fn record(&mut self, before: &Snapshot, after: &Snapshot, mut kind: Kind, now: Instant) {
        self.reconcile(&before.text);
        if before.text == after.text {
            return;
        }
        let change = Change::between(before, after, kind);
        if matches!(kind, Kind::Backspace | Kind::Delete)
            && (before.cursor.selection.is_some() || change.removed.contains(['\r', '\n']))
        {
            kind = Kind::Atomic;
        }
        let merge = kind != Kind::Atomic
            && self.group.is_some_and(|(previous, time)| {
                previous == kind && now.saturating_duration_since(time) < TYPING_PAUSE
            });
        if !(merge
            && self
                .undo
                .back_mut()
                .is_some_and(|last| last.merge(&change, kind)))
        {
            self.undo.push_back(change);
        }
        self.redo.clear();
        self.fingerprint = fingerprint(&after.text);
        self.group = (kind != Kind::Atomic).then_some((kind, now));
        let mut bytes: usize = self
            .undo
            .iter()
            .map(|step| step.removed.len() + step.inserted.len())
            .sum();
        // Keep at least the latest transaction, even for a single large paste.
        while self.undo.len() > 1 && (self.undo.len() > MAX_STEPS || bytes > MAX_PATCH_BYTES) {
            let old = self.undo.pop_front().unwrap();
            bytes -= old.removed.len() + old.inserted.len();
        }
    }
    fn travel(&mut self, mut current: Snapshot, redo: bool) -> Option<Snapshot> {
        self.reconcile(&current.text);
        self.group = None;
        let step = if redo {
            self.redo.pop()?
        } else {
            self.undo.pop_back()?
        };
        let (remove, insert, cursor) = if redo {
            (&step.removed, &step.inserted, step.after)
        } else {
            (&step.inserted, &step.removed, step.before)
        };
        let range = step.start..step.start + remove.len();
        if current.text.get(range.clone()) != Some(remove.as_str()) {
            *self = Self::new(&current.text);
            return None;
        }
        current.text.replace_range(range, insert);
        current.cursor = cursor;
        self.fingerprint = fingerprint(&current.text);
        if redo {
            self.undo.push_back(step);
        } else {
            self.redo.push(step);
        }
        Some(current)
    }
}

#[derive(Default)]
pub(super) struct Histories(VecDeque<(String, History)>);
impl Histories {
    pub fn open(&mut self, id: &str, source: &str) {
        let mut history = self
            .0
            .iter()
            .position(|(note, _)| note == id)
            .and_then(|index| self.0.remove(index))
            .map_or_else(|| History::new(source), |(_, history)| history);
        history.reconcile(source);
        history.group = None;
        self.0.push_back((id.into(), history));
        while self.0.len() > MAX_NOTES {
            self.0.pop_front();
        }
    }
    pub fn break_group(&mut self) {
        if let Some((_, history)) = self.0.back_mut() {
            history.group = None;
        }
    }
    pub fn record(&mut self, before: &Snapshot, after: &Snapshot, kind: Kind, now: Instant) {
        if let Some((_, history)) = self.0.back_mut() {
            history.record(before, after, kind, now);
        }
    }
    pub fn travel(&mut self, current: Snapshot, redo: bool) -> Option<Snapshot> {
        self.0.back_mut()?.1.travel(current, redo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::{Action, Edit, Motion};

    struct Editor {
        content: text_editor::Content,
        history: History,
        now: Instant,
    }
    impl Editor {
        fn new(source: &str) -> Self {
            Self {
                content: text_editor::Content::with_text(source),
                history: History::new(source),
                now: Instant::now(),
            }
        }
        fn edit(&mut self, edit: Edit, kind: Kind) {
            let before = Snapshot::capture(&self.content);
            self.content.perform(Action::Edit(edit));
            self.history
                .record(&before, &Snapshot::capture(&self.content), kind, self.now);
            self.now += Duration::from_millis(10);
        }
        fn travel(&mut self, redo: bool) -> bool {
            if let Some(snapshot) = self.history.travel(Snapshot::capture(&self.content), redo) {
                snapshot.restore(&mut self.content);
                true
            } else {
                false
            }
        }
    }

    #[test]
    fn typing_groups_end_at_pauses_and_explicit_boundaries() {
        let mut editor = Editor::new("");
        for c in "one".chars() {
            editor.edit(Edit::Insert(c), Kind::Typing);
        }
        editor.now += TYPING_PAUSE;
        editor.edit(Edit::Insert('2'), Kind::Typing);
        editor.history.group = None;
        editor.edit(Edit::Insert('3'), Kind::Typing);
        for expected in ["one2", "one", ""] {
            assert!(editor.travel(false));
            assert_eq!(editor.content.text(), expected);
        }
        assert!(!editor.travel(false));
        for expected in ["one", "one2", "one23"] {
            assert!(editor.travel(true));
            assert_eq!(editor.content.text(), expected);
        }
        assert!(!editor.travel(true));
    }

    #[test]
    fn typing_before_repeated_text_still_undoes_as_one_group() {
        for source in ["aaa", "ééé", "🙂🙂🙂"] {
            let mut editor = Editor::new(source);
            editor.edit(Edit::Insert(source.chars().next().unwrap()), Kind::Typing);
            editor.edit(Edit::Insert('x'), Kind::Typing);
            assert!(editor.travel(false));
            assert_eq!(editor.content.text(), source);
            assert!(!editor.travel(false));
        }
    }

    #[test]
    fn consecutive_deletions_group_in_both_directions() {
        for backward in [true, false] {
            let mut editor = Editor::new("aé🙂");
            if backward {
                editor.content.perform(Action::Move(Motion::DocumentEnd));
            }
            let cursor = editor.content.cursor();
            for _ in 0..2 {
                if backward {
                    editor.edit(Edit::Backspace, Kind::Backspace);
                } else {
                    editor.edit(Edit::Delete, Kind::Delete);
                }
            }
            let deleted = editor.content.text();
            assert_eq!(deleted, if backward { "a" } else { "🙂" });
            assert!(editor.travel(false));
            assert_eq!(editor.content.text(), "aé🙂");
            assert_eq!(editor.content.cursor(), cursor);
            assert!(!editor.travel(false));
            assert!(editor.travel(true));
            assert_eq!(editor.content.text(), deleted);
        }
    }

    #[test]
    fn line_break_deletion_is_a_separate_step_and_preserves_crlf() {
        let mut editor = Editor::new("one\r\ntwo\r\n");
        editor.content.perform(Action::Move(Motion::DocumentEnd));
        editor.edit(Edit::Backspace, Kind::Backspace);
        assert_eq!(editor.content.text(), "one\r\ntwo");
        editor.edit(Edit::Backspace, Kind::Backspace);
        assert!(editor.travel(false));
        assert_eq!(editor.content.text(), "one\r\ntwo");
        assert!(editor.travel(false));
        assert_eq!(editor.content.text(), "one\r\ntwo\r\n");
    }

    #[test]
    fn replacements_restore_selection_and_multibyte_source_exactly() {
        for source in ["ê🙂\r\n", "ééé", "**café**\r\n- [X] café  \r\n"] {
            let mut editor = Editor::new(source);
            editor.content.perform(Action::SelectAll);
            let original = editor.content.cursor();
            editor.edit(Edit::Insert('é'), Kind::Typing);
            editor.edit(Edit::Insert('🙂'), Kind::Typing);
            let replaced = editor.content.cursor();
            assert!(editor.travel(false));
            assert_eq!(editor.content.text(), source);
            // iced exposes selected lines with LF separators; the document
            // itself must still retain every authored CRLF byte.
            assert_eq!(
                editor.content.selection(),
                Some(source.replace("\r\n", "\n"))
            );
            assert_eq!(editor.content.cursor(), original);
            assert!(!editor.travel(false));
            assert!(editor.travel(true));
            assert_eq!(editor.content.text(), "é🙂");
            assert_eq!(editor.content.cursor(), replaced);
            assert_eq!(editor.content.selection(), None);
        }
    }

    #[test]
    fn noops_keep_redo_but_a_new_edit_discards_it() {
        let mut editor = Editor::new("");
        editor.edit(Edit::Insert('a'), Kind::Typing);
        assert!(editor.travel(false));
        editor.edit(Edit::Backspace, Kind::Backspace);
        assert!(editor.travel(true));
        assert!(editor.travel(false));
        editor.edit(Edit::Insert('b'), Kind::Typing);
        assert!(!editor.travel(true));
        assert_eq!(editor.content.text(), "b");
    }

    #[test]
    fn changed_source_resets_history_without_applying_stale_edits() {
        let mut editor = Editor::new("local");
        editor.edit(Edit::Insert('!'), Kind::Typing);
        editor.content = text_editor::Content::with_text("remote");
        assert!(!editor.travel(false));
        assert!(!editor.travel(true));
        assert_eq!(editor.content.text(), "remote");
        editor.edit(Edit::Insert('x'), Kind::Typing);
        assert!(editor.travel(false));
        assert_eq!(editor.content.text(), "remote");
    }

    #[test]
    fn history_is_bounded_while_retaining_the_latest_large_edit() {
        let mut editor = Editor::new("");
        for _ in 0..MAX_STEPS + 5 {
            editor.edit(Edit::Insert('x'), Kind::Atomic);
        }
        for _ in 0..MAX_STEPS {
            assert!(editor.travel(false));
        }
        assert!(!editor.travel(false));
        assert_eq!(editor.content.text(), "xxxxx");
        let cursor = editor.content.cursor();
        let large = "a".repeat(MAX_PATCH_BYTES + 1);
        let snapshot = |text: &str| Snapshot {
            text: text.into(),
            cursor,
        };
        editor.history.record(
            &snapshot("xxxxx"),
            &snapshot(&large),
            Kind::Atomic,
            editor.now,
        );
        assert_eq!(editor.history.undo.len(), 1);
        let restored = editor.history.travel(snapshot(&large), false).unwrap();
        assert_eq!(restored.text, "xxxxx");
        let redone = editor.history.travel(restored, true).unwrap();
        assert_eq!(redone.text, large);
    }
}
