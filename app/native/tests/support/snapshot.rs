//! One reviewed CPU baseline, exact CPU checks, and bounded GPU edge tolerance.
use std::path::{Path, PathBuf};

#[path = "visual_diff.rs"]
mod visual_diff;

pub fn backend() -> &'static str {
    match std::env::var("ICED_TEST_BACKEND").as_deref() {
        Ok("wgpu") => "wgpu",
        Ok("tiny-skia") => "tiny-skia",
        other => panic!("Set ICED_TEST_BACKEND to tiny-skia or wgpu (got {other:?})"),
    }
}

pub fn assert_snapshot(name: &str, mut matches: impl FnMut(&Path) -> bool) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let baseline = root.join("tests/screenshots").join(name);
    let image = backend_image_path(&baseline, "tiny-skia");
    let update =
        std::env::var_os("UPDATE_SCREENSHOTS").as_deref() == Some(std::ffi::OsStr::new("1"));
    assert!(
        !update || backend() == "tiny-skia",
        "Only tiny-skia may update reviewed baselines"
    );
    if update {
        remove_existing(&image);
        assert!(matches(&baseline));
        return;
    }
    assert!(
        image.exists(),
        "Missing reviewed baseline {}. Regenerate explicitly with UPDATE_SCREENSHOTS=1",
        image.display()
    );
    if backend() == "wgpu" {
        // iced_test exposes a PNG writer instead of raw pixels. Always write a
        // fresh capture; never compare against an unreviewed GPU golden file.
        let actual = root.join("target/screenshot-gpu").join(name);
        let output = image_path(&actual);
        remove_existing(&output);
        assert!(matches(&actual));
        assert!(
            output.exists(),
            "Requested wgpu, but no wgpu capture was produced"
        );
        let expected = visual_diff::Image::read(&image);
        let rendered = visual_diff::Image::read(&output);
        if let Err(difference) = visual_diff::compare(&expected, &rendered) {
            let failure = root.join("target/screenshot-failures").join(name);
            std::fs::create_dir_all(failure.parent().unwrap()).unwrap();
            std::fs::copy(&image, backend_image_path(&failure, "expected")).unwrap();
            std::fs::copy(&output, image_path(&failure)).unwrap();
            difference
                .heatmap
                .write(&backend_image_path(&failure, "diff"));
            panic!(
                "GPU screenshot changed: {name}: {}. Expected, actual, and diff: {}-*",
                difference.reason,
                failure.display()
            );
        }
    } else if !matches(&baseline) {
        let actual = root.join("target/screenshot-failures").join(name);
        let output = image_path(&actual);
        remove_existing(&output);
        matches(&actual);
        panic!(
            "Screenshot changed: {}. Actual render: {}",
            image.display(),
            output.display()
        );
    }
}

fn remove_existing(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path).unwrap();
    }
}

pub fn image_path(prefix: &Path) -> PathBuf {
    backend_image_path(prefix, backend())
}

fn backend_image_path(prefix: &Path, backend: &str) -> PathBuf {
    prefix.with_file_name(format!(
        "{}-{backend}.png",
        prefix.file_name().unwrap().to_str().unwrap()
    ))
}
