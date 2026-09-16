//! Shared geometry for painting, hit testing, and dragging the editor scrollbar.
use iced::{Point, Rectangle, Size};

pub(super) struct Scrollbar {
    pub track: Rectangle,
    pub thumb: Rectangle,
    max_scroll: f32,
}

impl Scrollbar {
    pub fn new(viewport: Rectangle, content_height: f32, scroll: f32) -> Option<Self> {
        if viewport.height <= 0.0 || content_height <= viewport.height {
            return None;
        }
        let height = (viewport.height.powi(2) / content_height)
            .max(24.0)
            .min(viewport.height);
        let max_scroll = content_height - viewport.height;
        let y = scroll / max_scroll * (viewport.height - height);
        Some(Self {
            track: Rectangle::new(
                Point::new(viewport.x + viewport.width - 14.0, viewport.y),
                Size::new(14.0, viewport.height),
            ),
            thumb: Rectangle::new(
                Point::new(viewport.x + viewport.width - 8.0, viewport.y + y),
                Size::new(6.0, height),
            ),
            max_scroll,
        })
    }

    pub fn scroll_to(&self, pointer_y: f32, grabbed_at: f32) -> f32 {
        let travel = self.track.height - self.thumb.height;
        if travel <= 0.0 {
            return 0.0;
        }
        ((pointer_y - self.track.y - self.thumb.height * grabbed_at) / travel).clamp(0.0, 1.0)
            * self.max_scroll
    }
}
