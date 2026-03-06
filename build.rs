// build.rs
// 完全用 Rust 完成代码生成，不依赖 Python：
//   1. 解析 brushsettings.json → 生成 src/brush_settings/generated.rs
//   2. 调用 cbindgen → 生成 include/mypaint_rs.h

use std::fmt::Write as FmtWrite;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest);

    println!("cargo:rerun-if-changed=brushsettings.json");
    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    gen_brush_settings(root);
    gen_c_header(root);
}

// ── 1. brushsettings.json → generated.rs ─────────────────────────────────────

fn gen_brush_settings(root: &Path) {
    let json_path = root.join("brushsettings.json");
    let out_path  = root.join("src/brush_settings/generated.rs");

    let raw = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {e}", json_path.display()));

    let data: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("JSON parse error in brushsettings.json: {e}"));

    let content = render_generated_rs(&data);

    // 只在内容变化时写入，避免不必要的重编译
    if out_path.exists() {
        if std::fs::read_to_string(&out_path).unwrap_or_default() == content {
            return;
        }
    }

    std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
    std::fs::write(&out_path, &content)
        .unwrap_or_else(|e| panic!("Cannot write {}: {e}", out_path.display()));
}

fn render_generated_rs(data: &serde_json::Value) -> String {
    let inputs   = data["inputs"].as_array().expect("inputs array");
    let settings = data["settings"].as_array().expect("settings array");
    let states   = data["states"].as_array().expect("states array");

    let mut s = String::with_capacity(64 * 1024);

    s.push_str("// generated.rs\n");
    s.push_str("// Auto-generated from brushsettings.json — do not edit by hand.\n");
    s.push_str("// To regenerate: cargo build\n\n");

    // ── BrushInput ────────────────────────────────────────────────────────────
    s.push_str("/// Input channels available to the brush engine.\n");
    s.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    s.push_str("#[repr(usize)]\n");
    s.push_str("pub enum BrushInput {\n");
    for (i, inp) in inputs.iter().enumerate() {
        let id = inp["id"].as_str().unwrap();
        writeln!(s, "    {} = {i},", to_pascal(id)).unwrap();
    }
    s.push_str("}\n");
    writeln!(s, "pub const BRUSH_INPUTS_COUNT: usize = {};", inputs.len()).unwrap();
    s.push('\n');

    // ── BrushSetting ──────────────────────────────────────────────────────────
    s.push_str("/// Brush parameter settings.\n");
    s.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    s.push_str("#[repr(usize)]\n");
    s.push_str("pub enum BrushSetting {\n");
    for (i, st) in settings.iter().enumerate() {
        let id = st["internal_name"].as_str().unwrap();
        writeln!(s, "    {} = {i},", to_pascal(id)).unwrap();
    }
    s.push_str("}\n");
    writeln!(s, "pub const BRUSH_SETTINGS_COUNT: usize = {};", settings.len()).unwrap();
    s.push('\n');

    // ── BrushState ────────────────────────────────────────────────────────────
    s.push_str("/// Internal brush state variables.\n");
    s.push_str("/// WARNING: only append — order must match replay files.\n");
    s.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    s.push_str("#[repr(usize)]\n");
    s.push_str("pub enum BrushState {\n");
    for (i, st) in states.iter().enumerate() {
        let id = st.as_str().unwrap();
        writeln!(s, "    {} = {i},", to_pascal(id)).unwrap();
    }
    s.push_str("}\n");
    writeln!(s, "pub const BRUSH_STATES_COUNT: usize = {};", states.len()).unwrap();
    s.push('\n');

    // ── Info structs ──────────────────────────────────────────────────────────
    s.push_str("/// Metadata for a brush input channel.\n");
    s.push_str("#[derive(Debug, Clone)]\n");
    s.push_str("pub struct BrushInputInfo {\n");
    s.push_str("    pub cname:    &'static str,\n");
    s.push_str("    pub hard_min: Option<f32>,\n");
    s.push_str("    pub soft_min: f32,\n");
    s.push_str("    pub normal:   f32,\n");
    s.push_str("    pub soft_max: f32,\n");
    s.push_str("    pub hard_max: Option<f32>,\n");
    s.push_str("    pub name:     &'static str,\n");
    s.push_str("    pub tooltip:  &'static str,\n");
    s.push_str("}\n\n");

    s.push_str("/// Metadata for a brush setting parameter.\n");
    s.push_str("#[derive(Debug, Clone)]\n");
    s.push_str("pub struct BrushSettingInfo {\n");
    s.push_str("    pub cname:    &'static str,\n");
    s.push_str("    pub name:     &'static str,\n");
    s.push_str("    pub constant: bool,\n");
    s.push_str("    pub min:      f32,\n");
    s.push_str("    pub default:  f32,\n");
    s.push_str("    pub max:      f32,\n");
    s.push_str("    pub tooltip:  &'static str,\n");
    s.push_str("}\n\n");

    // ── Static tables ─────────────────────────────────────────────────────────
    writeln!(s, "pub static BRUSH_INPUT_INFO: [BrushInputInfo; BRUSH_INPUTS_COUNT] = [").unwrap();
    for inp in inputs {
        writeln!(s, "    BrushInputInfo {{").unwrap();
        writeln!(s, "        cname:    \"{}\",",  esc(inp["id"].as_str().unwrap())).unwrap();
        writeln!(s, "        hard_min: {},",       opt_f32(&inp["hard_minimum"])).unwrap();
        writeln!(s, "        soft_min: {},",       f32_lit(&inp["soft_minimum"])).unwrap();
        writeln!(s, "        normal:   {},",       f32_lit(&inp["normal"])).unwrap();
        writeln!(s, "        soft_max: {},",       f32_lit(&inp["soft_maximum"])).unwrap();
        writeln!(s, "        hard_max: {},",       opt_f32(&inp["hard_maximum"])).unwrap();
        writeln!(s, "        name:     \"{}\",",  esc(inp["displayed_name"].as_str().unwrap())).unwrap();
        writeln!(s, "        tooltip:  \"{}\",",  esc(inp["tooltip"].as_str().unwrap())).unwrap();
        s.push_str("    },\n");
    }
    s.push_str("];\n\n");

    writeln!(s, "pub static BRUSH_SETTING_INFO: [BrushSettingInfo; BRUSH_SETTINGS_COUNT] = [").unwrap();
    for st in settings {
        writeln!(s, "    BrushSettingInfo {{").unwrap();
        writeln!(s, "        cname:    \"{}\",",  esc(st["internal_name"].as_str().unwrap())).unwrap();
        writeln!(s, "        name:     \"{}\",",  esc(st["displayed_name"].as_str().unwrap())).unwrap();
        writeln!(s, "        constant: {},",       if st["constant"].as_bool().unwrap_or(false) { "true" } else { "false" }).unwrap();
        writeln!(s, "        min:      {},",       f32_lit(&st["minimum"])).unwrap();
        writeln!(s, "        default:  {},",       f32_lit(&st["default"])).unwrap();
        writeln!(s, "        max:      {},",       f32_lit(&st["maximum"])).unwrap();
        writeln!(s, "        tooltip:  \"{}\",",  esc(st["tooltip"].as_str().unwrap())).unwrap();
        s.push_str("    },\n");
    }
    s.push_str("];\n");

    s
}

// ── 字符串辅助 ────────────────────────────────────────────────────────────────

/// dabs_per_second → DabsPerSecond
fn to_pascal(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

/// JSON number → "1.23_f32"
fn f32_lit(v: &serde_json::Value) -> String {
    format!("{}_f32", v.as_f64().expect("expected f64"))
}

/// JSON number | null → "Some(1.23_f32)" | "None"
fn opt_f32(v: &serde_json::Value) -> String {
    if v.is_null() {
        "None".to_string()
    } else {
        format!("Some({}_f32)", v.as_f64().expect("expected f64"))
    }
}

/// 转义 Rust 字符串字面量中的特殊字符
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('"', "\\\"")
     .replace('\n', "\\n")
}

// ── 2. cbindgen → include/mypaint_rs.h ───────────────────────────────────────

fn gen_c_header(root: &Path) {
    let out_dir = root.join("include");
    std::fs::create_dir_all(&out_dir).expect("Failed to create include/");
    let header = out_dir.join("mypaint_rs.h");

    let config = cbindgen::Config::from_file(root.join("cbindgen.toml"))
        .expect("Failed to read cbindgen.toml");

    cbindgen::Builder::new()
        .with_crate(root)
        .with_config(config)
        .with_src(root.join("src/ffi.rs"))
        .generate()
        .expect("cbindgen failed")
        .write_to_file(&header);
}