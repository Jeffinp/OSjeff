# OSjeff shell: design system

Status: wave 1 (foundation, system chrome, the toolkit). Wave 2 re-skins each app's
content on top of the toolkit described here. The visual values are tokens that live in
one place (`kernel/src/theme.rs`, backed by the pure tables in `osjeff_core::style`);
the code paths named below are the stable API.

The file keeps its original name (`ui-macos.md`) because the brief called the target
"macOS-like". The result is inspired by that family of desktops and deliberately not a
copy: it keeps the principles (a clear hierarchy, translucency where it is cheap, rounded
geometry, soft shadows, springy motion, generous spacing) and has its own identity.

## 0. Identity

| | OSjeff |
|---|---|
| Mark | a bold prompt chevron `>` (white) on an indigo squircle; the menu-bar version is the bare chevron. No fruit, no wordmark borrowed from anyone |
| Names | **Apps** (the grid, replaces the start panel), **Busca** (one field for apps, files and sums), **Barra de apps** (the floating bar), **Controles** (network, appearance, switches), **Arquivos**, **Tarefas**, **Monitor**, **Registro**, **Imagens**, **Componentes** (the widget gallery) |
| Accent | indigo `5B5CF6` by default; eight choices (Indigo, Turquesa, Violeta, Rosa, Coral, Âmbar, Verde, Grafite) |
| Icon language | a thick white glyph on a saturated vertical-gradient squircle with a faint top gloss; every glyph is drawn from our own vector paths (`osjeff_core::iconart`), none is a traced system icon |
| Wallpaper | "Dinâmico": pale lilac and sky by day, deep indigo by night, with soft colour glows; four more presets or an image |
| Motion | short and springy: windows pop in from 92 %, icons in the bar swell like a bump (spring per icon), launches hop twice; everything eases out and is interruptible |
| Language | Portuguese, plain: nothing in the UI talks about how the system is built; apps need no explanation |

## 1. Constraints that shape every decision

| Constraint | Consequence |
|---|---|
| Software renderer, 1280x720 (BIOS) or 1280x800 (UEFI), 24/32 bpp, BGR or RGB | every primitive is a format-aware `Canvas` method; no per-pixel format `match` in inner loops |
| `f32` is **emulated** in the kernel (the target has no SSE) | pixel work is integer / fixed point only (24.8 coverage, Q16 and Q8 maths); `f32` only for per-frame animation state (a few dozen values) |
| Damage tracking, cached static layer (`STATIC`), scene signature, drag and resize paths must stay correct | new visuals are layers inside the existing compositor paths (section 5), not a new compositor |
| An idle desktop must cost ~0 | every animation reports "active" only while it moves; springs and tweens snap to rest; nothing polls |
| 64 MiB heap, shared | caches are bounded and logged (section 9) |
| `osjeff_core` is `no_std`, `forbid(unsafe_code)`, host tested | geometry, easing, text measuring, rasterisation, shapes, blur, settings, search are pure and tested on the host; the kernel owns the framebuffer and the glue |

## 2. Tokens

All sizes are logical pixels on a 4 px grid; there is no HiDPI scaling. Colours are
`0xAARRGGBB` in `osjeff_core::style::{LIGHT, DARK}` and read through `theme::pal()`,
which follows the appearance in effect.

### 2.1 Colour (light | dark)

| Token | Light | Dark | Use |
|---|---|---|---|
| `window_bg` | `F6F6F8` | `2C2C2F` | unified title bar, toolbars, window body |
| `content_bg` | `FFFFFF` | `1E1E20` | text areas, lists, cards |
| `sidebar_bg` | `EDEDF1` | `262629` | sidebars |
| `separator` | black 10 % | white 10 % | hairlines |
| `text` / `text_secondary` / `text_tertiary` | `1D1D1F` / `6E6E73` / `A1A1A6` | `F5F5F7` / `A1A1A6` / `6E6E73` | text |
| `menubar_tint`, `dock_tint`, `menu_tint` | pale veils (70 %, 30 %, 80 %) | dark veils (62 %, 46 %, 80 %) | over the blurred backdrops |
| `field_bg`, `control_bg`, `control_border` | white, white, black 14 % | white 8 %, 12 %, white 14 % | fields and buttons |
| `danger` | `FF453A` | `FF453A` | destructive buttons, errors |
| lights | close `FF6B63`, minimise `FFC24A`, zoom `3FD07C`; unfocused grey `D1D1D6` / `4A4A4E` | | window controls |

Appearance setting: *Automática* (dark from 19:00 to 07:00 by the clock), *Clara*, *Escura*;
applied live from Controles or Configurações. Contrast on `window_bg`: body text 15.6:1 (light) and 12.8:1 (dark), secondary text
4.7:1 and 5.4:1 (all above 4.5:1).

### 2.2 Radii

`window` 12 (0 when zoomed) · `popover` 12 · `menu` 8 · `control` 6 · `dock` 22 · `tooltip` 6 ·
icon tile 22.5 % of its side (superellipse, n = 4). Corners are anti-aliased from cached
coverage masks (`osjeff_core::raster::CornerMasks`), identical for fills, strokes and the
window-corner repair.

### 2.3 Spacing and sizes

`4 8 12 16 20 24 32`. Menu bar 28 · title bar 32 (`window::TITLE_H`) · lights 12 px, pitch 20,
12 inset, 16 px hit area · menu row 24, separator 9 · bar icon 48 (magnified up to 76 over a
bump of 104 px), gap 8, padding 12/8, 8 from the bottom · Apps cell 136x128, icon 72 ·
Busca 640 wide, field 56, rows 40 · banners 344x68.

### 2.4 Type

Inter (Regular, Medium, Semibold; SIL OFL) for the interface and JetBrains Mono (Regular;
SIL OFL) for the terminal and the editor, both through the same glyph atlas
(`kernel/src/text.rs`, licences in `THIRD-PARTY.md`).

| Role | px | Use |
|---|---|---|
| caption | 11 | badges, scrollbar labels |
| footnote | 12 | secondary lines, tooltips |
| body | 13 | menu bar, buttons, lists, window titles (Medium) |
| callout | 15 | Busca rows |
| title3 / title2 / title1 | 17 / 22 / 28 | sheet and popover titles, Busca field, big numbers |
| mono | 15 (9 px pitch, 20 px line) | terminal, editor |

Vertical centring uses the cap height (`text::center_y`); strings are measured with the real
advances and kerning (`text::measure`) and cut with an ellipsis (`text::ellipsize`). The 8x8
bitmap font only survives in the crash screen.

### 2.5 Shadows

Separable analytic profiles (`osjeff_core::raster::shadow_profile`), two layers per window:
focused (blur 20, dy 14, 29 %) + (6, 3, 24 %); unfocused (14, 8, 17 %) + (4, 2, 15 %); menus
and popovers (12 to 14, dy 8, 31 %); the bar (12, dy 6, 24 %); banners (14, dy 8, 31 %). A
window being dragged keeps only the ambient layer.

### 2.6 Motion

Driven by real time (`dt` = timer ticks / 250), interruptible, and gated by the *reduce
motion* switch (`osjeff_core::anim::set_reduce_motion`; with it on every transition lands on
its end in the next frame).

| Transition | Model | Time |
|---|---|---|
| window open / close | bezier ease-out / ease-in, scale .92 and fade, from a cached texture | 220 / 160 ms |
| minimise / restore | scale and move to / from the app's icon in the bar | 300 ms |
| zoom | spring on the rectangle, content clipped live (never a squeezed bitmap) | ~280 ms |
| focus change | title bar and shadow cross-fade | 120 ms |
| bar magnification | one spring per icon (stiffness 420, damping 30) | ~200 ms |
| launch | two damped hops, 18 px | 640 ms |
| menu, popover, sheet, Busca | fade + 6 px slide | 140 ms |
| Apps | fade, icons rise 14 px | 220 ms |
| banners | slide from the right edge, ease-out in, ease-in out | 260 / 220 ms |
| tooltip | after 350 ms of rest, fade | 120 ms |

## 3. The toolkit (the API for wave 2)

Drawing lives in `kernel/src/desktop/ui.rs` (over `Canvas`, reading the current palette);
geometry, hit testing and state are pure and tested in `osjeff_core::widgets` and
`osjeff_core::chrome`. Draw functions take the interaction state as an argument, and the
caller derives `Control::Hover` / `Pressed` from the pointer.

| Widget | Draw | Geometry and state |
|---|---|---|
| push button (secondary, primary, destructive; normal, hover, pressed, disabled) | `ui::push_button(c, r, label, ButtonKind, Control)` | |
| text field (placeholder, focus ring, caret) | `ui::text_field(c, r, text, placeholder, focused, caret)`, `ui::text_field_frame` | |
| segmented control (tabs) | `ui::segmented(c, r, labels, selected)` | `widgets::segmented_rects/hit` |
| switch (animated knob) | `ui::switch(c, r, t256, enabled)` | `widgets::switch_rect/knob` |
| slider | `ui::slider(c, r, v, min, max, enabled)` | `widgets::slider_track/value/knob_x` |
| checkbox, radio | `ui::checkbox`, `ui::radio` | |
| list row, group box, separator, progress | `ui::list_row`, `ui::group_box`, `ui::separator`, `ui::progress` | |
| overlay scrollbar (fades) | `ui::overlay_scrollbar` | `widgets::ScrollbarFade`, `scroll_thumb` |
| menu row, tooltip | `ui::menu_item`, `ui::tooltip` | `chrome::menu_geom` |
| line graph, usage bar (for charts) | `ui::graph`, `ui::usage_bar` | |
| glass panel (blurred backdrop, tint, edge, shadow) | `glass::panel`, `BackdropSlot` | |
| text | `text::draw`, `draw_centered`, `draw_left`, `draw_right`, `draw_ellipsis`, `measure`, `wrap`, `draw_mono` | `osjeff_core::textlayout` |
| icons and glyphs | `icons::blit(c, Icon, x, y, size, opacity)`, `ui::draw_glyph(c, Glyph, x, y, size, argb)` | `osjeff_core::iconart` |
| colours | `theme::{pal, text, text_muted, window_body, toolbar, sidebar, surface, zebra, line, button_bg, tool_bg, ink, ink_dim, danger, ok, selection, accent}` | `osjeff_core::style` |

The **gallery** (`Ctrl+Alt+G`, also in the system menu) shows all of it live in four tabs
(controls, type, colours, icons) and is the reference for app authors. Tabs, a segmented
control and charts are all in the toolkit on purpose: the Tarefas app is meant to become an
activity monitor (CPU, memory, disk and network tabs with a process list).

## 4. System chrome

* **Menu bar** (28 px, glass baked into the cached wallpaper): the OSjeff mark (menu: Sobre,
  Configurações, Componentes, Reiniciar, Desligar, the last two behind a confirmation
  sheet), the focused app's name (Semibold; menu: Encerrar) and its menus Arquivo, Editar,
  Visualizar, Janela with real, enabled/disabled-aware entries (new window, close, minimise,
  zoom, undo/redo/cut/copy/paste/select all for the editor, browser zoom, open and save
  for the editor, the window list); on the right the network state, Controles, Busca and
  the clock with the date. Moving along the bar with a menu open switches menus; the
  keyboard (arrows, Enter, Esc) works too.
* **Controles** and the **calendar** popovers hang under their bar items.
* **Windows**: unified title bar, lights at the left (grey when unfocused, glyphs on
  hover), centred Medium title that never reaches the lights, 12 px corners, hairline,
  two-layer shadow, focus cross-fade, double click on the title zooms, resize edges and
  minimum sizes as before. Zoom fills the area between the menu bar and the bar.
* **Barra de apps**: floating glass panel centred at the bottom, an Apps button, a
  separator and the apps; magnification, running dots, tooltips, launch hop, a context
  menu per icon (Abrir / Nova janela / Encerrar). The backdrop is blurred once when the
  wallpaper is painted; a window under the panel makes it a plain translucent tint.
* **Apps** (`Apps` button): the wallpaper and windows blurred behind a grid of every app
  (system and installed) with a search field; type to filter, arrows move, Enter opens,
  the wheel scrolls, Esc closes.
* **Busca** (`Ctrl+Space`, or the bar's magnifier): apps, files of the volume (at most 600
  entries, five levels, indexed when it opens) and arithmetic (`12*(3+4)` shows `= 84`,
  Enter copies it).
* **Sheet** for restart and shut down (Enter or the red button confirms, Esc cancels).
* **Banners** slide in at the top right under the bar.
* **Pointer**: arrow, pointing hand over links, I-beam over text, drawn as vector shapes
  with an outline and a soft shadow (`osjeff_core::cursor`; one 24x28 box for all three).
* **HUD** (frame time, heap, threads) only with `Ctrl+Alt+H`.

## 5. Rendering model

Bottom to top, in every compose path:

1. the wallpaper with the menu-bar glass and the bar's blurred strip (cached in `BG`);
2. windows in z-order, each with its shadow (the static ones cached in `STATIC`);
3. the bar (live: its icons magnify; excluded from `STATIC`);
4. the menu-bar content (names, status items, clock);
5. overlays: menus, popovers, Apps, Busca, sheet, Alt+Tab;
6. banners, then the pointer, straight onto the framebuffer.

Blur is never computed per frame on a large area: a blurred backdrop is captured once when
a surface opens (menus about 200x200, Apps at a quarter of the resolution) and kept until it
closes. The hover repaint of Apps only redraws the cells that changed (`overlay_bounds`
returns that dirty rectangle). Scaled icons are cached per (icon, size) and the glyph atlas
is filled lazily.

## 6. Performance (QEMU TCG, 1280x720, `perf-trace` build)

Frame cost per path, mean over the run (microseconds; `tools/perf/summ.py`; before = the tree
at the start of this work):

| Scenario | before | after |
|---|---|---|
| idle: CPU per second | 0 + one 0.33 ms clock tick | 0 + one 0.35 ms clock tick |
| window drag (damage frame) | 2316 | 3673 |
| open and close a window (animation frame) | 11330 | 4134 |
| Apps / start panel: hover frame | 5051 | 642 |
| Apps / start panel: open (rebuild frame) | 14035 | 34264 |
| text, 48 characters | 283672 cycles | 39550 cycles at 13 px |
| rounded rectangle, 512x320 | 874120 cycles | 1113396 cycles (anti-aliased) |

The drag frame costs more because the window is now anti-aliased, has a soft shadow and a
title bar with real text; it stays under the 4 ms goal. Opening Apps costs one frame of
blur capture, paid once.

## 7. What changes where

| Area | Files |
|---|---|
| tokens, appearance | `kernel/src/theme.rs`, `osjeff_core/src/{style,settings}.rs` |
| text | `osjeff_core/src/{ttf,glyph,fontcache,textlayout}.rs`, `kernel/src/text.rs`, `assets/fonts/` |
| primitives | `osjeff_core/src/{gfx,raster}.rs`, `kernel/src/fb.rs`, `kernel/src/fb/{shapes,scale}.rs` |
| motion | `osjeff_core/src/anim.rs` |
| chrome geometry, search, cursor, widgets | `osjeff_core/src/{window,layout,chrome,widgets,search,cursor,iconart}.rs` |
| chrome drawing | `kernel/src/desktop/{chrome,dock,menubar,overlays,shell,glass,cursor,render}.rs` |
| toolkit and gallery | `kernel/src/desktop/{ui,gallery}.rs` |
| icons, glyphs | `kernel/src/{icons,glyphs}.rs` |

## 8. Compatibility rules for app content (wave 2)

* Draw inside `r.body()` (below `TITLE_H`); never use a literal for the title height.
* Content must not paint outside the window rectangle; the corners are repaired after the
  app draws, but a stray pixel under the hairline will show.
* Take colours from `theme::*` (they follow light and dark) and text from `text::*`;
  `ui::text`, `ui::button` and the other byte-string helpers still work and now use the
  new look.
* Fixed character grids (terminal, editor) use `text::mono_cell()`; other text is
  proportional, so measure instead of multiplying by a column count.

## 9. Memory

After the first frame (logged as `[trace] ui: memory ...`): glyph atlas about 62 KiB for 917
glyphs (grows as new sizes and characters are used), icon cache a few hundred KiB (128 px
sources plus the scaled copies; flushed when it passes 160 entries), the bar's blurred strip about
0.3 MiB, fonts 120 KiB embedded. The Apps backdrop (3.6 MiB at 1280x720) exists only while
the overlay is open. Building the atlas takes about 34 ms at boot (logged).

## 10. Known gaps after wave 1

* App interiors are token-driven but not redesigned: Tarefas, Imagens, the file list and
  the log still use the old layouts and fixed-pitch measurements in places; the browser page
  layout assumes fixed character widths. Wave 2 moves each app to the widgets above.
* No right-to-left or complex text; kerning is pair kerning only; no LCD text.
* The pointer ghost left by a frame that both repaints a region and moves the pointer is
  owned by another change (`main.rs` cursor path) and untouched here.
