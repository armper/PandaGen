//! Where `:w` puts the file, and what it does when it cannot.

use fs_view::{DirectoryResolver, DirectoryView};
use services_editor_vi::{EditorIo, OpenOptions, StorageEditorIo};
use services_fs_view::{FileSystemOperations, FileSystemViewService};
use services_storage::{JournaledStorage, ObjectId, ObjectKind, TransactionalStorage};

fn fixture() -> (FileSystemViewService, DirectoryView) {
    let root = DirectoryView::new(ObjectId::new());
    let mut fs = FileSystemViewService::new();
    fs.register_directory(root.clone());
    (fs, root)
}

/// `save_as` took `path.split('/').next_back()`, so `:w docs/notes.txt`
/// wrote `notes.txt` into the root and reported "Saved as: docs/notes.txt".
/// The user was told their work was at a path that did not contain it.
#[test]
fn save_as_puts_the_file_where_the_user_said() {
    let (mut fs, mut root) = fixture();
    fs.mkdir(&mut root, "docs").unwrap();

    let mut io = StorageEditorIo::with_fs_view(JournaledStorage::new(), fs, root);
    let saved = io.save_as("docs/notes.txt", "my work").unwrap();
    assert!(saved.message.contains("docs/notes.txt"));

    io.open(OpenOptions::new().with_path("docs/notes.txt"))
        .expect("reported saved to docs/notes.txt but it is not there");
}

/// And a path naming a directory that does not exist is refused, rather
/// than quietly writing into the root.
#[test]
fn save_as_refuses_a_directory_that_is_not_there() {
    let (fs, root) = fixture();
    let mut io = StorageEditorIo::with_fs_view(JournaledStorage::new(), fs, root);
    assert!(
        io.save_as("nowhere/notes.txt", "my work").is_err(),
        "a path through a directory that does not exist was accepted"
    );
}

/// `save_as` wrote and *committed* the whole buffer before trying to link,
/// so `:w <a name that already exists>` -- routine vi habit -- left a full
/// second copy committed into an object nothing links to, and reported an
/// error. A refusal that is also a leak.
#[test]
fn a_refused_save_does_not_commit_the_bytes_anyway() {
    let mut storage = JournaledStorage::new();
    let existing = ObjectId::new();
    let mut tx = storage.begin_transaction().unwrap();
    storage.write(&mut tx, existing, b"old").unwrap();
    storage.commit(&mut tx).unwrap();

    let (mut fs, mut root) = fixture();
    fs.link(&mut root, "notes.txt", existing, ObjectKind::Blob)
        .unwrap();

    let before = storage.journal_entries().len();
    let mut io = StorageEditorIo::with_fs_view(storage, fs, root);
    assert!(
        io.save_as("notes.txt", "brand new work").is_err(),
        "sanity: the link is refused"
    );
    assert_eq!(
        io.storage().journal_entries().len(),
        before,
        "a refused save committed the bytes anyway, into an object nothing links to"
    );
}

/// `register_directory` takes a clone and the mutating operations write to
/// the caller's root, so the service's own copy -- the file picker's only
/// way to load a directory -- never saw a file the user created.
#[test]
fn the_registered_root_tracks_the_callers_root() {
    let (mut fs, mut root) = fixture();
    let root_id = root.id;

    fs.link(&mut root, "a.txt", ObjectId::new(), ObjectKind::Blob)
        .unwrap();
    assert!(root.get_entry("a.txt").is_some(), "sanity");
    let seen = fs.resolve_directory(&root_id).unwrap();
    assert!(
        seen.get_entry("a.txt").is_some(),
        "the service's copy of the root never saw the new file"
    );

    fs.mkdir(&mut root, "docs").unwrap();
    assert!(fs
        .resolve_directory(&root_id)
        .unwrap()
        .get_entry("docs")
        .is_some());

    fs.unlink(&mut root, "a.txt").unwrap();
    assert!(
        fs.resolve_directory(&root_id)
            .unwrap()
            .get_entry("a.txt")
            .is_none(),
        "the service's copy still shows a file that was removed"
    );
}
