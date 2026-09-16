pub(super) fn attachment(uri: &str) -> Option<(&str, &str)> {
    let file = uri.strip_prefix("attachment://")?;
    let (hash, extension) = file.split_once('.')?;
    (hash.len() == 64
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && matches!(extension, "png" | "jpg" | "jpeg" | "gif" | "webp"))
    .then_some((file, hash))
}
#[cfg(test)]
mod tests {
    #[test]
    fn attachments_stay_inside_the_account_directory() {
        let hash = "a".repeat(64);
        assert!(super::attachment(&format!("attachment://{hash}.png")).is_some());
        for uri in [
            "attachment://../../secret.png",
            "file:///private/file.png",
            "https://example.com/a.png",
        ] {
            assert!(super::attachment(uri).is_none());
        }
    }
}
