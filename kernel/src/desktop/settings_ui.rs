//! The settings app: sections Aparencia, Hora e regiao, Teclado, Rede,
//! Armazenamento, Energia and Sobre.
//!
//! The model (`osjeff_core::settings::Settings`), its text form, the wallpaper
//! presets and the clock arithmetic are pure and tested in `osjeff_core`; this
//! file is the window: layout shared by drawing and hit-testing, the clicks,
//! and [`Desktop::settings_apply`], which makes a new `Settings` take effect
//! (accent, keyboard, time zone, clock format, wallpaper) and stores it
//! through the `SettingsStore` trait (FS v2 file `osjeff.conf` today).

use super::ui::*;
use super::*;
use core::fmt::Write as _;
use osjeff_core::hw::rtc::{DateTime, Field, TZ_MAX, TZ_MIN, local_to_utc, utc_to_local};
use osjeff_core::keymap::Layout;
use osjeff_core::klog::FixedBuf;
use osjeff_core::settings::{ACCENT_NAMES, ACCENTS, PATH_CAP, Settings, WallpaperChoice};
use osjeff_core::sysif::{DiskUsage, NetControl, NetControlError, NoNetControl, SettingsStore};
use osjeff_core::sysmon::{fmt_bytes, fmt_pct10, fmt_uptime};
use osjeff_core::wallpaper::{self, PRESETS, Style};

const SIDE_W: i32 = 200;
const SECTIONS: [&[u8]; 7] = [
    b"Aparencia",
    b"Hora e regiao",
    b"Teclado",
    b"Rede",
    b"Armazenamento",
    b"Energia",
    b"Sobre",
];
const SEC_APPEARANCE: u8 = 0;
const SEC_TIME: u8 = 1;
const SEC_KEYBOARD: u8 = 2;
const SEC_NET: u8 = 3;
const SEC_STORAGE: u8 = 4;
const SEC_POWER: u8 = 5;

/// Per-window state of the settings app.
pub(crate) struct SettingsState {
    section: u8,
    path: [u8; PATH_CAP],
    path_len: usize,
    path_focus: bool,
    msg: [u8; 60],
    msg_len: usize,
    /// The date and time being edited (local time) in "Hora e regiao".
    edit: DateTime,
    kbd_test: [u8; 26],
    kbd_len: usize,
    /// Power button waiting for a second click: 1 = reboot, 2 = shutdown.
    power_arm: u8,
}

impl SettingsState {
    pub(crate) fn new() -> Self {
        let s = crate::settings::get();
        let mut st = Self {
            section: 0,
            path: [0; PATH_CAP],
            path_len: 0,
            path_focus: false,
            msg: [0; 60],
            msg_len: 0,
            edit: read_local(),
            kbd_test: [0; 26],
            kbd_len: 0,
            power_arm: 0,
        };
        let p = s.image_path();
        st.path[..p.len()].copy_from_slice(p);
        st.path_len = p.len();
        st
    }

    fn set_msg(&mut self, m: &[u8]) {
        let n = m.len().min(self.msg.len());
        self.msg[..n].copy_from_slice(&m[..n]);
        self.msg_len = n;
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        0
    }
}

/// The RTC as local date and time.
fn read_local() -> DateTime {
    utc_to_local(crate::rtc::read_utc(), crate::rtc::tz_minutes())
}

// ------------------------------------------------------------------ geometry

struct Pane {
    side: [Rect; 7],
    pane: Rect,
}

impl Pane {
    fn of(r: Rect) -> Pane {
        let mut side = [Rect::new(0, 0, 0, 0); 7];
        for (i, s) in side.iter_mut().enumerate() {
            *s = Rect::new(r.x + 8, r.y + TITLE_H + 12 + i as i32 * 38, SIDE_W - 16, 34);
        }
        let x = r.x + SIDE_W + 18;
        Pane {
            side,
            pane: Rect::new(
                x,
                r.y + TITLE_H + 14,
                r.right() - 16 - x,
                r.h - TITLE_H - 24,
            ),
        }
    }
}

struct Appearance {
    chips: [Rect; 6],
    path: Rect,
    apply: Rect,
    swatches: [Rect; 8],
    clock24: Rect,
    clock12: Rect,
    toasts_on: Rect,
    toasts_off: Rect,
    msg: Rect,
}

impl Appearance {
    fn of(p: Rect) -> Appearance {
        let mut chips = [Rect::new(0, 0, 0, 0); 6];
        let cw = ((p.w - 5 * 8) / 6).clamp(60, 92);
        for (i, c) in chips.iter_mut().enumerate() {
            *c = Rect::new(p.x + i as i32 * (cw + 8), p.y + 26, cw, 50);
        }
        let path_y = p.y + 26 + 50 + 20 + 26 + 6;
        let apply = Rect::new(p.right() - 100, path_y, 100, 28);
        let path = Rect::new(p.x, path_y, apply.x - 8 - p.x, 28);
        let sw_y = path_y + 28 + 12 + 26;
        let mut swatches = [Rect::new(0, 0, 0, 0); 8];
        let sw = ((p.w - 7 * 8) / 8).clamp(30, 52);
        for (i, s) in swatches.iter_mut().enumerate() {
            *s = Rect::new(p.x + i as i32 * (sw + 8), sw_y, sw, 30);
        }
        let row_y = sw_y + 30 + 14 + 26;
        Appearance {
            chips,
            path,
            apply,
            swatches,
            clock24: Rect::new(p.x, row_y, 70, 28),
            clock12: Rect::new(p.x + 78, row_y, 70, 28),
            toasts_on: Rect::new(p.x + 200, row_y, 110, 28),
            toasts_off: Rect::new(p.x + 318, row_y, 130, 28),
            msg: Rect::new(p.x, row_y + 40, p.w, 20),
        }
    }
}

struct TimeSec {
    tz_minus: Rect,
    tz_plus: Rect,
    tz_value: Rect,
    tz_reset: Rect,
    /// `[+]` button over each editable field, in `FIELDS` order.
    up: [Rect; 6],
    val: [Rect; 6],
    down: [Rect; 6],
    apply: Rect,
    read: Rect,
}

/// The editable fields, left to right (and the width of each in the layout).
const FIELDS: [(Field, i32); 6] = [
    (Field::Day, 56),
    (Field::Month, 56),
    (Field::Year, 80),
    (Field::Hour, 56),
    (Field::Minute, 56),
    (Field::Second, 56),
];

impl TimeSec {
    fn of(p: Rect) -> TimeSec {
        let tz_y = p.y + 100;
        let mut up = [Rect::new(0, 0, 0, 0); 6];
        let mut val = up;
        let mut down = up;
        let ey = tz_y + 28 + 22 + 26;
        let mut x = p.x;
        for (i, (_, w)) in FIELDS.iter().enumerate() {
            // A little extra air between the date and the time groups.
            if i == 3 {
                x += 16;
            }
            up[i] = Rect::new(x, ey, *w, 24);
            val[i] = Rect::new(x, ey + 28, *w, 30);
            down[i] = Rect::new(x, ey + 62, *w, 24);
            x += w + 8;
        }
        TimeSec {
            tz_minus: Rect::new(p.x, tz_y, 36, 28),
            tz_value: Rect::new(p.x + 44, tz_y, 130, 28),
            tz_plus: Rect::new(p.x + 182, tz_y, 36, 28),
            tz_reset: Rect::new(p.x + 232, tz_y, 200, 28),
            up,
            val,
            down,
            apply: Rect::new(p.x, ey + 96, 120, 28),
            read: Rect::new(p.x + 128, ey + 96, 150, 28),
        }
    }
}

struct KbdSec {
    us: Rect,
    abnt2: Rect,
    test: Rect,
}

impl KbdSec {
    fn of(p: Rect) -> KbdSec {
        KbdSec {
            us: Rect::new(p.x, p.y + 28, 170, 34),
            abnt2: Rect::new(p.x + 180, p.y + 28, 200, 34),
            test: Rect::new(p.x, p.y + 176, (p.w).min(360), 32),
        }
    }
}

fn renew_btn(p: Rect) -> Rect {
    Rect::new(p.x, p.y + 232, 190, 30)
}

fn power_btns(p: Rect) -> [Rect; 2] {
    [
        Rect::new(p.x, p.y + 30, 170, 36),
        Rect::new(p.x + 182, p.y + 30, 170, 36),
    ]
}

// --------------------------------------------------------------------- actions

impl Desktop {
    fn settings_mut(&mut self, id: WindowId) -> Option<&mut SettingsState> {
        match self.app_mut(id) {
            Some(App::Settings(s)) => Some(s),
            _ => None,
        }
    }

    /// Make `new` the settings in effect: accent, keyboard layout, time zone,
    /// clock format and toasts apply at once, a changed wallpaper or accent
    /// asks the compositor to repaint the background, and the text form is
    /// stored through the `SettingsStore` (FS v2 file `osjeff.conf`).
    pub(crate) fn settings_apply(
        &mut self,
        new: Settings,
    ) -> Result<(), osjeff_core::sysif::SinkError> {
        let old = crate::settings::get();
        crate::settings::set(new);
        self.keymap.set_layout(new.layout);
        if new.wallpaper != old.wallpaper
            || new.image_path() != old.image_path()
            || new.accent != old.accent
        {
            self.bg_dirty = true;
        }
        // Everything on screen may change (clock text, accent colours).
        self.force_full = true;
        FsV2Store.save(&new.to_text())
    }

    /// Load the stored settings at boot (before the first wallpaper paint).
    pub fn load_settings(&mut self) {
        match FsV2Store.load() {
            Some(text) => {
                let s = Settings::parse(&text);
                crate::settings::set(s);
                self.keymap.set_layout(s.layout);
                crate::klog!(
                    Info,
                    "settings: loaded {} bytes from osjeff.conf",
                    text.len()
                );
            }
            None => crate::settings::set(Settings::default()),
        }
    }

    /// Did the wallpaper or accent change since the compositor last painted it?
    /// Consumed by the main loop, which repaints the cached background.
    pub fn take_bg_repaint(&mut self) -> bool {
        core::mem::take(&mut self.bg_dirty)
    }

    fn settings_wallpaper(&mut self, id: WindowId, choice: WallpaperChoice) {
        let mut s = crate::settings::get();
        if let WallpaperChoice::Image = choice {
            let (w, h) = (self.sw as usize, self.sh as usize);
            let Some(st) = self.settings_mut(id) else {
                return;
            };
            let path = st.path[..st.path_len].to_vec();
            if !s.set_image_path(&path) {
                st.set_msg(b"caminho invalido (use letras, numeros . _ - /)");
                return;
            }
            // Check the file now so a bad path does not blank the desktop.
            match read_path(&path) {
                None => {
                    st.set_msg(b"arquivo nao encontrado no disco");
                    return;
                }
                Some(bytes) => {
                    if let Err(e) = wallpaper::load(bytes, w, h) {
                        let mut m = FixedBuf::<60>::new();
                        let _ = write!(m, "imagem recusada: {e}");
                        st.set_msg(m.as_bytes());
                        return;
                    }
                }
            }
            st.set_msg(b"papel de parede aplicado");
        }
        s.wallpaper = choice;
        let _ = self.settings_apply(s);
    }

    pub(crate) fn settings_key(&mut self, id: WindowId, key: Key) {
        let Some(st) = self.settings_mut(id) else {
            return;
        };
        st.power_arm = 0;
        if st.path_focus && st.section == SEC_APPEARANCE {
            match key {
                Key::Enter => {
                    self.settings_wallpaper(id, WallpaperChoice::Image);
                    return;
                }
                Key::Esc => st.path_focus = false,
                Key::Backspace => st.path_len = st.path_len.saturating_sub(1),
                Key::Char(b) if (0x21..0x7F).contains(&b) && st.path_len < PATH_CAP => {
                    st.path[st.path_len] = b;
                    st.path_len += 1;
                }
                _ => {}
            }
            return;
        }
        if st.section == SEC_KEYBOARD
            && let Key::Char(b) = key
        {
            // The keyboard test line: shows what the layout types (accents included).
            if st.kbd_len == st.kbd_test.len() {
                st.kbd_test.copy_within(1.., 0);
                st.kbd_len -= 1;
            }
            st.kbd_test[st.kbd_len] = b;
            st.kbd_len += 1;
            return;
        }
        let mut close = false;
        match key {
            Key::Esc => close = true,
            Key::Tab | Key::Down => st.section = (st.section + 1) % 7,
            Key::Up => st.section = (st.section + 6) % 7,
            Key::Backspace if st.section == SEC_KEYBOARD => {
                st.kbd_len = st.kbd_len.saturating_sub(1)
            }
            _ => {}
        }
        if st.section == SEC_TIME {
            st.edit = read_local();
        }
        if close {
            self.request_close(id);
        }
    }

    pub(crate) fn settings_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let lay = Pane::of(rect);
        let Some(st) = self.settings_mut(id) else {
            return;
        };
        let was_armed = core::mem::take(&mut st.power_arm);
        if let Some(i) = lay.side.iter().position(|s| s.contains(px, py)) {
            st.section = i as u8;
            st.path_focus = false;
            if i as u8 == SEC_TIME {
                st.edit = read_local();
            }
            return;
        }
        let mut s = crate::settings::get();
        match st.section {
            SEC_APPEARANCE => {
                let a = Appearance::of(lay.pane);
                st.path_focus = a.path.contains(px, py);
                if let Some(i) = a.chips.iter().position(|c| c.contains(px, py)) {
                    if i < PRESETS.len() {
                        s.wallpaper = WallpaperChoice::Preset(i as u8);
                        st.set_msg(b"");
                        let _ = self.settings_apply(s);
                    } else {
                        self.settings_wallpaper(id, WallpaperChoice::Image);
                    }
                } else if a.apply.contains(px, py) {
                    self.settings_wallpaper(id, WallpaperChoice::Image);
                } else if let Some(i) = a.swatches.iter().position(|c| c.contains(px, py)) {
                    s.accent = i as u8;
                    let _ = self.settings_apply(s);
                } else if a.clock24.contains(px, py) || a.clock12.contains(px, py) {
                    s.clock24 = a.clock24.contains(px, py);
                    let _ = self.settings_apply(s);
                } else if a.toasts_on.contains(px, py) || a.toasts_off.contains(px, py) {
                    s.toasts = a.toasts_on.contains(px, py);
                    let _ = self.settings_apply(s);
                }
            }
            SEC_TIME => {
                let t = TimeSec::of(lay.pane);
                let tz = s.tz_minutes as i32;
                let mut new_tz = None;
                if t.tz_minus.contains(px, py) {
                    new_tz = Some((tz - 30).max(TZ_MIN));
                } else if t.tz_plus.contains(px, py) {
                    new_tz = Some((tz + 30).min(TZ_MAX));
                } else if t.tz_reset.contains(px, py) {
                    new_tz = Some(-180);
                }
                if let Some(z) = new_tz {
                    s.tz_minutes = z as i16;
                    let _ = self.settings_apply(s);
                    if let Some(st) = self.settings_mut(id) {
                        st.edit = read_local();
                    }
                    return;
                }
                for (i, (f, _)) in FIELDS.iter().enumerate() {
                    if t.up[i].contains(px, py) {
                        st.edit = st.edit.step(*f, 1);
                    } else if t.down[i].contains(px, py) {
                        st.edit = st.edit.step(*f, -1);
                    }
                }
                if t.read.contains(px, py) {
                    st.edit = read_local();
                    st.set_msg(b"lido do relogio");
                } else if t.apply.contains(px, py) {
                    let tzm = crate::rtc::tz_minutes();
                    if st.edit.is_valid() {
                        crate::rtc::set_utc(&local_to_utc(st.edit, tzm));
                        st.set_msg(b"relogio ajustado");
                        crate::klog!(Info, "rtc: clock set by the user");
                    } else {
                        st.set_msg(b"data invalida");
                    }
                }
            }
            SEC_KEYBOARD => {
                let k = KbdSec::of(lay.pane);
                let want = if k.us.contains(px, py) {
                    Some(Layout::Us)
                } else if k.abnt2.contains(px, py) {
                    Some(Layout::Abnt2)
                } else {
                    None
                };
                if let Some(l) = want {
                    s.layout = l;
                    let _ = self.settings_apply(s);
                }
            }
            SEC_NET => {
                if renew_btn(lay.pane).contains(px, py) {
                    let msg: &[u8] = match NoNetControl.renew_dhcp() {
                        Ok(()) => b"renovacao pedida",
                        Err(NetControlError::Unsupported) => b"indisponivel: sem API de rede ainda",
                        Err(NetControlError::Busy) => b"rede ocupada, tente de novo",
                    };
                    st.set_msg(msg);
                }
            }
            SEC_POWER => {
                let [reboot, shutdown] = power_btns(lay.pane);
                let which = if reboot.contains(px, py) {
                    1
                } else if shutdown.contains(px, py) {
                    2
                } else {
                    0
                };
                if which != 0 {
                    if was_armed == which {
                        crate::klog!(
                            Info,
                            "power: {} requested from settings",
                            if which == 1 { "reboot" } else { "shutdown" }
                        );
                        if which == 1 {
                            crate::power::reboot()
                        } else {
                            crate::power::shutdown()
                        }
                    }
                    st.power_arm = which;
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ drawing

    pub(crate) fn draw_settings(&self, c: &mut Canvas, r: Rect, st: &SettingsState) {
        let lay = Pane::of(r);
        // Sidebar.
        fill(
            c,
            Rect::new(r.x, r.y + TITLE_H, SIDE_W, r.h - TITLE_H),
            SIDEBAR,
        );
        for (i, rect) in lay.side.iter().enumerate() {
            if st.section as usize == i {
                fill_round(c, *rect, 8, theme::accent());
                text(
                    c,
                    rect.x + 10,
                    rect.y + 10,
                    rect.w - 14,
                    SECTIONS[i],
                    Color::rgb(0x08, 0x12, 0x1E),
                );
            } else {
                text(
                    c,
                    rect.x + 10,
                    rect.y + 10,
                    rect.w - 14,
                    SECTIONS[i],
                    SIDEBAR_TEXT,
                );
            }
        }
        let s = crate::settings::get();
        let p = lay.pane;
        match st.section {
            SEC_APPEARANCE => self.draw_appearance(c, p, st, &s),
            SEC_TIME => self.draw_time(c, p, st, &s),
            SEC_KEYBOARD => self.draw_keyboard(c, p, st, &s),
            SEC_NET => self.draw_network(c, p, st),
            SEC_STORAGE => self.draw_storage(c, p),
            SEC_POWER => self.draw_power(c, p, st),
            _ => self.draw_about(c, p),
        }
    }

    fn heading(&self, c: &mut Canvas, p: Rect, y: i32, t: &[u8]) {
        text(c, p.x, y, p.w, t, theme::TEXT_MUTED);
    }

    fn status(&self, c: &mut Canvas, r: Rect, st: &SettingsState) {
        text(
            c,
            r.x,
            r.y + 2,
            r.w,
            &st.msg[..st.msg_len],
            theme::TEXT_MUTED,
        );
    }

    fn draw_appearance(&self, c: &mut Canvas, p: Rect, st: &SettingsState, s: &Settings) {
        let a = Appearance::of(p);
        self.heading(c, p, p.y, b"Papel de parede");
        for (i, rect) in a.chips.iter().enumerate() {
            let selected = match s.wallpaper {
                WallpaperChoice::Preset(n) => i == n as usize,
                WallpaperChoice::Image => i == PRESETS.len(),
            };
            if selected {
                fill_round(c, rect.inflated(3), 9, theme::accent());
            }
            let prev = Rect::new(rect.x, rect.y, rect.w, rect.h);
            if let Some(pr) = PRESETS.get(i) {
                preview(c, prev, pr.style, pr.top, pr.bottom);
                let name = pr.name.as_bytes();
                text(c, rect.x, rect.bottom() + 4, rect.w + 8, name, theme::TEXT);
            } else {
                fill_round(c, prev, 8, Color::rgb(0x1E, 0x2A, 0x44));
                // A tiny "picture": sun and hills.
                fill_round(
                    c,
                    Rect::new(prev.x + 12, prev.y + 10, 10, 10),
                    5,
                    Color::rgb(0xF5, 0x9E, 0x0B),
                );
                fill_round(
                    c,
                    Rect::new(prev.x + 8, prev.y + 26, prev.w - 16, prev.h - 26 - 4),
                    6,
                    Color::rgb(0x34, 0xD3, 0x99),
                );
                text(
                    c,
                    rect.x,
                    rect.bottom() + 4,
                    rect.w + 8,
                    b"Imagem",
                    theme::TEXT,
                );
            }
        }
        let path_label_y = a.path.y - 22;
        self.heading(
            c,
            p,
            path_label_y,
            b"Imagem do usuario (arquivo PNG/BMP/PPM)",
        );
        input_box(
            c,
            a.path,
            &st.path[..st.path_len],
            b"ex.: papel.png",
            st.path_focus,
        );
        button(c, a.apply, b"Aplicar", Btn::Normal);
        self.heading(c, p, a.swatches[0].y - 24, b"Cor de destaque");
        for (i, rect) in a.swatches.iter().enumerate() {
            if s.accent as usize == i {
                fill_round(c, rect.inflated(3), 9, theme::TEXT);
            }
            let rgb = ACCENTS[i];
            fill_round(
                c,
                *rect,
                7,
                Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8),
            );
        }
        let name = ACCENT_NAMES[(s.accent as usize).min(7)].as_bytes();
        text(
            c,
            p.right() - 8 * CELL_W,
            a.swatches[0].y - 24,
            8 * CELL_W,
            name,
            theme::TEXT,
        );
        self.heading(c, p, a.clock24.y - 24, b"Relogio");
        button(
            c,
            a.clock24,
            b"24 h",
            if s.clock24 { Btn::On } else { Btn::Normal },
        );
        button(
            c,
            a.clock12,
            b"12 h",
            if s.clock24 { Btn::Normal } else { Btn::On },
        );
        text(
            c,
            a.toasts_on.x,
            a.clock24.y - 24,
            200,
            b"Notificacoes",
            theme::TEXT_MUTED,
        );
        button(
            c,
            a.toasts_on,
            b"Ligadas",
            if s.toasts { Btn::On } else { Btn::Normal },
        );
        button(
            c,
            a.toasts_off,
            b"Desligadas",
            if s.toasts { Btn::Normal } else { Btn::On },
        );
        self.status(c, a.msg, st);
    }

    fn draw_time(&self, c: &mut Canvas, p: Rect, st: &SettingsState, s: &Settings) {
        let t = TimeSec::of(p);
        self.heading(c, p, p.y, b"Agora (hora local)");
        let now = read_local();
        let mut b = FixedBuf::<48>::new();
        const WD: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sab"];
        let _ = write!(
            b,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} ({})",
            now.date.y,
            now.date.m,
            now.date.d,
            now.time.h,
            now.time.m,
            now.time.s,
            WD[now.weekday() as usize % 7]
        );
        text(c, p.x, p.y + 24, p.w, b.as_bytes(), theme::TEXT);
        let utc = now.shifted(-(s.tz_minutes as i32));
        let mut b = FixedBuf::<48>::new();
        let _ = write!(
            b,
            "UTC {:02}:{:02}:{:02}",
            utc.time.h, utc.time.m, utc.time.s
        );
        text(c, p.x, p.y + 46, p.w, b.as_bytes(), theme::TEXT_MUTED);
        self.heading(c, p, t.tz_minus.y - 24, b"Fuso horario");
        button(c, t.tz_minus, b"-", Btn::Normal);
        let tz = s.tz_minutes as i32;
        let mut b = FixedBuf::<16>::new();
        let _ = write!(
            b,
            "UTC{}{:02}:{:02}",
            if tz < 0 { '-' } else { '+' },
            tz.abs() / 60,
            tz.abs() % 60
        );
        fill_round(c, t.tz_value, 8, theme::WHITE);
        text_center(c, t.tz_value, b.as_bytes(), theme::TEXT);
        button(c, t.tz_plus, b"+", Btn::Normal);
        button(c, t.tz_reset, b"Brasilia (UTC-3)", Btn::Normal);
        self.heading(
            c,
            p,
            t.up[0].y - 24,
            b"Ajustar data e hora (dia mes ano - h m s)",
        );
        let e = st.edit;
        let vals: [u32; 6] = [
            e.date.d as u32,
            e.date.m as u32,
            e.date.y as u32,
            e.time.h as u32,
            e.time.m as u32,
            e.time.s as u32,
        ];
        for i in 0..6 {
            button(c, t.up[i], b"+", Btn::Normal);
            fill_round(c, t.val[i], 8, theme::WHITE);
            let mut b = FixedBuf::<8>::new();
            if FIELDS[i].1 > 70 {
                let _ = write!(b, "{:04}", vals[i]);
            } else {
                let _ = write!(b, "{:02}", vals[i]);
            }
            text_center(c, t.val[i], b.as_bytes(), theme::TEXT);
            button(c, t.down[i], b"-", Btn::Normal);
        }
        button(c, t.apply, b"Ajustar", Btn::On);
        button(c, t.read, b"Ler do relogio", Btn::Normal);
        text(
            c,
            p.x,
            t.apply.bottom() + 12,
            p.w,
            &st.msg[..st.msg_len],
            theme::TEXT_MUTED,
        );
    }

    fn draw_keyboard(&self, c: &mut Canvas, p: Rect, st: &SettingsState, s: &Settings) {
        let k = KbdSec::of(p);
        self.heading(c, p, p.y, b"Layout do teclado");
        button(
            c,
            k.us,
            b"US (padrao)",
            if s.layout == Layout::Us {
                Btn::On
            } else {
                Btn::Normal
            },
        );
        button(
            c,
            k.abnt2,
            b"ABNT2 (pt-BR)",
            if s.layout == Layout::Abnt2 {
                Btn::On
            } else {
                Btn::Normal
            },
        );
        let lines: [&[u8]; 3] = if s.layout == Layout::Abnt2 {
            [
                b"ABNT2: tecla de cedilha, acentos mortos",
                b"(' ` ~ ^ e trema) combinam com vogais.",
                b"Ex.: ' depois de a = a com acento agudo.",
            ]
        } else {
            [
                b"US: layout padrao do OSjeff.",
                b"Escolha ABNT2 para digitar \xE7, \xE1, \xE3, \xEA...",
                b"",
            ]
        };
        for (i, l) in lines.iter().enumerate() {
            text(c, p.x, p.y + 76 + i as i32 * 20, p.w, l, theme::TEXT_MUTED);
        }
        self.heading(c, p, k.test.y - 24, b"Teste de digitacao");
        input_box(c, k.test, &st.kbd_test[..st.kbd_len], b"digite aqui", true);
    }

    fn draw_network(&self, c: &mut Canvas, p: Rect, st: &SettingsState) {
        let mut y = p.y;
        self.heading(c, p, y, b"Rede (somente leitura)");
        y += 28;
        // Live view of the network owner (`netd`): the NIC, the link, the lease in
        // force and the resolver list, refreshed every time the page is drawn.
        let snap = crate::netd::stats();
        let has_nic = snap.nic != osjeff_core::netstats::NicKind::None;
        kv_row(
            c,
            p,
            &mut y,
            b"Placa",
            if has_nic {
                snap.nic.name().as_bytes()
            } else {
                b"nenhuma"
            },
        );
        let m = crate::nic::mac();
        let mut b = FixedBuf::<24>::new();
        let _ = write!(
            b,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            m[0], m[1], m[2], m[3], m[4], m[5]
        );
        kv_row(
            c,
            p,
            &mut y,
            b"MAC",
            if has_nic { b.as_bytes() } else { b"-" },
        );
        kv_row(
            c,
            p,
            &mut y,
            b"Link",
            if !has_nic {
                b"-"
            } else if snap.link_up {
                b"ativo"
            } else {
                b"sem sinal"
            },
        );
        match snap.config {
            Some(cfg) => {
                let mut b = FixedBuf::<24>::new();
                let _ = write!(b, "{}/{}", cfg.ip, cfg.prefix);
                kv_row(c, p, &mut y, b"IP", b.as_bytes());
                let mask = if cfg.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - cfg.prefix as u32)
                };
                let mut b = FixedBuf::<24>::new();
                let _ = write!(
                    b,
                    "{}.{}.{}.{}",
                    mask >> 24,
                    (mask >> 16) & 255,
                    (mask >> 8) & 255,
                    mask & 255
                );
                kv_row(c, p, &mut y, b"Mascara", b.as_bytes());
                let mut b = FixedBuf::<24>::new();
                match cfg.gateway {
                    Some(g) => {
                        let _ = write!(b, "{g}");
                    }
                    None => {
                        let _ = write!(b, "-");
                    }
                }
                kv_row(c, p, &mut y, b"Gateway", b.as_bytes());
                let mut b = FixedBuf::<48>::new();
                if cfg.dns.is_empty() {
                    let _ = write!(b, "-");
                }
                for (i, d) in cfg.dns.as_slice().iter().enumerate() {
                    if i > 0 {
                        let _ = write!(b, ", ");
                    }
                    let _ = write!(b, "{d}");
                }
                kv_row(c, p, &mut y, b"DNS", b.as_bytes());
                let mut b = FixedBuf::<48>::new();
                let fallback = cfg == osjeff_core::net::NetConfig::STATIC_FALLBACK;
                if fallback {
                    let _ = write!(b, "estatico (sem servidor DHCP)");
                } else if let Some(left) = snap.lease_remaining_ms {
                    let _ = write!(b, "DHCP ({}), restam {} s", snap.dhcp_state, left / 1000);
                } else {
                    let _ = write!(b, "DHCP ({}), sem expiracao", snap.dhcp_state);
                }
                kv_row(c, p, &mut y, b"Lease", b.as_bytes());
            }
            None => kv_row(c, p, &mut y, b"Endereco", b"sem endereco (procurando)"),
        }
        button(c, renew_btn(p), b"Renovar DHCP", Btn::Normal);
        text(
            c,
            p.x,
            renew_btn(p).bottom() + 10,
            p.w,
            &st.msg[..st.msg_len],
            theme::TEXT_MUTED,
        );
    }

    fn draw_storage(&self, c: &mut Canvas, p: Rect) {
        self.heading(c, p, p.y, b"Armazenamento");
        let usage = FsV2Usage;
        let u = usage.usage();
        let mut lbl = FixedBuf::<40>::new();
        let _ = write!(lbl, "{}", usage.label());
        text(c, p.x, p.y + 30, p.w, lbl.as_bytes(), theme::TEXT);
        match (u.used_bytes, u.total_bytes, u.used_permille()) {
            (Some(used), Some(total), Some(pm)) => {
                usage_bar(
                    c,
                    Rect::new(p.x, p.y + 56, p.w.min(420), 16),
                    pm,
                    theme::accent(),
                );
                let mut b = FixedBuf::<64>::new();
                let _ = write!(
                    b,
                    "{} de {} ({})",
                    fmt_bytes(used),
                    fmt_bytes(total),
                    fmt_pct10(pm)
                );
                text(c, p.x, p.y + 80, p.w, b.as_bytes(), theme::TEXT);
                if let (Some(a), Some(t)) = (u.items_used, u.items_total) {
                    let mut b = FixedBuf::<40>::new();
                    let _ = write!(b, "{a} de {t} entradas usadas");
                    text(c, p.x, p.y + 102, p.w, b.as_bytes(), theme::TEXT_MUTED);
                }
            }
            _ => text(c, p.x, p.y + 56, p.w, b"n/d", theme::TEXT_MUTED),
        }
        let mut y = p.y + 140;
        for (i, label) in [&b"Disco 0 (boot)"[..], b"Disco 1 (FS)"].iter().enumerate() {
            text(c, p.x, y, p.w, label, theme::TEXT_MUTED);
            let mut b = FixedBuf::<56>::new();
            match self.disks.get(i).copied().flatten() {
                Some(d) => {
                    for &ch in &d.model[..d.model_len] {
                        let _ = b.write_char(ch as char);
                    }
                    let _ = write!(b, " {} MiB", d.mib());
                }
                None => {
                    let _ = write!(b, "ausente");
                }
            }
            text(
                c,
                p.x + 16 * CELL_W,
                y,
                p.w - 16 * CELL_W,
                b.as_bytes(),
                theme::TEXT,
            );
            y += 26;
        }
    }

    fn draw_power(&self, c: &mut Canvas, p: Rect, st: &SettingsState) {
        self.heading(c, p, p.y, b"Energia");
        let [reboot, shutdown] = power_btns(p);
        let label = |armed: bool, normal: &'static [u8]| -> &'static [u8] {
            if armed { b"Confirmar?" } else { normal }
        };
        let state = |armed: bool| if armed { Btn::On } else { Btn::Normal };
        button(
            c,
            reboot,
            label(st.power_arm == 1, b"Reiniciar"),
            state(st.power_arm == 1),
        );
        button(
            c,
            shutdown,
            label(st.power_arm == 2, b"Desligar"),
            state(st.power_arm == 2),
        );
        text(
            c,
            p.x,
            p.y + 84,
            p.w,
            b"Clique duas vezes para confirmar.",
            theme::TEXT_MUTED,
        );
        text(
            c,
            p.x,
            p.y + 120,
            p.w,
            b"Bateria: n/d (sem ACPI)",
            theme::TEXT,
        );
        text(c, p.x, p.y + 144, p.w, b"Fonte: n/d", theme::TEXT_MUTED);
    }

    fn draw_about(&self, c: &mut Canvas, p: Rect) {
        icons::draw(c, Icon::Brand, p.x as usize, p.y as usize, 64);
        text(c, p.x + 84, p.y + 6, p.w, b"OSjeff", theme::TEXT);
        let mut b = FixedBuf::<48>::new();
        let _ = write!(b, "versao {}", env!("CARGO_PKG_VERSION"));
        text(c, p.x + 84, p.y + 30, p.w, b.as_bytes(), theme::TEXT_MUTED);
        let lines: [&[u8]; 5] = [
            b"Sistema operacional x86_64 bare-metal",
            b"escrito em Rust: kernel, drivers, GUI,",
            b"navegador e apps WebAssembly.",
            b"Licenca MIT.",
            b"",
        ];
        for (i, l) in lines.iter().enumerate() {
            text(c, p.x, p.y + 90 + i as i32 * 22, p.w, l, theme::TEXT);
        }
        let mut b = FixedBuf::<48>::new();
        let _ = write!(b, "ligado ha {}", fmt_uptime(self.sysmon.uptime_s));
        text(
            c,
            p.x,
            p.y + 90 + 5 * 22,
            p.w,
            b.as_bytes(),
            theme::TEXT_MUTED,
        );
        if let Some(si) = crate::sysinfo::get() {
            let mut b = FixedBuf::<60>::new();
            let _ = write!(b, "{}x{}  {}", si.width, si.height, si.boot_mode);
            text(
                c,
                p.x,
                p.y + 90 + 6 * 22,
                p.w,
                b.as_bytes(),
                theme::TEXT_MUTED,
            );
        }
    }
}

const SIDEBAR: Color = Color::rgb(0x18, 0x1D, 0x27);
const SIDEBAR_TEXT: Color = Color::rgb(0xE6, 0xEA, 0xF2);

/// A wallpaper thumbnail: the gradient / solid colour the preset paints.
fn preview(c: &mut Canvas, r: Rect, style: Style, top: u32, bottom: u32) {
    let rgb = |v: u32| Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
    fill_round(c, r, 8, rgb(top));
    match style {
        Style::Solid => {}
        Style::Gradient | Style::Indigo => {
            // Bands, clipped by the rounded top/bottom rows being drawn first.
            let inner = Rect::new(r.x, r.y + 6, r.w, r.h - 12);
            for i in 0..inner.h {
                let t = (i * 255 / inner.h.max(1)) as u32;
                fill(
                    c,
                    Rect::new(inner.x, inner.y + i, inner.w, 1),
                    rgb(wallpaper::lerp_rgb(top, bottom, t)),
                );
            }
            fill_round(c, Rect::new(r.x, r.bottom() - 12, r.w, 12), 8, rgb(bottom));
        }
    }
    if style == Style::Indigo {
        // The two soft glows of the original wallpaper.
        fill_round(
            c,
            Rect::new(r.x + 4, r.y + 4, 16, 16),
            8,
            Color::rgb(0x16, 0x32, 0x3C),
        );
        fill_round(
            c,
            Rect::new(r.right() - 22, r.bottom() - 22, 18, 18),
            9,
            Color::rgb(0x28, 0x2E, 0x58),
        );
    }
}

/// One "key  value" line of a read-only page at `*y`; advances `*y`.
fn kv_row(c: &mut Canvas, p: Rect, y: &mut i32, k: &[u8], v: &[u8]) {
    text(c, p.x, *y, 12 * CELL_W, k, theme::TEXT_MUTED);
    text(c, p.x + 12 * CELL_W, *y, p.w - 12 * CELL_W, v, theme::TEXT);
    *y += 26;
}
