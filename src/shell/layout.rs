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

    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x && other.y >= self.y && other.right() <= self.right() && other.bottom() <= self.bottom()
    }
}

/// Where a window (outer frame rectangle) has to go so it lies inside
/// `area`: shrunk to at most the area's size, then moved inside. `None` if
/// it already fits.
///
/// `min` is the smallest outer size the window accepts. A window that cannot
/// shrink enough is aligned so its bottom-right corner stays visible: that
/// is where GTK dialogs keep their action buttons (Save, OK).
pub fn fit_within(outer: Rect, area: Rect, min: (u32, u32)) -> Option<Rect> {
    if area.contains(&outer) {
        return None;
    }
    let width = outer.width.min(area.width).max(min.0.min(outer.width));
    let height = outer.height.min(area.height).max(min.1.min(outer.height));
    let x = if width > area.width {
        area.right() - width as i32
    } else {
        outer.x.clamp(area.x, area.right() - width as i32)
    };
    let y = if height > area.height {
        area.bottom() - height as i32
    } else {
        outer.y.clamp(area.y, area.bottom() - height as i32)
    };
    Some(Rect::new(x, y, width, height))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub orientation: Orientation,
    pub screen: Rect,
    pub top_bar: Rect,
    pub bottom_bar: Rect,
    /// Space left for applications (and the Today screen) between the bars.
    pub workarea: Rect,
    /// The largest the Start drop-down may be; it is as tall as its content.
    pub start_menu: Rect,
    /// Columns of favorites on Today.
    pub today_columns: u32,
    /// Columns of the Programs and Settings icon grids.
    pub grid_columns: u32,
    /// Today shows its items in two columns instead of one.
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

        // Pocket PC style: the Start menu drops down from the top-left
        // corner, under the Start button, in both orientations; the program
        // behind it stays visible.
        let menu_w = d.start_menu_width_px.clamp(200, width);
        let start_menu = Rect::new(0, top as i32, menu_w, work_h);

        // An icon cell is the icon plus a label of about eight characters
        // per line, never under ~90 px.
        let cell = (d.today_icon_px + d.font_px * 4).max(90);
        let today_side_by_side = orientation == Orientation::Landscape;
        let favorites_area = if today_side_by_side { width / 2 } else { width };
        let today_columns = (favorites_area / cell).clamp(2, 8);
        let grid_columns = (width / cell).clamp(3, 8);

        Self {
            orientation,
            screen,
            top_bar,
            bottom_bar,
            workarea,
            start_menu,
            today_columns,
            grid_columns,
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
    fn start_menu_drops_down_under_the_start_button() {
        for (w, h) in [(600, 1024), (1024, 600)] {
            let l = Layout::compute(w, h, &design());
            assert_eq!((l.start_menu.x, l.start_menu.y), (0, l.workarea.y), "{w}x{h}");
            assert_eq!(l.start_menu.width, design().start_menu_width_px, "not full width");
            assert_eq!(l.start_menu.height, l.workarea.height, "at most the work area");
        }
        assert!(!Layout::compute(600, 1024, &design()).today_side_by_side);
        assert!(Layout::compute(1024, 600, &design()).today_side_by_side);
    }

    #[test]
    fn pocketpc_grids_are_four_columns_in_portrait() {
        let mut d = design();
        d.font_px = 22;
        d.today_icon_px = 44;
        let p = Layout::compute(600, 1024, &d);
        assert_eq!(p.grid_columns, 4, "like the Pocket PC Programs folder");
        assert!(Layout::compute(1024, 600, &d).grid_columns > p.grid_columns);
    }

    #[test]
    fn today_columns_are_reasonable() {
        let p = Layout::compute(600, 1024, &design());
        let l = Layout::compute(1024, 600, &design());
        assert!((4..=8).contains(&p.today_columns), "portrait {}", p.today_columns);
        assert!((3..=8).contains(&l.today_columns), "landscape {}", l.today_columns);
    }

    #[test]
    fn larger_text_gives_fewer_shortcut_columns() {
        let mut d = design();
        let compact = Layout::compute(600, 1024, &d).today_columns;
        d.font_px = 22;
        d.today_icon_px = 44;
        let readable = Layout::compute(600, 1024, &d).today_columns;
        assert!(readable < compact, "{readable} < {compact}");
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
    fn fitting_windows_into_the_work_area() {
        let wa = Rect::new(0, 24, 600, 974);
        let any = (0, 0);
        // Already inside: nothing to do.
        assert_eq!(fit_within(Rect::new(10, 100, 300, 200), wa, any), None);
        // A file chooser wider than the portrait screen that can shrink.
        assert_eq!(fit_within(Rect::new(-100, 200, 800, 570), wa, any), Some(Rect::new(0, 200, 600, 570)));
        // Hanging below the command bar: moved up, size kept.
        assert_eq!(fit_within(Rect::new(50, 800, 400, 300), wa, any), Some(Rect::new(50, 698, 400, 300)));
        // Covering the status bar.
        assert_eq!(fit_within(Rect::new(50, 0, 400, 300), wa, any), Some(Rect::new(50, 24, 400, 300)));
        // Larger than the area in both directions.
        assert_eq!(fit_within(Rect::new(-5, -5, 2000, 2000), wa, any), Some(wa));
    }

    #[test]
    fn dialogs_that_cannot_shrink_keep_their_buttons_visible() {
        let wa = Rect::new(0, 24, 600, 974);
        // Gnumeric's Save As needs 667 px with its frame: it stays that wide
        // and is aligned right, so Save (bottom right) remains on screen.
        let fit = fit_within(Rect::new(0, 83, 667, 881), wa, (667, 450)).unwrap();
        assert_eq!(fit, Rect::new(-67, 83, 667, 881));
        assert_eq!(fit.right(), wa.right());
        // Too tall as well: the bottom edge (buttons) stays above the bar.
        let fit = fit_within(Rect::new(0, 0, 700, 1200), Rect::new(0, 24, 1024, 550), (700, 600)).unwrap();
        assert_eq!(fit.bottom(), 574);
        assert_eq!(fit.height, 600);
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
