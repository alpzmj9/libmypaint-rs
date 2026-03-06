// build.rs
// 在 cargo build / cargo test 前自动从 brushsettings.json 生成
// src/brush_settings/generated.rs。
//
// 依赖：系统需要安装 Python 3（命令为 python3 或 python）。

use std::path::Path;
use std::process::Command;

fn main() {
    let json = "brushsettings.json";
    let script = "codegen/gen_brush_settings.py";
    let output = "src/brush_settings/generated.rs";

    // 告诉 Cargo：只在这些文件变化时重新运行 build.rs
    println!("cargo:rerun-if-changed={json}");
    println!("cargo:rerun-if-changed={script}");

    // 找到可用的 Python 解释器
    let python = find_python().expect(
        "Python 3 not found. Please install Python 3 to build this project.\n\
         Tried: python3, python",
    );

    let status = Command::new(&python)
        .arg(script)
        .status()
        .unwrap_or_else(|e| panic!("Failed to run {script}: {e}"));

    if !status.success() {
        panic!("Code generation failed (exit code: {:?})", status.code());
    }

    // 确认输出文件存在
    assert!(
        Path::new(output).exists(),
        "Code generation succeeded but {output} was not created"
    );
}

/// 按优先级尝试 python3 → python，返回第一个可用的。
fn find_python() -> Option<String> {
    for candidate in ["python3", "python"] {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}
