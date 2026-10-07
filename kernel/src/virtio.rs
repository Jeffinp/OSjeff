//! virtio 1.0 modern PCI transport — discovery half.
//!
//! A virtio device advertises where its configuration structures live through
//! vendor-specific PCI capabilities (`virtio_pci_cap`). Each says which BAR and
//! offset holds the common config, the notify region, the ISR byte and the
//! device-specific config. [`discover`] walks the list and collects them; the
//! virtio-gpu driver then maps those MMIO windows and drives the device.

use crate::pci::PciDevice;
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{OffsetPageTable, PageTable, Translate};

/// Translate a kernel virtual address to its physical address by walking the
/// active page tables (reachable through `phys_offset`). Needed to hand the
/// virtio device the physical addresses of DMA buffers.
pub fn virt_to_phys(virt: u64, phys_offset: u64) -> Option<u64> {
    let (l4_frame, _) = Cr3::read();
    let l4_virt = phys_offset + l4_frame.start_address().as_u64();
    // SAFETY: the bootloader maps all physical memory at `phys_offset` (Mapping::Dynamic), so
    // `phys_offset + CR3 frame` is the live, 4 KiB-aligned L4 table; no other Rust reference to it
    // exists and it is only read here.
    // NOTE: `phys_offset` is trusted from the caller (safe fn, not validated).
    let l4: &mut PageTable = unsafe { &mut *(l4_virt as *mut PageTable) };
    // SAFETY: `l4` is the active L4 table and `phys_offset` maps all physical memory (see above),
    // which is what `OffsetPageTable::new` requires.
    let mapper = unsafe { OffsetPageTable::new(l4, VirtAddr::new(phys_offset)) };
    mapper
        .translate_addr(VirtAddr::new(virt))
        .map(|p| p.as_u64())
}

const VIRTIO_PCI_CAP: u8 = 0x09; // vendor-specific cap carrying virtio config

// `cfg_type` values inside a virtio_pci_cap.
pub const CFG_COMMON: u8 = 1;
pub const CFG_NOTIFY: u8 = 2;
pub const CFG_ISR: u8 = 3;
pub const CFG_DEVICE: u8 = 4;

/// Location of one virtio config structure: which BAR, and the offset/length
/// within it.
#[derive(Clone, Copy, Default, Debug)]
pub struct CapLoc {
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
}

impl CapLoc {
    /// Whether this config structure was advertised (used by the driver when it
    /// maps the MMIO windows).
    #[allow(dead_code)]
    pub fn present(&self) -> bool {
        self.length != 0
    }
}

/// The four virtio config structures plus the notify offset multiplier.
#[derive(Clone, Copy, Default, Debug)]
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
#[allow(dead_code)] // set after the virtqueues exist (next step)
pub const S_DRIVER_OK: u8 = 4;
pub const S_FEATURES_OK: u8 = 8;
pub const S_FAILED: u8 = 128;

/// Volatile accessor over a virtio common-config MMIO window. All access is
/// MMIO, so every read/write is volatile.
pub struct Common {
    base: *mut u8,
}

impl Common {
    /// # Safety
    /// `addr` must be the mapped virtual address of the common-config window.
    pub unsafe fn new(addr: u64) -> Self {
        Self {
            base: addr as *mut u8,
        }
    }

    // The raw accessors below are `unsafe` only because `o` must stay inside the window given to
    // `Common::new`. NOTE: that window's size is not validated against the PCI capability length
    // (docs/audit/01-memoria-unsafe.md finding 9).
    #[inline]
    unsafe fn r8(&self, o: usize) -> u8 {
        // SAFETY: by this fn's contract `o` is inside the mapped common-config window.
        unsafe { core::ptr::read_volatile(self.base.add(o)) }
    }
    #[inline]
    unsafe fn w8(&self, o: usize, v: u8) {
        // SAFETY: by this fn's contract `o` is inside the mapped common-config window.
        unsafe { core::ptr::write_volatile(self.base.add(o), v) }
    }
    #[inline]
    unsafe fn r16(&self, o: usize) -> u16 {
        // SAFETY: by this fn's contract `o` is inside the window and 2-aligned.
        unsafe { core::ptr::read_volatile(self.base.add(o) as *const u16) }
    }
    #[inline]
    unsafe fn w16(&self, o: usize, v: u16) {
        // SAFETY: by this fn's contract `o` is inside the window and 2-aligned.
        unsafe { core::ptr::write_volatile(self.base.add(o) as *mut u16, v) }
    }
    #[inline]
    unsafe fn r32(&self, o: usize) -> u32 {
        // SAFETY: by this fn's contract `o` is inside the window and 4-aligned.
        unsafe { core::ptr::read_volatile(self.base.add(o) as *const u32) }
    }
    #[inline]
    unsafe fn w32(&self, o: usize, v: u32) {
        // SAFETY: by this fn's contract `o` is inside the window and 4-aligned.
        unsafe { core::ptr::write_volatile(self.base.add(o) as *mut u32, v) }
    }
    #[inline]
    unsafe fn w64(&self, o: usize, v: u64) {
        // Common-config 64-bit fields are written as two 32-bit halves.
        // SAFETY: both halves (`o`, `o + 4`) are inside the window; callers use 0x20/0x28/0x30.
        unsafe {
            self.w32(o, v as u32);
            self.w32(o + 4, (v >> 32) as u32);
        }
    }

    // ---- virtqueue setup (after `queue_select`) ----
    pub fn select_queue(&self, q: u16) {
        // SAFETY: 0x16 (queue_select) is inside the common-cfg window, 2-aligned.
        unsafe { self.w16(0x16, q) }
    }
    pub fn queue_size(&self) -> u16 {
        // SAFETY: 0x18 (queue_size) is inside the common-cfg window, 2-aligned.
        unsafe { self.r16(0x18) }
    }
    pub fn set_queue_size(&self, n: u16) {
        // SAFETY: 0x18 (queue_size) is inside the common-cfg window, 2-aligned.
        unsafe { self.w16(0x18, n) }
    }
    pub fn set_queue_desc(&self, phys: u64) {
        // SAFETY: 0x20 (queue_desc, 8 bytes) is inside the common-cfg window.
        unsafe { self.w64(0x20, phys) }
    }
    pub fn set_queue_driver(&self, phys: u64) {
        // SAFETY: 0x28 (queue_driver, 8 bytes) is inside the common-cfg window.
        unsafe { self.w64(0x28, phys) }
    }
    pub fn set_queue_device(&self, phys: u64) {
        // SAFETY: 0x30 (queue_device, 8 bytes) is inside the common-cfg window.
        unsafe { self.w64(0x30, phys) }
    }
    pub fn enable_queue(&self) {
        // SAFETY: 0x1C (queue_enable) is inside the common-cfg window, 2-aligned.
        unsafe { self.w16(0x1C, 1) }
    }
    pub fn queue_notify_off(&self) -> u16 {
        // SAFETY: 0x1E (queue_notify_off) is inside the common-cfg window, 2-aligned.
        unsafe { self.r16(0x1E) }
    }

    pub fn status(&self) -> u8 {
        // SAFETY: 0x14 (device_status) is inside the common-cfg window.
        unsafe { self.r8(0x14) }
    }
    pub fn set_status(&self, s: u8) {
        // SAFETY: 0x14 (device_status) is inside the common-cfg window.
        unsafe { self.w8(0x14, s) }
    }
    /// Read a 32-bit window of the device feature bits (`sel` = 0 -> bits 0..31,
    /// 1 -> bits 32..63).
    pub fn device_features(&self, sel: u32) -> u32 {
        // SAFETY: 0x00 (feature select) and 0x04 (feature bits) are inside the common-cfg window, 4-aligned.
        unsafe {
            self.w32(0x00, sel);
            self.r32(0x04)
        }
    }
    /// Write a 32-bit window of the negotiated driver feature bits.
    pub fn set_driver_features(&self, sel: u32, v: u32) {
        // SAFETY: 0x08 (driver feature select) and 0x0C (driver features) are inside the window, 4-aligned.
        unsafe {
            self.w32(0x08, sel);
            self.w32(0x0C, v);
        }
    }
}

/// Drive the virtio 1.0 reset + feature negotiation up to FEATURES_OK (the
/// DRIVER_OK bit is set later, once the virtqueues exist). We accept only
/// `VIRTIO_F_VERSION_1` (bit 32). Returns false if the device rejects it.
pub fn negotiate(c: &Common) -> bool {
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

/// Physical base address of BAR `bar`, handling 64-bit (two-dword) BARs.
pub fn bar_base(dev: &PciDevice, bar: u8) -> u64 {
    let lo = dev.bar(bar);
    if lo & 0b110 == 0b100 {
        // 64-bit memory BAR: the high half is in the next BAR slot.
        ((dev.bar(bar + 1) as u64) << 32) | (lo as u64 & !0xF)
    } else {
        lo as u64 & !0xF
    }
}

/// Walk `dev`'s PCI capability list and collect its virtio config locations.
/// `None` if the device exposes no virtio common-config capability.
pub fn discover(dev: &PciDevice) -> Option<VirtioCaps> {
    let mut off = dev.cap_list()?;
    let mut caps = VirtioCaps::default();
    let mut have_common = false;

    // Capability lists are short; bound the walk to guard against a corrupt loop.
    for _ in 0..48 {
        if off == 0 {
            break;
        }
        let w0 = dev.cap_read32(off); // [cap_id][cap_next][cap_len][cfg_type]
        let id = (w0 & 0xFF) as u8;
        let next = ((w0 >> 8) & 0xFF) as u8 & 0xFC;

        if id == VIRTIO_PCI_CAP {
            let cfg_type = ((w0 >> 24) & 0xFF) as u8;
            let loc = CapLoc {
                bar: (dev.cap_read32(off + 4) & 0xFF) as u8,
                offset: dev.cap_read32(off + 8),
                length: dev.cap_read32(off + 12),
            };
            match cfg_type {
                CFG_COMMON => {
                    caps.common = loc;
                    have_common = true;
                }
                CFG_NOTIFY => {
                    caps.notify = loc;
                    caps.notify_off_mul = dev.cap_read32(off + 16);
                }
                CFG_ISR => caps.isr = loc,
                CFG_DEVICE => caps.device = loc,
                _ => {}
            }
        }
        off = next;
    }

    have_common.then_some(caps)
}
