//! Presentation backends for the native client.
//!
//! The current Windows-native path keeps `softbuffer` as the default backend,
//! but the backend boundary lives here so later Windows GPU experiments can be
//! added without rewriting the app/session glue again.

use core::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use winit::window::Window;

pub(crate) struct PresentStats {
    pub(crate) convert_micros: u128,
    pub(crate) present_micros: u128,
    pub(crate) total_micros: u128,
}

pub(crate) trait PresentationBackend {
    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> anyhow::Result<()>;
    fn present_rgba(&mut self, rgba: &[u8], width: u16, height: u16) -> anyhow::Result<PresentStats>;
}

pub(crate) struct SoftbufferBackend {
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    context: softbuffer::Context<Arc<Window>>,
}

impl SoftbufferBackend {
    pub(crate) fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let context = softbuffer::Context::new(Arc::clone(&window))
            .map_err(|e| anyhow::anyhow!("unable to initialize softbuffer context: {e}"))?;
        let surface = softbuffer::Surface::new(&context, window)
            .map_err(|e| anyhow::anyhow!("unable to initialize softbuffer surface: {e}"))?;

        Ok(Self { surface, context })
    }
}

impl PresentationBackend for SoftbufferBackend {
    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> anyhow::Result<()> {
        self.surface
            .resize(width, height)
            .map_err(|e| anyhow::anyhow!("failed to resize drawing surface: {e}"))
    }

    fn present_rgba(&mut self, rgba: &[u8], width: u16, height: u16) -> anyhow::Result<PresentStats> {
        let started_at = Instant::now();
        let _keep_context_alive = &self.context;
        let mut surface_buffer = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("failed to acquire surface buffer: {e}"))?;

        let convert_started_at = Instant::now();
        write_rgba_to_surface_words(rgba, width, height, surface_buffer.as_mut())?;
        let convert_micros = convert_started_at.elapsed().as_micros();

        let present_started_at = Instant::now();
        surface_buffer
            .present()
            .map_err(|e| anyhow::anyhow!("failed to present surface buffer: {e}"))?;

        Ok(PresentStats {
            convert_micros,
            present_micros: present_started_at.elapsed().as_micros(),
            total_micros: started_at.elapsed().as_micros(),
        })
    }
}

fn write_rgba_to_surface_words(rgba: &[u8], width: u16, height: u16, dst: &mut [u32]) -> anyhow::Result<()> {
    let (pixels, remainder) = rgba.as_chunks::<4>();
    if !remainder.is_empty() {
        anyhow::bail!("decoded image length is not divisible by four");
    }

    let expected_pixels = usize::from(width) * usize::from(height);
    if expected_pixels != pixels.len() {
        anyhow::bail!(
            "frame dimensions and pixel payload diverged: width={}, height={}, frame_pixels={}",
            width,
            height,
            pixels.len()
        );
    }

    if dst.len() != pixels.len() {
        anyhow::bail!(
            "surface and framebuffer sizes diverged: surface_pixels={}, frame_pixels={}",
            dst.len(),
            pixels.len()
        );
    }

    for (&[r, g, b, _alpha], dst_word) in pixels.iter().zip(dst.iter_mut()) {
        *dst_word = pack_softbuffer_rgb(r, g, b);
    }

    Ok(())
}

#[inline]
fn pack_softbuffer_rgb(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

#[cfg(test)]
mod tests {
    use super::{pack_softbuffer_rgb, write_rgba_to_surface_words};

    #[test]
    fn write_rgba_to_surface_words_converts_pixels_for_softbuffer() {
        let image = [
            0x11, 0x22, 0x33, 0xff, //
            0x44, 0x55, 0x66, 0x77,
        ];
        let mut surface = [0u32; 2];

        write_rgba_to_surface_words(&image, 2, 1, &mut surface).expect("write surface words");

        assert_eq!(surface, [0x0011_2233, 0x0044_5566]);
    }

    #[test]
    fn write_rgba_to_surface_words_rejects_unaligned_input() {
        let image = [0x11, 0x22, 0x33];
        let mut surface = [0u32; 1];

        let error = write_rgba_to_surface_words(&image, 1, 1, &mut surface).expect_err("unaligned input should fail");

        assert!(
            error
                .to_string()
                .contains("decoded image length is not divisible by four")
        );
    }

    #[test]
    fn write_rgba_to_surface_words_rejects_dimension_mismatch() {
        let image = [
            0x11, 0x22, 0x33, 0xff, //
            0x44, 0x55, 0x66, 0x77,
        ];
        let mut surface = [0u32; 2];

        let error =
            write_rgba_to_surface_words(&image, 1, 1, &mut surface).expect_err("dimension mismatch should fail");

        assert!(
            error
                .to_string()
                .contains("frame dimensions and pixel payload diverged")
        );
    }

    #[test]
    fn pack_softbuffer_rgb_uses_expected_channel_layout() {
        assert_eq!(pack_softbuffer_rgb(0x11, 0x22, 0x33), 0x0011_2233);
    }
}
