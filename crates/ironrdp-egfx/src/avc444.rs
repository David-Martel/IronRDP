//! AVC444 dual-stream YUV 4:4:4 reconstruction ([MS-RDPEGFX] 2.2.4.4/2.2.4.5).
//!
//! AVC444 transmits full-chroma (4:4:4) video as **two** H.264 4:2:0 streams:
//!
//! - the **luma** stream (`Encoding::LUMA` view): its Y plane is the real
//!   full-resolution luma; its subsampled U/V planes are the "main" chroma view;
//! - the **chroma-auxiliary** stream (`Encoding::CHROMA` view): a second 4:2:0
//!   frame whose planes pack the chroma samples that 4:2:0 subsampling of the
//!   main view dropped, so that main+aux together reconstruct full 4:4:4 chroma.
//!
//! Two packings exist: **v1** (codec `Avc444`) and **v2** (codec `Avc444v2`).
//!
//! The reconstruction here mirrors FreeRDP's `general_LumaToYUV444`,
//! `general_ChromaV1ToYUV444`, and `general_ChromaV2ToYUV444` primitives (the
//! authoritative, Windows-interop reference — [MS-RDPEGFX] specifies the format
//! but not the exact index math). All plane reads/writes are bounds-checked so
//! malformed or truncated input can never panic.
//!
//! **Scope**: only the two-stream case (`Encoding::LUMA_AND_CHROMA`, both
//! streams present in one PDU) is reconstructed here. Luma-only and chroma-only
//! partial updates require a persistent per-surface YUV 4:4:4 framebuffer to
//! composite against; that is intentionally out of scope (the caller warns and
//! skips them). End-to-end output is unverified: no AVC444-advertising peer was
//! available (GRD advertises AVC420 only), so this is validated against the spec
//! geometry via unit tests, not against a live encoder.
//!
//! [MS-RDPEGFX]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpegfx/da5c75f9-cd99-450c-98c4-014a496942b0

use crate::decode::Yuv420Frame;

// BT.601 limited-range YUV->RGB coefficients, identical to the ones OpenH264's
// `write_rgba8` uses for the AVC420 path, so AVC444 output color matches AVC420.
// https://en.wikipedia.org/wiki/YCbCr#ITU-R_BT.601_conversion
const Y_MUL: f32 = 255.0 / 219.0;
const RV_MUL: f32 = 255.0 / 224.0 * 1.402;
const GV_MUL: f32 = -255.0 / 224.0 * 1.402 * 0.299 / 0.687;
const GU_MUL: f32 = -255.0 / 224.0 * 1.772 * 0.114 / 0.587;
const BU_MUL: f32 = 255.0 / 224.0 * 1.772;

/// A full-resolution planar YUV 4:4:4 frame (every plane is `width * height`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Yuv444Frame {
    /// Luma plane (`width * height` bytes).
    pub y: Vec<u8>,
    /// Blue-difference chroma plane (`width * height` bytes).
    pub u: Vec<u8>,
    /// Red-difference chroma plane (`width * height` bytes).
    pub v: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

impl Yuv444Frame {
    /// Allocate a `width * height` YUV 4:4:4 frame with neutral content
    /// (luma 0, chroma 128 == achromatic).
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let len = as_usize(width).saturating_mul(as_usize(height));
        Self {
            y: vec![0u8; len],
            u: vec![128u8; len],
            v: vec![128u8; len],
            width,
            height,
        }
    }
}

/// Which AVC444 chroma packing the auxiliary stream uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromaVersion {
    /// AVC444 v1 (codec id `0x0E`) — `general_ChromaV1ToYUV444`.
    V1,
    /// AVC444 v2 (codec id `0x0F`) — `general_ChromaV2ToYUV444`.
    V2,
}

#[inline]
#[expect(
    clippy::as_conversions,
    reason = "u32 pixel dimension to usize index; lossless on all supported (>=32-bit) targets"
)]
fn as_usize(v: u32) -> usize {
    v as usize
}

/// Apply the luma (main) view of an AVC444 stream onto `dst`.
///
/// Mirrors FreeRDP `general_LumaToYUV444`: copies the full-resolution luma, then
/// nearest-neighbour-upsamples the main subsampled chroma into all four 4:4:4
/// positions of each 2x2 block. The chroma auxiliary pass overwrites the odd
/// positions afterwards with their true values.
#[expect(
    clippy::similar_names,
    reason = "target width/height (tw/th) and U/V chroma pairs intentionally differ by one axis character"
)]
pub fn luma_to_yuv444(dst: &mut Yuv444Frame, main: &Yuv420Frame) {
    let tw = as_usize(dst.width);
    let th = as_usize(dst.height);
    let main_stride = as_usize(main.width);
    let main_cstride = as_usize(Yuv420Frame::chroma_width(main.width));

    // B1: luma copied straight through.
    for y in 0..th {
        for x in 0..tw {
            if let Some(&yv) = main.y.get(y * main_stride + x) {
                dst.y[y * tw + x] = yv;
            }
        }
    }

    // B2/B3: main chroma sample (cx, cy) replicated to the whole 2x2 block.
    let half_w = tw.div_ceil(2);
    let half_h = th.div_ceil(2);
    for cy in 0..half_h {
        for cx in 0..half_w {
            let Some(&um) = main.u.get(cy * main_cstride + cx) else {
                continue;
            };
            let Some(&vm) = main.v.get(cy * main_cstride + cx) else {
                continue;
            };
            for dy in (2 * cy)..(2 * cy + 2).min(th) {
                for dx in (2 * cx)..(2 * cx + 2).min(tw) {
                    dst.u[dy * tw + dx] = um;
                    dst.v[dy * tw + dx] = vm;
                }
            }
        }
    }
}

/// Apply the v1 chroma-auxiliary view onto `dst` (FreeRDP `general_ChromaV1ToYUV444`).
///
/// - B4/B5: the aux **Y** plane packs full-width chroma for the odd rows. Rows
///   are grouped in 16-row macroblock bands: the first 8 rows of each band carry
///   U for the next odd destination row, the second 8 rows carry V.
/// - B6/B7: the aux **U/V** planes carry chroma for the odd columns of the even
///   destination rows.
#[expect(
    clippy::similar_names,
    reason = "target width/height (tw/th) and U/V chroma pairs intentionally differ by one axis character"
)]
pub fn chroma_v1_to_yuv444(dst: &mut Yuv444Frame, aux: &Yuv420Frame) {
    let tw = as_usize(dst.width);
    let th = as_usize(dst.height);
    let aux_stride = as_usize(aux.width);
    let aux_cstride = as_usize(Yuv420Frame::chroma_width(aux.width));

    // B4/B5: odd rows come from the aux luma plane, de-interleaved by 16-row band.
    // The aux frame is 16x16-macroblock aligned; iterate the padded height exactly
    // as FreeRDP (`nHeight + 16 - nHeight % 16`). Extra rows beyond the image are
    // guard-skipped below (and bounds-checked against the aux plane).
    let pad_h = th + 16 - th % 16;
    let mut u_row_counter = 0usize;
    let mut v_row_counter = 0usize;
    for y in 0..pad_h {
        let to_u = (y % 16) < 8;
        let pos = if to_u {
            let p = 2 * u_row_counter + 1;
            u_row_counter += 1;
            p
        } else {
            let p = 2 * v_row_counter + 1;
            v_row_counter += 1;
            p
        };
        if pos >= th {
            continue;
        }
        for x in 0..tw {
            let Some(&src) = aux.y.get(y * aux_stride + x) else {
                continue;
            };
            let dst_idx = pos * tw + x;
            if to_u {
                dst.u[dst_idx] = src;
            } else {
                dst.v[dst_idx] = src;
            }
        }
    }

    // B6/B7: odd columns of even rows come from the aux chroma planes.
    let half_w = tw / 2;
    let half_h = th / 2;
    for cy in 0..half_h {
        let row = 2 * cy;
        for cx in 0..half_w {
            let col = 2 * cx + 1;
            if col >= tw {
                continue;
            }
            if let Some(&ua) = aux.u.get(cy * aux_cstride + cx) {
                dst.u[row * tw + col] = ua;
            }
            if let Some(&va) = aux.v.get(cy * aux_cstride + cx) {
                dst.v[row * tw + col] = va;
            }
        }
    }
}

/// Apply the v2 chroma-auxiliary view onto `dst` (FreeRDP `general_ChromaV2ToYUV444`).
///
/// v2 packs differently from v1:
/// - B4/B5: the aux **Y** plane's left half holds U for the odd columns of every
///   row, the right half holds V for the odd columns of every row.
/// - B6-B9: the aux **U/V** planes hold, at quarter-width interleave, the chroma
///   for the odd destination rows.
///
/// Note: v2 has no split reference in FreeRDP's tree and no AVC444v2 peer was
/// available, so this variant is implemented to spec geometry but not
/// round-trip validated.
#[expect(
    clippy::similar_names,
    reason = "target width/height (tw/th) and U/V chroma pairs intentionally differ by one axis character"
)]
pub fn chroma_v2_to_yuv444(dst: &mut Yuv444Frame, aux: &Yuv420Frame) {
    let tw = as_usize(dst.width);
    let th = as_usize(dst.height);
    let aux_stride = as_usize(aux.width);
    let aux_cstride = as_usize(Yuv420Frame::chroma_width(aux.width));
    let total_width = as_usize(aux.width);

    // B4/B5: odd UV columns for every row, from the aux luma plane.
    let half_w = tw.div_ceil(2);
    let v_half_off = total_width / 2;
    for y in 0..th {
        let row_base = y * aux_stride;
        for x in 0..half_w {
            let col = 2 * x + 1;
            if col >= tw {
                continue;
            }
            if let Some(&u) = aux.y.get(row_base + x) {
                dst.u[y * tw + col] = u;
            }
            if let Some(&v) = aux.y.get(row_base + v_half_off + x) {
                dst.v[y * tw + col] = v;
            }
        }
    }

    // B6-B9: odd rows, from the aux chroma planes at quarter-width interleave.
    let half_h = th.div_ceil(2);
    let quarter_w = tw.div_ceil(4);
    let quarter_off = total_width / 4;
    for cy in 0..half_h {
        let row = 2 * cy + 1;
        if row >= th {
            continue;
        }
        let crow = cy * aux_cstride;
        for x in 0..quarter_w {
            let c0 = 4 * x;
            let c2 = 4 * x + 2;
            if c0 < tw {
                if let Some(&uau) = aux.u.get(crow + x) {
                    dst.u[row * tw + c0] = uau;
                }
                if let Some(&uav) = aux.v.get(crow + x) {
                    dst.v[row * tw + c0] = uav;
                }
            }
            if c2 < tw {
                if let Some(&vau) = aux.u.get(crow + quarter_off + x) {
                    dst.u[row * tw + c2] = vau;
                }
                if let Some(&vav) = aux.v.get(crow + quarter_off + x) {
                    dst.v[row * tw + c2] = vav;
                }
            }
        }
    }
}

/// Reconstruct a full YUV 4:4:4 frame of `target_width x target_height` from an
/// AVC444 luma stream and its chroma-auxiliary stream.
///
/// `target_*` are the logical (destination-rectangle) dimensions; the decoded
/// `main`/`aux` frames are macroblock-aligned and may be larger, which the
/// per-plane index math accounts for via each frame's own stride.
#[must_use]
pub fn reconstruct_yuv444(
    main: &Yuv420Frame,
    aux: &Yuv420Frame,
    version: ChromaVersion,
    target_width: u32,
    target_height: u32,
) -> Yuv444Frame {
    let mut dst = Yuv444Frame::new(target_width, target_height);
    luma_to_yuv444(&mut dst, main);
    match version {
        ChromaVersion::V1 => chroma_v1_to_yuv444(&mut dst, aux),
        ChromaVersion::V2 => chroma_v2_to_yuv444(&mut dst, aux),
    }
    dst
}

/// Convert a YUV 4:4:4 frame to RGBA8888 (4 bytes/pixel, opaque alpha), using the
/// same BT.601 limited-range math as the AVC420 path for consistent color.
#[must_use]
pub fn yuv444_to_rgba(frame: &Yuv444Frame) -> Vec<u8> {
    let w = as_usize(frame.width);
    let h = as_usize(frame.height);
    let mut rgba = vec![0u8; w.saturating_mul(h).saturating_mul(4)];

    for i in 0..w.saturating_mul(h) {
        let (Some(&yv), Some(&uv), Some(&vv)) = (frame.y.get(i), frame.u.get(i), frame.v.get(i)) else {
            break;
        };
        let y_mul = Y_MUL * (f32::from(yv) - 16.0);
        let u = f32::from(uv) - 128.0;
        let v = f32::from(vv) - 128.0;

        let r = RV_MUL.mul_add(v, y_mul);
        let g = GV_MUL.mul_add(v, GU_MUL.mul_add(u, y_mul));
        let b = BU_MUL.mul_add(u, y_mul);

        let base = i * 4;
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to [0, 255] before the cast"
        )]
        {
            rgba[base] = r.clamp(0.0, 255.0) as u8;
            rgba[base + 1] = g.clamp(0.0, 255.0) as u8;
            rgba[base + 2] = b.clamp(0.0, 255.0) as u8;
        }
        rgba[base + 3] = 255;
    }

    rgba
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "test code with small, statically-known dimensions"
    )]

    use super::*;

    fn frame420(width: u32, height: u32, y_fill: u8, u_fill: u8, v_fill: u8) -> Yuv420Frame {
        let cw = Yuv420Frame::chroma_width(width) as usize;
        let ch = Yuv420Frame::chroma_height(height) as usize;
        Yuv420Frame {
            y: vec![y_fill; (width * height) as usize],
            u: vec![u_fill; cw * ch],
            v: vec![v_fill; cw * ch],
            width,
            height,
        }
    }

    #[test]
    fn luma_copies_full_resolution_y() {
        // Distinct Y per pixel so we catch any stride/index error.
        let (w, h) = (4u32, 4u32);
        let mut main = frame420(w, h, 0, 128, 128);
        for (i, px) in main.y.iter_mut().enumerate() {
            *px = i as u8;
        }
        let mut dst = Yuv444Frame::new(w, h);
        luma_to_yuv444(&mut dst, &main);
        for i in 0..(w * h) as usize {
            assert_eq!(dst.y[i], i as u8, "luma pixel {i} mismatch");
        }
    }

    #[test]
    fn luma_replicates_main_chroma_across_2x2_block() {
        // 2x2 image => one chroma sample; it must fill all four 4:4:4 positions.
        let main = frame420(2, 2, 0, 200, 50);
        let mut dst = Yuv444Frame::new(2, 2);
        luma_to_yuv444(&mut dst, &main);
        assert_eq!(dst.u, vec![200, 200, 200, 200]);
        assert_eq!(dst.v, vec![50, 50, 50, 50]);
    }

    #[test]
    fn luma_chroma_uses_distinct_samples_per_block() {
        // 4x4 => 2x2 chroma grid. Give each chroma sample a unique value and
        // assert each 2x2 destination block is filled with it.
        let (w, h) = (4u32, 4u32);
        let mut main = frame420(w, h, 0, 0, 0);
        // chroma grid is 2x2, laid out row-major
        main.u = vec![10, 20, 30, 40];
        main.v = vec![11, 21, 31, 41];
        let mut dst = Yuv444Frame::new(w, h);
        luma_to_yuv444(&mut dst, &main);
        let at = |plane: &[u8], x: usize, y: usize| plane[y * w as usize + x];
        // top-left block (cols 0-1, rows 0-1) == chroma sample (0,0)=10/11
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(at(&dst.u, x, y), 10);
            assert_eq!(at(&dst.v, x, y), 11);
        }
        // top-right block (cols 2-3, rows 0-1) == sample (1,0)=20/21
        for (x, y) in [(2, 0), (3, 0), (2, 1), (3, 1)] {
            assert_eq!(at(&dst.u, x, y), 20);
            assert_eq!(at(&dst.v, x, y), 21);
        }
        // bottom-left == sample (0,1)=30/31 ; bottom-right == (1,1)=40/41
        assert_eq!(at(&dst.u, 0, 3), 30);
        assert_eq!(at(&dst.u, 3, 3), 40);
    }

    #[test]
    fn chroma_v1_fills_odd_columns_of_even_rows() {
        // 4x4 target. Aux chroma planes (2x2 each) feed odd cols of even rows.
        let (w, h) = (4u32, 4u32);
        let mut dst = Yuv444Frame::new(w, h);
        // Start from a luma pass so even/even positions are defined; then aux.
        let main = frame420(w, h, 0, 128, 128);
        luma_to_yuv444(&mut dst, &main);

        // Aux frame: needs a Y plane tall enough for the padded odd-row pass
        // (pad height for h=4 is 16) and 2x2 chroma planes for B6/B7.
        let mut aux = frame420(w, 16, 0, 0, 0);
        // B6/B7 source: aux.u/aux.v chroma grids (2x2). Unique values.
        aux.u = {
            let mut v = vec![0u8; (Yuv420Frame::chroma_width(w) * Yuv420Frame::chroma_height(16)) as usize];
            v[0] = 60; // cy=0,cx=0 -> row 0, col 1
            v[1] = 61; // cy=0,cx=1 -> row 0, col 3
            v
        };
        aux.v = {
            let mut v = vec![0u8; (Yuv420Frame::chroma_width(w) * Yuv420Frame::chroma_height(16)) as usize];
            v[0] = 70;
            v[1] = 71;
            v
        };
        chroma_v1_to_yuv444(&mut dst, &aux);
        let at = |plane: &[u8], x: usize, y: usize| plane[y * w as usize + x];
        // Even row 0: odd cols 1 and 3 overwritten by aux chroma.
        assert_eq!(at(&dst.u, 1, 0), 60);
        assert_eq!(at(&dst.u, 3, 0), 61);
        assert_eq!(at(&dst.v, 1, 0), 70);
        assert_eq!(at(&dst.v, 3, 0), 71);
    }

    #[test]
    fn chroma_v1_fills_odd_rows_from_aux_luma() {
        // The aux luma plane feeds full-width chroma for odd destination rows.
        // For h=4, pad height = 16: rows 0..8 of aux Y => U band, 8..16 => V band.
        // The first U-band row (y=0) targets destination U row (2*0+1)=1.
        // The first V-band row (y=8) targets destination V row (2*0+1)=1.
        let (w, h) = (4u32, 4u32);
        let mut dst = Yuv444Frame::new(w, h);
        let mut aux = frame420(w, 16, 0, 0, 0);
        // Set aux Y row 0 (U band) to a marker across the full width.
        for x in 0..w as usize {
            aux.y[x] = 90;
        }
        // Set aux Y row 8 (first V-band row) to a different marker.
        for x in 0..w as usize {
            aux.y[8 * w as usize + x] = 99;
        }
        chroma_v1_to_yuv444(&mut dst, &aux);
        let at = |plane: &[u8], x: usize, y: usize| plane[y * w as usize + x];
        // Destination U row 1 filled from aux Y row 0.
        for x in 0..w as usize {
            assert_eq!(at(&dst.u, x, 1), 90, "U odd-row col {x}");
        }
        // Destination V row 1 filled from aux Y row 8.
        for x in 0..w as usize {
            assert_eq!(at(&dst.v, x, 1), 99, "V odd-row col {x}");
        }
    }

    #[test]
    fn reconstruct_full_frame_is_consistent() {
        // Solid-color sanity: a uniform luma + uniform aux must yield a uniform
        // 4:4:4 frame (every position defined, no gaps left at 128).
        let (w, h) = (8u32, 8u32);
        let main = frame420(w, h, 120, 130, 140);
        let aux = frame420(w, 16, 200, 210, 220);
        let out = reconstruct_yuv444(&main, &aux, ChromaVersion::V1, w, h);
        assert_eq!(out.y.len(), (w * h) as usize);
        assert_eq!(out.u.len(), (w * h) as usize);
        assert_eq!(out.v.len(), (w * h) as usize);
        // Luma is copied from main everywhere.
        assert!(out.y.iter().all(|&p| p == 120));
        // Every chroma position is written (either main even/even, aux odd-col,
        // or aux odd-row) -- none should remain at the neutral 128 default.
        assert!(out.u.iter().all(|&p| p != 128), "some U left unwritten");
    }

    #[test]
    fn yuv444_to_rgba_shapes_and_alpha() {
        let frame = Yuv444Frame::new(3, 2);
        let rgba = yuv444_to_rgba(&frame);
        assert_eq!(rgba.len(), 3 * 2 * 4);
        for px in rgba.chunks_exact(4) {
            assert_eq!(px[3], 255, "alpha must be opaque");
        }
    }

    #[test]
    fn yuv444_to_rgba_white_and_black() {
        // Y=235 (limited-range white), chroma neutral => ~white.
        let mut frame = Yuv444Frame::new(1, 1);
        frame.y[0] = 235;
        let rgba = yuv444_to_rgba(&frame);
        assert!(
            rgba[0] > 250 && rgba[1] > 250 && rgba[2] > 250,
            "expected white, got {rgba:?}"
        );
        // Y=16 (limited-range black), chroma neutral => ~black.
        frame.y[0] = 16;
        let rgba = yuv444_to_rgba(&frame);
        assert!(
            rgba[0] < 5 && rgba[1] < 5 && rgba[2] < 5,
            "expected black, got {rgba:?}"
        );
    }

    #[test]
    fn combine_is_panic_free_on_undersized_aux() {
        // Truncated/mismatched aux must not panic (bounds-checked reads).
        let (w, h) = (16u32, 16u32);
        let main = frame420(w, h, 50, 60, 70);
        let tiny_aux = frame420(2, 2, 0, 0, 0);
        let _ = reconstruct_yuv444(&main, &tiny_aux, ChromaVersion::V1, w, h);
        let _ = reconstruct_yuv444(&main, &tiny_aux, ChromaVersion::V2, w, h);
    }

    #[test]
    fn chroma_v2_fills_odd_columns_from_aux_luma_halves() {
        // v2: aux Y left half -> U odd cols; right half -> V odd cols (all rows).
        let (w, h) = (4u32, 2u32);
        let mut dst = Yuv444Frame::new(w, h);
        let main = frame420(w, h, 0, 128, 128);
        luma_to_yuv444(&mut dst, &main);
        // aux width == target width (4). left half cols {0,1} -> U; right {2,3} -> V.
        let mut aux = frame420(w, h, 0, 0, 0);
        // row 0: U-half sample x=0 -> dst U col 1 ; x=1 -> dst U col 3
        aux.y[0] = 80;
        aux.y[1] = 81;
        // right half (offset total_width/2 = 2): V samples
        aux.y[2] = 90;
        aux.y[3] = 91;
        chroma_v2_to_yuv444(&mut dst, &aux);
        let at = |plane: &[u8], x: usize, y: usize| plane[y * w as usize + x];
        assert_eq!(at(&dst.u, 1, 0), 80);
        assert_eq!(at(&dst.u, 3, 0), 81);
        assert_eq!(at(&dst.v, 1, 0), 90);
        assert_eq!(at(&dst.v, 3, 0), 91);
    }
}
