//! PS/2 byte-stream decoding: set-1 scancodes and 3- or 4-byte mouse packets.
//!
//! The kernel's IRQ handlers only push raw bytes into a ring; [`Decoder`] turns
//! that byte stream into [`Event`]s. It is a pair of tiny state machines (the
//! `0xE0` extended-key prefix and the mouse packet cycle).
//!
//! # Wheel mouse (IntelliMouse)
//!
//! A plain PS/2 mouse sends 3-byte packets (`flags, dx, dy`). The *IntelliMouse*
//! extension adds a 4th byte with the wheel delta `dz`. It is negotiated by
//! setting the sample rate to 200, 100 and 80 (the "magic" sequence,
//! [`WHEEL_MAGIC`]) and then reading the device id (`0xF2`): a mouse that took
//! the sequence answers `3`, any other answers `0` (and must keep sending
//! 3-byte packets). [`MouseMode::from_id`] maps the answer and
//! [`Decoder::set_mode`] switches the packet size. The kernel does the port
//! I/O; everything that can be decided without hardware is here.
//!
//! Sign convention: a positive `dz` is "wheel toward the user" (scroll down),
//! as QEMU and real mice report it.

/// The sample-rate sequence that unlocks the wheel (`0xF3 <rate>` for each).
pub const WHEEL_MAGIC: [u8; 3] = [200, 100, 80];

/// Largest wheel magnitude a packet reports (the byte is a signed 8-bit value;
/// `-128` saturates to `-127` so the range is symmetric).
pub const WHEEL_MAX: i32 = 127;

/// Largest horizontal / vertical magnitude when the hardware flags an overflow.
pub const MOTION_MAX: i32 = 255;

/// How the attached mouse frames its packets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseMode {
    /// 3-byte packets (device id 0, or anything not understood).
    #[default]
    Basic,
    /// 4-byte IntelliMouse packets with the wheel axis (device id 3).
    Wheel,
}

impl MouseMode {
    /// The mode for the answer to "get device id" after the magic sequence.
    /// `None` (no answer in time) and every id other than 3 mean [`Basic`]:
    /// id 4 (Explorer, 5 buttons) is not negotiated, and a mouse that kept
    /// answering 0 simply has no wheel.
    ///
    /// [`Basic`]: MouseMode::Basic
    pub fn from_id(id: Option<u8>) -> Self {
        match id {
            Some(3) => MouseMode::Wheel,
            _ => MouseMode::Basic,
        }
    }

    /// Bytes per packet in this mode.
    pub const fn packet_len(self) -> u8 {
        match self {
            MouseMode::Basic => 3,
            MouseMode::Wheel => 4,
        }
    }
}

/// A decoded mouse packet (relative motion, wheel and button state).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Packet {
    pub dx: i32,
    pub dy: i32,
    /// Wheel delta, positive = toward the user (scroll down). Always `0` for a
    /// mouse in [`MouseMode::Basic`]. Within `-WHEEL_MAX..=WHEEL_MAX`.
    pub dz: i32,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

/// A decoded key press or release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub scan_code: u8,
    pub pressed: bool,
    /// Set when the scancode followed an `0xE0` prefix (arrows, Del, etc.).
    pub extended: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Mouse(Packet),
    Key(KeyEvent),
}

/// Decode one motion byte. With the overflow flag the 9-bit value cannot be
/// trusted, so it saturates in the direction of its sign.
fn motion(raw: u8, negative: bool, overflow: bool) -> i32 {
    if overflow {
        return if negative { -MOTION_MAX } else { MOTION_MAX };
    }
    let v = raw as i32;
    if negative { v - 256 } else { v }
}

/// Decode the 4th byte of an IntelliMouse (id 3) packet: a signed 8-bit
/// delta, `-128` saturating to `-127`.
pub fn wheel_delta(byte: u8) -> i32 {
    (byte as i8 as i32).max(-WHEEL_MAX)
}

/// Incremental decoder state for the scancode / mouse-packet state machines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decoder {
    mouse_cycle: u8,
    mouse_buf: [u8; 4],
    mode: MouseMode,
    key_extended: bool,
}

impl Decoder {
    pub const fn new() -> Self {
        Self {
            mouse_cycle: 0,
            mouse_buf: [0; 4],
            mode: MouseMode::Basic,
            key_extended: false,
        }
    }

    /// The current mouse framing.
    pub const fn mode(&self) -> MouseMode {
        self.mode
    }

    /// Switch the packet framing (after the wheel negotiation). A packet in
    /// progress is dropped: its bytes belong to the old framing.
    pub fn set_mode(&mut self, mode: MouseMode) {
        self.mode = mode;
        self.mouse_cycle = 0;
    }

    /// Feed one keyboard byte. `None` for a prefix byte (`0xE0`); the real
    /// scancode arrives next and picks up the `extended` flag.
    pub fn keyboard(&mut self, data: u8) -> Option<Event> {
        if data == 0xE0 {
            self.key_extended = true;
            return None;
        }
        let extended = self.key_extended;
        self.key_extended = false;
        Some(Event::Key(KeyEvent {
            scan_code: data & 0x7F,
            pressed: data & 0x80 == 0,
            extended,
        }))
    }

    /// Feed one mouse byte. Emits a [`Packet`] on the last byte of each packet
    /// (the third, or the fourth in [`MouseMode::Wheel`]). A first byte with
    /// bit 3 clear is treated as out of sync and dropped, so a lost byte costs
    /// at most the packet it belonged to.
    pub fn mouse(&mut self, data: u8) -> Option<Event> {
        let len = self.mode.packet_len();
        if self.mouse_cycle == 0 {
            if data & 0x08 == 0 {
                return None; // out of sync; bit3 of byte0 is always 1
            }
            self.mouse_buf[0] = data;
            self.mouse_cycle = 1;
            return None;
        }
        self.mouse_buf[self.mouse_cycle as usize] = data;
        self.mouse_cycle += 1;
        if self.mouse_cycle < len {
            return None;
        }
        self.mouse_cycle = 0;
        let flags = self.mouse_buf[0];
        let dz = if self.mode == MouseMode::Wheel {
            wheel_delta(self.mouse_buf[3])
        } else {
            0
        };
        Some(Event::Mouse(Packet {
            dx: motion(self.mouse_buf[1], flags & 0x10 != 0, flags & 0x40 != 0),
            dy: motion(self.mouse_buf[2], flags & 0x20 != 0, flags & 0x80 != 0),
            dz,
            left: flags & 0x01 != 0,
            right: flags & 0x02 != 0,
            middle: flags & 0x04 != 0,
        }))
    }
}

#[cfg(test)]
mod tests;
