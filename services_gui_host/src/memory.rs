//! Memory budgeting for surfaces and off-screen buffers (GFX-047).
//!
//! Large pixel buffers are the dominant memory users of the desktop, and on
//! bare metal they come out of a fixed heap. `SurfaceBudget` makes those
//! costs explicit: each buffer reserves a labelled amount up front, a
//! reservation that would exceed the limit fails with a typed error before
//! any allocation happens, and the current picture is reportable. Policy
//! (how much of the heap the desktop may take) belongs to the caller.

use alloc::vec::Vec;
use serde::Serialize;

/// Bytes for a tightly packed RGBA8888 surface.
pub const fn rgba_surface_bytes(width: usize, height: usize) -> usize {
    width.saturating_mul(height).saturating_mul(4)
}

/// Bytes for a strided native framebuffer surface.
pub const fn native_surface_bytes(
    stride_pixels: usize,
    height: usize,
    bytes_per_pixel: usize,
) -> usize {
    stride_pixels
        .saturating_mul(height)
        .saturating_mul(bytes_per_pixel)
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum BudgetError {
    /// The reservation would exceed the limit.
    Exceeded {
        label: &'static str,
        requested: usize,
        available: usize,
    },
    /// A reservation with this label already exists.
    Duplicate { label: &'static str },
}

/// A labelled reservation.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Reservation {
    pub label: &'static str,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SurfaceBudget {
    limit: usize,
    reservations: Vec<Reservation>,
    /// Reservations refused since creation.
    refusals: u32,
}

impl SurfaceBudget {
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            reservations: Vec::new(),
            refusals: 0,
        }
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }

    pub fn used(&self) -> usize {
        self.reservations.iter().map(|r| r.bytes).sum()
    }

    pub fn available(&self) -> usize {
        self.limit.saturating_sub(self.used())
    }

    pub const fn refusals(&self) -> u32 {
        self.refusals
    }

    pub fn reservations(&self) -> &[Reservation] {
        &self.reservations
    }

    pub fn has(&self, label: &str) -> bool {
        self.reservations.iter().any(|r| r.label == label)
    }

    /// Reserve `bytes` under `label`, or explain why not. Nothing is
    /// allocated here; the caller allocates only after success.
    pub fn reserve(
        &mut self,
        label: &'static str,
        bytes: usize,
    ) -> Result<Reservation, BudgetError> {
        if self.has(label) {
            return Err(BudgetError::Duplicate { label });
        }
        let available = self.available();
        if bytes > available {
            self.refusals = self.refusals.saturating_add(1);
            return Err(BudgetError::Exceeded {
                label,
                requested: bytes,
                available,
            });
        }
        let reservation = Reservation { label, bytes };
        self.reservations.push(reservation);
        Ok(reservation)
    }

    /// Release the reservation with `label`; returns the bytes freed.
    pub fn release(&mut self, label: &str) -> usize {
        match self.reservations.iter().position(|r| r.label == label) {
            Some(index) => self.reservations.remove(index).bytes,
            None => 0,
        }
    }

    /// Whether `bytes` more could be reserved right now.
    pub fn can_reserve(&self, bytes: usize) -> bool {
        bytes <= self.available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_sizes() {
        assert_eq!(rgba_surface_bytes(1280, 800), 4_096_000);
        assert_eq!(native_surface_bytes(1280, 800, 4), 4_096_000);
        assert_eq!(native_surface_bytes(1344, 800, 4), 4_300_800);
        assert_eq!(rgba_surface_bytes(usize::MAX, 2), usize::MAX);
    }

    #[test]
    fn test_reserve_release_and_refusal() {
        let mut budget = SurfaceBudget::new(10_000_000);
        let shadow = budget.reserve("text shadow", 4_096_000).unwrap();
        assert_eq!(shadow.bytes, 4_096_000);
        assert_eq!(budget.used(), 4_096_000);
        assert!(budget.can_reserve(4_096_000));
        budget.reserve("desktop target", 4_096_000).unwrap();
        assert_eq!(budget.available(), 1_808_000);
        assert!(!budget.can_reserve(2_000_000));

        // Over budget: typed error, nothing changes.
        let err = budget.reserve("glyph atlas", 2_000_000).unwrap_err();
        assert_eq!(
            err,
            BudgetError::Exceeded {
                label: "glyph atlas",
                requested: 2_000_000,
                available: 1_808_000,
            }
        );
        assert_eq!(budget.refusals(), 1);
        assert_eq!(budget.reservations().len(), 2);

        // Duplicate labels are rejected without counting as a refusal.
        assert_eq!(
            budget.reserve("text shadow", 1),
            Err(BudgetError::Duplicate {
                label: "text shadow"
            })
        );
        assert_eq!(budget.refusals(), 1);

        // Release frees the exact amount; unknown labels free nothing.
        assert_eq!(budget.release("desktop target"), 4_096_000);
        assert_eq!(budget.release("desktop target"), 0);
        assert!(budget.can_reserve(2_000_000));
        budget.reserve("glyph atlas", 2_000_000).unwrap();
        assert!(budget.has("glyph atlas"));
        assert_eq!(budget.limit(), 10_000_000);
    }
}
