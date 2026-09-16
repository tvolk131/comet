//! Stable, offline data for visual regression tests. Never used by normal startup.
use super::{Comet, ContextualTagNode, LoadedNote, NoteSummary};
pub fn notebook() -> Comet {
    let mut app = Comet {
        account_name: "Personal".into(),
        total: 4,
        ..Comet::default()
    };
    for (i, (title, preview)) in [
        (
            "A quieter place to think",
            "Collect ideas. Make connections. Keep the things that matter.",
        ),
        (
            "Weekend on the coast",
            "A small cabin, a long walk, and no particular plans.",
        ),
        ("Reading list", "Books and essays worth coming back to."),
        ("The next small thing", "A few ideas for the week ahead."),
    ]
    .into_iter()
    .enumerate()
    {
        app.notes.push(NoteSummary {
            id: format!("note-{i}"),
            title: title.into(),
            edited_at: 1_800_000_000_000,
            preview: preview.into(),
            search_snippet: None,
            archived_at: None,
            deleted_at: None,
            pinned_at: if i == 0 { Some(1) } else { None },
            readonly: false,
            has_conflict: false,
        });
    }
    app.tags = vec![
        ContextualTagNode {
            path: "personal".into(),
            label: "personal".into(),
            depth: 0,
            pinned: false,
            hide_subtag_notes: false,
            icon: None,
            direct_note_count: 2,
            inclusive_note_count: 2,
            children: vec![],
        },
        ContextualTagNode {
            path: "ideas".into(),
            label: "ideas".into(),
            depth: 0,
            pinned: false,
            hide_subtag_notes: false,
            icon: None,
            direct_note_count: 2,
            inclusive_note_count: 2,
            children: vec![],
        },
    ];
    app.set_note(LoadedNote { id:"note-0".into(),title:"A quieter place to think".into(),modified_at:1_800_000_000_000,
        markdown:"# A quieter place to think\n\nCollect ideas. Make connections. Keep the things that matter.\n\n## A little room for ideas\n\nGood notes start with paying attention. A line from a book,\na conversation, a question you want to sit with.\n\n- [x] Make space for a daily writing habit\n- [ ] Revisit the ideas from last week\n- [ ] Take a notebook on the next walk\n\n> The best way to have a good idea is to have lots of ideas.\n\n#personal #ideas\n".into(),archived_at:None,deleted_at:None,pinned_at:Some(1),readonly:false,tags:vec!["personal".into(),"ideas".into()],wikilink_resolutions:vec![],nostr_d_tag:None,published_at:None,published_kind:None });
    app
}
