//! virtio-gpu 2D driver — control virtqueue + command submission.
//!
//! Builds on [`crate::virtio`] (transport + feature negotiation). Sets up the
//! single control queue with DMA rings in a page whose physical address the
//! device can reach, flips DRIVER_OK, and submits commands by chaining two
//! descriptors (request + response) and polling the used ring. The first command
//! exercised is GET_DISPLAY_INFO, which verifies the whole path end to end.

use crate::serial_println;
use crate::sync::RacyCell;
use crate::virtio::{self, CapLoc, Common, VirtioCaps};
use core::sync::atomic::{Ordering, compiler_fence};

const QSIZE: u16 = 16;

// Split-virtqueue sub-structure offsets inside QUEUE_MEM (all in one 4 KiB page,
// each part separately addressable in virtio 1.0).
const DESC_OFF: usize = 0; // 16 descriptors * 16 bytes = 256
const AVAIL_OFF: usize = 256; // flags(2) idx(2) ring[QSIZE](2 each)
const USED_OFF: usize = 512; // flags(2) idx(2) ring[QSIZE](id:4,len:4)

// Descriptor flags.
const F_NEXT: u16 = 1;
const F_WRITE: u16 = 2;

// virtio-gpu control commands / responses.
const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;

#[repr(C, align(4096))]
struct DmaPage([u8; 4096]);

// One page for the queue rings, one for the request+response command buffers.
static QUEUE_MEM: RacyCell<DmaPage> = RacyCell::new(DmaPage([0; 4096]));
static CMD_MEM: RacyCell<DmaPage> = RacyCell::new(DmaPage([0; 4096]));
const RESP_OFF: usize = 2048; // response buffer lives in the second half

/// A live virtio-gpu device with its control queue ready.
pub struct GpuDevice {
    qsize: u16,
    queue_virt: *mut u8,
    notify_addr: u64,
    cmd_virt: *mut u8,
    cmd_phys: u64,
    resp_phys: u64,
    avail_idx: u16,
    last_used: u16,
}

impl GpuDevice {
    /// Initialize the device: negotiate, set up the control queue with DMA
    /// rings, and flip DRIVER_OK. `None` if anything looks wrong.
    pub fn init(gpu: &crate::pci::PciDevice, caps: VirtioCaps, phys_offset: u64) -> Option<GpuDevice> {
        let common_addr =
            virtio::bar_base(gpu, caps.common.bar) + phys_offset + caps.common.offset as u64;
        let common = unsafe { Common::new(common_addr) };

        if !virtio::negotiate(&common) {
            serial_println!("virtio-gpu: feature negotiation failed");
            return None;
        }

        let queue_virt = QUEUE_MEM.get() as *mut u8;
        let cmd_virt = CMD_MEM.get() as *mut u8;
        let queue_phys = virtio::virt_to_phys(queue_virt as u64, phys_offset)?;
        let cmd_phys = virtio::virt_to_phys(cmd_virt as u64, phys_offset)?;
        let resp_phys = cmd_phys + RESP_OFF as u64;

        // Program the control queue (index 0).
        common.select_queue(0);
        let qsize = common.queue_size().min(QSIZE);
        common.set_queue_size(qsize);
        common.set_queue_desc(queue_phys + DESC_OFF as u64);
        common.set_queue_driver(queue_phys + AVAIL_OFF as u64);
        common.set_queue_device(queue_phys + USED_OFF as u64);
        common.enable_queue();

        let notify_off = common.queue_notify_off();
        let notify_addr = notify_base(gpu, &caps.notify, phys_offset)
            + notify_off as u64 * caps.notify_off_mul as u64;

        // Driver is fully up.
        common.set_status(virtio::S_ACK | virtio::S_DRIVER | virtio::S_FEATURES_OK | virtio::S_DRIVER_OK);

        Some(GpuDevice {
            qsize,
            queue_virt,
            notify_addr,
            cmd_virt,
            cmd_phys,
            resp_phys,
            avail_idx: 0,
            last_used: 0,
        })
    }

    // ---- ring accessors (volatile; the device reads/writes these too) ----

    fn write_desc(&self, i: usize, addr: u64, len: u32, flags: u16, next: u16) {
        let d = unsafe { self.queue_virt.add(DESC_OFF + i * 16) };
        unsafe {
            core::ptr::write_volatile(d as *mut u64, addr);
            core::ptr::write_volatile(d.add(8) as *mut u32, len);
            core::ptr::write_volatile(d.add(12) as *mut u16, flags);
            core::ptr::write_volatile(d.add(14) as *mut u16, next);
        }
    }

    fn avail_set(&self, slot: u16, desc: u16) {
        let p = unsafe { self.queue_virt.add(AVAIL_OFF + 4 + (slot as usize) * 2) };
        unsafe { core::ptr::write_volatile(p as *mut u16, desc) };
    }
    fn avail_publish(&self, idx: u16) {
        let p = unsafe { self.queue_virt.add(AVAIL_OFF + 2) };
        unsafe { core::ptr::write_volatile(p as *mut u16, idx) };
    }
    fn used_idx(&self) -> u16 {
        let p = unsafe { self.queue_virt.add(USED_OFF + 2) };
        unsafe { core::ptr::read_volatile(p as *const u16) }
    }

    /// Submit a request (bytes) and wait (bounded) for the device to write a
    /// `resp_len`-byte response. Returns the response's leading u32 (its type),
    /// or `None` on timeout. The response bytes live at `CMD_MEM[RESP_OFF..]`.
    fn submit(&mut self, req: &[u8], resp_len: u32) -> Option<u32> {
        unsafe {
            core::ptr::copy_nonoverlapping(req.as_ptr(), self.cmd_virt, req.len());
        }
        self.write_desc(0, self.cmd_phys, req.len() as u32, F_NEXT, 1);
        self.write_desc(1, self.resp_phys, resp_len, F_WRITE, 0);

        let slot = self.avail_idx % self.qsize;
        self.avail_set(slot, 0); // head descriptor index
        compiler_fence(Ordering::SeqCst);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        self.avail_publish(self.avail_idx);
        compiler_fence(Ordering::SeqCst);

        // Notify the device that queue 0 has a new buffer.
        unsafe { core::ptr::write_volatile(self.notify_addr as *mut u16, 0) };

        // Poll the used ring (bounded).
        for _ in 0..50_000_000u64 {
            if self.used_idx() != self.last_used {
                self.last_used = self.used_idx();
                let resp = unsafe { self.cmd_virt.add(RESP_OFF) };
                return Some(unsafe { core::ptr::read_volatile(resp as *const u32) });
            }
            core::hint::spin_loop();
        }
        None
    }

    /// Ask the device for the display geometry — exercises the full command path.
    /// Returns `(width, height)` of display 0, or `None`.
    pub fn get_display_info(&mut self) -> Option<(u32, u32)> {
        let mut req = [0u8; 24]; // bare control header
        req[0..4].copy_from_slice(&CMD_GET_DISPLAY_INFO.to_le_bytes());
        let resp_type = self.submit(&req, 408)?; // header + 16 displays * 24
        if resp_type != RESP_OK_DISPLAY_INFO {
            serial_println!("virtio-gpu: display info resp {:#x}", resp_type);
            return None;
        }
        // Response: 24-byte header, then display[0] = { rect{x,y,w,h u32}, ... }.
        let resp = unsafe { self.cmd_virt.add(RESP_OFF) };
        let w = unsafe { core::ptr::read_volatile(resp.add(24 + 8) as *const u32) };
        let h = unsafe { core::ptr::read_volatile(resp.add(24 + 12) as *const u32) };
        Some((w, h))
    }
}

/// Virtual address of the notify region for this device.
fn notify_base(gpu: &crate::pci::PciDevice, notify: &CapLoc, phys_offset: u64) -> u64 {
    virtio::bar_base(gpu, notify.bar) + phys_offset + notify.offset as u64
}
