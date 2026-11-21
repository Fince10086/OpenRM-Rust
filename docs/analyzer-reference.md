# Analyzer 原版视觉基线与像素对照基准

> 本文档是 C++/iPlug2 原版 ORM Analyzer 与 Rust/egui 复刻版之间的**视觉验收基准**。
> 所有几何、色值、字号结论均来自对原版真机截图的实测，测量脚本与 diff 工具见文末。
> 对照源码：`plugins/analyzer/src/ui/layout.rs`、`crates/openrm-ui/src/theme.rs`、
> `crates/openrm-ui/src/fonts.rs`、`plugins/analyzer/src/ui/meter.rs`、`widgets.rs`。

## 1. 基准截图与测量方法

### 1.1 截图规格

| 项 | 值 |
|---|---|
| 基准文件 | `docs/screenshots/analyzer-original/01-main-default.png` |
| 像素尺寸 | **1920 × 1270**（与 2x 基准 960×635×2 完全吻合，SCALE = 2.000） |
| 逻辑画布 | 960 × 635 逻辑像素 |
| 坐标换算 | 逻辑 `(x, y)` = 像素 `(x×2, y×2)`；逆运算 像素 / 2 |
| 主题 | Light，hue = 45，sat_max = 15（`Theme::default()`） |

- 截图第 0–1 行、第 0–1 列及底部外缘有 1px 窗口阴影/抗锯齿，内容区仍为整幅 1920×1270；
  测量时以逻辑坐标为准，边缘 ±1px 不视为布局偏差。
- 本截图为**默认静止态**：无音频信号，VU/LUFS 条为空轨，Meter 黄/红/绿固定色未出现；
  PWR、M 两个按钮处于激活态；FREEZE / HOLD 未激活。

### 1.2 测量方法

- **几何**：在 `layout.rs` 常量矩形外扩 2px 的窗口内做底色掩码（col_300 / col_900 / 实测激活色），
  按行列投影求紧包围盒；内容卡（频谱/下半卡片）用「与背景 col_100 的差异掩码」求内容占用框。
- **取色**：以逻辑坐标为中心取 3×3 逻辑邻域（2x 下 6×6 像素），取通道中位数；
  文字墨色取字形区域内深/亮像素的中位数，避开抗锯齿混合。
- **字号墨高**：在文本盒内做墨色行投影，统计有墨行数并换算回逻辑 px；
  与原版 `IText(px, …)` 名义字号、egui 字号（`×0.8` 缩放，见 `fonts.rs` `TEXT_SCALE`）对照。
- 复测命令与复现细节见 §7。

## 2. 几何实测表（逻辑坐标 x, y, w, h）

「layout.rs 常量」照抄 `Layout::new()`；「实测」来自截图掩码。Δ 为实测 − 常量。

### 2.1 顶行按钮（y = 26，h = 26）

| 控件 | layout.rs 常量 | 实测 | Δ | 结论 |
|---|---|---|---|---|
| chan_mode PWR（激活） | 20, 26, 62, 26 | 20, 26, 62, 26 | 0, 0, 0, 0 | 一致 |
| mode STFT | 88, 26, 62, 26 | 88, 26, 64, 26 | 0, 0, +2, 0 | 相邻按钮共用边界，检测渗入 ±2px（见 §5.1） |
| res M（顶行） | 150, 26, 31, 26 | 148, 26, 33, 26 | −2, 0, +2, 0 | 同上，非真实偏移 |
| window_btn SHARP | 187, 26, 62, 26 | 187, 26, 62, 26 | 0 | 一致 |
| release_mode LOG | 255, 26, 62, 26 | 255, 26, 62, 26 | 0 | 一致 |
| slope 3 dB/Oct | 323, 26, 116, 26 | 323, 26, 116, 26 | 0 | 一致 |
| speed MAX | 445, 26, 62, 26 | 445, 26, 62, 26 | 0 | 一致 |

### 2.2 主频谱框与电平条列

| 控件 | layout.rs 常量 | 实测 | 结论 |
|---|---|---|---|
| plot 主频谱框 | 20, 58, 764, 270（右缘 784，下缘 328） | 内容占用 20, 58, 732.5, 271 | 左/上/下一致；发丝边框很浅，右缘含电平条列 |
| 频谱 Pad 右缘（不含电平条） | 584 | 584 | 一致 |

电平条列（METER_STRIP_W = 200，y 58–328）逐列实测（y = 200 取样）：

| 分段 | 常量宽 | 实测 x 区间 | 轨底色 |
|---|---|---|---|
| 增益条 ×2（dBTP） | 14 + 14 | 584 – 612 | col_300 空轨；填充为 OKLCH 绿→青渐变 |
| VU 刻度列 | 28 | 612 – 640 | col_100 背景 |
| VU 条 ×2 | 14 + 14 | 640 – 668 | col_300 空轨 |
| LUFS 刻度列 | 28 | 668 – 696 | col_100 背景 |
| 响度条 ×4 | 14 × 4 | 696 – 752 | col_300 空轨 |
| LRA 区 | 32 | 752 – 784 | col_100 背景 |

### 2.3 下半区按钮行（y = 334，h = 26）

| 控件 | layout.rs 常量 | 实测 | 结论 |
|---|---|---|---|
| scope_mode FIELD | 20, 334, 76, 26 | 20, 334, 76, 26 | 一致 |
| gonio_persist PERS | 102, 334, 86, 26 | — | 默认声像态无底色块（幽灵/隐藏，见 §5.3） |
| gonio_zoom ZOOM | 194, 334, 52, 26 | — | 同上 |
| scope_chan M（激活） | 420, 334, 34, 26 | 420, 334, 34.5, 26.5 | ±0.5px，一致 |
| scope_trig SYNC | 460, 334, 62, 26 | 460, 334, 62, 26 | 一致 |

### 2.4 下半两张卡片（y 366 – 599）

| 控件 | layout.rs 常量 | 实测内容占用框 | 结论 |
|---|---|---|---|
| scope_panel 声像卡 | 20, 366, 374, 233 | 24.5, 371, 364, 213.5 | 外框常量保留；内容相对边框内缩约 5/5/10/20px |
| oscilloscope 示波卡 | 400, 366, 296, 233 | 418, 368, 276, 211.5 | 同上，内缩约 18/2/20/21px |

> 卡片边框为 col_300 发丝线 + 内边距，截图中不可直接分辨；diff 验收时卡片**外框位置**必须一致，
> 卡片内部内容迹线按 §6 允许差异处理。

### 2.5 右栏（x 784 – 940）

| 控件 | layout.rs 常量 | 实测 | 结论 |
|---|---|---|---|
| cpu 5.18% | 784, 409, 156, 26 | 784, 409, 156, 26 | 一致 |
| freeze | 784, 439, 76, 26 | 784, 439, 76, 26 | 一致（未激活） |
| reset | 864, 439, 76, 26 | 864, 439, 76, 26 | 一致 |
| hold_toggle | 784, 469, 76, 26 | 784, 469, 76, 26 | 一致（未激活） |
| hold_time 2s | 864, 469, 76, 26 | 864, 469, 76, 26 | 一致 |
| icon_undo | 784, 499, 36, 26 | 784, 499, 36, 26 | 一致 |
| icon_redo | 824, 499, 36, 26 | 824, 499, 36, 26 | 一致 |
| icon_save | 864, 499, 36, 26 | 864, 499, 36, 26 | 一致 |
| icon_load | 904, 499, 36, 26 | 904, 499, 36, 26 | 一致 |

### 2.6 幽灵按钮与标题区

| 控件 | layout.rs 常量 | 实测 | 结论 |
|---|---|---|---|
| range_btn | 546, 309, 38, 19 | 无底色块 | 幽灵按钮，仅标签文字 |
| level_mode（dBTP 标签） | 584, 309, 28, 19 | 无底色块 | 幽灵按钮；「dBTP」文字落在增益条右下 |
| title_orm 排版盒 | 784, 533, 120, 34 | 墨色盒 ≈ 785, 540, 92, 22 | 排版盒用于定位；墨高见 §4 |
| title_analyzer 排版盒 | 784, 565, 156, 34 | 墨色盒 ≈ 784.5, 572.5, 104, 23.5 | 同上 |
| version 版本号 | 784, 530, 156, 34 | 墨行 y ≈ 540 – 543，col_700 | 齿轮右侧，左缘 = gearR + 8 |

### 2.7 声像面板相关度条

| 项 | 实测 |
|---|---|
| 相关度条填充段 | y 551 – 564（高约 14px），x 约 30 – 370；色 `(171, 192, 167)` 主题绿调浅绿 |
| 中置分隔 | 1px 主题竖线；下方左侧「CORRELATION +1.00」、右侧「BALANCE C」为静态标签 |

## 3. 色值实测表

### 3.1 主题色（Python 按 `theme.rs` HSB 公式复算 + 截图核对）

| 色名 | 复算 RGB | 截图实测 | 采样点（逻辑） | 备注 |
|---|---|---|---|---|
| col_100 背景 | (242, 242, 240) | (242, 242, 240) | (10, 10) | 窗口/卡片底色，刻度列底 |
| col_300 常态按钮底 | (230, 229, 226) | (230, 229, 226) | STFT 内角 (92, 30) | 含空轨条、图标按钮底 |
| col_500 中灰 | (153, 151, 144) | (153, 151, 145) | gonio L/R 字母 | 静止态字母落此灰调 |
| col_700 深灰文字 | (102, 100, 93) | (102, 100, 94) | 版本号字形 (905, 541) | 刻度标签/版本号；抗锯齿像素约 (110, 108, 103) |
| col_900 深墨 | (26, 25, 22) | (26, 25, 22) | STFT/FIELD 等深墨字形 | 常态按钮文字 |

### 3.2 激活态（本截图 PWR、M 激活）

| 项 | 实测 RGB | 复算/参照 | 备注 |
|---|---|---|---|
| 激活按钮底 | **(152, 147, 132)** | ≈ 通道 M 色 `hsb(45, 0.15, 0.60)` = (153, 147, 130) | 见 §5.2，与 Rust 当前 active 分支不一致 |
| 激活按钮文字 | **(242, 242, 242)** | ≈ col_100 | 白/近白墨 |

**未激活态推断**：本截图 FREEZE / HOLD 均未激活，实测底色 col_300 `(230, 229, 226)`、文字 col_900 `(26, 25, 22)`。
激活/反色态按原版 toggle 习惯应为「激活底 + 反色文字」；确切激活底色需另截一张 FREEZE/HOLD 激活态核对，
不要直接套用 §3.2 的 PWR 激活色（二者可能是不同按钮族）。

### 3.3 通道色（`channel_colors()` 复算，静止态未显现色相）

| 通道 | 复算 RGB | 说明 |
|---|---|---|
| L（hue−30） | (153, 133, 126) | 有信号时的左通道曲线色 |
| R（hue+30） | (146, 153, 126) | 有信号时的右通道曲线色 |
| M（hue） | (153, 147, 130) | 中通道；与实测激活底 (152, 147, 132) 吻合 |

### 3.4 频谱网格灰阶梯（自上而下五级，x = 120 取样）

| 层级 | 实测 RGB |
|---|---|
| 1（顶，最亮） | (230, 230, 227) |
| 2 | (225, 224, 221) |
| 3 | (218, 217, 214) |
| 4 | (212, 210, 206) |
| 5（底，最暗） | (188, 185, 178) |

> 对照 `meter.rs` `grid_gray()`（Light：`vv += (255−v)×0.18` 提亮后走 HSB）。
> 验收时逐级比对，偏差应 ≤ 3/通道。

### 3.5 电平条 / 相关度条实测

| 采样点 | 实测 RGB | 备注 |
|---|---|---|
| dBTP 增益条填充中段 (608, 265) | (69, 100, 81) | OKLCH 绿→青渐变中段，非固定绿；整段随量程 `meter_stops()` 解算 |
| VU 条轨 (647, 200) | (230, 229, 226) | 本信号下未填充 |
| LUFS 响度条轨 (703, 200) | (230, 229, 226) | 本信号下未填充 |
| 相关度条填充段 (120, 558) | (171, 192, 167) | 主题绿调浅绿 |

### 3.6 固定语义 Meter 色（本截图未出现）

| 常量 | RGB | 本截图检测 |
|---|---|---|
| METER_YELLOW | (232, 173, 40) | 0 像素（低信号，未达黄区） |
| METER_RED | (226, 60, 52) | 0 像素 |
| METER_GREEN | (96, 186, 96) | 0 像素 |

> 这三个色只在电平越过对应阈值的 LED/区段出现；需带信号截图再核对位置与色相。

## 4. 字号与墨高结论

原版 `IText(px)` 是 iPlug2 像素字号，macOS 按 72dpi→pt 落到 **0.8 倍**；
Rust 侧 `fonts.rs` 统一乘 `TEXT_SCALE = 0.8` 得到 egui 逻辑字号（`egui_px = IText × 0.8`）。

| 区域 | IText 名义 px | egui 字号 | 实测墨高（逻辑 px） | 墨色 |
|---|---|---|---|---|
| 顶行/下排按钮（PWR, STFT, FIELD, CPU, FREEZE, SYNC…） | 20 | 16.0 | **11.5**（大写墨高，墨行 px 14–36） | 深墨 col_900 / 激活白墨 |
| 轴刻度标签（20Hz 等） | 14 | 11.2 | **7.5** | col_700 |
| 小字（Anti Phase 等） | 14 | 11.2 | ≈ 5.0（小写 x 高度，测量受环形背景干扰） | col_700 |
| 标题 ORM | 待原版 `Analyzer.cpp` 标题常量确认 | — | **≈ 22**（y 540 – 561.5） | col_900 Bold |
| 标题 Analyzer | 同上 | — | **≈ 23.5**（y 572.5 – 595.5） | col_900 Bold |

结论：
- 20px 名义按钮字 → egui 16px，大写墨高 ≈ 11.5 逻辑 px（约字号 0.72）；复刻版墨高偏差应 ≤ 1px。
- 14px 名义刻度字 → egui 11.2px，大写/数字墨高 ≈ 7.5px。
- 标题字号 Rust 端尚未渲染；以实测墨高 22–23.5px 反推，需对照原版标题 `IText` 常量后定 `egui` 字号，
  并保证两行标题基线位置：ORM 行 y 540–561，Analyzer 行 y 572–596。

## 5. 与 `layout.rs` / 端口源码的不一致与待复核项

### 5.1 顶行 STFT / M 相邻边界（非错误）

`mode` 右缘与 `res` 左缘在 x = 150 处相接、无缝隙（常量如此）。连通域检测会把两个底色块视为一体，
各自测得 ±2px 渗入；**布局常量无需修改**，diff 时按整行底色带核对即可。

### 5.2 ⚠ 激活态配色与 Rust `widgets.rs` 当前实现不一致

- 原版实测：激活底 `(152, 147, 132)`（通道 M 主题色调）+ 近白文字 `(242, 242, 242)`。
- Rust `widgets.rs::draw_flat` 当前 active/pressed 分支：底 `col_900` `(26,25,22)`、文字 `col_100`。
- **待办**：回原版 `FlatActionButton` / `FlatToggleControl` / 反色开关源码确认激活底色到底取通道色还是 col_900；
  本截图支持「通道 M 色底 + 白字」。FREEZE/HOLD 类开关的激活态需另截激活态截图复核。

### 5.3 PERS / ZOOM 按钮默认态不可见

默认声像（gonio）状态下，`gonio_persist`(102,334,86,26) 与 `gonio_zoom`(194,334,52,26) 无底色块；
需切换到对应面板（矢量示波/缩放态）再截图，核对其底色、文字与几何。

### 5.4 卡片内缩量

卡片外框常量与截图外轮廓一致；实测内容占用框比常量内缩 10–20px，属于边框 + `kPad=20` 内边距，
不是布局错误。diff 验收只要求外框与轨道位置一致，内部迹线按 §6 放宽。

### 5.5 标题区尚未在 Rust 端渲染

`title_orm / title_analyzer / version` 排版盒已在 `layout.rs` 就位，但当前 UI 未绘制标题；
验收前需补齐：两行标题墨高与基线（§4）、齿轮位置（`orm_ink_w + 8`，见 `layout.rs::gear_rect`）、
版本号左缘 = gearR + 8。

### 5.6 网格灰阶梯待逐档核对

§3.4 五级实测灰度需与 `grid_gray()` 输出逐档比对；若整档偏亮/偏暗，调 `grid_gray` 的提亮系数而非硬编码 RGB。

## 6. 验收边界：必须逐像素一致 vs 允许内容差异

### 6.1 必须一致（静态布局/外观）

- 画布 960×635（2x 1920×1270），整体不偏移、不缩放。
- §2 全部实体按钮、卡片、右栏控件、图标的**位置与尺寸**（Δ ≤ 1 逻辑 px）。
- 常态/激活底色、文字色、背景色、§3.4 网格灰阶梯、电平条轨道分区宽度。
- 静态刻度/标签文字（20Hz/100Hz/1kHz/10kHz、+3/0/−20…、VU/LUFS 列标题、Anti Phase、
  CORRELATION/BALANCE 标签、FREEZE/RESET/HOLD/2s 等按钮文案）的字号、墨高、颜色与对齐。
- 四个图标的位置与绘制；相关度条轨道底色与高度。

### 6.2 允许内容差异（随信号/运行状态变化，不参与逐像素一致）

- 主频谱曲线形状、阴影填充、峰值保持迹线。
- 各电平条（dBTP/VU/LUFS/响度）的**填充高度**与渐变中段色；Meter 黄/红/绿 LED 是否点亮。
- CPU 百分比读数（截图 5.18%）、Det 频率/时间读数（2369.4 Hz / 0.42 ms）。
- 示波器迹线、矢量示波图形、L/R 字母随信号的通道色相。
- 相关度 / 平衡数值文字、LRA 区间显示。
- 任何悬停叠层、按下瞬时反馈（本截图为静止默认态，不含 hover）。

> diff 工具可对上述动态区打 `--ignore-region`（见 §7.2），避免把信号差异误判成布局缺陷。

## 7. 工具与复测

### 7.1 测量脚本（生成几何/色值/墨高对照）

- 路径：`/Users/feizao/个人文件/plugin-dev/OpenRM-Rust/tools/measure_reference.py`

```bash
cd /Users/feizao/个人文件/plugin-dev/OpenRM-Rust
python3 tools/measure_reference.py \
    --png docs/screenshots/analyzer-original/01-main-default.png
# 可选：--json <path> 导出结构化结果（不写回 analyzer-original 目录）
```

输出：主题色复算表、几何实测表（含 Δ 与备注）、取色表、固定 Meter 色检出、墨高表。

### 7.2 像素 diff 工具

- 路径：`/Users/feizao/个人文件/plugin-dev/OpenRM-Rust/tools/pixel_diff.py`

```bash
cd /Users/feizao/个人文件/plugin-dev/OpenRM-Rust
python3 tools/pixel_diff.py \
    --ref  docs/screenshots/analyzer-original/01-main-default.png \
    --test docs/screenshots/egui-capture.png \
    --out  docs/screenshots/diff/ \
    --ignore-region 20,58,564,270 \   # 可重复：忽略频谱曲线区（逻辑 x,y,w,h）
    --ignore-region 784,409,156,26 \ # 忽略 CPU% 文本
    --tolerance 3 --metric channel --grid 32 --top 20
```

参数：

| 参数 | 说明 |
|---|---|
| `--ref` / `--test` | 原版 2x 截图 / 待测截图；待测为 1x 时自动最近邻放大到 2x，尺寸不符时 LANCZOS 缩放并在报告中提示 |
| `--out` | 输出目录，生成 `diff_stats.json`、`heatmap.png`（32×32 网格热力图）、`comparison.png`（原版/待测/差异三栏） |
| `--ignore-region x,y,w,h` | 忽略区域，逻辑坐标，可重复；动态信号区用它排除 |
| `--tolerance` | 差异阈值：`channel` 为逐通道最大绝对差，`de` 为 ΔE76；默认 0 |
| `--metric` | `channel`（默认）或 `de` |
| `--grid` | 热力图/聚合块网格粒度，默认 32 |
| `--top` | 报告差异最大的前 N 个连通块，默认 20 |

`diff_stats.json` 字段：总像素、差异像素数与占比、平均/中位/最大色差、top-N 区域（逻辑 x,y,w,h + 差异像素数）。

### 7.3 自测结果

| 用例 | 结果 |
|---|---|
| 同图互比（ref vs ref） | 有效像素 2,438,400，**差异像素 0（0.000%）**，平均/中位/最大色差均为 0 |
| 故意右移+下移 2px 的副本（`--tolerance 3`） | 差异像素 **134,631（5.521%）**，平均色差 2.831，最大 218；Top-1 块覆盖全画布（整体平移的预期形态），热力图在按钮/卡片/右栏等内容区点亮、空白背景保持黑色 |
| 1x 半尺寸副本 | 自动按最近邻放大 2x 后比对；`--tolerance 8` 下检出 2.581% 残差（来自先 LANCZOS 缩小的重采样，属预期） |

### 7.4 推荐验收流程

1. 跑 `measure_reference.py` 确认基准截图未变、几何表与 §2 一致。
2. 截 Rust 版同默认态画面（2x），跑 `pixel_diff.py`，先不设 ignore 看全量热力图。
3. 对频谱曲线、CPU%、Det 读数等动态区补 `--ignore-region`，再看静态区残留差异。
4. 对照 §5 待复核项逐项确认；激活态、带信号电平、PERS/ZOOM 面板需补专门截图。

## 8. 最终验收数值

> 本节为移植收尾验收的最终轮实测汇总；几何 / 色值 / 墨高结论见 §2–§4，工具用法
> 见 §7。截图 `docs/screenshots/analyzer-m1.png` 由
> `cargo run -p orm-analyzer --example shot` 生成（2x 1920×1270）。

### 8.1 编译与测试（`cargo test --workspace`）

| crate | 测试目标 | passed | failed |
|---|---|---|---|
| openrm-analyzer-dsp | lib 单测 | 3 | 0 |
| openrm-analyzer-dsp | halfband_parity | 2 | 0 |
| openrm-analyzer-dsp | level_meter_parity | 2 | 0 |
| openrm-analyzer-dsp | loudness_parity | 9 | 0 |
| openrm-analyzer-dsp | pbt_parity | 3 | 0 |
| openrm-analyzer-dsp | rta_parity | 3 | 0 |
| openrm-analyzer-dsp | scope_parity | 2 | 0 |
| openrm-analyzer-dsp | stft_parity | 4 | 0 |
| openrm-analyzer-dsp | test_signal_parity | 2 | 0 |
| openrm-analyzer-dsp | vqt_parity | 2 | 0 |
| openrm-dsp | bandpass_parity | 13 | 0 |
| openrm-ui | lib 单测 | 0 | 0 |
| orm-analyzer | lib 单测 | 2 | 0 |
| orm-analyzer | audio_smoke | 9 | 0 |
| orm-analyzer | i18n | 3 | 0 |
| orm-analyzer | param_parity | 5 | 0 |
| orm-analyzer | scope_model | 5 | 0 |
| orm-analyzer | spectrum_model | 7 | 0 |
| orm-bandpass | lib 单测 | 1 | 0 |
| orm-bandpass | audio_smoke | 2 | 0 |

合计 **79 passed / 0 failed**；各 crate doc-tests 均为 0。`cargo check --workspace
--all-targets` **0 error**，仅既有 deprecation / dead_code / unused 警告（BandPass
与 openrm-ui 存量警告，Analyzer 侧为 2 条 egui `style`/`set_style` 改名弃用警告）。

### 8.2 DSP 离线 parity 实测值

黄金表由原版 C++ 头文件 + WDL fft.c 导出，Rust 侧逐项比对：

| 项目 | 实测值 | 对应用例 |
|---|---|---|
| 单声道 1kHz 0 dBFS 综合响度 I | **-3.0036 LUFS**（BS.1770-4 校准点 -3.01，差 0.0064） | `loudness_parity::loudness_calibration_and_msi` |
| 立体声等幅 1kHz 0 dBFS I | **+0.0067 LUFS**，M/S/I 三路一致 | 同上 |
| 12 LU 线性（-8 vs -20 dBFS） | 响度差 **12.0000 LU**（容差 0.05） | `loudness_parity::loudness_level_linearity` |
| LRA 双电平节目 | **16.0000 LU**（端点 -26.0000..-10.0000）；恒电平节目 0.0000 LU | `loudness_parity::loudness_lra_two_level_and_flat` |
| 电平峰值（0.5 幅度正弦） | peak L/R **-6.0206 dBFS**，true peak **-6.0218 dBTP**，与黄金表 Δ=0.0000 | `level_meter_parity` |
| M/L 幅度比 | **1.4142**（L 带峰 0.65789，M 带峰 0.93040；M 路按 1/√2 归一化，等幅时 M/L = √2） | `audio_smoke::frame_cadence_and_pwr_lanes` |
| STFT bin 中心正弦峰 | **1.4142135**（N=4096/8192 黄金值 1.41421353817；N=2048 为 1.41421341896） | `stft_parity::sine_on_bin_center_matches_cpp` |
| VQT 带表 | **9 组**（44.1/48/96 kHz × gamma 20/10/5 Hz）× **240 带**，逐带中心频率/分层/窗长/读出偏移/归一化/复指数核系数匹配黄金表 | `vqt_parity::band_table_matches_cpp_all_rates` |
| PBT 带表 | 三档 49 / 57 / 65 带，核最大绝对偏差 2.24e-8 / 6.71e-8 / 3.73e-9；单音中心带 0.4999，邻带 ≈0 | `pbt_parity` |
| RTA 单音响应 | 中心带误差 ≤0.11 dB（49.61 Hz），其余频点 ≤0.01 dB | `rta_parity::rta_sine_center_gain` |
| 冻结回放 | 冻结后帧三路最大偏差 0.000000e0；20 块（512）出帧 10 次，hop=1024 每 2 块一帧 | `audio_smoke::freeze_holds_last_frame` / `frame_cadence_and_pwr_lanes` |

### 8.3 像素比对（最终轮）

待测 `docs/screenshots/analyzer-m1.png`（1920×1270，2x），与基准
`docs/screenshots/analyzer-original/01-main-default.png` 同尺寸直接比对（`channel`
逐通道最大差，tolerance 3）：

| 口径 | 遮盖区域（逻辑 x,y,w,h） | 有效像素 | 差异像素 | 占比 | 均值色差 | 中位 | 最大 |
|---|---|---|---|---|---|---|---|
| 全量（无遮盖） | — | 2,438,400 | 172,196 | **7.062%** | **2.314** | 0 | 218 |
| 静态掩码 | 16,62,744,268（主频谱 + 电平条列区）；16,358,684,244（下半声像 / 示波器卡区） | 973,248 | 21,254 | **2.184%** | **1.739** | 0 | 218 |

结论：两种口径下中位色差均为 **0**，未遮盖静态区与原版一致；静态掩码残留
**2.18%** 集中在字形边缘抗锯齿像素，其余小块位于下排按钮行（420,331.5,120,39，
916 px）与顶行按钮区（570,39,210,39，115 px）。复现命令见 §7.2；静态掩码口径
输出在 `docs/screenshots/diff/analyzer-m1-static/`（`diff_stats.json` /
`heatmap.png` / `comparison.png`）。
