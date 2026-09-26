//! GPU display device exploration (GFX-051).
//!
//! This is the *minimum* a hardware-backed display device must offer for
//! the desktop's `RenderBackend` contract: a scanout surface of known size
//! and format, region uploads of RGBA pixels, and an explicit flush that
//! makes uploads visible. Composition semantics stay in `services_gui_host`
//! (see `CompositionStage`); a GPU backend built on this trait may run the
//! pixel stages on the device but must produce the same pixels as the
//! software backend.
//!
//! The QEMU target for a real implementation is virtio-gpu (2D scanout with
//! `RESOURCE_CREATE_2D`, `TRANSFER_TO_HOST_2D`, `SET_SCANOUT`,
//! `RESOURCE_FLUSH`), which maps onto exactly these three operations. Until
//! that driver exists, `FakeGpu` records the calls so the contract can be
//! tested on the host.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuSurfaceInfo {
    pub width: u32,
    pub height: u32,
    /// Bytes per row of the device's scanout buffer.
    pub stride_bytes: u32,
}

/// Pixel rectangle on the scanout surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    /// The rectangle is not inside the surface.
    OutOfBounds,
    /// The pixel slice does not match `width * height * 4`.
    LengthMismatch { expected: usize, actual: usize },
    /// The device rejected or timed out on the command.
    DeviceFailure,
}

/// A device that can scan out an RGBA surface.
pub trait GpuSurfaceDevice {
    fn surface_info(&self) -> GpuSurfaceInfo;

    /// Upload tightly packed RGBA8888 pixels into `rect`.
    fn upload_rgba(&mut self, rect: GpuRect, pixels: &[u8]) -> Result<(), GpuError>;

    /// Make uploads inside `rect` visible on the scanout.
    fn flush(&mut self, rect: GpuRect) -> Result<(), GpuError>;
}

/// Validate a rectangle against a surface.
pub fn check_rect(info: GpuSurfaceInfo, rect: GpuRect) -> Result<(), GpuError> {
    let right = rect
        .x
        .checked_add(rect.width)
        .ok_or(GpuError::OutOfBounds)?;
    let bottom = rect
        .y
        .checked_add(rect.height)
        .ok_or(GpuError::OutOfBounds)?;
    if right > info.width || bottom > info.height {
        return Err(GpuError::OutOfBounds);
    }
    Ok(())
}

/// Records uploads and flushes; the host-side stand-in for a device.
#[cfg(feature = "alloc")]
pub struct FakeGpu {
    info: GpuSurfaceInfo,
    pub uploads: alloc::vec::Vec<(GpuRect, usize)>,
    pub flushes: alloc::vec::Vec<GpuRect>,
    pub fail_next: bool,
}

#[cfg(feature = "alloc")]
impl FakeGpu {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            info: GpuSurfaceInfo {
                width,
                height,
                stride_bytes: width * 4,
            },
            uploads: alloc::vec::Vec::new(),
            flushes: alloc::vec::Vec::new(),
            fail_next: false,
        }
    }
}

#[cfg(feature = "alloc")]
impl GpuSurfaceDevice for FakeGpu {
    fn surface_info(&self) -> GpuSurfaceInfo {
        self.info
    }

    fn upload_rgba(&mut self, rect: GpuRect, pixels: &[u8]) -> Result<(), GpuError> {
        check_rect(self.info, rect)?;
        let expected = rect.width as usize * rect.height as usize * 4;
        if pixels.len() != expected {
            return Err(GpuError::LengthMismatch {
                expected,
                actual: pixels.len(),
            });
        }
        if core::mem::take(&mut self.fail_next) {
            return Err(GpuError::DeviceFailure);
        }
        self.uploads.push((rect, pixels.len()));
        Ok(())
    }

    fn flush(&mut self, rect: GpuRect) -> Result<(), GpuError> {
        check_rect(self.info, rect)?;
        self.flushes.push(rect);
        Ok(())
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_fake_gpu_validates_and_records() {
        let mut gpu = FakeGpu::new(64, 32);
        assert_eq!(gpu.surface_info().stride_bytes, 256);
        let rect = GpuRect {
            x: 8,
            y: 4,
            width: 2,
            height: 2,
        };
        gpu.upload_rgba(rect, &[0; 16]).unwrap();
        gpu.flush(rect).unwrap();
        assert_eq!(gpu.uploads, vec![(rect, 16)]);
        assert_eq!(gpu.flushes, vec![rect]);

        assert_eq!(
            gpu.upload_rgba(rect, &[0; 15]),
            Err(GpuError::LengthMismatch {
                expected: 16,
                actual: 15
            })
        );
        let outside = GpuRect {
            x: 63,
            y: 0,
            width: 2,
            height: 1,
        };
        assert_eq!(
            gpu.upload_rgba(outside, &[0; 8]),
            Err(GpuError::OutOfBounds)
        );
        assert_eq!(gpu.flush(outside), Err(GpuError::OutOfBounds));
        let overflow = GpuRect {
            x: u32::MAX,
            y: 0,
            width: 1,
            height: 1,
        };
        assert_eq!(gpu.flush(overflow), Err(GpuError::OutOfBounds));

        gpu.fail_next = true;
        assert_eq!(
            gpu.upload_rgba(rect, &[0; 16]),
            Err(GpuError::DeviceFailure)
        );
        assert_eq!(gpu.uploads.len(), 1, "failed upload not recorded");
    }
}
