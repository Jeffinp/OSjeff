use super::*;
use crate::window::Rect;
use alloc::vec;
use alloc::vec::Vec;

const SW: i32 = 40;
const SH: i32 = 30;
const BW: i32 = 10;
const BH: i32 = 16;
const HUD: Rect = Rect::new(24, 0, 16, 8);
const HUD_PX: u32 = 0x00AA_0000;

// ---- the erase / paint protocol on a simulated framebuffer ----

/// Does the sprite of `p` cover the pixel at (`x`, `y`)? An arbitrary but fixed pattern that
/// uses the whole box, so a missed restore anywhere shows.
fn sprite_px(p: Pointer, x: i32, y: i32) -> bool {
    let (dx, dy) = (x - p.x, y - p.y);
    (0..BW).contains(&dx) && (0..BH).contains(&dy) && (dx + dy + i32::from(p.shape)) % 2 == 0
}

fn cursor_value(p: Pointer) -> u32 {
    0xFF00_0000 | u32::from(p.shape)
}

struct Sim {
    fb: Vec<u32>,
    back: Vec<u32>,
    track: CursorTrack,
    ptr: Pointer,
    hud_drawn: bool,
    seed: u64,
}

impl Sim {
    fn new(seed: u64) -> Sim {
        let n = (SW * SH) as usize;
        Sim {
            fb: vec![1; n],
            back: vec![1; n],
            track: CursorTrack::new(BW, BH),
            ptr: Pointer {
                x: SW / 2,
                y: SH / 2,
                shape: 0,
            },
            hud_drawn: false,
            seed: seed | 1,
        }
    }

    fn rnd(&mut self, n: u64) -> i32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed % n) as i32
    }

    fn idx(x: i32, y: i32) -> usize {
        (y * SW + x) as usize
    }

    /// back -> fb for `r` (clipped), like the kernel's `blit_rect`.
    fn upload(&mut self, r: Rect) {
        let r = r.clamped_to(SW, SH);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.fb[Self::idx(x, y)] = self.back[Self::idx(x, y)];
            }
        }
    }

    fn draw_hud(&mut self) {
        for y in HUD.y..HUD.bottom() {
            for x in HUD.x..HUD.right() {
                self.fb[Self::idx(x, y)] = HUD_PX;
            }
        }
        self.hud_drawn = true;
    }

    fn draw_sprite(&mut self, p: Pointer) {
        for y in 0..SH {
            for x in 0..SW {
                if sprite_px(p, x, y) {
                    self.fb[Self::idx(x, y)] = cursor_value(p);
                }
            }
        }
    }

    /// The scene a frame changes: pixels of `back` get a new value inside `r`, and the usual
    /// uploader copies exactly `r` (the steady path uploads only what changed).
    fn change_scene(&mut self, r: Rect, v: u32) {
        let c = r.clamped_to(SW, SH);
        for y in c.y..c.bottom() {
            for x in c.x..c.right() {
                self.back[Self::idx(x, y)] = v;
            }
        }
        self.upload(r);
    }

    fn random_rect(&mut self) -> Rect {
        Rect::new(
            self.rnd(60) - 10,
            self.rnd(50) - 10,
            self.rnd(30),
            self.rnd(30),
        )
    }

    /// One frame with the documented protocol: erase first, anything in the middle, paint last.
    fn frame(&mut self, tick: u32) {
        // several mouse packets between two frames; the cursor may also be clamped or not move
        for _ in 0..self.rnd(5) {
            let dx = self.rnd(41) - 20;
            let dy = self.rnd(41) - 20;
            self.ptr.x = (self.ptr.x + dx).clamp(0, SW - 1);
            self.ptr.y = (self.ptr.y + dy).clamp(0, SH - 1);
        }
        if self.rnd(8) == 0 {
            self.ptr.shape ^= 1; // arrow <-> hand without moving
        }

        let mut hud_dirty = tick.is_multiple_of(7);
        // 1. erase: restore the sprite box from the back buffer.
        if let Some(r) = self.track.erase(SW, SH) {
            self.upload(r);
            // the HUD is drawn straight into the framebuffer: restoring under it wiped it
            hud_dirty |= r.intersection(&HUD).is_some();
        }
        // 2. the frame's own uploads
        for _ in 0..self.rnd(4) {
            let r = self.random_rect();
            self.change_scene(r, 100 + tick);
            // uploads that cover the HUD repaint it (the kernel redraws it on its own timer)
            hud_dirty |= r.intersection(&HUD).is_some();
        }
        if self.rnd(10) == 0 {
            self.upload(Rect::new(0, 0, SW, SH));
            hud_dirty = true;
        }
        if hud_dirty || !self.hud_drawn {
            self.draw_hud();
        }
        // 3. paint last
        let p = self.ptr;
        self.track.paint(p, SW, SH);
        self.draw_sprite(p);
    }

    /// Every pixel is the scene (or the HUD), except exactly the sprite's.
    fn check(&self) {
        for y in 0..SH {
            for x in 0..SW {
                let got = self.fb[Self::idx(x, y)];
                let want = if sprite_px(self.ptr, x, y) {
                    cursor_value(self.ptr)
                } else if HUD.intersection(&Rect::new(x, y, 1, 1)).is_some() {
                    HUD_PX
                } else {
                    self.back[Self::idx(x, y)]
                };
                assert_eq!(
                    got, want,
                    "pixel ({x},{y}) after the frame, ptr {:?}",
                    self.ptr
                );
            }
        }
    }
}

#[test]
fn erase_then_paint_leaves_no_stale_sprite() {
    for seed in 1..200u64 {
        let mut s = Sim::new(seed * 0x9E37_79B9);
        for tick in 0..120 {
            s.frame(tick);
            s.check();
        }
    }
}

#[test]
fn nothing_painted_means_nothing_to_erase() {
    let mut t = CursorTrack::new(BW, BH);
    assert_eq!(t.erase(SW, SH), None);
    let p = Pointer {
        x: 3,
        y: 4,
        shape: 0,
    };
    assert!(t.is_stale(p));
    assert_eq!(t.paint(p, SW, SH), Rect::new(3, 4, BW, BH));
    assert!(!t.is_stale(p));
    assert_eq!(t.erase(SW, SH), Some(Rect::new(3, 4, BW, BH)));
    // erasing forgets: a second erase has nothing left to restore
    assert_eq!(t.erase(SW, SH), None);
}

#[test]
fn shape_change_without_motion_is_stale() {
    let mut t = CursorTrack::new(BW, BH);
    let arrow = Pointer {
        x: 9,
        y: 9,
        shape: 0,
    };
    t.paint(arrow, SW, SH);
    assert!(!t.is_stale(arrow));
    assert!(t.is_stale(Pointer { shape: 1, ..arrow }));
    assert!(t.is_stale(Pointer { x: 10, ..arrow }));
}

#[test]
fn erase_box_is_clipped_at_the_screen_edges() {
    let mut t = CursorTrack::new(BW, BH);
    t.paint(
        Pointer {
            x: SW - 1,
            y: SH - 1,
            shape: 0,
        },
        SW,
        SH,
    );
    assert_eq!(t.erase(SW, SH), Some(Rect::new(SW - 1, SH - 1, 1, 1)));
    t.paint(
        Pointer {
            x: 0,
            y: 0,
            shape: 0,
        },
        SW,
        SH,
    );
    assert_eq!(t.erase(SW, SH), Some(Rect::new(0, 0, BW, BH)));
    // a pointer outside the screen (cannot happen, but must not index out of range)
    t.paint(
        Pointer {
            x: SW + 5,
            y: 0,
            shape: 0,
        },
        SW,
        SH,
    );
    assert_eq!(t.erase(SW, SH), None);
}

#[test]
fn damage_is_the_union_of_old_and_new() {
    let mut t = CursorTrack::new(BW, BH);
    let a = Pointer {
        x: 2,
        y: 2,
        shape: 0,
    };
    let b = Pointer {
        x: 20,
        y: 10,
        shape: 0,
    };
    assert_eq!(t.damage(b, SW, SH), Rect::new(20, 10, BW, BH));
    t.paint(a, SW, SH);
    assert_eq!(t.damage(b, SW, SH), Rect::new(2, 2, 28, 24));
    assert_eq!(t.damage(a, SW, SH), Rect::new(2, 2, BW, BH));
}

/// The flow the compositor had before: the old sprite is restored only on the paths that
/// handle `cursor_moved`; a frame that uploads other rectangles (a hover change, a click, a
/// keystroke) just paints the cursor at its new place. The same simulation shows the ghosts, so
/// the property test above is able to see this bug.
#[test]
fn the_old_per_path_restore_leaves_ghosts() {
    let mut ghosts = 0;
    for seed in 1..50u64 {
        let mut s = Sim::new(seed * 0x9E37_79B9);
        let mut prev = s.ptr;
        s.draw_sprite(prev);
        for tick in 0..60 {
            let r = s.random_rect();
            let moved = s.rnd(2) == 0;
            if moved {
                s.ptr.x = (s.ptr.x + s.rnd(61) - 30).clamp(0, SW - 1);
                s.ptr.y = (s.ptr.y + s.rnd(61) - 30).clamp(0, SH - 1);
            }
            let scene_dirty = s.rnd(2) == 0;
            if scene_dirty {
                // steady path: uploads its rectangles, paints the cursor, no restore
                s.change_scene(r, 100 + tick);
            } else if moved {
                // cursor path: restores the old box
                let b = s.track.sprite_box(prev, SW, SH);
                s.upload(b);
            }
            s.draw_sprite(s.ptr);
            prev = s.ptr;
            for y in 0..SH {
                for x in 0..SW {
                    let want = if sprite_px(s.ptr, x, y) {
                        cursor_value(s.ptr)
                    } else {
                        s.back[Sim::idx(x, y)]
                    };
                    if s.fb[Sim::idx(x, y)] != want {
                        ghosts += 1;
                    }
                }
            }
        }
    }
    assert!(ghosts > 0, "the legacy flow was expected to leave ghosts");
}

#[test]
fn hostile_pointer_coordinates_do_not_overflow() {
    let mut t = CursorTrack::new(BW, BH);
    for (x, y) in [
        (i32::MAX, i32::MAX),
        (i32::MIN, i32::MIN),
        (i32::MAX - 3, 0),
        (-5, -5),
    ] {
        let p = Pointer { x, y, shape: 0 };
        let _ = t.damage(p, SW, SH);
        let b = t.paint(p, SW, SH);
        assert!(b.x >= 0 && b.y >= 0 && b.right() <= SW && b.bottom() <= SH);
        let _ = t.erase(SW, SH);
    }
    // partly off the top left: only the on-screen part is restored
    t.paint(
        Pointer {
            x: -4,
            y: -3,
            shape: 0,
        },
        SW,
        SH,
    );
    assert_eq!(t.erase(SW, SH), Some(Rect::new(0, 0, BW - 4, BH - 3)));
}
