# OpenRM local patches for truce-egui 6.3.0

This directory is a source-vendored copy of `truce-egui` 6.3.0
(`LicenseRef-TruceLicense-1.0`), patched for the macOS editor open/close
problem in OpenRM.

## Why this exists

Truce's macOS renderer has two failure modes that both leave the editor
window blank with no recovery:

1. `EguiRenderer::render` called `surface.get_current_texture()` and silently
   returned on `Outdated` / `Lost`. A host that finishes attaching or sizing
   its child `NSView` after the first `surface.configure` can make Metal
   report `Outdated`; because acquisition only happens inside `render`, the
   editor never retried and stayed white forever. On reopen the same race
   happens again.
2. If `EguiRenderer::from_window` returned `None` (transient Metal layer /
   surface setup failure while the host is attaching the view),
   `EguiWindowHandler::on_frame` checked `painter_ready()` and returned, but
   nothing ever retried `from_window`. The window stayed blank for the whole
   editor session.

## Changes

- `src/renderer.rs`
  - `EguiRenderer` keeps the handler's `device_lost` flag.
  - On `Outdated`, reconfigure the surface once and retry the acquire.
  - On `Lost`, raise `device_lost` so `on_frame` rebuilds the renderer.
- `src/editor.rs`
  - When the inline renderer is `None`, retry `recover_device(window)` every
    200 ms. A successful retry recreates the `egui::Context` so the font
    atlas is re-emitted to the fresh renderer, and arms `force_paint`.

The broader open-time work (prewarming egui's pipeline, and not gating the
first frames on `NSWindowOcclusionStateVisible`) lives in
`crates/truce-gui-utils` and `crates/openrm-ui/src/gpu.rs`.
