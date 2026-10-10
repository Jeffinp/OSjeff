//! places (split out of `fileman.rs`).

use super::*;

/// Sidebar places.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// The user's folder (`/home`).
    Home,
    Documents,
    Images,
    /// Installed and bundled apps (`APPS_PATH`).
    Apps,
    Trash,
    /// The volume (its root); the entry shows the usage bar.
    Disk,
}

impl Place {
    /// The location the place opens.
    pub fn path(self) -> &'static [u8] {
        match self {
            Place::Home => b"/home",
            Place::Documents => b"/Documentos",
            Place::Images => b"/Imagens",
            Place::Apps => APPS_PATH,
            Place::Trash => TRASH_PATH,
            Place::Disk => b"/",
        }
    }

    /// Whether the place has to exist as a folder on the volume (and is created when it does
    /// not).
    pub fn is_folder(self) -> bool {
        !matches!(self, Place::Apps | Place::Trash | Place::Disk)
    }

    /// The place that contains `cwd`, for highlighting the sidebar: the deepest favourite
    /// that is a prefix of the path, else the disk for any other folder.
    pub fn of_path(cwd: &[u8]) -> Place {
        if cwd == TRASH_PATH {
            return Place::Trash;
        }
        if cwd == APPS_PATH {
            return Place::Apps;
        }
        for p in [Place::Home, Place::Documents, Place::Images] {
            let base = p.path();
            if cwd == base || (cwd.starts_with(base) && cwd.get(base.len()) == Some(&b'/')) {
                return p;
            }
        }
        Place::Disk
    }
}
