//! Commands that act on the desktop (`edit`, `files`, `tasks`, `reboot`...): they leave a request for the compositor.

use crate::desktop::*;
use crate::sync::YieldMutex;
use kitsune_core::shell::Shell;
use kitsune_core::shell::exec::CmdCtx;
use kitsune_core::shell::fs::{FsErr, Kind as FsKind};
use kitsune_core::{t, tk};

/// A request from a command to the compositor.
pub(crate) enum UiReq {
    /// `edit [FILE]`: open an editor (on `FILE`, created on save when new).
    Edit(Option<String>),
    /// `files [DIR]`: open a file manager.
    Files,
    Tasks,
    Calc,
    Reboot,
    Shutdown,
}

static UI: YieldMutex<Vec<UiReq>> = YieldMutex::new(Vec::new());

fn post_ui(r: UiReq) {
    if let Ok(mut q) = UI.lock()
        && q.len() < 16
    {
        q.push(r);
    }
}

/// Requests waiting for the compositor.
pub(crate) fn take_ui() -> Vec<UiReq> {
    match UI.lock() {
        Ok(mut q) if !q.is_empty() => core::mem::take(&mut *q),
        _ => Vec::new(),
    }
}

fn cmd_edit(cx: &mut CmdCtx<'_>) -> i32 {
    match cx.args.len() {
        1 => post_ui(UiReq::Edit(None)),
        2 => {
            let p = cx.fs.resolve(&cx.args[1]);
            if let Ok(st) = cx.fs.stat(&p)
                && st.kind == FsKind::Dir
            {
                cx.error(&alloc::format!("{p}: {}", FsErr::IsADirectory.message()));
                return 1;
            }
            post_ui(UiReq::Edit(Some(p)));
        }
        _ => {
            cx.error(t!("sh.edit.usage"));
            return 2;
        }
    }
    0
}

fn cmd_files(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Files);
    0
}

fn cmd_tasks(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Tasks);
    0
}

fn cmd_calc(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Calc);
    0
}

fn cmd_reboot(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Reboot);
    0
}

fn cmd_shutdown(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Shutdown);
    0
}

/// A shell with the desktop's own commands added to the standard ones.
pub(crate) fn new_shell() -> Shell {
    let mut sh = Shell::new();
    // The kernel thread has a 128 KiB stack; keep recursion well under the host-tested worst case.
    sh.limits.max_call_depth = 16;
    sh.limits.max_sub_depth = 4;
    sh.register("edit", tk!("sh.edit.help"), cmd_edit);
    sh.register("files", tk!("sh.files.help"), cmd_files);
    sh.register("tasks", tk!("sh.tasks.help"), cmd_tasks);
    sh.register("calc", tk!("sh.calc.help"), cmd_calc);
    sh.register("reboot", tk!("sh.reboot.help"), cmd_reboot);
    sh.register("shutdown", tk!("sh.shutdown.help"), cmd_shutdown);
    sh.env.set("PS1", "\\w\\$ ");
    sh.env.set("HOME", "/");
    sh
}

// ---- the command thread --------------------------------------------------------
