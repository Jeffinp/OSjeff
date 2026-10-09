//! The menu bar (system menu, app name, the focused app's menus, status items,
//! clock), the menus and the popovers (Controls, calendar) that hang from it.

use super::glass::panel;
use super::shell::*;
use super::*;
use crate::text::{self, BODY, FOOTNOTE, TITLE3, Weight};
use osjeff_core::chrome::{self, MenuRow, menu_geom, menubar_layout, popover_rect};
use osjeff_core::iconart::Glyph;
use osjeff_core::snap::SnapZone;
use osjeff_core::style::{R_MENU, R_POPOVER};

const WEEKDAYS: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
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

impl Desktop {
    pub fn menubar_rect(&self) -> Rect {
        Rect::new(0, 0, self.sw, MENUBAR_H)
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
        text::measure(t, BODY, Weight::Regular)
    }

    /// Names of the focused app's menus and its display name.
    fn bar_names(&self) -> (String, Vec<&'static str>) {
        match self.focused().and_then(|id| self.kind_of(id)) {
            Some(k) => (
                String::from(k.label()),
                alloc::vec!["Arquivo", "Editar", "Visualizar", "Janela"],
            ),
            None => (String::from("Mesa"), alloc::vec!["Arquivo", "Janela"]),
        }
    }

    /// Every menu-bar item with its rectangle.
    pub(crate) fn bar_items(&self) -> Vec<(BarItem, Rect)> {
        let (app, menus) = self.bar_names();
        let mut left_w = alloc::vec![18, text::measure(&app, BODY, Weight::Semibold)];
        for m in &menus {
            left_w.push(text::measure(m, BODY, Weight::Regular));
        }
        let right_w = [16, 16, 16, self.clock_width()];
        let (l, r) = menubar_layout(self.sw, &left_w, &right_w);
        let mut out = Vec::with_capacity(l.len() + r.len());
        for (i, rect) in l.into_iter().enumerate() {
            let item = match i {
                0 => BarItem::System,
                1 => BarItem::AppName,
                n => BarItem::Menu(n - 2),
            };
            out.push((item, rect));
        }
        for (rect, item) in r.into_iter().zip([
            BarItem::Network,
            BarItem::Control,
            BarItem::Search,
            BarItem::Clock,
        ]) {
            out.push((item, rect));
        }
        out
    }

    /// Screen rectangle of the clock item (what the per-second tick repaints).
    pub fn clock_rect(&self) -> Rect {
        self.bar_items()
            .iter()
            .find(|(i, _)| *i == BarItem::Clock)
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// True when the per-second clock tick can be repainted locally: no window
    /// that redraws itself every second is on screen.
    pub fn clock_repaint_is_local(&self) -> bool {
        self.task_window_rect().is_none()
    }

    /// Redo only the clock item in `back`: restore the wallpaper glass under it, then
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
        self.draw_bar_item(&mut c, BarItem::Clock, r, time);
    }

    /// Menu bar content over the glass strip baked into the wallpaper.
    pub(crate) fn draw_menubar(&self, c: &mut Canvas, time: Time) {
        for (item, rect) in self.bar_items() {
            self.draw_bar_item(c, item, rect, time);
        }
    }

    fn bar_item_active(&self, item: BarItem) -> bool {
        let sh = &self.shell;
        sh.menu
            .as_ref()
            .is_some_and(|m| !m.closing && m.origin == MenuOrigin::Bar(item))
            || sh.pop.as_ref().is_some_and(|p| {
                !p.closing
                    && matches!(
                        (p.kind, item),
                        (PopKind::Control, BarItem::Control) | (PopKind::Calendar, BarItem::Clock)
                    )
            })
            || (item == BarItem::Search && sh.search.as_ref().is_some_and(|s| !s.closing))
    }

    fn draw_bar_item(&self, c: &mut Canvas, item: BarItem, rect: Rect, time: Time) {
        let p = theme::pal();
        let fg = theme::solid(p.bar_text);
        let active = self.bar_item_active(item);
        let hover = self.shell.bar_hover == Some(item);
        if active || hover {
            let pill = Rect::new(rect.x + 2, rect.y + 3, rect.w - 4, rect.h - 6);
            let (hc, ha) = theme::tint(p.hover);
            c.fill_rrect(
                pill,
                6,
                Corner::Circle,
                hc,
                if active { (ha * 2).min(256) } else { ha },
            );
        }
        let ty = text::center_y(rect.y, rect.h, BODY, Weight::Regular);
        let argb = 0xFF00_0000 | pack(fg);
        match item {
            BarItem::System => ui::draw_glyph(
                c,
                Glyph::Brand,
                rect.x + (rect.w - 16) / 2,
                rect.y + 6,
                16,
                argb,
            ),
            BarItem::AppName => {
                let name = self.bar_names().0;
                text::draw(
                    c,
                    rect.x + chrome::BAR_PAD,
                    text::center_y(rect.y, rect.h, BODY, Weight::Semibold),
                    &name,
                    BODY,
                    Weight::Semibold,
                    fg,
                );
            }
            BarItem::Menu(i) => {
                let names = self.bar_names().1;
                if let Some(n) = names.get(i) {
                    text::draw(
                        c,
                        rect.x + chrome::BAR_PAD,
                        ty,
                        n,
                        BODY,
                        Weight::Regular,
                        fg,
                    );
                }
            }
            BarItem::Network => {
                let up = crate::netd::stats().link_up;
                let g = if up {
                    Glyph::Network
                } else {
                    Glyph::NetworkOff
                };
                ui::draw_glyph(c, g, rect.x + (rect.w - 16) / 2, rect.y + 6, 16, argb);
            }
            BarItem::Control => ui::draw_glyph(
                c,
                Glyph::Control,
                rect.x + (rect.w - 16) / 2,
                rect.y + 6,
                16,
                argb,
            ),
            BarItem::Search => ui::draw_glyph(
                c,
                Glyph::Search,
                rect.x + (rect.w - 16) / 2,
                rect.y + 6,
                16,
                argb,
            ),
            BarItem::Clock => {
                let t = self.clock_text(time);
                let w = text::measure(&t, BODY, Weight::Regular);
                text::draw(
                    c,
                    rect.right() - chrome::BAR_PAD - w,
                    ty,
                    &t,
                    BODY,
                    Weight::Regular,
                    fg,
                );
            }
        }
    }

    // ---- menus ----

    fn system_menu(&self) -> Vec<Entry> {
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

    fn app_menu(&self) -> Vec<Entry> {
        match self.focused().and_then(|id| self.kind_of(id)) {
            Some(k) => alloc::vec![Entry::item(
                &alloc::format!("Encerrar {}", k.label()),
                "",
                Cmd::Quit
            ),],
            None => alloc::vec![Entry::item("Mostrar apps", "", Cmd::ShowApps)],
        }
    }

    /// The focused app's menu number `i` (File, Edit, View, Window).
    fn kind_menu(&self, i: usize) -> Vec<Entry> {
        let name = self.bar_names().1.get(i).copied().unwrap_or("");
        self.named_menu(name)
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

    /// Open (or switch to) the menu of bar item `item`, anchored under it.
    pub(crate) fn open_bar_menu(&mut self, item: BarItem) {
        let Some((_, rect)) = self.bar_items().into_iter().find(|(i, _)| *i == item) else {
            return;
        };
        let entries = match item {
            BarItem::System => self.system_menu(),
            BarItem::AppName => self.app_menu(),
            BarItem::Menu(i) => self.kind_menu(i),
            _ => return,
        };
        self.open_menu(MenuOrigin::Bar(item), entries, (rect.x, MENUBAR_H));
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
        panel(
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

    /// Open the popover of `kind` under its bar item.
    pub(crate) fn open_popover(&mut self, kind: PopKind) {
        let item = match kind {
            PopKind::Control => BarItem::Control,
            PopKind::Calendar => BarItem::Clock,
        };
        let Some((_, anchor)) = self.bar_items().into_iter().find(|(i, _)| *i == item) else {
            return;
        };
        let (w, h) = match kind {
            PopKind::Control => (chrome::CONTROL_W, chrome::CONTROL_H),
            PopKind::Calendar => (chrome::CAL_W, chrome::CAL_H),
        };
        self.shell.menu = None;
        self.shell.pop = Some(Popover {
            kind,
            rect: popover_rect(anchor, w, h, self.sw),
            t: fade_in(MENU_FADE),
            closing: false,
            glass: Default::default(),
            month_off: 0,
        });
        self.shell.knobs = [
            tween_at(crate::settings::get().reduce_motion),
            tween_at(crate::settings::get().clock24),
            tween_at(crate::settings::get().toasts),
        ];
        self.force_full = true;
    }

    fn draw_popover(&self, c: &mut Canvas, pop: &Popover) {
        let p = theme::pal();
        let fade = level(&pop.t);
        let mut r = pop.rect;
        r.y -= ((256 - fade) as i32 * 6) / 256;
        panel(
            c,
            r,
            R_POPOVER,
            &pop.glass,
            12,
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
            PopKind::Control => self.draw_control(c, r),
            PopKind::Calendar => self.draw_calendar(c, r, pop.month_off),
        }
    }

    fn draw_control(&self, c: &mut Canvas, r: Rect) {
        let p = theme::pal();
        let g = chrome::control_geom(r);
        let s = crate::settings::get();
        text::draw_left(
            c,
            g.title,
            "Controles",
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        // Network card.
        ui::group_box(c, g.net);
        let st = crate::netd::stats();
        let up = st.link_up;
        ui::draw_glyph(
            c,
            if up {
                Glyph::Network
            } else {
                Glyph::NetworkOff
            },
            g.net.x + 14,
            g.net.y + (g.net.h - 20) / 2,
            20,
            0xFF00_0000
                | pack(if up {
                    theme::accent()
                } else {
                    theme::solid(p.text_secondary)
                }),
        );
        let (line1, line2) = match (&st.config, up) {
            (Some(cfg), true) => (String::from("Conectado"), alloc::format!("{}", cfg.ip)),
            (None, true) => (
                String::from("Conectando..."),
                String::from("Sem endereco ainda"),
            ),
            _ => (String::from("Sem rede"), String::from("Cabo desconectado")),
        };
        let tx = g.net.x + 46;
        text::draw(
            c,
            tx,
            g.net.y + 10,
            &line1,
            BODY,
            Weight::Medium,
            theme::solid(p.text),
        );
        text::draw(
            c,
            tx,
            g.net.y + 30,
            &line2,
            FOOTNOTE,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        // Appearance.
        ui::caption(c, g.appearance_label.x, g.appearance_label.y, "Aparência");
        let sel = match s.appearance {
            osjeff_core::style::AppearanceSetting::Auto => 0,
            osjeff_core::style::AppearanceSetting::Light => 1,
            osjeff_core::style::AppearanceSetting::Dark => 2,
        };
        ui::segmented(c, g.appearance, &["Automática", "Clara", "Escura"], sel);
        // Accent swatches.
        ui::caption(c, g.accent_label.x, g.accent_label.y, "Cor de destaque");
        for (i, sw) in g.swatches.iter().enumerate() {
            let rgb = osjeff_core::settings::ACCENTS[i];
            let col = Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
            if s.accent as usize == i {
                c.stroke_rrect(
                    sw.inflated(3),
                    15,
                    Corner::Circle,
                    theme::solid(p.text),
                    200,
                );
            }
            c.fill_rrect(*sw, sw.w / 2, Corner::Circle, col, 256);
        }
        // Switch rows.
        let rows = [
            ("Reduzir movimento", s.reduce_motion),
            ("Relógio de 24 horas", s.clock24),
            ("Notificações", s.toasts),
        ];
        for (i, ((label_r, sw_r), (label, _on))) in g.rows.iter().zip(rows).enumerate() {
            text::draw_left(
                c,
                *label_r,
                label,
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
            ui::switch(c, *sw_r, (self.shell.knobs[i].value() * 256.0) as i32, true);
        }
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
            PopKind::Control => {
                let g = chrome::control_geom(pop.rect);
                let mut s = crate::settings::get();
                if let Some(i) = osjeff_core::widgets::segmented_hit(g.appearance, 3, x, y) {
                    use osjeff_core::style::AppearanceSetting as A;
                    s.appearance = [A::Auto, A::Light, A::Dark][i];
                } else if let Some(i) = g.swatches.iter().position(|r| r.inflated(3).contains(x, y))
                {
                    s.accent = i as u8;
                } else if let Some(i) = g
                    .rows
                    .iter()
                    .position(|(l, sw)| l.contains(x, y) || sw.contains(x, y))
                {
                    match i {
                        0 => s.reduce_motion = !s.reduce_motion,
                        1 => s.clock24 = !s.clock24,
                        _ => s.toasts = !s.toasts,
                    }
                    let target = match i {
                        0 => s.reduce_motion,
                        1 => s.clock24,
                        _ => s.toasts,
                    };
                    self.shell.knobs[i].retarget(
                        if target { 1.0 } else { 0.0 },
                        0.18,
                        osjeff_core::anim::curves::ENTER,
                    );
                } else {
                    return true;
                }
                let _ = self.settings_apply(s);
                true
            }
            PopKind::Calendar => {
                let g = chrome::calendar_geom(pop.rect);
                if g.prev.contains(x, y) {
                    if let Some(p) = self.shell.pop.as_mut() {
                        p.month_off -= 1;
                    }
                } else if g.next.contains(x, y)
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
