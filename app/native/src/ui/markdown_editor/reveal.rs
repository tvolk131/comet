//! Interruptible delimiter expansion. The interpolated projection is used by
//! layout, painting, hit testing and the caret, so input follows the visible text.
use super::document::{Appearance, Document, Line, Run, Style};
use iced::time::Instant;
use std::{collections::BTreeMap, time::Duration};

#[derive(Default)]
pub(super) struct Motion {
    source: Option<String>,
    target: Document,
    from: Option<Document>,
    elapsed: Duration,
    duration: Duration,
    last_tick: Option<Instant>,
}

impl Motion {
    pub fn resolve(
        &mut self,
        source: &str,
        target: Document,
        duration: Duration,
        wrapped: impl Fn(&Line) -> bool,
    ) -> Document {
        if self.source.as_deref() != Some(source) || duration.is_zero() {
            self.source = Some(source.into());
            self.target = target;
            self.finish();
        } else if self.target != target {
            let mut from = self.frame();
            for (before, after) in from.lines.iter_mut().zip(&target.lines) {
                if before != after
                    && inline(before)
                    && inline(after)
                    && (wrapped(before) || wrapped(after))
                {
                    // Width interpolation can repeatedly rewrap a paragraph;
                    // fading the whole paragraph makes ordinary text flicker.
                    // Settle this paragraph in one layout change, preserving
                    // full opacity and the same geometry for paint and input.
                    *before = after.clone();
                }
            }
            self.target = target;
            if from == self.target {
                self.finish();
            } else {
                self.from = Some(from);
                self.elapsed = Duration::ZERO;
                self.duration = duration;
                self.last_tick = Some(Instant::now());
            }
        }
        self.frame()
    }

    pub fn finish(&mut self) {
        self.from = None;
        self.last_tick = None;
    }

    /// A held pointer pauses the displayed projection, including its geometry.
    pub fn tick(&mut self, now: Instant, paused: bool) -> bool {
        if self.from.is_none() {
            return false;
        }
        if paused {
            self.last_tick = None;
            return false;
        }
        if let Some(last) = self.last_tick {
            self.elapsed += now.saturating_duration_since(last);
        }
        if self.elapsed >= self.duration {
            self.finish();
            return true;
        }
        self.last_tick = Some(now);
        true
    }

    pub fn active(&self) -> bool {
        self.from.is_some()
    }

    fn frame(&self) -> Document {
        let Some(from) = &self.from else {
            return self.target.clone();
        };
        let t = (self.elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);
        // Ease out without overshoot. Redirects start at the currently displayed
        // widths, never at the previous animation's beginning or a queued target.
        let amount = 1.0 - (1.0 - t).powi(3);
        let mut frame = self.target.clone();
        for ((line, before), after) in frame
            .lines
            .iter_mut()
            .zip(&from.lines)
            .zip(&self.target.lines)
        {
            if before == after {
                continue;
            }
            // A table/image/rule replaces a representation, rather than adding
            // inline delimiters. Slide it out and the new representation in;
            // each phase keeps its own complete source-aware hit regions.
            let replacement = before.table_cells.is_empty() != after.table_cells.is_empty()
                || before.image != after.image
                || before.rule != after.rule;
            if replacement {
                let (opacity, offset_y) = if amount < 0.5 {
                    *line = before.clone();
                    (
                        before.opacity() * (1.0 - amount * 2.0),
                        before.offset_y() - amount * 8.0,
                    )
                } else {
                    (amount * 2.0 - 1.0, (1.0 - amount) * 8.0)
                };
                line.appearance = Some(Appearance { opacity, offset_y });
                for cell in &mut line.table_cells {
                    cell.appearance = Some(Appearance {
                        opacity,
                        offset_y: 0.0,
                    });
                }
                continue;
            }
            if before.appearance.is_some() {
                line.appearance = Some(Appearance {
                    opacity: before.opacity() + (1.0 - before.opacity()) * amount,
                    offset_y: before.offset_y() * (1.0 - amount),
                });
                for cell in &mut line.table_cells {
                    cell.appearance = line.appearance.map(|appearance| Appearance {
                        offset_y: 0.0,
                        ..appearance
                    });
                }
            }
            if !before.table_cells.is_empty()
                || !after.table_cells.is_empty()
                || before.image.is_some()
                || after.image.is_some()
                || before.rule
                || after.rule
            {
                continue;
            }
            interpolate_line(line, before, after, self.source.as_deref().unwrap(), amount);
        }
        frame
    }
}

fn inline(line: &Line) -> bool {
    !line.code && !line.hidden && !line.rule && line.image.is_none() && line.table_cells.is_empty()
}

fn characters(line: &Line) -> BTreeMap<usize, (&Style, f32)> {
    let mut characters = BTreeMap::new();
    let mut display = 0;
    for run in &line.runs {
        for (index, _) in run.text.char_indices() {
            characters.insert(
                line.source_at(display + index),
                (&run.style, if line.hidden { 0.0 } else { run.visibility }),
            );
        }
        display += run.text.len();
    }
    characters
}

fn interpolate_line(line: &mut Line, before: &Line, after: &Line, source: &str, amount: f32) {
    line.prefix_visibility =
        before.prefix_visibility + (after.prefix_visibility - before.prefix_visibility) * amount;
    let from = characters(before);
    let to = characters(after);
    line.text.clear();
    line.runs.clear();
    line.boundaries = vec![(0, line.source.start)];
    line.hidden = before.hidden && after.hidden;
    for (index, character) in source[line.source.clone()].char_indices() {
        let index = line.source.start + index;
        let a = from.get(&index);
        let b = to.get(&index);
        let start = a.map_or(0.0, |(_, visibility)| *visibility);
        let end = b.map_or(0.0, |(_, visibility)| *visibility);
        let visibility = start + (end - start) * amount;
        // Fences keep their complete layout even when invisible, preserving
        // code-block height. Other hidden syntax occupies no width at rest.
        if visibility <= 0.0 && !line.fence {
            line.boundaries.last_mut().unwrap().1 = index + character.len_utf8();
            continue;
        }
        let Some((style, _)) = b.or(a) else {
            continue;
        };
        if line.runs.last().is_none_or(|run| {
            run.style != **style || run.visibility.to_bits() != visibility.to_bits()
        }) {
            line.runs.push(Run {
                text: String::new(),
                style: (*style).clone(),
                visibility,
            });
        }
        line.runs.last_mut().unwrap().text.push(character);
        line.text.push(character);
        line.boundaries
            .push((line.text.len(), index + character.len_utf8()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fonts() {
        crate::ui::fonts::ensure_loaded();
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(std::borrow::Cow::Borrowed(iced_m3::fonts::ROBOTO));
    }

    fn measure(line: &Line, size: f32, width: f32) -> bool {
        let size = super::super::line_size(line, size);
        let paragraph = super::super::make_paragraph(
            line,
            size,
            width - super::super::line_indent(line),
            &iced::Theme::Light,
        );
        paragraph.buffer().layout_runs().nth(1).is_some()
    }

    #[test]
    fn wrapped_reveals_never_show_cascading_intermediate_rows() {
        load_fonts();
        let sources = [
            format!(
                "Text [link](https://example.com/{}) and following words",
                "long-path/".repeat(20)
            ),
            format!("Text ***`{}`*** end", "alpha beta café delta ".repeat(8)),
            format!("# **{}**", "A long heading ".repeat(4).trim_end()),
            format!(
                "- [ ] **{}**",
                "A long task with words ".repeat(4).trim_end()
            ),
            format!("> **{}**", "A long quotation ".repeat(5).trim_end()),
        ];
        let duration = Duration::from_millis(150);
        for source in sources {
            for width in [180.0, 360.0, 720.0] {
                for size in [17.0, 28.0] {
                    let measure = |line: &Line| measure(line, size, width);
                    let hidden = Document::parse(&source, None);
                    let shown = Document::parse(&source, Some(0..source.len()));
                    for (from, to) in [(&hidden, &shown), (&shown, &hidden)] {
                        let mut motion = Motion::default();
                        motion.resolve(&source, from.clone(), duration, measure);
                        motion.resolve(&source, to.clone(), duration, measure);
                        if !measure(&from.lines[0]) && !measure(&to.lines[0]) {
                            assert!(motion.active(), "Single-row text keeps inline motion");
                            continue;
                        }
                        let start = Instant::now();
                        for ms in (0..=150).step_by(5) {
                            motion.tick(start + Duration::from_millis(ms), false);
                            assert_eq!(
                                motion.frame(),
                                *to,
                                "Wrapped text must stay opaque in its final layout"
                            );
                        }
                        assert!(!motion.active());
                    }
                }
            }
        }
    }

    #[test]
    fn settling_a_wrapped_paragraph_preserves_other_inline_motion() {
        load_fonts();
        let source = format!(
            "Text **bold** end\n[link](https://example.com/{})",
            "path/".repeat(40)
        );
        let hidden = Document::parse(&source, None);
        let shown = Document::parse(&source, Some(0..source.len()));
        let wrapped = |line: &Line| measure(line, 17.0, 240.0);
        let mut motion = Motion::default();
        let duration = Duration::from_millis(150);
        motion.resolve(&source, hidden, duration, wrapped);
        motion.resolve(&source, shown.clone(), duration, wrapped);
        assert!(motion.active());
        motion.tick(motion.last_tick.unwrap() + Duration::from_millis(75), false);
        let frame = motion.frame();
        assert_eq!(frame.lines[1], shown.lines[1]);
        assert!(frame.lines[0]
            .runs
            .iter()
            .any(|run| run.visibility > 0.0 && run.visibility < 1.0));
    }

    #[test]
    fn inline_motion_can_reverse_pause_and_accept_edits_without_a_jump() {
        load_fonts();
        let source = "Text **bold** end".to_owned();
        let hidden = Document::parse(&source, None);
        let shown = Document::parse(&source, Some(5..5));
        let duration = Duration::from_millis(150);
        let measure = |line: &Line| measure(line, 17.0, 240.0);
        for ms in [20, 70, 130] {
            let mut motion = Motion::default();
            motion.resolve(&source, hidden.clone(), duration, measure);
            motion.resolve(&source, shown.clone(), duration, measure);
            let start = motion.last_tick.unwrap();
            motion.tick(start + Duration::from_millis(ms), false);
            let frame = motion.frame();
            assert!(!motion.tick(start + Duration::from_secs(5), true));
            assert_eq!(
                motion.frame(),
                frame,
                "A held pointer freezes hit-test geometry"
            );
            motion.tick(start + Duration::from_secs(6), false);
            assert_eq!(motion.frame(), frame, "Resuming must not count paused time");
            assert_eq!(
                motion.resolve(&source, hidden.clone(), duration, measure),
                frame,
                "Reversal starts from the displayed frame"
            );
            let edited = format!("{source}!");
            let target = Document::parse(&edited, Some(5..5));
            assert_eq!(
                motion.resolve(&edited, target.clone(), duration, measure),
                target
            );
            assert!(!motion.active(), "Typing takes effect immediately");
        }
        let mut motion = Motion::default();
        motion.resolve(&source, hidden, duration, measure);
        assert_eq!(
            motion.resolve(&source, shown.clone(), Duration::ZERO, measure),
            shown
        );
        assert!(!motion.active(), "Reduce Motion bypasses the transition");
    }
}
