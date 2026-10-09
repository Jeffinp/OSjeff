//! Bookkeeping for a software mouse cursor painted straight into the framebuffer.
//!
//! The compositor composes the scene in a back buffer, uploads rectangles of it to the
//! framebuffer, and paints the cursor sprite *over the framebuffer only* (never into the back
//! buffer, so moving the pointer does not recompose anything). That gives one invariant to keep:
//!
//! > **Every pixel the cursor painted must be restored from the back buffer before the next
//! > frame changes anything, and the cursor is painted again last.**
//!
//! Earlier the restore lived in each render path ("blit the old sprite box from `back`, then
//! draw"), and the paths that did not move the cursor but did upload rectangles (a hover
//! change, a click, a keystroke) forgot it: the old sprite stayed on screen, a trail of arrow
//! fragments. [`CursorTrack`] makes the order impossible to get wrong:
//!
//! 1. [`CursorTrack::erase`] at the **start** of every frame that renders anything: returns the
//!    screen rectangle where the sprite is currently painted (clipped to the screen), which the
//!    caller restores from the back buffer. After that the framebuffer holds no cursor pixel,
//!    whatever the frame does next (full blit, damage rectangles, an animation, several mouse
//!    packets folded into one frame).
//! 2. The frame's own uploads, toasts and HUD.
//! 3. [`CursorTrack::paint`] at the **end**: records where the sprite goes and says where it
//!    must be drawn. It always follows an `erase`, so the sprite is never painted twice at two
//!    places.
//!
//! The pure model (clipping, the erase/paint protocol) is here so it is unit-tested on
//! the host with a simulated framebuffer; the kernel only supplies the pixels.

use crate::window::Rect;

/// What the cursor looks like and where it is: the part of the pointer state that decides the
/// pixels. A shape change without motion (the pointer enters a link) is a change too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    /// Sprite id (kernel-defined: arrow, hand...).
    pub shape: u8,
}

/// Where the sprite is currently painted in the framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorTrack {
    /// Sprite bounding box (every sprite fits in it, hot spot at its top left).
    w: i32,
    h: i32,
    painted: Option<Pointer>,
}

impl CursorTrack {
    /// A tracker for sprites that fit in `w` x `h` pixels, nothing painted yet.
    pub const fn new(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            painted: None,
        }
    }

    /// The pointer state painted on screen, if any.
    pub const fn painted(&self) -> Option<Pointer> {
        self.painted
    }

    /// The box of the sprite for `p`, clipped to the screen.
    pub fn sprite_box(&self, p: Pointer, width: i32, height: i32) -> Rect {
        // Saturating: a pointer coordinate is trusted to be on screen, but this must stay total.
        let (x0, y0) = (p.x.max(0), p.y.max(0));
        let x1 = p.x.saturating_add(self.w).min(width);
        let y1 = p.y.saturating_add(self.h).min(height);
        if x1 <= x0 || y1 <= y0 {
            return Rect::new(0, 0, 0, 0);
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Does the screen show something other than `now`? (Never painted counts as yes.)
    pub fn is_stale(&self, now: Pointer) -> bool {
        self.painted != Some(now)
    }

    /// Step 1 of a frame: the rectangle to restore from the back buffer so the framebuffer holds
    /// no cursor pixel (`None` when nothing is painted). Forgets the painted sprite: a frame that
    /// calls this must end with [`CursorTrack::paint`].
    pub fn erase(&mut self, width: i32, height: i32) -> Option<Rect> {
        let old = self.painted.take()?;
        let r = self.sprite_box(old, width, height);
        (!r.is_empty()).then_some(r)
    }

    /// Step 3 of a frame: record that the sprite for `now` is about to be drawn and return its
    /// (clipped) box. Draw the sprite with the framebuffer's own clipping.
    pub fn paint(&mut self, now: Pointer, width: i32, height: i32) -> Rect {
        self.painted = Some(now);
        self.sprite_box(now, width, height)
    }

    /// The part of the screen this frame must refresh because of the cursor: the old sprite box
    /// united with the new one (what a rectangle-only uploader has to cover).
    pub fn damage(&self, now: Pointer, width: i32, height: i32) -> Rect {
        let new = self.sprite_box(now, width, height);
        match self.painted {
            Some(old) => self.sprite_box(old, width, height).union(&new),
            None => new,
        }
    }
}

#[cfg(test)]
mod tests;
