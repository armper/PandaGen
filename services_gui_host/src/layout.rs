//! Layout vocabulary (GFX-029).
//!
//! A tiny declarative model for placing rectangles: enough for shells and
//! app surfaces to say *stack these vertically, give the editor the leftover
//! space, pin the palette in the centre* without hand-computing coordinates.
//!
//! Deliberately not a constraint solver: every node resolves in one pass,
//! integer-only, with a documented rounding rule, so layout is cheap enough
//! to run per frame inside the kernel and deterministic under test.
//!
//! Units are whatever the caller's `RasterRect` uses (pixels or cells).

use alloc::boxed::Box;
use alloc::vec::Vec;
use graphics_rasterizer::RasterRect;
use serde::{Deserialize, Serialize};

/// Identity of a leaf, chosen by the caller.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayoutId(pub u32);

/// How much of a stack's main axis a child takes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Length {
    /// Fixed size in layout units.
    Fixed(usize),
    /// Share of whatever remains after fixed children and gaps, proportional
    /// to the weight. Zero weight receives nothing.
    Weight(u32),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Where a fixed-size child sits inside its parent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Insets {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}

impl Insets {
    pub const fn uniform(all: usize) -> Self {
        Self {
            top: all,
            right: all,
            bottom: all,
            left: all,
        }
    }

    pub const fn new(top: usize, right: usize, bottom: usize, left: usize) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Shrink `rect` by these insets, saturating to an empty rect.
    pub fn apply(&self, rect: RasterRect) -> RasterRect {
        RasterRect::new(
            rect.x.saturating_add(self.left),
            rect.y.saturating_add(self.top),
            rect.width.saturating_sub(self.left + self.right),
            rect.height.saturating_sub(self.top + self.bottom),
        )
    }
}

/// A layout tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LayoutNode {
    /// Receives the rect it is given.
    Leaf(LayoutId),
    /// Children laid out along `axis`, separated by `gap`.
    Stack {
        axis: Axis,
        gap: usize,
        children: Vec<(Length, LayoutNode)>,
    },
    /// Every child receives the full rect (layers, backgrounds, overlays).
    Overlay { children: Vec<LayoutNode> },
    /// A child of fixed `width` x `height` placed by `anchor`; larger than
    /// the parent it is clamped to the parent.
    Anchored {
        anchor: Anchor,
        width: usize,
        height: usize,
        child: Box<LayoutNode>,
    },
    /// Child shrunk by insets.
    Padded {
        insets: Insets,
        child: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaf(id: u32) -> Self {
        LayoutNode::Leaf(LayoutId(id))
    }

    pub fn stack(axis: Axis, gap: usize, children: Vec<(Length, LayoutNode)>) -> Self {
        LayoutNode::Stack {
            axis,
            gap,
            children,
        }
    }

    pub fn vstack(gap: usize, children: Vec<(Length, LayoutNode)>) -> Self {
        Self::stack(Axis::Vertical, gap, children)
    }

    pub fn hstack(gap: usize, children: Vec<(Length, LayoutNode)>) -> Self {
        Self::stack(Axis::Horizontal, gap, children)
    }

    pub fn overlay(children: Vec<LayoutNode>) -> Self {
        LayoutNode::Overlay { children }
    }

    pub fn anchored(anchor: Anchor, width: usize, height: usize, child: LayoutNode) -> Self {
        LayoutNode::Anchored {
            anchor,
            width,
            height,
            child: Box::new(child),
        }
    }

    pub fn padded(insets: Insets, child: LayoutNode) -> Self {
        LayoutNode::Padded {
            insets,
            child: Box::new(child),
        }
    }

    /// Resolve the tree inside `bounds`. Leaves appear in tree order.
    pub fn solve(&self, bounds: RasterRect) -> Vec<(LayoutId, RasterRect)> {
        let mut out = Vec::new();
        self.solve_into(bounds, &mut out);
        out
    }

    fn solve_into(&self, bounds: RasterRect, out: &mut Vec<(LayoutId, RasterRect)>) {
        match self {
            LayoutNode::Leaf(id) => out.push((*id, bounds)),
            LayoutNode::Stack {
                axis,
                gap,
                children,
            } => {
                let sizes = distribute(
                    main_extent(bounds, *axis),
                    *gap,
                    children.iter().map(|(length, _)| *length),
                );
                let mut cursor = main_start(bounds, *axis);
                for ((_, child), size) in children.iter().zip(sizes) {
                    let rect = match axis {
                        Axis::Vertical => RasterRect::new(bounds.x, cursor, bounds.width, size),
                        Axis::Horizontal => RasterRect::new(cursor, bounds.y, size, bounds.height),
                    };
                    child.solve_into(rect, out);
                    cursor = cursor.saturating_add(size).saturating_add(*gap);
                }
            }
            LayoutNode::Overlay { children } => {
                for child in children {
                    child.solve_into(bounds, out);
                }
            }
            LayoutNode::Anchored {
                anchor,
                width,
                height,
                child,
            } => {
                let width = (*width).min(bounds.width);
                let height = (*height).min(bounds.height);
                let free_x = bounds.width - width;
                let free_y = bounds.height - height;
                let (fx, fy) = anchor.fractions();
                let rect = RasterRect::new(
                    bounds.x + free_x * fx / 2,
                    bounds.y + free_y * fy / 2,
                    width,
                    height,
                );
                child.solve_into(rect, out);
            }
            LayoutNode::Padded { insets, child } => child.solve_into(insets.apply(bounds), out),
        }
    }
}

impl Anchor {
    /// Horizontal and vertical placement as halves: 0 = start, 1 = middle, 2 = end.
    const fn fractions(self) -> (usize, usize) {
        match self {
            Anchor::TopLeft => (0, 0),
            Anchor::Top => (1, 0),
            Anchor::TopRight => (2, 0),
            Anchor::Left => (0, 1),
            Anchor::Center => (1, 1),
            Anchor::Right => (2, 1),
            Anchor::BottomLeft => (0, 2),
            Anchor::Bottom => (1, 2),
            Anchor::BottomRight => (2, 2),
        }
    }
}

fn main_extent(rect: RasterRect, axis: Axis) -> usize {
    match axis {
        Axis::Vertical => rect.height,
        Axis::Horizontal => rect.width,
    }
}

fn main_start(rect: RasterRect, axis: Axis) -> usize {
    match axis {
        Axis::Vertical => rect.y,
        Axis::Horizontal => rect.x,
    }
}

/// Split `total` among children: fixed lengths first (truncated in order if
/// they overflow), then the remainder to weighted children proportionally,
/// with leftover units from integer division handed to the earliest weighted
/// children so the sizes always sum to the available space.
pub fn distribute(total: usize, gap: usize, lengths: impl Iterator<Item = Length>) -> Vec<usize> {
    let lengths: Vec<Length> = lengths.collect();
    let count = lengths.len();
    if count == 0 {
        return Vec::new();
    }
    let gaps = gap.saturating_mul(count - 1);
    let mut remaining = total.saturating_sub(gaps);
    let mut sizes = alloc::vec![0; count];

    for (index, length) in lengths.iter().enumerate() {
        if let Length::Fixed(size) = length {
            let granted = (*size).min(remaining);
            sizes[index] = granted;
            remaining -= granted;
        }
    }

    let total_weight: u64 = lengths
        .iter()
        .map(|l| match l {
            Length::Weight(w) => *w as u64,
            Length::Fixed(_) => 0,
        })
        .sum();
    if total_weight == 0 {
        return sizes;
    }

    let mut handed = 0usize;
    for (index, length) in lengths.iter().enumerate() {
        if let Length::Weight(weight) = length {
            let share = (remaining as u64 * *weight as u64 / total_weight) as usize;
            sizes[index] = share;
            handed += share;
        }
    }
    let mut leftover = remaining - handed;
    for (index, length) in lengths.iter().enumerate() {
        if leftover == 0 {
            break;
        }
        if matches!(length, Length::Weight(w) if *w > 0) {
            sizes[index] += 1;
            leftover -= 1;
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn rects(node: &LayoutNode, bounds: RasterRect) -> Vec<RasterRect> {
        node.solve(bounds).into_iter().map(|(_, r)| r).collect()
    }

    #[test]
    fn test_distribute_fixed_then_weights_with_rounding() {
        assert_eq!(
            distribute(
                100,
                0,
                [Length::Fixed(30), Length::Weight(1), Length::Weight(1)].into_iter()
            ),
            vec![30, 35, 35]
        );
        // 7 remaining split 1:2 -> 2.33/4.66 -> 2,4 plus leftover 1 to the first weight.
        assert_eq!(
            distribute(
                10,
                0,
                [Length::Fixed(3), Length::Weight(1), Length::Weight(2)].into_iter()
            ),
            vec![3, 3, 4]
        );
        // Gaps are reserved before distribution.
        assert_eq!(
            distribute(
                12,
                2,
                [Length::Weight(1), Length::Weight(1), Length::Weight(1)].into_iter()
            ),
            vec![3, 3, 2]
        );
        // Fixed children that overflow are truncated in order; weights get nothing.
        assert_eq!(
            distribute(
                10,
                0,
                [Length::Fixed(8), Length::Fixed(8), Length::Weight(1)].into_iter()
            ),
            vec![8, 2, 0]
        );
        // Zero weights take nothing; sizes always sum to the space.
        assert_eq!(
            distribute(9, 0, [Length::Weight(0), Length::Weight(1)].into_iter()),
            vec![0, 9]
        );
        assert_eq!(distribute(5, 0, core::iter::empty()), Vec::<usize>::new());
    }

    #[test]
    fn test_vertical_stack_positions_children_in_order() {
        let node = LayoutNode::vstack(
            1,
            vec![
                (Length::Weight(1), LayoutNode::leaf(1)),
                (Length::Fixed(3), LayoutNode::leaf(2)),
            ],
        );
        let solved = node.solve(RasterRect::new(0, 0, 20, 10));
        assert_eq!(
            solved,
            vec![
                (LayoutId(1), RasterRect::new(0, 0, 20, 6)),
                (LayoutId(2), RasterRect::new(0, 7, 20, 3)),
            ]
        );

        let node = LayoutNode::hstack(
            0,
            vec![
                (Length::Weight(1), LayoutNode::leaf(1)),
                (Length::Weight(1), LayoutNode::leaf(2)),
                (Length::Weight(1), LayoutNode::leaf(3)),
            ],
        );
        assert_eq!(
            rects(&node, RasterRect::new(5, 5, 10, 4)),
            vec![
                RasterRect::new(5, 5, 4, 4),
                RasterRect::new(9, 5, 3, 4),
                RasterRect::new(12, 5, 3, 4),
            ]
        );
    }

    #[test]
    fn test_overlay_padding_and_anchoring() {
        let node = LayoutNode::padded(
            Insets::uniform(1),
            LayoutNode::overlay(vec![
                LayoutNode::leaf(1),
                LayoutNode::anchored(Anchor::Center, 4, 2, LayoutNode::leaf(2)),
                LayoutNode::anchored(Anchor::BottomRight, 3, 1, LayoutNode::leaf(3)),
                LayoutNode::anchored(Anchor::Top, 2, 1, LayoutNode::leaf(4)),
                // Oversized child clamps to the parent.
                LayoutNode::anchored(Anchor::TopLeft, 100, 100, LayoutNode::leaf(5)),
            ]),
        );
        let solved = node.solve(RasterRect::new(0, 0, 12, 8));
        assert_eq!(solved[0], (LayoutId(1), RasterRect::new(1, 1, 10, 6)));
        assert_eq!(solved[1], (LayoutId(2), RasterRect::new(4, 3, 4, 2)));
        assert_eq!(solved[2], (LayoutId(3), RasterRect::new(8, 6, 3, 1)));
        assert_eq!(solved[3], (LayoutId(4), RasterRect::new(5, 1, 2, 1)));
        assert_eq!(solved[4], (LayoutId(5), RasterRect::new(1, 1, 10, 6)));

        // Padding larger than the rect yields an empty rect, not a panic.
        let tiny = LayoutNode::padded(Insets::new(5, 5, 5, 5), LayoutNode::leaf(9));
        assert_eq!(rects(&tiny, RasterRect::new(0, 0, 4, 4))[0].width, 0);
    }

    #[test]
    fn test_nested_split_matches_hand_layout() {
        // Editor | (file list above status), the classic three-pane arrangement.
        let node = LayoutNode::hstack(
            1,
            vec![
                (Length::Weight(2), LayoutNode::leaf(1)),
                (
                    Length::Weight(1),
                    LayoutNode::vstack(
                        0,
                        vec![
                            (Length::Weight(1), LayoutNode::leaf(2)),
                            (Length::Fixed(2), LayoutNode::leaf(3)),
                        ],
                    ),
                ),
            ],
        );
        let solved = node.solve(RasterRect::new(0, 0, 31, 10));
        assert_eq!(solved[0].1, RasterRect::new(0, 0, 20, 10));
        assert_eq!(solved[1].1, RasterRect::new(21, 0, 10, 8));
        assert_eq!(solved[2].1, RasterRect::new(21, 8, 10, 2));
    }
}
