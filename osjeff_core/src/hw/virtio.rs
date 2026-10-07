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

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::{Cell, RefCell};

    /// A 256-byte config space.
    struct Cfg {
        mem: [u8; 256],
        list: Option<u8>,
    }

    impl Cfg {
        fn new(list: Option<u8>) -> Self {
            Self {
                mem: [0; 256],
                list,
            }
        }
        fn put32(&mut self, off: usize, v: u32) {
            self.mem[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        /// Writes a virtio_pci_cap at `off`.
        fn virtio_cap(&mut self, off: usize, next: u8, cfg_type: u8, bar: u8, o: u32, len: u32) {
            self.put32(
                off,
                VIRTIO_PCI_CAP as u32 | (next as u32) << 8 | 16 << 16 | (cfg_type as u32) << 24,
            );
            self.put32(off + 4, bar as u32);
            self.put32(off + 8, o);
            self.put32(off + 12, len);
        }
    }

    impl CapSpace for Cfg {
        fn cap_list(&self) -> Option<u8> {
            self.list
        }
        fn read32(&self, offset: u8) -> u32 {
            let o = offset as usize & !3;
            u32::from_le_bytes(self.mem[o..o + 4].try_into().unwrap())
        }
    }

    fn standard() -> Cfg {
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0x50, CFG_COMMON, 4, 0x0000, 0x38);
        c.virtio_cap(0x50, 0x64, CFG_NOTIFY, 4, 0x3000, 0x1000);
        c.put32(0x60, 4); // notify_off_multiplier at cap + 16
        c.virtio_cap(0x64, 0x74, CFG_ISR, 4, 0x1000, 0x1000);
        c.virtio_cap(0x74, 0x00, CFG_DEVICE, 4, 0x2000, 0x1000);
        c
    }

    #[test]
    fn discovers_all_four_structures() {
        let caps = discover(&standard()).expect("caps");
        assert_eq!(
            caps.common,
            CapLoc {
                bar: 4,
                offset: 0,
                length: 0x38
            }
        );
        assert_eq!(caps.notify.offset, 0x3000);
        assert_eq!(caps.notify_off_mul, 4);
        assert_eq!(caps.isr.offset, 0x1000);
        assert_eq!(caps.device.offset, 0x2000);
        assert!(caps.common.present() && caps.device.present());
    }

    #[test]
    fn no_capability_list_means_no_caps() {
        assert_eq!(discover(&Cfg::new(None)), None);
    }

    #[test]
    fn empty_list_pointer_means_no_caps() {
        assert_eq!(discover(&Cfg::new(Some(0))), None);
    }

    #[test]
    fn missing_common_cfg_is_rejected() {
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0, CFG_NOTIFY, 4, 0, 0x100);
        assert_eq!(discover(&c), None);
    }

    #[test]
    fn foreign_capabilities_are_skipped() {
        let mut c = Cfg::new(Some(0x40));
        // MSI-X (0x11) and power management (0x01) before the virtio caps.
        c.put32(0x40, 0x11 | 0x50 << 8);
        c.put32(0x50, 0x01 | 0x60 << 8);
        c.virtio_cap(0x60, 0, CFG_COMMON, 1, 0x10, 0x38);
        let caps = discover(&c).expect("caps");
        assert_eq!(caps.common.bar, 1);
        assert_eq!(caps.common.offset, 0x10);
    }

    #[test]
    fn common_cfg_smaller_than_the_driver_needs_is_rejected() {
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0, CFG_COMMON, 0, 0, COMMON_CFG_LEN - 1);
        assert_eq!(discover(&c), None);
        c.virtio_cap(0x40, 0, CFG_COMMON, 0, 0, COMMON_CFG_LEN);
        assert!(discover(&c).is_some());
    }

    #[test]
    fn out_of_range_bar_or_overflowing_offset_is_rejected() {
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0, CFG_COMMON, 6, 0, 0x38); // BAR 6 does not exist
        assert_eq!(discover(&c), None);
        c.virtio_cap(0x40, 0, CFG_COMMON, 0, 0xFFFF_FFF0, 0x38); // offset + len wraps
        assert_eq!(discover(&c), None);
        c.virtio_cap(0x40, 0, CFG_COMMON, 0, 0xFFFF_FFC8, 0x38); // ends exactly at 2^32
        assert!(discover(&c).is_some());
    }

    #[test]
    fn malformed_optional_structures_read_as_absent() {
        let mut c = standard();
        c.virtio_cap(0x50, 0x64, CFG_NOTIFY, 9, 0x3000, 0x1000); // bad BAR
        c.virtio_cap(0x64, 0x74, CFG_ISR, 0, 0, 0); // zero length
        c.virtio_cap(0x74, 0, CFG_DEVICE, 1, u32::MAX, 16); // overflow
        let caps = discover(&c).expect("common is fine");
        assert!(!caps.notify.present());
        assert!(!caps.isr.present());
        assert!(!caps.device.present());
    }

    #[test]
    fn queue_size_validation() {
        assert_eq!(validate_queue_size(0, 16), None); // unavailable queue
        assert_eq!(validate_queue_size(3, 16), None); // not a power of two
        assert_eq!(validate_queue_size(12, 16), None);
        assert_eq!(validate_queue_size(8, 16), Some(8));
        assert_eq!(validate_queue_size(256, 16), Some(16)); // clamped to the ring
        assert_eq!(validate_queue_size(16, 16), Some(16));
        assert_eq!(validate_queue_size(0x8000, 16), Some(16));
    }

    #[test]
    fn notify_doorbell_must_fit_in_the_window() {
        let n = CapLoc {
            bar: 4,
            offset: 0x3000,
            length: 0x1000,
        };
        assert_eq!(notify_doorbell_offset(&n, 4, 0), Some(0));
        assert_eq!(notify_doorbell_offset(&n, 4, 1), Some(4));
        assert_eq!(notify_doorbell_offset(&n, 4, 0x3FE), Some(0xFF8));
        // rel = 0xFFE: the 2-byte doorbell ends exactly at the window end.
        assert_eq!(notify_doorbell_offset(&n, 2, 0x7FF), Some(0xFFE));
        assert_eq!(notify_doorbell_offset(&n, 4, 0x3FF), Some(0xFFC)); // ends at 0xFFE
        assert_eq!(notify_doorbell_offset(&n, 4, 0x400), None); // rel 0x1000: past the end
    }

    #[test]
    fn notify_doorbell_rejects_absent_and_huge_offsets() {
        assert_eq!(notify_doorbell_offset(&CapLoc::default(), 4, 0), None);
        let n = CapLoc {
            bar: 0,
            offset: 0,
            length: 0x1000,
        };
        assert_eq!(notify_doorbell_offset(&n, u32::MAX, u16::MAX), None); // no u32 overflow
        assert_eq!(notify_doorbell_offset(&n, 0, u16::MAX), Some(0)); // mul 0: shared doorbell
        assert_eq!(notify_doorbell_offset(&n, 0x1000, 1), None);
    }

    #[test]
    fn unknown_cfg_types_are_ignored() {
        let mut c = standard();
        c.virtio_cap(0x74, 0x84, 9, 7, 0xDEAD, 0xBEEF); // vendor-specific cfg_type
        c.virtio_cap(0x84, 0, CFG_PCI_CFG_TYPE, 0, 0, 0);
        let caps = discover(&c).expect("caps");
        assert_eq!(caps.device, CapLoc::default());
        assert_eq!(caps.common.length, 0x38);
    }
    const CFG_PCI_CFG_TYPE: u8 = 5; // VIRTIO_PCI_CAP_PCI_CFG: advertised, not collected

    #[test]
    fn next_pointer_low_bits_are_masked() {
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0x53, CFG_COMMON, 0, 0, 0x38); // 0x53 -> 0x50
        c.virtio_cap(0x50, 0, CFG_ISR, 0, 0x80, 4);
        let caps = discover(&c).expect("caps");
        assert_eq!(caps.isr.offset, 0x80);
    }

    #[test]
    fn later_duplicate_overrides_earlier() {
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0x50, CFG_COMMON, 0, 0x100, 0x38);
        c.virtio_cap(0x50, 0, CFG_COMMON, 2, 0x200, 0x40);
        assert_eq!(discover(&c).unwrap().common.offset, 0x200);
    }

    #[test]
    fn looping_list_terminates() {
        // 0x40 -> 0x50 -> 0x40 -> ...: bounded by MAX_CAPS.
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0x50, CFG_ISR, 0, 1, 1);
        c.virtio_cap(0x50, 0x40, CFG_DEVICE, 0, 2, 2);
        assert_eq!(discover(&c), None); // no common cap, but it returned
        c.virtio_cap(0x50, 0x40, CFG_COMMON, 0, 3, 0x38);
        assert!(discover(&c).is_some());
    }

    #[test]
    fn self_referencing_cap_terminates() {
        let mut c = Cfg::new(Some(0x40));
        c.virtio_cap(0x40, 0x40, CFG_COMMON, 0, 0, 0x38);
        assert!(discover(&c).is_some());
    }

    #[test]
    fn caps_near_the_end_of_config_space_do_not_panic() {
        // A capability at 0xFC: its payload dwords wrap past 0xFF.
        let mut c = Cfg::new(Some(0xFC));
        c.put32(0xFC, VIRTIO_PCI_CAP as u32 | (CFG_COMMON as u32) << 24);
        let _ = discover(&c);
    }

    /// Mock common-config window recording the handshake.
    struct Dev {
        status: Cell<u8>,
        accept_features: bool,
        log: RefCell<Vec<(&'static str, u32)>>,
    }

    impl Dev {
        fn new(accept: bool) -> Self {
            Self {
                status: Cell::new(0xAA),
                accept_features: accept,
                log: RefCell::new(Vec::new()),
            }
        }
    }

    impl CommonCfg for Dev {
        fn status(&self) -> u8 {
            self.status.get()
        }
        fn set_status(&self, s: u8) {
            self.log.borrow_mut().push(("status", s as u32));
            // The device drops FEATURES_OK when it dislikes the features.
            let s = if s & S_FEATURES_OK != 0 && !self.accept_features {
                s & !S_FEATURES_OK
            } else {
                s
            };
            self.status.set(s);
        }
        fn device_features(&self, sel: u32) -> u32 {
            self.log.borrow_mut().push(("devfeat_sel", sel));
            1
        }
        fn set_driver_features(&self, sel: u32, v: u32) {
            self.log.borrow_mut().push(("drvfeat", sel << 16 | v));
        }
    }

    #[test]
    fn negotiation_follows_the_spec_order() {
        let d = Dev::new(true);
        assert!(negotiate(&d));
        assert_eq!(
            *d.log.borrow(),
            vec![
                ("status", 0),
                ("status", S_ACK as u32),
                ("status", (S_ACK | S_DRIVER) as u32),
                ("devfeat_sel", 1),
                ("drvfeat", 0),
                ("drvfeat", 1 << 16 | 1), // sel 1, VERSION_1
                ("status", (S_ACK | S_DRIVER | S_FEATURES_OK) as u32),
            ]
        );
        assert_eq!(d.status(), S_ACK | S_DRIVER | S_FEATURES_OK);
    }

    #[test]
    fn negotiation_fails_when_the_device_clears_features_ok() {
        assert!(!negotiate(&Dev::new(false)));
    }

    #[test]
    fn negotiation_fails_when_the_device_sets_failed() {
        struct Failing(Cell<u8>);
        impl CommonCfg for Failing {
            fn status(&self) -> u8 {
                self.0.get()
            }
            fn set_status(&self, s: u8) {
                self.0.set(s | S_FAILED);
            }
            fn device_features(&self, _: u32) -> u32 {
                0
            }
            fn set_driver_features(&self, _: u32, _: u32) {}
        }
        assert!(!negotiate(&Failing(Cell::new(0))));
    }
}
