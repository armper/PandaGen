//! Integration tests for bare-metal storage and editor I/O

#[cfg(test)]
mod tests {
    use crate::bare_metal_editor_io::BareMetalEditorIo;
    use crate::bare_metal_storage::BareMetalFilesystem;

    #[test]
    fn test_create_and_read_file() {
        let mut fs = BareMetalFilesystem::new().unwrap();

        // Create a file
        let content = b"Hello, PandaGen!";
        let file_id = fs.create_file("test.txt", content).unwrap();

        // Read it back
        let read_content = fs.read_file(file_id).unwrap();
        assert_eq!(read_content, content);

        // Read by name
        let read_by_name = fs.read_file_by_name("test.txt").unwrap();
        assert_eq!(read_by_name, content);
    }

    #[test]
    fn test_list_files() {
        let mut fs = BareMetalFilesystem::new().unwrap();

        // Initially empty
        let files = fs.list_files().unwrap();
        assert_eq!(files.len(), 0);

        // Create some files
        fs.create_file("file1.txt", b"content1").unwrap();
        fs.create_file("file2.txt", b"content2").unwrap();
        fs.create_file("file3.txt", b"content3").unwrap();

        // List them
        let files = fs.list_files().unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&"file1.txt".to_string()));
        assert!(files.contains(&"file2.txt".to_string()));
        assert!(files.contains(&"file3.txt".to_string()));
    }

    #[test]
    fn test_update_file() {
        let mut fs = BareMetalFilesystem::new().unwrap();

        // Create a file
        fs.create_file("test.txt", b"original").unwrap();

        // Update it
        fs.write_file_by_name("test.txt", b"updated").unwrap();

        // Verify update
        let content = fs.read_file_by_name("test.txt").unwrap();
        assert_eq!(content, b"updated");
    }

    #[test]
    fn test_delete_file() {
        let mut fs = BareMetalFilesystem::new().unwrap();

        // Create a file
        fs.create_file("test.txt", b"content").unwrap();

        // Verify it exists
        assert!(fs.read_file_by_name("test.txt").is_ok());

        // Delete it
        fs.delete_file("test.txt").unwrap();

        // Verify it's gone
        assert!(fs.read_file_by_name("test.txt").is_err());
    }

    #[test]
    fn test_editor_io_open_file() {
        let mut fs = BareMetalFilesystem::new().unwrap();
        fs.create_file("test.txt", b"Hello, World!").unwrap();

        let mut io = BareMetalEditorIo::new(fs);

        let (content, handle) = io.open("test.txt").unwrap();
        assert_eq!(content, "Hello, World!");
        assert!(handle.object_id.is_some());
        assert_eq!(handle.path, Some("test.txt".to_string()));
    }

    #[test]
    fn test_editor_io_save_file() {
        let mut fs = BareMetalFilesystem::new().unwrap();
        fs.create_file("test.txt", b"original").unwrap();

        let mut io = BareMetalEditorIo::new(fs);

        let (_, handle) = io.open("test.txt").unwrap();
        let msg = io.save(&handle, "updated content").unwrap();

        assert!(msg.contains("test.txt"));

        // Verify the content was updated
        let (new_content, _) = io.open("test.txt").unwrap();
        assert_eq!(new_content, "updated content");
    }

    #[test]
    fn test_editor_io_save_as() {
        let mut fs = BareMetalFilesystem::new().unwrap();
        let mut io = BareMetalEditorIo::new(fs);

        let (msg, handle) = io.save_as("new_file.txt", "new content").unwrap();

        assert!(msg.contains("new_file.txt"));
        assert_eq!(handle.path, Some("new_file.txt".to_string()));

        // Verify the file was created
        let (content, _) = io.open("new_file.txt").unwrap();
        assert_eq!(content, "new content");
    }

    #[test]
    fn test_editor_io_new_buffer() {
        let fs = BareMetalFilesystem::new().unwrap();
        let io = BareMetalEditorIo::new(fs);

        let handle = io.new_buffer(Some("untitled.txt".to_string()));
        assert!(handle.object_id.is_none());
        assert_eq!(handle.path, Some("untitled.txt".to_string()));
    }

    #[test]
    fn test_file_not_found() {
        let fs = BareMetalFilesystem::new().unwrap();
        let result = fs.read_file_by_name("nonexistent.txt");
        assert!(result.is_err());
    }
}
