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
    pub(crate) acquire_micros: u128,
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
        let acquire_started_at = Instant::now();
        let mut surface_buffer = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("failed to acquire surface buffer: {e}"))?;
        let acquire_micros = acquire_started_at.elapsed().as_micros();

        let convert_started_at = Instant::now();
        write_rgba_to_surface_words(rgba, width, height, surface_buffer.as_mut())?;
        let convert_micros = convert_started_at.elapsed().as_micros();

        let present_started_at = Instant::now();
        surface_buffer
            .present()
            .map_err(|e| anyhow::anyhow!("failed to present surface buffer: {e}"))?;

        Ok(PresentStats {
            acquire_micros,
            convert_micros,
            present_micros: present_started_at.elapsed().as_micros(),
            total_micros: started_at.elapsed().as_micros(),
        })
    }
}

/// Convert an RGBA framebuffer into the packed-RGB words expected by softbuffer.
///
/// Pixels are processed in batches of 8 so the compiler can auto-vectorize the
/// inner loop with SIMD instructions on x86-64 and AArch64 without requiring
/// unstable `std::simd` or platform-specific intrinsics.  The remainder (0–7
/// pixels) is handled by a scalar fallback.
///
/// # Errors
///
/// Returns an error when the input is not 4-byte-aligned, when the pixel count
/// implied by `width × height` does not match the payload, or when the surface
/// and payload sizes diverge.
#[inline(never)]
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

    // Split into complete 8-pixel chunks and a scalar tail.
    // Using chunks_exact exposes a fixed-size inner loop (8 iterations) that
    // the backend can reliably unroll and vectorize — e.g., AVX2 can pack all
    // 8 pixels into a single 256-bit pass.
    let pixel_chunks = pixels.chunks_exact(8);
    let dst_chunks = dst.chunks_exact_mut(8);
    let remainder_len = pixel_chunks.remainder().len();

    for (src_chunk, dst_chunk) in pixel_chunks.zip(dst_chunks) {
        // Fixed-count loop: the optimizer sees exactly 8 iterations and can
        // emit a fully-unrolled, potentially vectorized sequence.
        for i in 0..8 {
            let [r, g, b, _] = src_chunk[i];
            dst_chunk[i] = pack_softbuffer_rgb(r, g, b);
        }
    }

    // Scalar tail for the 0–7 pixels that did not fill a complete batch.
    let tail_start = pixels.len() - remainder_len;
    for (&[r, g, b, _alpha], dst_word) in pixels[tail_start..].iter().zip(dst[tail_start..].iter_mut()) {
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

    /// Exercises the vectorized (8-pixel chunk) path and verifies byte-for-byte
    /// equivalence with the scalar reference.
    #[test]
    fn write_rgba_to_surface_words_converts_full_8_pixel_chunk() {
        // Exactly 8 pixels — the entire input goes through the fast path.
        #[rustfmt::skip]
        let image: [u8; 32] = [
            0x10, 0x20, 0x30, 0xff,
            0x11, 0x21, 0x31, 0x00,
            0x12, 0x22, 0x32, 0x80,
            0x13, 0x23, 0x33, 0xff,
            0x14, 0x24, 0x34, 0x01,
            0x15, 0x25, 0x35, 0xfe,
            0x16, 0x26, 0x36, 0x00,
            0x17, 0x27, 0x37, 0xff,
        ];
        let mut surface = [0u32; 8];

        write_rgba_to_surface_words(&image, 8, 1, &mut surface).expect("write surface words");

        assert_eq!(
            surface,
            [
                0x0010_2030,
                0x0011_2131,
                0x0012_2232,
                0x0013_2333,
                0x0014_2434,
                0x0015_2535,
                0x0016_2636,
                0x0017_2737,
            ]
        );
    }

    /// Exercises the split across chunk boundary (9 pixels = 8 chunked + 1 tail).
    #[test]
    fn write_rgba_to_surface_words_handles_chunk_with_tail() {
        // 9 pixels: 8 through the fast path, 1 through the scalar tail.
        let mut image = vec![0u8; 9 * 4];
        for (i, chunk) in image.chunks_exact_mut(4).enumerate() {
            let v = u8::try_from(i).expect("fits in u8");
            chunk[0] = v;
            chunk[1] = v.wrapping_add(0x10);
            chunk[2] = v.wrapping_add(0x20);
            chunk[3] = 0xff;
        }
        let mut surface = [0u32; 9];

        write_rgba_to_surface_words(&image, 9, 1, &mut surface).expect("write surface words");

        for (i, &word) in surface.iter().enumerate() {
            let v = u8::try_from(i).expect("fits in u8");
            let expected = pack_softbuffer_rgb(v, v.wrapping_add(0x10), v.wrapping_add(0x20));
            assert_eq!(word, expected, "pixel {i} mismatch");
        }
    }
}
