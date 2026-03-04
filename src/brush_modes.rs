// mypaint-rs: brush_modes.rs
// Rewrite of libmypaint/brushmodes.c
// Original Copyright (C) 2007-2014 Martin Renold et al.
// Licensed under ISC License

// ── 像素格式说明 ──────────────────────────────────────────────────────────────
//
// rgba: 预乘 alpha 的 16 位 RGBA，每分量范围 [0, 2^15]。
// mask: LRE 编码的 dab 形状。布局为：
//         [非零像素强度, ..., 0（行结束）, 跳过像素数（0表示结束）, ...]
//       即交替出现"运行段"和"跳过段"，以 mask[1]==0 标记终止。
// opacity: 混合模式的整体强度，与 mask 值共同决定最终不透明度。

use crate::helpers::{rgb_to_spectral, spectral_to_rgb};
use rand::RngExt;

const SCALE: u32 = 1 << 15; // 32768

// ── 亮度系数（BT.601 / W3C Compositing spec）────────────────────────────────

const LUMA_R: f32 = 0.2126 * SCALE as f32;
const LUMA_G: f32 = 0.7152 * SCALE as f32;
const LUMA_B: f32 = 0.0722 * SCALE as f32;

#[inline(always)]
fn luma(r: u16, g: u16, b: u16) -> u32 {
    (r as f32 * LUMA_R + g as f32 * LUMA_G + b as f32 * LUMA_B) as u32 / SCALE
}

// ── mask 迭代器 ───────────────────────────────────────────────────────────────
//
// 将 LRE 编码的 mask 抽象为迭代器，每次 yield (mask_value, rgba_offset)。
// 调用方负责维护 rgba 切片的当前偏移量。

// ── 混合模式实现 ──────────────────────────────────────────────────────────────

/// Normal（正常）混合模式。
/// 对每个被 mask 覆盖的像素执行预乘 alpha 的 "over" 合成。
pub fn draw_dab_pixels_normal(
    mask: &[u16],
    rgba: &mut [u16],
    color_r: u16,
    color_g: u16,
    color_b: u16,
    opacity: u16,
) {
    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let opa_a = mask[mi] as u32 * opacity as u32 / SCALE;
            let opa_b = SCALE - opa_a;
            rgba[ri + 3] = (opa_a + opa_b * rgba[ri + 3] as u32 / SCALE) as u16;
            rgba[ri] = ((opa_a * color_r as u32 + opa_b * rgba[ri] as u32) / SCALE) as u16;
            rgba[ri + 1] = ((opa_a * color_g as u32 + opa_b * rgba[ri + 1] as u32) / SCALE) as u16;
            rgba[ri + 2] = ((opa_a * color_b as u32 + opa_b * rgba[ri + 2] as u32) / SCALE) as u16;
            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

/// Normal Paint（颜料正常）混合模式。
/// 使用加权几何均值（WGM）在光谱空间做减色混合。
/// 低不透明度时自动回退到加色混合以避免舍入噪声。
pub fn draw_dab_pixels_normal_paint(
    mask: &[u16],
    rgba: &mut [u16],
    color_r: u16,
    color_g: u16,
    color_b: u16,
    opacity: u16,
) {
    // 将笔刷颜色上采样到光谱
    let mut spectral_a = [0.0f32; 10];
    rgb_to_spectral(
        color_r as f32 / SCALE as f32,
        color_g as f32 / SCALE as f32,
        color_b as f32 / SCALE as f32,
        &mut spectral_a,
    );
    // 极低不透明度会导致 int↔float 往返噪声，强制最低值
    let opacity = opacity.max(150);

    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let opa_a = mask[mi] as u32 * opacity as u32 / SCALE;
            let opa_b = SCALE - opa_a;

            // 背景透明时直接加色混合
            if rgba[ri + 3] == 0 {
                rgba[ri + 3] = (opa_a + opa_b * rgba[ri + 3] as u32 / SCALE) as u16;
                rgba[ri] = ((opa_a * color_r as u32 + opa_b * rgba[ri] as u32) / SCALE) as u16;
                rgba[ri + 1] =
                    ((opa_a * color_g as u32 + opa_b * rgba[ri + 1] as u32) / SCALE) as u16;
                rgba[ri + 2] =
                    ((opa_a * color_b as u32 + opa_b * rgba[ri + 2] as u32) / SCALE) as u16;
                mi += 1;
                ri += 4;
                continue;
            }

            // WGM 混合因子（alpha 加权，总和为 1）
            let fac_a =
                opa_a as f32 / (opa_a as f32 + opa_b as f32 * rgba[ri + 3] as f32 / SCALE as f32);
            let fac_b = 1.0 - fac_a;

            // 背景去预乘后上采样到光谱
            let mut spectral_b = [0.0f32; 10];
            let alpha_f = rgba[ri + 3] as f32;
            rgb_to_spectral(
                rgba[ri] as f32 / alpha_f,
                rgba[ri + 1] as f32 / alpha_f,
                rgba[ri + 2] as f32 / alpha_f,
                &mut spectral_b,
            );

            // 光谱 WGM
            let mut spectral_mix = [0.0f32; 10];
            for i in 0..10 {
                spectral_mix[i] = spectral_a[i].powf(fac_a) * spectral_b[i].powf(fac_b);
            }

            // 转回 RGB 并重新预乘 alpha
            let mut rgb_result = [0.0f32; 3];
            spectral_to_rgb(&spectral_mix, &mut rgb_result);
            rgba[ri + 3] = (opa_a + opa_b * rgba[ri + 3] as u32 / SCALE) as u16;
            let a = rgba[ri + 3] as f32;
            rgba[ri] = (rgb_result[0] * a + 0.5) as u16;
            rgba[ri + 1] = (rgb_result[1] * a + 0.5) as u16;
            rgba[ri + 2] = (rgb_result[2] * a + 0.5) as u16;

            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

/// Posterize（色调分离）混合模式。
/// 将画布颜色量化为 `posterize_num` 级，再与原色按 opacity 混合。
/// 不影响 alpha 通道。
pub fn draw_dab_pixels_posterize(mask: &[u16], rgba: &mut [u16], opacity: u16, posterize_num: u16) {
    let pn = posterize_num as f32;
    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let r = rgba[ri] as f32 / SCALE as f32;
            let g = rgba[ri + 1] as f32 / SCALE as f32;
            let b = rgba[ri + 2] as f32 / SCALE as f32;

            let post_r = ((SCALE as f32 * (r * pn).round() / pn) as u32).min(SCALE);
            let post_g = ((SCALE as f32 * (g * pn).round() / pn) as u32).min(SCALE);
            let post_b = ((SCALE as f32 * (b * pn).round() / pn) as u32).min(SCALE);

            let opa_a = mask[mi] as u32 * opacity as u32 / SCALE;
            let opa_b = SCALE - opa_a;
            rgba[ri] = ((opa_a * post_r + opa_b * rgba[ri] as u32) / SCALE) as u16;
            rgba[ri + 1] = ((opa_a * post_g + opa_b * rgba[ri + 1] as u32) / SCALE) as u16;
            rgba[ri + 2] = ((opa_a * post_b + opa_b * rgba[ri + 2] as u32) / SCALE) as u16;

            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

/// 将 `bot` 的亮度替换为 `top` 的亮度，保留 `bot` 的色相和饱和度。
/// 对应 PDF/SVG 合成规范中的 SetLum + ClipColor。
fn set_rgb16_lum_from_rgb16(
    topr: u16,
    topg: u16,
    topb: u16,
    botr: &mut u16,
    botg: &mut u16,
    botb: &mut u16,
) {
    let botlum = luma(*botr, *botg, *botb) as i32;
    let toplum = luma(topr, topg, topb) as i32;
    let diff = botlum - toplum;

    let mut r = topr as i32 + diff;
    let mut g = topg as i32 + diff;
    let mut b = topb as i32 + diff;

    // ClipColor
    let lum = luma(
        r.clamp(0, SCALE as i32) as u16,
        g.clamp(0, SCALE as i32) as u16,
        b.clamp(0, SCALE as i32) as u16,
    ) as i32;
    let cmin = r.min(g).min(b);
    let cmax = r.max(g).max(b);

    if cmin < 0 {
        let d = lum - cmin;
        r = lum + (r - lum) * lum / d;
        g = lum + (g - lum) * lum / d;
        b = lum + (b - lum) * lum / d;
    }
    if cmax > SCALE as i32 {
        let d = cmax - lum;
        let room = SCALE as i32 - lum;
        r = lum + (r - lum) * room / d;
        g = lum + (g - lum) * room / d;
        b = lum + (b - lum) * room / d;
    }

    debug_assert!(r >= 0 && r <= SCALE as i32);
    debug_assert!(g >= 0 && g <= SCALE as i32);
    debug_assert!(b >= 0 && b <= SCALE as i32);

    *botr = r as u16;
    *botg = g as u16;
    *botb = b as u16;
}

/// Color（上色）混合模式。
/// 将笔刷的色相+饱和度应用于画布，保留画布亮度。
/// 参考 Adobe PDF Blend Modes Addendum（2006）。
pub fn draw_dab_pixels_color(
    mask: &[u16],
    rgba: &mut [u16],
    color_r: u16,
    color_g: u16,
    color_b: u16,
    opacity: u16,
) {
    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let a = rgba[ri + 3] as u32;

            // 去预乘
            let (mut r, mut g, mut b) = if a != 0 {
                (
                    (SCALE * rgba[ri] as u32 / a) as u16,
                    (SCALE * rgba[ri + 1] as u32 / a) as u16,
                    (SCALE * rgba[ri + 2] as u32 / a) as u16,
                )
            } else {
                (0u16, 0u16, 0u16)
            };

            set_rgb16_lum_from_rgb16(color_r, color_g, color_b, &mut r, &mut g, &mut b);

            // 重新预乘
            let r = r as u32 * a / SCALE;
            let g = g as u32 * a / SCALE;
            let b = b as u32 * a / SCALE;

            let opa_a = mask[mi] as u32 * opacity as u32 / SCALE;
            let opa_b = SCALE - opa_a;
            rgba[ri] = ((opa_a * r + opa_b * rgba[ri] as u32) / SCALE) as u16;
            rgba[ri + 1] = ((opa_a * g + opa_b * rgba[ri + 1] as u32) / SCALE) as u16;
            rgba[ri + 2] = ((opa_a * b + opa_b * rgba[ri + 2] as u32) / SCALE) as u16;

            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

/// Normal + Eraser 混合模式。
/// `color_a=1.0*SCALE` → 正常绘制；`color_a=0` → 完全擦除。
/// 中间值可模拟半透明拖拽（smudge）。
pub fn draw_dab_pixels_normal_and_eraser(
    mask: &[u16],
    rgba: &mut [u16],
    color_r: u16,
    color_g: u16,
    color_b: u16,
    color_a: u16,
    opacity: u16,
) {
    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let opa_a = mask[mi] as u32 * opacity as u32 / SCALE;
            let opa_b = SCALE - opa_a;
            let opa_a = opa_a * color_a as u32 / SCALE;
            rgba[ri + 3] = (opa_a + opa_b * rgba[ri + 3] as u32 / SCALE) as u16;
            rgba[ri] = ((opa_a * color_r as u32 + opa_b * rgba[ri] as u32) / SCALE) as u16;
            rgba[ri + 1] = ((opa_a * color_g as u32 + opa_b * rgba[ri + 1] as u32) / SCALE) as u16;
            rgba[ri + 2] = ((opa_a * color_b as u32 + opa_b * rgba[ri + 2] as u32) / SCALE) as u16;
            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

/// 平滑 sigmoid 函数，用于在加色和光谱混合之间过渡。
/// 低不透明度偏向加色，高不透明度偏向光谱。
#[inline]
fn spectral_blend_factor(x: f32) -> f32 {
    const VER_FAC: f32 = 1.65;
    const HOR_FAC: f32 = 8.0;
    const HOR_OFFS: f32 = 3.0;
    let b = x * HOR_FAC - HOR_OFFS;
    0.5 + b / (1.0 + b.abs() * VER_FAC)
}

/// Normal + Eraser Paint（颜料正常+擦除）混合模式。
/// 结合光谱减色混合与加色混合，根据画布当前 alpha 平滑过渡。
pub fn draw_dab_pixels_normal_and_eraser_paint(
    mask: &[u16],
    rgba: &mut [u16],
    color_r: u16,
    color_g: u16,
    color_b: u16,
    color_a: u16,
    opacity: u16,
) {
    let mut spectral_a = [0.0f32; 10];
    rgb_to_spectral(
        color_r as f32 / SCALE as f32,
        color_g as f32 / SCALE as f32,
        color_b as f32 / SCALE as f32,
        &mut spectral_a,
    );

    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let opa_a = mask[mi] as u32 * opacity as u32 / SCALE;
            let opa_b = SCALE - opa_a;
            let opa_a2 = opa_a * color_a as u32 / SCALE;
            let opa_out = opa_a2 + opa_b * rgba[ri + 3] as u32 / SCALE;

            let sf = (spectral_blend_factor(rgba[ri + 3] as f32 / SCALE as f32)).clamp(0.0, 1.0);
            let af = 1.0 - sf;

            let mut rgb = [0u32; 3];

            // 加色路径
            if af > 0.0 {
                rgb[0] = (opa_a2 * color_r as u32 + opa_b * rgba[ri] as u32) / SCALE;
                rgb[1] = (opa_a2 * color_g as u32 + opa_b * rgba[ri + 1] as u32) / SCALE;
                rgb[2] = (opa_a2 * color_b as u32 + opa_b * rgba[ri + 2] as u32) / SCALE;
            }

            // 光谱路径
            if sf > 0.0 && rgba[ri + 3] != 0 {
                let mut spectral_b = [0.0f32; 10];
                let alpha_f = rgba[ri + 3] as f32;
                rgb_to_spectral(
                    rgba[ri] as f32 / alpha_f,
                    rgba[ri + 1] as f32 / alpha_f,
                    rgba[ri + 2] as f32 / alpha_f,
                    &mut spectral_b,
                );

                let mut fac_a = opa_a as f32
                    / (opa_a as f32 + opa_b as f32 * rgba[ri + 3] as f32 / SCALE as f32);
                fac_a *= color_a as f32 / SCALE as f32;
                let fac_b = 1.0 - fac_a;

                let mut spectral_mix = [0.0f32; 10];
                for i in 0..10 {
                    spectral_mix[i] = spectral_a[i].powf(fac_a) * spectral_b[i].powf(fac_b);
                }

                let mut rgb_result = [0.0f32; 3];
                spectral_to_rgb(&spectral_mix, &mut rgb_result);

                for i in 0..3 {
                    rgb[i] = (af * rgb[i] as f32 + sf * rgb_result[i] * opa_out as f32) as u32;
                }
            }

            rgba[ri + 3] = opa_out as u16;
            rgba[ri] = rgb[0] as u16;
            rgba[ri + 1] = rgb[1] as u16;
            rgba[ri + 2] = rgb[2] as u16;

            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

/// LockAlpha（锁定透明度）混合模式。
/// 仅修改颜色，alpha 保持不变。
pub fn draw_dab_pixels_lock_alpha(
    mask: &[u16],
    rgba: &mut [u16],
    color_r: u16,
    color_g: u16,
    color_b: u16,
    opacity: u16,
) {
    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let opa_a = mask[mi] as u32 * opacity as u32 / SCALE * rgba[ri + 3] as u32 / SCALE;
            let opa_b = SCALE - opa_a;
            rgba[ri] = ((opa_a * color_r as u32 + opa_b * rgba[ri] as u32) / SCALE) as u16;
            rgba[ri + 1] = ((opa_a * color_g as u32 + opa_b * rgba[ri + 1] as u32) / SCALE) as u16;
            rgba[ri + 2] = ((opa_a * color_b as u32 + opa_b * rgba[ri + 2] as u32) / SCALE) as u16;
            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

/// LockAlpha Paint（颜料锁定透明度）混合模式。
pub fn draw_dab_pixels_lock_alpha_paint(
    mask: &[u16],
    rgba: &mut [u16],
    color_r: u16,
    color_g: u16,
    color_b: u16,
    opacity: u16,
) {
    let mut spectral_a = [0.0f32; 10];
    rgb_to_spectral(
        color_r as f32 / SCALE as f32,
        color_g as f32 / SCALE as f32,
        color_b as f32 / SCALE as f32,
        &mut spectral_a,
    );
    let opacity = opacity.max(150);

    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let raw_opa_a = mask[mi] as u32 * opacity as u32 / SCALE;
            let opa_b = SCALE - raw_opa_a;
            let opa_a = raw_opa_a * rgba[ri + 3] as u32 / SCALE;

            if rgba[ri + 3] == 0 {
                rgba[ri] = ((opa_a * color_r as u32 + opa_b * rgba[ri] as u32) / SCALE) as u16;
                rgba[ri + 1] =
                    ((opa_a * color_g as u32 + opa_b * rgba[ri + 1] as u32) / SCALE) as u16;
                rgba[ri + 2] =
                    ((opa_a * color_b as u32 + opa_b * rgba[ri + 2] as u32) / SCALE) as u16;
                mi += 1;
                ri += 4;
                continue;
            }

            let fac_a =
                opa_a as f32 / (opa_a as f32 + opa_b as f32 * rgba[ri + 3] as f32 / SCALE as f32);
            let fac_b = 1.0 - fac_a;

            let mut spectral_b = [0.0f32; 10];
            let alpha_f = rgba[ri + 3] as f32;
            rgb_to_spectral(
                rgba[ri] as f32 / alpha_f,
                rgba[ri + 1] as f32 / alpha_f,
                rgba[ri + 2] as f32 / alpha_f,
                &mut spectral_b,
            );

            let mut spectral_mix = [0.0f32; 10];
            for i in 0..10 {
                spectral_mix[i] = spectral_a[i].powf(fac_a) * spectral_b[i].powf(fac_b);
            }

            let mut rgb_result = [0.0f32; 3];
            spectral_to_rgb(&spectral_mix, &mut rgb_result);
            let a = rgba[ri + 3] as f32;
            rgba[ri] = (rgb_result[0] * a + 0.5) as u16;
            rgba[ri + 1] = (rgb_result[1] * a + 0.5) as u16;
            rgba[ri + 2] = (rgb_result[2] * a + 0.5) as u16;

            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }
}

// ── 颜色采样 ──────────────────────────────────────────────────────────────────

/// 传统颜色采样（不区分颜料模式）。
/// 对 mask 覆盖区域做加权求和，结果累加到 sum_* 中。
pub fn get_color_pixels_legacy(
    mask: &[u16],
    rgba: &[u16],
    sum_weight: &mut f32,
    sum_r: &mut f32,
    sum_g: &mut f32,
    sum_b: &mut f32,
    sum_a: &mut f32,
) {
    let mut weight = 0u32;
    let mut r = 0u32;
    let mut g = 0u32;
    let mut b = 0u32;
    let mut a = 0u32;

    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let opa = mask[mi] as u32;
            weight += opa;
            r += opa * rgba[ri] as u32 / SCALE;
            g += opa * rgba[ri + 1] as u32 / SCALE;
            b += opa * rgba[ri + 2] as u32 / SCALE;
            a += opa * rgba[ri + 3] as u32 / SCALE;
            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }

    *sum_weight += weight as f32;
    *sum_r += r as f32;
    *sum_g += g as f32;
    *sum_b += b as f32;
    *sum_a += a as f32;
}

/// 带间隔采样的颜色累积函数。
///
/// - `paint < 0`  → 回退到 legacy 采样
/// - `paint == 0` → 纯加色 RGB 采样
/// - `paint == 1` → 纯光谱采样
/// - 中间值      → 两者线性混合
///
/// `sample_interval`：每 N 个像素至少采样一次。
/// `random_sample_rate`：额外随机采样概率 [0, 1]。
pub fn get_color_pixels_accumulate(
    mask: &[u16],
    rgba: &[u16],
    sum_weight: &mut f32,
    sum_r: &mut f32,
    sum_g: &mut f32,
    sum_b: &mut f32,
    sum_a: &mut f32,
    paint: f32,
    sample_interval: u16,
    random_sample_rate: f32,
) {
    if paint < 0.0 {
        get_color_pixels_legacy(mask, rgba, sum_weight, sum_r, sum_g, sum_b, sum_a);
        return;
    }

    let mut avg_spectral = [0.0f32; 10];
    let mut avg_rgb = [*sum_r, *sum_g, *sum_b];
    if paint > 0.0 {
        rgb_to_spectral(*sum_r, *sum_g, *sum_b, &mut avg_spectral);
    }

    let mut interval_counter = 0u16;
    let mut rng = rand::rng();

    let mut mi = 0usize;
    let mut ri = 0usize;
    loop {
        while mask[mi] != 0 {
            let should_sample = interval_counter == 0 || rng.random::<f32>() < random_sample_rate;

            if should_sample {
                let a = mask[mi] as f32 * rgba[ri + 3] as f32 / (1u64 << 30) as f32;
                let alpha_sums = a + *sum_a;
                *sum_weight += mask[mi] as f32 / SCALE as f32;

                let (fac_a, fac_b) = if alpha_sums > 0.0 {
                    let fa = a / alpha_sums;
                    (fa, 1.0 - fa)
                } else {
                    (1.0, 1.0)
                };

                if paint > 0.0 && rgba[ri + 3] > 0 {
                    let mut spectral = [0.0f32; 10];
                    let alpha_f = rgba[ri + 3] as f32;
                    rgb_to_spectral(
                        rgba[ri] as f32 / alpha_f,
                        rgba[ri + 1] as f32 / alpha_f,
                        rgba[ri + 2] as f32 / alpha_f,
                        &mut spectral,
                    );
                    for i in 0..10 {
                        avg_spectral[i] = spectral[i].powf(fac_a) * avg_spectral[i].powf(fac_b);
                    }
                }

                if paint < 1.0 && rgba[ri + 3] > 0 {
                    let alpha_f = rgba[ri + 3] as f32;
                    for i in 0..3 {
                        avg_rgb[i] = rgba[ri + i] as f32 * fac_a / alpha_f + avg_rgb[i] * fac_b;
                    }
                }

                *sum_a += a;
            }

            interval_counter = (interval_counter + 1) % sample_interval;
            mi += 1;
            ri += 4;
        }
        if mask[mi + 1] == 0 {
            break;
        }
        ri += mask[mi + 1] as usize * 4;
        mi += 2;
    }

    let mut spec_rgb = [0.0f32; 3];
    spectral_to_rgb(&avg_spectral, &mut spec_rgb);

    *sum_r = spec_rgb[0] * paint + (1.0 - paint) * avg_rgb[0];
    *sum_g = spec_rgb[1] * paint + (1.0 - paint) * avg_rgb[1];
    *sum_b = spec_rgb[2] * paint + (1.0 - paint) * avg_rgb[2];
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最简单的连续 mask：N 个非零值 + 两个终止零
    fn simple_mask(values: &[u16]) -> Vec<u16> {
        let mut m: Vec<u16> = values.to_vec();
        m.push(0); // 行结束
        m.push(0); // 全局结束
        m
    }

    #[test]
    fn test_normal_full_opacity_overwrites() {
        // opacity = SCALE，mask = SCALE → opa_a = SCALE，完全覆盖
        let mask = simple_mask(&[SCALE as u16]);
        let mut rgba = vec![100u16, 200, 50, SCALE as u16];
        draw_dab_pixels_normal(&mask, &mut rgba, 1000, 2000, 3000, SCALE as u16);
        assert_eq!(rgba[0], 1000);
        assert_eq!(rgba[1], 2000);
        assert_eq!(rgba[2], 3000);
        assert_eq!(rgba[3], SCALE as u16);
    }

    #[test]
    fn test_normal_zero_opacity_no_change() {
        let mask = simple_mask(&[SCALE as u16]);
        let mut rgba = vec![100u16, 200, 50, 10000u16];
        let orig = rgba.clone();
        draw_dab_pixels_normal(&mask, &mut rgba, 1000, 2000, 3000, 0);
        assert_eq!(rgba, orig);
    }

    #[test]
    fn test_lock_alpha_does_not_change_alpha() {
        let mask = simple_mask(&[SCALE as u16]);
        let original_alpha = 20000u16;
        let mut rgba = vec![100u16, 100, 100, original_alpha];
        draw_dab_pixels_lock_alpha(&mask, &mut rgba, 5000, 5000, 5000, SCALE as u16);
        assert_eq!(
            rgba[3], original_alpha,
            "alpha must not change in LockAlpha mode"
        );
    }

    #[test]
    fn test_eraser_zero_color_a_clears_alpha() {
        let mask = simple_mask(&[SCALE as u16]);
        let mut rgba = vec![1000u16, 2000, 3000, SCALE as u16];
        draw_dab_pixels_normal_and_eraser(&mask, &mut rgba, 0, 0, 0, 0, SCALE as u16);
        assert_eq!(rgba[3], 0, "full erase should zero alpha");
    }

    #[test]
    fn test_posterize_preserves_alpha() {
        let mask = simple_mask(&[SCALE as u16]);
        let original_alpha = 15000u16;
        let mut rgba = vec![10000u16, 20000, 5000, original_alpha];
        draw_dab_pixels_posterize(&mask, &mut rgba, SCALE as u16, 4);
        assert_eq!(rgba[3], original_alpha, "posterize must not touch alpha");
    }

    #[test]
    fn test_normal_paint_transparent_bg_matches_normal() {
        // 背景透明时 Paint 模式应退化为普通加色混合
        let mask = simple_mask(&[SCALE as u16]);
        let color = (5000u16, 10000u16, 15000u16);
        let opacity = SCALE as u16 / 2;

        let mut rgba_normal = vec![0u16, 0, 0, 0];
        let mut rgba_paint = vec![0u16, 0, 0, 0];

        draw_dab_pixels_normal(&mask, &mut rgba_normal, color.0, color.1, color.2, opacity);
        draw_dab_pixels_normal_paint(&mask, &mut rgba_paint, color.0, color.1, color.2, opacity);

        // 结果应非常接近（允许最小不透明度截断带来的 1 LSB 误差）
        for i in 0..4 {
            let diff = (rgba_normal[i] as i32 - rgba_paint[i] as i32).abs();
            assert!(
                diff <= 1,
                "channel {i}: normal={} paint={}",
                rgba_normal[i],
                rgba_paint[i]
            );
        }
    }

    #[test]
    fn test_get_color_legacy_accumulates() {
        let mask = simple_mask(&[SCALE as u16, SCALE as u16]);
        let rgba = vec![
            10000u16,
            20000,
            5000,
            SCALE as u16,
            5000u16,
            10000,
            2500,
            SCALE as u16,
        ];
        let (mut sw, mut sr, mut sg, mut sb, mut sa) = (0.0f32, 0.0, 0.0, 0.0, 0.0);
        get_color_pixels_legacy(&mask, &rgba, &mut sw, &mut sr, &mut sg, &mut sb, &mut sa);
        assert!(sw > 0.0);
        assert!(sr > 0.0);
        assert!(sa > 0.0);
    }
}
