# Kitsune shell: design system

Status: wave 1 (foundation, system chrome, the toolkit) and, for the system apps, wave 2 (section 11:
Tarefas, Registro, Ajustes, Calculadora, notificações). The rest of wave 2 re-skins each app's
> The macOS-like look described in sections 0 and 4 was judged too close to macOS. The
> direction now is `docs/design/ui-identity.md`; this file keeps the tokens, the toolkit and
> the rendering rules, and is updated as each step of that document lands.

Status: wave 1 (foundation, system chrome, the toolkit). Wave 2 re-skins each app's
content on top of the toolkit described here. The visual values are tokens that live in
one place (`kernel/src/theme.rs`, backed by the pure tables in `kitsune_core::style`);
the code paths named below are the stable API.

The file keeps its original name (`ui-macos.md`) because the brief called the target
"macOS-like". The result is inspired by that family of desktops and deliberately not a
copy: it keeps the principles (a clear hierarchy, translucency where it is cheap, rounded
geometry, soft shadows, springy motion, generous spacing) and has its own identity.

## 0. Identity

| | Kitsune |
|---|---|
| Mark | a bold prompt chevron `>` (white) on an indigo squircle; the menu-bar version is the bare chevron. No fruit, no wordmark borrowed from anyone |
| Names | **Apps** (the grid, replaces the start panel), **Busca** (one field for apps, files and sums), **Barra de apps** (the floating bar), **Controles** (network, appearance, switches), **Arquivos**, **Tarefas** (the activity monitor: CPU, Memória, Disco, Rede, Processos), **Registro** (the log), **Ajustes** (preferences), **Calculadora**, **Imagens**, **Componentes** (the widget gallery) |
| Mark | a bold prompt chevron `>` (white) on a flat indigo rounded square; the panel version is the bare chevron. No fruit, no wordmark borrowed from anyone |
| Names | **Apps** (the grid, replaces the start panel), **Busca** (one field for apps, files and sums), **Barra de tarefas** (the floating bar), **Configurações rápidas** (network, appearance, switches), **Arquivos**, **Tarefas**, **Monitor**, **Registro**, **Imagens**, **Componentes** (the widget gallery) |
| Accent | indigo `5B5CF6` by default; eight choices (Indigo, Turquesa, Violeta, Rosa, Coral, Âmbar, Verde, Grafite) |
| Icon language | a thick white glyph on a flat saturated rounded-square tile (22 % radius) with one highlight facet across the top-left corner and a 1 px bevel, no gradient and no gloss; every glyph is drawn from our own vector paths (`kitsune_core::iconart`), none is a traced system icon |
| Wallpaper | six original presets (*Crepúsculo*: a dusk gradient with soft geometric facets, pale by day and deep indigo at night; *Aurora*, *Mono*, *Papel*, *Turquesa* with rolling hills, *Pôr do sol* with bands) or an image; none is a wave |
| Motion | short and springy: windows pop in from 92 %, taskbar icons lift and slide on a spring each, launches hop twice; everything eases out and is interruptible |
| Language | Portuguese, plain: nothing in the UI talks about how the system is built; apps need no explanation |

## 1. Constraints that shape every decision

| Constraint | Consequence |
|---|---|
| Software renderer, 1280x720 (BIOS) or 1280x800 (UEFI), 24/32 bpp, BGR or RGB | every primitive is a format-aware `Canvas` method; no per-pixel format `match` in inner loops |
| `f32` is **emulated** in the kernel (the target has no SSE) | pixel work is integer / fixed point only (24.8 coverage, Q16 and Q8 maths); `f32` only for per-frame animation state (a few dozen values) |
| Damage tracking, cached static layer (`STATIC`), scene signature, drag and resize paths must stay correct | new visuals are layers inside the existing compositor paths (section 5), not a new compositor |
| An idle desktop must cost ~0 | every animation reports "active" only while it moves; springs and tweens snap to rest; nothing polls |
| 64 MiB heap, shared | caches are bounded and logged (section 9) |
| `kitsune_core` is `no_std`, `forbid(unsafe_code)`, host tested | geometry, easing, text measuring, rasterisation, shapes, blur, settings, search are pure and tested on the host; the kernel owns the framebuffer and the glue |

## 2. Tokens

All sizes are logical pixels on a 4 px grid; there is no HiDPI scaling. Colours are
`0xAARRGGBB` in `kitsune_core::style::{LIGHT, DARK}` and read through `theme::pal()`,
which follows the appearance in effect.

### 2.1 Colour (light | dark)

| Token | Light | Dark | Use |
|---|---|---|---|
| `window_bg` | `F6F6F8` | `2C2C2F` | unified title bar, toolbars, window body |
| `content_bg` | `FFFFFF` | `1E1E20` | text areas, lists, cards |
| `sidebar_bg` | `EDEDF1` | `262629` | sidebars |
| `separator` | black 10 % | white 10 % | hairlines |
| `text` / `text_secondary` / `text_tertiary` | `1D1D1F` / `6E6E73` / `A1A1A6` | `F5F5F7` / `A1A1A6` / `6E6E73` | text |
| `menubar_tint` (the panel), `dock_tint` (the taskbar), `menu_tint` | pale veils (70 %, 82 %, 80 %) | dark veils (62 %, 82 %, 80 %) | over the lightly blurred panel strip, the plain taskbar and the popover backdrops |
| `field_bg`, `control_bg`, `control_border` | white, white, black 14 % | white 8 %, 12 %, white 14 % | fields and buttons |
| `danger` | `FF453A` | `FF453A` | destructive buttons, errors |
| lights | close `FF6B63`, minimise `FFC24A`, zoom `3FD07C`; unfocused grey `D1D1D6` / `4A4A4E` | | window controls |

Appearance setting: *Automática* (dark from 19:00 to 07:00 by the clock), *Clara*, *Escura*;
applied live from Controles or Ajustes. Contrast on `window_bg`: body text 15.6:1 (light) and 12.8:1 (dark), secondary text
4.7:1 and 5.4:1 (all above 4.5:1).

### 2.2 Radii

`window` 8 (0 when maximised) · `popover` 10 · `menu` 8 · `control` 6 · `taskbar` 12 · `tooltip` 6 ·
icon tile 22 % of its side (circular corners). Corners are anti-aliased from cached
coverage masks (`kitsune_core::raster::CornerMasks`), identical for fills, strokes and the
window-corner repair.

### 2.3 Spacing and sizes

`4 8 12 16 20 24 32`. Panel 30 · title bar 32 (`window::TITLE_H`) · title buttons 40x32, menu button 32 · menu row 24, separator 9 · taskbar icon 40, gap 6, padding 10/8, 8 from the bottom · Apps cell 136x128, icon 72 ·
Busca 640 wide, field 56, rows 40 · banners 344x68.

### 2.4 Type

Inter (Regular, Medium, Semibold; SIL OFL) for the interface and JetBrains Mono (Regular;
SIL OFL) for the terminal and the editor, both through the same glyph atlas
(`kernel/src/text.rs`, licences in `THIRD-PARTY.md`).

| Role | px | Use |
|---|---|---|
| caption | 11 | badges, scrollbar labels |
| footnote | 12 | secondary lines, tooltips |
| body | 13 | panel, buttons, lists, window titles (Medium) |
| callout | 15 | Busca rows |
| title3 / title2 / title1 | 17 / 22 / 28 | sheet and popover titles, Busca field, big numbers |
| mono | 15 (9 px pitch, 20 px line) | terminal, editor |

Vertical centring uses the cap height (`text::center_y`); strings are measured with the real
advances and kerning (`text::measure`) and cut with an ellipsis (`text::ellipsize`). The 8x8
bitmap font only survives in the crash screen.

### 2.5 Shadows

Separable analytic profiles (`kitsune_core::raster::shadow_profile`), two layers per window:
focused (blur 20, dy 14, 29 %) + (6, 3, 24 %); unfocused (14, 8, 17 %) + (4, 2, 15 %); menus
and popovers (12 to 14, dy 8, 31 %); the bar (12, dy 6, 24 %); banners (14, dy 8, 31 %). A
window being dragged keeps only the ambient layer.

### 2.6 Motion

Driven by real time (`dt` = timer ticks / 250), interruptible, and gated by the *reduce
motion* switch (`kitsune_core::anim::set_reduce_motion`; with it on every transition lands on
its end in the next frame).

| Transition | Model | Time |
|---|---|---|
| window open / close | bezier ease-out / ease-in, scale .92 and fade, from a cached texture | 220 / 160 ms |
| minimise / restore | scale and move to / from the app's icon in the bar | 300 ms |
| zoom | spring on the rectangle, content clipped live (never a squeezed bitmap) | ~280 ms |
| focus change | title bar and shadow cross-fade | 120 ms |
| taskbar icon lift and reorder slide | one spring per icon (stiffness 420, damping 30) | ~200 ms |
| launch | two damped hops, 12 px | 640 ms |
| menu, popover, sheet, Busca | fade + 6 px slide | 140 ms |
| Apps | fade, icons rise 14 px | 220 ms |
| banners | slide from the right edge, ease-out in, ease-in out | 260 / 220 ms |
| tooltip | after 350 ms of rest, fade | 120 ms |
| Tarefas, a new 1 Hz sample | the curve scrolls one step, the headline number glides (ease-out) | 450 ms |
| Registro, Tarefas table | scrolling glides to its target (exponential, 80-90 ms constant); overlay scrollbar fades | ~250 ms |
| Ajustes switches | the knob glides (60 ms constant) | ~200 ms |
| Calculadora keyboard press | the key stays lit | 140 ms |
| banners | the dismiss line runs down over the chosen time (2 to 15 s) | 4 s default |

## 3. The toolkit (the API for wave 2)

Drawing lives in `kernel/src/desktop/ui.rs` (over `Canvas`, reading the current palette);
geometry, hit testing and state are pure and tested in `kitsune_core::widgets` and
`kitsune_core::chrome`. Draw functions take the interaction state as an argument, and the
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
| line graph, usage bar (dark-panel, byte-string versions) | `ui::graph`, `ui::usage_bar` | |
| smooth history chart with a hover value | `kit::chart(c, r, &Chart)` (curves, axis labels, scroll progress, hovered sample) | `kit::plot_of`, `kitsune_core::activity::{slice_at, sample_under, smooth121}` |
| usage bar, pressure gauge, chip, card, stat and key/value rows | `kit::{bar, pressure_gauge, chip, card, stat, kv}` | |
| search field, button with a glyph, sort arrow | `kit::{search_field, icon_button, sort_arrow}` | |
| hover and press for a whole window | `Desktop::live_hover` (a key per control; the window repaints only when it changes), `live_step`, `live_busy` | `desktop/live.rs` |
| glass panel (blurred backdrop, tint, edge, shadow) | `glass::panel`, `BackdropSlot` | |
| text | `text::draw`, `draw_centered`, `draw_left`, `draw_right`, `draw_ellipsis`, `measure`, `wrap`, `draw_mono` | `kitsune_core::textlayout` |
| icons and glyphs | `icons::blit(c, Icon, x, y, size, opacity)`, `ui::draw_glyph(c, Glyph, x, y, size, argb)` | `kitsune_core::iconart` |
| colours | `theme::{pal, text, text_muted, window_body, toolbar, sidebar, surface, zebra, line, button_bg, tool_bg, ink, ink_dim, danger, ok, selection, accent}` | `kitsune_core::style` |

The **gallery** (`Ctrl+Alt+G`, also in the system menu) shows all of it live in four tabs
(controls, type, colours, icons) and is the reference for app authors. `desktop/kit.rs` holds
the pieces the system apps needed beyond `ui.rs` (chart, gauge, chips, search field); like
`ui.rs` they draw at a rectangle the caller computed, follow the palette and measure text with
the real font.

## 4. System chrome

* **Menu bar** (28 px, glass baked into the cached wallpaper): the Kitsune mark (menu: Sobre,
  Ajustes, Componentes, Reiniciar, Desligar, the last two behind a confirmation
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
* **Top panel** (30 px, the strip is baked into the cached wallpaper under a mostly opaque tint,
  a hairline and an inner highlight): at the left **Apps** (the Kitsune mark and the word; a right
  click opens the system menu: Sobre, Configurações, Componentes, Reiniciar, Desligar) and the Busca
  magnifier and the workspace dots (click one to go there); in the **centre** the day, month and time (a dot beside it when there are unread
  notifications); at the right the **status pill** (network, appearance, power) that opens Quick
  Settings. There is no app name and no per-app menu strip: the app's menus are behind the menu
  button of its own title bar.
* **Quick Settings** (popover under the pill): a tile grid (Rede, Aparência, Movimento, Não perturbe,
  Relógio 24 h, Configurações), the eight accent swatches and Reiniciar / Desligar (both ask first).
  Tiles are accent-filled when on.
* **Calendar and notification centre** (popover centred under the clock, two columns): the date and the
  notification history (warnings and errors of the system log, newest first, *Limpar*, a
  *Não perturbe* switch that mutes the banners) at the left, the month at the right.
* **Windows**: a flat title bar with the app icon and a left-aligned Medium title; the menu button and
  the minimise, maximise / restore and close buttons at the **right** (40x32 cells, a 36x26 rounded
  hover fill, red close, dimmed when unfocused), a 2 px accent line on the focused bar, 8 px
  corners, a 1 px border with a clear inner highlight, two-layer shadow, focus cross-fade, double click on the title zooms, resize
  edges and minimum sizes as before. Dragging a title to an edge snaps (see `ui-identity.md`);
  zoom fills the work area between the panel and the bar.
* **Barra de tarefas**: a floating rounded bar (radius 12) centred at the bottom: the Apps
  button, a separator, the pinned apps, the apps that run without being pinned, a separator and a
  *Mostrar área de trabalho* sliver. No magnification: an icon lifts 3 px under the pointer and a
  tooltip follows after 350 ms. A pill under the icon marks the focused app, a dot the others (dimmer
  when all their windows are minimised). A click focuses, restores or (when it already has the focus)
  minimises; Shift+click opens a new window; dragging a pinned icon reorders it (the others slide);
  a right click lists the app's windows with *Nova janela*, *Fixar / Desafixar* and *Fechar*.
  The surface is a plain translucent tint (nothing is blurred). `Ctrl+Alt+D` also shows the desktop.
* **Apps** (`Apps` button in the panel or on the taskbar): the wallpaper and windows blurred behind
  a **left category rail** (Todos, Sistema, Internet, Mídia, Utilitários, each with its count), the
  search field on top of the content, a **Recentes** row (the last five apps launched, on *Todos*
  with an empty search) and the grid of every app (system and installed). Type to filter (a search
  looks at every category), arrows move, `Ctrl`+arrows walk the rail, Enter opens, the wheel
  scrolls, Esc closes. Categories come from a built-in table (`kitsune_core::launcher`).
* **Busca** (`Ctrl+Space`, or the bar's magnifier): apps, files of the volume (at most 600
  entries, five levels, indexed when it opens) and arithmetic (`12*(3+4)` shows `= 84`,
  Enter copies it).
* **Sheet** for restart and shut down (Enter or the red button confirms, Esc cancels).
* **Banners** slide in at the top right under the bar.
* **Pointer**: arrow, pointing hand over links, I-beam over text, drawn as vector shapes
  with an outline and a soft shadow (`kitsune_core::cursor`; one 24x28 box for all three).
* **HUD** (frame time, heap, threads) only with `Ctrl+Alt+H`.

## 5. Rendering model

Bottom to top, in every compose path:

1. the wallpaper with the panel strip (cached in `BG`);
2. windows in z-order, each with its shadow (the static ones cached in `STATIC`);
3. the taskbar (live: icons lift and slide; excluded from `STATIC`);
4. the panel content (Apps, Busca, clock, status pill);
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
| tokens, appearance | `kernel/src/theme.rs`, `kitsune_core/src/{style,settings}.rs` |
| text | `kitsune_core/src/{ttf,glyph,fontcache,textlayout}.rs`, `kernel/src/text.rs`, `assets/fonts/` |
| primitives | `kitsune_core/src/{gfx,raster}.rs`, `kernel/src/fb.rs`, `kernel/src/fb/{shapes,scale}.rs` |
| motion | `kitsune_core/src/anim.rs` |
| chrome geometry, search, cursor, widgets | `kitsune_core/src/{window,layout,chrome,widgets,search,cursor,iconart}.rs` |
| chrome drawing | `kernel/src/desktop/{chrome,dock,menubar,overlays,shell,glass,cursor,render}.rs` |
| toolkit and gallery | `kernel/src/desktop/{ui,kit,gallery}.rs` |
| system apps (section 11) | `kernel/src/desktop/{tarefas,logview,settings_ui,calc_ui,toasts_ui,live}.rs`, `kitsune_core/src/{activity,calc,settings,notify,klog,layout}.rs` |
| chrome drawing | `kernel/src/desktop/{chrome,dock,panel,overlays,shell,glass,cursor,render}.rs` |
| chrome drawing | `kernel/src/desktop/{chrome,taskbar,panel,overlays,shell,glass,cursor,render}.rs` |
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
sources plus the scaled copies; flushed when it passes 160 entries), fonts 120 KiB embedded. The Apps backdrop (3.6 MiB at 1280x720) exists only while
the overlay is open. Building the atlas takes about 34 ms at boot (logged).

## 10. Known gaps

* The system apps (section 11) are redesigned; Imagens, the file list, the editor, the terminal
  and the browser page layout are the other half of wave 2 (fixed-pitch measurements remain
  there).
* App interiors are token-driven but not redesigned: Tarefas, Imagens, the file list and
  the log still use the old layouts and fixed-pitch measurements in places; Wave 2 moves each app to the widgets above.
* No right-to-left or complex text; kerning is pair kerning only; no LCD text.
* The pointer ghost left by a frame that both repaints a region and moves the pointer is
  owned by another change (`main.rs` cursor path) and untouched here.

## 11. The system apps (wave 2)

All five are drawn with the toolkit above, in Portuguese with accents, with real text measuring
(numbers are right-aligned in their column instead of padded), and follow light and dark.
Window sizes are on the 4 px grid; interiors use 16 px margins, 12 px card radius and 28 px
controls.

**Tarefas** (default 860 x 592, minimum 700 x 460). A segmented control (CPU, Memória, Disco,
Rede, Processos) under the title bar. The first four share one layout: a section title with the
headline number, a chart card, a column of stat cards on the right and a list under the chart.
Processos is the table. The window has no Monitor sibling any more (`Kind::Monitor` is gone;
Busca still answers "monitor", "desempenho", "memória", "disco", "rede" and opens the right tab).

| Tab | What it shows | Source |
|---|---|---|
| CPU | total share and its 60 s chart; uptime; load average (1/5/15 min of the busy share); thread and process count; the processor; bars per process (threads: the scheduler's tick counters, apps: the time spent drawing their window) | `sysmon::CpuSampler`, `activity::LoadAvg` |
| Memória | heap in use (chart, axis in powers of two), pressure gauge (normal under 60 %, attention under 85 %, critical above), in use / free / total / peak / physical RAM, approximate memory per app (`App::approx_bytes`, WASM apps report their real figure) | allocator, `activity::Pressure` |
| Disco | volume, usage bar and percent, used / free / total, files and folders (inode counters), read and write per second with a chart, totals since boot, the disk's model and size | `vfs::statfs`, `ata::io_bytes`, IDENTIFY |
| Rede | link state, IP, mask, router, DNS, lease time left, received and sent rate with a chart, bytes and packets, errors when there are any | `netd::stats()` |
| Processos | PID, friendly name (the internal name in a tooltip and in the detail line), state, CPU, memory, time active; sort by any column (a second click reverses), search, selection; Reiniciar / Encerrar; a footer with processes, threads, CPU, memory and disk | process table, scheduler, `activity::{friendly_name, sort_tasks}` |

Internal names are translated: `compositor` is Interface, `fetcher` Rede (busca), `appd`
Aplicativos, `shelld` and `shelld2` Terminal (execução), `logd` Registro, `kernel` Sistema;
`shell 2` becomes Terminal 2. Ending an app closes its window. A service asks first: ending
Aplicativos closes every installed app, ending Terminal (execução) interrupts the running
commands; the other services cannot be ended (the button is disabled).

The sample arrives once a second. For 450 ms the chart scrolls by one step and the headline
glides; only that part of the window is repainted, one frame every 20 ms, and a window that is
minimised or closed costs nothing beyond the sampler. Hover over a chart shows the value and
"há N s" in a bubble.

**Registro** (880 x 540, minimum 720 x 360). Search field, segmented level filter (Tudo, Info,
Aviso, Erro), Seguir switch, Limpar, Salvar; a table card with Hora (mono), Nível (chip), Origem
and Mensagem (mono); a status line ("771 linhas", "3 de 45 linhas", what the last action did).
Scrolling by the wheel, arrows, Page Up/Down, Home and End glides; scrolling up stops following,
reaching the end resumes it. Salvar writes `/var/log/syslog.txt`.

**Ajustes** (820 x 596, minimum 720 x 480). Sidebar with nine sections and a page of grouped
cards: Aparência (theme, accent, reduce motion, notifications and their duration), Papel de
parede (thumbnails of the presets drawn live in the current appearance, the user's image by path
or through Arquivos), Barra de apps (magnification slider with a preview of real icons), Teclado
(US / ABNT2, a test field), Data e hora (the clock, 24 h, a searchable city list, the editor),
Rede, Disco, Energia (Reiniciar... and Desligar... open the sheet) and Sobre. Controls apply at
once; a slider is stored when it is released. Pages longer than the window scroll.

**Calculadora** (320 x 520, minimum 280 x 440). History strip, display (48 px shrinking to
20 px), memory row, six rows of rounded keys; the operator column carries the accent and the
pending operator is inverted. Percent follows the pocket-calculator rule (200 + 10 % adds 20),
`n` or the sign key changes sign, M+ and M- accumulate in a register that C does not clear.
Typed `x` and `:` are the multiplication and division keys, `,` is the decimal point.

**Notificações.** A 344 x 68 banner: a disc in the level's colour with its glyph, title, two
lines of text, a repeat counter, a close button while the pointer is over it and a hairline that
runs down over the chosen time.

Pointer feedback: every system window keeps a hover key (the control under the pointer plus the
pressed bit) and the compositor repaints it only when the key changes, so moving over a window
costs nothing unless something lights up.

Captures (QEMU/UEFI, `tools/perf/scen/w25-shots.sh`): `docs/img/ui-tarefas-light.png` (CPU),
`ui-tarefas-procs-dark.png`, `ui-ajustes-dark.png`, `ui-calc-light.png`, `ui-registro-dark.png`,
`ui-toast-dark.png`.
## 11. App interiors, wave 2a: Arquivos, Imagens, Editor, Terminal (W23)

These four apps are redesigned on the toolkit above. The rules they follow: geometry, hit testing,
filters, plans and selection models are pure and host tested in `kitsune_core`; the kernel
only draws and routes; text is measured (`text::*`), never counted in cells, except the fixed grids
of the editor and the terminal (`text::mono_cell_px`); no `text::legacy`; light and dark both from the
palette; frames are requested only while something moves (each app has an `animating()` predicate
that `is_dynamic`/`has_animation` read), so an idle desktop with all four open costs no frames.

Shared pieces added for them (not in the wave-1 toolkit): `kernel/src/desktop/appui.rs` (glyph tool
buttons and segmented control, the path-bar pill, a one-line field with selection and an eased caret,
the window-attached sheet with dim and slide, empty states, selection colours, caret curve) and
`kernel/src/desktop/appart.rs` (a cache over `kitsune_core::appart`: the file-type icons Pasta, Texto,
Imagem, App, Genérico, Disco, drawn in the accent, and 33 monochrome tool glyphs).

| App | Content | Pure core | Kernel |
|---|---|---|---|
| Arquivos | sidebar (Favoritos, Locais, the disk with its usage bar) on an accent-tinted vertical gradient, toolbar (back, forward, clickable path bar, list/icon switch, sort, search, preview), header with sort arrows, rows or icon cells, preview pane (Espaço), status bar, sheets (copy progress with cancel, confirmations, properties), rubber band, drag and drop with highlighted targets and a ghost, inline rename, context menus, inertial scroll and overlay scrollbar, empty states | `fileman::ui` (layout, columns, crumbs, hit, item geometry, band, drop plan, `Scroller`, search keys, preview kind, text preview), `appart` | `files.rs`, `files_ui.rs` |
| Imagens | glass toolbar, canvas with fit / fill / actual and spring zoom, pan with inertia, animated rotation, checkerboard under transparency, translucent info inspector, filmstrip with lazily made thumbnails, slideshow, save sheet, friendly errors | `viewer::ui` (layout, filmstrip maths, `Inertia`, `Slideshow`, `RotMap`), `viewer` (fit, `info_rows`, messages) | `viewer.rs` |
| Editor | gutter, current line, indent guides, selection runs and fade, eased gliding caret, overlay scrollbar, slim find / replace bar with buttons, status bar, title dot, close question and Open / Save as sheets (sidebar, icons), Ctrl +/-/0 | `editor2::ui` (layout, cell mapping, selection runs, guides, sheet and bar geometry, `FindLay::hit`), `editor2::dialog::status_bar` | `edit.rs`, `edit_ui.rs` |
| Terminal | strip with the tab and folder, prompt colours, selection (drag, word, line) and copy, overlay scrollbar, block / bar cursor, running pill, Ctrl +/-/0 | `termui` (layout, `Grid::cell_at`, `Selection`, `word_bounds`, `extract`, `split_prompt`) | `term.rs` |

Text size: `editor_font` and `terminal_font` in the settings file (11 to 24 px, default 15; a value
out of range is ignored; the default size is not written), changed with
Ctrl +, Ctrl - and Ctrl 0 and applied to every window of the app at once.

Caret blink: solid for half a second after input, then an eased 1.06 s blink, solid again after 12 s
(`appui::caret_alpha`); only the focused window asks for frames while it blinks.

Screenshots (every one reviewed in light and dark, `tools/perf/scen/w23-*.sh`): `docs/img/w23-files-*.png`,
`w23-viewer-*.png`, `w23-editor-*.png`, `w23-terminal-*.png`.

Known gaps: the Arquivos sidebar is a tinted gradient, not a live blur of what is behind the window
(a window is composited opaque, there is no backdrop under it); there is no column view; the
terminal selection is relative to the visible rows and ends at the next key, output or scroll; the
terminal draws one colour (escape sequences are dropped, not interpreted); the editor's
caret glides only along a row (a jump to another row or a scroll is immediate); the Open / Save as
list scrolls by whole rows.

## 11. Browser (wave 2)

The Navegador keeps the indigo accent and is drawn from the toolkit (`kernel/src/desktop/browser_ui.rs`;
geometry in `kitsune_core::layout::BrowserChrome`, state in `desktop/browser.rs`).

* **Toolbar** (48 px, same colour as the title bar): back, forward, reload (becomes stop while
  loading), a rounded omnibox (security badge, host highlighted at rest, animated star) and "+".
  A 2 px progress line runs under the omnibox. Lock and "Conexão segura" only for verified TLS; a
  warning triangle for HTTP or an accepted certificate; a click opens a glass popover (host,
  issuer, validity, verification).
* **Tabs** (36 px strip, up to 8, shown from the second tab): rounded pills, letter badge,
  ellipsised title, close button, open and close animated with tweens (ghost entry for a closing tab).
* **Page area**: always light (never dark-inverted); proportional text from the real font
  (italic is a 12 degree slant), tables, forms in the toolkit look. Overlay scrollbar, inertial wheel,
  zoom pill, accent selection, slim glass find bar, context menu.
* **Start page** and **error pages** use the chrome's palette (light and dark) and one primary button.
* **Cost**: a scroll, hover or key in the browser repaints only its client area
  (`Desktop::client_dirty`, `render_client_only`, `client_only_frame`); a window in front, the app bar
  reaching the window, a drag, an overlay or a title change fall back to the full frame. An idle
  browser window costs no frames. Numbers in `docs/TESTING.md`.
* Toolkit additions made for it: eight `iconart::Glyph` variants (ChevronLeft, Reload, Lock,
  Warning, Star, StarFill, Globe, Home), `text::{measure_q8, has_glyph, draw_slanted}`,
  `fontcache::TextEngine::has_glyph`, glyph and kerning caches in `ttf::Font`.
