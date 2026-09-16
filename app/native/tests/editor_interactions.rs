//! Each checkpoint follows real key/pointer events through the production view
//! and reducer, and asserts source, cursor, selection, and rendered pixels.
#[path = "support/editor_driver.rs"]
mod editor_driver;
#[path = "support/snapshot.rs"]
mod snapshot;
use editor_driver::EditorDriver;
use iced::keyboard::key::Named::*;

// Exercise the entire press -> stationary event -> hand jitter -> release ->
// typing sequence. Each case runs in compact and wide production layouts.
fn held_click_case(name: &str, source: &str, point: fn(f32) -> (f32, f32), cursor: (usize, usize)) {
    for size in [(640, 480), (1440, 900)] {
        let mut ui = EditorDriver::new(source, size);
        let (x, y) = point(ui.bounds().width);
        ui.press(x, y);
        ui.move_to(x, y);
        ui.move_to(x + 0.5, y + 0.5);
        let prefix = format!("interactions/held-click/{name}-{}", size.0);
        ui.check(&format!("{prefix}/01-held"), source, cursor, None);
        ui.release();
        ui.check(&format!("{prefix}/02-released"), source, cursor, None);
        // Delimiter reveal after release must not change the insertion point.
        ui.type_text("X");
        let offset = source
            .split_inclusive('\n')
            .take(cursor.0)
            .map(str::len)
            .sum::<usize>()
            + cursor.1;
        let mut expected = source.to_owned();
        expected.insert(offset, 'X');
        ui.check(
            &format!("{prefix}/03-typed"),
            &expected,
            (cursor.0, cursor.1 + 1),
            None,
        );
    }
}

macro_rules! held_click_tests {
    ($($test:ident: $name:literal, $source:expr, $point:expr, $cursor:expr;)*) => {
        $(#[test]
        fn $test() { held_click_case($name, $source, $point, $cursor); })*
    };
}

held_click_tests! {
    held_click_on_bold_does_not_select_text: "bold", "**foo** and *bar*\nsecond line", |_| (14.0, 12.0), (0, 4);
    held_click_on_nested_formatting_does_not_select_text: "nested", "***foo*** and text", |_| (0.5, 12.0), (0, 3);
    held_click_on_italic_does_not_select_text: "italic", "*foo* and text", |_| (14.0, 12.0), (0, 3);
    held_click_on_strikethrough_does_not_select_text: "strike", "~~foo~~ and text", |_| (14.0, 12.0), (0, 4);
    held_click_on_inline_code_does_not_select_text: "code", "`foo` and text", |_| (20.0, 12.0), (0, 3);
    held_click_on_link_does_not_select_text: "link", "[foo](https://example.com/long/path) and text", |_| (14.0, 12.0), (0, 3);
    held_click_on_heading_does_not_select_text: "heading", "# foo\nbody", |_| (0.5, 18.0), (0, 2);
    held_click_in_quote_does_not_select_text: "quote", "> **foo**\n\nbody", |_| (32.0, 12.0), (0, 6);
    held_click_in_list_does_not_select_text: "list", "- **foo**\n- bar", |_| (46.0, 12.0), (0, 6);
    held_click_on_task_text_does_not_select_text: "task", "- [ ] **foo**\nAfter", |_| (46.0, 12.0), (0, 10);
    held_click_in_table_does_not_select_text: "table", "| Name | Status |\n| --- | --- |\n| Comet | Ready |\n\nAfter", |width| ((width - 8.0) / 2.0 + 13.0, 51.0), (2, 10);
    held_click_in_code_block_does_not_select_text: "fence", "```rust\nlet foo = 1;\n```\nAfter", |_| (20.0, 37.5), (1, 2);
}

#[test]
fn deliberate_drag_across_formatted_lines_keeps_its_endpoints() {
    let source = "**foo**\n**bar**";
    let mut ui = EditorDriver::new(source, (640, 480));
    ui.press(14.0, 12.0);
    ui.move_to(0.0, 38.0);
    ui.move_to(0.0, 38.0);
    ui.check(
        "interactions/held-drag/01-dragging",
        source,
        (1, 2),
        Some("o**\n**"),
    );
    ui.release();
    ui.check(
        "interactions/held-drag/02-released",
        source,
        (1, 2),
        Some("o**\n**"),
    );
    ui.type_text("X");
    ui.check(
        "interactions/held-drag/03-replaced",
        "**foXbar**",
        (0, 5),
        None,
    );
}

#[test]
fn shift_click_still_selects_between_formatted_words() {
    let source = "**foo**\n**bar**";
    let mut ui = EditorDriver::new(source, (640, 480));
    ui.click(14.0, 12.0);
    ui.shift_click(0.0, 38.0);
    ui.check(
        "interactions/shift-click/01-selected",
        source,
        (1, 2),
        Some("o**\n**"),
    );
    ui.type_text("X");
    ui.check(
        "interactions/shift-click/02-replaced",
        "**foXbar**",
        (0, 5),
        None,
    );
}

#[test]
fn type_markdown_and_edit_it_with_arrows() {
    let mut ui = EditorDriver::new("", (640, 480));
    ui.click(8.0, 12.0);
    ui.type_text("**foo**");
    ui.check(
        "interactions/typing/01-bold-at-cursor",
        "**foo**",
        (0, 7),
        None,
    );
    ui.key(Enter);
    ui.check(
        "interactions/typing/02-markers-hidden-after-enter",
        "**foo**\n",
        (1, 0),
        None,
    );
    ui.key(ArrowUp);
    ui.key(End);
    ui.key(ArrowLeft);
    ui.key(ArrowLeft);
    ui.key(Backspace);
    ui.check(
        "interactions/typing/03-backspace-inside-bold",
        "**fo**\n",
        (0, 4),
        None,
    );
}

#[test]
fn clicking_rendered_words_reveals_and_edits_the_source() {
    let mut ui = EditorDriver::new("**foo** and *bar*\nsecond line", (1024, 768));
    // After “fo” in the rendered bold word, before its markers are revealed.
    ui.click(14.0, 12.0);
    ui.check(
        "interactions/click/01-reveal-bold",
        "**foo** and *bar*\nsecond line",
        (0, 4),
        None,
    );
    ui.type_text("X");
    ui.check(
        "interactions/click/02-insert-at-click",
        "**foXo** and *bar*\nsecond line",
        (0, 5),
        None,
    );
    ui.click(0.0, 38.0);
    ui.check(
        "interactions/click/03-hide-on-another-line",
        "**foXo** and *bar*\nsecond line",
        (1, 0),
        None,
    );
}

#[test]
fn shift_arrows_select_and_replace_only_the_selected_word() {
    let mut ui = EditorDriver::new("one **two** three\nnext line", (640, 480));
    ui.click(0.0, 12.0);
    ui.key(End);
    for _ in 0..5 {
        ui.shift_key(ArrowLeft);
    }
    ui.check(
        "interactions/selection/01-shift-left",
        "one **two** three\nnext line",
        (0, 12),
        Some("three"),
    );
    ui.type_text("four");
    ui.check(
        "interactions/selection/02-replaced",
        "one **two** four\nnext line",
        (0, 16),
        None,
    );
    ui.key(Home);
    ui.key(ArrowDown);
    ui.check(
        "interactions/selection/03-next-line-start",
        "one **two** four\nnext line",
        (1, 0),
        None,
    );
}

#[test]
fn vertical_arrows_remember_the_column_across_a_short_line() {
    let mut ui = EditorDriver::new("abcdefghij\nx\nabcdefghij", (640, 480));
    ui.click(0.0, 12.0);
    for _ in 0..8 {
        ui.key(ArrowRight);
    }
    ui.key(ArrowDown);
    ui.check(
        "interactions/arrows/01-short-line",
        "abcdefghij\nx\nabcdefghij",
        (1, 1),
        None,
    );
    ui.key(ArrowDown);
    ui.check(
        "interactions/arrows/02-column-restored",
        "abcdefghij\nx\nabcdefghij",
        (2, 8),
        None,
    );
    ui.key(ArrowUp);
    ui.key(ArrowUp);
    ui.check(
        "interactions/arrows/03-round-trip",
        "abcdefghij\nx\nabcdefghij",
        (0, 8),
        None,
    );
}

#[test]
fn creating_and_clicking_task_boxes_keeps_editing_working() {
    let mut ui = EditorDriver::new("", (640, 480));
    ui.click(0.0, 12.0);
    ui.type_text("- [ ] Plan\n- [ ] Review");
    ui.check(
        "interactions/tasks/01-typed-two-tasks",
        "- [ ] Plan\n- [ ] Review",
        (1, 12),
        None,
    );
    ui.click(10.0, 12.0);
    ui.check(
        "interactions/tasks/02-click-checked",
        "- [x] Plan\n- [ ] Review",
        (1, 12),
        None,
    );
    ui.click(10.0, 12.0);
    ui.type_text(" soon");
    ui.check(
        "interactions/tasks/03-uncheck-and-continue-typing",
        "- [ ] Plan\n- [ ] Review soon",
        (1, 17),
        None,
    );
}

#[test]
fn dragging_across_lines_replaces_the_selected_markdown() {
    let mut ui = EditorDriver::new("first line\nsecond line\nthird line", (1024, 768));
    ui.drag((0.0, 12.0), (400.0, 64.0));
    ui.check(
        "interactions/drag/01-multiline-selection",
        "first line\nsecond line\nthird line",
        (2, 10),
        Some("first line\nsecond line\nthird line"),
    );
    ui.type_text("Replacement");
    ui.check(
        "interactions/drag/02-replaced",
        "Replacement",
        (0, 11),
        None,
    );
}

#[test]
fn unicode_navigation_and_deletion_preserve_crlf() {
    let mut ui = EditorDriver::new("café Ω\r\n**été**", (640, 480));
    ui.click(0.0, 12.0);
    ui.key(End);
    ui.key(ArrowLeft);
    ui.check(
        "interactions/unicode/01-before-omega",
        "café Ω\r\n**été**",
        (0, 6),
        None,
    );
    ui.key(Backspace);
    ui.key(Delete);
    ui.check(
        "interactions/unicode/02-deleted-omega",
        "café\r\n**été**",
        (0, 5),
        None,
    );
    ui.key(ArrowDown);
    ui.key(End);
    ui.key(ArrowLeft);
    ui.key(ArrowLeft);
    ui.type_text("λ");
    ui.check(
        "interactions/unicode/03-insert-unicode",
        "café\r\n**étéλ**",
        (1, 9),
        None,
    );
}

#[test]
fn copy_and_paste_preserve_hidden_markdown() {
    let mut ui = EditorDriver::new("**café**", (1024, 768));
    ui.click(12.0, 12.0);
    ui.shortcut("a");
    ui.shortcut("c");
    ui.check(
        "interactions/clipboard/01-copy",
        "**café**",
        (0, 9),
        Some("**café**"),
    );
    ui.key(End);
    ui.key(Enter);
    ui.shortcut("v");
    ui.check(
        "interactions/clipboard/02-paste",
        "**café**\n**café**",
        (1, 9),
        None,
    );
}

#[test]
fn focus_loss_hides_markers_and_refocus_restores_editing() {
    let mut ui = EditorDriver::new("**foo**", (640, 480));
    ui.click(14.0, 12.0);
    ui.blur();
    ui.type_text("ignored");
    ui.check("interactions/focus/01-blurred", "**foo**", (0, 4), None);
    ui.click(14.0, 12.0);
    ui.type_text("X");
    ui.check("interactions/focus/02-refocused", "**foXo**", (0, 5), None);
}

fn sidebar_focus_case(name: &str, label: &'static str, tag: bool) {
    let source = "**foo**\n- [ ] Keep\n- [x] Done";
    for size in [(1280, 800), (1440, 900)] {
        let mut ui = EditorDriver::new(source, size);
        let prefix = format!("interactions/sidebar-focus/{name}-{}", size.0);
        ui.click(14.0, 12.0);
        ui.assert_editor_focus(&format!("{prefix}/00-editing"), true);
        if tag {
            ui.press_control(iced_test::selector::id(label));
        } else {
            ui.press_control(label);
        }
        // The checkbox must not claim a click already handled by the sidebar.
        ui.assert_editor_focus(&format!("{prefix}/01-pressed"), false);
        ui.check(&format!("{prefix}/01-pressed"), source, (0, 4), None);
        ui.release();
        ui.key(ArrowRight);
        ui.type_text("ignored");
        ui.key(Backspace);
        ui.assert_editor_focus(&format!("{prefix}/02-released"), false);
        ui.check(&format!("{prefix}/02-released"), source, (0, 4), None);

        // A real document checkbox click should return focus to the saved caret,
        // toggle only its own marker, and allow typing at that insertion point.
        ui.click(10.0, 38.0);
        ui.assert_editor_focus(&format!("{prefix}/03-checkbox"), true);
        ui.type_text("X");
        ui.check(
            &format!("{prefix}/03-checkbox"),
            "**foXo**\n- [x] Keep\n- [x] Done",
            (0, 5),
            None,
        );
    }
}

#[test]
fn sidebar_buttons_do_not_restore_editor_focus_through_task_boxes() {
    for (name, label) in [
        ("all", "All notes"),
        ("today", "Today"),
        ("todo", "To-do"),
        ("pinned", "Pinned"),
        ("untagged", "Untagged"),
        ("archive", "Archive"),
        ("trash", "Trash"),
    ] {
        sidebar_focus_case(name, label, false);
    }
}

#[test]
fn sidebar_tags_do_not_restore_editor_focus_through_task_boxes() {
    for (name, id) in [
        ("personal", "tag-chip-personal"),
        ("ideas", "tag-chip-ideas"),
    ] {
        sidebar_focus_case(name, id, true);
    }
}

#[test]
fn readonly_notes_allow_selection_but_block_edits_and_toggles() {
    let mut ui = EditorDriver::new("**foo**\n- [ ] Keep", (640, 480)).readonly();
    ui.click(14.0, 12.0);
    ui.type_text("X");
    ui.key(Backspace);
    ui.check(
        "interactions/readonly/01-no-typing",
        "**foo**\n- [ ] Keep",
        (0, 4),
        None,
    );
    ui.click(10.0, 38.0);
    ui.shortcut("a");
    ui.check(
        "interactions/readonly/02-select-without-toggle",
        "**foo**\n- [ ] Keep",
        (1, 10),
        Some("**foo**\n- [ ] Keep"),
    );
}

#[test]
fn editing_a_rendered_table_cell_preserves_the_other_cells() {
    let source = "| Name | Status |\n| --- | --- |\n| Comet | Ready |\n\nAfter";
    let mut ui = EditorDriver::new(source, (1024, 768));
    let second_cell = (ui.bounds().width - 8.0) / 2.0 + 13.0;
    ui.click(second_cell, 51.0);
    ui.check("interactions/table/01-reveal-row", source, (2, 10), None);
    ui.type_text("Almost ");
    ui.check(
        "interactions/table/02-edit-cell",
        "| Name | Status |\n| --- | --- |\n| Comet | Almost Ready |\n\nAfter",
        (2, 17),
        None,
    );
    ui.key(ArrowDown);
    ui.key(ArrowDown);
    ui.key(Home);
    ui.check(
        "interactions/table/03-render-row-again",
        "| Name | Status |\n| --- | --- |\n| Comet | Almost Ready |\n\nAfter",
        (4, 0),
        None,
    );
}

#[test]
fn resizing_keeps_the_caret_and_subsequent_typing() {
    let mut ui = EditorDriver::new("**foo** and *bar*", (1440, 900));
    ui.click(14.0, 12.0);
    ui.resize((1024, 768));
    ui.type_text("X");
    ui.check(
        "interactions/resize/01-medium",
        "**foXo** and *bar*",
        (0, 5),
        None,
    );
    ui.resize((640, 480));
    ui.type_text("Y");
    ui.check(
        "interactions/resize/02-compact",
        "**foXYo** and *bar*",
        (0, 6),
        None,
    );
}

#[test]
fn wrapped_rows_support_click_home_end_and_vertical_arrows() {
    let source = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega ".repeat(3);
    let mut ui = EditorDriver::new(&source, (640, 480));
    ui.click(100.0, 38.0);
    let clicked = ui.cursor();
    assert_eq!(clicked.0, 0, "Wrapped rows share the same source line");
    assert!(
        clicked.1 > 60 && clicked.1 < 120,
        "Click maps to the second visible row: {clicked:?}"
    );
    ui.check(
        "interactions/wrapping/01-click-second-row",
        &source,
        clicked,
        None,
    );
    ui.key(Home);
    let row_start = ui.cursor();
    assert!(row_start.1 > 0 && row_start.1 < clicked.1);
    ui.key(ArrowDown);
    let third_row = ui.cursor();
    assert!(
        third_row.1 > clicked.1,
        "Down reaches the third visible row"
    );
    ui.key(ArrowUp);
    assert_eq!(
        ui.cursor(),
        row_start,
        "Vertical navigation returns to the start of the second row"
    );
    ui.check(
        "interactions/wrapping/02-home-and-arrow-round-trip",
        &source,
        row_start,
        None,
    );
    ui.key(End);
    let row_end = ui.cursor();
    assert!(row_end.1 > clicked.1 && row_end.1 <= third_row.1);
    ui.type_text("INSERT");
    let mut expected = source;
    expected.insert_str(row_end.1, "INSERT");
    ui.check(
        "interactions/wrapping/03-insert-at-row-end",
        &expected,
        (0, row_end.1 + 6),
        None,
    );
}

#[test]
fn double_click_selects_a_word_for_replacement() {
    let mut ui = EditorDriver::new("one **two** three", (640, 480));
    ui.double_click(14.0, 12.0);
    ui.check(
        "interactions/double-click/01-selected-word",
        "one **two** three",
        (0, 3),
        Some("one"),
    );
    ui.type_text("first");
    ui.check(
        "interactions/double-click/02-replaced-word",
        "first **two** three",
        (0, 5),
        None,
    );
}

#[test]
fn arrow_scrolling_and_wheel_scrolling_keep_clicks_on_the_right_line() {
    let source = (0..40)
        .map(|i| format!("Line {i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut ui = EditorDriver::new(&source, (640, 480));
    ui.click(0.0, 12.0);
    for _ in 0..30 {
        ui.key(ArrowDown);
    }
    ui.check(
        "interactions/scroll/01-caret-scrolled-into-view",
        &source,
        (30, 0),
        None,
    );
    ui.wheel(100.0);
    ui.check(
        "interactions/scroll/02-wheel-back-to-top",
        &source,
        (30, 0),
        None,
    );
    ui.click(0.0, 12.0);
    ui.type_text("X");
    ui.check(
        "interactions/scroll/03-edit-clicked-line",
        &format!("X{source}"),
        (0, 1),
        None,
    );
}

#[test]
fn command_z_undoes_a_typing_group_and_shift_command_z_redoes_it() {
    let mut ui = EditorDriver::new("", (640, 480));
    ui.click(0.0, 12.0);
    ui.type_text("**foo**");
    ui.shortcut("z");
    ui.check("interactions/undo/typing-01-undone", "", (0, 0), None);
    ui.redo();
    ui.check(
        "interactions/undo/typing-02-redone",
        "**foo**",
        (0, 7),
        None,
    );
}

#[test]
fn command_z_restores_a_replaced_selection_and_redo_restores_the_caret() {
    let mut ui = EditorDriver::new("one **two** three", (1440, 900));
    ui.double_click(14.0, 12.0);
    ui.type_text("first");
    ui.shortcut("z");
    ui.check(
        "interactions/undo/selection-01-undone",
        "one **two** three",
        (0, 3),
        Some("one"),
    );
    ui.redo();
    ui.check(
        "interactions/undo/selection-02-redone",
        "first **two** three",
        (0, 5),
        None,
    );
    ui.type_text("!");
    ui.shortcut("z");
    ui.check(
        "interactions/undo/selection-03-new-typing-undone",
        "first **two** three",
        (0, 5),
        None,
    );
}

#[test]
fn command_z_reverses_formatting_as_one_step() {
    let mut ui = EditorDriver::new("foo", (1024, 768));
    ui.click(0.0, 12.0);
    ui.shortcut("a");
    ui.shortcut("b");
    ui.check("interactions/undo/format-01-bold", "**foo**", (0, 7), None);
    ui.shortcut("z");
    ui.check(
        "interactions/undo/format-02-undone",
        "foo",
        (0, 3),
        Some("foo"),
    );
    ui.redo();
    ui.check(
        "interactions/undo/format-03-redone",
        "**foo**",
        (0, 7),
        None,
    );
}

#[test]
fn command_z_works_immediately_after_clicking_the_format_toolbar() {
    let mut ui = EditorDriver::new("foo", (640, 480));
    ui.double_click(14.0, 12.0);
    ui.click_control("B");
    ui.check("interactions/undo/toolbar-01-bold", "**foo**", (0, 7), None);
    ui.shortcut("z");
    ui.check(
        "interactions/undo/toolbar-02-undone",
        "foo",
        (0, 3),
        Some("foo"),
    );
    ui.redo();
    ui.check(
        "interactions/undo/toolbar-03-redone",
        "**foo**",
        (0, 7),
        None,
    );
}

#[test]
fn command_z_keeps_navigation_separate_and_new_edits_clear_redo() {
    let mut ui = EditorDriver::new("", (640, 480));
    ui.click(0.0, 12.0);
    ui.type_text("abc");
    ui.key(ArrowLeft);
    ui.type_text("X");
    ui.shortcut("z");
    ui.check(
        "interactions/undo/navigation-01-undone",
        "abc",
        (0, 2),
        None,
    );
    ui.shortcut("z");
    ui.check(
        "interactions/undo/navigation-02-first-group-undone",
        "",
        (0, 0),
        None,
    );
    ui.redo();
    ui.type_text("Y");
    ui.redo();
    ui.check(
        "interactions/undo/navigation-03-new-branch",
        "abcY",
        (0, 4),
        None,
    );
}

#[test]
fn command_z_restores_cut_and_pasted_markdown() {
    let source = "**café**\n- [X] task  \n";
    let mut ui = EditorDriver::new(source, (640, 480));
    ui.click(0.0, 12.0);
    ui.shortcut("a");
    ui.shortcut("x");
    ui.shortcut("z");
    ui.check(
        "interactions/undo/cut-01-undone",
        source,
        (2, 0),
        Some(source),
    );
    ui.redo();
    ui.shortcut("v");
    ui.shortcut("z");
    ui.check("interactions/undo/paste-01-undone", "", (0, 0), None);
    ui.redo();
    ui.check("interactions/undo/paste-02-redone", source, (2, 0), None);
}

#[test]
fn command_z_does_not_edit_an_unfocused_note() {
    let mut ui = EditorDriver::new("", (640, 480));
    ui.click(0.0, 12.0);
    ui.type_text("abc");
    ui.blur();
    ui.shortcut("z");
    ui.check("interactions/undo/focus-01-unfocused", "abc", (0, 3), None);
    ui.click(0.0, 12.0);
    ui.shortcut("z");
    ui.check("interactions/undo/focus-02-refocused", "", (0, 0), None);
}

#[test]
fn command_z_reverses_a_checkbox_click_without_moving_the_caret() {
    let source = "- [ ] Plan\n- [ ] Review";
    let mut ui = EditorDriver::new(source, (1440, 900));
    ui.click(90.0, 38.0);
    ui.key(End);
    ui.click(10.0, 12.0);
    ui.shortcut("z");
    ui.check("interactions/undo/task-01-undone", source, (1, 12), None);
    ui.redo();
    ui.check(
        "interactions/undo/task-02-redone",
        "- [x] Plan\n- [ ] Review",
        (1, 12),
        None,
    );
    ui.type_text("!");
    ui.shortcut("z");
    ui.check(
        "interactions/undo/task-03-typing-undone",
        "- [x] Plan\n- [ ] Review",
        (1, 12),
        None,
    );
}

#[test]
fn formatting_reveal_keeps_text_and_following_lines_stationary() {
    for size in [(640, 480), (1440, 900)] {
        for (name, source, before) in [
            ("bold", "Some **bold** text\nBelow", 4),
            ("nested", "Some******bold******\nBelow", 3),
            ("italic", "Some *italic* text\nBelow", 4),
            ("code", "Some `code` text\nBelow", 4),
        ] {
            let mut ui = EditorDriver::new(source, size);
            ui.click(0.0, 12.0);
            for _ in 0..before {
                ui.key(ArrowRight);
            }
            let prefix = format!("interactions/stable-type/{name}-{}", size.0);
            ui.check(&format!("{prefix}/01-before"), source, (0, before), None);
            // The same letters in “Some” and “Below” must retain identical pixels.
            // The caret and changing syntax lie to the right of this region.
            let area = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(28.0, 60.0));
            let resting = ui.region(area);
            ui.key(ArrowRight);
            ui.check(
                &format!("{prefix}/02-revealed"),
                source,
                (0, before + 1),
                None,
            );
            assert_eq!(ui.region(area), resting, "{name}: reveal moved the text");
            ui.key(ArrowLeft);
            assert_eq!(ui.region(area), resting, "{name}: hiding moved the text");
        }
    }
}

#[test]
fn revealing_bold_inline_code_keeps_the_line_stationary() {
    let source = "Text **`foo`** end\nBelow";
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        for font_size in [17.0, 28.0] {
            let mut ui = EditorDriver::new(source, size).font_size(font_size);
            ui.click(0.0, 12.0);
            for _ in 0..4 {
                ui.key(ArrowRight);
            }
            let prefix = format!("interactions/stable-bold-code/{}-{font_size}", size.0);
            ui.check(&format!("{prefix}/01-hidden"), source, (0, 4), None);
            // Unchanged letters before the code and on the following line must
            // not move when revealing the bold and inline-code delimiters.
            let area =
                iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(25.0, font_size * 3.0));
            let resting = ui.region(area);
            ui.key(ArrowRight);
            ui.check(&format!("{prefix}/02-revealed"), source, (0, 5), None);
            assert!(
                ui.region(area) == resting,
                "revealing bold code moved the line"
            );
            for _ in 0..4 {
                ui.key(ArrowRight);
            }
            ui.check(&format!("{prefix}/03-inside-word"), source, (0, 9), None);
            assert!(
                ui.region(area) == resting,
                "entering bold code moved the line"
            );
            for _ in 0..5 {
                ui.key(ArrowLeft);
            }
            ui.check(&format!("{prefix}/04-hidden-again"), source, (0, 4), None);
            assert!(
                ui.region(area) == resting,
                "hiding bold code moved the line"
            );
        }
    }
}

#[test]
#[allow(clippy::float_cmp)] // Raster bounds are exact multiples of half a logical pixel.
fn nested_formatting_reveals_together_when_crossing_either_edge() {
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        for (name, token) in [
            ("triple", "***foo***"),
            ("six", "******foo******"),
            ("mixed", "**~~*foo*~~**"),
        ] {
            let source = format!("Before {token} after\nBelow");
            let prefix = format!("interactions/nested-reveal/{name}-{}", size.0);
            let start = "Before ".len();
            let end = start + token.len();
            let inside = source.find("foo").unwrap() + 1;
            let mut ui = EditorDriver::new(&source, size);
            ui.click(0.0, 12.0);
            for _ in 0..start - 1 {
                ui.key(ArrowRight);
            }
            ui.check(
                &format!("{prefix}/01-outside-left"),
                &source,
                (0, start - 1),
                None,
            );
            ui.key(ArrowRight);
            ui.check(&format!("{prefix}/02-left-edge"), &source, (0, start), None);
            // The suffix must stay in exactly the same place through every
            // delimiter and content position, in both arrow-key directions.
            let row = iced::Rectangle::new(
                iced::Point::ORIGIN,
                iced::Size::new(ui.bounds().width - 8.0, 25.5),
            );
            let right_edge = |bounds: iced::Rectangle| bounds.x + bounds.width;
            let right = right_edge(ui.ink_bounds(row));
            for column in start + 1..=end {
                ui.key(ArrowRight);
                assert_eq!(ui.cursor(), (0, column));
                if column == inside {
                    ui.check(
                        &format!("{prefix}/03-inside-word"),
                        &source,
                        (0, column),
                        None,
                    );
                }
                if column == end {
                    ui.check(
                        &format!("{prefix}/04-right-edge"),
                        &source,
                        (0, column),
                        None,
                    );
                }
                assert_eq!(
                    right_edge(ui.ink_bounds(row)),
                    right,
                    "{prefix}: right at {column}"
                );
            }
            ui.key(ArrowRight);
            ui.check(
                &format!("{prefix}/05-outside-right"),
                &source,
                (0, end + 1),
                None,
            );
            ui.key(ArrowLeft);
            ui.check(
                &format!("{prefix}/06-return-from-right"),
                &source,
                (0, end),
                None,
            );
            assert_eq!(right_edge(ui.ink_bounds(row)), right);
            for column in (start..end).rev() {
                ui.key(ArrowLeft);
                assert_eq!(ui.cursor(), (0, column));
                assert_eq!(
                    right_edge(ui.ink_bounds(row)),
                    right,
                    "{prefix}: left at {column}"
                );
            }
            // Selection that reaches the stack from outside also reveals it all.
            ui.key(ArrowLeft);
            ui.shift_key(ArrowRight);
            ui.check(
                &format!("{prefix}/07-selection-at-edge"),
                &source,
                (0, start),
                Some(" "),
            );
        }
    }
}

#[test]
fn code_block_fences_reveal_together_without_moving_content() {
    let source = "Before\n```rust\nlet value = 1;\n\nvalue\n```\nAfter";
    for size in [(640, 480), (1440, 900)] {
        let mut ui = EditorDriver::new(source, size);
        let prefix = format!("interactions/code-block-{}", size.0);
        ui.check(&format!("{prefix}/01-resting"), source, (0, 0), None);
        let after =
            iced::Rectangle::new(iced::Point::new(0.0, 153.0), iced::Size::new(100.0, 27.0));
        let unchanged = ui.region(after);
        ui.click(0.0, 63.0);
        ui.check(&format!("{prefix}/02-body"), source, (2, 0), None);
        assert_eq!(ui.region(after), unchanged);
        ui.key(ArrowUp);
        ui.check(&format!("{prefix}/03-opening-fence"), source, (1, 0), None);
        assert_eq!(ui.region(after), unchanged);
        for _ in 0..2 {
            ui.key(ArrowDown);
        }
        ui.check(&format!("{prefix}/04-blank-line"), source, (3, 0), None);
        for _ in 0..2 {
            ui.key(ArrowDown);
        }
        ui.check(&format!("{prefix}/05-closing-fence"), source, (5, 0), None);
        assert_eq!(ui.region(after), unchanged);
        ui.key(ArrowDown);
        ui.check(&format!("{prefix}/06-after-block"), source, (6, 0), None);
        ui.blur();
        assert_eq!(ui.region(after), unchanged);
        ui.click(0.0, 63.0);
        ui.type_text("X");
        ui.check(
            &format!("{prefix}/07-edited"),
            &source.replacen("let", "Xlet", 1),
            (2, 1),
            None,
        );
        ui.shortcut("z");
        ui.check(&format!("{prefix}/08-undone"), source, (2, 0), None);
    }
}

#[test]
fn task_boxes_align_with_the_first_text_line_at_different_font_sizes() {
    for size in [(640, 480), (1440, 900)] {
        for font_size in [12.0, 17.0, 22.0, 28.0] {
            let source = "- [ ] Plan a daily writing habit\n- [x] Review the next big idea\n- [ ] A task with enough words to wrap onto another line when the editor is narrow and the font is large.";
            let mut ui = EditorDriver::new(source, size).font_size(font_size);
            ui.blur();
            let height = font_size * 1.5;
            for row in [0.0, 1.0] {
                let y = row * height;
                let box_ink = ui.ink_bounds(iced::Rectangle::new(
                    iced::Point::new(0.0, y),
                    iced::Size::new(24.0, height),
                ));
                let text_ink = ui.ink_bounds(iced::Rectangle::new(
                    iced::Point::new(32.0, y),
                    iced::Size::new(450.0, height),
                ));
                assert!(
                    (box_ink.center().y - text_ink.center().y).abs() <= 1.0,
                    "{font_size}px: checkbox {box_ink:?}, label {text_ink:?}"
                );
            }
            ui.check(
                &format!("interactions/task-alignment/{}-{font_size}", size.0),
                source,
                (0, 0),
                None,
            );
            ui.click(12.0, height / 2.0);
            ui.check(
                &format!("interactions/task-alignment/{}-{font_size}-toggled", size.0),
                &source.replacen("[ ]", "[x]", 1),
                (0, 0),
                None,
            );
        }
    }
}

#[test]
fn inline_tags_stay_editable_and_distinct_in_both_themes() {
    let source =
        "Tags: #work/project #café\nPlain `#code` and \\#escaped\n#123 is a number\nNext line";
    for size in [(640, 480), (1440, 900)] {
        for dark in [false, true] {
            let mut ui = EditorDriver::new(source, size);
            if dark {
                ui = ui.dark();
            }
            let prefix = format!(
                "interactions/tags/{}-{}",
                size.0,
                if dark { "dark" } else { "light" }
            );
            ui.check(&format!("{prefix}/01-resting"), source, (0, 0), None);
            ui.click(74.0, 12.0);
            assert!(
                matches!(ui.cursor(), (0, 6..=19)),
                "Click must land within the tag"
            );
            let cursor = ui.cursor();
            ui.check(&format!("{prefix}/02-clicked"), source, cursor, None);
            ui.type_text("X");
            let mut expected = source.to_owned();
            expected.insert(cursor.1, 'X');
            ui.check(
                &format!("{prefix}/03-edited"),
                &expected,
                (0, cursor.1 + 1),
                None,
            );
            ui.shortcut("z");
            ui.check(&format!("{prefix}/04-undone"), source, cursor, None);
            ui.key(Home);
            for _ in 0..6 {
                ui.key(ArrowRight);
            }
            ui.key(Delete);
            ui.check(
                &format!("{prefix}/05-remove-hash"),
                &source.replacen('#', "", 1),
                (0, 6),
                None,
            );
            ui.shortcut("z");
            ui.check(&format!("{prefix}/06-restore-tag"), source, (0, 6), None);
        }
    }
}

#[test]
fn empty_lines_keep_full_spacing_when_spaces_are_added_or_removed() {
    let source = "Above\n\nBelow\n\nTail";
    let with_space = "Above\n \nBelow\n\nTail";
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        for font_size in [17.0, 28.0] {
            let mut ui = EditorDriver::new(source, size).font_size(font_size);
            let line_height = font_size * 1.5;
            let prefix = format!("interactions/empty-lines/{}-{font_size}", size.0);
            ui.click(0.0, line_height + 1.0);
            ui.check(&format!("{prefix}/01-empty"), source, (1, 0), None);
            // Exclude the caret at the left edge, but retain all the following
            // text. Adding invisible whitespace must not move any of its pixels.
            let area = iced::Rectangle::new(
                iced::Point::new(16.0, line_height),
                iced::Size::new(168.0, line_height * 4.0),
            );
            let empty = ui.region(area);
            ui.type_text(" ");
            ui.check(&format!("{prefix}/02-space"), with_space, (1, 1), None);
            assert!(
                ui.region(area) == empty,
                "{prefix}: adding a space moved the following text"
            );
            ui.key(Backspace);
            ui.check(&format!("{prefix}/03-removed"), source, (1, 0), None);
            assert!(
                ui.region(area) == empty,
                "{prefix}: removing the space moved the following text"
            );
            // One arrow press traverses one full row, including blank rows.
            ui.key(ArrowDown);
            ui.check(&format!("{prefix}/04-below"), source, (2, 0), None);
            ui.key(ArrowDown);
            ui.check(&format!("{prefix}/05-next-empty"), source, (3, 0), None);
            ui.key(ArrowDown);
            ui.check(&format!("{prefix}/06-tail"), source, (4, 0), None);
            for expected in [(3, 0), (2, 0), (1, 0), (0, 0)] {
                ui.key(ArrowUp);
                assert_eq!(
                    ui.cursor(),
                    expected,
                    "{prefix}: upward navigation skipped a row"
                );
            }
            // The blank row also accepts clicks across the editor's full width.
            let right = ui.bounds().width - 12.0;
            ui.click(right, line_height * 1.5);
            ui.check(&format!("{prefix}/07-click-right"), source, (1, 0), None);
            ui.type_text("x");
            ui.check(
                &format!("{prefix}/08-character"),
                "Above\nx\nBelow\n\nTail",
                (1, 1),
                None,
            );
            assert!(
                ui.region(area) == empty,
                "{prefix}: typing moved the following text"
            );
        }
    }
}

fn scrolling_source() -> String {
    (0..80)
        .map(|i| format!("Line {i:02}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn scrolling_preserves_pixel_deltas_and_moves_one_row_per_wheel_line() {
    use iced::{Point, Rectangle, Size};
    let source = scrolling_source();
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        for font in [17.0, 28.0] {
            for scale in [1.0, 2.0] {
                let mut ui = EditorDriver::new(&source, size)
                    .font_size(font)
                    .display_scale(scale);
                let prefix = format!("interactions/scroll-input/{}-{font}", size.0);
                let region = |y| Rectangle::new(Point::new(0.0, y), Size::new(120.0, 100.0));
                let original = ui.region(region(60.0));
                ui.wheel_pixels(0.0, -10.0 * scale);
                ui.check(&format!("{prefix}/01-pixels"), &source, (0, 0), None);
                assert!(
                    ui.region(region(50.0)) == original,
                    "Trackpad pixels must map 1:1, at every font size"
                );
                for _ in 0..4 {
                    ui.wheel_pixels(0.0, -0.25 * scale);
                }
                ui.check(&format!("{prefix}/02-fractional"), &source, (0, 0), None);
                assert!(
                    ui.region(region(49.0)) == original,
                    "Small trackpad deltas must accumulate without line-sized jumps"
                );
                let stationary = ui.region(region(49.0));
                ui.wheel_pixels(-100.0, 0.0);
                assert!(
                    ui.region(region(49.0)) == stationary,
                    "Horizontal scrolling must not move the document vertically"
                );
                ui.wheel_pixels(0.0, 10_000.0);
                ui.wheel(-1.0);
                ui.check(&format!("{prefix}/03-wheel-line"), &source, (0, 0), None);
                assert!(
                    ui.region(region(60.0 - font * 1.5)) == original,
                    "One wheel line should move one rendered text row"
                );
            }
        }
    }
}

#[test]
fn scrollbar_dragging_and_track_clicks_preserve_the_document_cursor() {
    use iced::{Point, Rectangle, Size};
    let source = scrolling_source();
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        let mut ui = EditorDriver::new(&source, size);
        let prefix = format!("interactions/scrollbar/{}", size.0);
        ui.click(0.0, 12.0);
        let area = Rectangle::new(Point::new(0.0, 0.0), Size::new(120.0, 200.0));
        let top = ui.region(area);
        let x = ui.right_edge() - 5.0;
        let height = ui.bounds().height;
        ui.press(x, 10.0);
        ui.check(&format!("{prefix}/01-grabbed"), &source, (0, 0), None);
        assert!(ui.region(area) == top, "Grabbing the thumb must not jump");
        ui.move_to(x - 200.0, height + 100.0);
        ui.check(
            &format!("{prefix}/02-dragged-bottom"),
            &source,
            (0, 0),
            None,
        );
        let bottom = ui.region(area);
        assert!(
            bottom != top,
            "Dragging the scrollbar must scroll, even outside its track"
        );
        ui.release();
        ui.assert_editor_focus(&prefix, true);
        ui.wheel(-10_000.0);
        assert!(
            ui.region(area) == bottom,
            "Dragging beyond the viewport must clamp to the bottom"
        );
        ui.move_to(x, height * 0.5);
        assert!(ui.region(area) == bottom, "Release must end the drag");
        ui.press(x, height - 10.0);
        ui.move_to(x, -100.0);
        ui.release();
        ui.check(&format!("{prefix}/03-dragged-top"), &source, (0, 0), None);
        assert!(
            ui.region(area) == top,
            "Dragging above the viewport must clamp to the top"
        );
        ui.click(x, height * 0.7);
        ui.check(&format!("{prefix}/04-track-click"), &source, (0, 0), None);
        assert!(
            ui.region(area) != top,
            "Clicking the track must move to that part of the note"
        );
        ui.type_text("X");
        ui.check(
            &format!("{prefix}/05-resume-typing"),
            &format!("X{source}"),
            (0, 1),
            None,
        );
    }
}

#[test]
fn wrapped_reveal_does_not_fade_unchanged_text() {
    use iced::{time::Instant, Rectangle};
    use std::time::Duration;
    for (size, font) in [((640, 480), 28.0), ((1024, 768), 17.0), ((1440, 900), 17.0)] {
        for source in [
            format!(
                "Text **{}** end\nBelow",
                "alpha beta gamma delta ".repeat(12).trim_end()
            ),
            format!(
                "Text [link](https://example.com/{}) end\nBelow",
                "long-path/".repeat(30)
            ),
        ] {
            let mut ui = EditorDriver::new(&source, size).font_size(font);
            ui.click(0.0, 12.0);
            for _ in 0..4 {
                ui.key(ArrowRight);
            }
            // The first letters never move and exclude both caret positions.
            let area = Rectangle {
                x: 1.0,
                y: 0.0,
                width: 24.0,
                height: font * 1.5,
            };
            let resting = ui.region(area);
            for key in [ArrowRight, ArrowLeft, ArrowRight, ArrowLeft] {
                ui.key(key);
                let now = Instant::now();
                for ms in [0, 30, 75, 120, 200] {
                    assert!(
                        ui.region_at(area, now + Duration::from_millis(ms)) == resting,
                        "Unchanged text flickered at {ms} ms, size {size:?}, font {font}"
                    );
                }
            }
            assert_eq!(ui.cursor(), (0, 4));
        }
    }
}

#[test]
fn heading_markers_hide_after_crossing_a_line_break() {
    use iced::Rectangle;
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        for (ending_name, ending) in [("lf", "\n"), ("crlf", "\r\n")] {
            let source = format!("# Heading{ending}Body");
            let mut ui = EditorDriver::new(&source, size);
            let heading_area = Rectangle {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 50.0,
            };
            let resting = ui.region(heading_area);
            let prefix = format!(
                "interactions/heading-line-boundary/{ending_name}-{}",
                size.0
            );
            ui.click(0.0, 18.0);
            ui.key(End);
            ui.check(&format!("{prefix}/01-heading-end"), &source, (0, 9), None);
            assert!(
                ui.region(heading_area) != resting,
                "Heading markers reveal before the break"
            );
            ui.key(ArrowRight);
            ui.check(&format!("{prefix}/02-next-line"), &source, (1, 0), None);
            assert!(
                ui.region(heading_area) == resting,
                "Heading markers must hide after the break"
            );
            ui.key(ArrowLeft);
            assert_eq!(ui.cursor(), (0, 9));
            assert!(ui.region(heading_area) != resting);
            // The next physical line starts after the H1 row and its spacing.
            ui.click(0.0, 17.0 * 1.5 * 1.85 + 8.0 + 12.0);
            ui.check(
                &format!("{prefix}/03-click-next-line"),
                &source,
                (1, 0),
                None,
            );
            assert!(ui.region(heading_area) == resting);
            if ending == "\n" {
                ui.key(ArrowLeft);
                ui.key(Enter);
                ui.check(
                    &format!("{prefix}/04-enter-empty-line"),
                    "# Heading\n\nBody",
                    (1, 0),
                    None,
                );
                assert!(
                    ui.region(heading_area) == resting,
                    "An empty new line must not reveal the preceding heading"
                );
            }
        }
    }
}

#[test]
fn list_and_quote_markers_reveal_and_can_be_edited() {
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        for (name, prefix, x) in [
            ("bullet", "- ", 32.0),
            ("ordered", "12) ", 32.0),
            ("quote", "> ", 18.0),
            ("nested-quote", "  > > ", 34.0),
        ] {
            let raw = format!("{prefix}café");
            let source = format!("{raw}\nAfter");
            let mut ui = EditorDriver::new(&source, size);
            let snapshots = format!("interactions/block-prefix/{name}-{}", size.0);
            ui.check(&format!("{snapshots}/01-rendered"), &source, (0, 0), None);
            ui.click(x + 1.0, 12.0);
            ui.key(End);
            ui.check(
                &format!("{snapshots}/02-revealed"),
                &source,
                (0, raw.len()),
                None,
            );
            ui.key(ArrowRight);
            ui.check(&format!("{snapshots}/03-next-line"), &source, (1, 0), None);
            ui.key(ArrowLeft);
            ui.key(Home);
            let leading = prefix.len() - prefix.trim_start().len();
            for _ in 0..leading {
                ui.key(ArrowRight);
            }
            ui.key(Delete);
            let mut edited = source.clone();
            edited.remove(leading);
            ui.check(
                &format!("{snapshots}/04-delete-marker"),
                &edited,
                (0, leading),
                None,
            );
        }
    }
}

#[test]
fn task_markers_are_text_while_editing_and_checkboxes_elsewhere() {
    for size in [(640, 480), (1024, 768), (1440, 900)] {
        let source = "- [ ] Plan\nAfter";
        let checked = "- [x] Plan\nAfter";
        let mut ui = EditorDriver::new(source, size);
        let prefix = format!("interactions/task-source/{}", size.0);
        ui.check(&format!("{prefix}/01-rendered"), source, (0, 0), None);
        ui.click(45.0, 12.0);
        ui.key(End);
        ui.check(
            &format!("{prefix}/02-label-reveals-marker"),
            source,
            (0, 10),
            None,
        );
        // The old checkbox hit area must now place a text caret, never toggle.
        ui.click(10.0, 12.0);
        ui.key(Home);
        for _ in 0..3 {
            ui.key(ArrowRight);
        }
        ui.check(
            &format!("{prefix}/03-caret-in-marker"),
            source,
            (0, 3),
            None,
        );
        ui.key(Delete);
        ui.type_text("x");
        ui.check(&format!("{prefix}/04-edit-marker"), checked, (0, 4), None);
        ui.key(End);
        ui.key(ArrowRight);
        ui.check(&format!("{prefix}/05-leave-line"), checked, (1, 0), None);
        ui.click(10.0, 12.0);
        ui.check(
            &format!("{prefix}/06-toggle-elsewhere"),
            source,
            (1, 0),
            None,
        );
        ui.shortcut("z");
        ui.check(&format!("{prefix}/07-undo-toggle"), checked, (1, 0), None);
        ui.key(ArrowLeft);
        ui.check(
            &format!("{prefix}/08-return-to-checked-task"),
            checked,
            (0, 10),
            None,
        );
        ui.blur();
        ui.check(
            &format!("{prefix}/09-blur-restores-checkbox"),
            checked,
            (0, 10),
            None,
        );
        // Restoring focus must not remove the control between press and release.
        ui.press(10.0, 12.0);
        ui.move_to(10.5, 12.5);
        ui.check(
            &format!("{prefix}/10-checkbox-held"),
            checked,
            (0, 10),
            None,
        );
        ui.release();
        ui.check(
            &format!("{prefix}/11-checkbox-released"),
            source,
            (0, 10),
            None,
        );
    }
}
