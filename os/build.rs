use std::path::PathBuf;

fn main() {
    // Path to the compiled kernel binary, injected by cargo's artifact-dependency support.
    let kernel = PathBuf::from(std::env::var_os("CARGO_BIN_FILE_KERNEL_kernel").unwrap());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    let uefi_path = out_dir.join("osjeff-uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&uefi_path)
        .expect("failed to build UEFI image");

    let bios_path = out_dir.join("osjeff-bios.img");
    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios_path)
        .expect("failed to build BIOS image");

    println!("cargo:rustc-env=UEFI_IMAGE={}", uefi_path.display());
    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_path.display());
}
