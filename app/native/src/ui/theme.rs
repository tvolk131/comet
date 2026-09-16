//! Map Comet's bundled light/dark palettes to Material roles. Keep the original
//! neutral surfaces and blue accent instead of tinting every pane with the seed.
use crate::infra::themes::ThemeData;
use iced::Color;
use iced_m3::Theme;
use std::sync::OnceLock;

pub fn theme(dark: bool) -> Theme {
    static THEMES: OnceLock<[Theme; 2]> = OnceLock::new();
    THEMES.get_or_init(|| {
        [
            material(include_str!("../../assets/themes/light.json"), false),
            material(include_str!("../../assets/themes/dark.json"), true),
        ]
    })[usize::from(dark)]
    .clone()
}

fn material(source: &str, dark: bool) -> Theme {
    let palette: ThemeData = serde_json::from_str(source).expect("valid bundled Comet palette");
    let color = |name: &str| -> Color {
        palette.colors[name]
            .parse()
            .expect("valid bundled Comet color")
    };
    let background = color("background");
    let foreground = color("foreground");
    let primary = color("primary");
    let mut theme = Theme::from_accent(primary, dark);
    let c = &mut theme.colors;
    c.primary = primary;
    c.primary_container = blend(background, primary, if dark { 0.18 } else { 0.12 });
    c.on_primary_container = foreground;
    c.secondary = color("muted-foreground");
    c.on_secondary = background;
    c.secondary_container = color("accent");
    c.on_secondary_container = color("accent-foreground");
    c.tertiary_container = c.primary_container;
    c.on_tertiary_container = foreground;
    c.background = background;
    c.on_background = foreground;
    c.surface = background;
    c.on_surface = foreground;
    c.on_surface_variant = color("muted-foreground");
    c.surface_variant = color("secondary");
    c.surface_container_lowest = background;
    c.surface_container_low = color("sidebar");
    c.surface_container = color("sidebar");
    c.surface_container_high = color("muted");
    c.surface_container_highest = color("accent");
    c.surface_dim = color("secondary");
    c.surface_bright = if dark { color("accent") } else { background };
    c.surface_tint = primary;
    c.outline = color("border");
    c.outline_variant = color("separator");
    c.error = color("destructive");
    c.inverse_surface = foreground;
    c.inverse_on_surface = background;
    c.shadow = color("shadow-color");
    c.scrim = Color {
        a: 1.0,
        ..color("overlay-backdrop")
    };
    theme
}

fn blend(background: Color, foreground: Color, amount: f32) -> Color {
    Color::from_rgb(
        background.r + (foreground.r - background.r) * amount,
        background.g + (foreground.g - background.g) * amount,
        background.b + (foreground.b - background.b) * amount,
    )
}
