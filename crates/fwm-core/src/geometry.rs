use serde::{Deserialize, Serialize};
use std::fmt;

/// Top-left corner of a window in GUI pixels. Same layout as `agui::Point`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Same layout as `agui::Rectangle`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{},{}", self.x, self.y)
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Move `pos` so a window of size `window` stays fully inside `bounds`. A window
/// bigger than the screen is pinned to the top/left edge so its title bar stays reachable.
pub fn clamp_to_bounds(pos: Point, window: Size, bounds: Size) -> Point {
    let max_x = (bounds.width - window.width).max(0);
    let max_y = (bounds.height - window.height).max(0);
    Point {
        x: pos.x.clamp(0, max_x),
        y: pos.y.clamp(0, max_y),
    }
}

/// The area `agui::Window::center` centred a window in, worked out from where it put it.
/// Only a fallback for when the game window's client size can't be read.
pub fn bounds_from_centered(centered: Point, window: Size) -> Size {
    Size {
        width: centered.x * 2 + window.width,
        height: centered.y * 2 + window.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size {
        width: 1920,
        height: 1080,
    };
    const WINDOW: Size = Size {
        width: 800,
        height: 600,
    };

    #[test]
    fn position_inside_the_screen_is_kept() {
        let p = Point { x: 100, y: 200 };
        assert_eq!(clamp_to_bounds(p, WINDOW, SCREEN), p);
    }

    #[test]
    fn position_past_the_right_and_bottom_edges_is_pulled_back() {
        let p = Point { x: 1800, y: 1000 };
        assert_eq!(
            clamp_to_bounds(p, WINDOW, SCREEN),
            Point { x: 1120, y: 480 }
        );
    }

    #[test]
    fn negative_position_is_pulled_onto_the_screen() {
        let p = Point { x: -50, y: -10 };
        assert_eq!(clamp_to_bounds(p, WINDOW, SCREEN), Point { x: 0, y: 0 });
    }

    #[test]
    fn window_bigger_than_the_screen_is_pinned_top_left() {
        let huge = Size {
            width: 2500,
            height: 1200,
        };
        let p = Point { x: 300, y: 300 };
        assert_eq!(clamp_to_bounds(p, huge, SCREEN), Point { x: 0, y: 0 });
    }

    #[test]
    fn bounds_are_recovered_from_a_centred_window() {
        let centered = Point { x: 560, y: 240 };
        assert_eq!(bounds_from_centered(centered, WINDOW), SCREEN);
    }

    #[test]
    fn display_formats_match_the_log_format() {
        assert_eq!(Point { x: -3, y: 7 }.to_string(), "-3,7");
        assert_eq!(WINDOW.to_string(), "800x600");
    }
}
