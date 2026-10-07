//! Kernel-owned GDT and TSS.
//!
//! The bootloader leaves its own small GDT in place and no TSS at all, so an
//! exception taken on an exhausted stack (#PF while pushing the frame) cannot
//! be delivered and escalates #PF -> #DF -> triple fault: an instant, silent
//! reset. A TSS whose IST slot 0 points at a dedicated stack lets the #DF
//! handler run on known-good memory and report the fault on COM1 instead.
//!
//! The page-fault handler gets an IST slot of its own too: a thread that runs off
//! the end of its stack hits the guard page below it (see `vm`), and the CPU must
//! push the #PF frame somewhere that is not that exhausted stack. Using a different
//! slot from #DF keeps a fault *inside* the #PF handler from being delivered on top
//! of the stack the handler is already using.
//!
//! Everything here runs in ring 0; there are no user-mode segments (see
//! docs/audit/adr-isolamento.md).

use crate::sync::RacyCell;
use x86_64::VirtAddr;
use x86_64::instructions::tables::load_tss;
use x86_64::registers::segmentation::{CS, DS, ES, SS, Segment};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;

/// IST slot (0-based) used by the double-fault handler.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
/// IST slot (0-based) used by the page-fault handler.
pub const PAGE_FAULT_IST_INDEX: u16 = 1;

const IST_STACK_SIZE: usize = 32 * 1024;

#[repr(C, align(16))]
struct IstStack([u8; IST_STACK_SIZE]);

static IST_STACK: RacyCell<IstStack> = RacyCell::new(IstStack([0; IST_STACK_SIZE]));
static PF_IST_STACK: RacyCell<IstStack> = RacyCell::new(IstStack([0; IST_STACK_SIZE]));
static TSS: RacyCell<TaskStateSegment> = RacyCell::new(TaskStateSegment::new());
static GDT: RacyCell<GlobalDescriptorTable> = RacyCell::new(GlobalDescriptorTable::new());

/// Build and load the GDT/TSS and reload the segment registers.
///
/// Must run once, before interrupts are enabled and before anything captures
/// `CS`/`SS` (the IDT gates and `sched::spawn` both read the live selectors).
pub fn init() {
    // SAFETY: called once on the boot thread with interrupts still disabled, so
    // nothing else touches these statics; the statics live for 'static, which
    // `GDT.load()` and the TSS descriptor require.
    unsafe {
        let stack_top = VirtAddr::from_ptr(IST_STACK.get()) + IST_STACK_SIZE as u64;
        (*TSS.get()).interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_top;
        let pf_top = VirtAddr::from_ptr(PF_IST_STACK.get()) + IST_STACK_SIZE as u64;
        (*TSS.get()).interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] = pf_top;

        let gdt = &mut *GDT.get();
        let code = gdt.append(Descriptor::kernel_code_segment());
        let data = gdt.append(Descriptor::kernel_data_segment());
        let tss = gdt.append(Descriptor::tss_segment(&*TSS.get()));
        (*GDT.get()).load();

        CS::set_reg(code);
        SS::set_reg(data);
        DS::set_reg(data);
        ES::set_reg(data);
        load_tss(tss);
    }
}
