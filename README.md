# OpenRM-Rust

ORM 系列音频插件的 Rust 重写仓库，插件框架用 [truce](https://truce.audio)。

与 C++ 版 `../OpenRM`（iPlug2）并行存在、独立管理。C++ 仓库继续作为算法参考与
回归基准，不参与本仓库构建。

当前范围：

| 插件 | 状态 | 说明 |
|---|---|---|
| BandPass | 已复刻（DSP + 界面 + 交互） | 与原版 916×646 界面逐控件对齐，见下方对照 |
| Analyzer | 计划中 | 4 套分析引擎 + 电平/响度/声场显示，体量最大 |
| CombFilter | 后续用 Rust 重做 | C++ 版本为半成品 |
| Narrator | 不在范围内 | TTS 引擎与 ROM 数据继续留在 C++ 仓库 |

## BandPass 复刻范围

- **DSP**：`BandPassCore` 全量移植，参数平滑、随机游走、morph 叠加与 C++ 一致。
- **界面**：916×646 布局、`config.h` 主题色（HSB hue/sat/B 五档）、Mixed 三档
  字重（Regular / SemiBold / Bold，按 iPlug2 的 72dpi→pt 系数 0.8 缩放）、
  滤波垫 zebra/频谱/角标/BP·BR 胶囊/侧标/手柄、带宽滑杆、增益条、预设槽、
  MORPH/RANGE/SPEED/MIX、图标行（undo/redo/save/load）、quick 槽 1–8 与
  fade 轨道（8 刻度贯穿）。
- **交互**：滤波垫拖动/滚轮调带宽、双击复位、角标与低切/高切/增益/混合/
  MORPH 数值点击输入（支持 `1k`、`1kHz` 写法）、随机色块开关与右键选色、
  预设单击加载 / Cmd+单击保存 / 右键菜单 / 拖动 8px 交换、quick 槽与 fade
  吸附、撤销/重做（0.4s 手势窗口、100 步上限）、L→R / R→L / 同步（link）/
  翻转、24 个工厂预设、MORPH 按归一化空间插值（与原版 `MixSnapshots` 一致）。
- **数据**：`.orm` 读写的 24×34 预设表、`~/Library/Application Support/OpenRM/ORMBandPass.settings`
  （语言/主题/色相/饱和度），关闭设置面板即落盘。
- **i18n**：中英全量字符串 + 系统语言探测；16 处 tooltip 与原版一一对应。

对照截图：`docs/screenshots/bandpass-m4.png`（当前渲染，
`cargo run -p orm-bandpass --example shot` 生成 `target/bandpass-ui.png`）；
`docs/screenshots/bandpass-m2.png`、`bandpass-m3.png` 保留早期骨架。
`shot` 自己建 egui Context 并跑两帧，因为 egui 的字体定义要到下一帧才生效，
truce 自带单帧截图会把三档字重退化成 Regular。

### 已知与原版的差异 / 有意保留

- C++ 版界面允许增益 +12 dB，但核心 `setParams` 把线性增益 clamp 到 0..2
  （实际最多 +6.02 dB）。Rust 版按原版行为保留，待确认后修正。
- 右垫“干信号”频谱在原版取左声道（`mSpecInL`），Rust 版沿用，属原版 bug。
- `SLOPE` 值单位 `dB/oct` 不随语言变化，沿用原版。
- 频谱显示依赖真实音频输入；无信号时与原版一样不绘制曲线。

## 仓库结构

```
OpenRM-Rust/
├─ Cargo.toml               workspace
├─ truce.toml               truce 的插件清单（vendor + [[plugin]]）
├─ crates/
│  ├─ openrm-ui/            egui 主题 / 字体 / 自绘控件 / 中英文案
│  └─ openrm-dsp/           纯 Rust DSP 核心，零框架依赖，可离线测试
│     ├─ src/svf.rs         TPT 状态变量滤波器（对应 common/dsp/SvfFilter.h）
│     ├─ src/random_walk.rs 平滑随机游走（对应 common/dsp/RandomWalk.h）
│     ├─ src/bandpass.rs    BandPass 核心（对应 BandPass/src/dsp/BandPassCore.h）
│     └─ src/spectrum.rs    4096/4 STFT + 256 对数段聚合（对应 SpectrumSTFT.h）
└─ plugins/
   └─ bandpass/             truce 插件：参数、块边界、编辑器、.orm/.settings IO
```

分层原则沿用 C++ 版：**算法与框架解耦**。`openrm-dsp` 不知道 truce 的存在，
插件 crate 只做参数换算、块边界和 UI。这样 Analyzer、将来的 CombFilter 可以
直接复用同一层，回归测试也不需要拉起插件框架。

## 构建与测试

```sh
cargo test                     # DSP 回归 + 插件端到端冒烟
cargo test -p orm-bandpass --no-default-features   # 只跑逻辑，不编译格式封装

cargo install cargo-truce      # 一次性
cargo run -p orm-bandpass --example shot   # 渲染一张界面截图（target/bandpass-ui.png）
cargo truce run                # 独立运行，不需要 DAW
cargo truce install --clap     # 装到 ~/Library/Audio/Plug-Ins/CLAP
cargo truce install --vst3     # 装到 ~/Library/Audio/Plug-Ins/VST3
```

## 移植验证基线

`openrm-dsp` 的 `tests/bandpass_parity.rs` 复刻了 C++ `tests/dsp_test.cpp` 的
全部断言。同一 fs=48000 / block=512 下两侧实测值对照：

| 项目 | C++ | Rust |
|---|---|---|
| -3dB 带宽（12/24/48/96 dB/oct） | 631.8 / 705.0 / 707.1 / 707.1 Hz | 631.8 / 705.0 / 707.1 / 707.1 Hz |
| 高切外 1 oct 衰减 | -12.5 / -24.4 / -48.8 / -97.5 dB | -12.5 / -24.4 / -48.8 / -97.4 dB |
| 随机调制后再测增益（无/频率/增益/混音） | 1.414 / 0.787 / 45.251 / 1.311 | 1.414 / 0.787 / 45.251 / 1.311 |
| Reset 后随机游走最大偏移 | 4.728 oct | 4.728 oct |

差异只剩 f32 累加顺序与 FMA 收缩带来的末位噪声（-97.5 vs -97.4 dB）。

参数 id 固定沿用原 `EParams` 序号 0..=33，旧 `.orm` 预设与宿主自动化可以直接
迁移；参数平滑保留在 DSP 核心（15 ms 一阶），truce 侧声明 `smooth = "none"`，
避免二次平滑。

下一步是 Analyzer 的 Rust 版；CombFilter 等 C++ 版定稿后直接用 Rust 重写。
