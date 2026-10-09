//! virtio-net: the pure half of the driver (feature bits, the packet header, the
//! split-virtqueue layout and index arithmetic, buffer-slot bookkeeping).
//!
//! The kernel's `virtio_net.rs` owns the DMA memory and the MMIO accesses; every
//! decision about *what to write where* and *how to read what the device wrote*
//! is here so it can be tested on the host. Reference: virtio 1.0 spec section
//! 2.4 (split virtqueues) and 5.1 (network device).
//!
//! Device model used: two queues (0 = receiveq, 1 = transmitq), no control queue,
//! no offloads, no MSI-X, polled. Because `VIRTIO_F_VERSION_1` is negotiated, every
//! packet carries the 12-byte header (with `num_buffers`) in front of the frame.

use crate::network::net::Mac;

/// Feature bits in the low 32-bit feature word.
pub const F_MAC: u32 = 1 << 5;
pub const F_STATUS: u32 = 1 << 16;
/// What the driver asks for: the device's MAC and its link status. Everything
/// else (checksum/GSO offloads, mergeable buffers, multiqueue) stays off, which
/// is also what lets the receive path assume one descriptor per frame.
pub const WANTED_FEATURES_LO: u32 = F_MAC | F_STATUS;

/// Queue indexes.
pub const RX_QUEUE: u16 = 0;
pub const TX_QUEUE: u16 = 1;

/// Size of `struct virtio_net_hdr` with `num_buffers` (always, under VERSION_1).
pub const HDR_LEN: usize = 12;
/// Largest Ethernet frame we send or accept (MTU 1500 + 14).
pub const MAX_FRAME: usize = 1514;
// A full frame plus the header fits one buffer, and buffers tile a 4 KiB page, so
// each is a single physical range for the device.
const _: () = assert!(HDR_LEN + MAX_FRAME <= BUF_LEN);
const _: () = assert!(4096 % BUF_LEN == 0);

/// Smallest frame on the wire without FCS (shorter ones are padded).
pub const MIN_FRAME: usize = 60;
/// Bytes of one DMA buffer: header + a full frame fits with room to spare, and
/// 2048-byte buffers never straddle a 4 KiB page, so each is one physical range.
pub const BUF_LEN: usize = 2048;

/// Device config `status` word: link is up.
pub const STATUS_LINK_UP: u16 = 1;

/// Descriptor flags.
pub const DESC_F_NEXT: u16 = 1;
pub const DESC_F_WRITE: u16 = 2;

/// `struct virtio_net_hdr` (virtio 1.0, with `num_buffers`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NetHdr {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
    pub num_buffers: u16,
}

impl NetHdr {
    /// A header for a plain frame: no checksum offload, no segmentation.
    pub const fn plain() -> NetHdr {
        NetHdr {
            flags: 0,
            gso_type: 0,
            hdr_len: 0,
            gso_size: 0,
            csum_start: 0,
            csum_offset: 0,
            num_buffers: 0,
        }
    }

    /// Write the header (little endian) into `out[..HDR_LEN]`. `false` if `out`
    /// is too short.
    pub fn encode(&self, out: &mut [u8]) -> bool {
        let Some(o) = out.get_mut(..HDR_LEN) else {
            return false;
        };
        o[0] = self.flags;
        o[1] = self.gso_type;
        o[2..4].copy_from_slice(&self.hdr_len.to_le_bytes());
        o[4..6].copy_from_slice(&self.gso_size.to_le_bytes());
        o[6..8].copy_from_slice(&self.csum_start.to_le_bytes());
        o[8..10].copy_from_slice(&self.csum_offset.to_le_bytes());
        o[10..12].copy_from_slice(&self.num_buffers.to_le_bytes());
        true
    }

    /// Read a header from the start of `b`.
    pub fn decode(b: &[u8]) -> Option<NetHdr> {
        let b = b.get(..HDR_LEN)?;
        let w = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        Some(NetHdr {
            flags: b[0],
            gso_type: b[1],
            hdr_len: w(2),
            gso_size: w(4),
            csum_start: w(6),
            csum_offset: w(8),
            num_buffers: w(10),
        })
    }
}

/// Byte offsets of the three parts of a split virtqueue inside one memory block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueueLayout {
    pub qsize: u16,
    /// Descriptor table: `qsize * 16` bytes, 16-aligned.
    pub desc: usize,
    /// Available ring: `flags, idx, ring[qsize], used_event`, 2-aligned.
    pub avail: usize,
    /// Used ring: `flags, idx, ring[qsize] of {id, len}, avail_event`, 4-aligned.
    pub used: usize,
    /// Total bytes needed.
    pub total: usize,
}

const fn align_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

impl QueueLayout {
    /// Layout for a queue of `qsize` entries (a power of two, 1..=32768, as the
    /// spec requires), or `None`.
    pub fn new(qsize: u16) -> Option<QueueLayout> {
        if qsize == 0 || !qsize.is_power_of_two() || qsize > 32768 {
            return None;
        }
        let q = usize::from(qsize);
        let desc = 0;
        let avail = desc + 16 * q;
        let avail_end = avail + 4 + 2 * q + 2;
        let used = align_up(avail_end, 4);
        let total = used + 4 + 8 * q + 2;
        Some(QueueLayout {
            qsize,
            desc,
            avail,
            used,
            total,
        })
    }

    /// Offset of descriptor `i`.
    pub fn desc_off(&self, i: u16) -> usize {
        self.desc + 16 * usize::from(i % self.qsize)
    }

    /// Offset of `avail.idx`.
    pub fn avail_idx_off(&self) -> usize {
        self.avail + 2
    }

    /// Offset of `avail.ring[slot]` (`slot` is taken modulo the queue size).
    pub fn avail_ring_off(&self, slot: u16) -> usize {
        self.avail + 4 + 2 * usize::from(slot % self.qsize)
    }

    /// Offset of `used.idx`.
    pub fn used_idx_off(&self) -> usize {
        self.used + 2
    }

    /// Offset of `used.ring[slot]` (an `{id: u32, len: u32}` element).
    pub fn used_elem_off(&self, slot: u16) -> usize {
        self.used + 4 + 8 * usize::from(slot % self.qsize)
    }
}

/// Encode a descriptor `{addr: u64, len: u32, flags: u16, next: u16}`.
pub fn encode_desc(addr: u64, len: u32, flags: u16, next: u16) -> [u8; 16] {
    let mut d = [0u8; 16];
    d[0..8].copy_from_slice(&addr.to_le_bytes());
    d[8..12].copy_from_slice(&len.to_le_bytes());
    d[12..14].copy_from_slice(&flags.to_le_bytes());
    d[14..16].copy_from_slice(&next.to_le_bytes());
    d
}

/// The device broke a ring invariant (more completions than buffers we posted).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingCorrupt;

/// Completions waiting in the used ring: `used_idx - last_used` (wrapping
/// 16-bit). More than `qsize` pending cannot happen with a correct device.
pub fn used_pending(used_idx: u16, last_used: u16, qsize: u16) -> Result<u16, RingCorrupt> {
    let n = used_idx.wrapping_sub(last_used);
    if n > qsize { Err(RingCorrupt) } else { Ok(n) }
}

/// Validate one used-ring element and split it into `(descriptor head, written
/// length)`. The id must name a descriptor of this queue.
pub fn decode_used(id: u32, len: u32, qsize: u16) -> Option<(u16, u32)> {
    let head = u16::try_from(id).ok()?;
    (head < qsize).then_some((head, len))
}

/// How a received buffer maps to a frame handed to the caller.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rx {
    /// A frame: copy `copy` bytes, which is the whole frame unless `truncated`
    /// (the caller's buffer was smaller).
    Frame { copy: usize, truncated: bool },
    /// Too short to hold the header plus an Ethernet header: count an error.
    Runt,
}

/// Interpret a completed receive buffer of `used_len` bytes (header included,
/// as the device reports it) for a caller buffer of `out_cap` bytes.
pub fn rx_frame(used_len: u32, out_cap: usize) -> Rx {
    let used = used_len as usize;
    // Header + at least a full Ethernet header (14 bytes).
    if used < HDR_LEN + 14 {
        return Rx::Runt;
    }
    let frame = (used - HDR_LEN).min(BUF_LEN - HDR_LEN);
    Rx::Frame {
        copy: frame.min(out_cap),
        truncated: frame > out_cap,
    }
}

/// Bytes put on the transmit descriptor for a frame of `frame_len` bytes: the
/// header plus the frame padded to the Ethernet minimum. `None` for an empty
/// frame or one over [`MAX_FRAME`].
pub fn tx_total(frame_len: usize) -> Option<usize> {
    if frame_len == 0 || frame_len > MAX_FRAME {
        return None;
    }
    Some(HDR_LEN + frame_len.max(MIN_FRAME))
}

/// Which of up to 32 DMA buffer slots are in use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotSet {
    used: u32,
    n: u8,
}

impl SlotSet {
    /// `n` slots (1..=32), all free.
    pub fn new(n: usize) -> SlotSet {
        SlotSet {
            used: 0,
            n: n.clamp(1, 32) as u8,
        }
    }

    /// Take the lowest free slot.
    pub fn alloc(&mut self) -> Option<usize> {
        let i = (!self.used).trailing_zeros() as usize;
        if i >= usize::from(self.n) {
            return None;
        }
        self.used |= 1 << i;
        Some(i)
    }

    /// Release slot `i`. `false` if it was not in use (a duplicate or bogus
    /// completion from the device) or out of range.
    pub fn free(&mut self, i: usize) -> bool {
        if i >= usize::from(self.n) || self.used & (1 << i) == 0 {
            return false;
        }
        self.used &= !(1 << i);
        true
    }

    pub fn in_use(&self) -> usize {
        self.used.count_ones() as usize
    }

    pub fn is_used(&self, i: usize) -> bool {
        i < usize::from(self.n) && self.used & (1 << i) != 0
    }
}

/// The MAC from the device-specific config (offset 0, six bytes) if the device
/// offered `F_MAC` and it is a usable unicast address; `None` otherwise (the
/// caller then falls back to its own address).
pub fn config_mac(negotiated_lo: u32, cfg: &[u8]) -> Option<Mac> {
    if negotiated_lo & F_MAC == 0 {
        return None;
    }
    let b: [u8; 6] = cfg.get(..6)?.try_into().ok()?;
    // Not all-zero and not a group (multicast/broadcast) address.
    (b != [0; 6] && b[0] & 1 == 0).then_some(Mac(b))
}

/// Link state from the config `status` word. Without `F_STATUS` the device does
/// not report it and the link is assumed up.
pub fn link_up(negotiated_lo: u32, status: u16) -> bool {
    negotiated_lo & F_STATUS == 0 || status & STATUS_LINK_UP != 0
}

#[cfg(test)]
mod tests;
