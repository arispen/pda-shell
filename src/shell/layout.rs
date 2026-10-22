//! Screen layout decisions. Pure functions of the screen size and the design
//! tokens, so both orientations can be unit-tested without a display.

use crate::config::Design;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn of(width: u32, height: u32) -> Self {
        if height > width {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub orientation: Orientation,
    pub screen: Rect,
    pub top_bar: Rect,
    pub bottom_bar: Rect,
    /// Space left for applications (and the Today screen) between the bars.
    pub workarea: Rect,
    pub start_menu: Rect,
    /// Columns of application shortcuts on Today.
    pub today_columns: u32,
    /// Today puts recent documents beside the shortcuts instead of below.
    pub today_side_by_side: bool,
}

impl Layout {
    pub fn compute(width: u32, height: u32, d: &Design) -> Self {
        let orientation = Orientation::of(width, height);
        let top = d.top_bar_px.min(height / 4);
        let bottom = d.bottom_bar_px.min(height / 4);
        let work_h = height.saturating_sub(top + bottom);

        let screen = Rect::new(0, 0, width, height);
        let top_bar = Rect::new(0, 0, width, top);
        let bottom_bar = Rect::new(0, (height - bottom) as i32, width, bottom);
        let workarea = Rect::new(0, top as i32, width, work_h);

        // Pocket PC style: the Start menu drops down from the top-left corner.
        // In portrait it uses the full width; in landscape a fixed column so
        // the running application stays visible beside it.
        let menu_w = match orientation {
            Orientation::Portrait => width,
            Orientation::Landscape => d.start_menu_width_px.clamp(200, width),
        };
        let start_menu = Rect::new(0, top as i32, menu_w, work_h);

        // A shortcut cell is roughly icon + label; keep cells at least ~90px
        // wide so labels like "Spreadsheet" fit at small font sizes.
        let cell = (d.today_icon_px + 66).max(90);
        let today_side_by_side = orientation == Orientation::Landscape;
        let shortcut_area = if today_side_by_side { width / 2 } else { width };
        let today_columns = (shortcut_area / cell).clamp(2, 8);

        Self {
            orientation,
            screen,
            top_bar,
            bottom_bar,
            workarea,
            start_menu,
            today_columns,
            today_side_by_side,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn design() -> Design {
        Design::default()
    }

    #[test]
    fn orientation_from_size() {
        assert_eq!(Orientation::of(600, 1024), Orientation::Portrait);
        assert_eq!(Orientation::of(1024, 600), Orientation::Landscape);
        assert_eq!(Orientation::of(800, 800), Orientation::Landscape);
    }

    #[test]
    fn bars_and_workarea_tile_the_screen() {
        for (w, h) in [(600, 1024), (1024, 600)] {
            let l = Layout::compute(w, h, &design());
            assert_eq!(l.top_bar.y, 0);
            assert_eq!(l.workarea.y as u32, l.top_bar.height);
            assert_eq!(
                l.top_bar.height + l.workarea.height + l.bottom_bar.height,
                h,
                "{w}x{h}"
            );
            assert_eq!(l.bottom_bar.y as u32 + l.bottom_bar.height, h);
            assert_eq!(l.workarea.width, w);
        }
    }

    #[test]
    fn portrait_start_menu_is_full_width() {
        let l = Layout::compute(600, 1024, &design());
        assert_eq!(l.start_menu.width, 600);
        assert_eq!(l.start_menu.y, l.workarea.y);
        assert_eq!(l.start_menu.height, l.workarea.height);
        assert!(!l.today_side_by_side);
    }

    #[test]
    fn landscape_start_menu_is_a_column() {
        let l = Layout::compute(1024, 600, &design());
        assert_eq!(l.start_menu.width, design().start_menu_width_px);
        assert!(l.today_side_by_side);
    }

    #[test]
    fn today_columns_are_reasonable() {
        let p = Layout::compute(600, 1024, &design());
        let l = Layout::compute(1024, 600, &design());
        assert!((4..=8).contains(&p.today_columns), "portrait {}", p.today_columns);
        assert!((3..=8).contains(&l.today_columns), "landscape {}", l.today_columns);
    }

    #[test]
    fn oversized_bars_are_clamped() {
        let mut d = design();
        d.top_bar_px = 500;
        d.bottom_bar_px = 500;
        let l = Layout::compute(1024, 600, &d);
        assert_eq!(l.top_bar.height, 150);
        assert_eq!(l.workarea.height, 300);
    }

    #[test]
    fn narrow_start_menu_width_is_clamped() {
        let mut d = design();
        d.start_menu_width_px = 50;
        assert_eq!(Layout::compute(1024, 600, &d).start_menu.width, 200);
        d.start_menu_width_px = 5000;
        assert_eq!(Layout::compute(1024, 600, &d).start_menu.width, 1024);
    }
}
