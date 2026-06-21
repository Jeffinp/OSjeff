//! virtio 1.0 modern PCI transport — discovery half.
//!
//! A virtio device advertises where its configuration structures live through
//! vendor-specific PCI capabilities (`virtio_pci_cap`). Each says which BAR and
//! offset holds the common config, the notify region, the ISR byte and the
//! device-specific config. [`discover`] walks the list and collects them; the
//! virtio-gpu driver then maps those MMIO windows and drives the device.

use crate::pci::PciDevice;

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
