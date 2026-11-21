#!/usr/bin/env python3
r"""原版截图 vs Rust/egui 待测截图的像素级对照工具。

用法：
    python3 tools/pixel_diff.py --ref <原版2x.png> --test <待测.png> \
        --out docs/screenshots/diff/ [--ignore-region 20,58,564,270] \
        [--tolerance 3] [--metric channel] [--grid 32] [--top 20]

输出（写入 --out 目录）：
    diff_stats.json   逐像素统计 + top-N 差异区域
    heatmap.png       32×32 网格差异热力图
    comparison.png    原版 / 待测 / 差异 三栏对照图

坐标：逻辑画布 960×635。2x 截图 1920×1270；若待测是 1x，会按最近邻放大到同尺寸。
--ignore-region 可重复，坐标为逻辑坐标 x,y,w,h。
"""

from __future__ import annotations

import argparse
import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

UI_W, UI_H = 960.0, 635.0


# ---------------------------------------------------------------------------
# 图像装载与对齐
# ---------------------------------------------------------------------------

def load_rgb(path: str) -> np.ndarray:
    return np.asarray(Image.open(path).convert("RGB")).astype(np.int16)


def align_to(ref: np.ndarray, test: np.ndarray) -> tuple[np.ndarray, str]:
    """把待测图对齐到参考图尺寸；返回 (对齐后数组, 说明)。"""
    rh, rw = ref.shape[:2]
    th, tw = test.shape[:2]
    if (tw, th) == (rw, rh):
        return test, "同尺寸直接比对"
    # 1x → 2x 最近邻放大
    if abs(tw * 2 - rw) <= 2 and abs(th * 2 - rh) <= 2:
        img = Image.fromarray(test.astype(np.uint8)).resize((rw, rh), Image.NEAREST)
        return np.asarray(img).astype(np.int16), "待测 1x，已最近邻放大 2x"
    # 其它尺寸：直接缩放到参考尺寸（会引入重采样模糊，报告会提示）
    img = Image.fromarray(test.astype(np.uint8)).resize((rw, rh), Image.LANCZOS)
    return np.asarray(img).astype(np.int16), f"待测 {tw}×{th} 已缩放到 {rw}×{rh}（重采样）"


# ---------------------------------------------------------------------------
# 色差
# ---------------------------------------------------------------------------

def rgb_to_lab(arr: np.ndarray) -> np.ndarray:
    """简易 sRGB→Lab（ΔE76 用）。arr: (...,3) int16 → float Lab。"""
    a = arr.astype(np.float64) / 255.0
    a = np.where(a > 0.04045, ((a + 0.055) / 1.055) ** 2.4, a / 12.92)
    m = a @ np.array([[0.4124, 0.2126, 0.0193],
                      [0.3576, 0.7152, 0.1192],
                      [0.1805, 0.0722, 0.9505]])
    m = m / np.array([0.95047, 1.0, 1.08883])
    m = np.where(m > 0.008856, m ** (1 / 3), 7.787 * m + 16 / 116)
    L = 116 * m[:, :, 1] - 16
    A = 500 * (m[:, :, 0] - m[:, :, 1])
    B = 200 * (m[:, :, 1] - m[:, :, 2])
    return np.stack([L, A, B], axis=2)


def color_diff(ref: np.ndarray, test: np.ndarray, metric: str) -> np.ndarray:
    if metric == "de":
        d = np.sqrt(((rgb_to_lab(ref) - rgb_to_lab(test)) ** 2).sum(2))
        return d
    # channel：逐通道最大绝对差
    return np.abs(ref - test).max(2)


# ---------------------------------------------------------------------------
# 忽略区域
# ---------------------------------------------------------------------------

def parse_region(text: str) -> tuple[float, float, float, float]:
    parts = [float(v) for v in text.split(",")]
    if len(parts) != 4:
        raise argparse.ArgumentTypeError(f"区域格式应为 x,y,w,h：{text}")
    return tuple(parts)  # type: ignore[return-value]


def ignore_mask(shape: tuple[int, int], regions: list[tuple[float, float, float, float]],
                scale: float) -> np.ndarray:
    mask = np.zeros(shape, dtype=bool)
    for x, y, w, h in regions:
        x0, y0 = int(round(x * scale)), int(round(y * scale))
        x1, y1 = int(round((x + w) * scale)), int(round((y + h) * scale))
        mask[y0:y1, x0:x1] = True
    return mask


# ---------------------------------------------------------------------------
# 网格 / 连通块
# ---------------------------------------------------------------------------

def grid_blocks(diff_mask: np.ndarray, grid: int, scale: float) -> list[dict]:
    """把差异掩码聚合到 grid×grid 网格，相邻差异格合并成块。"""
    h, w = diff_mask.shape
    gh, gw = h // grid, w // grid
    cell_mask = diff_mask[:gh * grid, :gw * grid].reshape(
        grid, gh, grid, gw).any(axis=(1, 3))

    visited = np.zeros_like(cell_mask, dtype=bool)
    blocks: list[dict] = []
    for r in range(grid):
        for c in range(grid):
            if not cell_mask[r, c] or visited[r, c]:
                continue
            stack = [(r, c)]
            visited[r, c] = True
            cells: list[tuple[int, int]] = []
            while stack:
                cr, cc = stack.pop()
                cells.append((cr, cc))
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nr, nc = cr + dr, cc + dc
                    if (0 <= nr < grid and 0 <= nc < grid
                            and cell_mask[nr, nc] and not visited[nr, nc]):
                        visited[nr, nc] = True
                        stack.append((nr, nc))
            rows = [c_[0] for c_ in cells]
            cols = [c_[1] for c_ in cells]
            px0 = min(cols) * gw
            py0 = min(rows) * gh
            px1 = (max(cols) + 1) * gw
            py1 = (max(rows) + 1) * gh
            sub = diff_mask[py0:py1, px0:px1]
            blocks.append({
                "logical": [round(px0 / scale, 1), round(py0 / scale, 1),
                            round((px1 - px0) / scale, 1), round((py1 - py0) / scale, 1)],
                "cells": len(cells),
                "diff_px": int(sub.sum()),
            })
    return blocks


def heatmap_image(diff: np.ndarray, grid: int, scale: float,
                  ignore: np.ndarray) -> Image.Image:
    h, w = diff.shape
    gh, gw = h // grid, w // grid
    cell_mean = diff[:gh * grid, :gw * grid].reshape(
        grid, gh, grid, gw).mean(axis=(1, 3))
    heat = np.zeros((*cell_mean.shape, 3), dtype=np.uint8)
    vmax = max(cell_mean.max(), 1.0)
    norm = cell_mean / vmax
    # 黑 → 红 → 黄
    heat[:, :, 0] = (255 * np.clip(norm * 1.6, 0, 1)).astype(np.uint8)
    heat[:, :, 1] = (180 * norm ** 2).astype(np.uint8)
    heat[:, :, 2] = (60 * norm ** 3).astype(np.uint8)
    # 忽略区置灰
    ig = ignore[:gh * grid, :gw * grid].reshape(grid, gh, grid, gw).any(axis=(1, 3))
    heat[ig] = (200, 200, 200)
    img = Image.fromarray(heat).resize((w, h), Image.NEAREST)
    draw = ImageDraw.Draw(img)
    for i in range(grid + 1):
        x = i * gw
        y = i * gh
        draw.line([(x, 0), (x, h)], fill=(120, 120, 120))
        draw.line([(0, y), (w, y)], fill=(120, 120, 120))
    # 逻辑坐标刻度
    for lx in range(0, 961, 120):
        draw.text((int(lx * scale) + 3, 3), str(lx), fill=(0, 0, 255))
    for ly in range(0, 636, 120):
        draw.text((3, int(ly * scale) + 3), str(ly), fill=(0, 0, 255))
    return img


def comparison_image(ref: np.ndarray, test: np.ndarray, diff: np.ndarray,
                     ignore: np.ndarray) -> Image.Image:
    h, w = ref.shape[:2]
    diff_amp = np.clip(diff * 4, 0, 255).astype(np.uint8)
    # 白底，差异越大红越深
    diff_rgb = np.stack([255 - diff_amp // 3, 255 - diff_amp, 255 - diff_amp], axis=2)
    diff_rgb[ignore] = (230, 230, 230)
    panels = [Image.fromarray(ref.astype(np.uint8)),
              Image.fromarray(test.astype(np.uint8)),
              Image.fromarray(diff_rgb)]
    labels = ["reference", "test", "diff (×4)"]
    pad = 12
    label_h = 28
    out_w = w * 3 + pad * 4
    out_h = h + label_h + pad * 2
    out = Image.new("RGB", (out_w, out_h), (255, 255, 255))
    draw = ImageDraw.Draw(out)
    for i, (panel, label) in enumerate(zip(panels, labels)):
        x = pad + i * (w + pad)
        out.paste(panel, (x, label_h + pad))
        draw.text((x + 4, 6), label, fill=(0, 0, 0))
    return out


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(description="像素级截图对照工具")
    ap.add_argument("--ref", required=True, help="原版 2x 截图")
    ap.add_argument("--test", required=True, help="待测截图（2x 或 1x）")
    ap.add_argument("--out", required=True, help="输出目录")
    ap.add_argument("--ignore-region", action="append", default=[], type=parse_region,
                    help="忽略区域，逻辑坐标 x,y,w,h，可重复")
    ap.add_argument("--tolerance", type=float, default=0.0,
                    help="差异阈值（channel=单通道最大差；de=ΔE76），默认 0")
    ap.add_argument("--metric", choices=["channel", "de"], default="channel")
    ap.add_argument("--grid", type=int, default=32)
    ap.add_argument("--top", type=int, default=20)
    args = ap.parse_args()

    ref = load_rgb(args.ref)
    test_raw = load_rgb(args.test)
    test, note = align_to(ref, test_raw)
    h, w = ref.shape[:2]
    scale = w / UI_W

    diff = color_diff(ref, test, args.metric)
    ignore = ignore_mask((h, w), args.ignore_region, scale)
    valid = ~ignore
    diff_mask = (diff > args.tolerance) & valid

    total = int(valid.sum())
    diff_count = int(diff_mask.sum())
    blocks = grid_blocks(diff_mask, args.grid, scale)
    blocks.sort(key=lambda b: b["diff_px"], reverse=True)

    stats = {
        "ref": args.ref, "test": args.test, "align_note": note,
        "pixel_size": [w, h], "scale": scale,
        "metric": args.metric, "tolerance": args.tolerance,
        "ignore_regions": [list(r) for r in args.ignore_region],
        "total_px": total, "diff_px": diff_count,
        "diff_ratio": round(diff_count / total, 6) if total else 0.0,
        "mean_diff": round(float(diff[valid].mean()), 3),
        "median_diff": round(float(np.median(diff[valid])), 3),
        "max_diff": round(float(diff[valid].max()), 3),
        "top_regions": blocks[:args.top],
    }

    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "diff_stats.json"), "w") as f:
        json.dump(stats, f, ensure_ascii=False, indent=2)

    heatmap_image(diff, args.grid, scale, ignore).save(
        os.path.join(args.out, "heatmap.png"))
    comparison_image(ref, test, diff, ignore).save(
        os.path.join(args.out, "comparison.png"))

    print(f"对齐: {note}")
    print(f"尺寸: {w}×{h}（scale={scale:.2f}），metric={args.metric}, "
          f"tolerance={args.tolerance}")
    print(f"有效像素 {total}，差异像素 {diff_count} "
          f"（{stats['diff_ratio'] * 100:.3f}%）")
    print(f"平均色差 {stats['mean_diff']}，中位 {stats['median_diff']}，"
          f"最大 {stats['max_diff']}")
    print(f"\nTop-{min(args.top, len(blocks))} 差异区域（逻辑 x,y,w,h）：")
    print("| # | 区域 | 网格块 | 差异像素 |")
    print("|---|---|---|---|")
    for i, b in enumerate(blocks[:args.top], 1):
        print(f"| {i} | {b['logical']} | {b['cells']} | {b['diff_px']} |")
    print(f"\n输出目录: {os.path.abspath(args.out)}")
    print("  - diff_stats.json / heatmap.png / comparison.png")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
