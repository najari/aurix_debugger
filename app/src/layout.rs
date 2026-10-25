//! The sizes of the panels, changed by dragging the splitters between them and kept in
//! `%APPDATA%\aurix-debugger\layout.json`. A drag moves a splitter by the distance the mouse
//! went since it was pressed; a double click puts it back where it starts.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The height of the toolbar, about: the center column is the window less it and the bottom
/// panel.
pub const TOOLBAR: f32 = 36.;
/// What a panel keeps at least.
const MIN_SIDE: f32 = 140.;
const MIN_BOTTOM: f32 = 90.;
/// What the center column keeps at least, across and down.
const MIN_CENTER_WIDTH: f32 = 320.;
const MIN_CENTER_HEIGHT: f32 = 160.;
/// The source view's share of the center column, at least and at most.
const MIN_SOURCE: f32 = 0.1;
const MAX_SOURCE: f32 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// The threads and call stack, across.
    pub left: f32,
    /// The variables, across.
    pub right: f32,
    /// The bottom panel, down.
    pub bottom: f32,
    /// The source view's share of the center column (the disassembly has the rest).
    pub source: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            left: 260.,
            right: 340.,
            bottom: 210.,
            source: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Splitter {
    /// Between the threads and the center.
    Left,
    /// Between the center and the variables.
    Right,
    /// Above the bottom panel.
    Bottom,
    /// Between the source and the disassembly.
    Source,
}

impl Splitter {
    /// Whether it is dragged across (a vertical bar).
    pub fn across(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// A drag under way: the splitter, where the mouse was pressed, the layout then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub splitter: Splitter,
    start: (f32, f32),
    layout: Layout,
}

impl Layout {
    pub fn begin(&self, splitter: Splitter, mouse: (f32, f32)) -> Drag {
        Drag {
            splitter,
            start: mouse,
            layout: *self,
        }
    }

    /// The layout with `drag`'s splitter moved to `mouse`, in a window of `window` (width,
    /// height).
    pub fn dragged(drag: &Drag, mouse: (f32, f32), window: (f32, f32)) -> Self {
        let (dx, dy) = (mouse.0 - drag.start.0, mouse.1 - drag.start.1);
        let mut layout = drag.layout;
        match drag.splitter {
            Splitter::Left => layout.left += dx,
            Splitter::Right => layout.right -= dx,
            Splitter::Bottom => layout.bottom -= dy,
            Splitter::Source => {
                let center = layout.center_height(window.1);
                layout.source += dy / center;
            }
        }
        layout.fitted(window)
    }

    /// The splitter back where it starts.
    pub fn reset(&mut self, splitter: Splitter, window: (f32, f32)) {
        let start = Self::default();
        match splitter {
            Splitter::Left => self.left = start.left,
            Splitter::Right => self.right = start.right,
            Splitter::Bottom => self.bottom = start.bottom,
            Splitter::Source => self.source = start.source,
        }
        *self = self.fitted(window);
    }

    pub fn center_height(&self, window_height: f32) -> f32 {
        (window_height - TOOLBAR - self.bottom).max(1.)
    }

    /// The source view's height in a window `window_height` high.
    pub fn source_height(&self, window_height: f32) -> f32 {
        self.source * self.center_height(window_height)
    }

    /// The layout limited so that every panel keeps its least in a window of `window`.
    pub fn fitted(mut self, (width, height): (f32, f32)) -> Self {
        let max_sides = (width - MIN_CENTER_WIDTH).max(2. * MIN_SIDE);
        // The side that is not dragged keeps its width while it can.
        let right = self
            .right
            .clamp(MIN_SIDE, (max_sides - MIN_SIDE).max(MIN_SIDE));
        self.left = self.left.clamp(MIN_SIDE, (max_sides - right).max(MIN_SIDE));
        self.right = self
            .right
            .clamp(MIN_SIDE, (max_sides - self.left).max(MIN_SIDE));
        let max_bottom = (height - TOOLBAR - MIN_CENTER_HEIGHT).max(MIN_BOTTOM);
        self.bottom = self.bottom.clamp(MIN_BOTTOM, max_bottom);
        self.source = self.source.clamp(MIN_SOURCE, MAX_SOURCE);
        self
    }
}

fn layout_file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(base)
            .join("aurix-debugger")
            .join("layout.json"),
    )
}

/// The layout kept, or the one it starts with.
pub fn load() -> Layout {
    layout_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(layout: &Layout) -> Result<(), String> {
    let Some(path) = layout_file() else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(layout).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: (f32, f32) = (1280., 800.);

    #[test]
    fn a_drag_moves_by_the_distance_of_the_mouse() {
        let layout = Layout::default();
        let drag = layout.begin(Splitter::Left, (260., 300.));
        assert_eq!(Layout::dragged(&drag, (360., 310.), WINDOW).left, 360.);
        // The right panel grows when its splitter goes left.
        let drag = layout.begin(Splitter::Right, (940., 300.));
        assert_eq!(Layout::dragged(&drag, (900., 300.), WINDOW).right, 380.);
        let drag = layout.begin(Splitter::Bottom, (600., 590.));
        assert_eq!(Layout::dragged(&drag, (600., 490.), WINDOW).bottom, 310.);
        // The center column is 800 - 36 - 210 = 554 high: 55.4 px down is a tenth more.
        let drag = layout.begin(Splitter::Source, (600., 300.));
        let source = Layout::dragged(&drag, (600., 355.4), WINDOW).source;
        assert!((source - 0.6).abs() < 1e-4);
    }

    #[test]
    fn panels_keep_their_least_and_the_center_too() {
        let layout = Layout::default();
        let drag = layout.begin(Splitter::Left, (260., 0.));
        assert_eq!(Layout::dragged(&drag, (0., 0.), WINDOW).left, MIN_SIDE);
        let wide = Layout::dragged(&drag, (5000., 0.), WINDOW);
        assert_eq!(wide.left + wide.right, WINDOW.0 - MIN_CENTER_WIDTH);
        assert_eq!(wide.right, layout.right);
        let drag = layout.begin(Splitter::Bottom, (0., 590.));
        let tall = Layout::dragged(&drag, (0., -1000.), WINDOW);
        assert_eq!(tall.bottom, WINDOW.1 - TOOLBAR - MIN_CENTER_HEIGHT);
        let drag = layout.begin(Splitter::Source, (0., 300.));
        assert_eq!(
            Layout::dragged(&drag, (0., 5000.), WINDOW).source,
            MAX_SOURCE
        );

        let mut moved = wide;
        moved.reset(Splitter::Left, WINDOW);
        assert_eq!(moved.left, 260.);
    }

    #[test]
    fn a_kept_layout_reads_back_and_missing_fields_start_as_default() {
        let layout: Layout = serde_json::from_str(r#"{ "left": 300 }"#).unwrap();
        assert_eq!(layout.left, 300.);
        assert_eq!(layout.bottom, Layout::default().bottom);
    }
}
