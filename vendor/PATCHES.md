# Vendor patches

`vendor/gpui` is gpui 0.2.2 from crates.io, and `vendor/taffy` is taffy 0.9.0. Both carry local patches. The project uses these directories through `[patch.crates-io]` in `Cargo.toml`. Patch locations have `quiet.md:` comments; consult the tables and diffs for the complete list.

To compare with upstream:

```sh
R=~/.cargo/registry/src/index.crates.io-*/
diff -ru $R/gpui-0.2.2/src vendor/gpui/src
diff -ru $R/taffy-0.9.0/src vendor/taffy/src
```

When upgrading upstream, reapply each patch in the tables and retain its comments.

## GPUI 0.2.2

| # | File | Change | Purpose |
|---|---|---|---|
| 1 | `platform/mac/text_system.rs` | Explicitly enable font smoothing during rasterization (`set_allows_font_smoothing(true)`, `set_should_smooth_fonts(true)`). | Match native AppKit / Catalyst text (stroke ink 5.73 vs 5.74 per pixel width); disabling smoothing reduces it by about 15%. |
| 2 | `platform/mac/text_system.rs` | Measure glyph bounds with a CTFont at the rasterization size. | System fonts switch optical sizes with font size (SF Text to SF Display); large heading glyphs previously had their left edges clipped. |
| 3 | `platform/mac/text_system.rs` | Allow fonts named with the `KaTeX_` prefix to load even without an `m` glyph. | KaTeX symbol fonts (Size1–4, AMS, Caligraphic, Script) lack lowercase letters and were previously rejected. |
| 4 | `text_system/line_wrapper.rs`, `line_layout.rs` | Apply `is_no_break_before` and `is_no_break_after` in both line-breaking algorithms. | Chinese closing punctuation cannot start a line, and opening punctuation cannot end one, in body text and search excerpts. |
| 5 | `text_system/line.rs` | `paint_rows` / `paint_background_rows` paint at explicitly specified row tops. | Rows containing formulas or buttons can have different heights. |
| 6 | `style.rs`, `styled.rs`, `taffy.rs` | `grid_col_widths(Vec<f32>)` sets column widths in pixels and overrides `grid_cols`; add a type to the `repeat` literal. | Table column widths. |
| 7 | `elements/list.rs` | `ListState::estimated_scroll_metrics` / `scroll_to_estimated_offset` estimate unmeasured rows using average row height. | Scrollbars and scrollbar dragging in long documents. |
| 8 | `platform.rs`, `platform/mac/platform.rs` | `PathPromptOptions.extensions` restricts selectable file types in NSOpenPanel. | Import dialogs list only Markdown, text, and `.xml` files. |
| 9 | `platform/mac/window.rs` | Return NO from `mouseDownCanMoveWindow` and add `start_window_move`, which defers `performWindowDragWithEvent`. | The transparent title bar no longer drags the window itself or covers app buttons; the app identifies draggable areas. |
| 10 | `platform/mac/metal_renderer.rs`, `window.rs`, `platform.rs` | `render_to_bgra` / `Window::render_offscreen` render a scene to an offscreen texture and read it back; add `Window::needs_draw`. | Scripted screenshots and video recording work when the window is covered or the screen is locked (devtools). |
| 11 | `window.rs` | Make `DispatchEventResult`, the return type of `Window::dispatch_event`, public. | Devtools calls `dispatch_event` when replaying events. |
| 12 | `platform/mac/text_system.rs` | For requested italics without an italic face, including CoreText fallback fonts, use a slanted twin FontId. Shear glyphs by 14° during rasterization, matching WebKit's synthetic oblique angle, and widen their bounds. | Chinese italic text slants like browser text; PingFang has no italic face and previously displayed it upright. |
| 13 | `text_system/line.rs` | Include centered and right-aligned glyph positions and oversized text in scene ordering bounds for text and background decorations. | Image captions no longer show through floating toolbars. |
| 14 | `platform/mac/text_system.rs`, `platform/mac.rs`, `platform.rs` | Allow `font_ids_by_postscript_name` to return multiple FontIds per name. Match CoreText fallback fonts by glyph count; add `check_fallback_glyphs` for `editor::text::tests`. | System fallback PingFang (`PingFangUI.ttc`) and downloaded `PingFang.ttc` both use `PingFangSC-Regular` but have different glyph tables. Name-only lookup previously selected the wrong font and rendered incorrect Chinese characters. |
| 15 | `assets.rs`, `svg_renderer.rs` | Add `AssetSource::render_mask`, which `SvgRenderer::render` tries before loading an SVG. | SF Symbols are drawn by AppKit at the exact device size and go through the normal tinted sprite path. |

## taffy 0.9.0

| File | Change | Purpose |
|---|---|---|
| `compute/grid/track_sizing.rs` | In the "beyond limits" phase, allocate space only to affected tracks and use only the `fit-content()` argument as the growth limit. This fix is backported from taffy 0.13. | Merged cells spanning multiple auto rows previously failed to expand those rows under a min-content constraint. |

## Removed upstream files

`gpui/examples/image/black-cat-typing.gif` (4.3 MB) is used only by `examples/gif_viewer.rs` and `examples/opacity.rs`. Those examples are not part of this project's build. The file was removed to reduce repository size.
