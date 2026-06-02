/*
 * Preemptive context switch — x86_64 (Intel syntax).
 *
 * Entered directly from the IDT on every PIT tick (IRQ0). The CPU has already
 * pushed the interrupt frame (SS, RSP, RFLAGS, CS, RIP) onto the *current*
 * thread's stack. We save the 15 general-purpose registers below it, hand the
 * resulting stack pointer to the Rust scheduler, switch RSP to whatever stack
 * it returns (the next thread), restore that thread's registers, and `iretq`.
 *
 * Because the entire interrupted state lives on each thread's own stack, the
 * `iretq` resumes a *different* thread exactly where it was preempted — no
 * cooperation required. This is the one place that cannot be written in Rust:
 * swapping the stack out from under the running code and returning elsewhere.
 *
 * The Rust half ({schedule} = sched::timer_schedule) saves the outgoing RSP,
 * does round-robin + FPU/SSE save/restore, EOIs the PIC, and returns the next
 * thread's RSP in RAX.
 */
.text
.global timer_isr
timer_isr:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp        /* arg0 = current (outgoing) stack pointer */
    call {schedule}
    mov rsp, rax        /* switch to the next thread's stack        */
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    iretq
