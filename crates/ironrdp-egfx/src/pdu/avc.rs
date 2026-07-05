use core::fmt;

use bit_field::BitField as _;
use bitflags::bitflags;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_pdu::{
    Decode, DecodeResult, Encode, EncodeResult, ReadCursor, WriteCursor, cast_length, ensure_fixed_part_size,
    ensure_size, invalid_field_err,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantQuality {
    pub quantization_parameter: u8,
    pub progressive: bool,
    pub quality: u8,
}

impl QuantQuality {
    const NAME: &'static str = "GfxQuantQuality";

    const FIXED_PART_SIZE: usize = 1 /* data */ + 1 /* quality */;
}

impl Encode for QuantQuality {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);

        let mut data = 0u8;
        data.set_bits(0..6, self.quantization_parameter);
        data.set_bit(7, self.progressive);
        dst.write_u8(data);
        dst.write_u8(self.quality);
        Ok(())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

impl<'de> Decode<'de> for QuantQuality {
    fn decode(src: &mut ReadCursor<'de>) -> DecodeResult<Self> {
        ensure_fixed_part_size!(in: src);

        let data = src.read_u8();
        let qp = data.get_bits(0..6);
        let progressive = data.get_bit(7);
        let quality = src.read_u8();
        Ok(QuantQuality {
            quantization_parameter: qp,
            progressive,
            quality,
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Avc420BitmapStream<'a> {
    pub rectangles: Vec<InclusiveRectangle>,
    pub quant_qual_vals: Vec<QuantQuality>,
    pub data: &'a [u8],
}

impl fmt::Debug for Avc420BitmapStream<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Avc420BitmapStream")
            .field("rectangles", &self.rectangles)
            .field("quant_qual_vals", &self.quant_qual_vals)
            .field("data_len", &self.data.len())
            .finish()
    }
}

impl Avc420BitmapStream<'_> {
    const NAME: &'static str = "Avc420BitmapStream";

    const FIXED_PART_SIZE: usize = 4 /* nRect */;
}

impl Encode for Avc420BitmapStream<'_> {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_size!(in: dst, size: self.size());

        // INVARIANT: rectangles.len() == quant_qual_vals.len()
        debug_assert_eq!(self.rectangles.len(), self.quant_qual_vals.len());

        dst.write_u32(cast_length!("len", self.rectangles.len())?);
        for rectangle in &self.rectangles {
            rectangle.encode(dst)?;
        }
        for quant_qual_val in &self.quant_qual_vals {
            quant_qual_val.encode(dst)?;
        }
        dst.write_slice(self.data);

        Ok(())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn size(&self) -> usize {
        // Each rectangle is 8 bytes and 2 bytes for each quant val
        Self::FIXED_PART_SIZE + self.rectangles.len() * 10 + self.data.len()
    }
}

impl<'de> Decode<'de> for Avc420BitmapStream<'de> {
    fn decode(src: &mut ReadCursor<'de>) -> DecodeResult<Self> {
        ensure_fixed_part_size!(in: src);

        let num_regions = src.read_u32();
        #[expect(clippy::as_conversions, reason = "num_regions bounded by practical limits")]
        let num_regions_usize = num_regions as usize;
        let mut rectangles = Vec::with_capacity(num_regions_usize);
        let mut quant_qual_vals = Vec::with_capacity(num_regions_usize);
        for _ in 0..num_regions {
            rectangles.push(InclusiveRectangle::decode(src)?);
        }
        for _ in 0..num_regions {
            quant_qual_vals.push(QuantQuality::decode(src)?);
        }
        let data = src.remaining();
        Ok(Avc420BitmapStream {
            rectangles,
            quant_qual_vals,
            data,
        })
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Encoding: u8 {
        const LUMA_AND_CHROMA = 0x00;
        const LUMA = 0x01;
        const CHROMA = 0x02;

        const _ = !0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avc444BitmapStream<'a> {
    pub encoding: Encoding,
    pub stream1: Avc420BitmapStream<'a>,
    pub stream2: Option<Avc420BitmapStream<'a>>,
}

impl Avc444BitmapStream<'_> {
    const NAME: &'static str = "Avc444BitmapStream";

    const FIXED_PART_SIZE: usize = 4 /* streamInfo */;
}

impl Encode for Avc444BitmapStream<'_> {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);

        let mut stream_info = 0u32;
        stream_info.set_bits(0..30, cast_length!("stream1size", self.stream1.size())?);
        stream_info.set_bits(30..32, self.encoding.bits().into());
        dst.write_u32(stream_info);
        self.stream1.encode(dst)?;
        if let Some(stream) = self.stream2.as_ref() {
            stream.encode(dst)?;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn size(&self) -> usize {
        let stream2_size = if let Some(stream) = self.stream2.as_ref() {
            stream.size()
        } else {
            0
        };

        Self::FIXED_PART_SIZE + self.stream1.size() + stream2_size
    }
}

impl<'de> Decode<'de> for Avc444BitmapStream<'de> {
    fn decode(src: &mut ReadCursor<'de>) -> DecodeResult<Self> {
        ensure_fixed_part_size!(in: src);

        let stream_info = src.read_u32();
        let stream_len = stream_info.get_bits(0..30);
        #[expect(clippy::unwrap_used, reason = "2-bit extraction always fits in u8")]
        let encoding_raw: u8 = stream_info.get_bits(30..32).try_into().unwrap();
        // Only 0x00 (LUMA_AND_CHROMA), 0x01 (LUMA), 0x02 (CHROMA) are defined.
        if encoding_raw > 2 {
            return Err(invalid_field_err!("encoding", "reserved encoding value"));
        }
        let encoding = Encoding::from_bits_retain(encoding_raw);

        if stream_len == 0 {
            if encoding == Encoding::LUMA_AND_CHROMA {
                return Err(invalid_field_err!("encoding", "invalid encoding"));
            }

            let stream1 = Avc420BitmapStream::decode(src)?;
            Ok(Avc444BitmapStream {
                encoding,
                stream1,
                stream2: None,
            })
        } else {
            #[expect(clippy::as_conversions, reason = "30-bit value fits in usize")]
            let (mut stream1, mut stream2) = src.split_at(stream_len as usize);
            let stream1 = Avc420BitmapStream::decode(&mut stream1)?;
            let stream2 = if encoding == Encoding::LUMA_AND_CHROMA {
                Some(Avc420BitmapStream::decode(&mut stream2)?)
            } else {
                None
            };
            Ok(Avc444BitmapStream {
                encoding,
                stream1,
                stream2,
            })
        }
    }
}

// ============================================================================
// Server-side utilities for H.264/AVC encoding
// ============================================================================

/// Region metadata for AVC420 bitmap streams (server-side)
///
/// Describes a rectangular region within the frame along with its
/// H.264 encoding parameters.
///
/// # Example
///
/// ```
/// use ironrdp_egfx::pdu::Avc420Region;
///
/// // Create a region covering a 1920x1080 frame
/// let region = Avc420Region::full_frame(1920, 1080, 22);
/// assert_eq!(region.left, 0);
/// assert_eq!(region.right, 1919);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avc420Region {
    /// Left edge of the region (inclusive)
    pub left: u16,
    /// Top edge of the region (inclusive)
    pub top: u16,
    /// Right edge of the region (inclusive)
    pub right: u16,
    /// Bottom edge of the region (inclusive)
    pub bottom: u16,
    /// H.264 quantization parameter (0-51, lower = higher quality)
    pub quantization_parameter: u8,
    /// Quality value (0-100)
    pub quality: u8,
}

impl Avc420Region {
    /// Create a region covering the entire frame
    ///
    /// # Arguments
    ///
    /// * `width` - Frame width in pixels
    /// * `height` - Frame height in pixels
    /// * `qp` - H.264 quantization parameter (0-51)
    #[must_use]
    pub fn full_frame(width: u16, height: u16, qp: u8) -> Self {
        Self {
            left: 0,
            top: 0,
            right: width.saturating_sub(1),
            bottom: height.saturating_sub(1),
            quantization_parameter: qp,
            quality: 100,
        }
    }

    /// Create a region with custom bounds
    #[must_use]
    pub fn new(left: u16, top: u16, right: u16, bottom: u16, qp: u8, quality: u8) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
            quantization_parameter: qp,
            quality,
        }
    }

    /// Convert to `InclusiveRectangle` for PDU encoding
    #[must_use]
    pub fn to_rectangle(&self) -> InclusiveRectangle {
        InclusiveRectangle {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }

    /// Convert to `QuantQuality` for PDU encoding
    #[must_use]
    pub fn to_quant_quality(&self) -> QuantQuality {
        QuantQuality {
            quantization_parameter: self.quantization_parameter,
            progressive: false,
            quality: self.quality,
        }
    }
}

/// Convert H.264 Annex B format to AVC format
///
/// MS-RDPEGFX requires AVC format (length-prefixed NAL units),
/// but most encoders output Annex B format (start code prefixed).
///
/// ```text
/// Annex B: 00 00 00 01 <NAL> 00 00 00 01 <NAL> ...
/// AVC:     <4-byte BE length> <NAL> <4-byte BE length> <NAL> ...
/// ```
///
/// # Arguments
///
/// * `data` - H.264 bitstream in Annex B format
///
/// # Returns
///
/// H.264 bitstream in AVC format with 4-byte big-endian length prefixes
///
/// # Example
///
/// ```
/// use ironrdp_egfx::pdu::annex_b_to_avc;
///
/// // NAL unit with 3-byte start code
/// let annex_b = [0x00, 0x00, 0x01, 0x67, 0x42, 0x00];
/// let avc = annex_b_to_avc(&annex_b);
/// // Result: [0x00, 0x00, 0x00, 0x03, 0x67, 0x42, 0x00]
/// assert_eq!(avc[0..4], [0, 0, 0, 3]); // 4-byte length = 3
/// ```
#[must_use]
pub fn annex_b_to_avc(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        // Find start code (00 00 01 or 00 00 00 01)
        let start;

        if i + 4 <= data.len() && data[i..i + 4] == [0, 0, 0, 1] {
            start = i + 4;
        } else if i + 3 <= data.len() && data[i..i + 3] == [0, 0, 1] {
            start = i + 3;
        } else {
            i += 1;
            continue;
        }

        // Find next start code or end of data
        let mut end = data.len();
        for j in start..data.len().saturating_sub(2) {
            if data[j..j + 3] == [0, 0, 1] {
                // Could be 3-byte or 4-byte start code
                // Check if there's a leading zero (4-byte)
                if j > 0 && data[j - 1] == 0 {
                    end = j - 1;
                } else {
                    end = j;
                }
                break;
            }
        }

        // Write length-prefixed NAL unit
        let nal_data = &data[start..end];
        if !nal_data.is_empty() {
            // NAL units in H.264 are limited to ~4GB (32-bit length), so truncation is not a concern
            #[expect(
                clippy::cast_possible_truncation,
                clippy::as_conversions,
                reason = "NAL unit length fits in u32"
            )]
            let len = nal_data.len() as u32;
            result.extend_from_slice(&len.to_be_bytes());
            result.extend_from_slice(nal_data);
        }

        // Move to end of current NAL unit; next iteration will find the next start code
        i = end;
    }

    result
}

/// Align a dimension to 16-pixel boundary
///
/// H.264 operates on 16x16 macroblocks. This function rounds up
/// a dimension to the nearest multiple of 16.
///
/// # Example
///
/// ```
/// use ironrdp_egfx::pdu::align_to_16;
///
/// assert_eq!(align_to_16(1920), 1920); // Already aligned
/// assert_eq!(align_to_16(1080), 1088); // Rounded up
/// assert_eq!(align_to_16(1), 16);
/// ```
#[must_use]
pub const fn align_to_16(dimension: u32) -> u32 {
    (dimension + 15) & !15
}

/// Create an owned AVC420 bitmap stream from regions and H.264 data
///
/// This is a helper for server-side frame encoding. It creates
/// the bitmap stream structure that can be embedded in a
/// `WireToSurface1Pdu`.
///
/// # Arguments
///
/// * `regions` - List of regions with their encoding parameters
/// * `h264_data` - H.264 encoded data (should be in AVC format, not Annex B)
///
/// # Returns
///
/// Encoded `Avc420BitmapStream` as a byte vector
///
/// # Panics
///
/// Panics if internal encoding fails (should not happen with valid inputs).
#[must_use]
pub fn encode_avc420_bitmap_stream(regions: &[Avc420Region], h264_data: &[u8]) -> Vec<u8> {
    let rectangles: Vec<InclusiveRectangle> = regions.iter().map(Avc420Region::to_rectangle).collect();

    let quant_qual_vals: Vec<QuantQuality> = regions.iter().map(Avc420Region::to_quant_quality).collect();

    let stream = Avc420BitmapStream {
        rectangles,
        quant_qual_vals,
        data: h264_data,
    };

    // Calculate size and encode
    let size = stream.size();
    let mut buf = vec![0u8; size];
    let mut cursor = WriteCursor::new(&mut buf);

    // This should not fail as we pre-allocated the exact size
    stream
        .encode(&mut cursor)
        .expect("encode_avc420_bitmap_stream: encoding failed");

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_avc420_region_full_frame() {
        let region = Avc420Region::full_frame(1920, 1080, 22);
        assert_eq!(region.left, 0);
        assert_eq!(region.top, 0);
        assert_eq!(region.right, 1919);
        assert_eq!(region.bottom, 1079);
        assert_eq!(region.quantization_parameter, 22);
        assert_eq!(region.quality, 100);
    }

    #[test]
    fn test_align_to_16() {
        assert_eq!(align_to_16(0), 0);
        assert_eq!(align_to_16(1), 16);
        assert_eq!(align_to_16(15), 16);
        assert_eq!(align_to_16(16), 16);
        assert_eq!(align_to_16(17), 32);
        assert_eq!(align_to_16(1920), 1920);
        assert_eq!(align_to_16(1080), 1088);
    }

    #[test]
    fn test_annex_b_to_avc_3byte_start() {
        // NAL with 3-byte start code: 00 00 01 <NAL>
        let annex_b = [0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1E];
        let avc = annex_b_to_avc(&annex_b);

        // Should be: 4-byte length (4) + NAL data
        assert_eq!(avc.len(), 8);
        assert_eq!(&avc[0..4], &[0, 0, 0, 4]); // Length = 4
        assert_eq!(&avc[4..8], &[0x67, 0x42, 0x00, 0x1E]);
    }

    #[test]
    fn test_annex_b_to_avc_4byte_start() {
        // NAL with 4-byte start code: 00 00 00 01 <NAL>
        let annex_b = [0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00];
        let avc = annex_b_to_avc(&annex_b);

        assert_eq!(avc.len(), 7);
        assert_eq!(&avc[0..4], &[0, 0, 0, 3]); // Length = 3
        assert_eq!(&avc[4..7], &[0x67, 0x42, 0x00]);
    }

    #[test]
    fn test_annex_b_to_avc_multiple_nals() {
        // Two NAL units
        let annex_b = [
            0x00, 0x00, 0x00, 0x01, 0x67, 0x42, // SPS
            0x00, 0x00, 0x01, 0x68, 0xCE, // PPS with 3-byte start
        ];
        let avc = annex_b_to_avc(&annex_b);

        // First NAL: 4 bytes length + 2 bytes data
        // Second NAL: 4 bytes length + 2 bytes data
        assert!(avc.len() >= 12);
    }

    #[test]
    fn test_annex_b_to_avc_empty() {
        let avc = annex_b_to_avc(&[]);
        assert!(avc.is_empty());
    }

    #[test]
    fn test_encode_avc420_bitmap_stream() {
        let regions = vec![Avc420Region::full_frame(1920, 1080, 22)];
        let h264_data = [0x00, 0x00, 0x00, 0x01, 0x67]; // Minimal H.264

        let encoded = encode_avc420_bitmap_stream(&regions, &h264_data);

        // Should have: 4 bytes (nRect=1) + 8 bytes (rectangle) + 2 bytes (quant) + 5 bytes (data)
        assert_eq!(encoded.len(), 4 + 8 + 2 + 5);

        // Verify we can decode it back
        let mut cursor = ReadCursor::new(&encoded);
        let decoded = Avc420BitmapStream::decode(&mut cursor).expect("decode failed");

        assert_eq!(decoded.rectangles.len(), 1);
        assert_eq!(decoded.quant_qual_vals.len(), 1);
        assert_eq!(decoded.data, &h264_data);
    }

    // ========================================================================
    // AVC444 dual-stream (Avc444BitmapStream) parse tests
    // ========================================================================

    fn sample_avc420(data: &[u8]) -> Avc420BitmapStream<'_> {
        Avc420BitmapStream {
            rectangles: vec![InclusiveRectangle {
                left: 0,
                top: 0,
                right: 15,
                bottom: 15,
            }],
            quant_qual_vals: vec![QuantQuality {
                quantization_parameter: 22,
                progressive: false,
                quality: 100,
            }],
            data,
        }
    }

    fn encode_avc444(stream: &Avc444BitmapStream<'_>) -> Vec<u8> {
        let mut buf = vec![0u8; stream.size()];
        let mut cursor = WriteCursor::new(&mut buf);
        stream.encode(&mut cursor).expect("avc444 encode");
        buf
    }

    #[test]
    fn avc444_round_trip_luma_and_chroma() {
        // LC == LUMA_AND_CHROMA: both sub-streams present; the 30-bit streamInfo
        // length selects where stream1 ends and stream2 (chroma) begins.
        let stream = Avc444BitmapStream {
            encoding: Encoding::LUMA_AND_CHROMA,
            stream1: sample_avc420(b"luma-nalu"),
            stream2: Some(sample_avc420(b"chroma-nalu")),
        };
        let buf = encode_avc444(&stream);

        let mut cursor = ReadCursor::new(&buf);
        let decoded = Avc444BitmapStream::decode(&mut cursor).expect("avc444 decode");
        assert_eq!(decoded.encoding, Encoding::LUMA_AND_CHROMA);
        assert_eq!(decoded.stream1.data, b"luma-nalu".as_slice());
        let stream2 = decoded.stream2.as_ref().expect("stream2 present for LUMA_AND_CHROMA");
        assert_eq!(stream2.data, b"chroma-nalu".as_slice());
    }

    #[test]
    fn avc444_round_trip_luma_only() {
        // LC == LUMA: only stream1; decoder must yield stream2 == None.
        let stream = Avc444BitmapStream {
            encoding: Encoding::LUMA,
            stream1: sample_avc420(b"luma-only"),
            stream2: None,
        };
        let buf = encode_avc444(&stream);

        let mut cursor = ReadCursor::new(&buf);
        let decoded = Avc444BitmapStream::decode(&mut cursor).expect("avc444 decode");
        assert_eq!(decoded.encoding, Encoding::LUMA);
        assert_eq!(decoded.stream1.data, b"luma-only".as_slice());
        assert!(decoded.stream2.is_none(), "LUMA carries no chroma sub-stream");
    }

    #[test]
    fn avc444_round_trip_chroma_only() {
        // LC == CHROMA: only stream1 (the chroma view); stream2 == None.
        let stream = Avc444BitmapStream {
            encoding: Encoding::CHROMA,
            stream1: sample_avc420(b"chroma-only"),
            stream2: None,
        };
        let buf = encode_avc444(&stream);

        let mut cursor = ReadCursor::new(&buf);
        let decoded = Avc444BitmapStream::decode(&mut cursor).expect("avc444 decode");
        assert_eq!(decoded.encoding, Encoding::CHROMA);
        assert_eq!(decoded.stream1.data, b"chroma-only".as_slice());
        assert!(decoded.stream2.is_none(), "CHROMA carries no second sub-stream");
    }

    #[test]
    fn avc444_decode_rejects_reserved_encoding() {
        // streamInfo: bits 30..32 = encoding. 0b11 (== 3) is reserved -> error.
        // Little-endian u32 0xC000_0000 => length 0, encoding 3.
        let bytes = [0x00, 0x00, 0x00, 0xC0];
        let mut cursor = ReadCursor::new(&bytes);
        assert!(
            Avc444BitmapStream::decode(&mut cursor).is_err(),
            "reserved encoding value must be rejected"
        );
    }

    #[test]
    fn avc444_decode_rejects_zero_len_luma_and_chroma() {
        // streamInfo == 0 => length 0 with encoding LUMA_AND_CHROMA, which is
        // contradictory (both views require a non-zero split length) -> error.
        let bytes = [0x00, 0x00, 0x00, 0x00];
        let mut cursor = ReadCursor::new(&bytes);
        assert!(
            Avc444BitmapStream::decode(&mut cursor).is_err(),
            "zero-length LUMA_AND_CHROMA must be rejected"
        );
    }
}
