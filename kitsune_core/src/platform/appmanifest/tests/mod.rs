use super::*;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;

const MIN: &str = "id=hello\nname=Hello\nversion=1.2.3\n";

fn parse(s: &str) -> Result<Manifest, ManifestError> {
    Manifest::parse(s.as_bytes())
}

fn with(extra: &str) -> Result<Manifest, ManifestError> {
    parse(&format!("{MIN}{extra}\n"))
}

fn leb(v: &mut Vec<u8>, mut n: u32) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            v.push(b);
            return;
        }
        v.push(b | 0x80);
    }
}

fn custom(name: &str, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    leb(&mut payload, name.len() as u32);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    let mut s = Vec::new();
    s.push(0);
    leb(&mut s, payload.len() as u32);
    s.extend_from_slice(&payload);
    s
}

fn module(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut v = wasmsec::HEADER.to_vec();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

fn icon_png(w: usize, h: usize) -> Vec<u8> {
    let px = alloc::vec![0xFF2080C0u32; w * h];
    let img = Image::from_pixels(w, h, px).unwrap();
    png::encode(&img).unwrap()
}

fn pkg(manifest: &str) -> Vec<u8> {
    module(&[custom(MANIFEST_SECTION, manifest.as_bytes())])
}

fn with_hosts(net: &str, hosts: &str) -> Result<Manifest, ManifestError> {
    Manifest::parse(
        alloc::format!("id=n\nname=N\nversion=1.0.0\nnet={net}\nnet_hosts={hosts}\n").as_bytes(),
    )
}

mod basics;
mod id_name;
mod localized_names;
mod net_hosts;
mod package;
mod permissions;
mod quotas;
mod version;
mod window;
