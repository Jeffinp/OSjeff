//! virtio-net driver (virtio 1.0 PCI transport, polled, no offloads).
//!
//! QEMU: `-device virtio-net-pci,netdev=n0,mac=52:54:00:12:34:56`. The device shows
//! up as `1af4:1000` (transitional, QEMU's default) or `1af4:1041` (modern); both
//! expose the modern capabilities, which is what this driver uses, so a legacy-only
//! device (`disable-modern=on`, no capabilities) is refused with a log line.
//!
//! Layout: queue 0 is the receive queue, queue 1 the transmit queue, each a split
//! virtqueue of at most [`QN`] entries in its own 4 KiB page, with one descriptor
//! per 2 KiB DMA buffer (so a buffer never straddles a page and its physical
//! address is a single translation). The receive queue is filled with writable
//! buffers up front and every completed buffer is re-posted at once; the transmit
//! queue copies each frame into a free buffer slot and posts it, reclaiming
//! completed slots lazily. Everything arithmetic (ring offsets, index wrap, header,
//! length rules) is in `kitsune_core::hw::virtio_net`, unit-tested on the host.
//!
//! Features negotiated: `VERSION_1`, `MAC` and `STATUS` only (no checksum or GSO
//! offload, no mergeable buffers, no multiqueue), so each received frame is exactly
//! one descriptor behind a 12-byte header.

use crate::nic::{Nic, STATS, TxError};
use crate::sync::RacyCell;
use crate::virtio::{self, Common};
use crate::{pci, serial_println};
use core::sync::atomic::{AtomicBool, Ordering, fence};
use kitsune_core::hw::virtio_net::{
    self as vn, BUF_LEN, HDR_LEN, NetHdr, QueueLayout, Rx, SlotSet, WANTED_FEATURES_LO,
};
use kitsune_core::net::Mac;
use kitsune_core::netstats::NicKind;

/// Entries per queue (and DMA buffers per direction) the driver provides.
const QN: usize = 16;

/// Spin bound while waiting for a free transmit buffer: a device that never
/// completes a send is reported as a transmit error instead of hanging the caller.
const TX_WAIT_SPINS: u32 = 2_000_000;

#[repr(C, align(4096))]
struct Page([u8; 4096]);

#[repr(C, align(4096))]
struct Bufs([u8; QN * BUF_LEN]);

// DMA memory: one page per queue's rings, one block of buffers per direction. Static
// (so physical addresses are stable) and owned by the single `VirtioNet` that `probe`
// hands out (`TAKEN`).
static RX_RING: RacyCell<Page> = RacyCell::new(Page([0; 4096]));
static TX_RING: RacyCell<Page> = RacyCell::new(Page([0; 4096]));
static RX_BUFS: RacyCell<Bufs> = RacyCell::new(Bufs([0; QN * BUF_LEN]));
static TX_BUFS: RacyCell<Bufs> = RacyCell::new(Bufs([0; QN * BUF_LEN]));
/// Set by the first `probe` that gets as far as touching the statics above.
static TAKEN: AtomicBool = AtomicBool::new(false);

/// One split virtqueue inside a 4 KiB ring page.
struct Vq {
    mem: *mut u8,
    layout: QueueLayout,
    /// MMIO address of this queue's doorbell.
    notify: u64,
    /// Queue index written to the doorbell.
    index: u16,
    avail_idx: u16,
    last_used: u16,
}

impl Vq {
    fn qsize(&self) -> u16 {
        self.layout.qsize
    }

    fn write_u16(&self, off: usize, v: u16) {
        // SAFETY: `off` comes from `QueueLayout`, whose parts all lie inside `layout.total`
        // bytes, and `total <= 4096` was checked when the queue was built (`mem` is one
        // 4 KiB page); the offsets used with this are 2-aligned. Volatile: the device reads it.
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

    /// Write descriptor `i`: `{addr, len, flags, next}`.
    fn write_desc(&self, i: u16, addr: u64, len: u32, flags: u16) {
        let d = vn::encode_desc(addr, len, flags, 0);
        let lo = u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        let hi = u64::from_le_bytes([d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]]);
        let off = self.layout.desc_off(i);
        // SAFETY: `desc_off` is inside the descriptor table (inside the ring page) and 16-aligned,
        // so both 8-byte halves are in bounds and aligned; volatile: the device DMA-reads them.
        unsafe {
            let p = self.mem.add(off);
            core::ptr::write_volatile(p as *mut u64, lo);
            core::ptr::write_volatile(p.add(8) as *mut u64, hi);
        }
    }

    /// Make descriptor `head` available to the device.
    fn push_avail(&mut self, head: u16) {
        let slot = self.avail_idx % self.qsize();
        self.write_u16(self.layout.avail_ring_off(slot), head);
        // The ring entry must be visible before the index that publishes it.
        fence(Ordering::Release);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        self.write_u16(self.layout.avail_idx_off(), self.avail_idx);
    }

    fn used_idx(&self) -> u16 {
        self.read_u16(self.layout.used_idx_off())
    }

    /// Completions waiting, or `Err` if the device's index is impossible.
    fn pending(&self) -> Result<u16, vn::RingCorrupt> {
        vn::used_pending(self.used_idx(), self.last_used, self.qsize())
    }

    /// Take the oldest completion: `(id, len)` as the device wrote them.
    fn take_used(&mut self) -> (u32, u32) {
        // The device wrote the element before bumping `used.idx`, which we just read.
        fence(Ordering::Acquire);
        let slot = self.last_used % self.qsize();
        let off = self.layout.used_elem_off(slot);
        let id = self.read_u32(off);
        let len = self.read_u32(off + 4);
        self.last_used = self.last_used.wrapping_add(1);
        (id, len)
    }

    /// Ring the doorbell.
    fn kick(&self) {
        fence(Ordering::SeqCst);
        // SAFETY: `notify` is this queue's doorbell: notify-BAR base + `queue_notify_off * mul`
        // reached through the linear physical map; `probe` checked that the 16-bit doorbell lies
        // inside the notify capability and that the address is mapped. 16-bit volatile MMIO write.
        unsafe { core::ptr::write_volatile(self.notify as *mut u16, self.index) }
    }
}

/// A live virtio-net device.
pub struct VirtioNet {
    rx: Vq,
    tx: Vq,
    rx_bufs: *mut u8,
    tx_bufs: *mut u8,
    rx_phys: [u64; QN],
    tx_phys: [u64; QN],
    tx_slots: SlotSet,
    features_lo: u32,
    mac: Mac,
    /// Virtual address of the device-specific config (MAC at 0, link status at 6), if advertised.
    dev_cfg: Option<u64>,
}

impl VirtioNet {
    /// Find, initialize and start the first virtio-net device. `None` (with a log
    /// line saying why) if there is none or it cannot be driven.
    pub fn probe(phys_offset: u64) -> Option<VirtioNet> {
        let dev = pci::find_virtio_net()?;
        serial_println!(
            "virtio-net @ pci {:02x}:{:02x}.{} id {:04x}:{:04x}",
            dev.bus,
            dev.slot,
            dev.func,
            dev.vendor,
            dev.device
        );
        // The statics below are handed to exactly one driver instance.
        if TAKEN.swap(true, Ordering::AcqRel) {
            serial_println!("virtio-net: already initialized");
            return None;
        }
        dev.enable_bus_master();
        let Some(caps) = virtio::discover(&dev) else {
            serial_println!("virtio-net: no modern virtio capabilities (legacy-only device)");
            return None;
        };
        let Some(common_addr) = virtio::cap_addr(&dev, &caps.common, phys_offset) else {
            serial_println!("virtio-net: common-config BAR missing or unmapped");
            return None;
        };
        // SAFETY: `common_addr` is BAR base + `phys_offset` + capability offset of a memory BAR,
        // checked by `cap_addr` (BAR usable, no overflow, address mapped); `discover` guarantees the
        // capability is at least `COMMON_CFG_LEN` (0x38) bytes, covering every register `Common` touches.
        let common = unsafe { Common::new(common_addr) };

        let Some(features_lo) = virtio::negotiate_features(&common, WANTED_FEATURES_LO) else {
            serial_println!("virtio-net: feature negotiation failed (no VERSION_1?)");
            return None;
        };

        // DMA memory (zeroed statics): translate every buffer; give up if any is unmapped.
        let rx_ring = RX_RING.get() as *mut u8;
        let tx_ring = TX_RING.get() as *mut u8;
        let rx_bufs = RX_BUFS.get() as *mut u8;
        let tx_bufs = TX_BUFS.get() as *mut u8;
        let mut rx_phys = [0u64; QN];
        let mut tx_phys = [0u64; QN];
        for i in 0..QN {
            // SAFETY: `i < QN`, so `i * BUF_LEN` is inside each `QN * BUF_LEN` static block.
            let (rv, tv) = unsafe {
                (
                    rx_bufs.add(i * BUF_LEN) as u64,
                    tx_bufs.add(i * BUF_LEN) as u64,
                )
            };
            rx_phys[i] = virtio::virt_to_phys(rv, phys_offset)?;
            tx_phys[i] = virtio::virt_to_phys(tv, phys_offset)?;
        }

        let rx = setup_queue(&common, &dev, &caps, phys_offset, vn::RX_QUEUE, rx_ring)?;
        let tx = setup_queue(&common, &dev, &caps, phys_offset, vn::TX_QUEUE, tx_ring)?;

        let dev_cfg = if caps.device.present() {
            virtio::cap_addr(&dev, &caps.device, phys_offset)
        } else {
            None
        };
        let mac = read_mac(dev_cfg, features_lo).unwrap_or(crate::ne2000::MAC);

        let n_rx = rx.qsize();
        let tx_n = usize::from(tx.qsize());
        let mut nic = VirtioNet {
            rx,
            tx,
            rx_bufs,
            tx_bufs,
            rx_phys,
            tx_phys,
            tx_slots: SlotSet::new(tx_n),
            features_lo,
            mac,
            dev_cfg,
        };

        // Receive buffers: one writable descriptor each, all posted before DRIVER_OK.
        for i in 0..n_rx {
            nic.rx.write_desc(
                i,
                nic.rx_phys[usize::from(i)],
                BUF_LEN as u32,
                vn::DESC_F_WRITE,
            );
            nic.rx.push_avail(i);
        }
        // Transmit descriptors point at their slot's buffer for good; only `len` changes.
        for i in 0..nic.tx.qsize() {
            nic.tx.write_desc(i, nic.tx_phys[usize::from(i)], 0, 0);
        }

        common.set_status(
            virtio::S_ACK | virtio::S_DRIVER | virtio::S_FEATURES_OK | virtio::S_DRIVER_OK,
        );
        nic.rx.kick();
        serial_println!(
            "virtio-net: up, features {:#x}, rx/tx queue {}/{}, link {}",
            features_lo,
            nic.rx.qsize(),
            nic.tx.qsize(),
            if nic.link_up() { "up" } else { "down" }
        );
        Some(nic)
    }

    /// Hand completed transmit buffers back to the slot allocator.
    fn reclaim_tx(&mut self) {
        loop {
            match self.tx.pending() {
                Ok(0) => return,
                Ok(_) => {
                    let (id, _len) = self.tx.take_used();
                    let freed = vn::decode_used(id, 0, self.tx.qsize())
                        .is_some_and(|(head, _)| self.tx_slots.free(usize::from(head)));
                    if !freed {
                        STATS.on_tx_error(); // the device completed something we never posted
                    }
                }
                Err(_) => {
                    // Impossible index: resynchronize rather than loop on garbage.
                    STATS.on_tx_error();
                    self.tx.last_used = self.tx.used_idx();
                    return;
                }
            }
        }
    }
}

/// Program queue `q` (select, size, ring addresses, enable) over `ring`, one 4 KiB page.
fn setup_queue(
    common: &Common,
    dev: &pci::PciDevice,
    caps: &virtio::VirtioCaps,
    phys_offset: u64,
    q: u16,
    ring: *mut u8,
) -> Option<Vq> {
    common.select_queue(q);
    let Some(qsize) = virtio::validate_queue_size(common.queue_size(), QN as u16) else {
        serial_println!("virtio-net: queue {} unavailable or invalid size", q);
        common.set_status(virtio::S_FAILED);
        return None;
    };
    let layout = QueueLayout::new(qsize)?;
    if layout.total > 4096 {
        return None;
    }
    let phys = virtio::virt_to_phys(ring as u64, phys_offset)?;
    common.set_queue_size(qsize);
    common.set_queue_desc(phys + layout.desc as u64);
    common.set_queue_driver(phys + layout.avail as u64);
    common.set_queue_device(phys + layout.used as u64);
    let notify_off = common.queue_notify_off();
    let doorbell = virtio::notify_doorbell_offset(&caps.notify, caps.notify_off_mul, notify_off)
        .and_then(|rel| virtio::cap_addr(dev, &caps.notify, phys_offset)?.checked_add(rel));
    let Some(notify) = doorbell else {
        serial_println!("virtio-net: queue {} doorbell outside the notify window", q);
        common.set_status(virtio::S_FAILED);
        return None;
    };
    common.enable_queue();
    Some(Vq {
        mem: ring,
        layout,
        notify,
        index: q,
        avail_idx: 0,
        last_used: 0,
    })
}

/// The device's MAC from its config window, if it offered one.
fn read_mac(dev_cfg: Option<u64>, features_lo: u32) -> Option<Mac> {
    let base = dev_cfg?;
    let mut b = [0u8; 6];
    for (i, v) in b.iter_mut().enumerate() {
        // SAFETY: the device-specific config capability is at least 6 bytes when `F_MAC` is set (the
        // spec places the MAC at offset 0); `cap_addr` checked the address is mapped; byte-wise
        // volatile MMIO reads.
        *v = unsafe { core::ptr::read_volatile((base + i as u64) as *const u8) };
    }
    vn::config_mac(features_lo, &b)
}

impl Nic for VirtioNet {
    fn kind(&self) -> NicKind {
        NicKind::VirtioNet
    }

    fn mac(&self) -> Mac {
        self.mac
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), TxError> {
        let total = vn::tx_total(frame.len()).ok_or(TxError::BadLength)?;
        self.reclaim_tx();
        let mut spins = 0u32;
        let slot = loop {
            if let Some(s) = self.tx_slots.alloc() {
                break s;
            }
            self.reclaim_tx();
            spins += 1;
            if spins > TX_WAIT_SPINS {
                return Err(TxError::Full);
            }
            core::hint::spin_loop();
        };
        // SAFETY: `slot < tx_slots.n <= qsize <= QN`, so `slot * BUF_LEN .. + BUF_LEN` is inside the
        // TX_BUFS block this driver exclusively owns (TAKEN); `total <= BUF_LEN` (tx_total's bound, checked
        // by a const assert in the core), so header + frame + padding fit the buffer.
        let buf =
            unsafe { core::slice::from_raw_parts_mut(self.tx_bufs.add(slot * BUF_LEN), total) };
        NetHdr::plain().encode(buf);
        buf[HDR_LEN..HDR_LEN + frame.len()].copy_from_slice(frame);
        buf[HDR_LEN + frame.len()..].fill(0); // pad to the Ethernet minimum
        self.tx
            .write_desc(slot as u16, self.tx_phys[slot], total as u32, 0);
        self.tx.push_avail(slot as u16);
        self.tx.kick();
        Ok(())
    }

    fn poll(&mut self, out: &mut [u8]) -> Option<usize> {
        loop {
            match self.rx.pending() {
                Ok(0) => return None,
                Err(_) => {
                    STATS.on_rx_error();
                    self.rx.last_used = self.rx.used_idx();
                    return None;
                }
                Ok(_) => {}
            }
            let (id, len) = self.rx.take_used();
            let Some((head, len)) = vn::decode_used(id, len, self.rx.qsize()) else {
                STATS.on_rx_error(); // a completion for a descriptor that does not exist
                continue;
            };
            let got = match vn::rx_frame(len, out.len()) {
                Rx::Runt => {
                    STATS.on_rx_error();
                    None
                }
                Rx::Frame { copy, truncated } => {
                    // SAFETY: `head < qsize <= QN` (decode_used), so the buffer is inside RX_BUFS, which this
                    // driver exclusively owns; `rx_frame` bounds `copy` by `BUF_LEN - HDR_LEN`, so the
                    // source range stays inside that buffer, and by `out.len()`, so the destination fits.
                    // The device wrote the buffer before `used.idx` advanced (acquire fence in `take_used`).
                    unsafe {
                        let src = self.rx_bufs.add(usize::from(head) * BUF_LEN + HDR_LEN);
                        core::ptr::copy_nonoverlapping(src, out.as_mut_ptr(), copy);
                    }
                    if truncated {
                        STATS.on_rx_dropped();
                    }
                    Some(copy)
                }
            };
            // Give the buffer back to the device immediately.
            self.rx.push_avail(head);
            self.rx.kick();
            if got.is_some() {
                return got;
            }
        }
    }

    fn link_up(&self) -> bool {
        let Some(base) = self.dev_cfg else {
            return true;
        };
        if self.features_lo & vn::F_STATUS == 0 {
            return true; // the device does not report link state
        }
        // SAFETY: the status word sits at offset 6 of the device config (2-aligned) and is only read
        // when the device negotiated `F_STATUS`, i.e. it exists; `cap_addr` checked the window is
        // mapped; 16-bit volatile MMIO read.
        let status = unsafe { core::ptr::read_volatile((base + 6) as *const u16) };
        vn::link_up(self.features_lo, status)
    }
}
