//! Operations on the simulated desktop and the sources that choose them.

use super::model::{Kind, World};
use super::paint::{H, PANEL_H, W};
use crate::window::Rect;

/// Choose values: a seeded generator in the tests, the fuzz input in the fuzz target.
pub trait Source {
    fn next_u32(&mut self) -> u32;

    /// A value below `n` (0 when `n` is 0).
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }

    /// A value in `lo..=hi`.
    fn between(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1).max(1) as u32) as i32
    }
}

/// SplitMix64: tiny, fast, good enough for test inputs, fully reproducible from the seed.
pub struct SplitMix(pub u64);

impl Source for SplitMix {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as u32
    }
}

/// Values read from a byte string (the fuzz input); zero once it runs out.
pub struct Bytes<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Bytes { data, pos: 0 }
    }

    pub fn exhausted(&self) -> bool {
        self.pos >= self.data.len()
    }
}

impl Source for Bytes<'_> {
    fn next_u32(&mut self) -> u32 {
        let mut v = 0u32;
        for _ in 0..2 {
            v = (v << 8) | u32::from(self.data.get(self.pos).copied().unwrap_or(0));
            self.pos += 1;
        }
        v
    }
}

/// One thing that can happen to the desktop between two frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Open(Kind, Rect),
    Close(usize),
    Move(usize, i32, i32),
    Resize(usize, i32, i32),
    Raise(usize),
    Minimize(usize),
    Restore,
    Snap(usize, u32),
    Unmaximize(usize),
    Workspace(u8),
    SendToWorkspace(usize, u8),
    Popover(Option<Rect>),
    PopoverRepaint,
    Toast(Option<Rect>),
    SnapPreview(Option<Rect>),
    Hover(u32),
    ClockTick,
    /// Advance the window animations one step.
    Tick,
    /// Live charts and games advance.
    LiveTick,
    /// A character is typed in the focused window.
    Edit,
    /// A window's content changed everywhere.
    Repaint(usize),
    /// Nothing happens: the plan must be empty.
    Idle,
}

fn rect_in(s: &mut impl Source, min_w: i32, max_w: i32, min_h: i32, max_h: i32) -> Rect {
    let w = s.between(min_w, max_w);
    let h = s.between(min_h, max_h);
    // Windows may hang off any edge of the screen.
    let x = s.between(-w / 2, W - w / 2);
    let y = s.between(PANEL_H - 4, H - h / 2);
    Rect::new(x, y, w, h)
}

/// Pick a random operation with weights that keep a healthy number of windows alive.
pub fn random_op(world: &World, s: &mut impl Source) -> Op {
    let n = world.wins.len() as u32;
    let i = s.below(n) as usize;
    let roll = s.below(100);
    match roll {
        0..=7 => {
            let kind = match s.below(6) {
                0 => Kind::Live,
                1 => Kind::Game,
                _ => Kind::Plain,
            };
            Op::Open(kind, rect_in(s, 30, 110, 22, 80))
        }
        8..=11 => Op::Close(i),
        12..=24 => Op::Move(i, s.between(-12, 12), s.between(-12, 12)),
        25..=28 => Op::Move(i, s.between(-1, 1), s.between(-1, 1)),
        29..=33 => Op::Resize(i, s.between(-14, 14), s.between(-14, 14)),
        34..=40 => Op::Raise(i),
        41..=43 => Op::Minimize(i),
        44..=47 => Op::Restore,
        48..=51 => Op::Snap(i, s.below(7)),
        52..=53 => Op::Unmaximize(i),
        54..=55 => Op::Workspace(s.below(3) as u8),
        56..=57 => Op::SendToWorkspace(i, s.below(3) as u8),
        58..=59 => Op::Popover(s.below(3).ne(&0).then(|| rect_in(s, 24, 70, 16, 50))),
        60 => Op::PopoverRepaint,
        61..=62 => Op::Toast(s.below(3).ne(&0).then(|| rect_in(s, 30, 60, 8, 18))),
        63..=64 => Op::SnapPreview(s.below(3).ne(&0).then(|| rect_in(s, 40, 100, 30, 90))),
        65..=67 => Op::Hover(s.below(5)),
        68..=70 => Op::ClockTick,
        71..=82 => Op::Tick,
        83..=90 => Op::LiveTick,
        91..=95 => Op::Edit,
        96 => Op::Repaint(i),
        _ => Op::Idle,
    }
}

/// Apply `op` to `world`. `ClockTick` returns the rectangle to invalidate explicitly.
pub fn apply(world: &mut World, op: Op) -> Option<Rect> {
    match op {
        Op::Open(k, r) => world.open(k, r),
        Op::Close(i) => world.close(i),
        Op::Move(i, dx, dy) => world.move_by(i, dx, dy),
        Op::Resize(i, dw, dh) => world.resize_by(i, dw, dh),
        Op::Raise(i) => world.raise(i),
        Op::Minimize(i) => world.minimize(i),
        Op::Restore => world.restore_one(),
        Op::Snap(i, z) => world.snap(i, z),
        Op::Unmaximize(i) => world.unmaximize(i),
        Op::Workspace(n) => world.switch_ws(n),
        Op::SendToWorkspace(i, n) => world.send_to_ws(i, n),
        Op::Popover(r) => world.popover = r,
        Op::PopoverRepaint => world.popover_ver += 1,
        Op::Toast(r) => world.toast = r,
        Op::SnapPreview(r) => world.snap = r,
        Op::Hover(h) => world.hover = h,
        Op::ClockTick => {
            world.clock += 1;
            return Some(super::model::clock_rect());
        }
        Op::Tick => world.tick(),
        Op::LiveTick => world.live_tick(),
        Op::Edit => world.edit(),
        Op::Repaint(i) => world.repaint(i),
        Op::Idle => {}
    }
    None
}
