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

/// Mock device with chosen feature words that records the accepted ones.
struct Feat {
    lo: u32,
    hi: u32,
    status: Cell<u8>,
    drv: RefCell<[u32; 2]>,
    reject: bool,
}

impl Feat {
    fn new(lo: u32, hi: u32, reject: bool) -> Self {
        Self {
            lo,
            hi,
            status: Cell::new(0),
            drv: RefCell::new([0; 2]),
            reject,
        }
    }
}

impl CommonCfg for Feat {
    fn status(&self) -> u8 {
        self.status.get()
    }
    fn set_status(&self, s: u8) {
        let s = if self.reject { s & !S_FEATURES_OK } else { s };
        self.status.set(s);
    }
    fn device_features(&self, sel: u32) -> u32 {
        if sel == 0 { self.lo } else { self.hi }
    }
    fn set_driver_features(&self, sel: u32, v: u32) {
        self.drv.borrow_mut()[sel as usize & 1] = v;
    }
}

#[test]
fn negotiate_features_accepts_the_wanted_subset_and_version_1() {
    // Device offers MAC, STATUS, CSUM, GSO...: we take only what we asked for.
    let d = Feat::new(0xFFFF_FFFF, 1, false);
    assert_eq!(
        negotiate_features(&d, (1 << 5) | (1 << 16)),
        Some((1 << 5) | (1 << 16))
    );
    assert_eq!(*d.drv.borrow(), [(1 << 5) | (1 << 16), F_VERSION_1_HI]);
    assert_eq!(d.status(), S_ACK | S_DRIVER | S_FEATURES_OK);
    // A device lacking MAC: we do not claim it.
    let d = Feat::new(1 << 16, 1, false);
    assert_eq!(negotiate_features(&d, (1 << 5) | (1 << 16)), Some(1 << 16));
    // Asking for nothing is allowed (what `negotiate` does).
    let d = Feat::new(0xFFFF_FFFF, 1, false);
    assert_eq!(negotiate_features(&d, 0), Some(0));
    assert_eq!(*d.drv.borrow(), [0, F_VERSION_1_HI]);
}

#[test]
fn negotiate_features_refuses_a_device_without_version_1() {
    let d = Feat::new(0xFFFF_FFFF, 0, false);
    assert_eq!(negotiate_features(&d, 0xFF), None);
    assert!(d.status() & S_FAILED != 0, "handshake marked FAILED");
    assert_eq!(d.status() & S_DRIVER_OK, 0);
}

#[test]
fn negotiate_features_fails_when_features_ok_is_cleared() {
    let d = Feat::new(0xFF, 1, true);
    assert_eq!(negotiate_features(&d, 0xFF), None);
}
