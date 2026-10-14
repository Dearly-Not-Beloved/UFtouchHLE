/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The Scale2x (a.k.a. EPX) pixel art scaling algorithm, used for
//! [crate::options::FilterMode::Hq]. It doubles the size of an image, rounding
//! off the staircase edges of pixel art without blurring it.
//!
//! See <https://www.scale2x.it/algorithm>.

/// Scale up RGBA8 `pixels` of size `width`×`height` to twice the size,
/// writing them into `out`.
pub fn scale2x_rgba8(pixels: &[u8], (width, height): (u32, u32), out: &mut Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    assert_eq!(pixels.len(), w * h * 4);
    // [u8; 4] comparisons compile to 32-bit integer comparisons.
    let (src, _) = pixels.as_chunks::<4>();

    // Every byte is overwritten below, so there's no need to clear it.
    out.resize(w * h * 16, 0);
    if w == 0 || h == 0 {
        return;
    }
    let (dst, _) = out.as_chunks_mut::<4>();
    let out_w = w * 2;
    for y in 0..h {
        let above = &src[y.saturating_sub(1) * w..][..w];
        let row = &src[y * w..][..w];
        let below = &src[(y + 1).min(h - 1) * w..][..w];
        for x in 0..w {
            // B
            // D E F
            //   H
            let b = above[x];
            let d = row[x.saturating_sub(1)];
            let e = row[x];
            let f = row[(x + 1).min(w - 1)];
            let hh = below[x];
            let (e0, e1, e2, e3) = if b != hh && d != f {
                (
                    if d == b { d } else { e },
                    if b == f { f } else { e },
                    if d == hh { d } else { e },
                    if hh == f { f } else { e },
                )
            } else {
                (e, e, e, e)
            };
            let top = 2 * y * out_w + 2 * x;
            dst[top] = e0;
            dst[top + 1] = e1;
            dst[top + out_w] = e2;
            dst[top + out_w + 1] = e3;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_bytes(pixels: &[u32]) -> Vec<u8> {
        pixels.iter().flat_map(|p| p.to_ne_bytes()).collect()
    }

    #[test]
    fn test_scale2x() {
        // A flat image stays flat.
        let mut out = Vec::new();
        scale2x_rgba8(&to_bytes(&[7; 6]), (3, 2), &mut out);
        assert_eq!(out, to_bytes(&[7; 24]));

        // A diagonal edge is smoothed: the corner of the X pixel next to two
        // O pixels becomes O.
        //   O O        O O O O
        //   O X   ->   O O O O
        //              O O O X
        //              O O X X
        let (o, x) = (0x11111111, 0x22222222);
        scale2x_rgba8(&to_bytes(&[o, o, o, x]), (2, 2), &mut out);
        #[rustfmt::skip]
        let expected = [
            o, o, o, o,
            o, o, o, o,
            o, o, o, x,
            o, o, x, x,
        ];
        assert_eq!(out, to_bytes(&expected));
    }
}
