//! The top panel: the Apps button and Busca at the left, the date and time in the centre
//! (opening the calendar and notification centre) and the status pill at the right (opening
//! Quick Settings); plus the menus (the system menu, a window's menu button, context menus) and
//! the popovers that hang from the panel. See `docs/design/ui-identity.md`.

use super::glass::panel as glass_panel;
use super::shell::*;
use super::*;
use crate::text::{self, BODY, FOOTNOTE, TITLE2, TITLE3, Weight};
use osjeff_core::chrome::PANEL_PAD;
use osjeff_core::chrome::{
    self, MenuRow, QuickTile, centre_geom, menu_geom, panel_layout, popover_centered, popover_rect,
    quick_geom,
};
use osjeff_core::iconart::Glyph;
use osjeff_core::snap::SnapZone;
use osjeff_core::style::{PANEL_H, R_CONTROL, R_MENU, R_POPOVER};

const WEEKDAYS: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
const WEEKDAYS_LONG: [&str; 7] = [
    "Domingo",
    "Segunda-feira",
    "Terça-feira",
    "Quarta-feira",
    "Quinta-feira",
    "Sexta-feira",
    "Sábado",
];
const MONTHS: [&str; 12] = [
    "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
];
const MONTH_NAMES: [&str; 12] = [
    "Janeiro",
    "Fevereiro",
    "Março",
    "Abril",
    "Maio",
    "Junho",
    "Julho",
    "Agosto",
    "Setembro",
    "Outubro",
    "Novembro",
    "Dezembro",
];

/// Widest the clock text can be: reserves the item's width so it never reflows.
const CLOCK_TEMPLATE: &str = "qua 00 out  00:00";
const CLOCK_TEMPLATE_12: &str = "qua 00 out  00:00 PM";
/// Room kept left of the clock text for the unread-notifications dot.
const CLOCK_DOT_W: i32 = 14;
/// Most notifications kept for the centre.
const NOTIF_MAX: usize = 24;

/// One entry of the notification centre's history.
pub(crate) struct Notif {
    pub level: crate::klog::Level,
    pub text: String,
    pub ms: u32,
}

fn level_title(l: crate::klog::Level) -> &'static str {
    use crate::klog::Level;
    match l {
        Level::Trace | Level::Debug | Level::Info => "Informação",
        Level::Warn => "Aviso",
        Level::Error => "Erro",
        Level::Fatal => "Falha grave",
    }
}

fn level_color(l: crate::klog::Level) -> Color {
    use crate::klog::Level;
    match l {
        Level::Warn => Color::rgb(0xF5, 0xA6, 0x23),
        Level::Error => theme::CLOSE,
        Level::Fatal => Color::rgb(0xFF, 0x4D, 0x9D),
        _ => theme::accent(),
    }
}

/// "agora", "3 min", "2 h": how long ago a notification arrived.
fn age_text(now_ms: u32, ms: u32) -> String {
    let secs = now_ms.wrapping_sub(ms) / 1000;
    match secs {
        0..=44 => String::from("agora"),
        45..=3599 => alloc::format!("{} min", (secs + 30) / 60),
        _ => alloc::format!("{} h", secs / 3600),
    }
}

impl Desktop {
    /// The panel's rectangle (its glass is baked into the cached wallpaper).
    pub fn panel_rect(&self) -> Rect {
        Rect::new(0, 0, self.sw, PANEL_H)
    }

    /// The text of the clock item (`qui 8 out  18:09`).
    fn clock_text(&self, time: Time) -> String {
        let (_, month, day) = self.today.get();
        let wd = self.weekday.get() as usize % 7;
        let mut buf = [0u8; osjeff_core::hw::rtc::CLOCK_LEN];
        let n = osjeff_core::hw::rtc::format_clock(
            osjeff_core::hw::rtc::Time {
                h: time.h,
                m: time.m,
                s: time.s,
            },
            crate::settings::clock24(),
            &mut buf,
        );
        // `format_clock` gives `HH:MM:SS` (or with AM/PM): the bar shows no seconds.
        let full = core::str::from_utf8(&buf[..n]).unwrap_or("");
        let hm: String = {
            let mut parts = full.splitn(3, ':');
            let h = parts.next().unwrap_or("");
            let m = parts.next().unwrap_or("");
            let rest = parts.next().unwrap_or("");
            let suffix = rest.split_once(' ').map(|(_, s)| s).unwrap_or("");
            if suffix.is_empty() {
                alloc::format!("{h}:{m}")
            } else {
                alloc::format!("{h}:{m} {suffix}")
            }
        };
        alloc::format!(
            "{} {} {}  {}",
            WEEKDAYS[wd],
            day,
            MONTHS[(month as usize).clamp(1, 12) - 1],
            hm
        )
    }

    fn clock_width(&self) -> i32 {
        let t = if crate::settings::clock24() {
            CLOCK_TEMPLATE
        } else {
            CLOCK_TEMPLATE_12
        };
        text::measure(t, BODY, Weight::Medium) + CLOCK_DOT_W
    }

    /// Every panel item with its rectangle.
    pub(crate) fn panel_items(&self) -> Vec<(PanelItem, Rect)> {
        let apps_w = 16 + 8 + text::measure("Apps", BODY, Weight::Medium);
        let ws_w = chrome::workspace_width(self.wm.visible_workspaces());
        let left = [apps_w, 16, ws_w];
        let right = [chrome::pill_width(3)];
        let g = panel_layout(self.sw, &left, self.clock_width(), &right);
        alloc::vec![
            (PanelItem::Apps, g.left[0]),
            (PanelItem::Search, g.left[1]),
            (PanelItem::Workspaces, g.left[2]),
            (PanelItem::Clock, g.center),
            (PanelItem::Tray, g.right[0]),
        ]
    }

    /// The item under `(x, y)`.
    pub(crate) fn panel_item_at(&self, x: i32, y: i32) -> Option<(PanelItem, Rect)> {
        if y >= PANEL_H {
            return None;
        }
        self.panel_items()
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
    }

    /// Screen rectangle of the clock item (what the per-second tick repaints).
    pub fn clock_rect(&self) -> Rect {
        self.panel_items()
            .iter()
            .find(|(i, _)| *i == PanelItem::Clock)
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// True when the per-second clock tick can be repainted locally: no window
    /// that redraws itself every second is on screen.
    pub fn clock_repaint_is_local(&self) -> bool {
        self.task_window_rect().is_none()
    }

    /// Redo only the clock item in `back`: restore the wallpaper's panel under it, then
    /// draw the text. Valid when [`clock_repaint_is_local`] holds.
    pub fn repaint_clock(
        &self,
        back: &mut [u8],
        bg: &[u8],
        info: bootloader_api::info::FrameBufferInfo,
        time: Time,
    ) {
        let r = self.clock_rect();
        copy_region(back, bg, info, r);
        let mut c = Canvas::new(back, info);
        self.draw_panel_item(&mut c, PanelItem::Clock, r, time);
    }

    /// Panel content over the strip baked into the wallpaper.
    pub(crate) fn draw_panel(&self, c: &mut Canvas, time: Time) {
        for (item, rect) in self.panel_items() {
            self.draw_panel_item(c, item, rect, time);
        }
    }

    fn panel_item_active(&self, item: PanelItem) -> bool {
        let sh = &self.shell;
        sh.pop.as_ref().is_some_and(|p| {
            !p.closing
                && matches!(
                    (p.kind, item),
                    (PopKind::Quick, PanelItem::Tray) | (PopKind::Centre, PanelItem::Clock)
                )
        }) || (item == PanelItem::Search && sh.search.as_ref().is_some_and(|s| !s.closing))
            || (item == PanelItem::Apps && sh.apps.as_ref().is_some_and(|a| !a.closing))
    }

    fn draw_panel_item(&self, c: &mut Canvas, item: PanelItem, rect: Rect, time: Time) {
        let p = theme::pal();
        let fg = theme::solid(p.bar_text);
        let active = self.panel_item_active(item);
        let hover = self.shell.panel_hover == Some(item);
        let pill = Rect::new(rect.x + 1, rect.y + 3, rect.w - 2, rect.h - 6);
        let (hc, ha) = theme::tint(p.hover);
        if item == PanelItem::Tray {
            // The status pill is always a pill; it darkens with hover and while its popover is open.
            let a = if active {
                ha * 3
            } else if hover {
                ha * 2
            } else {
                ha
            };
            c.fill_rrect(pill, pill.h / 2, Corner::Circle, hc, a.min(256));
        } else if active || hover {
            c.fill_rrect(
                pill,
                R_CONTROL,
                Corner::Circle,
                hc,
                if active { (ha * 2).min(256) } else { ha },
            );
        }
        let argb = 0xFF00_0000 | pack(fg);
        let ty = text::center_y(rect.y, rect.h, BODY, Weight::Medium);
        match item {
            PanelItem::Apps => {
                ui::draw_glyph(c, Glyph::Brand, rect.x + PANEL_PAD, rect.y + 7, 16, argb);
                text::draw(
                    c,
                    rect.x + PANEL_PAD + 16 + 8,
                    ty,
                    "Apps",
                    BODY,
                    Weight::Medium,
                    fg,
                );
            }
            PanelItem::Search => {
                ui::draw_glyph(c, Glyph::Search, rect.x + PANEL_PAD, rect.y + 7, 16, argb)
            }
            PanelItem::Workspaces => {
                let (n, cur) = (self.wm.visible_workspaces(), self.wm.workspace());
                for i in 0..n {
                    let d = chrome::workspace_dot(rect, i, cur);
                    let (dc, da) = theme::tint(p.bar_text);
                    if i == cur {
                        c.fill_rrect(d, d.h / 2, Corner::Circle, theme::accent(), 256);
                    } else {
                        // A workspace with windows is a stronger dot than an empty one.
                        let a = if self.wm.windows_on(i) > 0 {
                            da.min(170)
                        } else {
                            da.min(80)
                        };
                        c.fill_rrect(d, d.h / 2, Corner::Circle, dc, a);
                    }
                }
            }
            PanelItem::Clock => {
                let t = self.clock_text(time);
                let w = text::measure(&t, BODY, Weight::Medium);
                let dot = self.shell.notif_unread > 0;
                let room = rect.w - 2 * PANEL_PAD - CLOCK_DOT_W;
                let x = rect.x + PANEL_PAD + CLOCK_DOT_W + (room - w) / 2;
                if dot {
                    let d = Rect::new(x - 12, rect.y + (rect.h - 6) / 2, 6, 6);
                    c.fill_rrect(d, 3, Corner::Circle, theme::accent(), 256);
                }
                text::draw(c, x, ty, &t, BODY, Weight::Medium, fg);
            }
            PanelItem::Tray => {
                let up = crate::netd::stats().link_up;
                let net = if up {
                    Glyph::Network
                } else {
                    Glyph::NetworkOff
                };
                let look = if theme::dark() {
                    Glyph::Moon
                } else {
                    Glyph::Sun
                };
                for (i, g) in [net, look, Glyph::Power].into_iter().enumerate() {
                    let r = chrome::pill_icon(rect, i);
                    ui::draw_glyph(c, g, r.x, r.y, r.w, argb);
                }
            }
        }
    }

    // ---- notification history ----

    /// Collect new warnings and errors of the system log into the notification centre's list.
    pub(crate) fn refresh_notifs(&mut self) {
        let mut warns = [None; 4];
        let n = crate::klog::take_warnings(&mut self.shell.notif_seen, &mut warns);
        let now = crate::klog::ticks_to_ms_now();
        for w in warns.iter().take(n).flatten() {
            let open = self
                .shell
                .pop
                .as_ref()
                .is_some_and(|p| p.kind == PopKind::Centre && !p.closing);
            let text = String::from(&*crate::text::from_bytes(w.text()));
            if self.shell.notifs.len() >= NOTIF_MAX {
                self.shell.notifs.remove(0);
            }
            self.shell.notifs.push(Notif {
                level: w.level,
                text,
                ms: now,
            });
            if !open {
                self.shell.notif_unread += 1;
            }
            self.force_full |= open;
        }
    }

    // ---- menus ----

    pub(crate) fn system_menu(&self) -> Vec<Entry> {
        alloc::vec![
            Entry::item("Sobre o OSjeff", "", Cmd::About),
            Entry::sep(),
            Entry::item("Ajustes do sistema…", "", Cmd::Settings),
            Entry::item("Componentes", "Ctrl+Alt+G", Cmd::Gallery),
            Entry::sep(),
            Entry::item("Reiniciar…", "", Cmd::Reboot),
            Entry::item("Desligar…", "", Cmd::Shutdown),
        ]
    }

    /// The focused app's menu called `name` ("Arquivo", "Editar", "Visualizar", "Janela").
    fn named_menu(&self, name: &str) -> Vec<Entry> {
        let kind = self.focused().and_then(|id| self.kind_of(id));
        let wasm = kind == Some(Kind::WasmApp);
        match name {
            "Arquivo" => {
                let mut v = alloc::vec![Entry::item(
                    "Nova janela",
                    if wasm { "" } else { "Ctrl+N" },
                    Cmd::NewWindow
                )];
                match kind {
                    Some(Kind::Editor) => {
                        v.push(Entry::item("Abrir…", "Ctrl+O", Cmd::OpenFile));
                        v.push(Entry::item("Salvar", "Ctrl+S", Cmd::SaveFile));
                    }
                    None => {}
                    _ => {}
                }
                v.push(Entry::sep());
                v.push(
                    Entry::item("Fechar janela", "Ctrl+W", Cmd::CloseWindow)
                        .disabled_if(kind.is_none()),
                );
                v
            }
            "Editar" => {
                let editor = kind == Some(Kind::Editor);
                alloc::vec![
                    Entry::item("Desfazer", "Ctrl+Z", Cmd::Undo).disabled_if(!editor),
                    Entry::item("Refazer", "Ctrl+Y", Cmd::Redo).disabled_if(!editor),
                    Entry::sep(),
                    Entry::item("Recortar", "Ctrl+X", Cmd::Cut).disabled_if(!editor),
                    Entry::item(
                        "Copiar",
                        if kind == Some(Kind::Terminal) {
                            "Ctrl+Shift+C"
                        } else {
                            "Ctrl+C"
                        },
                        Cmd::Copy
                    )
                    .disabled_if(kind.is_none() || wasm),
                    Entry::item("Colar", "Ctrl+V", Cmd::Paste).disabled_if(kind.is_none() || wasm),
                    Entry::sep(),
                    Entry::item("Selecionar tudo", "Ctrl+A", Cmd::SelectAll).disabled_if(!editor),
                ]
            }
            "Visualizar" => {
                let browser = kind == Some(Kind::Browser);
                let mut v = alloc::vec![];
                if browser {
                    v.push(Entry::item("Ampliar", "Ctrl++", Cmd::BrowserZoomIn));
                    v.push(Entry::item("Reduzir", "Ctrl+-", Cmd::BrowserZoomOut));
                    v.push(Entry::item("Tamanho real", "Ctrl+0", Cmd::BrowserZoomReset));
                    v.push(Entry::sep());
                }
                v.push(Entry::item("Zoom da janela", "", Cmd::Zoom));
                v.push(Entry::sep());
                v.push(Entry::item("Mostrar apps", "", Cmd::ShowApps));
                v.push(Entry::item("Buscar", "Ctrl+Espaço", Cmd::ShowSearch));
                v
            }
            _ => {
                let mut v = alloc::vec![
                    Entry::item("Minimizar", "Ctrl+M", Cmd::Minimize).disabled_if(kind.is_none()),
                    Entry::item("Zoom", "", Cmd::Zoom).disabled_if(kind.is_none()),
                ];
                let list = self.wm.switch_list();
                if !list.is_empty() {
                    v.push(Entry::sep());
                    let focused = self.focused();
                    for id in list.into_iter().take(10) {
                        if let Some(w) = self.wm.get(id) {
                            let mut e = Entry::item(&w.app.title, "", Cmd::Activate(id));
                            e.checked = focused == Some(id);
                            v.push(e);
                        }
                    }
                }
                v
            }
        }
    }

    /// The menu behind a window's title-bar button: the app's File, Edit and View entries
    /// (disabled ones left out), then the window commands. `id` must be the focused window.
    fn window_menu(&self, id: WindowId) -> Vec<Entry> {
        let mut v: Vec<Entry> = Vec::new();
        let section = |v: &mut Vec<Entry>, items: Vec<Entry>, keep_disabled: bool| {
            let items: Vec<Entry> = items
                .into_iter()
                .filter(|e| e.cmd == Cmd::Sep || e.enabled || keep_disabled)
                .collect();
            // Drop separators that would lead, trail or double up.
            let mut out: Vec<Entry> = Vec::new();
            for e in items {
                if e.cmd == Cmd::Sep && out.last().is_none_or(|l| l.cmd == Cmd::Sep) {
                    continue;
                }
                out.push(e);
            }
            while out.last().is_some_and(|l| l.cmd == Cmd::Sep) {
                out.pop();
            }
            if out.is_empty() {
                return;
            }
            if !v.is_empty() {
                v.push(Entry::sep());
            }
            v.extend(out);
        };
        section(&mut v, self.named_menu("Arquivo"), true);
        section(&mut v, self.named_menu("Editar"), false);
        let view: Vec<Entry> = self
            .named_menu("Visualizar")
            .into_iter()
            .filter(|e| !matches!(e.cmd, Cmd::Zoom | Cmd::ShowApps | Cmd::ShowSearch))
            .collect();
        section(&mut v, view, false);
        let state = self
            .wm
            .get(id)
            .map(|w| (w.snap_state().is_some(), w.resizable));
        let (tiled, resizable) = state.unwrap_or((false, false));
        let mut win = alloc::vec![Entry::item("Minimizar", "Ctrl+M", Cmd::Minimize)];
        let mut zoom = Entry::item(
            if tiled { "Restaurar" } else { "Maximizar" },
            "Alt+↑",
            Cmd::Zoom,
        );
        zoom.enabled = resizable;
        win.push(zoom);
        let mut left = Entry::item("Ajustar à esquerda", "Alt+←", Cmd::Snap(SnapZone::Left));
        left.enabled = resizable;
        let mut right = Entry::item("Ajustar à direita", "Alt+→", Cmd::Snap(SnapZone::Right));
        right.enabled = resizable;
        win.push(left);
        win.push(right);
        section(&mut v, win, true);
        // Move the window to another workspace.
        let cur = self.wm.workspace();
        let moves: Vec<Entry> = (0..self.wm.visible_workspaces())
            .filter(|&i| i != cur)
            .map(|i| {
                Entry::item(
                    &alloc::format!("Mover para a área de trabalho {}", i + 1),
                    "",
                    Cmd::MoveToWorkspace(i),
                )
            })
            .collect();
        section(&mut v, moves, true);
        v
    }

    /// Open the menu button's menu of window `id`, under the button, right edge aligned.
    pub(crate) fn open_window_menu(&mut self, id: WindowId) {
        if self
            .shell
            .menu
            .as_ref()
            .is_some_and(|m| !m.closing && m.origin == MenuOrigin::Window(id))
        {
            self.close_transients();
            return;
        }
        let Some((rect, resizable)) = self.wm.get(id).map(|w| (w.rect, w.resizable)) else {
            return;
        };
        let Some(btn) = rect.title_layout(resizable, true).menu else {
            return;
        };
        let entries = self.window_menu(id);
        self.open_menu_at(
            MenuOrigin::Window(id),
            entries,
            (btn.right(), btn.bottom() + 2),
            true,
        );
    }

    /// Open a menu with its top-left near `at`.
    pub(crate) fn open_menu(&mut self, origin: MenuOrigin, entries: Vec<Entry>, at: (i32, i32)) {
        self.open_menu_at(origin, entries, at, false);
    }

    /// Open a menu near `at`: with its top-left there, or (`right_edge`) its top-right.
    pub(crate) fn open_menu_at(
        &mut self,
        origin: MenuOrigin,
        entries: Vec<Entry>,
        at: (i32, i32),
        right_edge: bool,
    ) {
        let rows: Vec<MenuRow> = entries
            .iter()
            .map(|e| {
                if e.cmd == Cmd::Sep {
                    MenuRow::Separator
                } else {
                    MenuRow::Item {
                        label_w: text::measure(&e.label, BODY, Weight::Regular),
                        shortcut_w: text::measure(e.shortcut, BODY, Weight::Regular),
                    }
                }
            })
            .collect();
        let mut geom = menu_geom(&rows, at, self.sw, self.sh);
        if right_edge {
            geom = menu_geom(&rows, (at.0 - geom.rect.w, at.1), self.sw, self.sh);
        }
        // Opening a menu closes whatever else was transient.
        if let Some(p) = self.shell.pop.as_mut() {
            p.closing = true;
            p.t = fade_in(0.01);
        }
        self.shell.pop = None;
        self.shell.menu = Some(OpenMenu {
            origin,
            entries,
            rows,
            geom,
            hover: None,
            t: fade_in(MENU_FADE),
            closing: false,
            glass: Default::default(),
        });
        self.force_full = true;
    }

    /// Run row `i` of the open menu and close it.
    pub(crate) fn menu_pick(&mut self, i: usize) {
        let cmd = match self.shell.menu.as_ref().and_then(|m| m.entries.get(i)) {
            Some(e) if e.enabled => e.cmd,
            _ => return,
        };
        self.close_transients();
        self.execute(cmd);
    }

    fn draw_open_menu(&self, c: &mut Canvas, m: &OpenMenu) {
        let p = theme::pal();
        let fade = level(&m.t);
        let slide = ((256 - fade) as i32 * 6) / 256;
        let mut g = m.geom.rect;
        g.y -= slide;
        glass_panel(
            c,
            g,
            R_MENU,
            &m.glass,
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 12,
                dy: 8,
                alpha: 80,
            },
            fade,
        );
        let a = fade as u16;
        for (i, (rect, e)) in m.geom.rows.iter().zip(&m.entries).enumerate() {
            let mut r = *rect;
            r.y -= slide;
            if e.cmd == Cmd::Sep {
                let line = Rect::new(r.x + 8, r.y + r.h / 2, r.w - 16, 1);
                let (sc, sa) = theme::tint(p.separator);
                c.blend_rect(line, sc, (sa as u32 * fade / 256) as u16);
                continue;
            }
            // Items fade with the panel through the snapshot-free route: draw at full
            // strength once the panel is mostly in.
            if a > 100 {
                ui::menu_item(
                    c,
                    r,
                    &e.label,
                    e.shortcut,
                    m.hover == Some(i),
                    e.enabled,
                    e.checked,
                );
            }
        }
    }

    // ---- popovers ----

    /// Open the popover of `kind` under its panel item.
    pub(crate) fn open_popover(&mut self, kind: PopKind) {
        let item = match kind {
            PopKind::Quick => PanelItem::Tray,
            PopKind::Centre => PanelItem::Clock,
        };
        let Some((_, anchor)) = self.panel_items().into_iter().find(|(i, _)| *i == item) else {
            return;
        };
        let rect = match kind {
            PopKind::Quick => popover_rect(anchor, chrome::QUICK_W, chrome::QUICK_H, self.sw),
            PopKind::Centre => {
                popover_centered(anchor, chrome::CENTRE_W, chrome::CENTRE_H, self.sw)
            }
        };
        self.shell.menu = None;
        self.shell.pop = Some(Popover {
            kind,
            rect,
            t: fade_in(MENU_FADE),
            closing: false,
            glass: Default::default(),
            month_off: 0,
        });
        self.shell.knobs = [
            tween_at(crate::settings::get().reduce_motion),
            tween_at(crate::settings::get().clock24),
            tween_at(!crate::settings::get().toasts),
        ];
        if kind == PopKind::Centre {
            self.shell.notif_unread = 0;
        }
        self.force_full = true;
    }

    fn draw_popover(&self, c: &mut Canvas, pop: &Popover) {
        let p = theme::pal();
        let fade = level(&pop.t);
        let mut r = pop.rect;
        r.y -= ((256 - fade) as i32 * 6) / 256;
        glass_panel(
            c,
            r,
            R_POPOVER,
            &pop.glass,
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 14,
                dy: 8,
                alpha: 80,
            },
            fade,
        );
        if fade < 150 {
            return;
        }
        match pop.kind {
            PopKind::Quick => self.draw_quick(c, r),
            PopKind::Centre => self.draw_centre(c, r, pop.month_off),
        }
    }

    /// Quick Settings: a tile grid, the accent swatches and the power buttons.
    fn draw_quick(&self, c: &mut Canvas, r: Rect) {
        let p = theme::pal();
        let g = quick_geom(r);
        let s = crate::settings::get();
        text::draw_left(
            c,
            g.title,
            "Configurações rápidas",
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        let st = crate::netd::stats();
        let up = st.link_up;
        for (rect, tile) in g.tiles.iter().zip(chrome::QUICK_ORDER) {
            let (glyph, label, sub, on): (Glyph, &str, String, bool) = match tile {
                QuickTile::Network => (
                    if up {
                        Glyph::Network
                    } else {
                        Glyph::NetworkOff
                    },
                    "Rede",
                    match (&st.config, up) {
                        (Some(cfg), true) => alloc::format!("{}", cfg.ip),
                        (None, true) => String::from("Conectando..."),
                        _ => String::from("Sem rede"),
                    },
                    up,
                ),
                QuickTile::Appearance => (
                    if theme::dark() {
                        Glyph::Moon
                    } else {
                        Glyph::Sun
                    },
                    "Aparência",
                    String::from(match s.appearance {
                        osjeff_core::style::AppearanceSetting::Auto => "Automática",
                        osjeff_core::style::AppearanceSetting::Light => "Clara",
                        osjeff_core::style::AppearanceSetting::Dark => "Escura",
                    }),
                    theme::dark(),
                ),
                QuickTile::ReduceMotion => (
                    Glyph::Wave,
                    "Movimento",
                    String::from(if s.reduce_motion {
                        "Reduzido"
                    } else {
                        "Completo"
                    }),
                    s.reduce_motion,
                ),
                QuickTile::DoNotDisturb => (
                    Glyph::Bell,
                    "Não perturbe",
                    String::from(if s.toasts { "Desligado" } else { "Ligado" }),
                    !s.toasts,
                ),
                QuickTile::Clock24 => (
                    Glyph::Clock,
                    "Relógio 24 h",
                    String::from(if s.clock24 { "24 horas" } else { "12 horas" }),
                    s.clock24,
                ),
                QuickTile::Settings => (
                    Glyph::Control,
                    "Configurações",
                    String::from("Abrir"),
                    false,
                ),
            };
            let hover = rect.contains(self.cursor_x, self.cursor_y);
            self.draw_tile(c, *rect, glyph, label, &sub, on, hover);
        }
        ui::caption(c, g.accent_label.x, g.accent_label.y, "Cor de destaque");
        for (i, sw) in g.swatches.iter().enumerate() {
            let rgb = osjeff_core::settings::ACCENTS[i];
            let col = Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
            if s.accent as usize == i {
                c.stroke_rrect(sw.inflated(3), 9, Corner::Circle, theme::solid(p.text), 200);
            }
            c.fill_rrect(*sw, 7, Corner::Circle, col, 256);
        }
        let hov = |r: Rect| {
            if r.contains(self.cursor_x, self.cursor_y) {
                ui::Control::Hover
            } else {
                ui::Control::Normal
            }
        };
        ui::push_button(
            c,
            g.restart,
            "Reiniciar",
            ui::ButtonKind::Secondary,
            hov(g.restart),
        );
        ui::push_button(
            c,
            g.shutdown,
            "Desligar",
            ui::ButtonKind::Secondary,
            hov(g.shutdown),
        );
    }

    /// One Quick Settings tile: a disc with a glyph, a label and a status line; accent when on.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_tile(
        &self,
        c: &mut Canvas,
        r: Rect,
        glyph: Glyph,
        label: &str,
        sub: &str,
        on: bool,
        hover: bool,
    ) {
        let p = theme::pal();
        let rad = 10;
        if on {
            c.fill_rrect(r, rad, Corner::Circle, theme::accent(), 256);
            if hover {
                c.fill_rrect(r, rad, Corner::Circle, theme::WHITE, 28);
            }
        } else {
            ui::fill_token(c, r, rad, p.control_bg);
            if hover {
                ui::fill_token(c, r, rad, p.hover);
            }
            ui::stroke_token(c, r, rad, p.control_border);
        }
        let disc = Rect::new(r.x + 10, r.y + (r.h - 32) / 2, 32, 32);
        let (dc, da) = if on {
            (theme::WHITE, 56)
        } else {
            theme::tint(p.hover)
        };
        c.fill_rrect(disc, 8, Corner::Circle, dc, da);
        let ink = if on {
            theme::WHITE
        } else {
            theme::solid(p.text)
        };
        ui::draw_glyph(
            c,
            glyph,
            disc.x + 7,
            disc.y + 7,
            18,
            0xFF00_0000 | pack(ink),
        );
        let tx = disc.right() + 10;
        let tw = r.right() - tx - 8;
        let (c1, c2) = if on {
            (theme::WHITE, theme::WHITE)
        } else {
            (theme::solid(p.text), theme::solid(p.text_secondary))
        };
        draw_fit(
            c,
            Rect::new(tx, r.y + 11, tw, 18),
            label,
            BODY,
            Weight::Medium,
            c1,
        );
        draw_fit(
            c,
            Rect::new(tx, r.y + 29, tw, 16),
            sub,
            FOOTNOTE,
            Weight::Regular,
            if on { Color::rgb(0xE8, 0xE8, 0xFF) } else { c2 },
        );
    }

    /// The calendar and notification centre: the date and the notification list at the left, the
    /// month at the right.
    fn draw_centre(&self, c: &mut Canvas, r: Rect, month_off: i32) {
        let p = theme::pal();
        let g = centre_geom(r);
        let (_, month, day) = self.today.get();
        let wd = self.weekday.get() as usize % 7;
        text::draw_left(
            c,
            g.day,
            WEEKDAYS_LONG[wd],
            TITLE2,
            Weight::Semibold,
            theme::solid(p.text),
        );
        let (year, _, _) = self.today.get();
        let date = alloc::format!(
            "{} de {} de {}",
            day,
            MONTH_NAMES[(month as usize).clamp(1, 12) - 1].to_lowercase(),
            year
        );
        text::draw_left(
            c,
            g.date,
            &date,
            BODY,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        text::draw_left(
            c,
            g.notif_title,
            "Notificações",
            BODY,
            Weight::Semibold,
            theme::solid(p.text),
        );
        let notifs = &self.shell.notifs;
        if !notifs.is_empty() {
            let hov = g.clear.contains(self.cursor_x, self.cursor_y);
            if hov {
                ui::fill_token(c, g.clear, 6, p.hover);
            }
            text::draw_centered(
                c,
                g.clear,
                "Limpar",
                FOOTNOTE,
                Weight::Medium,
                theme::accent(),
            );
        }
        if notifs.is_empty() {
            text::draw_centered(
                c,
                g.empty,
                "Sem notificações",
                BODY,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        } else {
            let now = crate::klog::ticks_to_ms_now();
            // Newest first.
            for (row, n) in g.rows.iter().zip(notifs.iter().rev()) {
                ui::fill_token(c, *row, 8, p.control_bg);
                ui::stroke_token(c, *row, 8, p.control_border);
                let disc = Rect::new(row.x + 10, row.y + (row.h - 24) / 2, 24, 24);
                c.fill_rrect(disc, 12, Corner::Circle, level_color(n.level), 256);
                text::draw_centered(c, disc, "!", BODY, Weight::Semibold, theme::WHITE);
                let tx = disc.right() + 10;
                let age = age_text(now, n.ms);
                let aw = text::measure(&age, FOOTNOTE, Weight::Regular) + 8;
                draw_fit(
                    c,
                    Rect::new(tx, row.y + 5, row.right() - tx - aw - 8, 18),
                    level_title(n.level),
                    BODY,
                    Weight::Medium,
                    theme::solid(p.text),
                );
                text::draw_right(
                    c,
                    Rect::new(row.x, row.y + 5, row.w - 10, 18),
                    &age,
                    FOOTNOTE,
                    Weight::Regular,
                    theme::solid(p.text_tertiary),
                );
                draw_fit(
                    c,
                    Rect::new(tx, row.y + 23, row.right() - tx - 10, 16),
                    &n.text,
                    FOOTNOTE,
                    Weight::Regular,
                    theme::solid(p.text_secondary),
                );
            }
        }
        text::draw_left(
            c,
            g.dnd_label,
            "Não perturbe",
            BODY,
            Weight::Regular,
            theme::solid(p.text),
        );
        ui::switch(
            c,
            g.dnd_switch,
            (self.shell.knobs[2].value() * 256.0) as i32,
            true,
        );
        // A hairline between the two columns.
        let (sc, sa) = theme::tint(p.separator);
        c.blend_rect(Rect::new(g.calendar.x - 12, r.y + 14, 1, r.h - 28), sc, sa);
        self.draw_calendar(c, g.calendar, month_off);
    }

    fn draw_calendar(&self, c: &mut Canvas, r: Rect, month_off: i32) {
        let p = theme::pal();
        let g = chrome::calendar_geom(r);
        let (ty, tm, td) = self.today.get();
        let idx = (ty * 12 + tm as i32 - 1) + month_off;
        let (year, month) = (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u8);
        let title = alloc::format!("{} {}", MONTH_NAMES[month as usize - 1], year);
        text::draw_left(
            c,
            g.title,
            &title,
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        ui::draw_glyph(
            c,
            Glyph::ChevronRight,
            g.next.x + 4,
            g.next.y + 6,
            16,
            0xFF00_0000 | pack(theme::solid(p.text_secondary)),
        );
        // The "previous" chevron is the next one mirrored.
        let chev = crate::glyphs::get(
            Glyph::ChevronRight,
            16,
            0xFF00_0000 | pack(theme::solid(p.text_secondary)),
        );
        let mirrored = mirror_x(chev);
        c.blit_surface(&mirrored, g.prev.x + 4, g.prev.y + 6, 256);
        for (i, wd) in ["D", "S", "T", "Q", "Q", "S", "S"].iter().enumerate() {
            text::draw_centered(
                c,
                g.weekdays[i],
                wd,
                FOOTNOTE,
                Weight::Medium,
                theme::solid(p.text_tertiary),
            );
        }
        let grid = chrome::month_grid(year, month);
        for (row, line) in grid.iter().enumerate() {
            for (col, &d) in line.iter().enumerate() {
                if d == 0 {
                    continue;
                }
                let cell = g.cells[row][col];
                let today = month_off == 0 && d == td;
                let label = alloc::format!("{d}");
                if today {
                    let disc = Rect::new(cell.x + (cell.w - 28) / 2, cell.y + 3, 28, 28);
                    c.fill_rrect(disc, 14, Corner::Circle, theme::accent(), 256);
                    text::draw_centered(
                        c,
                        disc,
                        &label,
                        BODY,
                        Weight::Semibold,
                        theme::ACCENT_TEXT,
                    );
                } else {
                    let weekend = col == 0 || col == 6;
                    let col_c = if weekend { p.text_secondary } else { p.text };
                    text::draw_centered(
                        c,
                        cell,
                        &label,
                        BODY,
                        Weight::Regular,
                        theme::solid(col_c),
                    );
                }
            }
        }
    }

    /// A click inside the open popover. Returns true when it was handled.
    pub(crate) fn popover_click(&mut self, x: i32, y: i32) -> bool {
        let Some(pop) = self.shell.pop.as_ref() else {
            return false;
        };
        if !pop.rect.contains(x, y) {
            return false;
        }
        match pop.kind {
            PopKind::Quick => {
                let g = quick_geom(pop.rect);
                let mut s = crate::settings::get();
                if let Some(tile) = chrome::quick_tile_at(&g, x, y) {
                    match tile {
                        QuickTile::Network | QuickTile::Settings => {
                            self.close_transients();
                            self.open_settings(0);
                            return true;
                        }
                        QuickTile::Appearance => s.appearance = s.appearance.next(),
                        QuickTile::ReduceMotion => s.reduce_motion = !s.reduce_motion,
                        QuickTile::DoNotDisturb => s.toasts = !s.toasts,
                        QuickTile::Clock24 => s.clock24 = !s.clock24,
                    }
                } else if let Some(i) = g.swatches.iter().position(|r| r.inflated(3).contains(x, y))
                {
                    s.accent = i as u8;
                } else if g.restart.contains(x, y) {
                    self.close_transients();
                    self.execute(Cmd::Reboot);
                    return true;
                } else if g.shutdown.contains(x, y) {
                    self.close_transients();
                    self.execute(Cmd::Shutdown);
                    return true;
                } else {
                    return true;
                }
                let look = s.appearance != crate::settings::get().appearance;
                let _ = self.settings_apply(s);
                if look {
                    // The look changed: the popover's blurred backdrop is stale, so it comes up again.
                    self.open_popover(PopKind::Quick);
                }
                true
            }
            PopKind::Centre => {
                let g = centre_geom(pop.rect);
                let cal = chrome::calendar_geom(g.calendar);
                if g.clear.contains(x, y) {
                    self.shell.notifs.clear();
                    self.shell.notif_unread = 0;
                } else if g.dnd_label.contains(x, y) || g.dnd_switch.contains(x, y) {
                    let mut s = crate::settings::get();
                    s.toasts = !s.toasts;
                    self.shell.knobs[2].retarget(
                        if s.toasts { 0.0 } else { 1.0 },
                        0.18,
                        osjeff_core::anim::curves::ENTER,
                    );
                    let _ = self.settings_apply(s);
                } else if cal.prev.contains(x, y) {
                    if let Some(p) = self.shell.pop.as_mut() {
                        p.month_off -= 1;
                    }
                } else if cal.next.contains(x, y)
                    && let Some(p) = self.shell.pop.as_mut()
                {
                    p.month_off += 1;
                }
                true
            }
        }
    }

    /// Draw the menu, popover and sheet layers.
    pub(crate) fn draw_menu_layers(&self, c: &mut Canvas) {
        if let Some(m) = &self.shell.menu {
            self.draw_open_menu(c, m);
        }
        if let Some(p) = &self.shell.pop {
            self.draw_popover(c, p);
        }
    }
}

impl Entry {
    fn disabled_if(mut self, off: bool) -> Entry {
        if off {
            self.enabled = false;
        }
        self
    }
}

fn tween_at(on: bool) -> osjeff_core::anim::Tween {
    osjeff_core::anim::Tween::at(if on { 1.0 } else { 0.0 })
}

/// Draw `label` left-aligned in `r`, cut with an ellipsis if it does not fit.
fn draw_fit(c: &mut Canvas, r: Rect, label: &str, px: u16, w: Weight, col: Color) {
    let t = text::ellipsize(label, px, w, r.w);
    text::draw_left(c, r, &t, px, w, col);
}

fn pack(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// A horizontally mirrored copy of a surface (for the "previous" chevron).
fn mirror_x(s: &osjeff_core::raster::Surface) -> osjeff_core::raster::Surface {
    let mut m = osjeff_core::raster::Surface::new(s.w, s.h);
    for y in 0..s.h {
        for x in 0..s.w {
            m.px[y * s.w + x] = s.px[y * s.w + (s.w - 1 - x)];
        }
    }
    m
}
