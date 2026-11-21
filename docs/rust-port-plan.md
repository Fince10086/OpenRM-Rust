# ORM 系列插件的 Rust + truce 迁移方案

面向实现者。范围：BandPass（完整迁移）、Analyzer（完整迁移）、CombFilter（重做）、
Narrator 暂不迁移。

---

## 一、可行性判断

现有 C++ 代码已经把算法和 iPlug2 拆开了：`BandPassCore`、`CombFilter`、Analyzer 的
各分析器都只依赖 `plugins/common/dsp`，不知道 iPlug2 的存在。这意味着 Rust 重写
的主体工作不是"重新实现算法"，而是"换一层壳"：

- 算法层：逐文件翻译成 Rust（可验证，改动可控）；
- 框架层：iPlug2 的 `Plugin` / `IGraphics` 换成 truce 的 `PluginLogic` / egui；
- 产品层：预设槽、主题、设置面板、i18n 这些自研 UI 逻辑需要重写，但不涉及算法。

已经用 BandPass 验证过一轮：Rust 核心在 fs=48000、block=512 下跑 C++ 自测的全部
断言，`-3dB BW`、滚降、随机调制增益、Reset 后游走幅度与 C++ 实测一致（见根 README
对照表），只剩 f32 累加顺序带来的末位噪声。

因此建议的顺序是：**BandPass 全链路打通 → Analyzer DSP → Analyzer UI → CombFilter
重做**。BandPass 体量小、已 100% 完成、有现成回归测试，最适合当"框架样板"。

---

## 二、目标架构

```
OpenRM-Rust/
├─ crates/
│  ├─ openrm-dsp/     纯算法，零依赖：svf / random_walk / bandpass / (后续) stft / loudness
│  └─ openrm-ui/      (待建) egui 公共件：主题 token、旋钮、滑块、字体、i18n
├─ plugins/
│  ├─ bandpass/       truce 插件 crate
│  └─ analyzer/       (待建)
└─ truce.toml         vendor + 每个 [[plugin]] 的元数据
```

三条边界，和 C++ 版完全对应：

| 层 | 职责 | 约束 |
|---|---|---|
| `openrm-dsp` | 滤波、调制、FFT、响度 | 不依赖 truce / egui，可单测、可被任何插件复用 |
| `plugins/*` | 宿主参数 ↔ 核心参数换算、块边界、线程边界、编辑器 | 只引用 `openrm-dsp` + truce + `openrm-ui` |
| `openrm-ui` | 视觉与交互组件 | 只认 `Arc<Params>` 与共享快照，不碰 DSP 状态 |

---

## 三、框架映射

| iPlug2 | truce | 备注 |
|---|---|---|
| `Plugin` 子类 + `MakeConfig` | `impl PluginLogic` + `truce::plugin!` | 参数表由 `#[derive(Params)]` 生成 |
| `GetParam(i)->Value()` | `FloatParam::raw_target()` / `EnumParam::index()` / `BoolParam::value()` | 块起始取一次快照 |
| `OnParamChange` + `ParamMailbox` | 每块 `set_params()` | 参数已由框架原子发布，不再需要 mailbox |
| `ProcessBlock(sample**, …)` | `process(state, params, buffer, events, ctx)` | `AudioBuffer` 是通道切片 |
| `OnReset` | `reset(state, params, config)` | `config.sample_rate` / `max_block_size` |
| `IPlugQueue` / `ISender` | `AudioTap` + `StreamWorker` | FFT、电平分析移到工作线程 |
| `std::atomic` 标量快照 | 同上（`#[skip]` 字段里的 atomics） | 音频线程写、UI 线程读 |
| `IGraphics` 自绘控件 | egui `Painter` / `Response` | 内置栅格 UI 不支持自由绘制 |
| `OnIdle` | `Editor::idle`（帧回调） | egui 每帧自动重绘 |
| `SerializeState` / 自研 `.orm` 预设 | `snapshot_into` / `load_state` + 自研预设文件 | 见第六节 |
| APP / AU / VST3 目标 | CLAP / VST3 / AU v2 / AU v3 / LV2 / AAX / standalone | 由 Cargo feature 决定 |

**精度**：C++ 版 `plugins/BandPass/CMakeLists.txt` 定义了 `SAMPLE_TYPE_FLOAT`，宿主
样本本来就是 f32，所以用 truce 默认 `prelude`（f32），不做 `prelude64` 的块边界
加宽，数值路径与 C++ 完全同构。

**平滑**：C++ 的 15 ms 一阶平滑在 `BandPassCore::updateSmoothing()` 里，是算法的一部分。
迁移时保留它，truce 侧参数声明 `smooth = "none"`，否则会二次平滑、改变手感和 A/B 结果。
将来若想用 truce 的逐样本平滑（`log(15)`），必须同时删掉核心里的 `updateSmoothing`，
并重跑回归测试，不能两边都开。

---

## 四、BandPass 迁移细节

### 4.1 参数

34 个参数，id 固定为原 `EParams` 序号（0..33），这样旧 `.orm` 预设与自动化映射不需要
重排。truce 侧的类型与范围：

| id | 字段 | 类型 / 范围 | → 核心 |
|---|---|---|---|
| 0/3 | `freq_l` / `freq_r` | `log(20, 20000)` Hz，默认 1095.445 | Hz |
| 1/4 | `bw_l` / `bw_r` | `log(1, 31)` 倍率，默认 3.65 | `2*log2(m)` octave |
| 2/5 | `gain_l` / `gain_r` | `linear(-96, 12)` dB | `10^(dB/20)`，核心再 clamp 到 0..2 |
| 6 | `link` | bool | 镜像 R←L |
| 7 | `mix` | `linear(0, 1)` | 等功率交叉淡化 |
| 8/9 | `random_amount_r` / `random_rate_r` | 0..1 / `log(0.01, 60)` s | 红色槽 |
| 10/11 | `slope_l` / `slope_r` | 枚举 4 档，默认 96 dB/oct | 档位→dB/oct |
| 12–18 | 7 个 `random_color_*` | 枚举 Red/Yellow/Blue/Green | 选择 amount/speed 槽 |
| 19–25 | 7 个 `random_enable_*` | bool | 7 路随机流开关 |
| 26–31 | 黄/蓝/绿三组 amount+rate | 同红色槽 | |
| 32/33 | `pass_l` / `pass_r` | bool，默认 true | true=带通，false=带阻 |

单位换算集中在 `plugins/bandpass/src/params.rs::to_core()`，与 C++ `CollectParams()`
一一对应，不要散落在 `process()` 里。

### 4.2 处理块

`process()` 只做四件事：取参数快照 → `set_params` → `update_smoothing(n)` →
`process_stereo`。输入先拷进 `DspState` 的 scratch（和 C++ 为频谱另存一份输入的做法
一致），输出再拷回 `buffer.output(ch)`。等频谱/波形显示接上后，这份输入副本正好是
干信号来源，不需要再拷一次。

### 4.3 界面

C++ 界面共约 2600 行自绘代码，全部需要重写：

| 组件 | 行数 | 迁移做法 |
|---|---|---|
| `FilterNodePad` | 719 | egui 自绘：对数频率轴、干/湿频谱填充、节点拖拽、带宽双角、斜率菜单、Pass 开关、随机幽灵点 |
| `BandRangeSlider` | 242 | 自绘水平滑块（对数刻度 + 带宽括号） |
| `ORMSlider` / `GainSlider` | 333 | 统一成 `openrm-ui` 的旋钮/滑块组件 |
| `FlatButton` / `FlatCycleButton` | 419 | egui `Button` + 自绘样式 |
| `PresetSlotControl` / `PresetFadeSlider` | 207 | 24 槽网格 + 拖放 + 渐变滑杆 |
| `SettingsPanelControl` | 466 | 侧滑面板：语言、主题色相、随机色配置、关于 |
| `Theme` | 182 | 主题 token（HSB 色相/饱和、明暗模式）平移为 Rust 结构 |
| `RandomColorPickerControl` | 41 | 四色选择器 |

结论：**必须用 egui 后端**。truce 内置栅格只有 knob/slider/toggle/dropdown/meter/xy_pad，
没有自由绘制能力，做不了频谱垫和拖拽节点。内置 UI 只保留为里程碑 1 的临时界面
（当前仓库里就是它）。

字体：C++ 版把 `Mixed-Regular/SemiBold/Bold.ttf` 拷进 bundle；Rust 版用
`include_bytes!` 嵌进二进制，省掉 Resources 目录与部署顺序问题（C++ 版为此专门
绕开了 iPlug2 的自动部署）。

### 4.4 频谱链路

C++：音频线程逐样本写入 `SpectrumSTFT` 的 hop 缓冲，满 hop 后经 `ISender` 把整块
样本推给 UI 线程，UI 线程做 WDL 实数 FFT、取模、画曲线。

Rust：音频线程把干/湿 4 通道（`L_in, R_in, L_wet, R_wet`）交错写入
`AudioTap::new(32k, 4)`；专用 `StreamWorker` 负责重采样对齐、窗函数、FFT
（`realfft`），结果按 bin 写进原子数组（`AtomicU32` 存 `f32::to_bits()`），
egui 每帧读一次。

必须一起搬过去的三件事：

1. **窗函数**：BandPass 固定 4 阶 Blackman-Harris，Analyzer 可切 Hann / 5 阶 BH；
2. **标定档位**：BandPass 用 `kCalHalf`（满幅正弦读 -3 dB），Analyzer 用
   `kCalDouble`（读 +3 dB），这个 ±3 dB 是产品既定外观，不能"顺手统一"；
3. **重叠**：BandPass 4096/4，Analyzer 分辨率可调（2048/4096/8192）。

---

## 五、Analyzer 迁移细节

### 5.1 DSP

| 模块 | 行数 | 迁移要点 |
|---|---|---|
| `SpectrumSTFT` + `HalfbandDec2` | 514 | STFT 换 `realfft`；半带抽取器按原系数搬 |
| `VQTAnalyzer` | 635 | 多速率金字塔 + 最小相位半带 + 群延迟查找表，纯数值代码，逐段搬 |
| `PBTAnalyzer` | 437 | 三分频带能量比，搬 |
| `RTAAnalyzer` | 325 | 1/6、1/12、1/24 倍频程滤波组，搬 |
| `LevelMeter` | 316 | dBTP 4× 过采样真峰值、RMS、VU、hold，搬 |
| `LoudnessMeter` | 318 | BS.1770-4 K 加权、M/S/I、双门限、LRA 百分位，搬 |
| `StereoScope` / `StereoCorr` | 211 | 方位角/弹道计算目前在 UI 线程，Rust 版放 worker |
| `TestSignalGenerator` | 336 | 开发者工具，用 feature 裁掉 |

优先级：STFT + LevelMeter + LoudnessMeter 先做（界面主视觉都依赖它们），
VQT / PBT / RTA 三套引擎随后，每套单独做离线对齐测试。

**校准是验收关键**：同一段粉噪/正弦，C++ 与 Rust 的频谱读数、LUFS、dBTP 要在
±0.1 dB 以内。C++ 有 `tests/loudness_test.cpp`，把它的输入与期望值直接搬成 Rust 测试。

### 5.2 线程模型

Analyzer 现在的电平/响度统计在音频线程算，UI 线程只读原子量。Rust 版建议：

- 音频线程：只 `tap.push_frames()` + `context.set_meter()` 这类 O(1) 操作；
- `StreamWorker`：STFT、VQT、电平、响度全部在这里跑；
- 发布：频谱 bin 原子数组 + 少量标量原子（LUFS/LRA/dBTP）；
- 代价：显示延迟一个 block（约 10 ms），可接受；但 **freeze / hold 曲线的状态要放在
  UI 侧**，否则 worker 一停就丢。

### 5.3 界面

`SpectrumPad` 1611 行、`OscilloscopeControl` 920 行、`SettingsPanelControl` 708 行、
`StereoFieldControl` 541 行、`GoniometerControl` 368 行。这是整个项目最大的一块，
建议拆成独立 crate `openrm-ui` 里的可复用面板，按图层实现（网格 → 曲线 → 电平条 →
LRA 括号 → HUD）。

---

## 六、状态、预设与版本共存

### 6.1 宿主会话状态

旧版 VST3/AU 的 state chunk 是 iPlug2 自己序列化的，truce 无法解析。两个选择：

1. **换 bundle_id**（推荐）：新插件用 `orm.bandpass.rs` 之类的新 id，两版可并存，
   宿主不会把旧 chunk 喂给新插件；用户用 `.orm` 预设文件迁移音色。
2. 沿用旧 id + 写 `migrate_state` 解析 iPlug2 chunk：工作量大、容易踩格式细节。

自动化映射不受影响：参数 id 沿用 0..33，宿主里重建自动化只是一次性成本。

### 6.2 `.orm` 预设文件

格式是"首行 magic + JSON，`presets` 是 24×N 的 double 数组"。建议**原样保留**：
Rust 侧用 `serde_json` 读写同一个结构，老用户的预设文件继续能打开。
这属于产品级数据，不要塞进 truce 的 `#[persist]`（那是给宿主存会话用的）。

truce 的 factory preset（`.preset` TOML）可以另外生成一份，用于新用户和跨格式
预设包，两套不冲突。

### 6.3 设置文件

`~/Library/Application Support/OpenRM/<Plugin>.settings`（语言、主题色相、随机色）
继续按原路径读写，保证用户升级后外观不变。只有"随工程走"的状态（比如当前选中的
预设槽）才考虑放进 `#[persist]`。

---

## 七、分阶段路线

| 阶段 | 内容 | 验收 |
|---|---|---|
| M1 ✅ | workspace + `openrm-dsp` BandPass 核心 + 移植回归测试 + truce 插件骨架（临时栅格 UI） | `cargo test` 全绿；`driver!` 端到端 1 kHz 增益 1.000、mix=0 直通 |
| M2 | BandPass 正式 UI（egui）：滤波垫、频谱、带宽滑杆、24 预设槽、设置面板、主题、i18n | 与 C++ 版逐屏对照；噪声/正弦听感 A/B 无差异；`pluginval` 通过；CLAP/VST3/AU 三格式装进 DAW 正常 |
| M3 | Analyzer DSP 移植：STFT、电平、响度 → VQT/PBT/RTA | 频谱/LUFS/dBTP 与 C++ 偏差 ≤0.1 dB；`loudness_test` 断言通过 |
| M4 | Analyzer UI | 四种分析模式、示波器、声场图、HUD 与 C++ 版一致 |
| M5 | CombFilter 重做 | 按新规格直接写 Rust，不再迁就半成品 |
| M6 | 打包与 CI | `cargo truce package`、签名/公证、安装器、GitHub Actions 矩阵 |

M2 是最大的人力项（自绘控件多），建议先把"主题 token + 旋钮/滑块/按钮"抽进
`openrm-ui`，再做滤波垫。

---

## 八、风险与已发现的问题

1. **truce 版本漂移**：6.3.0 与官网文档已有出入（文档里还留着旧的 `custom_editor`
   写法）。移植期建议 `truce = "=6.3.0"` 精确锁定，升级单独做一次并跑全部测试。
2. **truce 许可**：Truce License 1.0 = Apache-2.0 / MIT 双许可 + 一条"商业框架再分发"
   rider。作为插件作者不受影响，但仓库 `LICENSES/` 要加一份说明。
3. **增益上限 bug（C++ 版既有）**：界面允许 +12 dB，核心 `setParams` 把线性增益
   clamp 到 0..2，实际最多 +6.02 dB。迁移时要么修（改变现有用户听感），要么保留
   并记入文档。建议**先保留**，等 M2 时作为独立决策处理。
4. **参数平滑重复**：见 3.3，两边都开会让扫频手感变"糊"。
5. **FMA 收缩**：C++ 的 `-O2` 允许 FMA，Rust 默认不允许，窄带长时运行的混沌峰值
   会有差异（稳态响应一致）。不要为此开 `fast-math`；回归测试断言用稳态指标，
   不用逐样本 bit 相等。
6. **`AudioTap` 满环丢弃整帧**：频谱时间轴会出现空洞，需要按帧计数补零，
   否则 hold/平均曲线会被拉偏。
7. **AU v2 / AU v3 / AAX**：AU v2 需要 Xcode CLI tools，AU v3 需要完整 Xcode 与
   Developer ID 签名，AAX 需要 SDK + PACE。先把 CLAP + VST3 + standalone 跑通，
   AU 放到 M6。
8. **字体与许可**：OPPO Sans 是专有（免费使用）字体，嵌进二进制前确认再分发条款；
   西文 Outfit 是 OFL，没问题。
9. **Narrator 继续留在 C++ 仓库**：它和 OpenRM 主仓库的构建、许可、ROM 数据都绑在一起，
   不要为了"统一技术栈"顺手搬。

---

## 九、构建与发布

```sh
cargo truce build --clap --vst3      # 产出 bundle
cargo truce install --clap --vst3    # 装到用户目录，无需 sudo
cargo truce package                  # 打包（签名配置见 truce 文档）
```

- macOS 签名/公证走环境变量（`cargo truce` 读取 `[env]` 配置），不再需要
  `cmake/OpenRMPlugin.cmake` 里那套"先拷字体再 ad-hoc 签名"的顺序问题。
- 版本号单一来源可以继续放在 `truce.toml` 的 `[[plugin]] version`，或者各 crate 的
  `Cargo.toml`；不要两处都写。
- Windows 安装器（Inno Setup）在 M6 重新接到 `cargo truce package` 的产物目录。

---

## 十、待定的产品决策

1. BandPass 新版是**替换**旧插件，还是与旧版**并存**（影响 bundle_id，见 6.1）。
2. `.orm` 预设是否继续作为唯一预设格式，还是再提供 truce 原生 `.preset`。
3. 增益 clamp 是否修正（+12 dB 实际只有 +6 dB）。
4. egui 之外是否要给 Analyzer 更高帧率的自绘路径（当前 60 fps 足够，先不优化）。
