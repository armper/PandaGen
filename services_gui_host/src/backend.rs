//! Renderer backend contract (GFX-050).
//!
//! A backend turns a `DesktopScene` into pixels that the presenter can put
//! on a framebuffer. The software backend is authoritative: it is the
//! reference for correctness (golden fixtures, property tests) and always
//! available. A GPU backend later is an optional implementation of this same
//! trait; it may accelerate composition but must produce the same
//! `DesktopScene` semantics, and the software path remains the fallback.
//!
//! The contract is deliberately narrow: describe capabilities, render a
//! scene (optionally limited to damage), expose the RGBA result, and report
//! stats. Presentation stays with the kernel presenter.

use alloc::string::String;
use graphics_rasterizer::{RasterRect, RgbaBuffer};
use serde::{Deserialize, Serialize};

use crate::{Compositor, DesktopScene, RasterRenderStats, Theme};

/// What a backend can do; lets the desktop pick features safely.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub name: String,
    /// Repaints limited to a damage rectangle are supported.
    pub damage_repaint: bool,
    /// Rendering happens on separate hardware (true for a GPU backend).
    pub accelerated: bool,
    /// Largest surface the backend will allocate, in pixels.
    pub max_surface_pixels: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackendError {
    /// The requested surface exceeds `max_surface_pixels`.
    SurfaceTooLarge { requested: usize, max: usize },
    /// The scene's size does not match the backend surface.
    SizeMismatch {
        expected: (usize, usize),
        actual: (usize, usize),
    },
}

/// A renderer that owns a surface and composes scenes into it.
pub trait RenderBackend {
    fn capabilities(&self) -> BackendCapabilities;

    /// Surface size in pixels.
    fn surface_size(&self) -> (usize, usize);

    /// Compose `scene` in full into the surface.
    fn render(&mut self, scene: &DesktopScene) -> Result<RasterRenderStats, BackendError>;

    /// Repaint only `damage` (the surface must already hold the previous
    /// frame). Backends without damage support may render in full.
    fn render_damage(
        &mut self,
        scene: &DesktopScene,
        damage: RasterRect,
    ) -> Result<RasterRenderStats, BackendError>;

    /// Tightly packed RGBA8888 pixels of the current surface.
    fn pixels(&self) -> &[u8];

    /// Frames rendered since creation.
    fn frames_rendered(&self) -> u64;
}

/// The authoritative CPU backend over the compositor.
pub struct SoftwareBackend {
    compositor: Compositor,
    surface: RgbaBuffer,
    frames: u64,
}

impl SoftwareBackend {
    /// Largest surface the software backend accepts (4K at 4 bytes/pixel is
    /// about 33 MiB; this keeps a misconfigured mode from exhausting a
    /// bare-metal heap).
    pub const MAX_SURFACE_PIXELS: usize = 3840 * 2160;

    pub fn new(width: usize, height: usize, theme: Theme) -> Result<Self, BackendError> {
        let requested = width.saturating_mul(height);
        if requested > Self::MAX_SURFACE_PIXELS {
            return Err(BackendError::SurfaceTooLarge {
                requested,
                max: Self::MAX_SURFACE_PIXELS,
            });
        }
        Ok(Self {
            compositor: Compositor::with_theme(theme),
            surface: RgbaBuffer::new(width, height, theme.background),
            frames: 0,
        })
    }

    pub fn compositor(&self) -> &Compositor {
        &self.compositor
    }

    fn check_size(&self, scene: &DesktopScene) -> Result<(), BackendError> {
        let expected = self.surface_size();
        let actual = scene.pixel_size();
        // Scenes may be smaller than the surface (unused margin); larger is
        // an error because content would be cut off silently.
        if actual.0 > expected.0 || actual.1 > expected.1 {
            return Err(BackendError::SizeMismatch { expected, actual });
        }
        Ok(())
    }
}

impl RenderBackend for SoftwareBackend {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: String::from("software"),
            damage_repaint: true,
            accelerated: false,
            max_surface_pixels: Self::MAX_SURFACE_PIXELS,
        }
    }

    fn surface_size(&self) -> (usize, usize) {
        (self.surface.width(), self.surface.height())
    }

    fn render(&mut self, scene: &DesktopScene) -> Result<RasterRenderStats, BackendError> {
        self.check_size(scene)?;
        let stats = self.compositor.render_scene_full(&mut self.surface, scene);
        self.frames += 1;
        Ok(stats)
    }

    fn render_damage(
        &mut self,
        scene: &DesktopScene,
        damage: RasterRect,
    ) -> Result<RasterRenderStats, BackendError> {
        self.check_size(scene)?;
        let mut limited = scene.clone();
        limited.damage = Some(damage);
        let stats = self.compositor.render_scene(&mut self.surface, &limited);
        self.frames += 1;
        Ok(stats)
    }

    fn pixels(&self) -> &[u8] {
        self.surface.as_bytes()
    }

    fn frames_rendered(&self) -> u64 {
        self.frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DesktopCursor, DesktopWindow, SurfaceRect, SurfaceSize, RASTER_CELL_HEIGHT,
        RASTER_CELL_WIDTH,
    };
    use alloc::string::ToString;
    use alloc::vec;
    use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

    fn scene() -> DesktopScene {
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["backend".to_string()]),
            0,
        )
        .with_title("Main");
        DesktopScene::new(
            SurfaceSize::new(20, 8),
            vec![DesktopWindow::new(frame, SurfaceRect::new(1, 1, 10, 5)).focused()],
        )
        .with_cursor(Some(DesktopCursor::new(30, 30)))
    }

    #[test]
    fn test_software_backend_matches_direct_compositor_rendering() {
        let s = scene();
        let (w, h) = s.pixel_size();
        let mut backend = SoftwareBackend::new(w, h, Theme::DEFAULT).unwrap();
        assert_eq!(backend.capabilities().name, "software");
        assert!(backend.capabilities().damage_repaint);
        assert!(!backend.capabilities().accelerated);
        let stats = backend.render(&s).unwrap();
        assert_eq!(stats.painted_windows, 1);
        let direct = Compositor::new().render_scene_rgba(&s);
        assert_eq!(backend.pixels(), direct.pixels.as_slice());
        assert_eq!(backend.frames_rendered(), 1);

        // Damage repaint of a moved cursor equals a full render.
        let mut moved = s.clone();
        moved.cursor = Some(DesktopCursor::new(60, 40));
        let damage = crate::diff_scenes(&s, &moved).damage.unwrap();
        backend.render_damage(&moved, damage).unwrap();
        let full = Compositor::new().render_scene_rgba(&moved);
        assert_eq!(backend.pixels(), full.pixels.as_slice());
        assert_eq!(backend.frames_rendered(), 2);
    }

    #[test]
    fn test_backend_rejects_oversized_surfaces_and_scenes() {
        assert_eq!(
            SoftwareBackend::new(10_000, 10_000, Theme::DEFAULT).err(),
            Some(BackendError::SurfaceTooLarge {
                requested: 100_000_000,
                max: SoftwareBackend::MAX_SURFACE_PIXELS
            })
        );
        let mut backend =
            SoftwareBackend::new(5 * RASTER_CELL_WIDTH, 5 * RASTER_CELL_HEIGHT, Theme::LIGHT)
                .unwrap();
        let err = backend.render(&scene()).unwrap_err();
        assert!(matches!(err, BackendError::SizeMismatch { .. }));
        assert_eq!(backend.frames_rendered(), 0);
        // A smaller scene than the surface is fine.
        let small = DesktopScene::new(SurfaceSize::new(3, 3), vec![]);
        backend.render(&small).unwrap();
        assert_eq!(
            backend.pixels()[0..4],
            [
                Theme::LIGHT.background.r,
                Theme::LIGHT.background.g,
                Theme::LIGHT.background.b,
                255
            ]
        );
    }

    #[test]
    fn test_backend_is_usable_through_the_trait_object() {
        let s = scene();
        let (w, h) = s.pixel_size();
        let mut backend: alloc::boxed::Box<dyn RenderBackend> =
            alloc::boxed::Box::new(SoftwareBackend::new(w, h, Theme::DEFAULT).unwrap());
        backend.render(&s).unwrap();
        assert_eq!(backend.surface_size(), (w, h));
        assert_eq!(backend.pixels().len(), w * h * 4);
    }
}
