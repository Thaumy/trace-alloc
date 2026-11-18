use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub(super) fn checked_path(path: &Path) -> io::Result<PathBuf> {
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(invalid("parent directory components are not allowed"));
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid("symbolic links are not allowed"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(path.to_path_buf())
}
