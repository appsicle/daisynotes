//! Filesystem locations owned by Daisy Notes.

use std::path::PathBuf;

/// Daisy Notes's local data directory: `$HOME/Library/Application Support/DaisyNotes`.
///
/// This function only computes the path; [`crate::Store::open`] (and therefore
/// [`crate::Store::open_default`]) performs the `create_dir_all`. If `$HOME` is
/// unset or empty — never the case in a normal macOS session — it falls back to
/// a `DaisyNotes` directory under the system temp dir so the app always has somewhere
/// writable.
pub fn data_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("DaisyNotes"),
        _ => std::env::temp_dir().join("DaisyNotes"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::data_dir;

    #[test]
    fn data_dir_ends_with_daisynotes() {
        assert!(data_dir().ends_with("DaisyNotes"));
    }
}
