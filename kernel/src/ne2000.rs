//! NE2000 (DP8390) ISA NIC driver — polled, byte-wide remote DMA.
//!
//! The NE2000 is the simplest NIC to drive: plain port I/O, no PCI enumeration
//! and no DMA descriptor rings. We poll (no IRQ): the network owner calls
//! [`Nic::poll`] and transmits with [`Nic::send`]. Every wait is bounded and
//! [`Ne2000::probe`] returns `None` when no card answers, so a build with no NIC
//! simply runs without networking. The driver is a value ([`Ne2000`]) that holds
//! the ring read pointer, and `nic::Port` owns it: no second handle exists.
//!
//! QEMU: `-device ne2k_isa,netdev=...,mac=52:54:00:12:34:56` (I/O base 0x300).

use crate::io::{inb, outb};
use crate::nic::{Nic, STATS, TxError};
use osjeff_core::net::{Mac, ring_prev_page, tx_len};
use osjeff_core::netstats::NicKind;

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

/// A probed NE2000. Holds the software ring read pointer, so it cannot be
/// duplicated: `probe` is called once, at boot, and the result is moved into the
/// network owner.
pub struct Ne2000 {
    /// Next ring page to read.
    next: u8,
}

impl Ne2000 {
    /// Reset and configure the card; `None` if none answers.
    pub fn probe() -> Option<Ne2000> {
        init().then_some(Ne2000 { next: RX_START + 1 })
    }
}

impl Nic for Ne2000 {
    fn kind(&self) -> NicKind {
        NicKind::Ne2000
    }
    fn mac(&self) -> Mac {
        MAC
    }
    fn send(&mut self, frame: &[u8]) -> Result<(), TxError> {
        if frame.is_empty() || frame.len() > osjeff_core::net::MAX_TX_FRAME {
            return Err(TxError::BadLength);
        }
        if send(frame) {
            Ok(())
        } else {
            Err(TxError::Full)
        }
    }
    fn poll(&mut self, buf: &mut [u8]) -> Option<usize> {
        poll(&mut self.next, buf)
    }
    fn link_up(&self) -> bool {
        true // the DP8390 has no link-status register we read
    }
}

#[inline]
fn r(reg: u16) -> u8 {
    inb(IO + reg)
}
#[inline]
fn w(reg: u16, v: u8) {
    outb(IO + reg, v);
}

/// Reset and configure the NIC. Returns `false` if no card responds.
fn init() -> bool {
    // Pulse reset and wait (bounded) for the chip to acknowledge.
    outb(RESET, inb(RESET));
    let mut ok = false;
    for _ in 0..SPIN {
        let isr = r(ISR);
        // An empty ISA slot reads as a floating bus (0xFF), which also has the reset bit set.
        if isr & ISR_RST != 0 && isr != 0xFF {
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
fn poll(next_page_ptr: &mut u8, buf: &mut [u8]) -> Option<usize> {
    // Read CURR from page 1.
    w(CR, CR_PAGE1 | CR_START | CR_RD_ABORT);
    let curr = r(CURR);
    w(CR, CR_START | CR_RD_ABORT);

    let next = *next_page_ptr;
    if next == curr {
        return None; // ring empty
    }

    let mut hdr = [0u8; 4];
    dma_read((next as u16) << 8, &mut hdr);
    let next_page = hdr[1];
    let total = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;

    // Sanity-check the length; on garbage, drop the whole ring to resync.
    if !(4..=1518 + 4).contains(&total) || !(RX_START..=RX_STOP).contains(&next_page) {
        *next_page_ptr = curr;
        STATS.on_rx_error();
        w(BNRY, ring_prev_page(curr, RX_START, RX_STOP));
        return None;
    }

    let data_len = total - 4;
    let n = data_len.min(buf.len());
    if n < data_len {
        STATS.on_rx_dropped(); // truncated: the caller's buffer was too small
    }
    dma_read(((next as u16) << 8) + 4, &mut buf[..n]);

    // Advance the read pointer and the hardware boundary.
    *next_page_ptr = next_page;
    w(BNRY, ring_prev_page(next_page, RX_START, RX_STOP));
    crate::netstats::record_rx(n);
    Some(n)
}

/// Transmit `frame` (padded to the 60-byte Ethernet minimum, cut at the
/// 1514-byte maximum: the transmit buffer is only 6 pages wide).
fn send(frame: &[u8]) -> bool {
    let len = tx_len(frame.len());
    crate::netstats::record_tx(frame.len());

    // Remote-DMA write the frame into the transmit page.
    w(CR, CR_START | CR_RD_ABORT);
    w(ISR, ISR_RDC); // clear remote-DMA-complete
    w(RBCR0, (len & 0xFF) as u8);
    w(RBCR1, (len >> 8) as u8);
    w(RSAR0, 0x00);
    w(RSAR1, TX_PAGE);
    w(CR, CR_START | CR_RD_WRITE);
    for i in 0..len {
        let byte = if i < frame.len() { frame[i] } else { 0 };
        outb(DATA, byte);
    }
    let mut loaded = false;
    for _ in 0..SPIN {
        if r(ISR) & ISR_RDC != 0 {
            loaded = true;
            break;
        }
    }
    if !loaded {
        return false; // the chip never finished the copy: do not transmit garbage
    }

    // Issue the transmit.
    w(TPSR, TX_PAGE);
    w(TBCR0, (len & 0xFF) as u8);
    w(TBCR1, (len >> 8) as u8);
    w(CR, CR_START | CR_TXP | CR_RD_ABORT);
    true
}
