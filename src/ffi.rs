// ffi.rs — C ABI 导出层
// Rust 2024 edition: #[unsafe(no_mangle)], unsafe ops 需在 unsafe{} 块中

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_float, c_int};

use crate::brush::{Brush, Surface};
use crate::brush_settings::generated::{
    BrushInput, BrushSetting, BrushState,
    BRUSH_INPUTS_COUNT, BRUSH_SETTINGS_COUNT, BRUSH_STATES_COUNT,
};

// ── 不透明句柄 ────────────────────────────────────────────────────────────────

pub struct MyPaintBrush {
    inner: Brush,
}

// ── C Surface 回调表 ──────────────────────────────────────────────────────────

#[repr(C)]
pub struct MyPaintSurfaceVtable {
    pub draw_dab: Option<unsafe extern "C" fn(
        surface_data: *mut std::ffi::c_void,
        x: c_float, y: c_float, radius: c_float,
        color_r: c_float, color_g: c_float, color_b: c_float,
        opaque: c_float, hardness: c_float, softness: c_float,
        eraser_target_alpha: c_float,
        aspect_ratio: c_float, angle: c_float,
        lock_alpha: c_float, colorize: c_float,
        posterize: c_float, posterize_num: c_float,
        paint_mode: c_float,
    ) -> c_int>,
    pub get_color: Option<unsafe extern "C" fn(
        surface_data: *mut std::ffi::c_void,
        x: c_float, y: c_float, radius: c_float,
        paint_mode: c_float,
        r_out: *mut c_float, g_out: *mut c_float,
        b_out: *mut c_float, a_out: *mut c_float,
    )>,
}

#[repr(C)]
pub struct MyPaintSurface {
    pub vtable: MyPaintSurfaceVtable,
    pub surface_data: *mut std::ffi::c_void,
}

struct CSurface(*mut MyPaintSurface);

impl Surface for CSurface {
    fn draw_dab(&mut self, x: f32, y: f32, radius: f32,
                cr: f32, cg: f32, cb: f32,
                opaque: f32, hardness: f32, softness: f32, eta: f32,
                ar: f32, aa: f32, la: f32, col: f32,
                po: f32, pn: f32, pm: f32) -> bool {
        unsafe {
            let s = &*self.0;
            if let Some(f) = s.vtable.draw_dab {
                f(s.surface_data, x, y, radius, cr, cg, cb,
                  opaque, hardness, softness, eta, ar, aa, la, col, po, pn, pm) != 0
            } else { false }
        }
    }
    fn get_color(&mut self, x: f32, y: f32, radius: f32, pm: f32)
        -> (f32, f32, f32, f32)
    {
        unsafe {
            let s = &*self.0;
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0, 0.0, 0.0);
            if let Some(f) = s.vtable.get_color {
                f(s.surface_data, x, y, radius, pm,
                  &mut r, &mut g, &mut b, &mut a);
            }
            (r, g, b, a)
        }
    }
}

// ── 生命周期 ──────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_new() -> *mut MyPaintBrush {
    mypaint_brush_new_with_buckets(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_new_with_buckets(
    num_smudge_buckets: c_int,
) -> *mut MyPaintBrush {
    let b = Box::new(MyPaintBrush {
        inner: Brush::with_buckets(num_smudge_buckets.max(0) as usize),
    });
    Box::into_raw(b)
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_free(self_: *mut MyPaintBrush) {
    if !self_.is_null() {
        unsafe { drop(Box::from_raw(self_)); }
    }
}

// ── 笔画控制 ──────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_reset(self_: *mut MyPaintBrush) {
    unsafe { (*self_).inner.reset(); }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_new_stroke(self_: *mut MyPaintBrush) {
    unsafe { (*self_).inner.new_stroke(); }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_stroke_to(
    self_: *mut MyPaintBrush,
    surface: *mut MyPaintSurface,
    x: c_float, y: c_float, pressure: c_float,
    xtilt: c_float, ytilt: c_float,
    dtime: c_double,
    viewzoom: c_float, viewrotation: c_float,
    barrel_rotation: c_float,
    linear: c_int,
) -> c_int {
    unsafe {
        let brush = &mut (*self_).inner;
        let mut surf = CSurface(surface);
        brush.stroke_to(
            &mut surf, x, y, pressure, xtilt, ytilt,
            dtime, viewzoom, viewrotation, barrel_rotation,
            linear != 0,
        ) as c_int
    }
}

// ── 设置访问 ──────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_set_base_value(
    self_: *mut MyPaintBrush, id: c_int, value: c_float,
) {
    if let Some(s) = setting_from_int(id) {
        unsafe { (*self_).inner.set_base_value(s, value); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_get_base_value(
    self_: *mut MyPaintBrush, id: c_int,
) -> c_float {
    setting_from_int(id)
        .map(|s| unsafe { (*self_).inner.get_base_value(s) })
        .unwrap_or(0.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_is_constant(
    self_: *mut MyPaintBrush, id: c_int,
) -> c_int {
    setting_from_int(id)
        .map(|s| unsafe { (*self_).inner.is_constant(s) as c_int })
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_get_inputs_used_n(
    self_: *mut MyPaintBrush, id: c_int,
) -> c_int {
    setting_from_int(id)
        .map(|s| unsafe { (*self_).inner.get_inputs_used_n(s) as c_int })
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_set_mapping_n(
    self_: *mut MyPaintBrush, setting: c_int, input: c_int, n: c_int,
) {
    if let (Some(s), Some(i)) = (setting_from_int(setting), input_from_int(input)) {
        unsafe { (*self_).inner.set_mapping_n(s, i, n.max(0) as usize); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_get_mapping_n(
    self_: *mut MyPaintBrush, setting: c_int, input: c_int,
) -> c_int {
    match (setting_from_int(setting), input_from_int(input)) {
        (Some(s), Some(i)) => unsafe { (*self_).inner.get_mapping_n(s, i) as c_int },
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_set_mapping_point(
    self_: *mut MyPaintBrush,
    setting: c_int, input: c_int, index: c_int,
    x: c_float, y: c_float,
) {
    if let (Some(s), Some(i)) = (setting_from_int(setting), input_from_int(input)) {
        unsafe { (*self_).inner.set_mapping_point(s, i, index as usize, x, y); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_get_mapping_point(
    self_: *mut MyPaintBrush,
    setting: c_int, input: c_int, index: c_int,
    x_out: *mut c_float, y_out: *mut c_float,
) {
    if let (Some(s), Some(i)) = (setting_from_int(setting), input_from_int(input)) {
        let (x, y) = unsafe { (*self_).inner.get_mapping_point(s, i, index as usize) };
        unsafe {
            if !x_out.is_null() { *x_out = x; }
            if !y_out.is_null() { *y_out = y; }
        }
    }
}

// ── 状态访问 ──────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_get_state(
    self_: *mut MyPaintBrush, i: c_int,
) -> c_float {
    state_from_int(i)
        .map(|s| unsafe { (*self_).inner.get_state(s) })
        .unwrap_or(0.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_set_state(
    self_: *mut MyPaintBrush, i: c_int, value: c_float,
) {
    if let Some(s) = state_from_int(i) {
        unsafe { (*self_).inner.set_state(s, value); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_get_total_stroke_painting_time(
    self_: *mut MyPaintBrush,
) -> c_double {
    unsafe { (*self_).inner.get_total_stroke_painting_time() }
}

// ── JSON 加载 ─────────────────────────────────────────────────────────────────

/// 从 MyPaint v3 JSON 字符串加载笔刷设置。返回 1 成功，0 失败。
#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_from_string(
    self_: *mut MyPaintBrush,
    string: *const c_char,
) -> c_int {
    if string.is_null() { return 0; }
    let s = match unsafe { CStr::from_ptr(string) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    match unsafe { (*self_).inner.from_string(s) } {
        Ok(_) => 1,
        Err(e) => { eprintln!("mypaint_brush_from_string: {e}"); 0 }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_from_defaults(self_: *mut MyPaintBrush) {
    unsafe { (*self_).inner.from_defaults(); }
}

// ── 调试 ──────────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn mypaint_brush_set_print_inputs(
    self_: *mut MyPaintBrush, enabled: c_int,
) {
    unsafe { (*self_).inner.print_inputs = enabled != 0; }
}

// ── 枚举转换辅助 ──────────────────────────────────────────────────────────────

#[inline]
fn setting_from_int(id: c_int) -> Option<BrushSetting> {
    if id >= 0 && (id as usize) < BRUSH_SETTINGS_COUNT {
        Some(unsafe { std::mem::transmute::<usize, BrushSetting>(id as usize) })
    } else { None }
}

#[inline]
fn input_from_int(id: c_int) -> Option<BrushInput> {
    if id >= 0 && (id as usize) < BRUSH_INPUTS_COUNT {
        Some(unsafe { std::mem::transmute::<usize, BrushInput>(id as usize) })
    } else { None }
}

#[inline]
fn state_from_int(id: c_int) -> Option<BrushState> {
    if id >= 0 && (id as usize) < BRUSH_STATES_COUNT {
        Some(unsafe { std::mem::transmute::<usize, BrushState>(id as usize) })
    } else { None }
}