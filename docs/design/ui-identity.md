# Kitsune shell: the decisions

The shell is what surrounds the apps: the top panel, the taskbar, the Apps launcher, Busca, window
snapping, workspaces, quick settings and the notification centre. This page records what each piece
does and why it is built that way. The tokens, the toolkit and the rendering rules are in
[`ui-design.md`](ui-design.md); the mark and palette in [`../brand/README.md`](../brand/README.md).

## 1. The decisions

| Aspect | Decision | Why |
|---|---|---|
| Window controls | **At the right, flat glyphs, 40x32 hit areas** (the corner pixel hits close). A 36x26 rounded hover fill, red fill and white glyph on close, dimmed when the window is inactive. | The corner is the easiest target to reach with the pointer, and glyphs read the same on any wallpaper or accent. |
| Title bar | **Small app tile and a left-aligned Medium title**, a **menu button** next to the controls, and a thin 2 px accent line on the focused bar. | The icon says whose window it is; the accent line is a quiet focus signature that works in light and dark. |
| Top panel | **Slim, 30 px**: Apps button, Busca and workspace dots at the left, **date and time in the centre**, a status pill at the right. | One strip carries the system state; the centred clock opens the calendar and the notification centre. An app's menus live behind its own window's menu button. |
| Launcher | **Full-screen Apps grid with a left category rail** (Todos, Sistema, Internet, Mídia, Utilitários), search on top and a Recentes row. | Categories make a long list scannable; the search looks at every category at once. |
| Taskbar | **Centred rounded floating bar**, radius 12, 40 px icons, Apps button first, a *Mostrar área de trabalho* sliver at the right end. Icons lift 3 px and show a tooltip; a pill marks the focused app and a dot the others. Drag to reorder pinned apps. | Predictable positions (icons do not change size under the pointer) and a clear running/focused state. |
| Notifications | **Calendar and notification centre** popover from the clock, a *Não perturbe* switch, banners that slide in at the top right under the panel. | History is one click away and silence is one switch. |
| Quick settings | A **status pill** in the panel opens a tile grid: Rede, Aparência (Claro / Escuro), Reduzir movimento, Não perturbar, Relógio 24 h, Reiniciar / Desligar. | The things people change most are one click from anywhere. |
| Window snapping | Top edge maximises, left / right edges give halves, corners give quarters; an **animated translucent preview**; `Alt+arrows` do the same from the keyboard. Geometry respects the work area (panel and taskbar). | Tiling by dragging or by keys, with a preview that shows the result before the button is released. |
| Workspaces | **2 to 4**, `Ctrl+Alt+Left/Right`, a dot indicator in the panel, a slide transition, `Ctrl+Alt+Shift+arrows` move a window. | Separate contexts without separate machines; opening a window of another workspace brings that workspace. |
| Search | **Busca** (`Ctrl+Space`): a centred field with apps, files and sums. | One field for what the user wants to find or compute. |
| Appearance | Light, dark or automatic (dark from 19:00 to 07:00), eight accent colours, applied live. | Contrast ratios are checked: body text 15.6:1 (light) and 12.8:1 (dark). |
| Motion | Short, springy, interruptible; every animation reports "active" only while it moves; the *reduce motion* switch makes every transition land on its end in the next frame. | An idle desktop costs no frames, and motion can be turned off. |

## 2. Visual language

* **Colour**: the indigo accent and the light / dark appearances; palettes with a clear contrast between
  the title bar and the body, and a 1 px light inner edge in dark.
* **Shape**: windows 8 px, controls 6 px, popovers 10 px, menus 8 px, taskbar 12 px, tooltip 6 px,
  icon tile 22 % of its side. 1 px crisp borders with a clear inner highlight; small shadows.
* **Blur**: only where it is cheap and meaningful (the panel strip, the popovers, the launcher backdrop).
  The taskbar is a plain translucent surface.
* **Icons**: a flat "tile" (rounded square, flat colour and one highlight shape), the glyph drawn from
  our own vector paths.
* **Pointer**: a slim arrow with a notched tail and an indigo-tinted outline.
* **Wallpapers**: six original presets: *Crepúsculo* (dusk gradient with soft geometric shapes),
  *Aurora*, *Mono escuro*, *Papel*, *Campo turquesa*, *Faixas de pôr do sol*.

## 3. Geometry (4 px grid)

| Item | Value |
|---|---|
| Panel | 30 px high, full width, translucent over a lightly blurred strip, 1 px bottom hairline |
| Title bar | 32 px (`TITLE_H`; apps draw under `r.body()`), buttons 40x32, menu button 32x32 |
| Taskbar | icons 40, gap 6, padding 10/8, 8 px above the bottom, radius 12, sliver 10 px |
| Snap zones | 6 px at the screen edges (top / left / right), corner zones 48 px along the edges |
| Work area | below the panel, above the taskbar (plus 8 px), full width |

## 4. Workspaces

2 to 4 virtual desktops, `Ctrl+Alt+Left/Right`, a dot indicator in the panel (the current one is a
pill; a dot is stronger when its workspace holds windows; click to go), a sideways slide with a fade
(`Anim::slide_in/out`), `Ctrl+Alt+Shift+Left/Right` and the window menu move a window (the view
follows it). Opening, activating from the taskbar or Alt+Tab a window of another workspace brings that
workspace. The state is `Window::{ws, off_ws}` and
`WindowManager::{switch_workspace, move_to_workspace, visible_workspaces}`, all pure and tested.

`Alt+Left` / `Alt+Right` go back / forward in the Navegador when it is focused;
`Alt+Shift+arrows` snaps in every window, including the browser.

## 5. How it was verified

Checked on the screenshots (`tools/perf/scen/w26-*.sh`, `w22-readme.sh`; dark and light; BIOS
1280x720 and UEFI 1280x800):

| # | Item | Result |
|---|---|---|
| 1 | controls are flat glyphs at the right | menu, minimise, maximise / restore and close at the right, red close on hover |
| 2 | no global menu bar | Apps, Busca and the workspace dots at the left; per-app menus behind the window's menu button |
| 3 | title left-aligned next to an icon | app tile and Medium title |
| 4 | taskbar | floating bar of flat tiles with pill and dot indicators, no magnification |
| 5 | clock in the centre of the panel | yes |
| 6 | launcher with a category rail | yes |
| 7 | Quick Settings and the calendar are popovers of the panel | a tile grid and a two-column calendar with notifications |

## 6. What is next in the shell

Persistence of the pinned order and of the workspace of each window across reboots; middle click on a
taskbar icon (the pointer driver reports only the left and right buttons: Shift+click opens a new window
instead); window thumbnails in the taskbar menu (titles only today). The overall plan is in
[`../ROADMAP.md`](../ROADMAP.md).
