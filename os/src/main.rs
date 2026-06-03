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

    // Persistent filesystem disk on the secondary IDE channel (master). Created
    // blank on first run; the kernel formats it if it holds no filesystem.
    let fs_img = "osjeff-fs.img";
    if !std::path::Path::new(fs_img).exists() {
        std::fs::write(fs_img, vec![0u8; 64 * 1024]).expect("failed to create fs disk image");
    }
    qemu.arg("-drive")
        .arg(format!("format=raw,file={fs_img},if=ide,index=2"));

    // NE2000 NIC on user-mode (SLIRP) networking, with a packet dump so the
    // traffic (gratuitous ARP on boot, ARP/ping replies) is visible offline.
    qemu.arg("-netdev").arg("user,id=n0");
    qemu.arg("-device")
        .arg("ne2k_isa,netdev=n0,mac=52:54:00:12:34:56");
    qemu.arg("-object")
        .arg("filter-dump,id=dump,netdev=n0,file=osjeff-net.pcap");

    let status = qemu.status().expect("failed to launch qemu-system-x86_64");
    std::process::exit(status.code().unwrap_or(-1));
}

/// OVMF firmware path provided by the `ovmf_prebuilt` crate would be cleaner,
/// but to keep deps minimal we rely on a system-installed OVMF for UEFI mode.
fn ovmf_prebuilt() -> String {
    std::env::var("OVMF_PATH").unwrap_or_else(|_| "/usr/share/ovmf/OVMF.fd".to_string())
}
