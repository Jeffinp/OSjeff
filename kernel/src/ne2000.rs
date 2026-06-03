//! NE2000 (DP8390) ISA NIC driver — polled, byte-wide remote DMA.
//!
//! The NE2000 is the simplest NIC to drive: plain port I/O, no PCI enumeration
//! and no DMA descriptor rings. We poll (no IRQ): the compositor loop calls
//! [`poll`] each wake and transmits replies with [`send`]. Every wait is bounded
//! and [`init`] returns `false` when no card answers, so a build with no NIC
//! simply runs without networking.
//!
//! QEMU: `-device ne2k_isa,netdev=...,mac=52:54:00:12:34:56` (I/O base 0x300).

use crate::io::{inb, outb};
use osjeff_core::net::Mac;

const IO: u16 = 0x300; // ISA I/O base (QEMU ne2k_isa default)
const DATA: u16 = IO + 0x10; // NE2000 data port (remote DMA window)
const RESET: u16 = IO + 0x1F;

// Page 0 registers (offsets from IO).
const CR: u16 = 0x00;
const PSTART: u16 = 0x01;
const PSTOP: u16 = 0x02;
const BNRY: u16 = 0x03;
const TPSR: u16 = 0x04;
const TBCR0: u16 = 0x05;
const TBCR1: u16 = 0x06;
const ISR: u16 = 0x07;
const RSAR0: u16 = 0x08;
const RSAR1: u16 = 0x09;
const RBCR0: u16 = 0x0A;
const RBCR1: u16 = 0x0B;
const RCR: u16 = 0x0C;
const TCR: u16 = 0x0D;
const DCR: u16 = 0x0E;
const IMR: u16 = 0x0F;
// Page 1.
const PAR0: u16 = 0x01;
const CURR: u16 = 0x07;

// Command register bits.
const CR_STOP: u8 = 0x01;
const CR_START: u8 = 0x02;
const CR_TXP: u8 = 0x04;
const CR_RD_READ: u8 = 0x08;
const CR_RD_WRITE: u8 = 0x10;
const CR_RD_ABORT: u8 = 0x20;
const CR_PAGE1: u8 = 0x40;

const ISR_RDC: u8 = 0x40; // remote DMA complete
const ISR_RST: u8 = 0x80; // reset status

// Receive ring buffer pages (256 bytes each) in the chip's 16 KiB.
const TX_PAGE: u8 = 0x40;
const RX_START: u8 = 0x46;
const RX_STOP: u8 = 0x80;

const SPIN: u32 = 1_000_000;

/// Our hardware address (must match the QEMU `mac=` option).
pub const MAC: Mac = Mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);

/// Next ring page to read (the software read pointer).
static mut NEXT: u8 = RX_START + 1;

#[inline]
fn r(reg: u16) -> u8 {
    inb(IO + reg)
}
#[inline]
fn w(reg: u16, v: u8) {
    outb(IO + reg, v);
}

/// Reset and configure the NIC. Returns `false` if no card responds.
pub fn init() -> bool {
    // Pulse reset and wait (bounded) for the chip to acknowledge.
    outb(RESET, inb(RESET));
    let mut ok = false;
    for _ in 0..SPIN {
        if r(ISR) & ISR_RST != 0 {
            ok = true;
            break;
        }
    }
    if !ok {
        return false; // no NIC present
    }

    w(CR, CR_STOP | CR_RD_ABORT); // stop, page 0
    w(DCR, 0x48); // byte-wide DMA, normal operation
    w(RBCR0, 0);
    w(RBCR1, 0);
    w(RCR, 0x20); // monitor mode while configuring
    w(TCR, 0x02); // internal loopback while configuring
    w(TPSR, TX_PAGE);
    w(PSTART, RX_START);
    w(BNRY, RX_START);
    w(PSTOP, RX_STOP);
    w(ISR, 0xFF); // clear pending status
    w(IMR, 0x00); // polled: mask all interrupts

    // Page 1: program our MAC into PAR0..5 and set CURR.
    w(CR, CR_PAGE1 | CR_STOP | CR_RD_ABORT);
    for (i, &b) in MAC.0.iter().enumerate() {
        w(PAR0 + i as u16, b);
    }
    w(CURR, RX_START + 1);

    // Back to page 0, go live: accept broadcast + unicast-to-us.
    w(CR, CR_STOP | CR_RD_ABORT);
    w(TCR, 0x00); // normal transmit
    w(RCR, 0x04); // accept broadcast (unicast matches PAR)
    w(CR, CR_START | CR_RD_ABORT); // start

    unsafe { NEXT = RX_START + 1 };
    true
}

/// Remote-DMA read `buf.len()` bytes from chip address `src`.
fn dma_read(src: u16, buf: &mut [u8]) {
    let len = buf.len() as u16;
    w(CR, CR_START | CR_RD_ABORT);
    w(RBCR0, (len & 0xFF) as u8);
    w(RBCR1, (len >> 8) as u8);
    w(RSAR0, (src & 0xFF) as u8);
    w(RSAR1, (src >> 8) as u8);
    w(CR, CR_START | CR_RD_READ);
    for b in buf.iter_mut() {
        *b = inb(DATA);
    }
}

/// Receive one frame into `buf`, returning its length, or `None` if the ring is
/// empty. Each ring entry is a 4-byte header (status, next page, len lo/hi)
/// followed by the frame.
pub fn poll(buf: &mut [u8]) -> Option<usize> {
    // Read CURR from page 1.
    w(CR, CR_PAGE1 | CR_START | CR_RD_ABORT);
    let curr = r(CURR);
    w(CR, CR_START | CR_RD_ABORT);

    let next = unsafe { NEXT };
    if next == curr {
        return None; // ring empty
    }

    let mut hdr = [0u8; 4];
    dma_read((next as u16) << 8, &mut hdr);
    let next_page = hdr[1];
    let total = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;

    // Sanity-check the length; on garbage, drop the whole ring to resync.
    if !(4..=1518 + 4).contains(&total) || !(RX_START..=RX_STOP).contains(&next_page) {
        unsafe { NEXT = curr };
        let bnry = if curr == RX_START {
            RX_STOP - 1
        } else {
            curr - 1
        };
        w(BNRY, bnry);
        return None;
    }

    let data_len = total - 4;
    let n = data_len.min(buf.len());
    dma_read(((next as u16) << 8) + 4, &mut buf[..n]);

    // Advance the read pointer and the hardware boundary.
    unsafe { NEXT = next_page };
    let bnry = if next_page == RX_START {
        RX_STOP - 1
    } else {
        next_page - 1
    };
    w(BNRY, bnry);
    Some(n)
}

/// Transmit `frame` (padded to the 60-byte Ethernet minimum).
pub fn send(frame: &[u8]) {
    let len = frame.len().max(60);

    // Remote-DMA write the frame into the transmit page.
    w(CR, CR_START | CR_RD_ABORT);
    w(ISR, ISR_RDC); // clear remote-DMA-complete
    w(RBCR0, (len & 0xFF) as u8);
    w(RBCR1, (len >> 8) as u8);
    w(RSAR0, 0x00);
    w(RSAR1, TX_PAGE);
    w(CR, CR_START | CR_RD_WRITE);
    for i in 0..len {
        outb(DATA, frame.get(i).copied().unwrap_or(0));
    }
    for _ in 0..SPIN {
        if r(ISR) & ISR_RDC != 0 {
            break;
        }
    }

    // Issue the transmit.
    w(TPSR, TX_PAGE);
    w(TBCR0, (len & 0xFF) as u8);
    w(TBCR1, (len >> 8) as u8);
    w(CR, CR_START | CR_TXP | CR_RD_ABORT);
}
