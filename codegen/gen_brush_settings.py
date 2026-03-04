#!/usr/bin/env python3
"""
codegen/gen_brush_settings.py
从 brushsettings.json 生成 src/brush_settings/generated.rs。
由 build.rs 在每次编译前自动调用，也可以手动执行：
    python3 codegen/gen_brush_settings.py
"""

import json
import re
import sys
from pathlib import Path

REPO_ROOT   = Path(__file__).parent.parent
JSON_PATH   = REPO_ROOT / "brushsettings.json"
OUTPUT_PATH = REPO_ROOT / "src" / "brush_settings" / "generated.rs"


def to_pascal(s: str) -> str:
    """dabs_per_second -> DabsPerSecond"""
    return "".join(w.capitalize() for w in re.split(r"[_]+", s))


def opt_f32(v) -> str:
    return "None" if v is None else f"Some({float(v)}_f32)"


def f32_lit(v) -> str:
    return f"{float(v)}_f32"


def escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def generate(data: dict) -> str:
    lines = []

    lines += [
        "// generated.rs",
        "// Auto-generated from brushsettings.json — do not edit by hand.",
        "// To regenerate: python3 codegen/gen_brush_settings.py",
        "",
    ]

    # ── BrushInput enum ───────────────────────────────────────────────────────
    lines += [
        "/// Input channels available to the brush engine.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]",
        "#[repr(usize)]",
        "pub enum BrushInput {",
    ]
    for i, inp in enumerate(data["inputs"]):
        lines.append(f'    {to_pascal(inp["id"])} = {i},')
    lines.append("}")
    lines.append(f'pub const BRUSH_INPUTS_COUNT: usize = {len(data["inputs"])};')
    lines.append("")

    # ── BrushSetting enum ─────────────────────────────────────────────────────
    lines += [
        "/// Brush parameter settings.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]",
        "#[repr(usize)]",
        "pub enum BrushSetting {",
    ]
    for i, s in enumerate(data["settings"]):
        lines.append(f'    {to_pascal(s["internal_name"])} = {i},')
    lines.append("}")
    lines.append(f'pub const BRUSH_SETTINGS_COUNT: usize = {len(data["settings"])};')
    lines.append("")

    # ── BrushState enum ───────────────────────────────────────────────────────
    lines += [
        "/// Internal brush state variables.",
        "/// WARNING: only append — order must match replay files.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]",
        "#[repr(usize)]",
        "pub enum BrushState {",
    ]
    for i, st in enumerate(data["states"]):
        lines.append(f"    {to_pascal(st)} = {i},")
    lines.append("}")
    lines.append(f'pub const BRUSH_STATES_COUNT: usize = {len(data["states"])};')
    lines.append("")

    # ── Info structs ──────────────────────────────────────────────────────────
    lines += [
        "/// Metadata for a brush input channel.",
        "#[derive(Debug, Clone)]",
        "pub struct BrushInputInfo {",
        "    pub cname:    &'static str,",
        "    pub hard_min: Option<f32>,",
        "    pub soft_min: f32,",
        "    pub normal:   f32,",
        "    pub soft_max: f32,",
        "    pub hard_max: Option<f32>,",
        "    pub name:     &'static str,",
        "    pub tooltip:  &'static str,",
        "}",
        "",
        "/// Metadata for a brush setting parameter.",
        "#[derive(Debug, Clone)]",
        "pub struct BrushSettingInfo {",
        "    pub cname:    &'static str,",
        "    pub name:     &'static str,",
        "    pub constant: bool,",
        "    pub min:      f32,",
        "    pub default:  f32,",
        "    pub max:      f32,",
        "    pub tooltip:  &'static str,",
        "}",
        "",
    ]

    # ── Static tables ─────────────────────────────────────────────────────────
    lines.append(
        f"pub static BRUSH_INPUT_INFO: [BrushInputInfo; BRUSH_INPUTS_COUNT] = ["
    )
    for inp in data["inputs"]:
        lines += [
            "    BrushInputInfo {",
            f'        cname:    "{escape(inp["id"])}",',
            f'        hard_min: {opt_f32(inp["hard_minimum"])},',
            f'        soft_min: {f32_lit(inp["soft_minimum"])},',
            f'        normal:   {f32_lit(inp["normal"])},',
            f'        soft_max: {f32_lit(inp["soft_maximum"])},',
            f'        hard_max: {opt_f32(inp["hard_maximum"])},',
            f'        name:     "{escape(inp["displayed_name"])}",',
            f'        tooltip:  "{escape(inp["tooltip"])}",',
            "    },",
        ]
    lines.append("];")
    lines.append("")

    lines.append(
        f"pub static BRUSH_SETTING_INFO: [BrushSettingInfo; BRUSH_SETTINGS_COUNT] = ["
    )
    for s in data["settings"]:
        lines += [
            "    BrushSettingInfo {",
            f'        cname:    "{escape(s["internal_name"])}",',
            f'        name:     "{escape(s["displayed_name"])}",',
            f'        constant: {"true" if s["constant"] else "false"},',
            f'        min:      {f32_lit(s["minimum"])},',
            f'        default:  {f32_lit(s["default"])},',
            f'        max:      {f32_lit(s["maximum"])},',
            f'        tooltip:  "{escape(s["tooltip"])}",',
            "    },",
        ]
    lines.append("];")

    return "\n".join(lines) + "\n"


def main():
    if not JSON_PATH.exists():
        print(f"error: {JSON_PATH} not found", file=sys.stderr)
        sys.exit(1)

    with open(JSON_PATH, encoding="utf-8") as f:
        data = json.load(f)

    content = generate(data)

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)

    # 只在内容发生变化时写入，避免不必要的重编译
    if OUTPUT_PATH.exists() and OUTPUT_PATH.read_text(encoding="utf-8") == content:
        print(f"up-to-date: {OUTPUT_PATH}")
        return

    OUTPUT_PATH.write_text(content, encoding="utf-8")
    print(f"generated:  {OUTPUT_PATH}")


if __name__ == "__main__":
    main()