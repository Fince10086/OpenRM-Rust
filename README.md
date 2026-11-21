# OpenRM-Rust

ORM 系列音频插件的 Rust 重写仓库，插件框架用 [truce](https://truce.audio)。

与 C++ 版 `../OpenRM`（iPlug2）并行存在、独立管理。C++ 仓库继续作为算法参考与
回归基准，不参与本仓库构建。

当前范围：

| 插件 | 状态 | 说明 |
|---|---|---|
| BandPass | 移植中（DSP 已完成并验证） | 首个打通全链路的插件 |
| Analyzer | 计划中 | 4 套分析引擎 + 电平/响度/声场显示，体量最大 |
| CombFilter | 暂不迁移 | C++ 版本为半成品，等重做时直接用 Rust 写 |
| Narrator | 不在范围内 | TTS 引擎与 ROM 数据继续留在 C++ 仓库 |

## 仓库结构

```
OpenRM-Rust/
├─ Cargo.toml               workspace
├─ truce.toml               truce 的插件清单（vendor + [[plugin]]）
├─ crates/
│  └─ openrm-dsp/           纯 Rust DSP 核心，零框架依赖，可离线测试
│     ├─ src/svf.rs         TPT 状态变量滤波器（对应 common/dsp/SvfFilter.h）
│     ├─ src/random_walk.rs 平滑随机游走（对应 common/dsp/RandomWalk.h）
│     └─ src/bandpass.rs    BandPass 核心（对应 BandPass/src/dsp/BandPassCore.h）
└─ plugins/
   └─ bandpass/             truce 插件：参数、块边界、编辑器
```

分层原则沿用 C++ 版：**算法与框架解耦**。`openrm-dsp` 不知道 truce 的存在，
插件 crate 只做参数换算、块边界和 UI。这样 Analyzer、将来的 CombFilter 可以
直接复用同一层，回归测试也不需要拉起插件框架。

## 构建与测试

```sh
cargo test                     # DSP 回归 + 插件端到端冒烟
cargo test -p orm-bandpass --no-default-features   # 只跑逻辑，不编译格式封装

cargo install cargo-truce      # 一次性
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

进一步的工作计划见 `docs/rust-port-plan.md`。
