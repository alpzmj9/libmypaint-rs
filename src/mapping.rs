// mypaint-rs: mapping.rs
// Rewrite of libmypaint/mypaint-mapping.c
// Original Copyright (C) 2007-2008 Martin Renold <martinxyz@gmx.ch>
// Licensed under ISC License

/// Maximum number of control points per input curve.
const MAX_POINTS: usize = 64;

/// A piecewise-linear curve defined by up to 64 control points.
#[derive(Clone)]
struct ControlPoints {
    x: [f32; MAX_POINTS],
    y: [f32; MAX_POINTS],
    n: usize,
}

impl ControlPoints {
    const fn new() -> Self {
        Self {
            x: [0.0; MAX_POINTS],
            y: [0.0; MAX_POINTS],
            n: 0,
        }
    }

    /// Evaluate the piecewise-linear curve at `x_in`.
    fn evaluate(&self, x_in: f32) -> f32 {
        debug_assert!(self.n >= 2);

        let mut x0 = self.x[0];
        let mut y0 = self.y[0];
        let mut x1 = self.x[1];
        let mut y1 = self.y[1];

        let mut i = 2;
        while i < self.n && x_in > x1 {
            x0 = x1;
            y0 = y1;
            x1 = self.x[i];
            y1 = self.y[i];
            i += 1;
        }

        if x0 == x1 || y0 == y1 {
            y0
        } else {
            // linear interpolation
            (y1 * (x_in - x0) + y0 * (x1 - x_in)) / (x1 - x0)
        }
    }
}

/// A user-configurable input→setting mapping.
///
/// Each brush setting has one `Mapping`. The final value of a setting is
/// computed as `base_value + Σ curve_i(input_i)` over all active input curves.
pub struct Mapping {
    pub base_value: f32,
    inputs: usize,
    points: Vec<ControlPoints>,
    inputs_used: usize,
}

impl Mapping {
    /// Create a new mapping for a brush setting with `n_inputs` possible inputs.
    pub fn new(n_inputs: usize) -> Self {
        Self {
            base_value: 0.0,
            inputs: n_inputs,
            points: vec![ControlPoints::new(); n_inputs],
            inputs_used: 0,
        }
    }

    /// Reset to all-zero, `n_inputs`-wide mapping.
    pub fn reset(&mut self) {
        self.base_value = 0.0;
        for cp in self.points.iter_mut() {
            cp.n = 0;
        }
        self.inputs_used = 0;
    }

    // ── base value ────────────────────────────────────────────────────────────

    #[inline]
    pub fn get_base_value(&self) -> f32 {
        self.base_value
    }

    #[inline]
    pub fn set_base_value(&mut self, value: f32) {
        self.base_value = value;
    }

    // ── curve shape ──────────────────────────────────────────────────────────

    /// Set the number of control points for `input`'s curve.
    /// `n == 0` disables the input; `n == 1` is forbidden (need ≥ 2 for interpolation).
    pub fn set_n(&mut self, input: usize, n: usize) {
        assert!(input < self.inputs, "input index out of range");
        assert!(n <= MAX_POINTS, "n exceeds MAX_POINTS");
        assert!(n != 1, "n=1 is invalid (need 0 or ≥2)");

        let was_active = self.points[input].n != 0;
        let will_be_active = n != 0;

        if !was_active && will_be_active {
            self.inputs_used += 1;
        } else if was_active && !will_be_active {
            self.inputs_used -= 1;
        }

        self.points[input].n = n;
    }

    pub fn get_n(&self, input: usize) -> usize {
        assert!(input < self.inputs);
        self.points[input].n
    }

    /// Set the (x, y) coordinates of control point `index` for `input`.
    /// Points must be set in ascending x order.
    pub fn set_point(&mut self, input: usize, index: usize, x: f32, y: f32) {
        assert!(input < self.inputs);
        let cp = &mut self.points[input];
        assert!(index < cp.n, "index {} >= n {}", index, cp.n);
        if index > 0 {
            assert!(
                x >= cp.x[index - 1],
                "x values must be non-decreasing: {} < {}",
                x,
                cp.x[index - 1]
            );
        }
        cp.x[index] = x;
        cp.y[index] = y;
    }

    pub fn get_point(&self, input: usize, index: usize) -> (f32, f32) {
        assert!(input < self.inputs);
        let cp = &self.points[input];
        assert!(index < cp.n);
        (cp.x[index], cp.y[index])
    }

    // ── queries ───────────────────────────────────────────────────────────────

    /// Returns true if no input curves are active (base_value only).
    #[inline]
    pub fn is_constant(&self) -> bool {
        self.inputs_used == 0
    }

    /// Number of inputs that have an active curve.
    #[inline]
    pub fn inputs_used_n(&self) -> usize {
        self.inputs_used
    }

    // ── evaluation ────────────────────────────────────────────────────────────

    /// Evaluate the mapping given a full input array.
    /// `data[i]` is the current value of input channel `i`.
    pub fn calculate(&self, data: &[f32]) -> f32 {
        debug_assert!(data.len() >= self.inputs);

        if self.inputs_used == 0 {
            return self.base_value;
        }

        let mut result = self.base_value;
        for (i, cp) in self.points.iter().enumerate() {
            if cp.n >= 2 {
                result += cp.evaluate(data[i]);
            }
        }
        result
    }

    /// Convenience: evaluate with a single-input mapping.
    pub fn calculate_single_input(&self, input: f32) -> f32 {
        assert_eq!(self.inputs, 1, "calculate_single_input requires inputs==1");
        self.calculate(&[input])
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_linear(input: usize, x0: f32, y0: f32, x1: f32, y1: f32) -> Mapping {
        let mut m = Mapping::new(input + 1);
        m.set_n(input, 2);
        m.set_point(input, 0, x0, y0);
        m.set_point(input, 1, x1, y1);
        m
    }

    #[test]
    fn test_constant_mapping() {
        let mut m = Mapping::new(3);
        m.set_base_value(1.5);
        assert!(m.is_constant());
        assert_eq!(m.calculate(&[0.0, 0.0, 0.0]), 1.5);
    }

    #[test]
    fn test_linear_curve_midpoint() {
        // curve: (0,0)→(1,1), evaluate at 0.5 → 0.5
        let m = make_linear(0, 0.0, 0.0, 1.0, 1.0);
        let v = m.calculate(&[0.5]);
        assert!((v - 0.5).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn test_linear_curve_extrapolation_left() {
        // x < x0：向左线性外推，不夹紧（与 C 原版行为一致）
        // 曲线 (0,2)→(1,4)，斜率=2，x=-1 → y = 2 + (-1)*2 = 0
        let m = make_linear(0, 0.0, 2.0, 1.0, 4.0);
        let v = m.calculate(&[-1.0]);
        assert!((v - 0.0).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn test_linear_curve_extrapolation_right() {
        // x > x1：向右线性外推，不夹紧（与 C 原版行为一致）
        // 曲线 (0,0)→(1,1)，斜率=1，x=2 → y = 2
        let m = make_linear(0, 0.0, 0.0, 1.0, 1.0);
        let v = m.calculate(&[2.0]);
        assert!((v - 2.0).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn test_flat_curve_returns_y0() {
        // y0 == y1: should always return y0
        let m = make_linear(0, 0.0, 3.0, 1.0, 3.0);
        assert!((m.calculate(&[0.5]) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_multipoint_curve() {
        // three points: (0,0)→(1,1)→(2,0)
        let mut m = Mapping::new(1);
        m.set_n(0, 3);
        m.set_point(0, 0, 0.0, 0.0);
        m.set_point(0, 1, 1.0, 1.0);
        m.set_point(0, 2, 2.0, 0.0);

        assert!((m.calculate(&[0.5]) - 0.5).abs() < 1e-5);
        assert!((m.calculate(&[1.0]) - 1.0).abs() < 1e-5);
        assert!((m.calculate(&[1.5]) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_multi_input_additive() {
        // two inputs, both with linear curves
        let mut m = Mapping::new(2);
        m.set_base_value(1.0);
        // input 0: (0,0)→(1,1)
        m.set_n(0, 2);
        m.set_point(0, 0, 0.0, 0.0);
        m.set_point(0, 1, 1.0, 1.0);
        // input 1: (0,0)→(1,0.5)
        m.set_n(1, 2);
        m.set_point(1, 0, 0.0, 0.0);
        m.set_point(1, 1, 1.0, 0.5);

        let v = m.calculate(&[0.5, 1.0]);
        // base=1.0, curve0(0.5)=0.5, curve1(1.0)=0.5 → 2.0
        assert!((v - 2.0).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn test_inputs_used_tracking() {
        let mut m = Mapping::new(3);
        assert_eq!(m.inputs_used_n(), 0);
        m.set_n(0, 2);
        assert_eq!(m.inputs_used_n(), 1);
        m.set_n(2, 2);
        assert_eq!(m.inputs_used_n(), 2);
        m.set_n(0, 0);
        assert_eq!(m.inputs_used_n(), 1);
        assert!(m.is_constant() == false);
        m.set_n(2, 0);
        assert!(m.is_constant());
    }

    #[test]
    fn test_calculate_single_input() {
        let mut m = Mapping::new(1);
        m.set_base_value(0.0);
        m.set_n(0, 2);
        m.set_point(0, 0, 0.0, 0.0);
        m.set_point(0, 1, 2.0, 4.0);
        let v = m.calculate_single_input(1.0);
        assert!((v - 2.0).abs() < 1e-6, "got {v}");
    }
}
