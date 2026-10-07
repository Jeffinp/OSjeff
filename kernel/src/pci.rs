//! Minimal PCI configuration-space access (legacy mechanism #1) and bus
//! enumeration — the foundation for the virtio-gpu driver. It locates devices
//! by reading config space through the 0xCF8/0xCFC port pair.

use crate::io::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

use osjeff_core::hw::pci::{
    VIRTIO_GPU_LEGACY, VIRTIO_GPU_MODERN, VIRTIO_NET_LEGACY, VIRTIO_NET_MODERN, VIRTIO_VENDOR,
    bar_offset, cap_list_start, config_address, extract_u16,
};

/// A located PCI function.
#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
}

/// Read a 32-bit dword from config space (`offset` is dword-aligned).
pub fn read32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    outl(CONFIG_ADDRESS, config_address(bus, slot, func, offset));
    inl(CONFIG_DATA)
}

/// Read a 16-bit word from config space.
pub fn read16(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    extract_u16(read32(bus, slot, func, offset & 0xFC), offset)
}

/// Write a 32-bit dword to config space.
pub fn write32(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    outl(CONFIG_ADDRESS, config_address(bus, slot, func, offset));
    outl(CONFIG_DATA, value);
}

impl PciDevice {
    /// Raw value of base address register `i` (0..6); 0 for a non-existent BAR.
    pub fn bar(&self, i: u8) -> u32 {
        match bar_offset(i) {
            Some(off) => read32(self.bus, self.slot, self.func, off),
            None => 0,
        }
    }

    /// Set the memory-space + bus-master bits in the command register, required
    /// before a DMA-capable device (like virtio-gpu) can be used.
    pub fn enable_bus_master(&self) {
        let mut cmd = read32(self.bus, self.slot, self.func, 0x04);
        cmd |= 0x0006; // bit1 = memory space, bit2 = bus master
        write32(self.bus, self.slot, self.func, 0x04, cmd);
    }

    /// Offset of the first entry in the PCI capability list, or `None` if the
    /// device has none (status register bit 4 clears it).
    pub fn cap_list(&self) -> Option<u8> {
        let status = read16(self.bus, self.slot, self.func, 0x06);
        let ptr = read16(self.bus, self.slot, self.func, 0x34);
        cap_list_start(status, ptr)
    }

    /// Read a config-space dword at `offset` (for walking capabilities).
    pub fn cap_read32(&self, offset: u8) -> u32 {
        read32(self.bus, self.slot, self.func, offset)
    }
}

/// Visit every present function on bus 0, calling `f` for each. QEMU places its
/// virtio devices on bus 0, so a single-bus scan suffices here.
pub fn for_each<F: FnMut(PciDevice)>(mut f: F) {
    osjeff_core::hw::pci::enumerate(
        |slot, func, off| read16(0, slot, func, off),
        |slot, func, vendor, device| {
            f(PciDevice {
                bus: 0,
                slot,
                func,
                vendor,
                device,
            })
        },
    );
}

/// Find the first function matching `vendor`/`device` on bus 0.
pub fn find(vendor: u16, device: u16) -> Option<PciDevice> {
    let mut found = None;
    for_each(|d| {
        if found.is_none() && d.vendor == vendor && d.device == device {
            found = Some(d);
        }
    });
    found
}

/// Locate a virtio-gpu device (modern, then transitional).
pub fn find_virtio_gpu() -> Option<PciDevice> {
    find(VIRTIO_VENDOR, VIRTIO_GPU_MODERN).or_else(|| find(VIRTIO_VENDOR, VIRTIO_GPU_LEGACY))
}

/// Locate a virtio-net device (modern, then transitional).
pub fn find_virtio_net() -> Option<PciDevice> {
    find(VIRTIO_VENDOR, VIRTIO_NET_MODERN).or_else(|| find(VIRTIO_VENDOR, VIRTIO_NET_LEGACY))
}
