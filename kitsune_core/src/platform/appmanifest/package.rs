//! package (split out of `appmanifest.rs`).

use super::*;

/// Resource limits enforced on one running app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quotas {
    pub mem_bytes: usize,
    pub fuel_frame: u64,
    pub disk_bytes: u64,
    pub max_fds: usize,
}

/// Why an icon was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconError {
    TooLarge,
    /// Header unreadable or not a PNG.
    NotPng,
    /// Wider or taller than 64.
    Dimensions,
    /// The pixel data did not decode.
    Corrupt,
}

impl IconError {
    /// Catalog key of the reason.
    pub fn key(self) -> &'static str {
        match self {
            IconError::TooLarge => tk!("apps.err.icon_large"),
            IconError::NotPng => tk!("apps.err.icon_png"),
            IconError::Dimensions => tk!("apps.err.icon_size"),
            IconError::Corrupt => tk!("apps.err.icon_corrupt"),
        }
    }
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IconError::TooLarge => "icon is larger than 64 KiB",
            IconError::NotPng => "icon is not a PNG",
            IconError::Dimensions => "icon is larger than 64x64",
            IconError::Corrupt => "icon PNG is corrupt",
        })
    }
}

/// Decodes an `kitsune.icon` payload. The size and dimensions are checked from
/// the header **before** any pixel is decoded.
pub fn decode_icon(data: &[u8]) -> Result<Image, IconError> {
    if data.len() > MAX_ICON_BYTES {
        return Err(IconError::TooLarge);
    }
    let h = png::read_header(data).map_err(|_| IconError::NotPng)?;
    if h.width > MAX_ICON_DIM || h.height > MAX_ICON_DIM {
        return Err(IconError::Dimensions);
    }
    image::decode(data).map_err(|_| IconError::Corrupt)
}

/// Why a package was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageError {
    Wasm(WasmError),
    NoManifest,
    DuplicateManifest,
    DuplicateIcon,
    Manifest(ManifestError),
    Icon(IconError),
}

impl PackageError {
    /// The reason in `lang`, in words for the person installing the app.
    pub fn message_in(&self, lang: Lang) -> alloc::string::String {
        match self {
            PackageError::Wasm(e) => alloc::string::String::from(i18n::tr_in(lang, e.key())),
            PackageError::NoManifest => {
                alloc::string::String::from(i18n::tr_in(lang, tk!("apps.err.no_manifest")))
            }
            PackageError::DuplicateManifest => {
                alloc::string::String::from(i18n::tr_in(lang, tk!("apps.err.two_manifests")))
            }
            PackageError::DuplicateIcon => {
                alloc::string::String::from(i18n::tr_in(lang, tk!("apps.err.two_icons")))
            }
            PackageError::Manifest(e) => e.message_in(lang),
            PackageError::Icon(e) => alloc::string::String::from(i18n::tr_in(lang, e.key())),
        }
    }
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::Wasm(e) => e.fmt(f),
            PackageError::NoManifest => f.write_str("package has no kitsune.manifest section"),
            PackageError::DuplicateManifest => f.write_str("package has two manifests"),
            PackageError::DuplicateIcon => f.write_str("package has two icons"),
            PackageError::Manifest(e) => e.fmt(f),
            PackageError::Icon(e) => e.fmt(f),
        }
    }
}

/// A parsed package: manifest plus the optional decoded icon.
#[derive(Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub icon: Option<Image>,
}

/// How many custom sections are named `name` or `legacy` (together), and the payload
/// of the first one found (the current name wins). A package that carries both names
/// therefore counts as a duplicate.
pub(super) fn find_section<'a>(
    wasm: &'a [u8],
    name: &str,
    legacy: &str,
) -> Result<(usize, Option<&'a [u8]>), WasmError> {
    let (n, first) = wasmsec::find_custom(wasm, name.as_bytes())?;
    let (nl, first_legacy) = wasmsec::find_custom(wasm, legacy.as_bytes())?;
    Ok((n + nl, first.or(first_legacy)))
}

/// Reads and validates the manifest and icon of `wasm` (sections named `kitsune.*`, or
/// the old `osjeff.*`).
pub fn parse_package(wasm: &[u8]) -> Result<Package, PackageError> {
    let (n, m) = find_section(wasm, MANIFEST_SECTION, LEGACY_MANIFEST_SECTION)
        .map_err(PackageError::Wasm)?;
    if n > 1 {
        return Err(PackageError::DuplicateManifest);
    }
    let m = m.ok_or(PackageError::NoManifest)?;
    let (ni, icon) =
        find_section(wasm, ICON_SECTION, LEGACY_ICON_SECTION).map_err(PackageError::Wasm)?;
    if ni > 1 {
        return Err(PackageError::DuplicateIcon);
    }
    let manifest = Manifest::parse(m).map_err(PackageError::Manifest)?;
    let icon = match icon {
        Some(d) => Some(decode_icon(d).map_err(PackageError::Icon)?),
        None => None,
    };
    Ok(Package { manifest, icon })
}

/// Like [`parse_package`], but a module **without** a manifest yields `None`
/// (a legacy app) instead of an error. Any other problem is still an error.
pub fn parse_package_opt(wasm: &[u8]) -> Result<Option<Package>, PackageError> {
    match parse_package(wasm) {
        Ok(p) => Ok(Some(p)),
        Err(PackageError::NoManifest) => Ok(None),
        Err(e) => Err(e),
    }
}
