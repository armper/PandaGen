//! # Tiling Module
//!
//! Provides split-screen tiling for VGA console. Enables multiple components
//! to share the screen in a tiled layout without full context swapping.
//!
//! ## Philosophy
//!
//! - **Text-mode only**: No pixel-based windowing
//! - **Deterministic layout**: Same inputs → same tile positions
//! - **Explicit focus**: Focus is explicitly managed, not implicit
//! - **No overlapping**: Tiles are non-overlapping rectangles

use core::cmp::min;

/// Tile bounds in character coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileBounds {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl TileBounds {
    /// Create new tile bounds
    pub fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Check if a position is inside this tile
    pub fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Get the area of the tile in characters
    pub fn area(&self) -> usize {
        self.width * self.height
    }
}

/// Tile identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileId {
    Top,
    Bottom,
}

/// Split layout types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitLayout {
    /// Horizontal split (editor top, CLI bottom)
    Horizontal { top_height: usize },
}

impl SplitLayout {
    /// Create a horizontal split layout
    pub fn horizontal(top_height: usize) -> Self {
        Self::Horizontal { top_height }
    }
}

/// Tile manager for split-screen layouts
pub struct TileManager {
    /// Screen dimensions
    screen_width: usize,
    screen_height: usize,
    /// Current layout
    layout: SplitLayout,
    /// Currently focused tile
    focused: TileId,
}

impl TileManager {
    /// Create a new tile manager
    pub fn new(screen_width: usize, screen_height: usize, layout: SplitLayout) -> Self {
        Self {
            screen_width,
            screen_height,
            layout,
            focused: TileId::Top, // Default focus to top tile
        }
    }

    /// Get the bounds of a specific tile
    pub fn get_tile_bounds(&self, tile_id: TileId) -> TileBounds {
        match self.layout {
            SplitLayout::Horizontal { top_height } => {
                let top_height = min(top_height, self.screen_height - 1); // Leave at least 1 line for bottom
                match tile_id {
                    TileId::Top => TileBounds::new(0, 0, self.screen_width, top_height),
                    TileId::Bottom => TileBounds::new(
                        0,
                        top_height,
                        self.screen_width,
                        self.screen_height - top_height,
                    ),
                }
            }
        }
    }

    /// Get the currently focused tile
    pub fn focused_tile(&self) -> TileId {
        self.focused
    }

    /// Switch focus to the next tile
    pub fn focus_next(&mut self) {
        self.focused = match self.focused {
            TileId::Top => TileId::Bottom,
            TileId::Bottom => TileId::Top,
        };
    }

    /// Set focus to a specific tile
    pub fn set_focus(&mut self, tile_id: TileId) {
        self.focused = tile_id;
    }

    /// Check if a tile has focus
    pub fn has_focus(&self, tile_id: TileId) -> bool {
        self.focused == tile_id
    }

    /// Get the layout
    pub fn layout(&self) -> SplitLayout {
        self.layout
    }

    /// Update the layout
    pub fn set_layout(&mut self, layout: SplitLayout) {
        self.layout = layout;
    }

    /// Get screen dimensions
    pub fn screen_size(&self) -> (usize, usize) {
        (self.screen_width, self.screen_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_bounds_creation() {
        let bounds = TileBounds::new(10, 5, 20, 10);
        assert_eq!(bounds.x, 10);
        assert_eq!(bounds.y, 5);
        assert_eq!(bounds.width, 20);
        assert_eq!(bounds.height, 10);
    }

    #[test]
    fn test_tile_bounds_contains() {
        let bounds = TileBounds::new(10, 5, 20, 10);

        // Inside
        assert!(bounds.contains(10, 5)); // Top-left
        assert!(bounds.contains(29, 14)); // Bottom-right
        assert!(bounds.contains(15, 10)); // Middle

        // Outside
        assert!(!bounds.contains(9, 5)); // Left of bounds
        assert!(!bounds.contains(30, 5)); // Right of bounds
        assert!(!bounds.contains(10, 4)); // Above bounds
        assert!(!bounds.contains(10, 15)); // Below bounds
    }

    #[test]
    fn test_tile_bounds_area() {
        let bounds = TileBounds::new(0, 0, 80, 25);
        assert_eq!(bounds.area(), 2000);

        let bounds = TileBounds::new(0, 0, 40, 10);
        assert_eq!(bounds.area(), 400);
    }

    #[test]
    fn test_split_layout_creation() {
        let layout = SplitLayout::horizontal(15);
        match layout {
            SplitLayout::Horizontal { top_height } => {
                assert_eq!(top_height, 15);
            }
        }
    }

    #[test]
    fn test_tile_manager_creation() {
        let layout = SplitLayout::horizontal(15);
        let manager = TileManager::new(80, 25, layout);

        assert_eq!(manager.screen_size(), (80, 25));
        assert_eq!(manager.focused_tile(), TileId::Top);
    }

    #[test]
    fn test_tile_manager_horizontal_split() {
        let layout = SplitLayout::horizontal(15);
        let manager = TileManager::new(80, 25, layout);

        let top_bounds = manager.get_tile_bounds(TileId::Top);
        assert_eq!(top_bounds.x, 0);
        assert_eq!(top_bounds.y, 0);
        assert_eq!(top_bounds.width, 80);
        assert_eq!(top_bounds.height, 15);

        let bottom_bounds = manager.get_tile_bounds(TileId::Bottom);
        assert_eq!(bottom_bounds.x, 0);
        assert_eq!(bottom_bounds.y, 15);
        assert_eq!(bottom_bounds.width, 80);
        assert_eq!(bottom_bounds.height, 10);
    }

    #[test]
    fn test_tile_manager_focus_switching() {
        let layout = SplitLayout::horizontal(15);
        let mut manager = TileManager::new(80, 25, layout);

        assert_eq!(manager.focused_tile(), TileId::Top);
        assert!(manager.has_focus(TileId::Top));
        assert!(!manager.has_focus(TileId::Bottom));

        manager.focus_next();
        assert_eq!(manager.focused_tile(), TileId::Bottom);
        assert!(!manager.has_focus(TileId::Top));
        assert!(manager.has_focus(TileId::Bottom));

        manager.focus_next();
        assert_eq!(manager.focused_tile(), TileId::Top);
    }

    #[test]
    fn test_tile_manager_set_focus() {
        let layout = SplitLayout::horizontal(15);
        let mut manager = TileManager::new(80, 25, layout);

        manager.set_focus(TileId::Bottom);
        assert_eq!(manager.focused_tile(), TileId::Bottom);

        manager.set_focus(TileId::Top);
        assert_eq!(manager.focused_tile(), TileId::Top);
    }

    #[test]
    fn test_tile_manager_layout_change() {
        let layout1 = SplitLayout::horizontal(15);
        let mut manager = TileManager::new(80, 25, layout1);

        let bounds = manager.get_tile_bounds(TileId::Top);
        assert_eq!(bounds.height, 15);

        let layout2 = SplitLayout::horizontal(20);
        manager.set_layout(layout2);

        let bounds = manager.get_tile_bounds(TileId::Top);
        assert_eq!(bounds.height, 20);
    }

    #[test]
    fn test_tile_manager_minimum_bottom_height() {
        // Even if top_height is too large, bottom should get at least 1 line
        let layout = SplitLayout::horizontal(30);
        let manager = TileManager::new(80, 25, layout);

        let top_bounds = manager.get_tile_bounds(TileId::Top);
        assert_eq!(top_bounds.height, 24); // Clamped to leave 1 line

        let bottom_bounds = manager.get_tile_bounds(TileId::Bottom);
        assert_eq!(bottom_bounds.height, 1); // At least 1 line
    }

    #[test]
    fn test_tile_bounds_non_overlapping() {
        let layout = SplitLayout::horizontal(15);
        let manager = TileManager::new(80, 25, layout);

        let top_bounds = manager.get_tile_bounds(TileId::Top);
        let bottom_bounds = manager.get_tile_bounds(TileId::Bottom);

        // Check that tiles don't overlap
        for y in 0..25 {
            for x in 0..80 {
                let in_top = top_bounds.contains(x, y);
                let in_bottom = bottom_bounds.contains(x, y);
                // Each cell should be in exactly one tile
                assert!(in_top ^ in_bottom, "Position ({}, {}) overlap issue", x, y);
            }
        }
    }
}
