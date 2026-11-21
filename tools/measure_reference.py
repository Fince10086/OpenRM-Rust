#!/usr/bin/env python3
"""测量原版 C++/iPlug2 截图的几何 / 色值 / 字号，作为 Rust/egui 复刻的验收基准。

用法：
    python3 tools/measure_reference.py \
        --png docs/screenshots/analyzer-original/01-main-default.png \
        [--json docs/screenshots/analyzer-original/reference-measure.json]

坐标约定：原版逻辑尺寸 960×635，截图为 Retina 2x（1920×1270）。
    逻辑坐标 (x, y) = 像素 (x * SCALE, y * SCALE)，SCALE = 图片宽 / 960。

本脚本里的「布局常量」逐项镜像自 ``plugins/analyzer/src/ui/layout.rs``，
测量值与常量不一致时会在报告里列出（±1 逻辑 px 通常是视觉边框/抗锯齿造成）。
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from dataclasses import dataclass

import numpy as np
from PIL import Image

# ---------------------------------------------------------------------------
# 0. 镜像 layout.rs 的常量（修改 Rust 源码时不要改这里，这里只做对照基准）
# ---------------------------------------------------------------------------

UI_W, UI_H = 960.0, 635.0

COL1_X, BTN_W, BTN_GAP = 784.0, 76.0, 4.0
COL2_X = COL1_X + BTN_W + BTN_GAP          # 864
PANEL_R = COL2_X + BTN_W                    # 940

TOP_BTN_W, TOP_BTN_H, TOP_BTN_Y, TOP_BTN_GAP = 62.0, 26.0, 26.0, 6.0

PLOT = (20.0, 58.0, 764.0, 270.0)           # x,y,w,h  (max 784,328)

BOT_BTN_Y, BOT_BTN_H = 334.0, 26.0
BOTTOM_TOP = 366.0
SCOPE_H, SCOPE_B = 233.0, 599.0

GAIN_BAR_W, VU_SCALE_W, VU_BAR_W = 14.0, 28.0, 14.0
LUFS_SCALE_W, LOUD_BAR_W, LRA_ZONE_W = 28.0, 14.0, 32.0
METER_STRIP_W = (2 * GAIN_BAR_W + VU_SCALE_W + 2 * VU_BAR_W
                 + LUFS_SCALE_W + 4 * LOUD_BAR_W + LRA_ZONE_W)  # 200

ROW_H = 26.0
ICON_ROW_Y, HOLD_ROW_Y, FREEZE_ROW_Y, CPU_Y = 499.0, 469.0, 439.0, 409.0
ICON_BTN_W = (BTN_W - BTN_GAP) / 2.0        # 36

# 检测类别：fill = 实体底色按钮；content = 内容卡（非背景占用）；ghost = 仅文字/无底色
@dataclass
class Control:
    name: str
    rect: tuple[float, float, float, float]
    kind: str = "fill"


CONTROLS: list[Control] = [
    Control("chan_mode  PWR（激活）",   (20.0, 26.0, 62.0, 26.0)),
    Control("mode       STFT",          (88.0, 26.0, 62.0, 26.0)),
    Control("res        M(顶行)",       (150.0, 26.0, 31.0, 26.0)),
    Control("window_btn SHARP",        (187.0, 26.0, 62.0, 26.0)),
    Control("release_mode LOG",         (255.0, 26.0, 62.0, 26.0)),
    Control("slope      3dB/Oct",      (323.0, 26.0, 116.0, 26.0)),
    Control("speed      MAX",           (445.0, 26.0, 62.0, 26.0)),
    Control("plot 主频谱框",            PLOT, "content"),
    Control("scope_mode FIELD",         (20.0, 334.0, 76.0, 26.0)),
    Control("gonio_persist PERS",       (102.0, 334.0, 86.0, 26.0), "ghost"),
    Control("gonio_zoom ZOOM",          (194.0, 334.0, 52.0, 26.0), "ghost"),
    Control("scope_chan M（激活）",     (420.0, 334.0, 34.0, 26.0)),
    Control("scope_trig SYNC",          (460.0, 334.0, 62.0, 26.0)),
    Control("scope_panel 声像卡",       (20.0, 366.0, 374.0, 233.0), "content"),
    Control("oscilloscope 示波卡",      (400.0, 366.0, 296.0, 233.0), "content"),
    Control("cpu 5.18%",                (COL1_X, CPU_Y, PANEL_R - COL1_X, ROW_H)),
    Control("freeze",                   (COL1_X, FREEZE_ROW_Y, BTN_W, ROW_H)),
    Control("reset",                    (COL2_X, FREEZE_ROW_Y, PANEL_R - COL2_X, ROW_H)),
    Control("hold_toggle",              (COL1_X, HOLD_ROW_Y, BTN_W, ROW_H)),
    Control("hold_time 2s",             (COL2_X, HOLD_ROW_Y, PANEL_R - COL2_X, ROW_H)),
    Control("icon_undo",                (COL1_X, ICON_ROW_Y, ICON_BTN_W, ROW_H)),
    Control("icon_redo",                (COL1_X + 40.0, ICON_ROW_Y, ICON_BTN_W, ROW_H)),
    Control("icon_save",                (COL1_X + 80.0, ICON_ROW_Y, ICON_BTN_W, ROW_H)),
    Control("icon_load",                (COL1_X + 120.0, ICON_ROW_Y, 36.0, ROW_H)),
    Control("range_btn（幽灵）",         (546.0, 309.0, 38.0, 19.0), "ghost"),
    Control("level_mode（幽灵 dBTP）",  (584.0, 309.0, 28.0, 19.0), "ghost"),
    Control("title_orm 标题区",          (COL1_X, SCOPE_B - 66.0, 120.0, 34.0), "content"),
    Control("title_analyzer 标题区",    (COL1_X, SCOPE_B - 34.0, PANEL_R - COL1_X, 34.0), "content"),
    Control("version 版本号",           (COL1_X, SCOPE_B - 69.0, PANEL_R - COL1_X, 34.0), "ghost"),
]


# ---------------------------------------------------------------------------
# 1. 主题色（Python 端口，对齐 crates/openrm-ui/src/theme.rs）
# ---------------------------------------------------------------------------

def hsb(h: float, s: float, b: float) -> tuple[int, int, int]:
    c = b * s
    hh = (h % 360.0) / 60.0
    x = c * (1.0 - abs((hh % 2.0) - 1.0))
    if hh < 1:
        r, g, bl = c, x, 0.0
    elif hh < 2:
        r, g, bl = x, c, 0.0
    elif hh < 3:
        r, g, bl = 0.0, c, x
    elif hh < 4:
        r, g, bl = 0.0, x, c
    elif hh < 5:
        r, g, bl = x, 0.0, c
    else:
        r, g, bl = c, 0.0, x
    m = b - c
    return tuple(int(round(v * 255)) for v in (r + m, g + m, bl + m))  # type: ignore[return-value]


HUE, SAT_MAX = 45, 15
LIGHT_B = [10, 40, 60, 90, 95]


def _col(idx: int) -> tuple[int, int, int]:
    b = LIGHT_B[idx]
    s = SAT_MAX * (100 - b) / 100.0 / 100.0
    return hsb(HUE, s, b / 100.0)


THEME = {
    "col_900": _col(0),
    "col_700": _col(1),
    "col_500": _col(2),
    "col_300": _col(3),
    "col_100": _col(4),
}


def channel_colors() -> dict[str, tuple[int, int, int]]:
    b = LIGHT_B[2] / 100.0
    s_m = SAT_MAX / 100.0
    s_lr = max(SAT_MAX, 15) / 100.0
    s_mix = min(s_lr * 1.155, 1.0)
    return {
        "L (hue-30)": hsb(HUE - 30, s_mix, b),
        "R (hue+30)": hsb(HUE + 30, s_mix, b),
        "M (hue)":    hsb(HUE, s_m, b),
    }


METER_FIXED = {
    "METER_YELLOW": (232, 173, 40),
    "METER_RED":    (226, 60, 52),
    "METER_GREEN":  (96, 186, 96),
}

# 实测：原版激活按钮底色（PWR / M）。复算 ≈ 通道 M 色 hsb(45, 0.15, 0.60)。
ACTIVE_FILL_MEASURED = (152, 147, 132)
ACTIVE_TEXT_MEASURED = (242, 242, 242)


# ---------------------------------------------------------------------------
# 2. 图像装载与坐标换算
# ---------------------------------------------------------------------------

@dataclass
class Sample:
    name: str
    logical: tuple[float, float]
    rgb: tuple[int, int, int]
    expected: tuple[int, int, int] | None = None
    note: str = ""


class Reference:
    def __init__(self, png: str):
        self.img = Image.open(png).convert("RGB")
        self.arr = np.asarray(self.img).astype(np.int16)
        self.h, self.w = self.arr.shape[:2]
        self.scale = self.w / UI_W
        if abs(self.h / UI_H - self.scale) > 0.02:
            print(f"[warn] 尺寸 {self.w}×{self.h} 与 2x 基准不符", file=sys.stderr)

    def px(self, x: float, y: float) -> tuple[int, int]:
        return int(round(x * self.scale)), int(round(y * self.scale))

    def crop(self, rect: tuple[float, float, float, float]) -> np.ndarray:
        x, y, w, h = rect
        x0, y0 = self.px(x, y)
        x1, y1 = self.px(x + w, y + h)
        return self.arr[y0:y1, x0:x1]

    def median_color(self, x: float, y: float, pad: int = 1) -> tuple[int, int, int]:
        cx, cy = self.px(x, y)
        half = int(round(pad * self.scale))
        patch = self.arr[max(0, cy - half):cy + half + 1,
                         max(0, cx - half):cx + half + 1]
        med = np.median(patch.reshape(-1, 3), axis=0)
        return tuple(int(round(v)) for v in med)  # type: ignore[return-value]

    def fill_mask(self, region: np.ndarray, tol: int = 26) -> np.ndarray:
        c300 = np.array(THEME["col_300"])
        c900 = np.array(THEME["col_900"])
        cact = np.array(ACTIVE_FILL_MEASURED)
        d = np.minimum(np.abs(region - c300).sum(2),
                       np.abs(region - c900).sum(2))
        d = np.minimum(d, np.abs(region - cact).sum(2))
        return d < tol

    def content_mask(self, region: np.ndarray, tol: int = 14) -> np.ndarray:
        bg = np.array(THEME["col_100"])
        return np.abs(region - bg).sum(2) > tol

    def bbox_in(self, rect: tuple[float, float, float, float], kind: str,
                pad: float = 2.0, occ: float = 0.22) -> tuple[float, float, float, float] | None:
        x, y, w, h = rect
        win = (x - pad, y - pad, w + 2 * pad, h + 2 * pad)
        region = self.crop(win)
        if kind == "ghost":
            mask = self.fill_mask(region)
        elif kind == "fill":
            mask = self.fill_mask(region)
        else:
            mask = self.content_mask(region)
        rows = np.where(mask.sum(1) > occ * mask.shape[1])[0]
        cols = np.where(mask.sum(0) > occ * mask.shape[0])[0]
        if len(rows) == 0 or len(cols) == 0:
            return None
        rx0, ry0 = self.px(win[0], win[1])
        mx0 = (rx0 + cols[0]) / self.scale
        my0 = (ry0 + rows[0]) / self.scale
        mx1 = (rx0 + cols[-1] + 1) / self.scale
        my1 = (ry0 + rows[-1] + 1) / self.scale
        return (round(mx0, 1), round(my0, 1), round(mx1 - mx0, 1), round(my1 - my0, 1))

    def interior_fill(self, rect: tuple[float, float, float, float]) -> tuple[int, int, int]:
        """取按钮内部离文字最远的一角，返回底色中位数。"""
        x, y, w, h = rect
        corner = (x + 2, y + 2, min(10, w - 4), min(8, h - 4))
        region = self.crop(corner).reshape(-1, 3)
        return tuple(int(round(v)) for v in np.median(region, 0))  # type: ignore[return-value]

    def ink_color(self, rect: tuple[float, float, float, float],
                  white_ink: bool = False) -> tuple[int, int, int] | None:
        region = self.crop(rect).reshape(-1, 3)
        med = np.median(region, axis=0)
        if white_ink:
            ink = region.mean(1) > med.mean() + 25
        else:
            ink = region.mean(1) < 190
        if not ink.any():
            return None
        return tuple(int(round(v)) for v in np.median(region[ink], 0))  # type: ignore[return-value]

    def ink_extent(self, rect: tuple[float, float, float, float],
                   white_ink: bool = False) -> tuple[float, int, int, tuple[int, int, int] | None]:
        region = self.crop(rect)
        med = np.median(region.reshape(-1, 3), axis=0)
        if white_ink:
            ink = region.mean(2) > med.mean() + 25
        else:
            ink = np.abs(region - med).sum(2) > 70
        rows = np.where(ink.sum(1) > 0.10 * ink.shape[1])[0]
        if len(rows) == 0:
            return 0.0, 0, 0, None
        ink_px = region[ink]
        color = tuple(int(round(v)) for v in np.median(ink_px, 0))  # type: ignore[return-value]
        return (rows[-1] - rows[0] + 1) / self.scale, int(rows[0]), int(rows[-1]), color


# ---------------------------------------------------------------------------
# 3. 报告组装
# ---------------------------------------------------------------------------

def geometry_table(ref: Reference) -> list[dict]:
    out: list[dict] = []
    for ctl in CONTROLS:
        if ctl.kind == "ghost":
            out.append({"name": ctl.name, "layout": ctl.rect, "kind": ctl.kind,
                        "measured": None, "delta": None,
                        "note": "幽灵按钮：本状态下无底色块，仅文字/标签"})
            continue
        measured = ref.bbox_in(ctl.rect, ctl.kind)
        note = ""
        delta = None
        if measured is None:
            note = "未检出内容，需复核"
        else:
            measured = tuple(round(float(v), 1) for v in measured)
            delta = tuple(round(m - e, 1) for m, e in zip(measured, ctl.rect))
            if ctl.kind == "content":
                note = "内容占用框；卡片发丝边框很浅，边缘 ±2px"
            elif any(abs(v) > 1.5 for v in delta):
                note = "偏移 >1.5px，需复核"
            elif any(abs(v) > 0.5 for v in delta):
                note = "±1px 内（边框/抗锯齿）"
        out.append({"name": ctl.name, "layout": ctl.rect, "kind": ctl.kind,
                    "measured": measured, "delta": delta, "note": note})
    return out


def color_samples(ref: Reference) -> list[Sample]:
    bg = THEME["col_100"]
    samples: list[Sample] = [
        Sample("窗口背景 col_100", (10, 10), ref.median_color(10, 10), bg),
        Sample("常态按钮底 col_300（STFT 内角）", (92, 30),
               ref.interior_fill((88, 26, 62, 26)), THEME["col_300"]),
        Sample("激活按钮底（PWR 内角）", (24, 30),
               ref.interior_fill((20, 26, 62, 26)), ACTIVE_FILL_MEASURED,
               "复算 ≈ 通道 M 色；Rust widgets.rs 当前 active 用 col_900，需复核"),
        Sample("激活按钮文字（PWR 白墨中位）", (51, 39),
               ref.ink_color((20, 26, 62, 26), white_ink=True) or (0, 0, 0),
               ACTIVE_TEXT_MEASURED),
        Sample("FREEZE 常态底（内角）", (790, 445),
               ref.interior_fill((784, 439, 76, 26)), THEME["col_300"]),
        Sample("HOLD 常态底（内角）", (790, 475),
               ref.interior_fill((784, 469, 76, 26)), THEME["col_300"]),
        Sample("刻度/标签灰 col_700（20Hz 字形）", (26, 70),
               ref.ink_color((20, 60, 60, 16)) or (0, 0, 0), THEME["col_700"]),
        Sample("版本号灰（v0.1.0 字形）", (905, 541),
               ref.ink_color((868, 534, 72, 10)) or (0, 0, 0), THEME["col_700"]),
    ]
    # 频谱网格五级灰度（自上而下）——实测阶梯，作为 grid_gray() 对齐基准
    for i, y in enumerate((70, 130, 190, 250, 310)):
        samples.append(Sample(f"频谱网格灰 第{i+1}级 (y≈{y})", (120, y),
                              ref.median_color(120, y), None,
                              "亮→暗阶梯；对照 meter.rs grid_gray()"))
    # 通道色：本截图静止状态 L/R 字母落 col_500 灰调，未显示通道色相
    glyph_rect = (110.0, 395.0, 190.0, 35.0)
    gregion = ref.crop(glyph_rect).reshape(-1, 3)
    gmask = gregion.mean(1) < 190
    gcol = tuple(int(round(v)) for v in np.median(gregion[gmask], 0)) if gmask.any() else (0, 0, 0)
    samples.append(Sample("gonio L/R 字母墨色（静止态）", (206, 413),
                          gcol, THEME["col_500"],
                          "静止态 ≈ col_500；通道 L/R/M 色相仅在有信号时显现"))
    # 电平条
    samples += [
        Sample("dBTP 增益条填充（第 2 条）", (608, 265),
               ref.median_color(608, 265), None, "OKLCH 绿→青渐变中段"),
        Sample("VU 条轨（空）", (647, 200),
               ref.median_color(647, 200), THEME["col_300"], "本信号下未填充"),
        Sample("LUFS 响度条轨（空）", (703, 200),
               ref.median_color(703, 200), THEME["col_300"], "本信号下未填充"),
        Sample("相关度条 填充段（L—R，y≈552–564）", (120, 558),
               ref.median_color(120, 558), None, "主题绿调浅绿；中置分隔为 1px 主题线"),
    ]
    return samples


def ink_regions(ref: Reference) -> list[dict]:
    regions = [
        ("PWR（激活白墨）",   (20.0, 26.0, 62.0, 26.0), 20.0, True),
        ("STFT（常态深墨）",  (88.0, 26.0, 62.0, 26.0), 20.0, False),
        ("FIELD 按钮",        (20.0, 334.0, 76.0, 26.0), 20.0, False),
        ("CPU 5.18%",         (784.0, 409.0, 156.0, 26.0), 20.0, False),
        ("FREEZE",            (784.0, 439.0, 76.0, 26.0), 20.0, False),
        ("SYNC",              (460.0, 334.0, 62.0, 26.0), 20.0, False),
        ("轴刻度 20Hz",       (20.0, 60.0, 60.0, 16.0), 14.0, False),
        ("Anti Phase 小字",   (36.0, 538.0, 80.0, 12.0), 14.0, False),
        ("标题 ORM",          (784.0, 533.0, 100.0, 30.0), 40.0, False),
        ("标题 Analyzer",     (784.0, 565.0, 140.0, 30.0), 40.0, False),
    ]
    out = []
    for name, rect, nominal, white in regions:
        ink_h, r0, r1, color = ref.ink_extent(rect, white_ink=white)
        out.append({
            "name": name, "rect": rect, "itext_px": nominal,
            "egui_px": round(nominal * 0.8, 1),
            "ink_h_logical": round(ink_h, 1),
            "ink_rows_px": [r0, r1], "ink_color": color,
        })
    return out


def scan_fixed_meters(ref: Reference) -> dict[str, dict]:
    out = {}
    for name, t in METER_FIXED.items():
        target = np.array(t)
        d = np.abs(ref.arr - target).sum(2)
        ys, xs = np.where(d < 45)
        entry: dict = {"present": len(xs) > 0, "count": int(len(xs))}
        if len(xs):
            entry["logical_bbox"] = [round(xs.min() / ref.scale, 1),
                                     round(ys.min() / ref.scale, 1),
                                     round((xs.max() + 1) / ref.scale, 1),
                                     round((ys.max() + 1) / ref.scale, 1)]
        out[name] = entry
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--png", default="docs/screenshots/analyzer-original/01-main-default.png")
    ap.add_argument("--json", default="")
    args = ap.parse_args()

    ref = Reference(args.png)
    print("# 基准截图测量报告\n")
    print(f"- 文件: `{args.png}`")
    print(f"- 像素尺寸: {ref.w}×{ref.h}，缩放系数 SCALE = {ref.scale:.3f} "
          f"（逻辑 {UI_W:.0f}×{UI_H:.0f}）")
    print(f"- 主题: Light, hue={HUE}, sat_max={SAT_MAX}")

    print("\n## 1. 主题色端口对照（Python 复算）\n")
    print("| 色名 | RGB |")
    print("|---|---|")
    for k, v in THEME.items():
        print(f"| {k} | {v} |")
    print("\n通道色复算：")
    for k, v in channel_colors().items():
        print(f"| {k} | {v} |")
    print("\n固定语义色：")
    for k, v in METER_FIXED.items():
        print(f"| {k} | {v} |")
    print(f"\n- 实测激活按钮底: {ACTIVE_FILL_MEASURED}；激活文字: {ACTIVE_TEXT_MEASURED}")

    print("\n## 2. 几何实测（逻辑坐标 x,y,w,h）\n")
    geo = geometry_table(ref)
    print("| 控件 | layout.rs 常量 | 实测 | Δ (dx,dy,dw,dh) | 备注 |")
    print("|---|---|---|---|---|")
    for r in geo:
        lay = tuple(round(v, 1) for v in r["layout"])
        if r["measured"] is None:
            print(f"| {r['name']} | {lay} | — | — | {r['note']} |")
        else:
            print(f"| {r['name']} | {lay} | {r['measured']} | {r['delta']} | {r['note']} |")

    print("\n## 3. 实测取色（3×3 逻辑邻域中位数）\n")
    print("| 采样点 | 逻辑坐标 | 实测 RGB | 期望/参照 | 备注 |")
    print("|---|---|---|---|---|")
    for s in color_samples(ref):
        exp = s.expected if s.expected is not None else "—"
        print(f"| {s.name} | ({s.logical[0]:.0f},{s.logical[1]:.0f}) "
              f"| {s.rgb} | {exp} | {s.note} |")

    print("\n## 4. 固定 Meter 色出现位置\n")
    fixed = scan_fixed_meters(ref)
    for k, v in fixed.items():
        print(f"- {k}: {v}")

    print("\n## 5. 字形墨高（逻辑 px）\n")
    print("| 区域 | IText 名义 px | egui 字号 (×0.8) | 实测墨高 | 墨行(px) | 墨色 |")
    print("|---|---|---|---|---|---|")
    for row in ink_regions(ref):
        print(f"| {row['name']} | {row['itext_px']} | {row['egui_px']} "
              f"| {row['ink_h_logical']} | {row['ink_rows_px']} | {row['ink_color']} |")

    if args.json:
        payload = {
            "image": args.png, "pixel_size": [ref.w, ref.h], "scale": ref.scale,
            "theme": THEME, "channel_colors": channel_colors(),
            "meter_fixed": METER_FIXED,
            "active_fill_measured": ACTIVE_FILL_MEASURED,
            "active_text_measured": ACTIVE_TEXT_MEASURED,
            "geometry": geo,
            "samples": [{"name": s.name, "logical": s.logical, "rgb": s.rgb,
                         "expected": s.expected, "note": s.note} for s in color_samples(ref)],
            "fixed_meters": fixed,
            "ink": ink_regions(ref),
        }
        os.makedirs(os.path.dirname(os.path.abspath(args.json)), exist_ok=True)
        with open(args.json, "w") as f:
            json.dump(payload, f, ensure_ascii=False, indent=2)
        print(f"\nJSON 已写入: {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
