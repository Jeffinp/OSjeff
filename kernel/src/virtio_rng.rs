//! virtio-rng driver (the virtio entropy device, virtio 1.x PCI transport, polled).
//!
//! QEMU: `-device virtio-rng-pci`. The device shows up as `1af4:1005` (transitional,
//! QEMU's default) or `1af4:1044` (modern); both expose the modern capabilities this
//! driver uses, so a legacy-only device is refused with a log line (same rule as
//! `virtio_net`).
//!
//! One queue (0), one device-writable descriptor pointing at a 32-byte static buffer:
//! [`VirtioRng::request`] posts it and rings the doorbell, [`VirtioRng::poll`] returns
//! the bytes once the device has used the descriptor. It never blocks: the owner
//! (`rng`) posts a request, does other work and polls later, or spins for a bounded
//! time at boot.
//!
//! The device is the *host's* entropy (QEMU feeds it from `/dev/urandom` by default), so
//! trusting it means trusting the hypervisor; `docs/SECURITY-MODEL.md` says so. Its bytes
//! are never used raw: they go through `osjeff_core::entropy`.

use crate::sync::RacyCell;
use crate::virtio::{self, Common};
use crate::{pci, serial_println};
use core::sync::atomic::{AtomicBool, Ordering, fence};
use osjeff_core::hw::virtio_net::{self as vn, QueueLayout};

/// Bytes asked for per request (256 bits: one DRBG key).
pub const REQ_LEN: usize = 32;
/// Queue entries used (one in flight at a time, so a small ring is plenty).
const QN: u16 = 8;

#[repr(C, align(4096))]
struct Page([u8; 4096]);

// DMA memory: the ring page and the request buffer. Static (stable physical address) and
// owned by the single `VirtioRng` that `probe` hands out (`TAKEN`).
static RING: RacyCell<Page> = RacyCell::new(Page([0; 4096]));
static BUF: RacyCell<Page> = RacyCell::new(Page([0; 4096]));
static TAKEN: AtomicBool = AtomicBool::new(false);

/// A live virtio-rng device.
pub struct VirtioRng {
    mem: *mut u8,
    layout: QueueLayout,
    /// MMIO address of queue 0's doorbell.
    notify: u64,
    buf: *mut u8,
    buf_phys: u64,
    avail_idx: u16,
    last_used: u16,
    inflight: bool,
}

impl VirtioRng {
    /// Find, initialize and start the first virtio-rng device. `None` (with a log line
    /// saying why when a device exists but cannot be driven) if there is none.
    pub fn probe(phys_offset: u64) -> Option<VirtioRng> {
        let dev = pci::find_virtio_rng()?;
        serial_println!(
            "virtio-rng @ pci {:02x}:{:02x}.{} id {:04x}:{:04x}",
            dev.bus,
            dev.slot,
            dev.func,
            dev.vendor,
            dev.device
        );
        if TAKEN.swap(true, Ordering::AcqRel) {
            serial_println!("virtio-rng: already initialized");
            return None;
        }
        dev.enable_bus_master();
        let Some(caps) = virtio::discover(&dev) else {
            serial_println!("virtio-rng: no modern virtio capabilities (legacy-only device)");
            return None;
        };
        let Some(common_addr) = virtio::cap_addr(&dev, &caps.common, phys_offset) else {
            serial_println!("virtio-rng: common-config BAR missing or unmapped");
            return None;
        };
        // SAFETY: `common_addr` is BAR base + `phys_offset` + capability offset of a memory BAR,
        // checked by `cap_addr` (BAR usable, no overflow, address mapped); `discover` guarantees the
        // capability is at least `COMMON_CFG_LEN` (0x38) bytes, covering every register `Common` touches.
        let common = unsafe { Common::new(common_addr) };
        if virtio::negotiate_features(&common, 0).is_none() {
            serial_println!("virtio-rng: feature negotiation rejected (no VERSION_1?)");
            return None;
        }

        let mem = RING.get() as *mut u8;
        let buf = BUF.get() as *mut u8;
        let buf_phys = virtio::virt_to_phys(buf as u64, phys_offset)?;
        let ring_phys = virtio::virt_to_phys(mem as u64, phys_offset)?;

        common.select_queue(0);
        let Some(qsize) = virtio::validate_queue_size(common.queue_size(), QN) else {
            serial_println!("virtio-rng: queue 0 unavailable or invalid size");
            common.set_status(virtio::S_FAILED);
            return None;
        };
        let layout = QueueLayout::new(qsize)?;
        if layout.total > 4096 {
            return None;
        }
        common.set_queue_size(qsize);
        common.set_queue_desc(ring_phys + layout.desc as u64);
        common.set_queue_driver(ring_phys + layout.avail as u64);
        common.set_queue_device(ring_phys + layout.used as u64);
        let notify_off = common.queue_notify_off();
        let doorbell =
            virtio::notify_doorbell_offset(&caps.notify, caps.notify_off_mul, notify_off).and_then(
                |rel| virtio::cap_addr(&dev, &caps.notify, phys_offset)?.checked_add(rel),
            );
        let Some(notify) = doorbell else {
            serial_println!("virtio-rng: doorbell outside the notify window");
            common.set_status(virtio::S_FAILED);
            return None;
        };
        common.enable_queue();
        common.set_status(
            virtio::S_ACK | virtio::S_DRIVER | virtio::S_FEATURES_OK | virtio::S_DRIVER_OK,
        );
        serial_println!("virtio-rng: up, queue size {}", qsize);
        Some(VirtioRng {
            mem,
            layout,
            notify,
            buf,
            buf_phys,
            avail_idx: 0,
            last_used: 0,
            inflight: false,
        })
    }

    fn write_u16(&self, off: usize, v: u16) {
        // SAFETY: `off` comes from `QueueLayout`, whose parts all lie inside `layout.total <= 4096`
        // bytes (`mem` is one 4 KiB page, checked in `probe`); the offsets used are 2-aligned.
        // Volatile: the device reads it.
        unsafe { core::ptr::write_volatile(self.mem.add(off) as *mut u16, v) }
    }

    fn read_u16(&self, off: usize) -> u16 {
        // SAFETY: as in `write_u16` (inside the ring page, 2-aligned); volatile: the device writes it.
        unsafe { core::ptr::read_volatile(self.mem.add(off) as *const u16) }
    }

    fn read_u32(&self, off: usize) -> u32 {
        // SAFETY: `off` is a used-ring element field, inside the ring page and 4-aligned
        // (`used` is 4-aligned and elements are 8 bytes); volatile: the device writes it.
        unsafe { core::ptr::read_volatile(self.mem.add(off) as *const u32) }
    }

    /// Post a request for [`REQ_LEN`] bytes unless one is already in flight.
    pub fn request(&mut self) {
        if self.inflight {
            return;
        }
        let d = vn::encode_desc(self.buf_phys, REQ_LEN as u32, vn::DESC_F_WRITE, 0);
        let lo = u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        let hi = u64::from_le_bytes([d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]]);
        let off = self.layout.desc_off(0);
        // SAFETY: `desc_off(0)` is the start of the descriptor table inside the ring page and 16-aligned,
        // so both 8-byte halves are in bounds and aligned; volatile: the device DMA-reads them.
        unsafe {
            let p = self.mem.add(off);
            core::ptr::write_volatile(p as *mut u64, lo);
            core::ptr::write_volatile(p.add(8) as *mut u64, hi);
        }
        let slot = self.avail_idx % self.layout.qsize;
        self.write_u16(self.layout.avail_ring_off(slot), 0);
        // The ring entry must be visible before the index that publishes it.
        fence(Ordering::Release);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        self.write_u16(self.layout.avail_idx_off(), self.avail_idx);
        fence(Ordering::SeqCst);
        // SAFETY: `notify` is queue 0's doorbell: notify-BAR base + `queue_notify_off * mul` reached through
        // the linear physical map; `probe` checked that the 16-bit doorbell lies inside the notify
        // capability and that the address is mapped. 16-bit volatile MMIO write.
        unsafe { core::ptr::write_volatile(self.notify as *mut u16, 0) };
        self.inflight = true;
    }

    /// The bytes of the finished request, if there is one: `(buffer, valid_len)` with
    /// `1 <= valid_len <= REQ_LEN`. `None` while the device has not answered (or when nothing
    /// was requested). A completion that makes no sense (impossible index, wrong id, zero
    /// length) is dropped, never returned.
    pub fn poll(&mut self) -> Option<([u8; REQ_LEN], usize)> {
        if !self.inflight {
            return None;
        }
        match vn::used_pending(
            self.read_u16(self.layout.used_idx_off()),
            self.last_used,
            self.layout.qsize,
        ) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(_) => {
                // Impossible index: resynchronize and ask again later.
                self.last_used = self.read_u16(self.layout.used_idx_off());
                self.inflight = false;
                return None;
            }
        }
        // The device wrote the element before bumping `used.idx`, which we just read.
        fence(Ordering::Acquire);
        let off = self
            .layout
            .used_elem_off(self.last_used % self.layout.qsize);
        let id = self.read_u32(off);
        let len = self.read_u32(off + 4) as usize;
        self.last_used = self.last_used.wrapping_add(1);
        self.inflight = false;
        if id != 0 || len == 0 {
            return None;
        }
        let n = len.min(REQ_LEN);
        let mut out = [0u8; REQ_LEN];
        // SAFETY: `buf` is the start of the BUF page (4096 bytes) this driver owns; `n <= REQ_LEN = 32`
        // is far inside it. The device wrote the buffer before `used.idx` advanced (acquire fence above).
        unsafe { core::ptr::copy_nonoverlapping(self.buf, out.as_mut_ptr(), n) };
        Some((out, n))
    }
}
