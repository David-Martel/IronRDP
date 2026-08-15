//! Codec decoder traits for client-side EGFX processing
//!
//! This module provides pluggable decoder traits that allow consumers
//! to bring their own codec implementations (e.g., openh264, ffmpeg,
//! hardware decoders). The traits are designed for core tier: no I/O,
//! `Send` only. They are intended for use in `std` environments;
//! `no_std` + `alloc` support is not currently guaranteed.
//!
//! # Protocol Context
//!
//! H.264 data arrives inside [RFX_AVC420_BITMAP_STREAM][1] payloads
//! within `RDPGFX_WIRE_TO_SURFACE_PDU_1` messages. The NAL units
//! are in AVC format (4-byte big-endian length prefix per NAL unit),
//! not Annex B (start code prefix).
//!
//! [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpegfx/d65c3f9c-2088-4302-90c0-53adc0e11a78

use core::fmt;

// ============================================================================
// Decoded Frame
// ============================================================================

/// Decoded bitmap frame from an H.264 decoder
///
/// Contains RGBA pixel data for a decoded H.264 frame.
/// The pixel data is in RGBA format (4 bytes per pixel),
/// row-major, top-to-bottom, left-to-right.
#[derive(Clone)]
pub struct DecodedFrame {
    /// RGBA pixel data (4 bytes per pixel)
    pub data: Vec<u8>,
    /// Frame width in pixels
    pub width: u32,
    /// Frame height in pixels
    pub height: u32,
}

impl fmt::Debug for DecodedFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecodedFrame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("data_len", &self.data.len())
            .finish()
    }
}

/// Decoded YUV 4:2:0 planar frame from an H.264 decoder.
///
/// Unlike [`DecodedFrame`] (which is already converted to RGBA), this exposes
/// the raw luma/chroma planes needed to reconstruct YUV 4:4:4 from an AVC444
/// dual-stream ([MS-RDPEGFX] 2.2.4.4/2.2.4.5). Planes are tightly packed
/// (stride == plane width, no padding): `y` is `width * height` bytes and each
/// of `u`/`v` is `chroma_width * chroma_height` bytes, where
/// `chroma_width = (width + 1) / 2` and `chroma_height = (height + 1) / 2`.
///
/// `width`/`height` are the decoder's reported dimensions, which are
/// macroblock-aligned (multiples of 16) and therefore may exceed the logical
/// destination rectangle. Callers reconstruct/crop against the destination.
///
/// [MS-RDPEGFX]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpegfx/da5c75f9-cd99-450c-98c4-014a496942b0
#[derive(Clone)]
pub struct Yuv420Frame {
    /// Luma plane (`width * height` bytes, tightly packed).
    pub y: Vec<u8>,
    /// Blue-difference chroma plane (`chroma_width * chroma_height` bytes).
    pub u: Vec<u8>,
    /// Red-difference chroma plane (`chroma_width * chroma_height` bytes).
    pub v: Vec<u8>,
    /// Frame width in pixels (macroblock-aligned).
    pub width: u32,
    /// Frame height in pixels (macroblock-aligned).
    pub height: u32,
}

impl Yuv420Frame {
    /// Chroma plane width for a `width`-pixel luma plane (4:2:0 subsampling).
    #[must_use]
    pub const fn chroma_width(width: u32) -> u32 {
        width.div_ceil(2)
    }

    /// Chroma plane height for a `height`-pixel luma plane (4:2:0 subsampling).
    #[must_use]
    pub const fn chroma_height(height: u32) -> u32 {
        height.div_ceil(2)
    }
}

impl fmt::Debug for Yuv420Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Yuv420Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("y_len", &self.y.len())
            .field("u_len", &self.u.len())
            .field("v_len", &self.v.len())
            .finish()
    }
}

// ============================================================================
// Decoder Error
// ============================================================================

/// Error type for decoder operations
#[derive(Debug)]
pub struct DecoderError {
    context: String,
    source: Option<Box<dyn core::error::Error + Send + Sync>>,
}

impl DecoderError {
    /// Create a decoder error with a source error
    pub fn new(context: impl Into<String>, source: impl core::error::Error + Send + Sync + 'static) -> Self {
        Self {
            context: context.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create a decoder error with only a message
    pub fn msg(context: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            source: None,
        }
    }
}

impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "decoder error: {}", self.context)?;
        if let Some(ref source) = self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl core::error::Error for DecoderError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.source.as_deref().map(|e| {
            let err: &(dyn core::error::Error + 'static) = e;
            err
        })
    }
}

/// Result type for decoder operations
pub type DecoderResult<T> = Result<T, DecoderError>;

// ============================================================================
// H.264 Decoder Trait
// ============================================================================

/// Trait for H.264 (AVC) decoders
///
/// Implement this trait to provide H.264 decode capability to the
/// EGFX client. The decoder receives AVC-format NAL units (length-prefixed,
/// not Annex B) from `RFX_AVC420_BITMAP_STREAM` payloads.
///
/// # Thread Safety
///
/// Implementations must be `Send` to work with the DVC framework.
///
/// # Example
///
/// ```ignore
/// use ironrdp_egfx::decode::{H264Decoder, DecodedFrame, DecoderResult};
///
/// struct MyH264Decoder { /* ... */ }
///
/// impl H264Decoder for MyH264Decoder {
///     fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame> {
///         // Decode H.264 NAL units to RGBA
///         todo!()
///     }
/// }
/// ```
pub trait H264Decoder: Send {
    /// Decode AVC-format H.264 NAL units (4-byte BE length prefix, not Annex B)
    /// into an RGBA bitmap.
    ///
    /// Frame dimensions may exceed the destination rectangle due to
    /// macroblock alignment (16x16). The caller crops to fit.
    fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame>;

    /// Decode AVC-format H.264 NAL units into a planar YUV 4:2:0 frame.
    ///
    /// This is the entry point used by the AVC444 dual-stream path, which needs
    /// the raw luma/chroma planes (not RGBA) to reconstruct YUV 4:4:4 before the
    /// color conversion. Decoders that only produce RGBA may leave this
    /// unimplemented; the default returns an error so AVC444 simply degrades to
    /// "not decoded" rather than failing the whole channel.
    ///
    /// A given decoder instance is a single H.264 decode context and MUST NOT be
    /// shared between the luma and chroma sub-streams of AVC444: those are two
    /// independent H.264 sequences and interleaving them corrupts inter-frame
    /// prediction. The client uses a dedicated decoder for the chroma stream.
    fn decode_yuv(&mut self, _data: &[u8]) -> DecoderResult<Yuv420Frame> {
        Err(DecoderError::msg("YUV 4:2:0 decode not supported by this decoder"))
    }

    /// Reset the decoder state
    ///
    /// Called when surfaces are reset (e.g., on `ResetGraphics`).
    /// The decoder should drop any internal state and prepare for
    /// a new stream.
    fn reset(&mut self) {
        // Default: no-op
    }
}

// ============================================================================
// OpenH264 Implementation
// ============================================================================

#[cfg(feature = "openh264")]
mod openh264_impl {
    use openh264::formats::YUVSource as _;

    use super::{DecodedFrame, DecoderError, DecoderResult, H264Decoder, Yuv420Frame};
    use tracing::warn;

    /// Copy a decoder plane (which may have `src_stride > width` padding) into a
    /// tightly packed `width * height` buffer.
    fn pack_plane(src: &[u8], src_stride: usize, width: usize, height: usize) -> Vec<u8> {
        let mut dst = vec![0u8; width.saturating_mul(height)];
        for row in 0..height {
            let src_start = row.saturating_mul(src_stride);
            let src_end = src_start.saturating_add(width);
            let dst_start = row.saturating_mul(width);
            let dst_end = dst_start.saturating_add(width);
            if src_end <= src.len() && dst_end <= dst.len() {
                dst[dst_start..dst_end].copy_from_slice(&src[src_start..src_end]);
            }
        }
        dst
    }

    /// H.264 decoder backed by Cisco's OpenH264 library
    ///
    /// This decoder converts AVC-format NAL units to Annex B format
    /// (as required by OpenH264), decodes to YUV420p, then converts
    /// to RGBA for the client pipeline.
    ///
    /// # Feature Gates
    ///
    /// Two construction paths are available depending on the feature flags:
    ///
    /// - `openh264-bundled`: compiles OpenH264 from source at build time.
    ///   Use [`OpenH264Decoder::new()`] to construct.
    ///
    /// - `openh264-libloading`: loads a prebuilt Cisco OpenH264 binary at
    ///   runtime. Use [`OpenH264Decoder::from_library_path()`] to construct.
    ///   The library is verified against known Cisco release hashes.
    pub struct OpenH264Decoder {
        decoder: openh264::decoder::Decoder,
        annex_b_buffer: Vec<u8>,
    }

    impl OpenH264Decoder {
        /// Create a decoder using the bundled (source-compiled) OpenH264 library
        ///
        /// This compiles OpenH264 C code at build time. The resulting binary
        /// has no patent coverage from Cisco's license agreement.
        #[cfg(feature = "openh264-bundled")]
        pub fn new() -> DecoderResult<Self> {
            let decoder = openh264::decoder::Decoder::new()
                .map_err(|e| DecoderError::new("failed to create OpenH264 decoder", e))?;

            Ok(Self {
                decoder,
                annex_b_buffer: Vec::new(),
            })
        }

        /// Create a decoder using a dynamically loaded OpenH264 library
        ///
        /// `library_path` should point to a Cisco OpenH264 prebuilt binary,
        /// which is verified against known Cisco release hashes before loading.
        /// Cisco's prebuilt binaries carry patent coverage under their license.
        #[cfg(feature = "openh264-libloading")]
        pub fn from_library_path(library_path: &std::path::Path) -> DecoderResult<Self> {
            let api = openh264::OpenH264API::from_blob_path(library_path)
                .map_err(|e| DecoderError::new("failed to load OpenH264 library", e))?;
            let decoder = openh264::decoder::Decoder::with_api_config(api, Default::default())
                .map_err(|e| DecoderError::new("failed to create OpenH264 decoder", e))?;

            Ok(Self {
                decoder,
                annex_b_buffer: Vec::new(),
            })
        }

        /// Convert AVC format (4-byte BE length prefix) to Annex B (start codes)
        fn avc_to_annex_b(&mut self, data: &[u8]) {
            self.annex_b_buffer.clear();
            let mut offset = 0;

            while offset + 4 <= data.len() {
                let nal_len = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);

                #[expect(clippy::as_conversions, reason = "NAL length from wire format")]
                let nal_len = nal_len as usize;
                offset += 4;

                // Use checked addition to prevent overflow on malicious input
                let Some(end) = offset.checked_add(nal_len) else {
                    warn!(nal_len, offset, "AVC NAL length overflow, discarding remaining data");
                    break;
                };
                if end > data.len() {
                    warn!(
                        nal_len,
                        offset,
                        data_len = data.len(),
                        "AVC NAL extends beyond buffer, discarding remaining data"
                    );
                    break;
                }

                // Annex B start code
                self.annex_b_buffer.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                self.annex_b_buffer.extend_from_slice(&data[offset..offset + nal_len]);
                offset += nal_len;
            }
        }
    }

    impl H264Decoder for OpenH264Decoder {
        fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame> {
            self.avc_to_annex_b(data);

            let yuv = self
                .decoder
                .decode(&self.annex_b_buffer)
                .map_err(|e| DecoderError::new("OpenH264 decode failed", e))?
                .ok_or_else(|| DecoderError::msg("OpenH264 returned no picture"))?;

            let (width, height) = openh264::formats::YUVSource::dimensions(&yuv);

            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "H.264 frame dimensions are always within u32 range"
            )]
            let (w32, h32) = (width as u32, height as u32);

            let rgba_size = width
                .checked_mul(height)
                .and_then(|s| s.checked_mul(4))
                .ok_or_else(|| DecoderError::msg("frame dimensions too large for RGBA allocation"))?;
            let mut rgba = vec![0u8; rgba_size];
            yuv.write_rgba8(&mut rgba);

            Ok(DecodedFrame {
                data: rgba,
                width: w32,
                height: h32,
            })
        }

        fn decode_yuv(&mut self, data: &[u8]) -> DecoderResult<Yuv420Frame> {
            self.avc_to_annex_b(data);

            let yuv = self
                .decoder
                .decode(&self.annex_b_buffer)
                .map_err(|e| DecoderError::new("OpenH264 decode failed", e))?
                .ok_or_else(|| DecoderError::msg("OpenH264 returned no picture"))?;

            let (width, height) = yuv.dimensions();
            let (y_stride, u_stride, v_stride) = yuv.strides();
            let chroma_width = width.div_ceil(2);
            let chroma_height = height.div_ceil(2);

            let y = pack_plane(yuv.y(), y_stride, width, height);
            let u = pack_plane(yuv.u(), u_stride, chroma_width, chroma_height);
            let v = pack_plane(yuv.v(), v_stride, chroma_width, chroma_height);

            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "H.264 frame dimensions are always within u32 range"
            )]
            let (w32, h32) = (width as u32, height as u32);

            Ok(Yuv420Frame {
                y,
                u,
                v,
                width: w32,
                height: h32,
            })
        }

        fn reset(&mut self) {
            // Recreate decoder from source when available
            #[cfg(feature = "openh264-bundled")]
            match openh264::decoder::Decoder::new() {
                Ok(new_decoder) => self.decoder = new_decoder,
                Err(e) => warn!("Failed to reset OpenH264 decoder, reusing existing state: {e}"),
            }
            // In libloading-only mode, we don't have the library path stored,
            // so we can't recreate. The existing decoder handles new SPS/PPS
            // transparently when the next I-frame arrives.
        }
    }
}

#[cfg(feature = "openh264")]
pub use openh264_impl::OpenH264Decoder;
