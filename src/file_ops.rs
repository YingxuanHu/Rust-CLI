//! File operation utilities for safe file reading, writing, and listing.
//!
//! All operations are sandboxed to the current working directory or repository root.

use std::path::{Path, PathBuf};
use std::fs;

use anyhow::{Context, Result, bail};

/// Check if a path is safe (within the allowed base directory).
#[allow(dead_code)] // Reserved for future validation
pub fn is_safe_path(path: &Path, base: &Path) -> bool {
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    match path.canonicalize() {
        Ok(canonical) => canonical.starts_with(&canonical_base),
        Err(_) => {
            // If path doesn't exist yet, check if parent is safe
            if let Some(parent) = path.parent() {
                match parent.canonicalize() {
                    Ok(canonical_parent) => canonical_parent.starts_with(&canonical_base),
                    Err(_) => false,
                }
            } else {
                false
            }
        }
    }
}

/// Resolve a path relative to a base directory.
pub fn resolve_path(path_str: &str, base: &Path) -> PathBuf {
    let path = PathBuf::from(path_str);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// List files in a directory.
pub fn list_directory(path: &Path, base: &Path) -> Result<Vec<FileInfo>> {
    let canonical = path.canonicalize().context("canonicalizing path")?;
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    
    if !canonical.starts_with(&canonical_base) {
        bail!("Access denied: path outside allowed directory");
    }

    let entries = fs::read_dir(&canonical).context("reading directory")?;
    let mut files = Vec::new();

    for entry in entries {
        let entry = entry.context("reading directory entry")?;
        let metadata = entry.metadata().context("reading file metadata")?;
        let file_type = if metadata.is_dir() {
            "dir"
        } else if metadata.is_symlink() {
            "link"
        } else {
            "file"
        };

        files.push(FileInfo {
            name: entry.file_name().to_string_lossy().to_string(),
            file_type: file_type.to_string(),
            size: if metadata.is_file() {
                Some(metadata.len())
            } else {
                None
            },
        });
    }

    files.sort_by(|a, b| {
        // Directories first, then files, alphabetically
        match (a.file_type.as_str(), b.file_type.as_str()) {
            ("dir", "file") => std::cmp::Ordering::Less,
            ("file", "dir") => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    Ok(files)
}

/// Information about a file or directory.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub file_type: String,
    pub size: Option<u64>,
}

/// Read a file's contents.
pub fn read_file(path: &Path, base: &Path) -> Result<String> {
    let canonical = path.canonicalize().context("canonicalizing path")?;
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    
    if !canonical.starts_with(&canonical_base) {
        bail!("Access denied: path outside allowed directory");
    }

    fs::read_to_string(&canonical).context("reading file")
}

/// Write content to a file (creates or overwrites).
///
/// Relative paths are always resolved from `base`, rather than from the
/// process working directory. This keeps confirmed workflows inside the
/// repository they were created for even when the application was launched
/// from somewhere else.
pub fn write_file(path: &Path, content: &str, base: &Path) -> Result<()> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };

    // Check if path is safe (parent must exist and be within base)
    let parent = path.parent().context("file must have a parent directory")?;
    let canonical_parent = parent
        .canonicalize()
        .context("parent directory does not exist")?;
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    
    if !canonical_parent.starts_with(&canonical_base) {
        bail!("Access denied: path outside allowed directory");
    }

    // `exists()` follows symlinks and returns false for a dangling link. Check
    // the directory entry itself before allowing creation of a new file.
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            let canonical_target = path.canonicalize().context(
                "cannot resolve existing file; repair or remove a dangling symlink before writing",
            )?;
            if !canonical_target.starts_with(&canonical_base) {
                bail!("Access denied: file resolves outside allowed directory");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("checking file before writing"),
    }

    fs::write(path, content).context("writing file")?;
    Ok(())
}

/// Check if a file exists.
pub fn file_exists(path: &Path) -> bool {
    path.exists()
}

/// Format file size in human-readable format.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1048576), "1.00 MB");
        assert_eq!(format_size(1073741824), "1.00 GB");
    }

    #[test]
    fn test_resolve_path() {
        let base = PathBuf::from("/home/user/project");
        
        // Relative path
        let resolved = resolve_path("src/main.rs", &base);
        assert_eq!(resolved, PathBuf::from("/home/user/project/src/main.rs"));
        
        // Absolute path
        let resolved = resolve_path("/etc/hosts", &base);
        assert_eq!(resolved, PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn test_is_safe_path() {
        let temp_dir = env::temp_dir();
        let safe_path = temp_dir.join("test_file.txt");
        
        assert!(is_safe_path(&safe_path, &temp_dir));
    }

    #[test]
    fn read_file_rejects_a_path_outside_the_base_directory() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();

        let error = read_file(outside.path(), base.path()).unwrap_err();
        assert!(error.to_string().contains("outside allowed directory"));
    }

    #[test]
    fn write_file_rejects_a_path_outside_the_base_directory() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("not-allowed.txt");

        let error = write_file(&target, "secret", base.path()).unwrap_err();
        assert!(error.to_string().contains("outside allowed directory"));
        assert!(!target.exists());
    }

    #[test]
    fn write_file_resolves_relative_paths_against_the_base_directory() {
        let base = tempfile::tempdir().unwrap();

        write_file(Path::new("generated.txt"), "inside the repository", base.path()).unwrap();

        assert_eq!(
            fs::read_to_string(base.path().join("generated.txt")).unwrap(),
            "inside the repository"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_file_rejects_a_dangling_symlink_to_an_outside_target() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("new-file.txt");
        let link = base.path().join("looks-local.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = write_file(&link, "must not escape", base.path()).unwrap_err();

        assert!(error.to_string().contains("dangling symlink"));
        assert!(!target.exists());
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn write_file_rejects_an_existing_symlink_to_an_outside_target() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("existing.txt");
        fs::write(&target, "original").unwrap();
        let link = base.path().join("looks-local.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = write_file(&link, "must not escape", base.path()).unwrap_err();

        assert!(error.to_string().contains("outside allowed directory"));
        assert_eq!(fs::read_to_string(target).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn write_file_allows_a_symlink_to_an_existing_file_inside_the_base() {
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("existing.txt");
        fs::write(&target, "original").unwrap();
        let link = base.path().join("local-link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_file(&link, "updated", base.path()).unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "updated");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }
}
