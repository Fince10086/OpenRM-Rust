# OpenRM-Rust

ORM 系列音频插件的 Rust 重写仓库，插件框架用 [truce](https://truce.audio)。

与 C++ 版 `../OpenRM`（iPlug2）并行存在、独立管理。C++ 仓库继续作为算法参考与
回归基准，不参与本仓库构建。

当前范围：

| 插件 | 状态 | 说明 |
|---|---|---|
| BandPass | 已复刻（DSP + 界面 + 交互） | 与原版 916×646 界面逐控件对齐，见下方对照 |
| Fusion | 已并入（DSP 全量 + 新界面） | 原 `GRM-Rust/plugins/fusion` 迁入，DSP 与 11 个 parity 测试保留；界面按 BandPass 设计语言重做 |
| Analyzer | 已复刻（DSP 四引擎 + 界面 + 交互） | 960×635 界面，STFT/VQT/PBT/RTA 四套引擎 + 电平/响度/声场显示，见下方对照 |
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

对照截图：`docs/screenshots/bandpass-m6.png`（当前渲染，
`cargo run -p orm-bandpass --example shot` 生成 `target/bandpass-ui.png`）；
`docs/screenshots/bandpass-m2.png`、`bandpass-m3.png` 保留早期骨架。
`shot` 自己建 egui Context 并跑两帧，因为 egui 的字体定义要到下一帧才生效，
truce 自带单帧截图会把三档字重退化成 Regular。

### 与原版的差异（有意修正）

- **增益范围**：C++ 版界面允许 +12 dB，但核心把线性增益 clamp 到 0..2（实际最多
  +6.02 dB）。Rust 版把上限改为 `10^(12/20) = 3.981`，界面数值与实际增益一致。
- **右垫频谱**：C++ 版右垫“干信号”误取左声道（`mSpecInL`），Rust 版改为取右声道本身的
  `inR`，左右频谱各自独立。
- `SLOPE` 值单位固定为 `dB/oct`，不随语言变化——这是原版有意设计，保留。
- 频谱显示依赖真实音频输入；无信号时不绘制曲线，与原版一致。

## Analyzer 复刻范围

- **DSP / 引擎**：`openrm-analyzer-dsp` crate 只保留算法，四套引擎对外走统一的
  `EngineFrame` 契约（每 hop 一帧、线性幅度，dB 换算与弹道平滑在 UI 侧）。
  - **STFT**：Res 低/中/高 = 2048/4096/8192；窗 SHARP（Hann）/ CLEAN（5 阶
    Blackman-Harris）；斜率 0 / 3 / 4.5 dB/oct。
  - **VQT**：24 带/倍频程，gamma 低/中/高 = 20 / 10 / 5 Hz；窗 SHARP / CLEAN；
    斜率 -3 / 0 / 1.5 dB/oct。
  - **PBT**：低频分辨率低/中/高 = 40 / 20 / 10 Hz；斜率 -3 / 0 / 1.5 dB/oct，
    三档 49 / 57 / 65 带。
  - **RTA**：1/6、1/12、1/24 倍频程（59 / 118 / 236 带）；斜率 -3 / 0 / 1.5 dB/oct。
  - 声道合并 PWR（能量和）/ L/R 立体声 / SUM（信号和）；电平表 dBTP 真峰值 /
    dBFS+RMS，VU 表常驻；峰值保持 0.5s / 2s / ∞，含 over 锁存与 persist 余辉；
    响度计按 BS.1770-4 / EBU R128 出 M / S / I / LRA / true peak；内置测试信号
    发生器（类型 / 频率 / 电平 / 锁相位 / 送输出 / 重开 / 换种子）。
- **界面**：960×635 逻辑像素（2x 截图 1920×1270）。顶行 ChanMode/Mode/Res/
  Window/ReleaseMode/Slope/Speed 按钮，Range/LfRes 等为对应档位的循环控件；
  主频谱框 + 右侧电平条列（dBTP / VU / LUFS / LRA 分区）；下半声像卡（FIELD
  频域 PAZ 扇形 / VECTOR 时域李萨如散点，相关度 + 平衡条，矢量余辉
  OFF/SHORT/LONG、倍率 x1/x2/x4）与示波器卡（ROLL / SYNC / SWEEP，声道 M/L/R，
  时基 10ms–2s，幅度 1x–8x）；右栏 CPU、FREEZE、RESET、HOLD、2s、
  undo/redo/save/load 图标与标题区；设置面板含语言、主题（深/浅）、色相、
  饱和度四档与发生器、驱动/设备选择区。
- **参数**：共 **21** 个参数（以原版 `kNumParams` / `EParams` 枚举为准，id 0..=20；
  早期背景材料写的“22 个”与枚举不符）。Speed 五档 × LOG/LIN 释放、Range
  -80/-100/-120 dB、四引擎各自斜率与窗档、RTA 分辨率、VQT gamma、响度预设
  -9 / -14 / -23 / -24 LUFS、响度刻度 +9 / +18 LU；档位 → 引擎单位的换算只在
  `params.rs` 出现一次。
- **数据 / 落盘**：`.orm` 状态文件魔数 `analyzer`，JSON `version=1` + 21 项
  `values` 数组；设置落盘 `~/Library/Application Support/OpenRM/ORMAnalyzer.settings`
  （语言 / 主题 / 色相 / 饱和度 / hold / 发生器配置），关闭设置面板即写回。
- **i18n**：中英双语 94 条（Analyzer 专有 71 + common 共用 23），系统语言探测沿用
  BandPass 口径；同英文不同义的歧义条目（HOLD 保持 / 锁相位）单列显式取词。
- **冻结回放**：冻结环形缓冲 `1 << 19`（≈10.92 s @48 kHz），每 tick 回放 16 帧
  历史 hop 包，分析 hop = 1024；冻结中切引擎用新算法重分析同一段冻结音频。

### 实测数值（DSP 离线 parity）

以下为 `cargo test -p openrm-analyzer-dsp` / `-p orm-analyzer` 离线用例的实测输出，
黄金表由原版 C++ 头文件 + WDL fft.c 导出：

| 项目 | 实测值 | 对应用例 |
|---|---|---|
| 单声道 1kHz 0 dBFS 综合响度 I | **-3.0036 LUFS**（校准点 -3.01，差 0.0064） | `loudness_parity::loudness_calibration_and_msi` |
| 立体声等幅 1kHz 0 dBFS I | **+0.0067 LUFS**，M/S/I 一致 | 同上 |
| 12 LU 线性（-8 vs -20 dBFS） | 响度差 **12.0000 LU** | `loudness_parity::loudness_level_linearity` |
| LRA 双电平节目 | **16.0000 LU**（端点 -26.0000..-10.0000）；恒电平节目 0.0000 | `loudness_parity::loudness_lra_two_level_and_flat` |
| 电平峰值（0.5 幅度正弦） | peak L/R **-6.0206 dBFS**，true peak **-6.0218 dBTP** | `level_meter_parity` |
| M/L 幅度比 | **1.4142**（L 带峰 0.65789，M 带峰 0.93040，即 √2） | `audio_smoke::frame_cadence_and_pwr_lanes` |

| STFT bin 中心正弦峰 | **1.4142135**（N=4096/8192 黄金值 1.41421353817） | `stft_parity::sine_on_bin_center_matches_cpp` |
| VQT 带表 | **9 组**（44.1/48/96 kHz × gamma 20/10/5 Hz）× **240 带**，逐带中心频率/分层/窗长/读出偏移/归一化/核系数匹配黄金表 | `vqt_parity::band_table_matches_cpp_all_rates` |

对照截图：`docs/screenshots/analyzer-m1.png`（当前渲染，
`cargo run -p orm-analyzer --example shot` 直接写入该路径，2x 1920×1270；
例程自建 egui Context，先跑 40 帧预热让字体两阶段安装完成、控件滑窗与平滑收敛到
稳态，最后一帧出图；renderer 跨帧存活并逐帧喂 textures_delta）。
对照基准 `docs/screenshots/analyzer-original/01-main-default.png`。逐像素比对：

```bash
python3 tools/pixel_diff.py \
    --ref  docs/screenshots/analyzer-original/01-main-default.png \
    --test docs/screenshots/analyzer-m1.png \
    --out  docs/screenshots/diff/analyzer-m1-static/ \
    --ignore-region 16,62,744,268 \
    --ignore-region 16,358,684,244 \
    --tolerance 3 --metric channel --grid 32 --top 20
```

静态掩码口径：差异 21,254 / 973,248 = **2.184%**，均值色差 1.739，中位 0；
全量（无遮盖）口径：172,196 / 2,438,400 = **7.062%**，均值色差 2.314，中位 0。

### 与原版的差异（已知，未擅改）

- **FIELD 径向渐变**：epaint 0.34 没有径向渐变图元；按原版 8-stop alpha 表，用
  「外轮廓环 + 中环 + 中心顶点」三层扇形 Mesh 逼近（`ui/field.rs`）。
- **hover 与动态内容**：离屏 shot 无交互、动画时间置 0，频谱/示波器悬停叠层与
  按下瞬时反馈不进入截图；动态 mock 内容在 diff 中按区域遮盖。
- **字形抗锯齿**：egui/wgpu 与 iPlug2/macOS 的文本光栅化路径不同，静态掩码中位
  色差为 0，残留 2.18% 落在字形边缘抗锯齿像素。
- **mUIOpen 挂起分析未实现**：原版编辑器开合时置位/清零 `mUIOpen`，关闭期间挂起
  分析；truce 外壳没有 editor 开合钩子，Rust 侧始终在音频线程推进引擎。
- **SetDriver / SetAudioDevice 待宿主 APP_API**：面板内驱动与输入/输出设备选择只
  更新本地状态，设备枚举与实际打开待宿主集成阶段接入，未伪造效果。
- **hop 相位由外壳自跟踪**：`hop_phase = (phase + n) % HOP`，不读引擎内部
  `buf_count`；冷启动同步归零、四引擎 hop 恒为 1024（STFT 三档 overlap 2/4/8，
  VQT/PBT/RTA `K_HOP=1024`）前提下两口径等价；冻结回放分支不推进相位，与原版一致。
- **原版 LIN 速度 tooltip 文案与常量不一致**：`Strings.h` 写「LIN ≈ ×1.2
  （4.8/2.4/1.2/0.6/0.25 s）」，`kSpeedReleaseLin` 实际为 {9.6, 4.8, 2.4, 1.2,
  0.5}；属原版文档笔误，文案照抄未改。

## 仓库结构

```
OpenRM-Rust/
├─ Cargo.toml               workspace
├─ truce.toml               truce 的插件清单（vendor + [[plugin]]）
├─ crates/
│  ├─ openrm-ui/            egui 主题 / 字体 / 自绘控件 / 中英文案
│  ├─ openrm-dsp/           纯 Rust DSP 核心，零框架依赖，可离线测试
│  │  ├─ src/svf.rs         TPT 状态变量滤波器（对应 common/dsp/SvfFilter.h）
│  │  ├─ src/random_walk.rs 平滑随机游走（对应 common/dsp/RandomWalk.h）
│  │  ├─ src/bandpass.rs    BandPass 核心（对应 BandPass/src/dsp/BandPassCore.h）
│  │  └─ src/spectrum.rs    4096/4 STFT + 256 对数段聚合（对应 SpectrumSTFT.h）
│  └─ openrm-analyzer-dsp/  Analyzer 纯算法层：STFT/VQT/PBT/RTA/半带/电平/响度/声场/发生器
└─ plugins/
   ├─ bandpass/             truce 插件：参数、块边界、编辑器、.orm/.settings IO
   └─ analyzer/             truce 插件：21 参数、共享分析通道、界面装配、离屏 shot 例程
```

分层原则沿用 C++ 版：**算法与框架解耦**。`openrm-dsp` 不知道 truce 的存在，
插件 crate 只做参数换算、块边界和 UI。这样 Analyzer、将来的 CombFilter 可以
直接复用同一层，回归测试也不需要拉起插件框架。

## Fusion（并入与重做）

Fusion 的 DSP 来自原 `GRM-Rust/crates/grm-fusion-dsp`，迁移时只改了 crate 名
（`openrm-fusion-dsp`），FFT/重叠/频段/反馈/延迟语义与 11 个 `fusion_parity` 测试
原样保留。插件壳改名 `orm-fusion`，bundle id `orm.fusion`，fourcc `ORMF`。

界面没有沿用 GRM 的原始排版，而是与 BandPass 共用同一套 916×646 外壳：

- **共用组件**（`crates/openrm-ui/src/shell.rs`）：预设标题、4×4 预设槽、底部
  quick 1–8、fade 轨道、撤销/重做/保存/读取图标行、ORM 标题区、设置卡片
  （语言/主题/色相/饱和度 + 可选扩展区），以及 `value_row` / `compact_bar`。
- **共用数据**（`crates/openrm-ui/src/preset.rs`）：24 槽 `PresetBank`（9–24 网格 +
  1–8 quick、拖动交换编号）、100 步 `History`、预设切换的 `Fade`。
- **共用字体**：Outfit 三档（Latin）+ OPPO Sans 4.0 三档（GB2312 中文子集），
  `Mixed` 缺字不再出现方框；字号取整像素以改善中文抗锯齿。
- Fusion 自己的部分：8 个 band 开关、时间×频率平面（X=时间、Y=频率，拖动中心改
  时间/频率）、顶部 中心/延迟/长度/角度/幅度 读数、底部 幅度/反馈/声像/宽度
  四个滑块、共用的 RANDOM（范围/速度/取色）、贴平面右缘的增益条、混合、FFT
  尺寸设置（256…131k）。
- 数据与 BandPass 对齐：`.orm` 24 槽预设表、`~/Library/Application Support/OpenRM/ORMFusion.settings`、
  Cmd+单击存槽、右键存档/恢复默认、quick/fade 吸附、撤销重做。

截图：`docs/screenshots/fusion-m3.png`（8 段全开）、`fusion-m3-dark.png`（深色主题）。
BandPass 的当前渲染在 `docs/screenshots/bandpass-m6.png`。

## 构建与测试

```sh
cargo test                     # DSP 回归 + 插件端到端冒烟
cargo test -p orm-bandpass --no-default-features   # 只跑逻辑，不编译格式封装

cargo install cargo-truce      # 一次性
cargo run -p orm-bandpass --example shot   # 渲染 BandPass 界面截图（target/bandpass-ui.png）
cargo run -p orm-analyzer --example shot   # 渲染 Analyzer 界面截图（docs/screenshots/analyzer-m1.png）
cargo truce run                # 独立运行，不需要 DAW
cargo truce install --clap     # 装到 ~/Library/Audio/Plug-Ins/CLAP
cargo truce install --vst3     # 装到 ~/Library/Audio/Plug-Ins/VST3
cargo truce install --vst3 --au2 -p orm-bandpass   # VST3 + AU2
cargo truce install --vst3 --au2 -p orm-fusion     # VST3 + AU2
auval -v aufx ORMB OpRM        # BandPass AU 校验
auval -v aufx ORMF OpRM        # Fusion AU 校验
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

Analyzer 已完成移植；下一步是 CombFilter，等 C++ 版定稿后直接用 Rust 重写。
