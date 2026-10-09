//! virtio 1.0 modern PCI transport: capability discovery and the reset/feature
//! negotiation handshake, independent of how config space or the common-config
//! MMIO window are actually reached.
//!
//! A virtio device advertises where its configuration structures live through
//! vendor-specific PCI capabilities (`virtio_pci_cap`). Each says which BAR and
//! offset holds the common config, the notify region, the ISR byte and the
//! device-specific config. [`discover`] walks the list and collects them.

/// PCI capability id of the vendor-specific cap that carries virtio config.
const VIRTIO_PCI_CAP: u8 = 0x09;

// `cfg_type` values inside a virtio_pci_cap.
pub const CFG_COMMON: u8 = 1;
pub const CFG_NOTIFY: u8 = 2;
pub const CFG_ISR: u8 = 3;
pub const CFG_DEVICE: u8 = 4;

/// Location of one virtio config structure: which BAR, and the offset/length
/// within it.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct CapLoc {
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
}

impl CapLoc {
    /// Whether this config structure was advertised.
    pub fn present(&self) -> bool {
        self.length != 0
    }

    /// Whether the capability is well formed and at least `min_len` bytes long:
    /// BAR index 0..=5 and the byte range `offset..offset + length` inside a 32-bit
    /// BAR offset space (no wrap-around).
    pub fn is_valid(&self, min_len: u32) -> bool {
        self.bar < BAR_COUNT
            && self.length >= min_len
            && self.offset as u64 + self.length as u64 <= 1 << 32
    }
}

/// Size of the common-config structure the driver accesses (through
/// `queue_device`, offset 0x30 + 8).
pub const COMMON_CFG_LEN: u32 = 0x38;
/// Smallest notify window: one 16-bit doorbell.
const NOTIFY_MIN_LEN: u32 = 2;

/// Clamps the device's advertised queue size to the driver's ring capacity
/// `max`. `None` when the queue is unavailable (size 0) or the device reports a
/// size that is not a power of two (the split ring requires one).
pub fn validate_queue_size(device_qsize: u16, max: u16) -> Option<u16> {
    if device_qsize == 0 || !device_qsize.is_power_of_two() {
        return None;
    }
    Some(device_qsize.min(max))
}

/// Byte offset of queue `queue_notify_off`'s doorbell from the start of the
/// notify structure (`queue_notify_off * notify_off_mul`), or `None` if the
/// structure is absent or the 16-bit doorbell would not fit inside it.
pub fn notify_doorbell_offset(
    notify: &CapLoc,
    notify_off_mul: u32,
    queue_notify_off: u16,
) -> Option<u64> {
    if !notify.present() {
        return None;
    }
    let rel = queue_notify_off as u64 * notify_off_mul as u64;
    if rel + NOTIFY_MIN_LEN as u64 > notify.length as u64 {
        return None;
    }
    Some(rel)
}

/// The four virtio config structures plus the notify offset multiplier.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct VirtioCaps {
    pub common: CapLoc,
    pub notify: CapLoc,
    pub notify_off_mul: u32,
    pub isr: CapLoc,
    pub device: CapLoc,
}

// virtio device_status bits.
pub const S_ACK: u8 = 1;
pub const S_DRIVER: u8 = 2;
pub const S_DRIVER_OK: u8 = 4;
pub const S_FEATURES_OK: u8 = 8;
pub const S_FAILED: u8 = 128;

/// Read access to a device's PCI config space, as needed to walk capabilities.
pub trait CapSpace {
    /// Offset of the first capability, or `None` if the device has no list.
    fn cap_list(&self) -> Option<u8>;
    /// Reads the config-space dword at `offset`.
    fn read32(&self, offset: u8) -> u32;
}

use super::pci::BAR_COUNT;

/// Maximum capabilities visited; guards against a corrupt (looping) list.
const MAX_CAPS: usize = 48;

/// Walks the PCI capability list and collects the virtio config locations.
/// `None` if the device exposes no virtio common-config capability.
pub fn discover<S: CapSpace>(dev: &S) -> Option<VirtioCaps> {
    let mut off = dev.cap_list()?;
    let mut caps = VirtioCaps::default();
    let mut have_common = false;

    for _ in 0..MAX_CAPS {
        if off == 0 {
            break;
        }
        let w0 = dev.read32(off); // [cap_id][cap_next][cap_len][cfg_type]
        let id = (w0 & 0xFF) as u8;
        let next = ((w0 >> 8) & 0xFF) as u8 & 0xFC;

        if id == VIRTIO_PCI_CAP {
            let cfg_type = ((w0 >> 24) & 0xFF) as u8;
            let loc = CapLoc {
                bar: (dev.read32(off.wrapping_add(4)) & 0xFF) as u8,
                offset: dev.read32(off.wrapping_add(8)),
                length: dev.read32(off.wrapping_add(12)),
            };
            match cfg_type {
                CFG_COMMON => {
                    caps.common = loc;
                    have_common = true;
                }
                CFG_NOTIFY => {
                    caps.notify = loc;
                    caps.notify_off_mul = dev.read32(off.wrapping_add(16));
                }
                CFG_ISR => caps.isr = loc,
                CFG_DEVICE => caps.device = loc,
                _ => {}
            }
        }
        off = next;
    }

    // Reject a common-config window the driver could not safely touch, and drop
    // any other structure whose BAR/offset/length is nonsensical (it then reads
    // as "not present").
    if !have_common || !caps.common.is_valid(COMMON_CFG_LEN) {
        return None;
    }
    if !caps.notify.is_valid(NOTIFY_MIN_LEN) {
        caps.notify = CapLoc::default();
    }
    if !caps.isr.is_valid(1) {
        caps.isr = CapLoc::default();
    }
    if !caps.device.is_valid(0) {
        caps.device = CapLoc::default();
    }
    Some(caps)
}

/// The device-status and feature registers of the common-config window that the
/// handshake in [`negotiate`] touches.
pub trait CommonCfg {
    fn status(&self) -> u8;
    fn set_status(&self, s: u8);
    /// Reads a 32-bit window of the device feature bits (`sel` 0 = bits 0..31,
    /// 1 = bits 32..63).
    fn device_features(&self, sel: u32) -> u32;
    /// Writes a 32-bit window of the negotiated driver feature bits.
    fn set_driver_features(&self, sel: u32, v: u32);
}

/// Drives the virtio 1.0 reset + feature negotiation up to FEATURES_OK (the
/// DRIVER_OK bit is set later, once the virtqueues exist). Only
/// `VIRTIO_F_VERSION_1` (bit 32) is accepted. Returns `false` if the device
/// rejects the feature set.
pub fn negotiate<C: CommonCfg>(c: &C) -> bool {
    c.set_status(0); // reset
    let _ = c.status(); // read back to flush the reset
    c.set_status(S_ACK);
    c.set_status(S_ACK | S_DRIVER);

    let _have = c.device_features(1); // bits 32..63 (must contain VERSION_1)
    c.set_driver_features(0, 0);
    c.set_driver_features(1, 1 << 0); // VIRTIO_F_VERSION_1 (bit 32)

    c.set_status(S_ACK | S_DRIVER | S_FEATURES_OK);
    let s = c.status();
    s & S_FEATURES_OK != 0 && s & S_FAILED == 0
}

/// `VIRTIO_F_VERSION_1`: bit 32, i.e. bit 0 of the high feature word.
pub const F_VERSION_1_HI: u32 = 1;

/// Like [`negotiate`], but a driver that needs device-specific features asks for
/// the low-word bits in `want_lo` and gets back the subset the device offers
/// (`Some(accepted_lo)`). `VIRTIO_F_VERSION_1` is mandatory here: a device that
/// does not offer it (a legacy-only device) is refused (`None`) after marking the
/// handshake FAILED, so the caller does not drive it with the modern layout.
/// `None` as well when the device rejects the final feature set.
pub fn negotiate_features<C: CommonCfg>(c: &C, want_lo: u32) -> Option<u32> {
    c.set_status(0); // reset
    let _ = c.status(); // read back to flush the reset
    c.set_status(S_ACK);
    c.set_status(S_ACK | S_DRIVER);

    let lo = c.device_features(0);
    let hi = c.device_features(1);
    if hi & F_VERSION_1_HI == 0 {
        c.set_status(S_ACK | S_DRIVER | S_FAILED);
        return None;
    }
    let accepted = lo & want_lo;
    c.set_driver_features(0, accepted);
    c.set_driver_features(1, F_VERSION_1_HI);

    c.set_status(S_ACK | S_DRIVER | S_FEATURES_OK);
    let s = c.status();
    (s & S_FEATURES_OK != 0 && s & S_FAILED == 0).then_some(accepted)
}

#[cfg(test)]
mod tests;
