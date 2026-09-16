//! Deliberately damage reviewed editor images to verify that GPU tolerance
//! cannot hide the small regressions the interaction suite is meant to catch.
#[path = "support/visual_diff.rs"]
mod visual_diff;
use std::{ops::Range, path::PathBuf};
use visual_diff::{compare, Image};

fn baseline(name: &str) -> Image {
    Image::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/screenshots/interactions")
            .join(format!("{name}-tiny-skia.png")),
    )
}

fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
    image.rgba[(y * image.width + x) * 4..][..4]
        .try_into()
        .unwrap()
}

fn paint(image: &mut Image, xs: Range<usize>, ys: Range<usize>, color: [u8; 4]) {
    for y in ys {
        for x in xs.clone() {
            let offset = (y * image.width + x) * 4;
            image.rgba[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

fn rejects(expected: &Image, actual: &Image, what: &str) {
    assert_ne!(expected.rgba, actual.rgba, "Mutation must change {what}");
    assert!(
        compare(expected, actual).is_err(),
        "Comparison missed {what}"
    );
    assert!(
        compare(actual, expected).is_err(),
        "Comparison missed added {what}"
    );
}

#[test]
fn accepts_identical_pixels_and_small_color_rounding() {
    let expected = baseline("typing/01-bold-at-cursor");
    compare(&expected, &expected).unwrap();
    let mut actual = expected.clone();
    for pixel in actual.rgba.chunks_exact_mut(4) {
        for channel in &mut pixel[..3] {
            *channel = channel.saturating_add(2);
        }
    }
    compare(&expected, &actual).unwrap();
}

#[test]
fn accepts_local_antialiasing_coverage_changes() {
    let mut expected = Image {
        width: 64,
        height: 64,
        rgba: vec![255; 64 * 64 * 4],
    };
    paint(&mut expected, 20..22, 8..56, [80, 80, 80, 255]);
    paint(&mut expected, 19..20, 8..56, [210, 210, 210, 255]);
    let mut actual = expected.clone();
    paint(&mut actual, 19..20, 8..56, [225, 225, 225, 255]);
    compare(&expected, &actual).unwrap();
}

#[test]
fn rejects_missing_caret_even_in_a_mostly_empty_editor() {
    let expected = baseline("typing/01-bold-at-cursor");
    let mut actual = expected.clone();
    let background = pixel(&expected, 300, 100);
    paint(&mut actual, 109..113, 0..52, background);
    rejects(&expected, &actual, "caret");
}

#[test]
fn rejects_caret_moved_one_logical_pixel() {
    let expected = baseline("typing/01-bold-at-cursor");
    let mut actual = expected.clone();
    let background = pixel(&expected, 300, 100);
    paint(&mut actual, 109..113, 0..52, background);
    for y in 0..52 {
        for x in 109..113 {
            paint(&mut actual, x + 2..x + 3, y..y + 1, pixel(&expected, x, y));
        }
    }
    rejects(&expected, &actual, "caret position");
}

#[test]
fn rejects_a_missing_glyph() {
    let expected = baseline("typing/01-bold-at-cursor");
    let mut actual = expected.clone();
    paint(&mut actual, 43..61, 10..41, pixel(&expected, 300, 100));
    rejects(&expected, &actual, "glyph");
}

#[test]
fn rejects_missing_selection_background() {
    let expected = baseline("selection/01-shift-left");
    let mut actual = expected.clone();
    let selected = pixel(&expected, 140, 2);
    let background = pixel(&expected, 300, 100);
    assert_ne!(selected, background);
    for rgba in actual.rgba.chunks_exact_mut(4) {
        if rgba == selected {
            rgba.copy_from_slice(&background);
        }
    }
    rejects(&expected, &actual, "selection highlight");
}

#[test]
fn rejects_line_shift_wrapping_and_clipping() {
    let expected = baseline("selection/01-shift-left");
    let background = pixel(&expected, 300, 100);
    for (label, dx, dy) in [("line position", 0, 2), ("wrapping", 50, 51)] {
        let mut actual = expected.clone();
        paint(&mut actual, 0..208, 0..51, background);
        for y in 0..51 {
            for x in 0..208 {
                paint(
                    &mut actual,
                    x + dx..x + dx + 1,
                    y + dy..y + dy + 1,
                    pixel(&expected, x, y),
                );
            }
        }
        rejects(&expected, &actual, label);
    }
    let mut actual = expected.clone();
    paint(&mut actual, 0..208, 30..38, background);
    rejects(&expected, &actual, "clipped text");
}

#[test]
fn rejects_surface_color_and_opacity_changes() {
    let expected = baseline("typing/01-bold-at-cursor");
    let mut actual = expected.clone();
    for rgba in actual.rgba.chunks_exact_mut(4) {
        for channel in &mut rgba[..3] {
            *channel = channel.saturating_sub(5);
        }
    }
    rejects(&expected, &actual, "surface color");
    let mut actual = expected.clone();
    actual.rgba[3] = 254;
    rejects(&expected, &actual, "opacity");
}

#[test]
fn rejects_changed_image_dimensions() {
    let expected = baseline("typing/01-bold-at-cursor");
    let mut actual = expected.clone();
    actual.height -= 1;
    actual.rgba.truncate(actual.width * actual.height * 4);
    assert!(compare(&expected, &actual).is_err());
}

#[test]
fn diagnostic_png_preserves_the_failed_pixels() {
    let expected = baseline("typing/01-bold-at-cursor");
    let mut actual = expected.clone();
    paint(&mut actual, 109..113, 0..52, pixel(&expected, 300, 100));
    let difference = compare(&expected, &actual).unwrap_err();
    assert!(difference.reason.contains("first difference"));
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("diff.png");
    difference.heatmap.write(&path);
    assert_eq!(Image::read(&path).rgba, difference.heatmap.rgba);
}
