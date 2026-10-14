//! Pure geometry for placing the panel: coordinate conversion, screen selection and sizing.
//! No system calls, so it is tested on every platform.

/// A rectangle in points. Which corner is the origin depends on the coordinate system in use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Converts a rectangle from top-left-origin coordinates (Quartz / Accessibility) to AppKit's
/// bottom-left-origin coordinates. `primary_height` is the height of the primary screen.
pub fn quartz_to_cocoa(rect: Rect, primary_height: f64) -> Rect {
    Rect {
        y: primary_height - rect.y - rect.height,
        ..rect
    }
}

/// Index of the screen whose frame contains `point`.
pub fn screen_containing(screens: &[Rect], point: (f64, f64)) -> Option<usize> {
    screens.iter().position(|s| s.contains(point))
}

/// Panel size for a screen's visible area (spec §6): at most 1040pt wide with a 16pt margin on
/// each side, and at most 640pt or 72% of the height.
pub fn panel_size(visible: Rect) -> (f64, f64) {
    (
        (visible.width - 32.0).min(1040.0),
        (visible.height * 0.72).min(640.0).round(),
    )
}

/// Bottom-left origin (AppKit) that centers a panel of `size` in `visible`, a little above the
/// middle, which looks more centered.
pub fn centered_origin(visible: Rect, (width, height): (f64, f64)) -> (f64, f64) {
    let x = visible.x + (visible.width - width) / 2.0;
    let free = visible.height - height;
    let y = visible.y + free / 2.0 + (free / 2.0).min(visible.height * 0.04);
    (x.round(), y.round())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn converts_top_left_origin_to_appkit_coordinates() {
        // A 400x300 window 100pt below the top of a 1000pt-tall primary screen.
        let cocoa = quartz_to_cocoa(rect(50.0, 100.0, 400.0, 300.0), 1000.0);
        assert_eq!(cocoa, rect(50.0, 600.0, 400.0, 300.0));
    }

    #[test]
    fn finds_the_screen_containing_a_point() {
        // Primary 1440x900 at the origin, secondary 1920x1080 to its right.
        let screens = [
            rect(0.0, 0.0, 1440.0, 900.0),
            rect(1440.0, 0.0, 1920.0, 1080.0),
        ];
        assert_eq!(screen_containing(&screens, (100.0, 100.0)), Some(0));
        assert_eq!(screen_containing(&screens, (2000.0, 500.0)), Some(1));
        assert_eq!(screen_containing(&screens, (-5.0, 100.0)), None);
    }

    #[test]
    fn panel_size_fits_the_screen() {
        assert_eq!(panel_size(rect(0.0, 0.0, 1920.0, 1055.0)), (1040.0, 640.0));
        // A small screen: width minus margins, 72% of the height.
        assert_eq!(panel_size(rect(0.0, 0.0, 1000.0, 600.0)), (968.0, 432.0));
    }

    #[test]
    fn panel_is_centered_slightly_above_the_middle() {
        let visible = rect(1440.0, 25.0, 1920.0, 1055.0);
        let (x, y) = centered_origin(visible, (1040.0, 640.0));
        assert_eq!(x, 1440.0 + 440.0);
        assert!(y > 25.0 + (1055.0 - 640.0) / 2.0);
        assert!(y + 640.0 <= 25.0 + 1055.0);
    }
}
