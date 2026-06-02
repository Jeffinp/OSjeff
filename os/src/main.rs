use std::process::Command;

/// Boots the generated OSjeff disk image in QEMU.
///
/// Default: BIOS image. Pass `uefi` as the first arg to boot the UEFI image
/// (requires OVMF firmware installed and discoverable by QEMU).
fn main() {
    let uefi = std::env::args().nth(1).as_deref() == Some("uefi");

    let mut qemu = Command::new("qemu-system-x86_64");
    if uefi {
        qemu.arg("-bios").arg(ovmf_prebuilt());
        qemu.arg("-drive")
            .arg(format!("format=raw,file={}", env!("UEFI_IMAGE")));
    } else {
        qemu.arg("-drive")
            .arg(format!("format=raw,file={}", env!("BIOS_IMAGE")));
    }
    // 128 MiB RAM is plenty for a framebuffer demo.
    qemu.arg("-m").arg("128M");

    let status = qemu.status().expect("failed to launch qemu-system-x86_64");
    std::process::exit(status.code().unwrap_or(-1));
}

/// OVMF firmware path provided by the `ovmf_prebuilt` crate would be cleaner,
/// but to keep deps minimal we rely on a system-installed OVMF for UEFI mode.
fn ovmf_prebuilt() -> String {
    std::env::var("OVMF_PATH").unwrap_or_else(|_| "/usr/share/ovmf/OVMF.fd".to_string())
}
