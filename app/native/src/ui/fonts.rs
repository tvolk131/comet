//! Static emphasis faces avoid platform font fallback for variable-font weights.
pub fn ensure_loaded() {
    static LOAD: std::sync::Once = std::sync::Once::new();
    LOAD.call_once(|| {
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system");
        for font in [
            include_bytes!("../../assets/fonts/Roboto-Bold.ttf").as_slice(),
            include_bytes!("../../assets/fonts/FiraMono-Medium.ttf").as_slice(),
            include_bytes!("../../assets/fonts/Roboto-Italic.ttf").as_slice(),
            include_bytes!("../../assets/fonts/Roboto-BoldItalic.ttf").as_slice(),
        ] {
            fonts.load_font(std::borrow::Cow::Borrowed(font));
        }
    });
}
