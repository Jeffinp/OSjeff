//! The simulated desktop: windows with workspaces, z-order, animations and three kinds of
//! content, plus a panel, a taskbar, a popover, a toast and a snap preview. It produces the
//! [`Scene`] the engine plans from; the model painter draws the same state from the same data.

use super::paint::{H, PANEL_H, RADIUS, SHADOW_DY, SHADOW_R, W};
use crate::windowing::compositor::{Layer, LayerId, Scene};
use crate::windowing::window::Rect;
use alloc::vec::Vec;

/// Fixed layer ids (windows use their own small numbers).
pub mod ids {
    use crate::windowing::compositor::LayerId;
    pub const PANEL: LayerId = LayerId(1000);
    pub const TASKBAR: LayerId = LayerId(1001);
    pub const POPOVER: LayerId = LayerId(1002);
    pub const TOAST: LayerId = LayerId(1003);
    pub const SNAP: LayerId = LayerId(1004);
}

/// What a window's content does by itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Changes only when the user does something.
    Plain,
    /// A chart area changes every tick (the Tarefas case): reported as a dirty rectangle.
    Live,
    /// Everything changes every tick (the Snake case): reported as a new look.
    Game,
}

/// How an animation ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    Stay,
    Close,
    Minimize,
}

/// A window animation: the rectangle and the opacity travel from `from` to `to`.
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    pub from: Rect,
    pub to: Rect,
    pub a_from: u32,
    pub a_to: u32,
    pub step: u32,
    pub steps: u32,
    pub end: End,
}

#[derive(Clone, Debug)]
pub struct Win {
    pub id: u32,
    pub kind: Kind,
    pub rect: Rect,
    pub restore: Rect,
    pub maximized: bool,
    pub minimized: bool,
    pub ws: u8,
    pub ver: u32,
    pub part: u32,
    pub part_dirty: bool,
    pub anim: Option<Anim>,
}

/// What the painter needs to draw a window right now.
#[derive(Clone, Copy, Debug)]
pub struct WinView {
    pub id: u32,
    pub rect: Rect,
    pub alpha: u32,
    pub ver: u32,
    pub part: u32,
    pub focused: bool,
    pub maximized: bool,
    pub shadow: bool,
}

/// The rectangle a shadowed rectangle's shadow ring and body occupy.
pub fn shadow_extent(r: &Rect) -> Rect {
    Rect::new(
        r.x - SHADOW_R,
        r.y + SHADOW_DY - SHADOW_R,
        r.w + 2 * SHADOW_R,
        r.h + 2 * SHADOW_R,
    )
    .union(r)
}

/// Where the clock of the panel is.
pub fn clock_rect() -> Rect {
    Rect::new(88, 1, 24, PANEL_H - 2)
}

/// The taskbar's bar.
pub fn taskbar_rect() -> Rect {
    Rect::new(56, H - 18, 80, 14)
}

/// The area windows maximise into.
pub fn work_area() -> Rect {
    Rect::new(0, PANEL_H, W, H - PANEL_H)
}

impl WinView {
    /// Everything the painter draws for this window (honest).
    pub fn paint_extent(&self) -> Rect {
        if self.shadow {
            shadow_extent(&self.rect)
        } else {
            self.rect
        }
    }
}

#[derive(Clone, Debug)]
pub struct World {
    /// Windows from the bottom of the stack to the top.
    pub wins: Vec<Win>,
    pub ws: u8,
    next_id: u32,
    pub clock: u32,
    pub hover: u32,
    pub popover: Option<Rect>,
    pub popover_ver: u32,
    pub toast: Option<Rect>,
    pub snap: Option<Rect>,
}

impl Default for World {
    fn default() -> Self {
        World {
            wins: Vec::new(),
            ws: 0,
            next_id: 1,
            clock: 0,
            hover: 0,
            popover: None,
            popover_ver: 0,
            toast: None,
            snap: None,
        }
    }
}

fn lerp(a: i32, b: i32, step: u32, steps: u32) -> i32 {
    a + (b - a) * step as i32 / steps.max(1) as i32
}

impl Anim {
    fn rect(&self) -> Rect {
        let (s, n) = (self.step, self.steps);
        Rect::new(
            lerp(self.from.x, self.to.x, s, n),
            lerp(self.from.y, self.to.y, s, n),
            lerp(self.from.w, self.to.w, s, n).max(1),
            lerp(self.from.h, self.to.h, s, n).max(1),
        )
    }

    fn alpha(&self) -> u32 {
        lerp(self.a_from as i32, self.a_to as i32, self.step, self.steps) as u32
    }
}

impl Win {
    fn view(&self, focused: bool) -> WinView {
        let (rect, alpha) = match &self.anim {
            Some(a) => (a.rect(), a.alpha()),
            None => (self.rect, 255),
        };
        WinView {
            id: self.id,
            rect,
            alpha,
            ver: self.ver,
            part: self.part,
            focused,
            maximized: self.maximized && self.anim.is_none(),
            shadow: !(self.maximized && self.anim.is_none()),
        }
    }

    fn on_screen(&self, ws: u8) -> bool {
        self.ws == ws && (!self.minimized || self.anim.is_some())
    }

    fn leaving(&self) -> bool {
        matches!(&self.anim, Some(a) if a.end != End::Stay)
    }
}

impl World {
    /// The window that has the focus: the topmost one on this workspace that is not on its way out.
    pub fn focus_id(&self) -> u32 {
        self.wins
            .iter()
            .rev()
            .find(|w| w.on_screen(self.ws) && !w.leaving())
            .map_or(0, |w| w.id)
    }

    pub fn view_of(&self, id: u32) -> Option<WinView> {
        let focus = self.focus_id();
        self.wins
            .iter()
            .find(|w| w.id == id && w.on_screen(self.ws))
            .map(|w| w.view(w.id == focus))
    }

    fn open_anim(rect: Rect) -> Anim {
        let c = (rect.x + rect.w / 2, rect.y + rect.h / 2);
        Anim {
            from: Rect::new(c.0 - rect.w / 4, c.1 - rect.h / 4, rect.w / 2, rect.h / 2),
            to: rect,
            a_from: 0,
            a_to: 255,
            step: 0,
            steps: 4,
            end: End::Stay,
        }
    }

    pub fn open(&mut self, kind: Kind, rect: Rect) {
        if self.wins.len() >= 24 {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.wins.push(Win {
            id,
            kind,
            rect,
            restore: rect,
            maximized: false,
            minimized: false,
            ws: self.ws,
            ver: 0,
            part: 0,
            part_dirty: false,
            anim: Some(Self::open_anim(rect)),
        });
    }

    pub fn close(&mut self, i: usize) {
        let Some(w) = self.wins.get_mut(i) else {
            return;
        };
        if w.anim.is_some() || !w.on_screen(self.ws) {
            // A hidden window is just dropped; an animating one finishes first.
            if !w.on_screen(self.ws) {
                self.wins.remove(i);
            }
            return;
        }
        let r = w.rect;
        w.anim = Some(Anim {
            from: r,
            to: Rect::new(
                r.x + r.w / 4,
                r.y + r.h / 4,
                (r.w / 2).max(1),
                (r.h / 2).max(1),
            ),
            a_from: 255,
            a_to: 0,
            step: 0,
            steps: 3,
            end: End::Close,
        });
    }

    pub fn move_by(&mut self, i: usize, dx: i32, dy: i32) {
        if let Some(w) = self.wins.get_mut(i)
            && !w.maximized
            && w.anim.is_none()
        {
            w.rect.x += dx;
            w.rect.y += dy;
            w.restore = w.rect;
        }
    }

    pub fn resize_by(&mut self, i: usize, dw: i32, dh: i32) {
        if let Some(w) = self.wins.get_mut(i)
            && !w.maximized
            && w.anim.is_none()
        {
            w.rect.w = (w.rect.w + dw).clamp(24, W);
            w.rect.h = (w.rect.h + dh).clamp(16, H);
            w.restore = w.rect;
        }
    }

    pub fn raise(&mut self, i: usize) {
        if i < self.wins.len() && self.wins[i].on_screen(self.ws) {
            let w = self.wins.remove(i);
            self.wins.push(w);
        }
    }

    pub fn minimize(&mut self, i: usize) {
        let Some(w) = self.wins.get_mut(i) else {
            return;
        };
        if w.anim.is_some() || !w.on_screen(self.ws) {
            return;
        }
        let t = taskbar_rect();
        w.anim = Some(Anim {
            from: w.rect,
            to: Rect::new(t.x + 10, t.y, 12, 8),
            a_from: 255,
            a_to: 40,
            step: 0,
            steps: 4,
            end: End::Minimize,
        });
    }

    /// Bring back the oldest minimised window of this workspace.
    pub fn restore_one(&mut self) {
        let ws = self.ws;
        let Some(i) = self.wins.iter().position(|w| w.ws == ws && w.minimized) else {
            return;
        };
        let mut w = self.wins.remove(i);
        w.minimized = false;
        let t = taskbar_rect();
        w.anim = Some(Anim {
            from: Rect::new(t.x + 10, t.y, 12, 8),
            to: w.rect,
            a_from: 40,
            a_to: 255,
            step: 0,
            steps: 4,
            end: End::Stay,
        });
        self.wins.push(w);
    }

    /// Tile the window to a half or quarter of the work area (zone 0..=5), or maximise (6).
    pub fn snap(&mut self, i: usize, zone: u32) {
        let Some(w) = self.wins.get_mut(i) else {
            return;
        };
        if w.anim.is_some() || !w.on_screen(self.ws) {
            return;
        }
        let a = work_area();
        let (hw, hh) = (a.w / 2, a.h / 2);
        let to = match zone % 7 {
            0 => Rect::new(a.x, a.y, hw, a.h),
            1 => Rect::new(a.x + hw, a.y, a.w - hw, a.h),
            2 => Rect::new(a.x, a.y, hw, hh),
            3 => Rect::new(a.x + hw, a.y, a.w - hw, hh),
            4 => Rect::new(a.x, a.y + hh, hw, a.h - hh),
            5 => Rect::new(a.x + hw, a.y + hh, a.w - hw, a.h - hh),
            _ => a,
        };
        if !w.maximized {
            w.restore = w.rect;
        }
        let from = w.rect;
        w.maximized = zone % 7 == 6;
        w.rect = to;
        w.anim = Some(Anim {
            from,
            to,
            a_from: 255,
            a_to: 255,
            step: 0,
            steps: 3,
            end: End::Stay,
        });
    }

    pub fn unmaximize(&mut self, i: usize) {
        let Some(w) = self.wins.get_mut(i) else {
            return;
        };
        if w.anim.is_some() || !w.maximized {
            return;
        }
        let from = w.rect;
        w.maximized = false;
        w.rect = w.restore;
        w.anim = Some(Anim {
            from,
            to: w.rect,
            a_from: 255,
            a_to: 255,
            step: 0,
            steps: 3,
            end: End::Stay,
        });
    }

    pub fn switch_ws(&mut self, n: u8) {
        self.ws = n % 3;
    }

    pub fn send_to_ws(&mut self, i: usize, n: u8) {
        if let Some(w) = self.wins.get_mut(i) {
            w.ws = n % 3;
            self.ws = n % 3;
            self.raise(i);
        }
    }

    /// Advance every animation one step; finished ones take effect.
    pub fn tick(&mut self) {
        let mut closed = Vec::new();
        for w in &mut self.wins {
            let Some(a) = &mut w.anim else { continue };
            a.step += 1;
            if a.step >= a.steps {
                match a.end {
                    End::Stay => {}
                    End::Close => closed.push(w.id),
                    End::Minimize => w.minimized = true,
                }
                w.anim = None;
            }
        }
        self.wins.retain(|w| !closed.contains(&w.id));
    }

    /// The live windows' charts and the game windows' content move on by themselves.
    pub fn live_tick(&mut self) {
        for w in &mut self.wins {
            match w.kind {
                Kind::Live => {
                    w.part += 1;
                    w.part_dirty = true;
                }
                Kind::Game => w.ver += 1,
                Kind::Plain => {}
            }
        }
    }

    /// The focused window's text area changes (a typed character): a partial update.
    pub fn edit(&mut self) {
        let f = self.focus_id();
        if let Some(w) = self.wins.iter_mut().find(|w| w.id == f) {
            w.part += 1;
            w.part_dirty = true;
        }
    }

    /// Something about window `i` changed everywhere (a title, a toggle).
    pub fn repaint(&mut self, i: usize) {
        if let Some(w) = self.wins.get_mut(i) {
            w.ver += 1;
        }
    }

    /// Build the scene. `dirty` flags are consumed: call once per plan.
    pub fn scene(&mut self, faults: &super::Faults) -> Scene {
        let mut s = Scene::new();
        let focus = self.focus_id();
        let ws = self.ws;
        for w in self.wins.iter_mut().filter(|w| w.on_screen(ws)) {
            let v = w.view(w.id == focus);
            let footprint = if faults.footprint_without_shadow {
                v.rect
            } else {
                v.paint_extent()
            };
            let opaque = if v.alpha < 255 {
                Rect::new(0, 0, 0, 0)
            } else if faults.opaque_whole_rect || v.maximized {
                v.rect
            } else {
                Rect::new(
                    v.rect.x,
                    v.rect.y + RADIUS,
                    v.rect.w,
                    (v.rect.h - 2 * RADIUS).max(0),
                )
            };
            let look = if faults.forget_look {
                0
            } else {
                u64::from(v.ver)
                    | (u64::from(v.focused) << 32)
                    | (u64::from(v.alpha) << 40)
                    | (u64::from(v.maximized) << 52)
            };
            let chart = Rect::new(v.rect.x + 4, v.rect.y + v.rect.h - 14, v.rect.w - 8, 10);
            let dirty = (w.part_dirty && !faults.forget_dirty).then_some(chart);
            w.part_dirty = false;
            s.push(
                Layer::new(LayerId(w.id), footprint)
                    .with_opaque(opaque)
                    .with_look(look)
                    .with_dirty(dirty),
            );
        }
        let bar = taskbar_rect();
        s.push(Layer::new(ids::TASKBAR, shadow_extent(&bar)).with_look(u64::from(self.hover)));
        s.push(
            Layer::new(ids::PANEL, Rect::new(0, 0, W, PANEL_H))
                .with_opaque(Rect::new(0, 0, W, PANEL_H))
                .with_look(u64::from(focus)),
        );
        if let Some(r) = self.popover {
            s.push(
                Layer::new(ids::POPOVER, shadow_extent(&r)).with_look(u64::from(self.popover_ver)),
            );
        }
        if let Some(r) = self.toast {
            s.push(Layer::new(ids::TOAST, r));
        }
        if let Some(r) = self.snap {
            s.push(Layer::new(ids::SNAP, r));
        }
        s
    }
}
