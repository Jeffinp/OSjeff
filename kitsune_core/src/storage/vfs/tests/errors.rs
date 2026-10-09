use super::*;

#[test]
fn every_error_has_a_message_in_both_languages() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let all = [
        VfsError::NotFound,
        VfsError::Exists,
        VfsError::NotDir,
        VfsError::IsDir,
        VfsError::NotEmpty,
        VfsError::InvalidName,
        VfsError::NameTooLong,
        VfsError::InvalidPath,
        VfsError::Reserved,
        VfsError::InvalidMove,
        VfsError::NoSpace,
        VfsError::NoInodes,
        VfsError::TooBig,
        VfsError::Busy,
        VfsError::Unavailable,
        VfsError::Io,
        VfsError::Corrupt,
        VfsError::Cancelled,
    ];
    for l in Lang::ALL {
        let _g = LangGuard::new(l);
        for e in all {
            assert!(!e.message().is_empty());
            assert_ne!(e.message(), "files.err.", "{e:?}");
            assert!(!e.message().starts_with("files."), "{e:?}: key shown");
        }
    }
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(VfsError::NotFound.message(), "Item não encontrado");
    assert_eq!(VfsError::Cancelled.message(), "Operação cancelada");
    let _g = LangGuard::new(Lang::En);
    assert_eq!(VfsError::NotFound.message(), "Item not found");
    assert_eq!(VfsError::NoSpace.message(), "Disk full");
}

#[test]
fn fs_errors_map() {
    assert_eq!(VfsError::from(FsError::NotFound), VfsError::NotFound);
    assert_eq!(VfsError::from(FsError::NoSpace), VfsError::NoSpace);
    assert_eq!(
        VfsError::from(FsError::Io(crate::storage::blockdev::IoError::Read)),
        VfsError::Io
    );
    assert_eq!(VfsError::from(FsError::Corrupt("x")), VfsError::Corrupt);
    assert_eq!(VfsError::from(FsError::Poisoned), VfsError::Io);
}
