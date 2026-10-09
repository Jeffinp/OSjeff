use super::*;

#[test]
fn places_map_to_paths_and_back() {
    assert_eq!(Place::Home.path(), b"/home");
    assert_eq!(Place::Disk.path(), b"/");
    assert_eq!(Place::Trash.path(), TRASH_PATH);
    assert_eq!(Place::Apps.path(), APPS_PATH);
    assert_eq!(Place::of_path(b"/"), Place::Disk);
    assert_eq!(Place::of_path(b"/Documentos"), Place::Documents);
    assert_eq!(Place::of_path(b"/Documentos/a/b"), Place::Documents);
    assert_eq!(Place::of_path(b"/DocumentosX"), Place::Disk);
    assert_eq!(Place::of_path(b"/Imagens/ferias"), Place::Images);
    assert_eq!(Place::of_path(b"/home"), Place::Home);
    assert_eq!(Place::of_path(b"/outra"), Place::Disk);
    assert_eq!(Place::of_path(TRASH_PATH), Place::Trash);
    assert_eq!(Place::of_path(APPS_PATH), Place::Apps);
    assert!(Place::Home.is_folder() && Place::Images.is_folder());
    assert!(!Place::Trash.is_folder() && !Place::Apps.is_folder() && !Place::Disk.is_folder());
}
