//! A lossless display projection. Every visible boundary points into the original
//! UTF-8 Markdown; presentation never becomes the value we save or synchronize.
use crate::domain::common::text::extract_tag_occurrences;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
use std::{borrow::Cow, ops::Range, sync::Arc};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub muted: bool,
    pub tag: bool,
    pub link: Option<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: Style,
    /// Presentation only: the fraction of a revealed delimiter's width/opacity.
    pub visibility: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    pub opacity: f32,
    pub offset_y: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub source: Range<usize>,
    pub text: String,
    pub runs: Vec<Run>,
    pub boundaries: Vec<(usize, usize)>,
    pub heading: Option<u8>,
    pub bullet: Option<String>,
    pub indent: usize,
    pub task: Option<(usize, bool)>,
    pub quote: bool,
    /// Reveal of the authored list/quote prefix; decorations use its inverse.
    pub prefix_visibility: f32,
    pub code: bool,
    pub fence: bool,
    pub hidden: bool,
    pub rule: bool,
    pub image: Option<String>,
    pub table_header: bool,
    pub table_cells: Vec<Line>,
    pub appearance: Option<Appearance>,
}
impl Line {
    pub fn opacity(&self) -> f32 {
        self.appearance.map_or(1.0, |a| a.opacity)
    }
    pub fn offset_y(&self) -> f32 {
        self.appearance.map_or(0.0, |a| a.offset_y)
    }
    pub fn source_at(&self, display: usize) -> usize {
        self.boundaries
            .iter()
            .rev()
            .find(|(i, _)| *i <= display)
            .map_or(self.source.start, |(_, source)| *source)
    }
    pub fn display_at(&self, source: usize) -> usize {
        self.boundaries
            .iter()
            .rev()
            .find(|(_, i)| *i <= source)
            .map_or(0, |(display, _)| *display)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub lines: Vec<Line>,
}

fn intersects(cursor: &Option<Range<usize>>, range: &Range<usize>) -> bool {
    // A caret immediately beside inline syntax must reveal it. Callers must
    // supply content boundaries, excluding any terminating line ending.
    cursor
        .as_ref()
        .is_some_and(|c| c.start <= range.end && c.end >= range.start)
}

fn parser_source<'a>(source: &'a str, lines: &[Line]) -> Cow<'a, str> {
    // iced recognizes CR and LFCR too. Give the Markdown parser equivalent LF
    // or CRLF endings without changing byte offsets or the authored source.
    let mut normalized: Option<Vec<u8>> = None;
    for pair in lines.windows(2) {
        let ending = pair[0].source.end..pair[1].source.start;
        let replacement: &[u8] = match &source[ending.clone()] {
            "\r" => b"\n",
            "\n\r" => b"\r\n",
            _ => continue,
        };
        normalized.get_or_insert_with(|| source.as_bytes().to_vec())[ending]
            .copy_from_slice(replacement);
    }
    normalized.map_or(Cow::Borrowed(source), |bytes| {
        Cow::Owned(String::from_utf8(bytes).expect("ASCII line-ending replacements preserve UTF-8"))
    })
}

impl Document {
    pub fn parse(source: &str, cursor: Option<Range<usize>>) -> Self {
        let mut styles = vec![Style::default(); source.len()];
        let mut hidden = vec![false; source.len()];
        let mut document = Self {
            lines: physical_lines(source)
                .into_iter()
                .map(|range| Line {
                    source: range,
                    ..Line::default()
                })
                .collect(),
        };
        let mut cells = Vec::new();
        let mut reveal_stack: Vec<(Range<usize>, Range<usize>)> = Vec::new();
        let parser_source = parser_source(source, &document.lines);
        for (event, range) in Parser::new_ext(
            &parser_source,
            Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES,
        )
        .into_offset_iter()
        {
            let mut syntax = Vec::new();
            let mut reveal_scope = range.clone();
            match event {
                Event::Start(Tag::Table(_)) => {
                    if let Some(separator) = document
                        .lines
                        .iter_mut()
                        .filter(|line| {
                            line.source.start >= range.start && line.source.start < range.end
                        })
                        .nth(1)
                    {
                        separator.hidden = !intersects(&cursor, &separator.source);
                    }
                }
                Event::Start(Tag::TableHead) => {
                    if let Some(line) = document
                        .lines
                        .iter_mut()
                        .find(|line| line.source.contains(&range.start))
                    {
                        line.table_header = true;
                    }
                }
                Event::Start(Tag::TableCell) => {
                    cells.push(range.clone());
                }
                Event::Start(Tag::Strong | Tag::Emphasis | Tag::Strikethrough) => {
                    let width = if matches!(event, Event::Start(Tag::Emphasis)) {
                        1
                    } else {
                        2
                    };
                    if range.len() < width * 2 {
                        continue;
                    }
                    // A partially deleted delimiter run stays literal instead of
                    // being reinterpreted as a smaller, unrelated emphasis pair.
                    let mark = source.as_bytes()[range.start];
                    let run_at = |at: usize| {
                        let before = source.as_bytes()[..at]
                            .iter()
                            .rev()
                            .take_while(|b| **b == mark)
                            .count();
                        let after = source.as_bytes()[at..]
                            .iter()
                            .take_while(|b| **b == mark)
                            .count();
                        before + after
                    };
                    if run_at(range.start) != run_at(range.end - 1) {
                        continue;
                    }
                    for style in &mut styles[range.start + width..range.end - width] {
                        match event {
                            Event::Start(Tag::Strong) => style.bold = true,
                            Event::Start(Tag::Emphasis) => style.italic = true,
                            _ => style.strike = true,
                        }
                    }
                    syntax.extend([
                        range.start..range.start + width,
                        range.end - width..range.end,
                    ]);
                }
                Event::Code(_) => {
                    let width = source[range.clone()]
                        .bytes()
                        .take_while(|c| *c == b'`')
                        .count();
                    if range.len() >= width * 2 {
                        // Revealed backticks belong to the code span too. If
                        // they inherit an enclosing emphasis font instead, its
                        // different ascent/descent moves the entire baseline.
                        for style in &mut styles[range.clone()] {
                            style.code = true;
                        }
                        syntax.extend([
                            range.start..range.start + width,
                            range.end - width..range.end,
                        ]);
                    }
                }
                Event::Start(Tag::Link { dest_url, .. }) => {
                    let raw = &source[range.clone()];
                    if raw.starts_with('[') {
                        if let Some(end) = raw.rfind("](").or_else(|| raw.find(']')) {
                            let link: Arc<str> = dest_url.as_ref().into();
                            for style in &mut styles[range.start + 1..range.start + end] {
                                style.link = Some(link.clone());
                            }
                            syntax.extend([
                                range.start..range.start + 1,
                                range.start + end..range.end,
                            ]);
                        }
                    }
                }
                Event::Start(Tag::Image { dest_url, .. }) => {
                    if let Some(line) = document.lines.iter_mut().find(|line| {
                        line.source.start <= range.start && line.source.end >= range.end
                    }) {
                        if source[line.source.clone()].trim() == source[range.clone()].trim()
                            && !intersects(&cursor, &range)
                        {
                            line.image = Some(dest_url.to_string());
                        }
                    }
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    if let Some(line) = document
                        .lines
                        .iter_mut()
                        .find(|line| line.source.contains(&range.start))
                    {
                        line.heading = Some(level as u8);
                        // Heading events include the trailing line ending;
                        // their editable content ends before it, like other
                        // line-based blocks (tables, rules and code fences).
                        reveal_scope = line.source.clone();
                        let raw = &source[line.source.clone()];
                        let prefix = raw.bytes().take_while(|c| *c == b'#').count();
                        if prefix > 0 {
                            syntax.push(
                                line.source.start
                                    ..line.source.start
                                        + prefix
                                        + usize::from(raw.as_bytes().get(prefix) == Some(&b' ')),
                            );
                        }
                    }
                }
                Event::TaskListMarker(checked) => {
                    if let Some(line) = document
                        .lines
                        .iter_mut()
                        .find(|line| line.source.contains(&range.start))
                    {
                        line.task = Some((range.start + 1, checked));
                        // Keep the hit target available while editing its label.
                        hidden[range.clone()].fill(true);
                        if source.as_bytes().get(range.end) == Some(&b' ') {
                            hidden[range.end] = true;
                        }
                    }
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    for style in &mut styles[range.clone()] {
                        style.code = true;
                    }
                    let block_lines: Vec<_> = document
                        .lines
                        .iter_mut()
                        .filter(|line| {
                            line.source.end >= range.start && line.source.start < range.end
                        })
                        .collect();
                    let length = block_lines.len();
                    let opening = source[range.clone()].trim_start();
                    let mark = opening.as_bytes()[0];
                    let fence_length = opening.bytes().take_while(|byte| *byte == mark).count();
                    let active =
                        block_lines
                            .first()
                            .zip(block_lines.last())
                            .is_some_and(|(first, last)| {
                                intersects(&cursor, &(first.source.start..last.source.end))
                            });
                    for (i, line) in block_lines.into_iter().enumerate() {
                        line.code = true;
                        if matches!(kind, CodeBlockKind::Fenced(_))
                            && (i == 0
                                || (i + 1 == length && {
                                    let raw = source[line.source.clone()].trim();
                                    let marks =
                                        raw.bytes().take_while(|byte| *byte == mark).count();
                                    marks >= fence_length && raw[marks..].trim().is_empty()
                                }))
                        {
                            line.fence = true;
                            line.hidden = !active;
                            for style in &mut styles[line.source.clone()] {
                                style.muted = true;
                            }
                        }
                    }
                }
                Event::Rule => {
                    if let Some(line) = document
                        .lines
                        .iter_mut()
                        .find(|line| line.source.contains(&range.start))
                    {
                        line.rule = !intersects(&cursor, &line.source);
                    }
                }
                _ => {}
            }
            let reveal_range = if let [opening, closing] = syntax.as_slice() {
                // Inline events arrive from outermost to innermost. Wrappers
                // whose entire content is another wrapper (***word***, or
                // **~~word~~**) share the outermost caret boundary. Ordinary
                // nested words separated by text keep their own boundaries.
                while reveal_stack.last().is_some_and(|(content, _)| {
                    content.start > range.start || content.end < range.end
                }) {
                    reveal_stack.pop();
                }
                let reveal = reveal_stack
                    .last()
                    .filter(|(content, _)| *content == range)
                    .map_or_else(|| reveal_scope.clone(), |(_, reveal)| reveal.clone());
                reveal_stack.push((opening.end..closing.start, reveal.clone()));
                reveal
            } else {
                reveal_scope
            };
            for syntax in syntax {
                if intersects(&cursor, &reveal_range) {
                    for style in &mut styles[syntax] {
                        style.muted = true;
                    }
                } else {
                    hidden[syntax].fill(true);
                }
            }
        }
        for tag in extract_tag_occurrences(source) {
            for style in &mut styles[tag.start..tag.end] {
                style.tag = !style.code;
            }
        }
        for line in &mut document.lines {
            let raw = &source[line.source.clone()];
            if !line.code {
                let leading = raw.len() - raw.trim_start_matches(' ').len();
                let mut prefix = leading;
                line.indent = leading / 2;
                while raw[prefix..].starts_with('>') {
                    line.quote = true;
                    prefix += 1 + usize::from(raw.as_bytes().get(prefix + 1) == Some(&b' '));
                }
                let rest = &raw[prefix..];
                let bullet =
                    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
                        Some((2, "•".to_owned()))
                    } else {
                        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                        (digits > 0
                            && (rest[digits..].starts_with(". ")
                                || rest[digits..].starts_with(") ")))
                        .then(|| (digits + 2, format!("{}.", &rest[..digits])))
                    };
                if let Some((length, label)) = bullet {
                    prefix += length;
                    line.bullet = Some(label);
                }
                if prefix > leading || line.bullet.is_some() {
                    let range = line.source.start..line.source.start + prefix;
                    let active = intersects(&cursor, &line.source);
                    line.prefix_visibility = if active { 1.0 } else { 0.0 };
                    hidden[range.clone()].fill(!active);
                    if active {
                        for style in &mut styles[range] {
                            style.muted = true;
                        }
                    }
                }
            }
            line.boundaries.push((0, line.source.start));
            for (offset, character) in raw.char_indices() {
                let offset = line.source.start + offset;
                if hidden[offset] {
                    line.boundaries.last_mut().unwrap().1 = offset + character.len_utf8();
                    continue;
                }
                let style = &styles[offset];
                if line.runs.last().is_none_or(|run| run.style != *style) {
                    line.runs.push(Run {
                        text: String::new(),
                        style: style.clone(),
                        visibility: 1.0,
                    });
                }
                line.runs.last_mut().unwrap().text.push(character);
                line.text.push(character);
                line.boundaries
                    .push((line.text.len(), offset + character.len_utf8()));
            }
            if !intersects(&cursor, &line.source) {
                for range in cells
                    .iter()
                    .filter(|cell| cell.start >= line.source.start && cell.end <= line.source.end)
                {
                    let mut cell = Line {
                        source: range.clone(),
                        table_header: line.table_header,
                        ..Line::default()
                    };
                    cell.boundaries.push((0, range.start));
                    for (index, character) in source[range.clone()].char_indices() {
                        let index = range.start + index;
                        if hidden[index] {
                            cell.boundaries.last_mut().unwrap().1 = index + character.len_utf8();
                            continue;
                        }
                        let style = &styles[index];
                        if cell.runs.last().is_none_or(|run| run.style != *style) {
                            cell.runs.push(Run {
                                text: String::new(),
                                style: style.clone(),
                                visibility: 1.0,
                            });
                        }
                        cell.runs.last_mut().unwrap().text.push(character);
                        cell.text.push(character);
                        cell.boundaries
                            .push((cell.text.len(), index + character.len_utf8()));
                    }
                    line.table_cells.push(cell);
                }
            }
            if let Some(uri) = &line.image {
                line.text = if uri.starts_with("attachment://") {
                    "Image attachment".into()
                } else {
                    format!("Image: {uri}")
                };
                line.runs = vec![Run {
                    text: line.text.clone(),
                    visibility: 1.0,
                    style: Style {
                        muted: true,
                        ..Style::default()
                    },
                }];
                line.boundaries = vec![(0, line.source.start), (line.text.len(), line.source.end)];
            }
        }
        document
    }
}

/// Match iced's physical line model, including CRLF, without normalizing it.
pub fn physical_lines(source: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut position = 0;
    let bytes = source.as_bytes();
    while position < bytes.len() {
        if matches!(bytes[position], b'\r' | b'\n') {
            lines.push(start..position);
            let ending = bytes[position];
            position += 1;
            if position < bytes.len()
                && matches!((ending, bytes[position]), (b'\r', b'\n') | (b'\n', b'\r'))
            {
                position += 1;
            }
            start = position;
        } else {
            position += 1;
        }
    }
    lines.push(start..source.len());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_and_quote_prefixes_reveal_only_on_their_own_line() {
        for prefix in [
            "- ", "* ", "+ ", "1. ", "12) ", "> ", ">> ", "> > ", "  - ", "  > ", "> - ",
        ] {
            for ending in ["\n", "\r\n", "\r", "\n\r"] {
                let raw = format!("{prefix}café");
                let source = format!("{raw}{ending}After");
                let resting = Document::parse(&source, None);
                assert_eq!(resting.lines[0].text, "café");
                for offset in (0..=raw.len()).filter(|offset| raw.is_char_boundary(*offset)) {
                    let active = Document::parse(&source, Some(offset..offset));
                    let line = &active.lines[0];
                    assert_eq!(line.text, raw);
                    assert_eq!(line.source_at(line.display_at(offset)), offset);
                    assert!(line.runs[0].style.muted);
                }
                let next = raw.len() + ending.len();
                assert_eq!(
                    Document::parse(&source, Some(next..next)).lines[0],
                    resting.lines[0]
                );
                assert_eq!(
                    Document::parse(&source, Some(0..source.len())).lines[0].text,
                    raw
                );
            }
        }
    }

    #[test]
    fn task_list_prefix_reveals_without_losing_the_checkbox() {
        for marker in ["[ ]", "[x]", "[X]"] {
            let source = format!("- {marker} Task\nAfter");
            let resting = Document::parse(&source, None);
            let active = Document::parse(&source, Some(8..8));
            assert_eq!(resting.lines[0].text, "Task");
            assert_eq!(active.lines[0].text, "- Task");
            assert_eq!(active.lines[0].task, resting.lines[0].task);
        }
        let code = "```md\n- item\n> quote\n```";
        let active = Document::parse(code, Some(7..7));
        assert_eq!(active.lines[1].text, "- item");
        assert!(active.lines[1].bullet.is_none());
        assert!(!active.lines[2].quote);
    }

    #[test]
    fn heading_reveal_stops_before_the_line_ending() {
        for ending in ["\n", "\r\n", "\r", "\n\r"] {
            for level in 1..=6 {
                for title in ["café", "", "café ###"] {
                    let heading = format!("{} {title}", "#".repeat(level));
                    for next in ["Body", "", "# Next"] {
                        let source = format!("{heading}{ending}{next}");
                        let lines = physical_lines(&source);
                        let resting = Document::parse(&source, None);
                        let next_start = lines[1].start;
                        for cursor in [Some(next_start..next_start), Some(next_start..source.len())]
                        {
                            assert_eq!(
                                Document::parse(&source, cursor.clone()).lines[0],
                                resting.lines[0],
                                "Heading leaked across {ending:?}: {source:?}, cursor {cursor:?}"
                            );
                        }
                        // The caret immediately before the break is still on
                        // the heading; selecting its text must reveal it too.
                        for cursor in [heading.len()..heading.len(), 0..source.len()] {
                            assert_eq!(
                                Document::parse(&source, Some(cursor)).lines[0].text,
                                heading
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn next_line_caret_does_not_reveal_previous_markdown_elements() {
        for ending in ["\n", "\r\n", "\r", "\n\r"] {
            for markdown in [
                "**bold**",
                "***nested***",
                "~~strike~~",
                "`code`",
                "[link](https://example.com)",
                "![image](attachment://image.png)",
                "---",
                "> **quote**",
                "- **list**",
                "- [ ] **task**",
                "| Name | Value |\n| --- | --- |\n| **cell** | data |",
                "```rust\nlet x = 1;\n```",
            ] {
                let markdown = markdown.replace('\n', ending);
                let source = format!("{markdown}{ending}Next");
                let lines = physical_lines(&source);
                let last = lines.len() - 1;
                let start = lines[last].start;
                let resting = Document::parse(&source, None);
                for cursor in [start..start, start..source.len()] {
                    assert_eq!(
                        Document::parse(&source, Some(cursor)).lines[..last],
                        resting.lines[..last],
                        "Reveal leaked across {ending:?} after {markdown:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn parser_line_endings_preserve_rendering_and_original_byte_positions() {
        let template = "# café\n\n***first\nsecond***\n\n| Name | State |\n| --- | --- |\n| **Row** | Ready |\n\n```rust\nlet x = 1;\n\n```\nAfter";
        for ending in ["\n", "\r\n", "\r", "\n\r"] {
            let source = template.replace('\n', ending);
            let reference = template.replace('\n', if ending.len() == 2 { "\r\n" } else { "\n" });
            for line in physical_lines(&source) {
                for cursor in [None, Some(line.start..line.start), Some(line.end..line.end)] {
                    assert_eq!(
                        Document::parse(&source, cursor.clone()),
                        Document::parse(&reference, cursor),
                        "Rendering and source offsets must agree for {ending:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn formatting_that_spans_lines_reveals_from_either_line() {
        for ending in ["\n", "\r\n", "\r", "\n\r"] {
            let source = format!("***first{ending}second***{ending}After");
            let lines = physical_lines(&source);
            for cursor in [lines[0].end, lines[1].start, lines[1].end] {
                let document = Document::parse(&source, Some(cursor..cursor));
                assert_eq!(document.lines[0].text, "***first");
                assert_eq!(document.lines[1].text, "second***");
            }
            let cursor = lines[2].start;
            let document = Document::parse(&source, Some(cursor..cursor));
            assert_eq!(document.lines[0].text, "first");
            assert_eq!(document.lines[1].text, "second");
        }
    }

    #[test]
    fn formatting_is_revealed_only_at_the_cursor() {
        let source = "**foo** and *bar*";
        let resting = Document::parse(source, None);
        assert_eq!(resting.lines[0].text, "foo and bar");
        assert!(resting.lines[0].runs[0].style.bold);
        let editing = Document::parse(source, Some(4..4));
        assert_eq!(editing.lines[0].text, "**foo** and bar");
        assert_eq!(
            Document::parse(source, Some(10..10)).lines[0].text,
            "foo and bar"
        );
    }
    #[test]
    fn tightly_nested_formatting_reveals_as_one_group_at_every_boundary() {
        for token in [
            "***foo***",
            "*****foo*****",
            "******foo******",
            "__*foo*__",
            "**~~*foo*~~**",
            "~~***foo***~~",
            "**`foo`**",
            "**[`foo`](/note)**",
            "[***foo***](/note)",
            "***café 👩‍💻***",
        ] {
            let source = format!("Before {token} after **other**");
            let start = "Before ".len();
            let end = start + token.len();
            let expected = format!("Before {token} after other");
            for cursor in (start..=end).filter(|i| source.is_char_boundary(*i)) {
                let document = Document::parse(&source, Some(cursor..cursor));
                let line = &document.lines[0];
                assert_eq!(line.text, expected, "{token}, caret at {}", cursor - start);
                assert_eq!(line.source_at(line.display_at(cursor)), cursor);
            }
            for cursor in [None, Some(start - 1..start - 1), Some(end + 1..end + 1)] {
                let line = &Document::parse(&source, cursor).lines[0];
                assert!(!line.text.contains(['*', '_', '~', '`', '[', ']']));
            }
            // A selection touching either outside edge activates the whole stack.
            for selection in [start - 1..start, end..end + 1] {
                assert_eq!(
                    Document::parse(&source, Some(selection)).lines[0].text,
                    expected
                );
            }
        }
    }

    #[test]
    fn separate_nested_words_keep_their_own_reveal_boundaries() {
        let source = "**outer ***inner*** tail** and _other_";
        assert_eq!(
            Document::parse(source, Some(0..0)).lines[0].text,
            "**outer inner tail** and other"
        );
        let start = source.find("***inner").unwrap();
        let end = start + "***inner***".len();
        for cursor in start..=end {
            assert_eq!(
                Document::parse(source, Some(cursor..cursor)).lines[0].text,
                "**outer ***inner*** tail** and other"
            );
        }
        let adjacent = "***foo***___bar___";
        assert_eq!(
            Document::parse(adjacent, Some(0..0)).lines[0].text,
            "***foo***bar"
        );
        assert_eq!(
            Document::parse(adjacent, Some(adjacent.len()..adjacent.len())).lines[0].text,
            "foo___bar___"
        );
    }
    #[test]
    fn unicode_boundaries_point_into_original_source() {
        let source = "**café 👩‍💻** and `code`\r\n- [ ] Task\r\n";
        let document = Document::parse(source, None);
        for line in &document.lines {
            for (display, original) in &line.boundaries {
                assert!(line.text.is_char_boundary(*display));
                assert!(source.is_char_boundary(*original));
            }
        }
        assert_eq!(document.lines[1].text, "Task");
        assert_eq!(
            document.lines[1].task,
            Some((source.find("[ ]").unwrap() + 1, false))
        );
        assert_eq!(document.lines.len(), 3);
    }
    #[test]
    fn unfinished_markers_remain_editable_text() {
        for source in ["**foo*", "**fo*", "`code", "[link](url"] {
            assert_eq!(Document::parse(source, None).lines[0].text, source);
        }
    }

    #[test]
    fn tables_render_cells_and_reveal_the_active_source_row() {
        let source = "| Feature | Status |\n| --- | --- |\n| **Bold** | Ready |\n";
        let resting = Document::parse(source, None);
        assert!(resting.lines[0].table_header);
        assert!(resting.lines[1].hidden);
        assert_eq!(resting.lines[2].table_cells.len(), 2);
        assert_eq!(resting.lines[2].table_cells[0].text.trim(), "Bold");
        let offset = source.find("Bold").unwrap() + 1;
        let active = Document::parse(source, Some(offset..offset));
        assert!(active.lines[2].table_cells.is_empty());
        assert_eq!(active.lines[2].text, "| **Bold** | Ready |");
    }

    #[test]
    fn code_fences_do_not_interpret_markdown_inside_them() {
        let source = "```md\n**literal**\n- [ ] also literal\n```\n";
        let resting = Document::parse(source, None);
        assert!(resting.lines[0].hidden && resting.lines[3].hidden);
        assert_eq!(resting.lines[1].text, "**literal**");
        assert!(resting.lines[2].task.is_none());
        assert!(resting.lines[1].runs[0].style.code);
    }

    #[test]
    fn both_fences_reveal_anywhere_inside_the_block() {
        for fence in ["```", "~~~~", "````"] {
            let source = format!("Before\r\n{fence}rust\r\nfirst\r\n\r\nlast\r\n{fence}\r\nAfter");
            let lines = physical_lines(&source);
            for line in &lines[1..=5] {
                for cursor in [line.start, line.end] {
                    let active = Document::parse(&source, Some(cursor..cursor));
                    assert!(active.lines[1].fence && active.lines[5].fence);
                    assert!(!active.lines[1].hidden && !active.lines[5].hidden);
                    assert_eq!(active.lines[1].text, format!("{fence}rust"));
                    assert_eq!(active.lines[5].text, fence);
                }
            }
            for cursor in [0, lines[6].start] {
                let resting = Document::parse(&source, Some(cursor..cursor));
                assert!(resting.lines[1].hidden && resting.lines[5].hidden);
            }
        }
        let unfinished = Document::parse("````md\n```", None);
        assert!(unfinished.lines[0].hidden);
        assert!(
            !unfinished.lines[1].hidden,
            "A shorter literal fence is code content"
        );
    }

    #[test]
    fn tags_use_the_domain_parser_and_keep_source_boundaries() {
        let source = "#work/project #café #123 `#code` [link](https://example.com/#anchor)\n\\#escaped\n```\n#literal\n```";
        for cursor in [None, Some(3..3)] {
            let document = Document::parse(source, cursor);
            let tags: Vec<_> = document
                .lines
                .iter()
                .flat_map(|line| &line.runs)
                .filter(|run| run.style.tag)
                .map(|run| run.text.as_str())
                .collect();
            assert_eq!(tags, ["#work/project", "#café"]);
            assert_eq!(
                document.lines[0].text,
                "#work/project #café #123 #code link"
            );
            for line in &document.lines {
                for (display, original) in &line.boundaries {
                    assert!(source.is_char_boundary(*original));
                    assert!(line.text.is_char_boundary(*display));
                }
            }
        }
    }
}
