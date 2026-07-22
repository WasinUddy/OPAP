// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2026 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR-SQL concepts:
// https://gitlab.com/CrimsonNape/OSCAR-SQL
// Upstream commit: 3741e5b423e4b5796c51a9d447e83b2525963d50
// Relevant upstream files: oscar/SleepLib/importcontext.h,
// oscar/SleepLib/machine_loader.h
// Modified: 2026-07-22

//! Filesystem-independent importer interfaces.
//!
//! An importer receives an [`ImportSource`] instead of a native path. Desktop
//! builds can use [`DirectorySource`], while a future WebAssembly wrapper can
//! provide an in-memory implementation backed by browser-selected files.

use crate::domain::{DeviceInfo, ImportReport, ImportWarning, UnixMillis};
use serde::{Deserialize, Serialize};
use std::fmt;

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
use cap_std::{
    ambient_authority,
    fs::{Dir, File, OpenOptions},
};
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
use std::{
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

/// Kind of item in an import source inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEntryKind {
    /// A readable file.
    File,
    /// A directory used to organize files.
    Directory,
}

/// Metadata for one source-relative file or directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntry {
    /// Forward-slash-separated path relative to the source root.
    pub relative_path: String,
    /// Whether the entry is a file or directory.
    pub kind: SourceEntryKind,
    /// File size in bytes. Directories use zero.
    pub size_bytes: u64,
}

/// Deterministically ordered contents of an import source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInventory {
    /// Entries sorted by normalized relative path.
    pub entries: Vec<SourceEntry>,
    /// Sum of all file sizes in [`Self::entries`].
    pub total_file_bytes: u64,
}

impl SourceInventory {
    /// Returns whether the inventory contains a file at `relative_path`.
    #[must_use]
    pub fn has_file(&self, relative_path: &str) -> bool {
        self.has_entry(relative_path, SourceEntryKind::File)
    }

    /// Returns whether a directory exists explicitly or is implied by a child.
    ///
    /// Browser directory pickers generally return files but not directory
    /// entries, so checking the child prefix is important for WebAssembly hosts.
    #[must_use]
    pub fn has_directory(&self, relative_path: &str) -> bool {
        let normalized = relative_path.trim_end_matches('/');
        self.has_entry(normalized, SourceEntryKind::Directory)
            || self.entries.iter().any(|entry| {
                entry
                    .relative_path
                    .strip_prefix(normalized)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            })
    }

    fn has_entry(&self, relative_path: &str, kind: SourceEntryKind) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.kind == kind && entry.relative_path == relative_path)
    }
}

/// Portable error category exposed across the importer boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportErrorKind {
    /// The source could not be listed or read.
    Source,
    /// A requested relative path was invalid or unsafe.
    InvalidPath,
    /// The source is not recognized by the selected importer.
    UnsupportedSource,
    /// A recognized file is malformed.
    InvalidData,
    /// A source file exceeded the maximum size allowed for that read.
    SizeLimitExceeded,
    /// The requested import operation has not been implemented.
    UnsupportedOperation,
}

/// Serializable error returned by source adapters and importers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportError {
    /// Stable error category.
    pub kind: ImportErrorKind,
    /// Human-readable context.
    pub message: String,
    /// Source-relative path associated with the failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
}

impl ImportError {
    /// Creates an importer error without an associated path.
    #[must_use]
    pub fn new(kind: ImportErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            relative_path: None,
        }
    }

    /// Associates this error with a normalized source-relative path.
    #[must_use]
    pub fn at_path(mut self, relative_path: impl Into<String>) -> Self {
        self.relative_path = Some(relative_path.into());
        self
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.relative_path {
            write!(formatter, "{} ({path})", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for ImportError {}

/// Read-only source of device files.
///
/// Paths passed to [`Self::read_file`] must use the normalized relative paths
/// returned by [`Self::inventory`]. The trait is synchronous so it is object
/// safe and can be implemented by native folders or browser-owned byte maps.
pub trait ImportSource {
    /// Lists all available files and directories.
    fn inventory(&self) -> Result<SourceInventory, ImportError>;

    /// Reads one complete source-relative file, up to `max_bytes`.
    ///
    /// Implementations must return [`ImportErrorKind::SizeLimitExceeded`]
    /// without returning file contents when the file is larger than the limit.
    /// Importers should still verify the returned length so an untrusted source
    /// adapter cannot bypass parser-specific limits.
    fn read_file(&self, relative_path: &str, max_bytes: usize) -> Result<Vec<u8>, ImportError>;
}

/// Import options shared by device loaders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOptions {
    /// Skip sessions that end before this UTC timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions_not_before_unix_ms: Option<UnixMillis>,
    /// Include high-resolution waveform samples in returned sessions.
    pub include_waveforms: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            sessions_not_before_unix_ms: None,
            include_waveforms: true,
        }
    }
}

/// Result of probing an import source without importing therapy sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDiscovery {
    /// Recognized device and importer identity.
    pub device: DeviceInfo,
    /// Complete source inventory used during detection.
    pub inventory: SourceInventory,
    /// Non-fatal discovery diagnostics.
    pub warnings: Vec<ImportWarning>,
}

/// Device-format parser independent of its storage host.
pub trait Importer {
    /// Stable identifier persisted with imported data.
    fn id(&self) -> &'static str;

    /// Probes a source, returning `None` when this importer does not recognize it.
    fn discover(&self, source: &dyn ImportSource) -> Result<Option<DeviceDiscovery>, ImportError>;

    /// Imports all eligible sessions from a recognized source.
    fn import(
        &self,
        source: &dyn ImportSource,
        options: &ImportOptions,
    ) -> Result<ImportReport, ImportError>;
}

/// Native [`ImportSource`] rooted at an open directory capability.
///
/// Every lookup is relative to the captured directory handle. Directory and
/// file symlinks are not followed, eliminating the check-then-open race present
/// in path/canonicalize based confinement.
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
#[derive(Debug)]
pub struct DirectorySource {
    root: PathBuf,
    directory: Dir,
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
impl DirectorySource {
    /// Opens `path` once and captures it as a directory capability.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ImportError> {
        let root = path.into();
        let directory = Dir::open_ambient_dir(&root, ambient_authority())
            .map_err(|source| root_io_error(&root, source, None))?;
        Ok(Self { root, directory })
    }

    /// Alias for [`Self::open`] retained for callers that used `new`.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, ImportError> {
        Self::open(path)
    }

    /// Returns the native root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn io_error(&self, source: io::Error, relative_path: Option<&str>) -> ImportError {
        root_io_error(&self.root, source, relative_path)
    }

    fn open_file_nofollow(&self, relative_path: &Path) -> Result<File, io::Error> {
        let mut directory = self.directory.try_clone()?;
        let mut components = relative_path.components().peekable();

        while let Some(component) = components.next() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "path is not normalized",
                ));
            };
            if components.peek().is_some() {
                directory = directory.open_dir_nofollow(name)?;
            } else {
                let mut options = OpenOptions::new();
                options.read(true).follow(FollowSymlinks::No);
                return directory.open_with(name, &options);
            }
        }

        Err(io::Error::new(io::ErrorKind::InvalidInput, "path is empty"))
    }
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
impl ImportSource for DirectorySource {
    fn inventory(&self) -> Result<SourceInventory, ImportError> {
        let mut entries = Vec::new();
        collect_entries(self, &self.directory, "", &mut entries)?;
        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let total_file_bytes = entries
            .iter()
            .filter(|entry| entry.kind == SourceEntryKind::File)
            .map(|entry| entry.size_bytes)
            .sum();

        Ok(SourceInventory {
            entries,
            total_file_bytes,
        })
    }

    fn read_file(&self, relative_path: &str, max_bytes: usize) -> Result<Vec<u8>, ImportError> {
        let path = safe_relative_path(relative_path)?;
        let file = self
            .open_file_nofollow(path)
            .map_err(|source| self.io_error(source, Some(relative_path)))?;
        let metadata = file
            .metadata()
            .map_err(|source| self.io_error(source, Some(relative_path)))?;
        if !metadata.is_file() {
            return Err(ImportError::new(
                ImportErrorKind::InvalidPath,
                "source path does not identify a regular file",
            )
            .at_path(relative_path));
        }
        let max_bytes_u64 = u64::try_from(max_bytes).unwrap_or(u64::MAX);
        if metadata.len() > max_bytes_u64 {
            return Err(size_limit_error(
                relative_path,
                max_bytes,
                Some(metadata.len()),
            ));
        }

        let mut bytes = Vec::with_capacity(
            usize::try_from(metadata.len())
                .unwrap_or(max_bytes)
                .min(max_bytes),
        );
        file.take(max_bytes_u64.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| self.io_error(source, Some(relative_path)))?;
        if bytes.len() > max_bytes {
            return Err(size_limit_error(
                relative_path,
                max_bytes,
                u64::try_from(bytes.len()).ok(),
            ));
        }
        Ok(bytes)
    }
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
fn collect_entries(
    source: &DirectorySource,
    directory: &Dir,
    relative_directory: &str,
    entries: &mut Vec<SourceEntry>,
) -> Result<(), ImportError> {
    let children = directory.entries().map_err(|error| {
        source.io_error(
            error,
            (!relative_directory.is_empty()).then_some(relative_directory),
        )
    })?;

    for child in children {
        let child = child.map_err(|error| source.io_error(error, Some(relative_directory)))?;
        let file_type = child
            .file_type()
            .map_err(|error| source.io_error(error, Some(relative_directory)))?;
        if !file_type.is_file() && !file_type.is_dir() {
            continue;
        }

        let name = child.file_name();
        let normalized_name = name
            .to_str()
            .ok_or_else(|| {
                ImportError::new(
                    ImportErrorKind::InvalidPath,
                    "source contains a path that is not valid UTF-8",
                )
            })?
            .to_owned();
        let relative_path = if relative_directory.is_empty() {
            normalized_name
        } else {
            format!("{relative_directory}/{normalized_name}")
        };

        if file_type.is_dir() {
            let child_directory = directory
                .open_dir_nofollow(&name)
                .map_err(|error| source.io_error(error, Some(&relative_path)))?;
            entries.push(SourceEntry {
                relative_path: relative_path.clone(),
                kind: SourceEntryKind::Directory,
                size_bytes: 0,
            });
            collect_entries(source, &child_directory, &relative_path, entries)?;
        } else {
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let file = child
                .open_with(&options)
                .map_err(|error| source.io_error(error, Some(&relative_path)))?;
            let metadata = file
                .metadata()
                .map_err(|error| source.io_error(error, Some(&relative_path)))?;
            if !metadata.is_file() {
                continue;
            }
            entries.push(SourceEntry {
                relative_path,
                kind: SourceEntryKind::File,
                size_bytes: metadata.len(),
            });
        }
    }

    Ok(())
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
fn safe_relative_path(relative_path: &str) -> Result<&Path, ImportError> {
    let path = Path::new(relative_path);
    if relative_path.is_empty()
        || relative_path.contains('\\')
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ImportError::new(
            ImportErrorKind::InvalidPath,
            "source path must be a normalized relative path",
        )
        .at_path(relative_path));
    }
    Ok(path)
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
fn root_io_error(root: &Path, source: io::Error, relative_path: Option<&str>) -> ImportError {
    let target = relative_path
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| root.display().to_string());
    let error = ImportError::new(
        ImportErrorKind::Source,
        format!("failed to access {target}: {source}"),
    );
    match relative_path {
        Some(path) => error.at_path(path),
        None => error,
    }
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
fn size_limit_error(relative_path: &str, max_bytes: usize, actual: Option<u64>) -> ImportError {
    let actual = actual.map_or_else(String::new, |bytes| format!("; found {bytes} bytes"));
    ImportError::new(
        ImportErrorKind::SizeLimitExceeded,
        format!("source file exceeds the {max_bytes}-byte read limit{actual}"),
    )
    .at_path(relative_path)
}

#[cfg(all(test, feature = "native-fs", not(target_family = "wasm")))]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn directory_source_lists_normalized_sorted_entries() {
        let root = TempDir::new().expect("temporary directory");
        fs::create_dir_all(root.path().join("DATALOG/night")).expect("nested directories");
        fs::write(root.path().join("z.edf"), b"123").expect("z file");
        fs::write(root.path().join("DATALOG/night/a.edf"), b"12345").expect("a file");

        let inventory = DirectorySource::open(root.path())
            .expect("open source")
            .inventory()
            .expect("inventory");

        assert_eq!(inventory.total_file_bytes, 8);
        assert_eq!(
            inventory
                .entries
                .iter()
                .map(|entry| entry.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["DATALOG", "DATALOG/night", "DATALOG/night/a.edf", "z.edf"]
        );
        assert!(inventory.has_directory("DATALOG"));
        assert!(inventory.has_file("z.edf"));
    }

    #[test]
    fn implied_directories_support_browser_style_inventories() {
        let inventory = SourceInventory {
            entries: vec![SourceEntry {
                relative_path: "DATALOG/20260101/example.edf".to_owned(),
                kind: SourceEntryKind::File,
                size_bytes: 0,
            }],
            total_file_bytes: 0,
        };

        assert!(inventory.has_directory("DATALOG"));
        assert!(inventory.has_directory("DATALOG/20260101"));
        assert!(!inventory.has_directory("DATA"));
    }

    #[test]
    fn directory_source_reads_inventory_paths_and_rejects_traversal() {
        let root = TempDir::new().expect("temporary directory");
        fs::write(root.path().join("file.edf"), b"contents").expect("source file");
        let source = DirectorySource::open(root.path()).expect("open source");

        assert_eq!(source.read_file("file.edf", 8).expect("read"), b"contents");
        let error = source
            .read_file("file.edf", 7)
            .expect_err("enforce size limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);
        let error = source
            .read_file("../file.edf", 8)
            .expect_err("reject traversal");
        assert_eq!(error.kind, ImportErrorKind::InvalidPath);
    }

    #[cfg(unix)]
    #[test]
    fn directory_source_never_follows_file_or_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().expect("source directory");
        let outside = TempDir::new().expect("outside directory");
        fs::write(outside.path().join("secret"), b"not card data").expect("outside file");
        symlink(outside.path().join("secret"), root.path().join("file-link"))
            .expect("file symlink");
        symlink(outside.path(), root.path().join("directory-link")).expect("directory symlink");
        let source = DirectorySource::open(root.path()).expect("open source");

        assert!(source.read_file("file-link", 64).is_err());
        assert!(source.read_file("directory-link/secret", 64).is_err());
        let inventory = source.inventory().expect("safe inventory");
        assert!(!inventory.has_file("file-link"));
        assert!(!inventory.has_directory("directory-link"));
    }
}
