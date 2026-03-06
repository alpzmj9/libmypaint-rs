pub mod brush; // ✅ 公开：主引擎，外部调用 Brush + Surface
pub mod brush_modes; // ✅ 公开：像素混合，surface 实现者需要
pub mod brush_settings; // ✅ 公开：BrushSetting/BrushInput 枚举，配置 Brush 必须用
pub mod helpers; // ✅ 公开：颜色空间转换，surface 实现者可能需要
pub mod mapping; // ✅ 公开：Mapping 类型，需要精细控制时使用
pub(crate) mod rng_double; // 🔒 仅 crate 内部：实现细节，外部无需触碰

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
