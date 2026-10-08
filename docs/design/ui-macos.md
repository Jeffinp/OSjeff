# UI design system ("macOS feel")

Status: wave 1 (foundation + system chrome). Wave 2 re-skins each app's content on
top of the toolkit described here. Code paths named below are the stable API; the
visual values are tokens that live in one place (`kernel/src/theme.rs`, mirrored
by the pure tables in `osjeff_core::style`).

The goal is a desktop that looks and moves like a current macOS (Big Sur ... Tahoe):
quiet, translucent where it is cheap to be, animated with springs, and presented as
a plain operating system (no self-referential or implementation-language text in
anything the user sees; the `OSJEFF` name only appears as the system menu).

## 1. Constraints that shape every decision

| Constraint | Consequence |
|---|---|
| Software renderer, 1280x720, 32 bpp (UEFI) or 24 bpp (BIOS), BGR or RGB | all primitives are format-aware `Canvas` methods; no per-pixel format `match` in inner loops |
| `f32` is **emulated** in the kernel (target has no SSE) | pixel work is integer/fixed point only. `f32` is allowed for per-frame animation state (tens of values), never per pixel |
| Damage tracking + cached static layer (`STATIC`), scene signature, wheel, drag and resize paths must stay correct | new visuals are layers inside the existing paths (section 8), not a new compositor |
| Idle desktop must cost ~0 | every animation reports "active" only while it moves; springs snap to rest below an epsilon; nothing polls |
| Heap is 64 MiB, shared | caches are bounded and logged (section 9) |
| `osjeff_core` is `no_std`, `forbid(unsafe_code)`, host tested | geometry, easing, text measuring, rasterisation, shapes, blur, settings are pure and tested on the host; the kernel only owns the framebuffer and the glue |

## 2. Tokens

All sizes are logical pixels on a 4 px grid (the screen is 1280x720 or larger;
there is no HiDPI scaling). Colours are `0xRRGGBB`; `a=` is straight alpha.

### 2.1 Colour (light | dark)

| Token | Light | Dark | Use |
|---|---|---|---|
| `window_bg` | `F6F6F8` | `2C2C2F` | unified title bar + toolbars + window body |
| `content_bg` | `FFFFFF` | `1E1E20` | text areas, lists, fields |
| `sidebar_bg` | `EDEDF1` | `262629` | sidebars |
| `separator` | `000000 a=.10` | `FFFFFF a=.10` | hairlines (1 px, 0.5 alpha look) |
| `text` | `1D1D1F` | `F5F5F7` | primary text |
| `text_secondary` | `6E6E73` | `A1A1A6` | secondary |
| `text_tertiary` | `A1A1A6` | `6E6E73` | placeholders, disabled |
| `menubar_tint` | `F6F6F8 a=.70` | `1E1E20 a=.62` | over the blurred wallpaper |
| `dock_tint` | `FFFFFF a=.30` | `1E1E22 a=.46` | over the blurred backdrop |
| `menu_tint` | `F2F2F5 a=.80` | `2A2A2E a=.80` | context menus, popovers |
| `field_bg` | `FFFFFF` | `FFFFFF a=.08` | text fields |
| `control_bg` | `FFFFFF` + border | `FFFFFF a=.12` | buttons |
| `accent` | from settings (default `0A84FF`) | same | selection, focus, primary button, toggles |
| `accent_text` | `FFFFFF` | `FFFFFF` | on accent |
| `danger` | `FF453A` | `FF453A` | |
| traffic lights | `FF5F57` `FEBC2E` `28C840` | same | active window |
| traffic lights, inactive | `D1D1D6` | `4A4A4E` | unfocused window |

Accent palette (`osjeff_core::settings::ACCENTS`): Blue `0A84FF` (default), Purple
`BF5AF2`, Pink `FF375F`, Red `FF453A`, Orange `FF9F0A`, Yellow `FFD60A`, Green
`32D74B`, Graphite `8E8E93`.

Legacy app content (wave 1 only) keeps its light surface in both appearances; see
section 10 for what that means and how wave 2 removes it.

### 2.2 Radii

| Token | px | Use |
|---|---|---|
| `r_window` | 12 | windows (0 when zoomed) |
| `r_popover` | 12 | popovers, Control Center, toasts (14) |
| `r_menu` | 8 | context menus; menu item highlight 5 |
| `r_control` | 6 | buttons, fields, segmented controls |
| `r_dock` | 22 | dock panel |
| `r_tile` | 22.5 % of the icon side | app icon squircle |
| `r_tooltip` | 6 | tooltips |

Corners are anti-aliased from cached coverage masks (`osjeff_core::raster::CornerMask`),
circular for UI and a superellipse (n=4) "continuous corner" for icon tiles and the dock.

### 2.3 Spacing and sizes

`4 8 12 16 20 24 32 40`. Menu bar 28. Title bar 32 (`window::TITLE_H`). Traffic light
12 with 8 gap, 12 inset (hit area 16). Toolbar row 40. Control height 24 (small 20,
large 32). Menu item 24 high, 8 px side padding, separator 9 px. Dock: icon 48, gap 8,
padding 8, bottom margin 8, magnification up to 76 over a bell of radius 96 px.
Launchpad grid cell 128x120, icon 72.

### 2.4 Type scale (Inter, SIL OFL; `kernel/src/text.rs`)

| Role | px | weight | Use |
|---|---|---|---|
| `caption` | 11 | Regular | tooltips, badges, scrollbar labels |
| `footnote` | 12 | Regular | secondary lines |
| `label` / body | 13 | Regular / Medium / Semibold | menu bar, buttons, lists, window titles (Medium, app name Semibold) |
| `callout` | 15 | Regular/Medium | Spotlight rows, toasts title |
| `title3` | 17 | Semibold | popover headers |
| `title2` | 22 | Semibold | section titles |
| `title1` | 28 | Semibold | Launchpad/Spotlight field, big numbers |

Line height = font ascent+descent; vertical centring uses the cap height
(`text::center_y`). The 8x8 bitmap font survives only for the terminal and editor
monospace grids (`font.rs`).

### 2.5 Shadows

Two layers per surface, drawn as separable blurred profiles (`osjeff_core::raster::shadow_profile`):

| Surface | ambient (blur, dy, a) | key (blur, dy, a) |
|---|---|---|
| window, focused | 40, 14, .30 | 12, 4, .30 |
| window, inactive | 28, 8, .20 | 8, 3, .20 |
| menu / popover | 24, 8, .26 | 6, 2, .22 |
| dock | 24, 6, .18 | n/a |
| toast | 20, 6, .24 | 6, 2, .20 |

### 2.6 Motion

Real-time driven (`dt` from the timer, not frame counts), interruptible, and gated by
the global *reduce motion* switch (`osjeff_core::anim::set_reduce_motion`): with it on,
every transition jumps to its end value in the next frame.

| Transition | Model | Duration / parameters |
|---|---|---|
| window open | bezier `(.2,.8,.2,1)`, scale .92 -> 1 + fade | 220 ms |
| window close | bezier `(.4,0,1,1)`, scale 1 -> .92 + fade | 160 ms |
| minimise / restore | scale + translate to/from the dock icon, bezier `(.3,.7,.2,1)` | 300 ms |
| zoom (maximise / restore) | spring, mass 1, stiffness 260, damping 28 on the rect | ~280 ms |
| focus change | title bar / shadow cross-fade, linear | 120 ms |
| dock magnification | spring per icon, stiffness 420, damping 30 | settles ~200 ms |
| dock bounce on launch | two damped hops | 2 x 320 ms |
| menu / popover | fade + 6 px slide, bezier `(.2,.8,.2,1)` | 140 ms |
| Launchpad / Spotlight | fade + scale .96 -> 1 (Launchpad icons scale 1.1 -> 1) | 260 ms / 160 ms |
| toast | slide from the right + fade; spring | 300 ms in, 220 ms out |
| scrollbar | fade out after 800 ms idle | 200 ms |

## 3. Components (the toolkit)

All widgets live in `kernel/src/desktop/ui.rs` (drawing, over `Canvas`) with the
pure state, hit-testing and geometry in `osjeff_core::widgets` and
`osjeff_core::chrome`. The old `button`/`input_box`/`scrollbar`/`graph` helpers keep
their signatures (so apps keep compiling) but now draw in the new style.

| Component | Drawing API | Pure state |
|---|---|---|
| text (`draw_text`, `measure`, `ellipsize`, `text_in_rect`) | `text.rs` | `osjeff_core::textlayout` |
| push button (default / primary / destructive / disabled / pressed) | `ui::push_button` | `widgets::ButtonState` |
| text field (focus ring, placeholder, caret, selection) | `ui::text_field` | `widgets::FieldState` |
| segmented control | `ui::segmented` | `widgets::Segmented` |
| toggle switch (animated knob) | `ui::switch` | `widgets::Switch` |
| slider | `ui::slider` | `widgets::Slider` |
| checkbox, radio | `ui::checkbox`, `ui::radio` | |
| scrollbar (thin overlay, fades) | `ui::overlay_scrollbar` | `widgets::ScrollbarFade` |
| list row (hover/selected with accent pill) | `ui::list_row` | |
| context menu / menu panel | `ui::menu_panel`, `ui::menu_item` | `chrome::MenuLayout` |
| tooltip | `ui::tooltip` | `chrome::Tooltip` |
| popover (arrow-less, anchored) | `ui::popover_frame` | `chrome::popover_rect` |
| progress bar | `ui::progress` | |
| separator, group box | `ui::separator`, `ui::group_box` | |
| icon (squircle tiles, scaled, cached) | `icons::blit` | `osjeff_core::raster::Surface` |

The **widget gallery** (hidden, `Ctrl+Alt+G`) shows every component in the current
appearance and has an appearance switch, so a screenshot run exercises light and dark.

## 4. System chrome

* **Menu bar** (28 px, full width, translucent over the blurred wallpaper, baked into
  the background cache). Left: the system menu (OSJEFF mark: About, Settings, Reboot,
  Shut Down with confirmation), the focused app name in Semibold and its menus
  (File, Edit, View, Window with real, wired entries). Right: network status, Control
  Center, clock (date + time, 24 h/12 h from settings), Spotlight magnifier. Item
  geometry is `osjeff_core::chrome::menubar_layout` (host tested). The performance
  HUD is hidden (`Ctrl+Alt+H`).
* **Windows**: unified title bar (32 px, same colour as the body), traffic lights at
  the left with `x`, `-`, `+` glyphs on hover, centred Medium title, 12 px AA corners,
  1 px hairline, large soft shadow (stronger when focused), inactive windows dimmed
  with grey lights. Double click on the title = zoom. Resize edges and minimum sizes
  as before.
* **Dock**: floating translucent panel centred at the bottom, magnification, running
  indicator dots, tooltips, bounce on launch, context menu. Geometry and the
  magnification curve are `osjeff_core::chrome::dock_*` (host tested).
* **Launchpad** (replaces the start panel): full-screen blurred overlay, icon grid,
  search field (type to filter), Esc closes. **Spotlight** (`Ctrl+Space`): one field,
  apps, files from the VFS (bounded), calculator expressions.
* **Notifications**: banners top-right under the menu bar.
* **Wallpapers**: dynamic-style gradients (light and dark presets) or an image.
* **Cursor**: arrow, pointing hand and I-beam sprites (`kernel/src/cursor_art.rs`;
  hot spot and size constants centralised in `desktop/mod.rs`).

## 5. Rendering model

Layers, bottom to top, in every compose path:

1. wallpaper + menu-bar backdrop + dock backdrop (cached in `BG`, painted once);
2. windows in z-order, each with its shadow (static ones cached in `STATIC`);
3. dock (icons are drawn live because they magnify; the panel is a blit of the
   cached blurred backdrop when no window is under it, else a tinted translucent fill);
4. menu bar content (title, status items, clock) over the cached strip;
5. overlays: menus, popovers, Control Center, Launchpad, Spotlight, Alt+Tab;
6. toasts (framebuffer overlay restored from `BACK`), then the cursor.

Blur is **never** computed per frame on large areas. A blurred backdrop is built when
a surface opens (menu: ~220x200, Launchpad: full screen at 1/4 resolution then
upsampled, toast: ~360x80) and kept until it closes. Shadows are analytic separable
profiles, no 2-D cache. Scaled icons are cached per (icon, size).

## 6. Performance budget

Targets on QEMU TCG (no KVM), 1280x720, `perf-trace` build:

| Interaction | Budget (frame cost) | Notes |
|---|---|---|
| idle (clock tick) | <= 0.3 ms per second, 0 frames otherwise | `ClockLocal` path |
| cursor move only | <= 0.1 ms | unchanged path |
| window drag (damage frame) | <= 4 ms **goal**; the 800x520 window damage blit dominates in TCG, see results | shadow is single-layer while dragging |
| dock magnification frame | <= 4 ms | 700x100 region, cached scaled icons |
| menu open / hover | <= 2 ms | cached blur |
| Launchpad open | one frame <= 60 ms (blur cache), animation frames <= 8 ms | 1/4 res blur |
| text: 100 glyphs | <= 0.1 ms | cached coverage bitmaps |

Measured numbers (before and after) are in `docs/TESTING.md`, section "Custo de
quadro da interface". Microbenchmarks of the pure primitives are in `bench/`.

## 7. What changes where

| Area | Files |
|---|---|
| tokens, appearance | `kernel/src/theme.rs`, `osjeff_core/src/style.rs`, `osjeff_core/src/settings.rs` |
| text | `osjeff_core/src/{ttf,glyph,textlayout}.rs`, `kernel/src/text.rs`, `assets/fonts/` |
| primitives | `osjeff_core/src/{gfx,raster}.rs`, `kernel/src/fb.rs` |
| motion | `osjeff_core/src/anim.rs`, `kernel/src/desktop/motion.rs` |
| chrome geometry | `osjeff_core/src/{window,layout,chrome}.rs` |
| chrome drawing | `kernel/src/desktop/{chrome,dock,menubar,launchpad,spotlight,controlcenter}.rs`, `render.rs` |
| widgets | `kernel/src/desktop/ui.rs`, `osjeff_core/src/widgets.rs`, `desktop/gallery.rs` |
| icons | `kernel/src/icons.rs` |
| cursor | `kernel/src/cursor_art.rs` |
| compositor glue | `kernel/src/main.rs` (menu bar upload line, animation flag), `desktop/mod.rs` |

## 8. Compatibility rules for app content (wave 2)

* Draw inside `Rect` below `r.y + TITLE_H`; `TITLE_H` is 32 now. Use `window::TITLE_H`,
  never a literal.
* Content must not paint outside the window rect; the chrome (corner rounding,
  hairline, traffic lights) is drawn by `draw_window` and clipped by its mask.
* Prefer `ui::*` widgets and `theme::tokens()` colours (they follow light/dark).
  Colours baked into legacy code keep working; they are simply light-themed.

## 9. Memory

Logged at boot (`[trace] ui:` lines) and at runtime in the monitor: glyph cache
(bytes, glyphs), icon cache (bytes), shadow/backdrop caches. Budget: fonts 90 KiB
embedded, glyph cache <= 1 MiB, icon sources (128 px) 0.9 MiB, scaled icon cache
<= 2 MiB, Launchpad backdrop 3.7 MiB while open.

## 10. Known gaps after wave 1

* App interiors still use the old 8x8 text and light surfaces; they sit inside the
  new chrome. Dark mode darkens the chrome and token-driven widgets only.
* No right-to-left or complex text; kerning is pair kerning only.
* No subpixel (LCD) text, deliberately: grayscale AA looks the same in every
  framebuffer format.
