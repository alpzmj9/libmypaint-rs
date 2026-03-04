// mypaint-rs: brush_settings.rs
// Rewrite of libmypaint/mypaint-brush-settings.c
// Original Copyright (C) 2012 Jon Nordby <jononor@gmail.com>
// Licensed under ISC License

pub mod generated;
pub use generated::*;

impl BrushSetting {
    /// 从 cname 字符串查找对应的 BrushSetting。
    pub fn from_cname(cname: &str) -> Option<Self> {
        BRUSH_SETTING_INFO
            .iter()
            .enumerate()
            .find(|(_, info)| info.cname == cname)
            .map(|(i, _)| unsafe { std::mem::transmute::<usize, BrushSetting>(i) })
    }

    /// 返回该设置的元数据。
    #[inline]
    pub fn info(self) -> &'static BrushSettingInfo {
        &BRUSH_SETTING_INFO[self as usize]
    }
}

impl BrushInput {
    /// 从 cname 字符串查找对应的 BrushInput。
    pub fn from_cname(cname: &str) -> Option<Self> {
        BRUSH_INPUT_INFO
            .iter()
            .enumerate()
            .find(|(_, info)| info.cname == cname)
            .map(|(i, _)| unsafe { std::mem::transmute::<usize, BrushInput>(i) })
    }

    /// 返回该输入的元数据。
    #[inline]
    pub fn info(self) -> &'static BrushInputInfo {
        &BRUSH_INPUT_INFO[self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setting_count() {
        assert_eq!(BRUSH_SETTINGS_COUNT, 65);
    }

    #[test]
    fn test_input_count() {
        assert_eq!(BRUSH_INPUTS_COUNT, 18);
    }

    #[test]
    fn test_state_count() {
        assert_eq!(BRUSH_STATES_COUNT, 44);
    }

    #[test]
    fn test_setting_from_cname_found() {
        let s = BrushSetting::from_cname("opaque").unwrap();
        assert_eq!(s, BrushSetting::Opaque);
    }

    #[test]
    fn test_setting_from_cname_not_found() {
        assert!(BrushSetting::from_cname("nonexistent").is_none());
    }

    #[test]
    fn test_input_from_cname_found() {
        let i = BrushInput::from_cname("pressure").unwrap();
        assert_eq!(i, BrushInput::Pressure);
    }

    #[test]
    fn test_input_from_cname_not_found() {
        assert!(BrushInput::from_cname("nonexistent").is_none());
    }

    #[test]
    fn test_setting_info_values() {
        let info = BrushSetting::Opaque.info();
        assert_eq!(info.cname, "opaque");
        assert!((info.default - 1.0).abs() < 1e-6);
        assert!((info.min - 0.0).abs() < 1e-6);
        assert!((info.max - 2.0).abs() < 1e-6);
        assert!(!info.constant);
    }

    #[test]
    fn test_input_info_optional_hard_limits() {
        let info = BrushInput::Pressure.info();
        assert_eq!(info.hard_min, Some(0.0_f32));
        assert_eq!(info.hard_max, None);

        let info = BrushInput::Speed1.info();
        assert_eq!(info.hard_min, None);
        assert_eq!(info.hard_max, None);

        let info = BrushInput::Direction.info();
        assert_eq!(info.hard_min, Some(0.0_f32));
        assert_eq!(info.hard_max, Some(180.0_f32));
    }

    #[test]
    fn test_all_settings_have_valid_ranges() {
        for info in BRUSH_SETTING_INFO.iter() {
            assert!(
                info.min <= info.default && info.default <= info.max,
                "setting '{}': default {} not in [{}, {}]",
                info.cname,
                info.default,
                info.min,
                info.max
            );
        }
    }

    #[test]
    fn test_all_inputs_have_valid_soft_ranges() {
        for info in BRUSH_INPUT_INFO.iter() {
            assert!(
                info.soft_min <= info.soft_max,
                "input '{}': soft_min {} > soft_max {}",
                info.cname,
                info.soft_min,
                info.soft_max
            );
        }
    }

    #[test]
    fn test_cname_roundtrip_all_settings() {
        for (i, info) in BRUSH_SETTING_INFO.iter().enumerate() {
            let found = BrushSetting::from_cname(info.cname).unwrap();
            assert_eq!(found as usize, i, "roundtrip failed for '{}'", info.cname);
        }
    }

    #[test]
    fn test_cname_roundtrip_all_inputs() {
        for (i, info) in BRUSH_INPUT_INFO.iter().enumerate() {
            let found = BrushInput::from_cname(info.cname).unwrap();
            assert_eq!(found as usize, i, "roundtrip failed for '{}'", info.cname);
        }
    }
}
