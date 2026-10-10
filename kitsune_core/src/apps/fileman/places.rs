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
    /// The location the place opens. Home and the two folders in it follow the signed-in user
    /// (`storage::homes::current_home`).
    pub fn path(self) -> Vec<u8> {
        match self {
            Place::Home => crate::storage::homes::current_home(),
            Place::Documents => vfs::join(&crate::storage::homes::current_home(), b"Documentos"),
            Place::Images => vfs::join(&crate::storage::homes::current_home(), b"Imagens"),
            Place::Apps => APPS_PATH.to_vec(),
            Place::Trash => TRASH_PATH.to_vec(),
            Place::Disk => b"/".to_vec(),
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
        // The deepest favourite first: Documents and Images live inside Home.
        for p in [Place::Documents, Place::Images, Place::Home] {
            let base = p.path();
            let base = base.as_slice();
            if cwd == base || (cwd.starts_with(base) && cwd.get(base.len()) == Some(&b'/')) {
                return p;
            }
        }
        Place::Disk
    }
}
