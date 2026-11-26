# 字体授权

- `Mixed-*.ttf`：OpenRM 原有界面字体（Outfit 变体 + 少量汉字子集），随项目分发。
- `OPPOSansSC-Regular.ttf` / `OPPOSansSC-SemiBold.ttf` / `OPPOSansSC-Bold.ttf`：
  OPPO Sans 4.0 的静态实例（wght 400/600/700），仅保留 GB2312 汉字与界面所需中文标点，
  供 Fusion 等新界面补字。来源为 OpenRM 项目内的 `assets/fonts/OPPOSans-Source.ttf`，
  版权归 OPPO 所有；子集由 `pyftsubset` 生成，不包含拉丁字母，拉丁仍由 Outfit 渲染。
