//! The `shelld` threads: the job queue, cancel, and running a command line off the compositor.

use super::fs::VfsFs;
use super::sys::KSys;
use super::sys::Snap;
use super::ui::new_shell;
use crate::desktop::*;
use crate::sync::YieldMutex;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use kitsune_core::shell::sys::MemInfo;
use kitsune_core::shell::{Host, RunResult, Shell};
use kitsune_core::t;

/// A terminal's shell and filesystem view; moves to the worker with each job.
pub(crate) struct Ctx {
    pub shell: Shell,
    pub fs: VfsFs,
}

impl Ctx {
    pub(crate) fn new() -> Box<Ctx> {
        Box::new(Ctx {
            shell: new_shell(),
            fs: VfsFs::new(),
        })
    }
}

/// A command line to run.
pub(crate) struct Job {
    pub uid: u32,
    pub ctx: Box<Ctx>,
    pub line: String,
    pub snap: Snap,
}

/// A finished command line.
pub(crate) struct Done {
    pub uid: u32,
    pub ctx: Box<Ctx>,
    pub result: RunResult,
    /// The prompt for the next line (the working directory may have changed).
    pub prompt: String,
    /// `kill` requests to carry out.
    pub kills: Vec<(u32, i32)>,
}

static QUEUE: YieldMutex<VecDeque<Job>> = YieldMutex::new(VecDeque::new());
static FINISHED: YieldMutex<Vec<Done>> = YieldMutex::new(Vec::new());
/// Jobs in `QUEUE` (an atomic so the scheduler's idle test needs no lock).
static PENDING: AtomicUsize = AtomicUsize::new(0);
/// Terminal whose running command was asked to stop (0 = none).
pub(super) static CANCEL: AtomicU32 = AtomicU32::new(0);
/// Scheduler slots of the workers (`usize::MAX` until each starts). Two threads, so a command
/// that waits (`sleep`, `ping`, `curl`) does not hold up another terminal's `ls`.
static TIDS: [AtomicUsize; WORKERS] = [const { AtomicUsize::new(usize::MAX) }; WORKERS];
/// How many `shelld` threads run command lines.
const WORKERS: usize = 2;

/// Wake every worker (a job arrived or a command was cancelled).
fn wake_workers() {
    for t in &TIDS {
        let tid = t.load(Ordering::Acquire);
        if tid != usize::MAX {
            crate::sched::wake(tid);
        }
    }
}

/// Most command lines waiting at once.
const QUEUE_MAX: usize = 16;

/// True once the worker thread has died: nothing will answer any more.
pub(crate) fn worker_dead() -> bool {
    TIDS.iter().all(|t| {
        let tid = t.load(Ordering::Acquire);
        tid != usize::MAX && crate::sched::is_dead(tid)
    })
}

/// Post a command line. The job comes back unchanged when the queue is full.
pub(crate) fn post(job: Job) -> Result<(), Box<Job>> {
    let Ok(mut q) = QUEUE.lock() else {
        return Err(Box::new(job));
    };
    if q.len() >= QUEUE_MAX {
        return Err(Box::new(job));
    }
    q.push_back(job);
    PENDING.fetch_add(1, Ordering::AcqRel);
    drop(q);
    wake_workers();
    Ok(())
}

/// Ctrl+C in terminal `uid`: drop its queued job, or stop the one running.
pub(crate) fn cancel(uid: u32) {
    if let Ok(mut q) = QUEUE.lock()
        && let Some(i) = q.iter().position(|j| j.uid == uid)
        && let Some(job) = q.remove(i)
    {
        PENDING.fetch_sub(1, Ordering::AcqRel);
        drop(q);
        let prompt = prompt_of(&job.ctx);
        finish(
            job.uid,
            job.ctx,
            RunResult {
                status: 130,
                output: alloc::format!("sh: {}\n", t!("sh.sys.interrupted")).into_bytes(),
                ..RunResult::default()
            },
            prompt,
            Vec::new(),
        );
        return;
    }
    CANCEL.store(uid, Ordering::Relaxed);
    wake_workers();
}

fn finish(uid: u32, ctx: Box<Ctx>, result: RunResult, prompt: String, kills: Vec<(u32, i32)>) {
    // Never drop a result: keep trying while the compositor holds the list for a moment.
    let mut done = Some(Done {
        uid,
        ctx,
        result,
        prompt,
        kills,
    });
    for _ in 0..1000 {
        if let Ok(mut f) = FINISHED.lock() {
            if let Some(d) = done.take() {
                f.push(d);
            }
            break;
        }
        crate::sched::yield_now();
    }
}

/// Finished command lines, oldest first.
pub(crate) fn take_done() -> Vec<Done> {
    match FINISHED.lock() {
        Ok(mut f) if !f.is_empty() => core::mem::take(&mut *f),
        _ => Vec::new(),
    }
}

fn run_job(job: Job) {
    let Job {
        uid,
        mut ctx,
        line,
        snap,
    } = job;
    // A Ctrl+C meant for an earlier command (it ended just before the key) must not stop this one.
    let _ = CANCEL.compare_exchange(uid, 0, Ordering::AcqRel, Ordering::Relaxed);
    let mut sys = KSys::new(uid, snap);
    let result = {
        let Ctx { shell, fs } = &mut *ctx;
        let mut host = Host { fs, sys: &mut sys };
        shell.run_line(&line, &mut host)
    };
    let prompt = ctx.shell.prompt(&ctx.fs, &sys);
    // A Ctrl+C that arrived after the command ended must not hit the next one.
    let _ = CANCEL.compare_exchange(uid, 0, Ordering::AcqRel, Ordering::Relaxed);
    finish(uid, ctx, result, prompt, core::mem::take(&mut sys.kills));
}

/// Thread entries: run posted command lines, one at a time each; parked while idle.
pub extern "C" fn worker() -> ! {
    worker_loop(0)
}

pub extern "C" fn worker2() -> ! {
    worker_loop(1)
}

fn worker_loop(slot: usize) -> ! {
    TIDS[slot].store(crate::sched::current(), Ordering::Release);
    loop {
        let job = match QUEUE.lock() {
            Ok(mut q) => q.pop_front(),
            Err(_) => None,
        };
        match job {
            Some(job) => {
                PENDING.fetch_sub(1, Ordering::AcqRel);
                run_job(job);
            }
            None => {
                crate::sched::block(crate::sched::FOREVER, || {
                    PENDING.load(Ordering::Acquire) == 0
                });
            }
        }
    }
}

/// The prompt `ctx` would show now (no system facts needed beyond the host name).
pub(crate) fn prompt_of(ctx: &Ctx) -> String {
    let sys = KSys::new(
        0,
        Snap {
            procs: Vec::new(),
            mem: MemInfo::default(),
            disks: [None, None],
        },
    );
    ctx.shell.prompt(&ctx.fs, &sys)
}
