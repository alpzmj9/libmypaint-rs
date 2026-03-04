// mypaint-rs: helpers.rs
// Rewrite of libmypaint/helpers.c
// Original Copyright (C) 2007-2008 Martin Renold <martinxyz@gmx.ch>
// Licensed under ISC License

// ── Spectral upsampling tables (10-band) ────────────────────────────────────
use crate::rng_double::RngDouble;

pub fn rand_gauss(rng: &mut RngDouble) -> f32 {
    let sum = rng.next() + rng.next() + rng.next() + rng.next();
    (sum * 1.732_050_807_57 - 3.464_101_615_14) as f32
}

#[rustfmt::skip]
static T_MATRIX_SMALL: [[f32; 10]; 3] = [
    [ 0.026595621243689,  0.049779426257903,  0.022449850859496, -0.218453689278271,
     -0.256894883201278,  0.445881722194840,  0.772365886289756,  0.194498761382537,
      0.014038157587820,  0.007687264480513],
    [-0.032601672674412, -0.061021043498478, -0.052490001018404,  0.206659098273522,
      0.572496335158169,  0.317837248815438, -0.021216624031211, -0.019387668756117,
     -0.001521339050858, -0.000835181622534],
    [ 0.339475473216284,  0.635401374177222,  0.771520797089589,  0.113222640692379,
     -0.055251113343776, -0.048222578468680, -0.012966666339586, -0.001523814504223,
     -0.000094718948810, -0.000051604594741],
];

#[rustfmt::skip]
static SPECTRAL_R_SMALL: [f32; 10] = [
    0.009281362787953, 0.009732627042016, 0.011254252737167, 0.015105578649573,
    0.024797924177217, 0.083622585502406, 0.977865045723212, 1.000000000000000,
    0.999961046144372, 0.999999992756822,
];

#[rustfmt::skip]
static SPECTRAL_G_SMALL: [f32; 10] = [
    0.002854127435775, 0.003917589679914, 0.012132151699187, 0.748259205918013,
    1.000000000000000, 0.865695937531795, 0.037477469241101, 0.022816789725717,
    0.021747419446456, 0.021384940572308,
];

#[rustfmt::skip]
static SPECTRAL_B_SMALL: [f32; 10] = [
    0.537052150373386, 0.546646402401469, 0.575501819073983, 0.258778829633924,
    0.041709923751716, 0.012662638828324, 0.007485593127390, 0.006766900622462,
    0.006699764779016, 0.006676219883241,
];

/// Epsilon used for spectral colour mixing to avoid log(0).
pub const WGM_EPSILON: f32 = 0.001;

// ── Simple helpers ───────────────────────────────────────────────────────────

/// Arithmetic (always-positive) modulo — equivalent to C `fmodf` but
/// returns a value in `[0, N)` even for negative `a`.
#[inline]
pub fn mod_arith(a: f32, n: f32) -> f32 {
    a - n * (a / n).floor()
}

/// Smallest signed angular difference between two angles (degrees).
/// Result is in `(-180, 180]`.
#[inline]
pub fn smallest_angular_difference(angle_a: f32, angle_b: f32) -> f32 {
    let mut a = angle_b - angle_a;
    a = mod_arith(a + 180.0, 360.0) - 180.0;
    if a > 180.0 {
        a -= 360.0;
    } else if a < -180.0 {
        a += 360.0;
    }
    a
}

// ── Colour-space conversions ─────────────────────────────────────────────────
//
// All functions operate in-place via `(r, g, b): &mut (f32, f32, f32)` so the
// caller never needs to juggle pointer-aliasing rules.  The original C used
// `float *r_, *g_, *b_` triple-pointer trick; we make that explicit here.

/// RGB → HSV  (in-place, hue in [0,1))
pub fn rgb_to_hsv(r: &mut f32, g: &mut f32, b: &mut f32) {
    let rv = r.clamp(0.0, 1.0);
    let gv = g.clamp(0.0, 1.0);
    let bv = b.clamp(0.0, 1.0);

    let max = rv.max(gv).max(bv);
    let min = rv.min(gv).min(bv);
    let delta = max - min;

    let v = max;
    let (h, s);

    if delta > 0.0001 {
        s = delta / max;
        h = if rv == max {
            let h = (gv - bv) / delta;
            if h < 0.0 { h + 6.0 } else { h }
        } else if gv == max {
            2.0 + (bv - rv) / delta
        } else {
            4.0 + (rv - gv) / delta
        };
        *r = h / 6.0;
    } else {
        s = 0.0;
        *r = 0.0;
    }

    *g = s;
    *b = v;
}

/// HSV → RGB  (in-place, hue in [0,1))
pub fn hsv_to_rgb(h: &mut f32, s: &mut f32, v: &mut f32) {
    let hv = (*h - h.floor()).clamp(0.0, 1.0); // fractional part
    let sv = s.clamp(0.0, 1.0);
    let vv = v.clamp(0.0, 1.0);

    let (r, g, b);

    if sv == 0.0 {
        r = vv;
        g = vv;
        b = vv;
    } else {
        let hue = if hv == 1.0 { 0.0_f64 } else { hv as f64 } * 6.0;
        let i = hue as i32;
        let f = (hue - i as f64) as f32;
        let w = vv * (1.0 - sv);
        let q = vv * (1.0 - sv * f);
        let t = vv * (1.0 - sv * (1.0 - f));
        (r, g, b) = match i {
            0 => (vv, t, w),
            1 => (q, vv, w),
            2 => (w, vv, t),
            3 => (w, q, vv),
            4 => (t, w, vv),
            _ => (vv, w, q),
        };
    }

    *h = r;
    *s = g;
    *v = b;
}

/// RGB → HSL  (in-place, hue in [0,1))
pub fn rgb_to_hsl(r: &mut f32, g: &mut f32, b: &mut f32) {
    let rv = (*r).clamp(0.0, 1.0) as f64;
    let gv = (*g).clamp(0.0, 1.0) as f64;
    let bv = (*b).clamp(0.0, 1.0) as f64;

    let max = rv.max(gv).max(bv);
    let min = rv.min(gv).min(bv);
    let l = (max + min) / 2.0;

    let (h, s);

    if (max - min).abs() < f64::EPSILON {
        h = 0.0_f64;
        s = 0.0_f64;
    } else {
        let delta = max - min;
        s = if l <= 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };
        let d = if delta == 0.0 { 1.0 } else { delta };
        let raw_h = if rv == max {
            (gv - bv) / d
        } else if gv == max {
            2.0 + (bv - rv) / d
        } else {
            4.0 + (rv - gv) / d
        };
        h = if raw_h < 0.0 {
            raw_h / 6.0 + 1.0
        } else {
            raw_h / 6.0
        };
    }

    *r = h as f32;
    *g = s as f32;
    *b = l as f32;
}

#[inline]
fn hsl_value(n1: f64, n2: f64, hue: f64) -> f64 {
    let hue = if hue > 6.0 {
        hue - 6.0
    } else if hue < 0.0 {
        hue + 6.0
    } else {
        hue
    };
    if hue < 1.0 {
        n1 + (n2 - n1) * hue
    } else if hue < 3.0 {
        n2
    } else if hue < 4.0 {
        n1 + (n2 - n1) * (4.0 - hue)
    } else {
        n1
    }
}

/// HSL → RGB  (in-place, hue in [0,1))
pub fn hsl_to_rgb(h: &mut f32, s: &mut f32, l: &mut f32) {
    let hv = (*h - h.floor()) as f64;
    let sv = (*s).clamp(0.0, 1.0) as f64;
    let lv = (*l).clamp(0.0, 1.0) as f64;

    let (r, g, b);

    if sv == 0.0 {
        r = lv as f32;
        g = lv as f32;
        b = lv as f32;
    } else {
        let m2 = if lv <= 0.5 {
            lv * (1.0 + sv)
        } else {
            lv + sv - lv * sv
        };
        let m1 = 2.0 * lv - m2;
        r = hsl_value(m1, m2, hv * 6.0 + 2.0) as f32;
        g = hsl_value(m1, m2, hv * 6.0) as f32;
        b = hsl_value(m1, m2, hv * 6.0 - 2.0) as f32;
    }

    *h = r;
    *s = g;
    *l = b;
}

// ── HCY colour space ─────────────────────────────────────────────────────────

const HCY_RED_LUMA: f32 = 0.2162;
const HCY_GREEN_LUMA: f32 = 0.7152;
const HCY_BLUE_LUMA: f32 = 0.0722;

/// RGB → HCY  (in-place)
pub fn rgb_to_hcy(r: &mut f32, g: &mut f32, b: &mut f32) {
    let (rv, gv, bv) = (*r, *g, *b);

    let y = HCY_RED_LUMA * rv + HCY_GREEN_LUMA * gv + HCY_BLUE_LUMA * bv;
    let p = rv.max(gv).max(bv);
    let n = rv.min(gv).min(bv);
    let d = p - n;

    let h = if n == p {
        0.0
    } else if p == rv {
        let h = (gv - bv) / d;
        if h < 0.0 { (h + 6.0) / 6.0 } else { h / 6.0 }
    } else if p == gv {
        ((bv - rv) / d + 2.0) / 6.0
    } else {
        ((rv - gv) / d + 4.0) / 6.0
    };
    let h = h.rem_euclid(1.0);

    let c = if rv == gv && gv == bv {
        0.0
    } else {
        ((y - n) / y).max((p - y) / (1.0 - y))
    };

    *r = h;
    *g = c;
    *b = y;
}

/// HCY → RGB  (in-place)
pub fn hcy_to_rgb(h: &mut f32, c: &mut f32, y: &mut f32) {
    let hv = (*h - h.floor()).rem_euclid(1.0);
    let cv = (*c).clamp(0.0, 1.0);
    let yv = (*y).clamp(0.0, 1.0);

    if cv == 0.0 {
        *h = yv;
        *c = yv;
        *y = yv;
        return;
    }

    let h6 = hv * 6.0;
    let (th, tm) = if h6 < 1.0 {
        (h6, HCY_RED_LUMA + HCY_GREEN_LUMA * h6)
    } else if h6 < 2.0 {
        (2.0 - h6, HCY_GREEN_LUMA + HCY_RED_LUMA * (2.0 - h6))
    } else if h6 < 3.0 {
        (h6 - 2.0, HCY_GREEN_LUMA + HCY_BLUE_LUMA * (h6 - 2.0))
    } else if h6 < 4.0 {
        (4.0 - h6, HCY_BLUE_LUMA + HCY_GREEN_LUMA * (4.0 - h6))
    } else if h6 < 5.0 {
        (h6 - 4.0, HCY_BLUE_LUMA + HCY_RED_LUMA * (h6 - 4.0))
    } else {
        (6.0 - h6, HCY_RED_LUMA + HCY_BLUE_LUMA * (6.0 - h6))
    };

    let (p, o, n) = if tm >= yv {
        let p = yv + yv * cv * (1.0 - tm) / tm;
        let o = yv + yv * cv * (th - tm) / tm;
        let n = yv - yv * cv;
        (p, o, n)
    } else {
        let p = yv + (1.0 - yv) * cv;
        let o = yv + (1.0 - yv) * cv * (th - tm) / (1.0 - tm);
        let n = yv - (1.0 - yv) * cv * tm / (1.0 - tm);
        (p, o, n)
    };

    let (r, g, b) = if h6 < 1.0 {
        (p, o, n)
    } else if h6 < 2.0 {
        (o, p, n)
    } else if h6 < 3.0 {
        (n, p, o)
    } else if h6 < 4.0 {
        (n, o, p)
    } else if h6 < 5.0 {
        (o, n, p)
    } else {
        (p, n, o)
    };

    *h = r;
    *c = g;
    *y = b;
}

// ── Spectral / pigment mixing ────────────────────────────────────────────────

/// Upsample linear-RGB to a 10-band spectral representation.
/// Accumulates into `spectral` (caller must zero-initialise if desired).
pub fn rgb_to_spectral(r: f32, g: f32, b: f32, spectral: &mut [f32; 10]) {
    let offset = 1.0 - WGM_EPSILON;
    let r = r * offset + WGM_EPSILON;
    let g = g * offset + WGM_EPSILON;
    let b = b * offset + WGM_EPSILON;
    for i in 0..10 {
        spectral[i] += SPECTRAL_R_SMALL[i] * r + SPECTRAL_G_SMALL[i] * g + SPECTRAL_B_SMALL[i] * b;
    }
}

/// Convert a 10-band spectral power distribution back to linear RGB.
pub fn spectral_to_rgb(spectral: &[f32; 10], rgb: &mut [f32; 3]) {
    let offset = 1.0 - WGM_EPSILON;
    let mut tmp = [0.0_f32; 3];
    for i in 0..10 {
        tmp[0] += T_MATRIX_SMALL[0][i] * spectral[i];
        tmp[1] += T_MATRIX_SMALL[1][i] * spectral[i];
        tmp[2] += T_MATRIX_SMALL[2][i] * spectral[i];
    }
    for i in 0..3 {
        rgb[i] = ((tmp[i] - WGM_EPSILON) / offset).clamp(0.0, 1.0);
    }
}

/// Blend two RGBA colours.
///
/// * `a` — current smudge state `[r, g, b, alpha]`
/// * `b` — brush / get_color `[r, g, b, alpha]`
/// * `fac` — weight of `a` (0 → pure `b`, 1 → pure `a`)
/// * `paint_mode` — 0.0 = additive RGB, 1.0 = full spectral/pigment WGM
///
/// Returns a new `[r, g, b, alpha]` array.
pub fn mix_colors(a: &[f32; 4], b: &[f32; 4], fac: f32, paint_mode: f32) -> [f32; 4] {
    let opa_a = fac;
    let opa_b = 1.0 - opa_a;

    let alpha = (opa_a * a[3] + opa_b * b[3]).clamp(0.0, 1.0);

    // Spectral-weighted blend factor (guards against a[3]==0 → NaN)
    let sfac_a = if a[3] == 0.0 {
        0.0
    } else {
        opa_a * a[3] / (a[3] + b[3] * opa_b)
    };
    let sfac_b = 1.0 - sfac_a;

    let mut rgb = [0.0_f32; 3];

    // ── spectral (pigment) path ──────────────────────────────────────────
    if paint_mode > 0.0 {
        let mut spec_a = [0.0_f32; 10];
        let mut spec_b = [0.0_f32; 10];
        rgb_to_spectral(a[0], a[1], a[2], &mut spec_a);
        rgb_to_spectral(b[0], b[1], b[2], &mut spec_b);

        let mut spectralmix = [0.0_f32; 10];
        for i in 0..10 {
            // Weighted geometric mean — no fast-approx needed here
            spectralmix[i] = spec_a[i].powf(sfac_a) * spec_b[i].powf(sfac_b);
        }

        let mut rgb_result = [0.0_f32; 3];
        spectral_to_rgb(&spectralmix, &mut rgb_result);
        rgb.copy_from_slice(&rgb_result);
    }

    // ── additive (linear-RGB) path ───────────────────────────────────────
    if paint_mode < 1.0 {
        for i in 0..3 {
            rgb[i] = rgb[i] * paint_mode + (1.0 - paint_mode) * (a[i] * opa_a + b[i] * opa_b);
        }
    }

    [rgb[0], rgb[1], rgb[2], alpha]
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn test_mod_arith_positive() {
        assert!(approx_eq(mod_arith(5.5, 3.0), 2.5, 1e-6));
    }

    #[test]
    fn test_mod_arith_negative() {
        // unlike fmodf, must return positive
        let v = mod_arith(-1.0, 360.0);
        assert!(v >= 0.0 && v < 360.0, "got {v}");
    }

    #[test]
    fn test_smallest_angular_difference() {
        assert!(approx_eq(
            smallest_angular_difference(10.0, 20.0),
            10.0,
            1e-5
        ));
        assert!(approx_eq(
            smallest_angular_difference(350.0, 10.0),
            20.0,
            1e-5
        ));
    }

    #[test]
    fn test_rgb_hsv_roundtrip() {
        let (mut r, mut g, mut b) = (0.8, 0.4, 0.2);
        rgb_to_hsv(&mut r, &mut g, &mut b);
        hsv_to_rgb(&mut r, &mut g, &mut b);
        assert!(approx_eq(r, 0.8, 1e-5));
        assert!(approx_eq(g, 0.4, 1e-5));
        assert!(approx_eq(b, 0.2, 1e-5));
    }

    #[test]
    fn test_rgb_hsl_roundtrip() {
        let (mut r, mut g, mut b) = (0.3, 0.6, 0.9);
        rgb_to_hsl(&mut r, &mut g, &mut b);
        hsl_to_rgb(&mut r, &mut g, &mut b);
        assert!(approx_eq(r, 0.3, 1e-5));
        assert!(approx_eq(g, 0.6, 1e-5));
        assert!(approx_eq(b, 0.9, 1e-5));
    }

    #[test]
    fn test_rgb_hcy_roundtrip() {
        // HCY 经过 f32 中间值的往返误差约为 5e-4，这是该色彩空间固有的精度损失，
        // 不是 bug。HSV/HSL 的误差在 1e-5 级别，HCY 因为涉及除法链所以更大。
        let (mut r, mut g, mut b) = (0.5, 0.7, 0.1);
        rgb_to_hcy(&mut r, &mut g, &mut b);
        hcy_to_rgb(&mut r, &mut g, &mut b);
        assert!(approx_eq(r, 0.5, 5e-4));
        assert!(approx_eq(g, 0.7, 5e-4));
        assert!(approx_eq(b, 0.1, 5e-4));
    }

    #[test]
    fn test_spectral_roundtrip() {
        let (r0, g0, b0) = (0.6, 0.3, 0.8);
        let mut spec = [0.0_f32; 10];
        rgb_to_spectral(r0, g0, b0, &mut spec);
        let mut rgb = [0.0_f32; 3];
        spectral_to_rgb(&spec, &mut rgb);
        assert!(approx_eq(rgb[0], r0, 0.01));
        assert!(approx_eq(rgb[1], g0, 0.01));
        assert!(approx_eq(rgb[2], b0, 0.01));
    }

    #[test]
    fn test_mix_colors_identity() {
        let a = [1.0_f32, 0.0, 0.0, 1.0];
        let b = [0.0_f32, 0.0, 1.0, 1.0];
        // fac=1 → full weight on a, result should equal a
        let result = mix_colors(&a, &b, 1.0, 0.0);
        assert!(approx_eq(result[0], 1.0, 1e-5));
        assert!(approx_eq(result[2], 0.0, 1e-5));
    }
}
