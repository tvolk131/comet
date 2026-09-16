use super::{
    Comet, ContextualTagNode, Dialog, Element, Message, NoteAction, NoteFilterInput, Panel,
    SyncState, Theme,
};
use iced::{
    alignment,
    widget::{column, container, responsive, row, scrollable, stack, text, themer, Space},
    Length,
};
use iced_m3::{
    button,
    dialog::{dialog, host},
    extended_fab, filter_chip, list_item, menu, slider, switch, text_field, ButtonVariant,
    MenuItem,
};

fn quiet(label: &str, message: Message) -> iced_m3::Button<'_, Message> {
    button(label).variant(ButtonVariant::Text).on_press(message)
}
fn heading<'a>(value: impl Into<String>) -> iced::widget::Text<'a, Theme> {
    text(value.into()).font(iced_m3::fonts::MEDIUM).size(22)
}
fn muted<'a>(value: impl Into<String>, theme: &Theme) -> iced::widget::Text<'a, Theme> {
    text(value.into())
        .size(12)
        .color(theme.colors.on_surface_variant)
}
fn surface<'a>(
    content: impl Into<Element<'a, Message>>,
    color: iced::Color,
) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style::default().background(color))
        .into()
}

pub fn view(app: &Comet) -> Element<'_, Message> {
    super::scroll_input::logical_pixels(
        responsive(move |size| frame(app, size)).into(),
        app.window_scale_factor,
    )
}
fn frame(app: &Comet, size: iced::Size) -> Element<'_, Message> {
    let theme = app.theme();
    let base = responsive(move |size| {
        let wide = size.width >= 1100.0;
        let split = size.width >= 850.0;
        // Keep each pane in the same tree slot across width breakpoints so the
        // editor retains focus, drag state and scroll when the window resizes.
        let show_notes = split || app.panel == Panel::Notes;
        let show_editor = split || app.panel == Panel::Editor;
        // from_vec retains zero-width slots (row! omits them).
        let panes = iced::widget::Row::from_vec(vec![
            container(if wide {
                sidebar(app)
            } else {
                Space::new().into()
            })
            .width(if wide { 220 } else { 0 })
            .height(Length::Fill)
            .into(),
            Space::new().width(u32::from(wide)).into(),
            container(if show_notes {
                notes(app, wide)
            } else {
                Space::new().into()
            })
            .width(if split {
                Length::Fixed(300.0)
            } else if show_notes {
                Length::Fill
            } else {
                Length::Fixed(0.0)
            })
            .height(Length::Fill)
            .into(),
            Space::new().width(u32::from(split)).into(),
            container(if show_editor {
                editor(app, split)
            } else {
                Space::new().into()
            })
            .width(if show_editor {
                Length::Fill
            } else {
                Length::Fixed(0.0)
            })
            .height(Length::Fill)
            .into(),
        ])
        .width(Length::Fill)
        .height(Length::Fill);
        let mut page = column![panes].height(Length::Fill);
        if let Some(error) = &app.error {
            let mut bar = row![text(error).size(13).width(Length::Fill)]
                .spacing(8)
                .align_y(alignment::Vertical::Center);
            if app.dirty {
                bar = bar.push(quiet("Save a copy", Message::SaveCopy));
            }
            bar = bar.push(quiet("Dismiss", Message::DismissError));
            page = page.push(container(bar).padding([8, 16]).style(move |t: &Theme| {
                container::Style::default()
                    .background(t.colors.error_container)
                    .color(t.colors.on_error_container)
            }));
        }
        surface(page, theme.colors.outline_variant)
    });
    // The host stays mounted from the first closed frame. Retain the most recent
    // content on close; the host owns the transition and blocks input until done.
    let overlay = if let Some(which) = app.dialog.content {
        dialog(dialog_content(app, which, (size.height - 128.0).max(220.0)))
            .width(if which == Dialog::Settings {
                640.0
            } else {
                560.0
            })
            .on_dismiss(Message::CloseDialog)
    } else {
        dialog(Space::new())
    };
    iced_m3::focus::scope(host(base, overlay, app.dialog.is_open()))
}

fn add_tags<'a>(
    mut column: iced::widget::Column<'a, Message, Theme>,
    nodes: &[ContextualTagNode],
    app: &Comet,
) -> iced::widget::Column<'a, Message, Theme> {
    for node in nodes {
        let selected = app.active_tag.as_ref() == Some(&node.path);
        column = column.push(
            container(
                row![
                    container(filter_chip(format!("#{}", node.label), selected,).on_press(
                        if selected {
                            Message::Filter(app.filter)
                        } else {
                            Message::Tag(node.path.clone())
                        }
                    ))
                    .id(format!("tag-chip-{}", node.path)),
                    Space::new().width(Length::Fill),
                    muted(node.inclusive_note_count.to_string(), &app.theme()),
                ]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
            )
            .padding(iced::Padding {
                left: [0.0, 10.0, 20.0, 30.0, 40.0][node.depth.min(4)],
                ..Default::default()
            }),
        );
        column = add_tags(column, &node.children, app);
    }
    column
}

fn sidebar(app: &Comet) -> Element<'_, Message> {
    let theme = app.theme();
    let mut navigation = column![
        container(
            row![
                text("◌").size(30).color(theme.colors.primary),
                text("Comet").font(iced_m3::fonts::MEDIUM).size(24)
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center)
        )
        .padding([8, 12]),
        Space::new().height(10)
    ]
    .spacing(4);
    for (filter, label) in [
        (NoteFilterInput::All, "All notes"),
        (NoteFilterInput::Today, "Today"),
        (NoteFilterInput::Todo, "To-do"),
        (NoteFilterInput::Pinned, "Pinned"),
        (NoteFilterInput::Untagged, "Untagged"),
        (NoteFilterInput::Archive, "Archive"),
        (NoteFilterInput::Trash, "Trash"),
    ] {
        navigation = navigation.push(
            list_item(label)
                .selected(app.filter == filter && app.active_tag.is_none())
                .on_press(Message::Filter(filter)),
        );
    }
    navigation = navigation.push(container(muted("TAGS", &theme)).padding([14, 16]));
    let mut tags = column![].spacing(8).padding([0, 8]);
    tags = add_tags(tags, &app.tags, app);
    if app.tags.is_empty() {
        tags = tags.push(container(muted("Add #tags to your notes", &theme)).padding(12));
    }
    navigation = navigation.push(scrollable(tags).height(Length::Fill));
    let sync = match &app.sync_state {
        SyncState::Connected => "Synced",
        SyncState::NeedsUnlock => "Unlock to sync",
        SyncState::Syncing => "Syncing…",
        SyncState::Connecting | SyncState::Authenticating => "Connecting…",
        SyncState::Error { .. } => "Sync needs attention",
        SyncState::Disconnected => "Local notes",
    };
    navigation = navigation
        .push(quiet("Settings", Message::Show(Dialog::Settings)).width(Length::Fill))
        .push(
            container(muted(
                format!(
                    "{}  ·  {sync}",
                    if app.account_name.is_empty() {
                        "Personal"
                    } else {
                        &app.account_name
                    }
                ),
                &theme,
            ))
            .padding([4, 12]),
        );
    surface(
        container(navigation).padding(12),
        theme.colors.surface_container_low,
    )
}

fn new_note_fab(busy: bool) -> Element<'static, Message> {
    let add = iced_m3::icon(iced::widget::svg::Handle::from_memory(
        include_bytes!("../../assets/icons/add.svg").as_slice(),
    ));
    container(
        container(
            extended_fab(add, "New note")
                .on_press(Message::New)
                .disabled(busy),
        )
        .id("new-note-fab"),
    )
    .padding(20)
    .align_right(Length::Fill)
    .align_bottom(Length::Fill)
    .into()
}

fn notes_header(app: &Comet, wide: bool) -> Element<'_, Message> {
    let theme = app.theme();
    let title = app.active_tag.as_ref().map_or_else(
        || {
            match app.filter {
                NoteFilterInput::All => "All notes",
                NoteFilterInput::Today => "Today",
                NoteFilterInput::Todo => "To-do",
                NoteFilterInput::Pinned => "Pinned",
                NoteFilterInput::Untagged => "Untagged",
                NoteFilterInput::Archive => "Archive",
                NoteFilterInput::Trash => "Trash",
            }
            .into()
        },
        |t| format!("#{t}"),
    );
    let mut header = column![].spacing(12);
    if !wide {
        header = header.push(quiet("Browse", Message::Show(Dialog::Navigation)));
    }
    header = header
        .push(
            row![
                heading(title).width(Length::Fill),
                menu(
                    "More",
                    [
                        MenuItem::new("Import Markdown", Message::ImportMarkdown),
                        MenuItem::new("Export notes", Message::Export),
                    ]
                )
                .placement(iced_m3::menu::Placement::BottomEnd),
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center),
        )
        .push(muted(format!("{} notes", app.total), &theme))
        .push(text_field("Search notes", &app.search).on_input(Message::Search));
    if app.active_tag.is_some() {
        header = header.push(quiet("Rename tag", Message::Show(Dialog::RenameTag)));
    }
    container(header).padding(20).into()
}

fn notes(app: &Comet, wide: bool) -> Element<'_, Message> {
    let theme = app.theme();
    let mut items = column![].spacing(4);
    for note in &app.notes {
        let title = if note.title.trim().is_empty() {
            "Untitled"
        } else {
            &note.title
        };
        let preview = note
            .search_snippet
            .as_ref()
            .unwrap_or(&note.preview)
            .chars()
            .take(105)
            .collect::<String>()
            .replace('\n', " ");
        let item = list_item(title)
            .supporting_text(preview)
            .overline(if note.has_conflict {
                "CONFLICT"
            } else if note.pinned_at.is_some() {
                "PINNED"
            } else if note.readonly {
                "READ ONLY"
            } else {
                "MARKDOWN"
            })
            .selected(app.selected.as_ref().is_some_and(|n| n.id == note.id))
            .disabled(app.busy)
            .on_press(Message::Select(note.id.clone()));
        items = items.push(item);
    }
    if app.notes.is_empty() {
        items = items.push(
            container(
                column![
                    heading("A little space to think"),
                    text(if app.search.is_empty() {
                        "Create a note to start writing."
                    } else {
                        "No notes match your search."
                    })
                    .size(14)
                ]
                .spacing(16),
            )
            .padding(20),
        );
    }
    if app.has_more {
        items = items.push(quiet("Load more", Message::More).width(Length::Fill));
    }
    // Scroll the final note fully above the floating action and its shadow.
    items = items.push(Space::new().height(96));
    surface(
        column![
            notes_header(app, wide),
            stack![
                scrollable(items).id("notes-list").height(Length::Fill),
                new_note_fab(app.busy)
            ]
            .height(Length::Fill)
        ],
        theme.colors.surface_container_lowest,
    )
}

#[allow(clippy::too_many_lines)] // One declarative editor layout.
fn editor(app: &Comet, split: bool) -> Element<'_, Message> {
    let theme = app.theme();
    let mut header = row![].spacing(8).align_y(alignment::Vertical::Center);
    if !split {
        header = header
            .push(quiet("Notes", Message::ShowPanel(Panel::Notes)))
            .push(quiet("Browse", Message::Show(Dialog::Navigation)));
    }
    header = header.push(Space::new().width(Length::Fill));
    let Some(note) = &app.selected else {
        return surface(
            column![
                container(header).padding(12),
                container(
                    column![
                        heading("Your next idea starts here"),
                        text("A calm place for notes, plans, and everything in between."),
                        button("Create a note").on_press(Message::New)
                    ]
                    .spacing(20)
                    .align_x(alignment::Horizontal::Center)
                )
                .center(Length::Fill)
            ],
            theme.colors.surface,
        );
    };
    let mut actions = vec![
        MenuItem::new(
            if note.pinned_at.is_some() {
                "Unpin"
            } else {
                "Pin"
            },
            Message::Action(NoteAction::Pin),
        ),
        MenuItem::new(
            if note.archived_at.is_some() {
                "Unarchive"
            } else {
                "Archive"
            },
            Message::Action(NoteAction::Archive),
        ),
        MenuItem::new(
            if note.readonly {
                "Unlock editing"
            } else {
                "Lock editing"
            },
            Message::Action(NoteAction::Readonly),
        ),
        MenuItem::new("Duplicate", Message::Action(NoteAction::Duplicate)),
        MenuItem::new("History", Message::Show(Dialog::History)),
        MenuItem::new("Resolve conflicts", Message::Show(Dialog::Conflict)),
        MenuItem::new("Publish…", Message::Show(Dialog::Publish)),
        MenuItem::new("Find in note", Message::ToggleFind),
    ];
    if note.deleted_at.is_some() {
        actions.push(MenuItem::new(
            "Restore",
            Message::Action(NoteAction::Restore),
        ));
        actions.push(MenuItem::new(
            "Delete permanently…",
            Message::Show(Dialog::Delete),
        ));
    } else {
        actions.push(MenuItem::new(
            "Move to trash",
            Message::Action(NoteAction::Trash),
        ));
    }
    let header: Element<'_, Message> = if !note.readonly && note.deleted_at.is_none() {
        let formatting = row![
            quiet("B", Message::Format("**", "**")),
            quiet("I", Message::Format("*", "*")),
            quiet("H", Message::Format("## ", "")),
            quiet("List", Message::Format("- ", "")),
            quiet("To-do", Message::Format("- [ ] ", "")),
            menu(
                "Insert",
                [
                    MenuItem::new("Link", Message::Format("[", "](https://)")),
                    MenuItem::new("Code block", Message::Format("```\n", "\n```")),
                    MenuItem::new("Quote", Message::Format("> ", "")),
                    MenuItem::new("Image", Message::AttachImage)
                ]
            )
        ]
        .spacing(4)
        .align_y(alignment::Vertical::Center);
        let tools = row![
            container(formatting).width(Length::Fill),
            menu("Note actions", actions)
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center);
        if split {
            tools.into()
        } else {
            column![header, tools].spacing(8).into()
        }
    } else {
        header.push(menu("Note actions", actions)).into()
    };
    let mut page = column![container(header).padding([12, 16])].spacing(0);
    if let Some(query) = &app.find {
        page = page.push(
            container(
                row![
                    text_field("Find in note", query)
                        .on_input(Message::Find)
                        .on_submit(Message::FindNext),
                    quiet("Next", Message::FindNext),
                    quiet("Done", Message::ToggleFind)
                ]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
            )
            .padding([8, 24]),
        );
    }
    if note.readonly || note.deleted_at.is_some() {
        page = page.push(
            container(muted(
                if note.readonly {
                    "This note is read only"
                } else {
                    "This note is in the trash"
                },
                &theme,
            ))
            .padding([8, 24]),
        );
    }
    let content: Element<'_, Message> = themer(
        Some(app.theme().iced()),
        super::markdown_editor::editor(
            &app.editor,
            &note.id,
            app.font_size,
            app.theme(),
            !note.readonly && note.deleted_at.is_none(),
            app.attachment_dir.as_deref(),
        ),
    )
    .into();
    page = page.push(
        container(content)
            .padding([20, 0])
            .width(Length::Fill)
            .height(Length::Fill),
    );
    let count = app.editor.text().split_whitespace().count();
    let status = if app.busy {
        "Working…"
    } else if app.dirty {
        "Unsaved changes"
    } else {
        "Saved locally"
    };
    page = page.push(
        container(
            row![
                muted(app.notice.as_deref().unwrap_or(status), &theme),
                Space::new().width(Length::Fill),
                muted(format!("{count} words  ·  Markdown"), &theme)
            ]
            .spacing(8),
        )
        .padding([12, 28]),
    );
    surface(page, theme.colors.surface)
}

#[allow(clippy::too_many_lines)] // Declarative dialog variants share one sizing policy.
fn dialog_content(app: &Comet, which: Dialog, height: f32) -> Element<'_, Message> {
    let theme = app.theme();
    let close = quiet("Done", Message::CloseDialog);
    let mut body = column![].spacing(20);
    match which {
        Dialog::Navigation => {
            return column![
                scrollable(container(sidebar(app)).height(800)).height(Length::Fill),
                quiet("Done", Message::CloseDialog)
            ]
            .spacing(12)
            .height(height)
            .into()
        }
        Dialog::Settings => {
            body = body
                .push(heading("Settings"))
                .push(
                    row![
                        text("Dark appearance").width(Length::Fill),
                        switch(app.dark).on_toggle(Message::Dark)
                    ]
                    .align_y(alignment::Vertical::Center),
                )
                .push(
                    column![
                        text(format!("Editor size · {:.0} px", app.font_size)),
                        slider(12.0..=28.0, app.font_size)
                            .step(1.0)
                            .on_change(Message::FontSize)
                    ]
                    .spacing(8),
                )
                .push(heading("Profile"));
            for account in &app.accounts {
                body = body.push(
                    list_item(&account.name)
                        .supporting_text(format!(
                            "{}…",
                            account.npub.chars().take(32).collect::<String>()
                        ))
                        .selected(account.is_active)
                        .disabled(app.busy)
                        .on_press(Message::SwitchAccount(account.public_key.clone())),
                );
            }
            body = body
                .push(
                    row![
                        text_field("Profile name", &app.account_name)
                            .on_input(Message::AccountName),
                        quiet("Rename", Message::RenameAccount)
                    ]
                    .spacing(8)
                    .align_y(alignment::Vertical::Center),
                )
                .push(quiet(
                    "Store secret in OS keychain",
                    Message::StoreInKeychain,
                ))
                .push(
                    text_field("Import account (nsec)", &app.import_key)
                        .secure(true)
                        .on_input(Message::ImportKey),
                )
                .push(
                    button("Import account")
                        .on_press(Message::ImportAccount)
                        .disabled(app.busy || app.import_key.trim().is_empty()),
                )
                .push(heading("Sync & publishing"))
                .push(
                    row![
                        text("Enable encrypted sync").width(Length::Fill),
                        switch(app.sync_enabled).on_toggle(Message::SyncEnabled)
                    ]
                    .align_y(alignment::Vertical::Center),
                );
            if let SyncState::Error { message } = &app.sync_state {
                body = body.push(text(message).color(theme.colors.error));
            }
            if app.sync_state == SyncState::NeedsUnlock {
                body = body.push(button("Unlock sync").on_press(Message::Unlock));
            }
            for relay in &app.relays {
                body = body.push(
                    column![
                        text(&relay.url).size(14),
                        row![
                            muted(
                                format!(
                                    "{}{}",
                                    relay.kind,
                                    if relay.active {
                                        " · active"
                                    } else if relay.preferred {
                                        " · preferred"
                                    } else {
                                        ""
                                    }
                                ),
                                &theme
                            ),
                            Space::new().width(Length::Fill),
                            button("Prefer")
                                .variant(ButtonVariant::Text)
                                .disabled(relay.kind != "sync")
                                .on_press(Message::PreferRelay(relay.url.clone())),
                            button("Remove").variant(ButtonVariant::Text).on_press(
                                Message::RemoveRelay(relay.url.clone(), relay.kind.clone())
                            )
                        ]
                    ]
                    .spacing(4),
                );
            }
            body=body.push(text_field("Add sync relay (wss://)",&app.relay_url).on_input(Message::RelayUrl))
                .push(text_field("Add publishing relay (wss://)",&app.publish_relay_url).on_input(Message::PublishRelayUrl))
                .push(text_field("Blossom server (https://)",&app.blossom_url).on_input(Message::BlossomUrl))
                .push(text_field("Relay access key",&app.access_key).secure(true).on_input(Message::AccessKey))
                .push(button("Save connections").on_press(Message::SaveSync).disabled(app.busy))
                .push(heading("Your notes"))
                .push(row![quiet("Import Markdown",Message::ImportMarkdown),quiet("Export notes",Message::Export)].spacing(8))
                .push(muted("Comet · Native desktop\n⌘/Ctrl N  New note     ⌘/Ctrl K  Quick open\n⌘/Ctrl F  Find     ⌘/Ctrl S  Save",&theme));
        }
        Dialog::Palette => {
            body = body
                .push(heading("Quick open"))
                .push(text_field("Search notes", &app.search).on_input(Message::Search))
                .push(
                    row![
                        quiet("New note", Message::New),
                        quiet("Settings", Message::Show(Dialog::Settings))
                    ]
                    .spacing(8),
                );
            for note in app.notes.iter().take(12) {
                body = body.push(list_item(&note.title).on_press(Message::Select(note.id.clone())));
            }
        }
        Dialog::Publish => {
            body = body
                .push(heading("Publish to Nostr"))
                .push(text(
                    "This shares your note publicly with your publishing relays.",
                ))
                .push(text_field("Title", &app.publish_title).on_input(Message::PublishTitle))
                .push(
                    text_field("Tags, separated by commas", &app.publish_tags)
                        .on_input(Message::PublishTags),
                )
                .push(
                    row![
                        button("Publish article")
                            .on_press(Message::Publish(false))
                            .disabled(app.busy),
                        button("Publish short note")
                            .variant(ButtonVariant::Outlined)
                            .on_press(Message::Publish(true))
                            .disabled(app.busy)
                    ]
                    .spacing(8),
                );
            if app
                .selected
                .as_ref()
                .is_some_and(|n| n.published_at.is_some())
            {
                body = body.push(quiet("Delete published version", Message::Unpublish));
            }
        }
        Dialog::History => {
            body = body.push(heading("Note history"));
            if let Some(history) = &app.history {
                if history.snapshots.is_empty() {
                    body = body.push(text("No saved snapshots yet."));
                }
                for (i, snapshot) in history.snapshots.iter().enumerate() {
                    body = body.push(
                        list_item(snapshot.title.as_deref().unwrap_or("Untitled"))
                            .supporting_text(snapshot.preview.as_deref().unwrap_or("No preview"))
                            .overline(if snapshot.is_current {
                                "CURRENT"
                            } else {
                                "SAVED VERSION"
                            })
                            .trailing(
                                button("Restore")
                                    .variant(ButtonVariant::Text)
                                    .disabled(snapshot.markdown.is_none() || snapshot.is_current)
                                    .on_press(Message::RestoreHistory(i)),
                            ),
                    );
                }
            } else {
                body = body.push(text("Loading snapshots…"));
            }
        }
        Dialog::Conflict => {
            body = body.push(heading("Resolve note conflict")).push(text(
                "Choose the version to keep. Other versions remain in note history.",
            ));
            if let Some(conflict) = &app.conflict {
                for (i, snapshot) in conflict.snapshots.iter().enumerate() {
                    body = body.push(
                        list_item(snapshot.title.as_deref().unwrap_or("Deleted note"))
                            .supporting_text(snapshot.preview.as_deref().unwrap_or(""))
                            .trailing(
                                button("Keep")
                                    .on_press(Message::Resolve(i))
                                    .disabled(app.busy || !snapshot.is_available),
                            ),
                    );
                }
            } else {
                body = body.push(text("No conflicting versions to resolve."));
            }
        }
        Dialog::Delete => {
            body = body
                .push(heading("Delete permanently?"))
                .push(text(
                    "This deletes the note and syncs its deletion to your other devices.",
                ))
                .push(button("Delete permanently").on_press(Message::Action(NoteAction::Delete)));
        }
        Dialog::RenameTag => {
            body = body
                .push(heading("Rename tag"))
                .push(text_field("Tag name", &app.tag_name).on_input(Message::RenameTag))
                .push(
                    button("Rename tag")
                        .on_press(Message::ConfirmRenameTag)
                        .disabled(app.tag_name.trim().is_empty()),
                );
        }
    }
    let mut layout = column![
        scrollable(body).height(Length::Fill),
        row![Space::new().width(Length::Fill), close.disabled(app.busy)]
    ]
    .spacing(16);
    if let Some(error) = &app.error {
        layout = layout.push(text(error).size(13).color(theme.colors.error));
    } else if let Some(notice) = &app.notice {
        layout = layout.push(muted(notice, &theme));
    }
    container(layout)
        .height(height.min(if which == Dialog::Settings {
            600.0
        } else {
            420.0
        }))
        .into()
}
