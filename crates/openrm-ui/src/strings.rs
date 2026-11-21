//! 原版 `plugins/common/Strings.h` + `plugins/BandPass/src/Strings.h` 的文本表。
//!
//! 条目、中英文文案、默认语言（跟随系统）逐字对齐；设置面板切语言时只换查表结果。

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Zh,
}

impl Default for Lang {
    fn default() -> Self {
        Lang::Zh
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Language,
    Dark,
    Light,
    ThemeColor,
    Settings,
    Theme,
    Hue,
    Saturation,
    SatNone,
    SatLow,
    SatMed,
    SatHigh,
    Chinese,
    Audio,
    AudioInput,
    AudioOutput,
    Driver,
    Red,
    Yellow,
    Blue,
    Green,
    SaveHere,
    RestoreDefault,
    Presets,
    Random,
    Morph,
    Range,
    Speed,
    Mix,
    GainL,
    GainR,
    Center,
    Bandwidth,
    Slope,
    LowCut,
    HighCut,
    Left,
    Right,
    Mono,
    Link,
    Flip,
    CopyLR,
    CopyRL,
    Undo,
    Redo,
    Save,
    Load,
    TipPad,
    TipBand,
    TipSlope,
    TipGain,
    TipMorph,
    TipRange,
    TipSpeed,
    TipMix,
    TipFade,
    TipPresets,
    TipRandom,
    TipDrag,
    TipSaveHere,
    TipMenu,
    TipRandomSwatch,
    TipRandomPicker,
    Pass,
    Reject,
    PresetFmt,
}

pub fn tr(lang: Lang, key: Key) -> &'static str {
    use Key::*;
    match (key, lang) {
        (Language, Lang::En) => "LANGUAGE",
        (Language, Lang::Zh) => "语言",
        (Dark, Lang::En) => "DARK",
        (Dark, Lang::Zh) => "深色",
        (Light, Lang::En) => "LIGHT",
        (Light, Lang::Zh) => "浅色",
        (ThemeColor, Lang::En) => "Theme Color",
        (ThemeColor, Lang::Zh) => "主题色",
        (Settings, Lang::En) => "SETTINGS",
        (Settings, Lang::Zh) => "设置",
        (Theme, Lang::En) => "THEME",
        (Theme, Lang::Zh) => "主题",
        (Hue, Lang::En) => "Hue",
        (Hue, Lang::Zh) => "色相",
        (Saturation, Lang::En) => "Saturation",
        (Saturation, Lang::Zh) => "饱和度",
        (SatNone, Lang::En) => "NONE",
        (SatNone, Lang::Zh) => "无",
        (SatLow, Lang::En) => "LOW",
        (SatLow, Lang::Zh) => "低",
        (SatMed, Lang::En) => "MED",
        (SatMed, Lang::Zh) => "中",
        (SatHigh, Lang::En) => "HIGH",
        (SatHigh, Lang::Zh) => "高",
        (Chinese, _) => "中文",
        (Audio, Lang::En) => "AUDIO",
        (Audio, Lang::Zh) => "音频",
        (AudioInput, Lang::En) => "Input",
        (AudioInput, Lang::Zh) => "输入",
        (AudioOutput, Lang::En) => "Output",
        (AudioOutput, Lang::Zh) => "输出",
        (Driver, Lang::En) => "Driver",
        (Driver, Lang::Zh) => "驱动",
        (Red, Lang::En) => "Red",
        (Red, Lang::Zh) => "红",
        (Yellow, Lang::En) => "Yellow",
        (Yellow, Lang::Zh) => "黄",
        (Blue, Lang::En) => "Blue",
        (Blue, Lang::Zh) => "蓝",
        (Green, Lang::En) => "Green",
        (Green, Lang::Zh) => "绿",
        (SaveHere, Lang::En) => "Save Here",
        (SaveHere, Lang::Zh) => "保存到此槽",
        (RestoreDefault, Lang::En) => "Restore Default",
        (RestoreDefault, Lang::Zh) => "恢复默认",
        (Presets, Lang::En) => "PRESETS",
        (Presets, Lang::Zh) => "预设",
        (Random, Lang::En) => "RANDOM",
        (Random, Lang::Zh) => "随机",
        (Morph, Lang::En) => "Morph",
        (Morph, Lang::Zh) => "渐变",
        (Range, Lang::En) => "Range",
        (Range, Lang::Zh) => "范围",
        (Speed, Lang::En) => "Speed",
        (Speed, Lang::Zh) => "速度",
        (Mix, Lang::En) => "MIX",
        (Mix, Lang::Zh) => "混合",
        (GainL, Lang::En) => "GAIN L",
        (GainL, Lang::Zh) => "增益 L",
        (GainR, Lang::En) => "GAIN R",
        (GainR, Lang::Zh) => "增益 R",
        (Center, Lang::En) => "CENTER",
        (Center, Lang::Zh) => "中心",
        (Bandwidth, Lang::En) => "WIDTH",
        (Bandwidth, Lang::Zh) => "宽度",
        (Slope, Lang::En) => "SLOPE",
        (Slope, Lang::Zh) => "滚降",
        (LowCut, Lang::En) => "LOWCUT",
        (LowCut, Lang::Zh) => "低切",
        (HighCut, Lang::En) => "HIGHCUT",
        (HighCut, Lang::Zh) => "高切",
        (Left, _) => "LEFT",
        (Right, _) => "RIGHT",
        (Mono, _) => "MONO",
        (Link, Lang::En) => "LINK",
        (Link, Lang::Zh) => "同步",
        (Flip, Lang::En) => "FLIP",
        (Flip, Lang::Zh) => "翻转",
        (CopyLR, Lang::En) => "L->R",
        (CopyLR, Lang::Zh) => "左→右",
        (CopyRL, Lang::En) => "R->L",
        (CopyRL, Lang::Zh) => "右→左",
        (Undo, Lang::En) => "UNDO",
        (Undo, Lang::Zh) => "撤销",
        (Redo, Lang::En) => "REDO",
        (Redo, Lang::Zh) => "重做",
        (Save, Lang::En) => "SAVE",
        (Save, Lang::Zh) => "保存",
        (Load, Lang::En) => "LOAD",
        (Load, Lang::Zh) => "读取",
        (TipPad, Lang::En) => "Drag to set center frequency and bandwidth",
        (TipPad, Lang::Zh) => "拖动设置中心频率与带宽",
        (TipBand, Lang::En) => "Drag the handles to set the low and high cut",
        (TipBand, Lang::Zh) => "拖动手柄设置低切与高切",
        (TipSlope, Lang::En) => "Band-pass rolloff slope",
        (TipSlope, Lang::Zh) => "带通滚降斜率",
        (TipGain, Lang::En) => "Channel gain",
        (TipGain, Lang::Zh) => "声道增益",
        (TipMorph, Lang::En) => "Fade time when switching presets",
        (TipMorph, Lang::Zh) => "切换预设时的渐变时间",
        (TipRange, Lang::En) => "Random amplitude range",
        (TipRange, Lang::Zh) => "随机幅度范围",
        (TipSpeed, Lang::En) => "Random speed (period)",
        (TipSpeed, Lang::Zh) => "随机速度（周期）",
        (TipMix, Lang::En) => "Dry/wet mix",
        (TipMix, Lang::Zh) => "干湿混合",
        (TipFade, Lang::En) => "Preset fade position",
        (TipFade, Lang::Zh) => "预设渐变位置",
        (TipPresets, Lang::En) => "Preset slots",
        (TipPresets, Lang::Zh) => "预设槽",
        (TipRandom, Lang::En) => "Random modulation",
        (TipRandom, Lang::Zh) => "随机调制",
        (TipDrag, Lang::En) => "Drag onto another slot to swap",
        (TipDrag, Lang::Zh) => "拖到其他槽位以交换",
        (TipSaveHere, Lang::En) => "Cmd+Click: save here",
        (TipSaveHere, Lang::Zh) => "Cmd+点击：保存到此槽",
        (TipMenu, Lang::En) => "Right-click: menu",
        (TipMenu, Lang::Zh) => "右键：菜单",
        (TipRandomSwatch, Lang::En) => "Click: toggle random / Right-click: color",
        (TipRandomSwatch, Lang::Zh) => "点击：开关随机 / 右键：选颜色",
        (TipRandomPicker, Lang::En) => "Choose the random color to edit",
        (TipRandomPicker, Lang::Zh) => "选择要编辑的随机颜色",
        (Pass, Lang::En) => "BP",
        (Pass, Lang::Zh) => "带通",
        (Reject, Lang::En) => "BR",
        (Reject, Lang::Zh) => "带阻",
        (PresetFmt, _) => "Preset %d",
    }
}

/// 系统语言探测：原版 macOS 读 `CFLocaleCopyPreferredLanguages`。
/// 这里读 `AppleLanguages` 首项，失败再退回 `LANG`/`LC_ALL`，最后默认中文。
pub fn detect_lang() -> Lang {
    if let Ok(out) = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        if s.contains("zh") || s.contains("Hans") || s.contains("Hant") {
            return Lang::Zh;
        }
        if s.contains("en") {
            return Lang::En;
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(key) {
            if v.to_lowercase().starts_with("zh") {
                return Lang::Zh;
            }
        }
    }
    Lang::Zh
}
