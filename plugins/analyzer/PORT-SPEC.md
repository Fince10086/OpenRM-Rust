# Analyzer 移植规范（冻结接口 / 文件归属）

目标：在 `OpenRM-Rust` 中把原版 C++/iPlug2 Analyzer **像素级 + 行为级**复刻成 truce 插件。
本文件是并行开发时唯一的接口契约，**不要修改本文件**；需要改契约先找 OrganizeAgent。

## 0. 权威来源

- 原版 C++：`/Users/feizao/个人文件/plugin-dev/OpenRM/plugins/Analyzer/src`
  （`Analyzer.cpp/.h`、`Params.h`、`Theme.h`、`Strings.h`、`controls/*.h`、`dsp/*.h`）
- 已完成的 Rust 范式：`/Users/feizao/个人文件/plugin-dev/OpenRM-Rust/plugins/bandpass`
  （`src/lib.rs` truce 外壳、`src/editor.rs` retained egui 控件、`examples/shot.rs` 离屏截图）
- 共用件：`crates/openrm-ui`（`fonts.rs` 字号 0.8× 换算、`theme.rs` HSB 色阶、`strings.rs` 双语文案、`widgets.rs`）
- 已移植完成，**不要重做**：`crates/openrm-analyzer-dsp/src/{fastmath.rs,level_meter.rs}`
  （`LevelSnapshot` 字段见该文件）
- 截图参考：`docs/screenshots/analyzer-original*.png`（由参考截图任务产出；`docs/analyzer-reference.md` 有实测尺寸/取色）

原版规格：960×635、60 FPS、`PLUG_CHANNEL_IO "1-1 2-2"`、bundle `ORMAnalyzer`、fourcc `ORMA`、版本串 `0.1.0`。

## 1. 文件归属（并行时**只准改自己名下的文件**）

| 任务 | 文件 |
|---|---|
| DSP-STFT/VQT | `crates/openrm-analyzer-dsp/src/{stft.rs,vqt.rs,halfband.rs,stereo_scope.rs,test_signal.rs}` |
| DSP-PBT/RTA/Loudness | `crates/openrm-analyzer-dsp/src/{pbt.rs,rta.rs,loudness_meter.rs}`、`crates/openrm-analyzer-dsp/tests/loudness_parity.rs` |
| 参数层 | `plugins/analyzer/src/params.rs`、`plugins/analyzer/tests/param_parity.rs` |
| 频谱控件 | `plugins/analyzer/src/ui/{spectrum.rs,meter.rs}` |
| 示波器/声像 | `plugins/analyzer/src/ui/{scope.rs,field.rs,gonio.rs}` |
| 设置面板/i18n | `plugins/analyzer/src/ui/settings.rs`、`crates/openrm-ui/src/{strings.rs,theme.rs}`（只增不改语义） |
| 通用控件/总布局 | `plugins/analyzer/src/ui/{widgets.rs,layout.rs,cpu.rs}` |
| 集成（第二轮） | `plugins/analyzer/src/{lib.rs,editor.rs}`、`plugins/analyzer/examples/shot.rs` |

`crates/openrm-analyzer-dsp/src/lib.rs` 已声明全部模块，需要加 `pub use` 时找 OrganizeAgent。
`plugins/analyzer/src/ui/mod.rs`、`src/lib.rs`、`ui/data.rs` 同样由 OrganizeAgent 维护。

## 2. 控件约定（照 bandpass/editor.rs 的 retained 风格）

每个控件是一个持有绝对 `egui::Rect` 的 struct，自己画自己，坐标常量来自原版同名常量：

```rust
pub struct SpectrumPad { pub rect: egui::Rect, /* 原版成员等价字段 */ }
impl SpectrumPad {
    pub fn new(rect: egui::Rect) -> Self;
    pub fn set_sample_rate(&mut self, sr: f64);
    // ...原版 kMsgTag* 一一对应
    pub fn update(&mut self, frame: &crate::ui::data::SpectrumFrame); // 等价 OnMsgFromDelegate(kUpdateMessage)
    pub fn draw(&mut self, painter: &egui::Painter, theme: &openrm_ui::Theme, locale: openrm_ui::strings::Lang);
    pub fn click(&mut self, pos: egui::Pos2) -> Option<Click>; // 内部命中测试，返回语义动作
}
```

- 命中测试由控件自己做（原版 `OnMouseDown` 的等价），编辑层只把 `pos` 转进来。
- 字号统一走 `openrm_ui::fonts::{regular,semibold,bold}(px)`；原版 iPlug2 的 px 已含 0.8× 换算。
- 颜色走 `openrm_ui::Theme`（HSB 实时计算），不可写死 RGB；原版语义色除外：
  `MeterYellow(232,173,40)`、`MeterRed(226,60,52)`、`MeterGreen(96,186,96)`、`MeterOverLed`。
  Analyzer 专有的 OKLCH→sRGB、`MeterChromaScale`(sat 0/15/30/50 → 0/0.55/0.80/1) 放在 `ui/meter.rs`。
- 网格线：浅色主题 `(255-v)*0.18` 提亮，深色 `30+(255-v)*0.40` 且 sat×0.5。

## 3. 数据契约（`plugins/analyzer/src/ui/data.rs`）

引擎只产出**线性幅度**，UI 负责 dB（20log10）、attack/release 平滑、峰值保持——与原版一致。

- `SpectrumFrame{l,r,m,freqs}`：FFT 模式长度 = `fft_size/2`；VQT/PBT/RTA 长度 = 带数，`freqs` 为带中心 Hz。
- `ScopeFrame{l,r}`：每次 hop（1024）一对。
- `LevelSnapshot`（已完成）、`LoudnessSnapshot` 字段固定为：
  `momentary, short_term, integrated, range, lra_min, lra_max, tp_max: f32; i_valid, lra_valid: bool`
  （默认值 -120/-120/-120/0/-120/-120/-120/false/false）。

## 4. 硬性数值档位（已从源码核实，不得近似）

- Speed：LOG 释放 `4 / 2 / 1 / 0.5 / 0.2 s`；LIN `9.6 / 4.8 / 2.4 / 1.2 / 0.5 s`
- Res：`2048 / 4096 / 8192`；PBT LfRes：`40 / 20 / 10 Hz`；RTA `1/6, 1/12, 1/24`（59/118/236 带）
- VQT gamma：`20 / 10 / 5 Hz`；Hold：`0.5 / 2 / ∞`；Loud target：`-9 / -14 / -23 / -24 LUFS`；Loud scale：`+9 / +18 LU`
- Slope：FFT `0 / 3 / 4.5 dB/oct`；VQT/PBT/RTA `-3 / 0 / 1.5 dB/oct`（单位是原版有意设计，**保留**）
- 模式：`STFT / VQT / PBT / RTA`；声道 `PWR / LR / SUM`；电平 `dBTP / dBFS`

## 5. 布局（960×635，全部为逻辑像素）

- 右栏：`kCol1X=784`、`kBtnW=76`、`kBtnGap=4`、`kCol2X=864`、`kPanelR=940`
- 顶行：`kTopBtnW=62`、`kTopBtnH=26`、`kTopBtnY=26`、gap 6；
  ChanMode `20..82`、Mode `88..150`、Res `150..181`、Window `187..249`、ReleaseMode `255..317`、Slope `323..439`、Speed `445..507`
- 主频谱 `IRECT(20,58,784,328)`；下半按钮行 `y=334 h=26 gap=6`；下半区 `top=366`；
  `kBottomMidX=398`；`kScopeB=599`
- 右栏（由 `kScopeB` 反推）：图标行 `y=499..525`、hold `469..495`、freeze `439..465`、CPU `409..435`；
  `kIconBtnW=36`，四图标 x = `784/824/864/904`；`ORM` 标题 `y=533..567`（gear 紧随实测宽度）、
  `Analyzer` 标题 `y=565..599`、版本号 `y=530..564`
- 下半区：声像面板 `IRECT(20,366,394,599)`；示波器 `IRECT(400,366,kScopeR,599)`；
  `kScopeR = 频谱右缘(784) - 200(meter strip) ...` 按 `Analyzer.cpp` 实际算式在 `ui/layout.rs` 落地，不许估
- 声像按钮：FIELD/VECTOR `x 20..96`、PERS OFF/S/L `102..188`、x1/x2/x4 `194..246`；
  示波器声道按钮从 `kBottomMidX+2+kSliderW` 起宽 34，随后 ROLL/SYNC/SWEEP 宽 62
- 按钮 `kRowH`、`kBotBtnH` 等在 `Analyzer.cpp` 原文取，`ui/layout.rs` 单一来源

## 6. 质量纪律

- 禁止占位/糊弄：不许 `todo!()`、不许“近似配色”、不许空实现；每个控件必须能画出真实图形。
- 每个文件改完保持 `~/.cargo/bin/cargo check -p orm-analyzer`（DSP 侧 `-p openrm-analyzer-dsp`）通过；
  别人的文件报错就只报告、不代改。
- 数值结论必须真算：对照原版 C++ 断言或测试，工作区里给证据（测试名/输出）。
- 原版明显 bug 记录后上报，不擅自修；SLOPE 单位保留。
- 完成后在回复里给出：改动文件绝对路径、cargo 检查结果、对照依据（原版文件行号/截图区域）。
