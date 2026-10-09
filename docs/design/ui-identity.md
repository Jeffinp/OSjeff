# Kitsune shell: identity

Status: this document replaces the "macOS-like" brief of `ui-macos.md` (which keeps the
token tables, the toolkit and the rendering rules, and is cross-linked from here). The owner
looked at the first shell and said it came out *exactly like macOS*: traffic lights on the
left, a global menu bar with a bold app name, a magnifying dock. The goal now is an **original**
shell that takes the best ergonomics of macOS, Windows 11 and the Linux desktops (GNOME, KDE
Plasma, Cinnamon, elementary) and combines them into something that does not read as a copy of
any of them.

## 0. Research method and what was looked at

Reference screenshots were fetched through the sandbox proxy from Wikipedia / Wikimedia Commons
into the scratch directory (never into the repository, never committed; deleted when the work
ended) and looked at one by one:

| Reference | File (Wikipedia / Commons) | What it showed |
|---|---|---|
| macOS Sonoma | `MacOS_Sonoma_Desktop.png` | menu bar with Apple mark + bold app name + text menus on the left, status icons and clock on the right; magnifying dock of gloss squircles; traffic lights |
| Windows 11 | `Windows_11_Desktop.png` | no top bar; centred floating-ish taskbar with Start, search, pinned and running icons, tray and clock at the right; Start menu with a search field, a pinned grid, recents and categories |
| GNOME 48 | `GNOME_48.0_on_GNOME_OS.png` | one slim top panel: Activities left, clock in the *centre*, a single pill of status icons at the right; the Quick Settings popover is a tile grid (network, dark style, power mode) with a slider; headerbar with the title centred and close at the right |
| KDE Plasma 6 | `KDE_Plasma_6.4.5_Light.png` | bottom panel, application launcher with a **left category rail**, search field at the top, a power row at the bottom; window controls at the **right** (minimise, maximise, close) as flat glyphs; title left aligned with the app icon; a menu button in the toolbar |
| elementary OS 8 | `ElementaryOS8.0-Desktop.png` | slim top panel (Applications left, clock centre, indicators right), a floating dock with a small running dot |
| Cinnamon (Mint 22) | `LinuxMint22-Wilma-English.png` | bottom panel with menu button, pinned launchers, window list and tray; controls at the right; flat, dense |

GNOME's headerbar and KDE's window controls are the two clearest answers to "not macOS":
controls on the right as flat glyph buttons, the title next to an icon. Windows 11 gives the
floating rounded taskbar; GNOME the centred clock with a calendar + notifications popover and
the Quick Settings tile grid; KDE the launcher with a category rail.

## 1. Comparison and decisions

| Aspect | macOS | Windows 11 | GNOME | KDE Plasma | Cinnamon / elementary | **Kitsune decision** and why |
|---|---|---|---|---|---|---|
| Window controls | three coloured dots, left | flat glyph buttons, right, 46x32, red close on hover | round glyph buttons, right (close only by default) | flat glyphs, right | flat glyphs, right | **Right, flat glyphs, 40x32 hit areas** (corner pixel hits close: Fitts), a 36x26 rounded hover fill, red fill and white glyph on close, dimmed when inactive. No dots, nothing on the left: this is the single biggest macOS tell |
| Title bar | centred title, no icon | left title, app icon | centred, headerbar | left title, app icon | left | **Small app tile + left-aligned Medium title**, a **menu button** next to the controls; a thin accent line on the focused bar (ours: a quiet signature, none of the others has it) |
| Panel / menu bar | global bar, bold app name, per-app menus | none (taskbar only) | top panel, clock centre | bottom panel | top panel (elementary) / bottom (Cinnamon) | **Slim top panel (30 px)**: Apps button + workspace dots at the left, **date and time in the centre**, a status pill at the right. No bold app name, no global menus (per-app menus live in the window's menu button) |
| Launcher | Launchpad (full screen grid, paged) | Start menu (pinned, recents, categories) | Activities grid | Kickoff (category rail) | Mint menu / Slingshot | **Full-screen Apps grid with a left category rail** (Todos, Sistema, Internet, Mídia, Utilitários), search on top, a Recentes row: GNOME overview + KDE rail + Windows recents |
| Taskbar / dock | magnifying dock, gloss icons, dot | floating-ish centred bar, pill indicator under the icon, no magnification | no dock by default | task manager in the panel | window list / floating dock | **Centred rounded floating taskbar**, radius 12, 40 px icons, **no magnification** (subtle lift + tooltip), pill indicator (long = focused, dot = running), Apps button first, a *Mostrar área de trabalho* sliver at the right end, drag to reorder pinned apps |
| Notifications | banners top right, centre in a side panel | banners bottom right, centre with the calendar | banners top centre, list under the calendar | popup + history in the tray | similar | **Calendar + notification centre popover from the clock** (GNOME/Windows 11), a *Não perturbe* switch, banners keep sliding in at the top right under the panel |
| Quick settings | Control Center (tile + slider grid) | tile grid + sliders flyout | Quick Settings pill + grid | tray popups | tray applets | **Quick Settings pill** in the panel opening a tile grid: Rede, Aparência (Claro / Escuro), Reduzir movimento, Não perturbar, Relógio 24 h, Reiniciar / Desligar. No sliders (no volume or brightness hardware to drive) |
| Window snapping | none native (tiling since Sequoia) | drag to edge: halves, quarters, maximise; snap layouts | half tiling to edges, maximise at top | quick tile to edges and corners | edge tiling | **Windows 11 / KDE model**: top edge = maximise, left / right = half, corners = quarter, an **animated translucent preview**, `Alt+arrows`; geometry respects the work area (panel and taskbar) |
| Workspaces | Spaces (Ctrl+arrows) | virtual desktops | dynamic workspaces, Ctrl+Alt+arrows | virtual desktops | workspaces | **2-4 workspaces**, `Ctrl+Alt+Left/Right`, a dot indicator in the panel, slide transition, `Ctrl+Alt+Shift+arrows` moves a window (see section 5) |
| Search | Spotlight (centre field) | search in Start / taskbar | type in the overview | KRunner | menu search | **Busca** (`Ctrl+Space`) stays: a centred field with apps, files and sums; the launcher has its own filter field |
| File dialogs | sheets slide from the title bar | modal window | modal with headerbar | modal | modal | unchanged (owned by the apps work); sheets stay centred modals |

## 2. Own visual language

* **Colour**: the indigo accent and the light / dark appearances stay; palettes get a little
  crisper contrast between the title bar and the body, and a 1 px light inner edge in dark.
* **Shape**: windows 8 px, controls 6 px, popovers 10 px, menus 8 px, taskbar 12 px, tooltip
  6 px, icon tile 22 % of its side. 1 px crisp borders with a clear inner highlight. Shadows
  smaller than before (the shell reads flatter and more confident).
* **Blur**: only where it is cheap and meaningful (the panel strip and the popovers, the
  launcher backdrop). The taskbar is a plain translucent surface.
* **Icons**: a squarer flat "tile" (rounded square, flat colour and one highlight shape, no
  vertical gradient or gloss), the glyph drawn from our own vector paths.
* **Pointer**: a slimmer arrow with a distinctive notched tail and an indigo-tinted outline.
* **Wallpapers**: six original presets (no wave): *Crepúsculo* (dusk gradient with soft
  geometric shapes), *Aurora*, *Mono escuro*, *Papel* (paper light), *Campo turquesa*, *Faixas
  de pôr do sol*.
* **Title bar signature**: the focused bar carries a 2 px accent line along its top edge.

## 3. Geometry summary (4 px grid)

| Item | Value |
|---|---|
| Panel | 30 px high, full width, translucent over a lightly blurred strip, 1 px bottom hairline |
| Title bar | 32 px (`TITLE_H`, unchanged: apps keep drawing under `r.body()`), buttons 40x32, menu button 32x32 |
| Taskbar | icons 40, gap 6, padding 10/8, 8 px above the bottom, radius 12, sliver 10 px |
| Snap zones | 6 px at the screen edges (top / left / right), corner zones 48 px along the edges |
| Work area | below the panel, above the taskbar (plus 8 px), full width |

## 4. Verification: the "distance from macOS" checklist

Checked at the end against our own screenshots, side by side with the references above (in
light and in dark):

1. Window controls are not coloured dots and are not on the left.
2. No global menu bar, no bold app name, no per-app menu strip across the top.
3. The title is left-aligned next to an app icon, not centred.
4. The bottom bar does not magnify and is not a gloss-squircle dock (flat tiles, pill
   indicators).
5. The wallpaper is not a purple-blue wave.
6. The pointer silhouette is not Apple's.
7. The clock is in the centre of the panel, not at the far right.
8. The launcher has a category rail (not a plain paged grid).
9. Quick Settings and the calendar are popovers of the panel (tile grid), not a Control Center.
10. Several things remain *recognisable ergonomics* that every desktop shares (rounded windows,
    soft shadows, springs): they are not claimed as unique.

The result of this comparison is recorded in `CHANGELOG.md` and in the section "Result" at the
end of this file once the last step lands.

## 5. Workspaces (step g: done)

2 to 4 virtual desktops, `Ctrl+Alt+Left/Right`, a dot indicator in the panel (the current one is a
pill; a dot is stronger when its workspace holds windows; click to go), a sideways slide with a fade
(`Anim::slide_in/out`), `Ctrl+Alt+Shift+Left/Right` and the window menu move a window (the view
follows it). Opening, activating from the taskbar or Alt+Tab a window of another workspace brings that
workspace. The structure allowed it cleanly: `Window::{ws, off_ws}` and
`WindowManager::{switch_workspace, move_to_workspace, visible_workspaces}`, all pure and tested.

## 6. Implementation order (small verified commits)

a. this document; b. controls at the right, left titles, title-bar menu button, edge snapping
with preview and `Alt+arrow`; c. top panel, calendar + notification centre, Quick Settings
(the menu bar and the Control Center are removed); d. the taskbar; e. tiles, pointer,
wallpapers, radii and borders; f. launcher categories; g. workspaces.

`Alt+Left` / `Alt+Right` keep going back / forward in the Navegador when it is focused (the
browser claimed them first); `Alt+Shift+arrows` snaps in every window, including the browser.

## 7. Result: the checklist against our own screenshots

Checked on the final screenshots (`tools/perf/scen/w26-*.sh`, `w22-readme.sh`; dark and light; BIOS
1280x720 and UEFI 1280x800) against the six references of section 0:

| # | Item | Result |
|---|---|---|
| 1 | controls are flat glyphs, at the right | yes: menu, minimise, maximise / restore and close at the right, red close hover; no dots |
| 2 | no global menu bar or bold app name | yes: Apps, Busca and the workspace dots at the left, per-app menus behind the window's menu button |
| 3 | title left-aligned next to an icon | yes (app tile, Medium title) |
| 4 | no magnification, no gloss squircle dock | yes: floating taskbar of flat tiles with pill and dot indicators |
| 5 | wallpaper is not a purple-blue wave | yes: six original presets; the default is a dusk gradient with geometric facets (it is purple at night, so it is the preset closest to a macOS mood by colour, but the shapes are angular facets, not a wave) |
| 6 | pointer silhouette not Apple's | yes: a slim dart with a round-capped tail and an indigo outline |
| 7 | clock in the centre | yes |
| 8 | launcher with a category rail | yes |
| 9 | Quick Settings and the calendar are popovers of the panel | yes: a tile grid and a two-column calendar with notifications |
| 10 | shared ergonomics acknowledged | rounded windows, soft shadows and springs remain, as every modern desktop has them |

What still echoes the references on purpose: the taskbar's centred floating form (Windows 11, elementary),
the centred clock with a notification popover (GNOME), the category rail (KDE) and edge snapping
(Windows 11 / KDE). What is new is the combination and the signature details: the accent line on the
focused title bar, the flat faceted tiles, the dart pointer, the workspace dots in the panel and the
Shell tab of the component gallery that documents them.

Not done: persistence of the pinned order and of the workspace of each window across reboots; middle
click on a taskbar icon (the pointer driver reports only the left and right buttons: Shift+click opens a
new window instead); window thumbnails in the taskbar menu (titles only); `docs/img/demo.gif` still shows
the first shell.
