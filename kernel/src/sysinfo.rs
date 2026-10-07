//! Static facts about the machine, collected once at boot for the resource
//! monitor's "Sistema" tab: CPU identity from CPUID, physical memory from the
//! bootloader's memory map, how we were booted, and the screen size.

use crate::sync::RacyCell;
use bootloader_api::info::{MemoryRegion, MemoryRegionKind};
use osjeff_core::klog::FixedBuf;

pub struct SysInfo {
    /// RAM the firmware reported as usable (free for the bootloader / kernel).
    pub usable_bytes: u64,
    /// RAM the bootloader itself holds (kernel image, page tables, boot info).
    pub boot_bytes: u64,
    /// Size of the kernel image in memory (code + data + BSS).
    pub kernel_bytes: u64,
    pub boot_mode: &'static str,
    pub width: usize,
    pub height: usize,
    pub vendor: FixedBuf<12>,
    pub brand: FixedBuf<48>,
    pub features: FixedBuf<64>,
    /// Hypervisor name from CPUID 0x4000_0000 (`"TCGTCGTCGTCG"`, `"KVMKVMKVM"`),
    /// empty on bare metal.
    pub hypervisor: FixedBuf<12>,
}

static INFO: RacyCell<Option<SysInfo>> = RacyCell::new(None);

/// Total RAM = usable + bootloader-owned.
impl SysInfo {
    pub fn total_ram(&self) -> u64 {
        self.usable_bytes + self.boot_bytes
    }
}

fn cpuid(leaf: u32) -> core::arch::x86_64::CpuidResult {
    // CPUID exists on every x86_64 CPU and only fills registers; any leaf is legal (an
    // unsupported one returns zeros or the data of the highest supported leaf).
    core::arch::x86_64::__cpuid(leaf)
}

fn push_regs(buf: &mut FixedBuf<48>, r: core::arch::x86_64::CpuidResult) {
    use core::fmt::Write;
    for reg in [r.eax, r.ebx, r.ecx, r.edx] {
        for b in reg.to_le_bytes() {
            if b != 0 {
                let _ = buf.write_char(b as char);
            }
        }
    }
}

fn read_cpu() -> (FixedBuf<12>, FixedBuf<48>, FixedBuf<64>, FixedBuf<12>) {
    use core::fmt::Write;
    let l0 = cpuid(0);
    let mut vendor = FixedBuf::<12>::new();
    for reg in [l0.ebx, l0.edx, l0.ecx] {
        for b in reg.to_le_bytes() {
            let _ = vendor.write_char(b as char);
        }
    }
    let mut brand = FixedBuf::<48>::new();
    if cpuid(0x8000_0000).eax >= 0x8000_0004 {
        for leaf in 0x8000_0002..=0x8000_0004u32 {
            push_regs(&mut brand, cpuid(leaf));
        }
    }
    let l1 = cpuid(1);
    let l7 = if l0.eax >= 7 { cpuid(7) } else { l1 };
    let flags: [(bool, &str); 11] = [
        (l1.edx & (1 << 25) != 0, "SSE"),
        (l1.edx & (1 << 26) != 0, "SSE2"),
        (l1.ecx & 1 != 0, "SSE3"),
        (l1.ecx & (1 << 9) != 0, "SSSE3"),
        (l1.ecx & (1 << 19) != 0, "SSE4.1"),
        (l1.ecx & (1 << 20) != 0, "SSE4.2"),
        (l1.ecx & (1 << 23) != 0, "POPCNT"),
        (l1.ecx & (1 << 25) != 0, "AES"),
        (l1.ecx & (1 << 28) != 0, "AVX"),
        (l0.eax >= 7 && l7.ebx & (1 << 5) != 0, "AVX2"),
        (l1.ecx & (1 << 30) != 0, "RDRAND"),
    ];
    let mut features = FixedBuf::<64>::new();
    for (on, name) in flags {
        if on {
            let _ = write!(features, "{name} ");
        }
    }
    let mut hv = FixedBuf::<12>::new();
    if l1.ecx & (1 << 31) != 0 {
        let h = cpuid(0x4000_0000);
        for reg in [h.ebx, h.ecx, h.edx] {
            for b in reg.to_le_bytes() {
                if b != 0 {
                    let _ = hv.write_char(b as char);
                }
            }
        }
    }
    (vendor, brand, features, hv)
}

/// Gather everything. Call once from `kernel_main`, after the framebuffer
/// layout is known.
pub fn capture(regions: &[MemoryRegion], kernel_bytes: u64, width: usize, height: usize) {
    let (mut usable, mut boot) = (0u64, 0u64);
    let (mut uefi, mut bios) = (false, false);
    for r in regions {
        let len = r.end.saturating_sub(r.start);
        match r.kind {
            MemoryRegionKind::Usable => usable += len,
            MemoryRegionKind::Bootloader => boot += len,
            MemoryRegionKind::UnknownUefi(_) => uefi = true,
            MemoryRegionKind::UnknownBios(_) => bios = true,
            _ => {}
        }
    }
    let (vendor, brand, features, hypervisor) = read_cpu();
    let info = SysInfo {
        usable_bytes: usable,
        boot_bytes: boot,
        kernel_bytes,
        boot_mode: if uefi {
            "UEFI"
        } else if bios {
            "BIOS"
        } else {
            "?"
        },
        width,
        height,
        vendor,
        brand,
        features,
        hypervisor,
    };
    // SAFETY: called once at boot on the boot thread before any other user of INFO exists (the
    // desktop reads it only after `capture`).
    unsafe { *INFO.get() = Some(info) };
}

/// The captured facts (`None` before [`capture`]).
pub fn get() -> Option<&'static SysInfo> {
    // SAFETY: written once at boot by `capture` and only read afterwards.
    // NOTE: not guaranteed by the type; nothing writes INFO after boot.
    unsafe { (*INFO.get()).as_ref() }
}
