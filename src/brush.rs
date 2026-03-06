// mypaint-rs: brush.rs
// Rewrite of libmypaint/mypaint-brush.c
// Original Copyright (C) 2007-2011 Martin Renold <martinxyz@gmx.ch>
// Licensed under ISC License

use std::f32::consts::PI;

use crate::brush_settings::generated::{
    BRUSH_INPUTS_COUNT, BRUSH_SETTING_INFO, BRUSH_SETTINGS_COUNT, BRUSH_STATES_COUNT, BrushInput,
    BrushSetting, BrushState,
};
use crate::helpers::{
    hsl_to_rgb, hsv_to_rgb, mix_colors, mod_arith, rgb_to_hsl, rgb_to_hsv,
    smallest_angular_difference,
};
use crate::mapping::Mapping;
use crate::rng_double::RngDouble;

// ── Constants ─────────────────────────────────────────────────────────────────

const ACTUAL_RADIUS_MIN: f32 = 0.2;
const ACTUAL_RADIUS_MAX: f32 = 1000.0;
const GRID_SIZE: f32 = 256.0;
/// Size of one smudge bucket (r, g, b, a, prev_r, prev_g, prev_b, prev_a, recentness).
const SMUDGE_BUCKET_SIZE: usize = 9;

// Named indices within a smudge bucket
const SMUDGE_R: usize = 0;
const SMUDGE_G: usize = 1;
const SMUDGE_B: usize = 2;
const SMUDGE_A: usize = 3;
const PREV_COL_R: usize = 4;
const PREV_COL_G: usize = 5;
const PREV_COL_B: usize = 6;
const PREV_COL_A: usize = 7;
const PREV_COL_RECENTNESS: usize = 8;

const WGM_EPSILON: f32 = 1e-6;

fn radians(deg: f32) -> f32 {
    deg * PI / 180.0
}
fn degrees(rad: f32) -> f32 {
    rad / (2.0 * PI) * 360.0
}

// ── Surface trait ─────────────────────────────────────────────────────────────

/// Trait representing the paint surface the brush draws on.
/// Implement this to connect the brush engine to your rendering backend.
pub trait Surface {
    /// Draw one dab onto the surface.
    #[allow(clippy::too_many_arguments)]
    fn draw_dab(
        &mut self,
        x: f32,
        y: f32,
        radius: f32,
        color_r: f32,
        color_g: f32,
        color_b: f32,
        opaque: f32,
        hardness: f32,
        softness: f32,
        eraser_target_alpha: f32,
        aspect_ratio: f32,
        angle: f32,
        lock_alpha: f32,
        colorize: f32,
        posterize: f32,
        posterize_num: f32,
        paint_mode: f32,
    ) -> bool;

    /// Sample the average color in a circle of `radius` around `(x, y)`.
    /// `paint_mode` < 0 selects legacy (additive) sampling.
    fn get_color(&mut self, x: f32, y: f32, radius: f32, paint_mode: f32) -> (f32, f32, f32, f32); // (r, g, b, a)
}

// ── Smudge bucket ─────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct SmudgeBucket {
    data: [f32; SMUDGE_BUCKET_SIZE],
}

impl SmudgeBucket {
    fn clear(&mut self) {
        self.data = [0.0; SMUDGE_BUCKET_SIZE];
    }
}

// ── Speed mapping cache ───────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct SpeedMappingCache {
    gamma: f32,
    m: f32,
    q: f32,
}

// ── Brush struct ──────────────────────────────────────────────────────────────

pub struct Brush {
    pub print_inputs: bool,

    // stroke timing (for undo/redo split)
    stroke_total_painting_time: f64,
    stroke_current_idling_time: f64,

    // per-dab state
    states: [f32; BRUSH_STATES_COUNT],

    // smudge buckets (index 0 = inline bucket used when num_buckets==0)
    inline_bucket: SmudgeBucket,
    smudge_buckets: Vec<SmudgeBucket>,
    min_bucket_used: Option<usize>,
    max_bucket_used: Option<usize>,

    // current random value fed as an input
    random_input: f64,

    // tracking noise skip
    skip: f32,
    skip_last_x: f32,
    skip_last_y: f32,
    skipped_dtime: f64,

    // per-setting mappings
    settings: Vec<Mapping>,

    // cached evaluated setting values (recalculated each simulation step)
    settings_value: [f32; BRUSH_SETTINGS_COUNT],

    // speed mapping precomputed params (index 0=fine, 1=gross)
    speed_cache: [SpeedMappingCache; 2],

    reset_requested: bool,

    rng: RngDouble,
}

impl Brush {
    // ── Constructors ─────────────────────────────────────────────────────────

    pub fn new() -> Self {
        Self::with_buckets(0)
    }

    pub fn with_buckets(num_smudge_buckets: usize) -> Self {
        let mut b = Self {
            print_inputs: false,
            stroke_total_painting_time: 0.0,
            stroke_current_idling_time: 0.0,
            states: [0.0; BRUSH_STATES_COUNT],
            inline_bucket: SmudgeBucket::default(),
            smudge_buckets: vec![SmudgeBucket::default(); num_smudge_buckets],
            min_bucket_used: None,
            max_bucket_used: None,
            random_input: 0.0,
            skip: 0.0,
            skip_last_x: 0.0,
            skip_last_y: 0.0,
            skipped_dtime: 0.0,
            settings: (0..BRUSH_SETTINGS_COUNT)
                .map(|_| Mapping::new(BRUSH_INPUTS_COUNT))
                .collect(),
            settings_value: [0.0; BRUSH_SETTINGS_COUNT],
            speed_cache: Default::default(),
            reset_requested: true,
            rng: RngDouble::new(1000),
        };

        b.brush_reset();
        b.new_stroke();
        b.settings_base_values_changed();
        b
    }

    // ── Stroke lifecycle ──────────────────────────────────────────────────────

    /// Queue a reset; will take effect on next `stroke_to`.
    pub fn reset(&mut self) {
        self.reset_requested = true;
    }

    /// Start a new stroke (resets timing counters).
    pub fn new_stroke(&mut self) {
        self.stroke_current_idling_time = 0.0;
        self.stroke_total_painting_time = 0.0;
    }

    /// Internal full reset of all state.
    fn brush_reset(&mut self) {
        self.skip = 0.0;
        self.skip_last_x = 0.0;
        self.skip_last_y = 0.0;
        self.skipped_dtime = 0.0;
        self.states = [0.0; BRUSH_STATES_COUNT];
        // FLIP starts at -1 so the first dab gets +1
        self.set_state(BrushState::Flip, -1.0);
        // clear smudge buckets
        for b in self.smudge_buckets.iter_mut() {
            b.clear();
        }
        self.inline_bucket.clear();
        self.min_bucket_used = None;
        self.max_bucket_used = None;
    }

    // ── Setting accessors ─────────────────────────────────────────────────────

    pub fn set_base_value(&mut self, id: BrushSetting, value: f32) {
        self.settings[id as usize].set_base_value(value);
        self.settings_base_values_changed();
    }

    pub fn get_base_value(&self, id: BrushSetting) -> f32 {
        self.settings[id as usize].get_base_value()
    }

    pub fn is_constant(&self, id: BrushSetting) -> bool {
        self.settings[id as usize].is_constant()
    }

    pub fn get_inputs_used_n(&self, id: BrushSetting) -> usize {
        self.settings[id as usize].inputs_used_n()
    }

    pub fn set_mapping_n(&mut self, id: BrushSetting, input: BrushInput, n: usize) {
        self.settings[id as usize].set_n(input as usize, n);
    }

    pub fn get_mapping_n(&self, id: BrushSetting, input: BrushInput) -> usize {
        self.settings[id as usize].get_n(input as usize)
    }

    pub fn set_mapping_point(
        &mut self,
        id: BrushSetting,
        input: BrushInput,
        index: usize,
        x: f32,
        y: f32,
    ) {
        self.settings[id as usize].set_point(input as usize, index, x, y);
    }

    pub fn get_mapping_point(
        &self,
        id: BrushSetting,
        input: BrushInput,
        index: usize,
    ) -> (f32, f32) {
        self.settings[id as usize].get_point(input as usize, index)
    }

    // ── State accessors ───────────────────────────────────────────────────────

    pub fn get_state(&self, i: BrushState) -> f32 {
        self.states[i as usize]
    }

    pub fn set_state(&mut self, i: BrushState, value: f32) {
        self.states[i as usize] = value;
    }

    #[inline]
    fn state(&self, i: BrushState) -> f32 {
        self.states[i as usize]
    }
    #[inline]
    fn state_mut(&mut self, i: BrushState) -> &mut f32 {
        &mut self.states[i as usize]
    }
    #[inline]
    fn setting(&self, i: BrushSetting) -> f32 {
        self.settings_value[i as usize]
    }
    #[inline]
    fn baseval(&self, i: BrushSetting) -> f32 {
        self.settings[i as usize].get_base_value()
    }

    // ── Smudge bucket accessors ───────────────────────────────────────────────

    pub fn get_total_stroke_painting_time(&self) -> f64 {
        self.stroke_total_painting_time
    }

    pub fn set_smudge_bucket(
        &mut self,
        index: usize,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
        prev_r: f32,
        prev_g: f32,
        prev_b: f32,
        prev_a: f32,
        recentness: f32,
    ) -> bool {
        if index >= self.smudge_buckets.len() {
            return false;
        }
        let d = &mut self.smudge_buckets[index].data;
        d[SMUDGE_R] = r;
        d[SMUDGE_G] = g;
        d[SMUDGE_B] = b;
        d[SMUDGE_A] = a;
        d[PREV_COL_R] = prev_r;
        d[PREV_COL_G] = prev_g;
        d[PREV_COL_B] = prev_b;
        d[PREV_COL_A] = prev_a;
        d[PREV_COL_RECENTNESS] = recentness;
        true
    }

    pub fn get_smudge_bucket(&self, index: usize) -> Option<[f32; SMUDGE_BUCKET_SIZE]> {
        if index >= self.smudge_buckets.len() {
            return None;
        }
        Some(self.smudge_buckets[index].data)
    }

    pub fn min_smudge_bucket_used(&self) -> Option<usize> {
        self.min_bucket_used
    }
    pub fn max_smudge_bucket_used(&self) -> Option<usize> {
        self.max_bucket_used
    }

    // ── Defaults ──────────────────────────────────────────────────────────────

    /// Reset all settings to their default values, with standard pressure mapping.
    pub fn from_defaults(&mut self) {
        for s in 0..BRUSH_SETTINGS_COUNT {
            for i in 0..BRUSH_INPUTS_COUNT {
                self.settings[s].set_n(i, 0);
            }
            let def = BRUSH_SETTING_INFO[s].default;
            self.settings[s].set_base_value(def);
        }
        // standard pressure→opacity curve
        let s = BrushSetting::OpaqueMultiply as usize;
        let i = BrushInput::Pressure as usize;
        self.settings[s].set_n(i, 2);
        self.settings[s].set_point(i, 0, 0.0, 0.0);
        self.settings[s].set_point(i, 1, 1.0, 1.0);

        self.settings_base_values_changed();
    }

    // ── JSON deserialization ──────────────────────────────────────────────────

    /// Load brush settings from a MyPaint v3 JSON string.
    pub fn from_string(&mut self, json_str: &str) -> Result<(), String> {
        let v: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("JSON parse error: {e}"))?;

        let version = v["version"].as_i64().ok_or("Missing 'version' field")?;
        if version != 3 {
            return Err(format!("Unsupported brush version: {version}"));
        }

        let settings = v["settings"]
            .as_object()
            .ok_or("Missing 'settings' field")?;

        for (setting_name, setting_obj) in settings {
            if let Err(e) = self.load_setting_from_json(setting_name, setting_obj) {
                eprintln!("Warning: {e}");
            }
        }

        self.settings_base_values_changed();
        Ok(())
    }

    fn load_setting_from_json(
        &mut self,
        name: &str,
        obj: &serde_json::Value,
    ) -> Result<(), String> {
        use crate::brush_settings::generated::BrushSetting;

        let id =
            BrushSetting::from_cname(name).ok_or_else(|| format!("Unknown setting: {name}"))?;

        let base = obj["base_value"]
            .as_f64()
            .ok_or_else(|| format!("No base_value for {name}"))? as f32;
        self.settings[id as usize].set_base_value(base);

        let inputs = obj["inputs"]
            .as_object()
            .ok_or_else(|| format!("No inputs for {name}"))?;

        for (input_name, input_arr) in inputs {
            use crate::brush_settings::generated::BrushInput;

            let input_id = BrushInput::from_cname(input_name)
                .ok_or_else(|| format!("Unknown input: {input_name}"))?;

            let points = input_arr
                .as_array()
                .ok_or_else(|| format!("inputs.{input_name} is not an array"))?;

            let n = points.len();
            self.settings[id as usize].set_n(input_id as usize, n);

            for (i, pt) in points.iter().enumerate() {
                let arr = pt
                    .as_array()
                    .ok_or_else(|| format!("point is not an array"))?;
                let x = arr[0].as_f64().unwrap_or(0.0) as f32;
                let y = arr[1].as_f64().unwrap_or(0.0) as f32;
                self.settings[id as usize].set_point(input_id as usize, i, x, y);
            }
        }

        Ok(())
    }

    // ── Internal: speed cache ─────────────────────────────────────────────────

    fn settings_base_values_changed(&mut self) {
        // Precompute the speed→input mapping:
        //   y = log(gamma + x) * m + q
        // fixed constraints:
        //   fix1: (45, 0.5)  fix2: at x=45, dy/dx = 0.015
        for i in 0..2 {
            let gamma_setting = if i == 0 {
                BrushSetting::Speed1Gamma
            } else {
                BrushSetting::Speed2Gamma
            };
            let gamma = self.baseval(gamma_setting).exp();
            let fix1_x = 45.0_f32;
            let fix1_y = 0.5_f32;
            let fix2_dy = 0.015_f32;

            let c1 = (fix1_x + gamma).ln();
            let m = fix2_dy * (fix1_x + gamma);
            let q = fix1_y - m * c1;

            self.speed_cache[i] = SpeedMappingCache { gamma, m, q };
        }
    }

    // ── exp_decay helper ──────────────────────────────────────────────────────

    #[inline]
    fn exp_decay(t_const: f32, t: f32) -> f32 {
        if t_const <= 0.001 {
            0.0
        } else {
            (-t / t_const).exp()
        }
    }

    // ── Directional offsets ───────────────────────────────────────────────────

    fn directional_offsets(&self, base_radius: f32, flip: i32) -> (f32, f32) {
        let offset_mult = self.setting(BrushSetting::OffsetMultiplier).exp();
        if !offset_mult.is_finite() {
            return (0.0, 0.0);
        }

        let mut dx = self.setting(BrushSetting::OffsetX);
        let mut dy = self.setting(BrushSetting::OffsetY);

        let adj = self.setting(BrushSetting::OffsetAngleAdj);
        let dir_dy = self.state(BrushState::DirectionAngleDy);
        let dir_dx = self.state(BrushState::DirectionAngleDx);
        let angle_deg = (degrees(dir_dy.atan2(dir_dx)) - 90.0).rem_euclid(360.0);

        let off_angle = self.setting(BrushSetting::OffsetAngle);
        if off_angle != 0.0 {
            let a = radians(angle_deg + adj);
            dx += a.cos() * off_angle;
            dy += a.sin() * off_angle;
        }

        let view_rot = self.state(BrushState::Viewrotation);

        let off_asc = self.setting(BrushSetting::OffsetAngleAsc);
        if off_asc != 0.0 {
            let asc = self.state(BrushState::Ascension);
            let a = radians(asc - view_rot + adj);
            dx += a.cos() * off_asc;
            dy += a.sin() * off_asc;
        }

        let off_view = self.setting(BrushSetting::OffsetAngleView);
        if off_view != 0.0 {
            let a = radians(view_rot + adj);
            dx += (-a).cos() * off_view;
            dy += (-a).sin() * off_view;
        }

        let off_dir_mirror = self.setting(BrushSetting::OffsetAngle2).max(0.0);
        if off_dir_mirror != 0.0 {
            let a = radians(angle_deg + adj * flip as f32);
            dx += a.cos() * off_dir_mirror * flip as f32;
            dy += a.sin() * off_dir_mirror * flip as f32;
        }

        let off_asc_mirror = self.setting(BrushSetting::OffsetAngle2Asc).max(0.0);
        if off_asc_mirror != 0.0 {
            let asc = self.state(BrushState::Ascension);
            let a = radians(asc - view_rot + adj * flip as f32);
            dx += a.cos() * off_asc_mirror * flip as f32;
            dy += a.sin() * off_asc_mirror * flip as f32;
        }

        let off_view_mirror = self.setting(BrushSetting::OffsetAngle2View).max(0.0);
        if off_view_mirror != 0.0 {
            let a = radians(view_rot + adj);
            dx += (-a).cos() * off_view_mirror * flip as f32;
            dy += (-a).sin() * off_view_mirror * flip as f32;
        }

        let lim = 3240.0_f32;
        let base_mul = base_radius * offset_mult;
        (
            (dx * base_mul).clamp(-lim, lim),
            (dy * base_mul).clamp(-lim, lim),
        )
    }

    // ── update_states_and_setting_values ─────────────────────────────────────

    fn update_states_and_setting_values(
        &mut self,
        step_ddab: f32,
        step_dx: f32,
        step_dy: f32,
        step_dpressure: f32,
        step_declination: f32,
        step_ascension: f32,
        step_dtime: f32,
        viewzoom: f32,
        viewrotation: f32,
        step_declinationx: f32,
        step_declinationy: f32,
        step_barrel_rotation: f32,
    ) {
        let step_dtime = if step_dtime <= 0.0 { 0.001 } else { step_dtime };

        *self.state_mut(BrushState::X) += step_dx;
        *self.state_mut(BrushState::Y) += step_dy;
        *self.state_mut(BrushState::Pressure) += step_dpressure;
        *self.state_mut(BrushState::Declination) += step_declination;
        *self.state_mut(BrushState::Ascension) += step_ascension;
        *self.state_mut(BrushState::Declinationx) += step_declinationx;
        *self.state_mut(BrushState::Declinationy) += step_declinationy;
        *self.state_mut(BrushState::Viewzoom) = viewzoom;

        let viewrotation_deg = mod_arith(degrees(viewrotation) + 180.0, 360.0) - 180.0;
        *self.state_mut(BrushState::Viewrotation) = viewrotation_deg;

        // gridmap
        {
            let x = self.state(BrushState::ActualX);
            let y = self.state(BrushState::ActualY);
            let scale = self.setting(BrushSetting::GridmapScale).exp();
            let scale_x = self.setting(BrushSetting::GridmapScaleX);
            let scale_y = self.setting(BrushSetting::GridmapScaleY);
            let scaled_size = scale * GRID_SIZE;
            let mut gx = mod_arith(x.abs() * scale_x, scaled_size) / scaled_size * GRID_SIZE;
            let mut gy = mod_arith(y.abs() * scale_y, scaled_size) / scaled_size * GRID_SIZE;
            if x < 0.0 {
                gx = GRID_SIZE - gx;
            }
            if y < 0.0 {
                gy = GRID_SIZE - gy;
            }
            *self.state_mut(BrushState::GridmapX) = gx;
            *self.state_mut(BrushState::GridmapY) = gy;
        }

        let base_radius = self.baseval(BrushSetting::RadiusLogarithmic).exp();
        *self.state_mut(BrushState::BarrelRotation) += step_barrel_rotation;

        if self.state(BrushState::Pressure) < 0.0 {
            *self.state_mut(BrushState::Pressure) = 0.0;
        }
        let pressure = self.state(BrushState::Pressure);

        // stroke start/end
        {
            let lim = 0.0001_f32;
            let threshold = self.baseval(BrushSetting::StrokeThreshold);
            let started = self.state(BrushState::StrokeStarted) != 0.0;
            if !started && pressure > threshold + lim {
                *self.state_mut(BrushState::StrokeStarted) = 1.0;
                *self.state_mut(BrushState::Stroke) = 0.0;
            } else if started && pressure <= threshold * 0.9 + lim {
                *self.state_mut(BrushState::StrokeStarted) = 0.0;
            }
        }

        let norm_dx = step_dx / step_dtime * self.state(BrushState::Viewzoom);
        let norm_dy = step_dy / step_dtime * self.state(BrushState::Viewzoom);
        let norm_speed = norm_dx.hypot(norm_dy);
        let norm_dist = (step_dx / step_dtime / base_radius)
            .hypot(step_dy / step_dtime / base_radius)
            * step_dtime;

        // build inputs array
        let mut inputs = [0.0_f32; BRUSH_INPUTS_COUNT];

        inputs[BrushInput::Pressure as usize] =
            pressure * self.baseval(BrushSetting::PressureGainLog).exp();

        let sc0 = &self.speed_cache[0];
        let (m0, q0, g0) = (sc0.m, sc0.q, sc0.gamma);
        let sc1 = &self.speed_cache[1];
        let (m1, q1, g1) = (sc1.m, sc1.q, sc1.gamma);
        inputs[BrushInput::Speed1 as usize] =
            (g0 + self.state(BrushState::NormSpeed1Slow)).ln() * m0 + q0;
        inputs[BrushInput::Speed2 as usize] =
            (g1 + self.state(BrushState::NormSpeed2Slow)).ln() * m1 + q1;

        inputs[BrushInput::Random as usize] = self.random_input as f32;
        inputs[BrushInput::Stroke as usize] = self.state(BrushState::Stroke).min(1.0);

        let dir_angle = self
            .state(BrushState::DirectionDy)
            .atan2(self.state(BrushState::DirectionDx));
        inputs[BrushInput::Direction as usize] =
            mod_arith(degrees(dir_angle) + viewrotation_deg + 180.0, 180.0);

        let dir_angle_360 = self
            .state(BrushState::DirectionAngleDy)
            .atan2(self.state(BrushState::DirectionAngleDx));
        inputs[BrushInput::DirectionAngle as usize] =
            (degrees(dir_angle_360) + viewrotation_deg + 360.0).rem_euclid(360.0);

        inputs[BrushInput::TiltDeclination as usize] = self.state(BrushState::Declination);
        inputs[BrushInput::TiltAscension as usize] = mod_arith(
            self.state(BrushState::Ascension) + viewrotation_deg + 180.0,
            360.0,
        ) - 180.0;
        inputs[BrushInput::Viewzoom as usize] = self.baseval(BrushSetting::RadiusLogarithmic)
            - (base_radius / self.state(BrushState::Viewzoom)).ln();
        inputs[BrushInput::AttackAngle as usize] = smallest_angular_difference(
            self.state(BrushState::Ascension),
            mod_arith(degrees(dir_angle_360) + 90.0, 360.0),
        );
        inputs[BrushInput::BrushRadius as usize] = self.baseval(BrushSetting::RadiusLogarithmic);
        inputs[BrushInput::GridmapX as usize] =
            self.state(BrushState::GridmapX).clamp(0.0, GRID_SIZE);
        inputs[BrushInput::GridmapY as usize] =
            self.state(BrushState::GridmapY).clamp(0.0, GRID_SIZE);
        inputs[BrushInput::TiltDeclinationx as usize] = self.state(BrushState::Declinationx);
        inputs[BrushInput::TiltDeclinationy as usize] = self.state(BrushState::Declinationy);
        inputs[BrushInput::Custom as usize] = self.state(BrushState::CustomInput);
        inputs[BrushInput::BarrelRotation as usize] =
            mod_arith(self.state(BrushState::BarrelRotation), 360.0);

        if self.print_inputs {
            eprintln!(
                "press={:.3} speed1={:.4} speed2={:.4} stroke={:.3} custom={:.3}",
                inputs[BrushInput::Pressure as usize],
                inputs[BrushInput::Speed1 as usize],
                inputs[BrushInput::Speed2 as usize],
                inputs[BrushInput::Stroke as usize],
                inputs[BrushInput::Custom as usize],
            );
        }

        // evaluate all setting mappings
        for i in 0..BRUSH_SETTINGS_COUNT {
            self.settings_value[i] = self.settings[i].calculate(&inputs);
        }

        *self.state_mut(BrushState::DabsPerBasicRadius) =
            self.setting(BrushSetting::DabsPerBasicRadius);
        *self.state_mut(BrushState::DabsPerActualRadius) =
            self.setting(BrushSetting::DabsPerActualRadius);
        *self.state_mut(BrushState::DabsPerSecond) = self.setting(BrushSetting::DabsPerSecond);

        // actual position tracking (slow tracking per dab)
        {
            let fac =
                1.0 - Self::exp_decay(self.setting(BrushSetting::SlowTrackingPerDab), step_ddab);
            let (x, y) = (self.state(BrushState::X), self.state(BrushState::Y));
            *self.state_mut(BrushState::ActualX) += (x - self.state(BrushState::ActualX)) * fac;
            *self.state_mut(BrushState::ActualY) += (y - self.state(BrushState::ActualY)) * fac;
        }

        // slow speed
        {
            let fac1 =
                1.0 - Self::exp_decay(self.setting(BrushSetting::Speed1Slowness), step_dtime);
            *self.state_mut(BrushState::NormSpeed1Slow) +=
                (norm_speed - self.state(BrushState::NormSpeed1Slow)) * fac1;
            let fac2 =
                1.0 - Self::exp_decay(self.setting(BrushSetting::Speed2Slowness), step_dtime);
            *self.state_mut(BrushState::NormSpeed2Slow) +=
                (norm_speed - self.state(BrushState::NormSpeed2Slow)) * fac2;
        }

        // slow speed as vector
        {
            let mut tc = (self.setting(BrushSetting::OffsetBySpeedSlowness) * 0.01).exp() - 1.0;
            if tc < 0.002 {
                tc = 0.002;
            }
            let fac = 1.0 - Self::exp_decay(tc, step_dtime);
            *self.state_mut(BrushState::NormDxSlow) +=
                (norm_dx - self.state(BrushState::NormDxSlow)) * fac;
            *self.state_mut(BrushState::NormDySlow) +=
                (norm_dy - self.state(BrushState::NormDySlow)) * fac;
        }

        // direction low-pass
        {
            let vz = self.state(BrushState::Viewzoom);
            let dx = step_dx * vz;
            let dy = step_dy * vz;
            let step_in_dabtime = dx.hypot(dy);
            let fac = 1.0
                - Self::exp_decay(
                    (self.setting(BrushSetting::DirectionFilter) * 0.5).exp() - 1.0,
                    step_in_dabtime,
                );

            let dx_old = self.state(BrushState::DirectionDx);
            let dy_old = self.state(BrushState::DirectionDy);

            *self.state_mut(BrushState::DirectionAngleDx) +=
                (dx - self.state(BrushState::DirectionAngleDx)) * fac;
            *self.state_mut(BrushState::DirectionAngleDy) +=
                (dy - self.state(BrushState::DirectionAngleDy)) * fac;

            // use opposite direction if closer (ignore 180° turns)
            let (dx, dy) = if (dx_old - dx).powi(2) + (dy_old - dy).powi(2)
                > (dx_old - (-dx)).powi(2) + (dy_old - (-dy)).powi(2)
            {
                (-dx, -dy)
            } else {
                (dx, dy)
            };
            *self.state_mut(BrushState::DirectionDx) +=
                (dx - self.state(BrushState::DirectionDx)) * fac;
            *self.state_mut(BrushState::DirectionDy) +=
                (dy - self.state(BrushState::DirectionDy)) * fac;
        }

        // custom input
        {
            let fac = 1.0 - Self::exp_decay(self.setting(BrushSetting::CustomInputSlowness), 0.1);
            let target = self.setting(BrushSetting::CustomInput);
            *self.state_mut(BrushState::CustomInput) +=
                (target - self.state(BrushState::CustomInput)) * fac;
        }

        // stroke length
        {
            let freq = (-self.setting(BrushSetting::StrokeDurationLogarithmic)).exp();
            let stroke = (self.state(BrushState::Stroke) + norm_dist * freq).max(0.0);
            let wrap = 1.0 + self.setting(BrushSetting::StrokeHoldtime).max(0.0);
            *self.state_mut(BrushState::Stroke) = if stroke >= wrap && wrap > 10.9 {
                1.0
            } else if stroke >= wrap {
                stroke.rem_euclid(wrap)
            } else {
                stroke
            };
        }

        // actual radius
        {
            let r = self.setting(BrushSetting::RadiusLogarithmic).exp();
            *self.state_mut(BrushState::ActualRadius) =
                r.clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);
        }

        // elliptical dab
        *self.state_mut(BrushState::ActualEllipticalDabRatio) =
            self.setting(BrushSetting::EllipticalDabRatio);
        *self.state_mut(BrushState::ActualEllipticalDabAngle) = mod_arith(
            self.setting(BrushSetting::EllipticalDabAngle) - viewrotation_deg + 180.0,
            180.0,
        ) - 180.0;
    }

    // ── Smudge bucket fetch ───────────────────────────────────────────────────

    fn fetch_smudge_bucket_index(&mut self) -> Option<usize> {
        if self.smudge_buckets.is_empty() {
            return None; // use inline bucket
        }
        let n = self.smudge_buckets.len();
        let idx = (self.setting(BrushSetting::SmudgeBucket).round() as usize).clamp(0, n - 1);
        match self.min_bucket_used {
            None => {
                self.min_bucket_used = Some(idx);
                self.max_bucket_used = Some(idx);
            }
            Some(min) => {
                if idx < min {
                    self.min_bucket_used = Some(idx);
                }
                if idx > self.max_bucket_used.unwrap_or(0) {
                    self.max_bucket_used = Some(idx);
                }
            }
        }
        Some(idx)
    }

    // ── update_smudge_color ───────────────────────────────────────────────────

    /// Returns `true` if the caller should return early (skip drawing).
    fn update_smudge_color<S: Surface>(
        &mut self,
        surface: &mut S,
        bucket_idx: Option<usize>,
        smudge_length: f32,
        px: f32,
        py: f32,
        radius: f32,
        legacy_smudge: bool,
        paint_factor: f32,
    ) -> bool {
        let update_factor = smudge_length.max(0.01);
        let smudge_length_log = self.setting(BrushSetting::SmudgeLengthLog);

        let recentness = {
            let b = self.get_bucket_data_mut(bucket_idx);
            b[PREV_COL_RECENTNESS] *= update_factor;
            b[PREV_COL_RECENTNESS]
        };

        let margin = 1e-16_f32;
        let threshold = (0.5 * update_factor).powf(smudge_length_log) + margin;

        if recentness < threshold.min(1.0) {
            let init = recentness == 0.0;
            let uf = if init { 0.0 } else { update_factor };

            let radius_log = self.setting(BrushSetting::SmudgeRadiusLog);
            let smudge_radius =
                (radius * radius_log.exp()).clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);

            let paint_mode_arg = if legacy_smudge { -1.0 } else { paint_factor };
            let (r, g, b, a) = surface.get_color(px, py, smudge_radius, paint_mode_arg);

            let smudge_op_lim = self.setting(BrushSetting::SmudgeTransparency);
            if (smudge_op_lim > 0.0 && a < smudge_op_lim)
                || (smudge_op_lim < 0.0 && a > -smudge_op_lim)
            {
                return true;
            }

            {
                let b_data = self.get_bucket_data_mut(bucket_idx);
                b_data[PREV_COL_RECENTNESS] = 1.0;
                b_data[PREV_COL_R] = r;
                b_data[PREV_COL_G] = g;
                b_data[PREV_COL_B] = b;
                b_data[PREV_COL_A] = a;
            }

            // update smudge color
            if legacy_smudge {
                let bd = self.get_bucket_data_mut(bucket_idx);
                let fac_old = uf;
                let fac_new = (1.0 - uf) * a;
                bd[SMUDGE_R] = fac_old * bd[SMUDGE_R] + fac_new * r;
                bd[SMUDGE_G] = fac_old * bd[SMUDGE_G] + fac_new * g;
                bd[SMUDGE_B] = fac_old * bd[SMUDGE_B] + fac_new * b;
                bd[SMUDGE_A] = (fac_old * bd[SMUDGE_A] + fac_new).clamp(0.0, 1.0);
            } else if a > WGM_EPSILON * 10.0 {
                let prev = {
                    let bd = self.get_bucket_data(bucket_idx);
                    [bd[SMUDGE_R], bd[SMUDGE_G], bd[SMUDGE_B], bd[SMUDGE_A]]
                };
                let sampled = [r, g, b, a];
                let mixed = mix_colors(&prev, &sampled, uf, paint_factor);
                let bd = self.get_bucket_data_mut(bucket_idx);
                bd[SMUDGE_R] = mixed[0];
                bd[SMUDGE_G] = mixed[1];
                bd[SMUDGE_B] = mixed[2];
                bd[SMUDGE_A] = mixed[3];
            } else {
                let bd = self.get_bucket_data_mut(bucket_idx);
                bd[SMUDGE_A] = (bd[SMUDGE_A] + a) / 2.0;
            }
        } else {
            // reuse cached color — nothing to do, bucket already holds it
        }

        false
    }

    fn get_bucket_data(&self, idx: Option<usize>) -> &[f32; SMUDGE_BUCKET_SIZE] {
        match idx {
            None => &self.inline_bucket.data,
            Some(i) => &self.smudge_buckets[i].data,
        }
    }

    fn get_bucket_data_mut(&mut self, idx: Option<usize>) -> &mut [f32; SMUDGE_BUCKET_SIZE] {
        match idx {
            None => &mut self.inline_bucket.data,
            Some(i) => &mut self.smudge_buckets[i].data,
        }
    }

    // ── apply_smudge ─────────────────────────────────────────────────────────

    fn apply_smudge(
        bucket: &[f32; SMUDGE_BUCKET_SIZE],
        smudge_value: f32,
        legacy_smudge: bool,
        paint_factor: f32,
        color_r: &mut f32,
        color_g: &mut f32,
        color_b: &mut f32,
    ) -> f32 {
        let smudge_factor = smudge_value.min(1.0);
        let eraser_target_alpha =
            ((1.0 - smudge_factor) + smudge_factor * bucket[SMUDGE_A]).clamp(0.0, 1.0);

        if eraser_target_alpha > 0.0 {
            if legacy_smudge {
                let cf = 1.0 - smudge_factor;
                *color_r = (smudge_factor * bucket[SMUDGE_R] + cf * *color_r) / eraser_target_alpha;
                *color_g = (smudge_factor * bucket[SMUDGE_G] + cf * *color_g) / eraser_target_alpha;
                *color_b = (smudge_factor * bucket[SMUDGE_B] + cf * *color_b) / eraser_target_alpha;
            } else {
                let smudge_color = [
                    bucket[SMUDGE_R],
                    bucket[SMUDGE_G],
                    bucket[SMUDGE_B],
                    bucket[SMUDGE_A],
                ];
                let brush_color = [*color_r, *color_g, *color_b, 1.0];
                let mixed = mix_colors(&smudge_color, &brush_color, smudge_factor, paint_factor);
                *color_r = mixed[0];
                *color_g = mixed[1];
                *color_b = mixed[2];
            }
        } else {
            // erasing only — debug color
            *color_r = 1.0;
            *color_g = 0.0;
            *color_b = 0.0;
        }

        eraser_target_alpha
    }

    // ── prepare_and_draw_dab ─────────────────────────────────────────────────

    fn prepare_and_draw_dab<S: Surface>(&mut self, surface: &mut S, linear: bool) -> bool {
        // opacity
        let opaque_fac = self.setting(BrushSetting::OpaqueMultiply);
        let mut opaque = self.setting(BrushSetting::Opaque).max(0.0);
        opaque = (opaque * opaque_fac).clamp(0.0, 1.0);

        let opaque_linearize = self.baseval(BrushSetting::OpaqueLinearize);
        if opaque_linearize != 0.0 {
            let mut dabs_per_pixel = (self.state(BrushState::DabsPerActualRadius)
                + self.state(BrushState::DabsPerBasicRadius))
                * 2.0;
            if dabs_per_pixel < 1.0 {
                dabs_per_pixel = 1.0;
            }
            dabs_per_pixel = 1.0 + opaque_linearize * (dabs_per_pixel - 1.0);

            let alpha = opaque;
            let beta = 1.0 - alpha;
            let beta_dab = beta.powf(1.0 / dabs_per_pixel);
            opaque = 1.0 - beta_dab;
        }

        let mut x = self.state(BrushState::ActualX);
        let mut y = self.state(BrushState::ActualY);
        let base_radius = self.baseval(BrushSetting::RadiusLogarithmic).exp();

        let flip = self.state(BrushState::Flip) as i32;
        let (ox, oy) = self.directional_offsets(base_radius, flip);
        x += ox;
        y += oy;

        let view_zoom = self.state(BrushState::Viewzoom);
        let offset_by_speed = self.setting(BrushSetting::OffsetBySpeed);
        if offset_by_speed != 0.0 {
            x += self.state(BrushState::NormDxSlow) * offset_by_speed * 0.1 / view_zoom;
            y += self.state(BrushState::NormDySlow) * offset_by_speed * 0.1 / view_zoom;
        }

        let offset_by_random = self.setting(BrushSetting::OffsetByRandom);
        if offset_by_random != 0.0 {
            let amp = offset_by_random.max(0.0);
            x += rand_gauss(&mut self.rng) * amp * base_radius;
            y += rand_gauss(&mut self.rng) * amp * base_radius;
        }

        let mut radius = self.state(BrushState::ActualRadius);
        let radius_by_random = self.setting(BrushSetting::RadiusByRandom);
        if radius_by_random != 0.0 {
            let noise = rand_gauss(&mut self.rng) * radius_by_random;
            let radius_log = self.setting(BrushSetting::RadiusLogarithmic) + noise;
            let r_new = radius_log.exp().clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);
            let alpha_correction = (self.state(BrushState::ActualRadius) / r_new).powi(2);
            if alpha_correction <= 1.0 {
                opaque *= alpha_correction;
            }
            radius = r_new;
        }

        let paint_factor = self.setting(BrushSetting::PaintMode);
        let paint_setting_constant = self.settings[BrushSetting::PaintMode as usize].is_constant();
        let legacy_smudge = paint_factor <= 0.0 && paint_setting_constant;

        // color
        let mut cr = self.baseval(BrushSetting::ColorH);
        let mut cg = self.baseval(BrushSetting::ColorS);
        let mut cb = self.baseval(BrushSetting::ColorV);
        hsv_to_rgb(&mut cr, &mut cg, &mut cb);

        // smudge color update
        let smudge_length = self.setting(BrushSetting::SmudgeLength);
        if smudge_length < 1.0
            && (self.setting(BrushSetting::Smudge) != 0.0
                || !self.settings[BrushSetting::Smudge as usize].is_constant())
        {
            let bucket_idx = self.fetch_smudge_bucket_index();
            let return_early = self.update_smudge_color(
                surface,
                bucket_idx,
                smudge_length,
                x.round(),
                y.round(),
                radius,
                legacy_smudge,
                paint_factor,
            );
            if return_early {
                return false;
            }
        }

        let mut eraser_target_alpha = 1.0_f32;
        let smudge_value = self.setting(BrushSetting::Smudge);
        if smudge_value > 0.0 {
            let bucket_idx = self.fetch_smudge_bucket_index();
            let bucket = *self.get_bucket_data(bucket_idx);
            eraser_target_alpha = Self::apply_smudge(
                &bucket,
                smudge_value,
                legacy_smudge,
                paint_factor,
                &mut cr,
                &mut cg,
                &mut cb,
            );
        }

        if self.setting(BrushSetting::Eraser) != 0.0 {
            eraser_target_alpha *= 1.0 - self.setting(BrushSetting::Eraser);
        }

        // color dynamics
        let using_hsv = self.setting(BrushSetting::ChangeColorH) != 0.0
            || self.setting(BrushSetting::ChangeColorHsvS) != 0.0
            || self.setting(BrushSetting::ChangeColorV) != 0.0;
        let using_hsl = self.setting(BrushSetting::ChangeColorL) != 0.0
            || self.setting(BrushSetting::ChangeColorHslS) != 0.0;

        if linear && (using_hsv || using_hsl) {
            cr = cr.powf(1.0 / 2.2);
            cg = cg.powf(1.0 / 2.2);
            cb = cb.powf(1.0 / 2.2);
        }

        if using_hsv {
            let (mut h, mut s, mut v) = (cr, cg, cb);
            rgb_to_hsv(&mut h, &mut s, &mut v);
            h += self.setting(BrushSetting::ChangeColorH);
            s += s * v * self.setting(BrushSetting::ChangeColorHsvS);
            v += self.setting(BrushSetting::ChangeColorV);
            hsv_to_rgb(&mut h, &mut s, &mut v);
            cr = h;
            cg = s;
            cb = v;
        }

        if using_hsl {
            let (mut h, mut s, mut l) = (cr, cg, cb);
            rgb_to_hsl(&mut h, &mut s, &mut l);
            l += self.setting(BrushSetting::ChangeColorL);
            s += s
                * (1.0_f32 - l).abs().min(l.abs())
                * 2.0
                * self.setting(BrushSetting::ChangeColorHslS);
            hsl_to_rgb(&mut h, &mut s, &mut l);
            cr = h;
            cg = s;
            cb = l;
        }

        if linear && (using_hsv || using_hsl) {
            cr = cr.powf(2.2);
            cg = cg.powf(2.2);
            cb = cb.powf(2.2);
        }

        let mut hardness = self.setting(BrushSetting::Hardness).clamp(0.0, 1.0);
        let softness = self.setting(BrushSetting::Softness).clamp(0.0, 1.0);

        // anti-aliasing
        let current_fadeout = radius * (1.0 - hardness);
        let min_fadeout = self.setting(BrushSetting::AntiAliasing);
        if current_fadeout < min_fadeout {
            let opt_r = radius - (1.0 - hardness) * radius / 2.0;
            let hardness_new = (opt_r - min_fadeout / 2.0) / (opt_r + min_fadeout / 2.0);
            radius = min_fadeout / (1.0 - hardness_new);
            hardness = hardness_new;
        }

        // snap to pixel
        let snap = self.setting(BrushSetting::SnapToPixel);
        if snap > 0.0 {
            let sx = x.floor() + 0.5;
            let sy = y.floor() + 0.5;
            x += (sx - x) * snap;
            y += (sy - y) * snap;
            let mut sr = (radius * 2.0).round() / 2.0;
            if sr < 0.5 {
                sr = 0.5;
            }
            if snap > 0.9999 {
                sr -= 0.0001;
            }
            radius += (sr - radius) * snap;
        }

        let dab_ratio = self.state(BrushState::ActualEllipticalDabRatio);
        let dab_angle = self.state(BrushState::ActualEllipticalDabAngle);
        let lock_alpha = self.setting(BrushSetting::LockAlpha);
        let colorize = self.setting(BrushSetting::Colorize);
        let posterize = self.setting(BrushSetting::Posterize);
        let posterize_num = self.setting(BrushSetting::PosterizeNum);

        surface.draw_dab(
            x,
            y,
            radius,
            cr,
            cg,
            cb,
            opaque,
            hardness,
            softness,
            eraser_target_alpha,
            dab_ratio,
            dab_angle,
            lock_alpha,
            colorize,
            posterize,
            posterize_num,
            paint_factor,
        )
    }

    // ── count_dabs_to ─────────────────────────────────────────────────────────

    fn count_dabs_to(&mut self, x: f32, y: f32, dt: f32) -> f32 {
        let base_radius_log = self.baseval(BrushSetting::RadiusLogarithmic);
        let base_radius = base_radius_log
            .exp()
            .clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);

        if self.state(BrushState::ActualRadius) == 0.0 {
            *self.state_mut(BrushState::ActualRadius) = base_radius;
        }

        let dx = x - self.state(BrushState::X);
        let dy = y - self.state(BrushState::Y);

        let dist = if self.state(BrushState::ActualEllipticalDabRatio) > 1.0 {
            let angle_rad = radians(self.state(BrushState::ActualEllipticalDabAngle));
            let cs = angle_rad.cos();
            let sn = angle_rad.sin();
            let yyr = (dy * cs - dx * sn) * self.state(BrushState::ActualEllipticalDabRatio);
            let xxr = dy * sn + dx * cs;
            (yyr * yyr + xxr * xxr).sqrt()
        } else {
            dx.hypot(dy)
        };

        let res1 = dist / self.state(BrushState::ActualRadius)
            * self.state(BrushState::DabsPerActualRadius);
        let res2 = dist / base_radius * self.state(BrushState::DabsPerBasicRadius);
        let res3 = dt * self.state(BrushState::DabsPerSecond);
        let res = res1 + res2 + res3;

        if res.is_nan() || res < 0.0 { 0.0 } else { res }
    }

    // ── stroke_to ─────────────────────────────────────────────────────────────

    /// Process one motion event.
    ///
    /// Returns `true` if the stroke should be split (for undo/redo).
    pub fn stroke_to<S: Surface>(
        &mut self,
        surface: &mut S,
        mut x: f32,
        mut y: f32,
        mut pressure: f32,
        xtilt: f32,
        ytilt: f32,
        dtime: f64,
        viewzoom: f32,
        viewrotation: f32,
        barrel_rotation: f32,
        linear: bool,
    ) -> bool {
        const MAX_DTIME: f64 = 5.0;

        // Tilt
        let mut tilt_ascension = 0.0_f32;
        let mut tilt_declination = 90.0_f32;
        let mut tilt_declinationx = 90.0_f32;
        let mut tilt_declinationy = 90.0_f32;

        if xtilt != 0.0 || ytilt != 0.0 {
            let xt = xtilt.clamp(-1.0, 1.0);
            let yt = ytilt.clamp(-1.0, 1.0);
            tilt_ascension = degrees((-xt).atan2(yt));
            let rad = xt.hypot(yt);
            tilt_declination = 90.0 - rad * 60.0;
            tilt_declinationx = xt * 60.0;
            tilt_declinationy = yt * 60.0;
        }

        if pressure < 0.0 {
            pressure = 0.0;
        }

        // sanity check
        if !x.is_finite() || !y.is_finite() || x.abs() > 1e10 || y.abs() > 1e10 {
            eprintln!("Warning: ignoring insane stroke_to inputs (x={x}, y={y})");
            x = 0.0;
            y = 0.0;
            pressure = 0.0;
        }

        let mut dtime = if dtime <= 0.0 { 0.0001 } else { dtime };

        // tablet without motion events workaround
        if dtime > 0.100 && pressure > 0.0 && self.state(BrushState::Pressure) == 0.0 {
            self.stroke_to(
                surface,
                x,
                y,
                0.0,
                90.0,
                0.0,
                dtime - 0.0001,
                viewzoom,
                viewrotation,
                0.0,
                linear,
            );
            dtime = 0.0001;
        }

        // tracking noise skip
        if self.skip > 0.001 {
            let dist = (self.skip_last_x - x).hypot(self.skip_last_y - y);
            self.skip_last_x = x;
            self.skip_last_y = y;
            self.skipped_dtime += dtime;
            self.skip -= dist;
            let dtime_acc = self.skipped_dtime;
            if self.skip > 0.001 && !(dtime_acc > MAX_DTIME || self.reset_requested) {
                return false;
            }
            self.skip = 0.0;
            self.skip_last_x = 0.0;
            self.skip_last_y = 0.0;
            self.skipped_dtime = 0.0;
            dtime = dtime_acc;
        }

        // slow tracking + tracking noise
        {
            let base_radius = self.baseval(BrushSetting::RadiusLogarithmic).exp();
            let noise_amount = base_radius * self.baseval(BrushSetting::TrackingNoise);
            if noise_amount > 0.001 {
                self.skip = 0.5 * noise_amount;
                self.skip_last_x = x;
                self.skip_last_y = y;
                x += noise_amount * rand_gauss(&mut self.rng);
                y += noise_amount * rand_gauss(&mut self.rng);
            }

            let fac = 1.0
                - Self::exp_decay(
                    self.baseval(BrushSetting::SlowTracking),
                    100.0 * dtime as f32,
                );
            x = self.state(BrushState::X) + (x - self.state(BrushState::X)) * fac;
            y = self.state(BrushState::Y) + (y - self.state(BrushState::Y)) * fac;
        }

        // reset handling
        if dtime > MAX_DTIME || self.reset_requested {
            self.reset_requested = false;
            self.brush_reset();
            self.random_input = self.rng.next();
            *self.state_mut(BrushState::X) = x;
            *self.state_mut(BrushState::Y) = y;
            *self.state_mut(BrushState::Pressure) = pressure;
            *self.state_mut(BrushState::ActualX) = x;
            *self.state_mut(BrushState::ActualY) = y;
            *self.state_mut(BrushState::Stroke) = 1.0;
            return true;
        }

        #[derive(PartialEq)]
        enum Painted {
            Unknown,
            Yes,
            No,
        }
        let mut painted = Painted::Unknown;
        let mut dtime_left = dtime as f32;

        let mut dabs_moved = self.state(BrushState::PartialDabs);
        let mut dabs_todo = self.count_dabs_to(x, y, dtime as f32);

        let mut step_dpressure;

        while dabs_moved + dabs_todo >= 1.0 {
            let step_ddab;
            if dabs_moved > 0.0 {
                step_ddab = 1.0 - dabs_moved;
                dabs_moved = 0.0;
            } else {
                step_ddab = 1.0;
            }
            let frac = step_ddab / dabs_todo;

            let step_dx = frac * (x - self.state(BrushState::X));
            let step_dy = frac * (y - self.state(BrushState::Y));
            step_dpressure = frac * (pressure - self.state(BrushState::Pressure));
            let step_dtime = frac * dtime_left;
            let step_decl = frac * (tilt_declination - self.state(BrushState::Declination));
            let step_declx = frac * (tilt_declinationx - self.state(BrushState::Declinationx));
            let step_decly = frac * (tilt_declinationy - self.state(BrushState::Declinationy));
            let step_asc = frac
                * smallest_angular_difference(self.state(BrushState::Ascension), tilt_ascension);
            let step_barrel = frac
                * smallest_angular_difference(
                    self.state(BrushState::BarrelRotation),
                    barrel_rotation * 360.0,
                );

            self.update_states_and_setting_values(
                step_ddab,
                step_dx,
                step_dy,
                step_dpressure,
                step_decl,
                step_asc,
                step_dtime,
                viewzoom,
                viewrotation,
                step_declx,
                step_decly,
                step_barrel,
            );

            *self.state_mut(BrushState::Flip) *= -1.0;
            let drawn = self.prepare_and_draw_dab(surface, linear);
            if drawn {
                painted = Painted::Yes;
            } else if painted == Painted::Unknown {
                painted = Painted::No;
            }

            self.random_input = self.rng.next();
            dtime_left -= step_dtime;
            dabs_todo = self.count_dabs_to(x, y, dtime_left);
        }

        // final partial step
        {
            let step_ddab = dabs_todo;
            let step_dx = x - self.state(BrushState::X);
            let step_dy = y - self.state(BrushState::Y);
            step_dpressure = pressure - self.state(BrushState::Pressure);
            let step_decl = tilt_declination - self.state(BrushState::Declination);
            let step_declx = tilt_declinationx - self.state(BrushState::Declinationx);
            let step_decly = tilt_declinationy - self.state(BrushState::Declinationy);
            let step_asc =
                smallest_angular_difference(self.state(BrushState::Ascension), tilt_ascension);
            let step_barrel = smallest_angular_difference(
                self.state(BrushState::BarrelRotation),
                barrel_rotation * 360.0,
            );

            self.update_states_and_setting_values(
                step_ddab,
                step_dx,
                step_dy,
                step_dpressure,
                step_decl,
                step_asc,
                dtime_left,
                viewzoom,
                viewrotation,
                step_declx,
                step_decly,
                step_barrel,
            );
        }

        *self.state_mut(BrushState::PartialDabs) = dabs_moved + dabs_todo;

        // stroke separation logic
        if painted == Painted::Unknown {
            painted = if self.stroke_current_idling_time > 0.0
                || self.stroke_total_painting_time == 0.0
            {
                Painted::No
            } else {
                Painted::Yes
            };
        }

        if painted == Painted::Yes {
            self.stroke_total_painting_time += dtime;
            self.stroke_current_idling_time = 0.0;
            if self.stroke_total_painting_time > 4.0 + 3.0 * pressure as f64 {
                if step_dpressure >= 0.0 {
                    return true;
                }
            }
        } else {
            self.stroke_current_idling_time += dtime;
            if self.stroke_total_painting_time == 0.0 {
                if self.stroke_current_idling_time > 1.0 {
                    return true;
                }
            } else if self.stroke_total_painting_time + self.stroke_current_idling_time
                > 0.9 + 5.0 * pressure as f64
            {
                return true;
            }
        }

        false
    }
}

// ── rand_gauss ────────────────────────────────────────────────────────────────

/// Box-Muller transform: generates a standard normal variate using the RNG.
pub fn rand_gauss(rng: &mut RngDouble) -> f32 {
    let mut x: f32;
    let mut y: f32;
    let mut r: f32;
    loop {
        x = 2.0 * rng.next() as f32 - 1.0;
        y = 2.0 * rng.next() as f32 - 1.0;
        r = x * x + y * y;
        if r > 0.0 && r < 1.0 {
            break;
        }
    }
    let fac = (-2.0 * r.ln() / r).sqrt();
    x * fac
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct NullSurface;
    impl Surface for NullSurface {
        fn draw_dab(
            &mut self,
            _x: f32,
            _y: f32,
            _r: f32,
            _cr: f32,
            _cg: f32,
            _cb: f32,
            _op: f32,
            _h: f32,
            _s: f32,
            _eta: f32,
            _ar: f32,
            _aa: f32,
            _la: f32,
            _col: f32,
            _po: f32,
            _pn: f32,
            _pm: f32,
        ) -> bool {
            true
        }
        fn get_color(&mut self, _x: f32, _y: f32, _r: f32, _pm: f32) -> (f32, f32, f32, f32) {
            (0.5, 0.5, 0.5, 1.0)
        }
    }

    #[test]
    fn test_brush_new() {
        let b = Brush::new();
        assert_eq!(b.get_base_value(BrushSetting::Opaque), 0.0);
    }

    #[test]
    fn test_from_defaults() {
        let mut b = Brush::new();
        b.from_defaults();
        let opaque_default = BRUSH_SETTING_INFO[BrushSetting::Opaque as usize].default;
        assert!((b.get_base_value(BrushSetting::Opaque) - opaque_default).abs() < 1e-6);
        // pressure mapping should be active
        assert!(!b.is_constant(BrushSetting::OpaqueMultiply));
    }

    #[test]
    fn test_set_get_state() {
        let mut b = Brush::new();
        b.set_state(BrushState::Pressure, 0.75);
        assert!((b.get_state(BrushState::Pressure) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn test_stroke_to_no_crash() {
        let mut b = Brush::new();
        b.from_defaults();
        b.set_base_value(BrushSetting::DabsPerActualRadius, 2.0);
        let mut surf = NullSurface;
        // Should not panic
        b.stroke_to(
            &mut surf, 0.0, 0.0, 0.5, 0.0, 0.0, 0.1, 1.0, 0.0, 0.0, false,
        );
        b.stroke_to(
            &mut surf, 10.0, 10.0, 0.5, 0.0, 0.0, 0.1, 1.0, 0.0, 0.0, false,
        );
    }

    #[test]
    fn test_reset_clears_state() {
        let mut b = Brush::new();
        b.set_state(BrushState::Pressure, 0.9);
        b.reset();
        let mut surf = NullSurface;
        // After reset, next stroke_to triggers brush_reset
        b.stroke_to(
            &mut surf, 0.0, 0.0, 0.5, 0.0, 0.0, 10.0, 1.0, 0.0, 0.0, false,
        );
        assert_eq!(b.get_state(BrushState::Pressure), 0.5);
    }

    #[test]
    fn test_rand_gauss_distribution() {
        let mut rng = RngDouble::new(42);
        let n = 10_000;
        let vals: Vec<f32> = (0..n).map(|_| rand_gauss(&mut rng)).collect();
        let mean = vals.iter().sum::<f32>() / n as f32;
        let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n as f32;
        assert!(mean.abs() < 0.1, "mean={mean}");
        assert!((var - 1.0).abs() < 0.1, "var={var}");
    }

    #[test]
    fn test_smudge_bucket_access() {
        let mut b = Brush::with_buckets(4);
        assert!(b.set_smudge_bucket(2, 0.1, 0.2, 0.3, 0.8, 0.0, 0.0, 0.0, 0.0, 0.5));
        let data = b.get_smudge_bucket(2).unwrap();
        assert!((data[SMUDGE_R] - 0.1).abs() < 1e-6);
        assert!((data[SMUDGE_A] - 0.8).abs() < 1e-6);
        assert!(b.get_smudge_bucket(5).is_none());
    }

    #[test]
    fn test_json_from_string_version_check() {
        let mut b = Brush::new();
        let bad = r#"{"version": 2, "settings": {}}"#;
        assert!(b.from_string(bad).is_err());
        let empty = r#"{"version": 3, "settings": {}}"#;
        assert!(b.from_string(empty).is_ok());
    }
}
