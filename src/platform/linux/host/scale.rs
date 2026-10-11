//! CPU conversion of a captured 32-bit frame into the tight BGRA layout the
//! software encoder consumes, with optional bilinear downscaling.

/// Convert `src` (BGRX or RGBX, `stride` bytes per row) into a tight BGRA
/// buffer of `dst_w`x`dst_h`. Fixed-point bilinear when the sizes differ.
#[allow(clippy::too_many_arguments)]
pub(crate) fn convert(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    stride: usize,
    rgb: bool,
    y_invert: bool,
    dst_w: u32,
    dst_h: u32,
    dst: &mut Vec<u8>,
) {
    let (sw, sh, dw, dh) = (src_w as usize, src_h as usize, dst_w as usize, dst_h as usize);
    dst.resize(dw * dh * 4, 0);
    let row = |y: usize| {
        let y = if y_invert { sh - 1 - y } else { y };
        &src[y * stride..y * stride + sw * 4]
    };
    let swap = |out: &mut [u8]| {
        if rgb {
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
    };
    if sw == dw && sh == dh {
        for y in 0..dh {
            let out = &mut dst[y * dw * 4..(y + 1) * dw * 4];
            out.copy_from_slice(row(y));
            swap(out);
        }
        return;
    }
    // 16.16 fixed point source coordinates of each destination pixel centre.
    let x_ratio = ((sw as u64) << 16) / dw as u64;
    let y_ratio = ((sh as u64) << 16) / dh as u64;
    let xs: Vec<(usize, u32)> = (0..dw)
        .map(|x| {
            let fx = ((x as u64 * 2 + 1) * x_ratio / 2).saturating_sub(1 << 15);
            let ix = ((fx >> 16) as usize).min(sw - 1);
            (ix, ((fx >> 8) & 0xff) as u32)
        })
        .collect();
    for y in 0..dh {
        let fy = ((y as u64 * 2 + 1) * y_ratio / 2).saturating_sub(1 << 15);
        let iy = ((fy >> 16) as usize).min(sh - 1);
        let wy = ((fy >> 8) & 0xff) as u32;
        let r0 = row(iy);
        let r1 = row((iy + 1).min(sh - 1));
        let out = &mut dst[y * dw * 4..(y + 1) * dw * 4];
        for (x, &(ix, wx)) in xs.iter().enumerate() {
            let ix1 = (ix + 1).min(sw - 1);
            let (a, b) = (&r0[ix * 4..ix * 4 + 4], &r0[ix1 * 4..ix1 * 4 + 4]);
            let (c, d) = (&r1[ix * 4..ix * 4 + 4], &r1[ix1 * 4..ix1 * 4 + 4]);
            for ch in 0..3 {
                let top = u32::from(a[ch]) * (256 - wx) + u32::from(b[ch]) * wx;
                let bottom = u32::from(c[ch]) * (256 - wx) + u32::from(d[ch]) * wx;
                out[x * 4 + ch] = ((top * (256 - wy) + bottom * wy + (1 << 15)) >> 16) as u8;
            }
            out[x * 4 + 3] = 255;
        }
        swap(out);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_and_swap() {
        let src = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut out = Vec::new();
        super::convert(&src, 2, 1, 8, true, false, 2, 1, &mut out);
        assert_eq!(out, [3, 2, 1, 4, 7, 6, 5, 8]);
    }
    #[test]
    fn downscale_uniform() {
        let src = vec![100u8; 8 * 8 * 4];
        let mut out = Vec::new();
        super::convert(&src, 8, 8, 32, false, false, 4, 4, &mut out);
        assert_eq!(out.len(), 64);
        assert!(out.chunks_exact(4).all(|p| p[0] == 100 && p[1] == 100 && p[2] == 100));
    }
}
