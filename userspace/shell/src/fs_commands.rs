//! File System Commands for the Kosh Shell
//!
//! Provides file system operations (ls, cd, pwd, mkdir, rmdir, rm, touch, cat)
//! that communicate with the FS service via the ServiceClient. Includes path
//! resolution and simulated responses for testing until real IPC is available.
//!
//! # Requirements
//! Implements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::error::{ShellError, ShellResult};
use crate::service_client::{ServiceClient, FileSystemRequest, ServiceResponse, ResponseStatus};

// ══════════════════════════════════════════════════════════════════════════════
// File Metadata Types
// ══════════════════════════════════════════════════════════════════════════════

/// Type of a file system entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Symlink,
    Device,
    Unknown,
}

impl FileType {
    /// Single-character indicator for display
    pub fn indicator(&self) -> char {
        match self {
            FileType::File => '-',
            FileType::Directory => 'd',
            FileType::Symlink => 'l',
            FileType::Device => 'c',
            FileType::Unknown => '?',
        }
    }

    /// Trailing character for ls output (e.g. '/' for dirs)
    pub fn suffix(&self) -> &'static str {
        match self {
            FileType::Directory => "/",
            FileType::Symlink => "@",
            _ => "",
        }
    }
}

/// Permission bits for a file system entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Permissions {
    pub fn all() -> Self {
        Self { read: true, write: true, execute: true }
    }

    pub fn read_only() -> Self {
        Self { read: true, write: false, execute: false }
    }

    pub fn format(&self) -> String {
        let mut s = String::with_capacity(3);
        s.push(if self.read { 'r' } else { '-' });
        s.push(if self.write { 'w' } else { '-' });
        s.push(if self.execute { 'x' } else { '-' });
        s
    }
}

/// Metadata for a single file system entry
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub file_type: FileType,
    pub size: u64,
    pub permissions: Permissions,
}

impl FileEntry {
    /// Format as a long-listing line (like `ls -l`)
    pub fn format_long(&self) -> String {
        format!(
            "{}{} {:>8}  {}",
            self.file_type.indicator(),
            self.permissions.format(),
            self.size,
            self.name,
        )
    }

    /// Format as a short name with type suffix
    pub fn format_short(&self) -> String {
        format!("{}{}", self.name, self.file_type.suffix())
    }
}


// ══════════════════════════════════════════════════════════════════════════════
// Flags
// ══════════════════════════════════════════════════════════════════════════════

/// Flags for the rm command
#[derive(Debug, Clone, Copy, Default)]
pub struct RmFlags {
    pub recursive: bool,
    pub force: bool,
}

/// Flags for the mkdir command
#[derive(Debug, Clone, Copy, Default)]
pub struct MkdirFlags {
    pub parents: bool,
}

// ══════════════════════════════════════════════════════════════════════════════
// Path Resolution
// ══════════════════════════════════════════════════════════════════════════════

/// Resolve `path` against `current_dir`, handling absolute, relative, `~`, and `.`/`..`.
pub fn resolve_path(path: &str, current_dir: &str, home_dir: &str) -> String {
    // Handle home directory expansion
    let expanded = if path == "~" {
        home_dir.to_string()
    } else if path.starts_with("~/") {
        format!("{}{}", home_dir, &path[1..])
    } else {
        path.to_string()
    };

    // Determine the base: absolute paths start from root, relative from current_dir
    let working = if expanded.starts_with('/') {
        expanded
    } else {
        if current_dir == "/" {
            format!("/{}", expanded)
        } else {
            format!("{}/{}", current_dir, expanded)
        }
    };

    // Normalize: resolve `.` and `..` components
    normalize_path(&working)
}

/// Normalize a path by resolving `.` and `..` components and collapsing slashes.
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();

    for component in path.split('/') {
        match component {
            "" | "." => { /* skip */ }
            ".." => { parts.pop(); }
            other => parts.push(other),
        }
    }

    if parts.is_empty() {
        return "/".to_string();
    }

    let mut result = String::new();
    for part in &parts {
        result.push('/');
        result.push_str(part);
    }
    result
}

// ══════════════════════════════════════════════════════════════════════════════
// Response Parsing Helpers
// ══════════════════════════════════════════════════════════════════════════════

/// Parse a service response into a list of file entries.
///
/// The FS service returns directory listings as text with one entry per line.
/// Each line may be a simple name or a structured format like:
///   `<type> <perms> <size> <name>`
/// Falls back to treating each line as a plain file name.
fn parse_listing_response(data: &str) -> Option<Vec<FileEntry>> {
    if data.is_empty() {
        return None;
    }

    let mut entries = Vec::new();
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Try structured format: "type perms size name"
        if let Some(entry) = parse_structured_entry(trimmed) {
            entries.push(entry);
        } else {
            // Plain name — infer type from trailing '/'
            let (name, file_type) = if trimmed.ends_with('/') {
                (trimmed.trim_end_matches('/').to_string(), FileType::Directory)
            } else {
                (trimmed.to_string(), FileType::File)
            };
            entries.push(FileEntry {
                name,
                file_type,
                size: 0,
                permissions: Permissions::all(),
            });
        }
    }

    if entries.is_empty() { None } else { Some(entries) }
}

/// Try to parse a structured entry line: `<type_char><perms> <size> <name>`
fn parse_structured_entry(line: &str) -> Option<FileEntry> {
    // Expected format: "drwx     4096  dirname" or "-rw-      256  file.txt"
    let parts: Vec<&str> = line.splitn(3, char::is_whitespace)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() < 3 {
        return None;
    }

    let type_perms = parts[0];
    if type_perms.len() < 4 {
        return None;
    }

    let file_type = match type_perms.as_bytes()[0] {
        b'd' => FileType::Directory,
        b'-' => FileType::File,
        b'l' => FileType::Symlink,
        b'c' => FileType::Device,
        _ => return None,
    };

    let perms_str = &type_perms[1..];
    let permissions = Permissions {
        read: perms_str.contains('r'),
        write: perms_str.contains('w'),
        execute: perms_str.contains('x'),
    };

    let size: u64 = parts[1].parse().ok()?;
    let name = parts[2].to_string();

    Some(FileEntry { name, file_type, size, permissions })
}

/// Check whether a service response indicates the path exists.
fn response_indicates_exists(response: &ServiceResponse) -> bool {
    response.is_success()
}

/// Map a non-success response status to an appropriate ShellError.
fn response_to_error(response: &ServiceResponse, path: &str) -> ShellError {
    match response.status {
        ResponseStatus::NotFound => ShellError::FileNotFound(path.to_string()),
        ResponseStatus::PermissionDenied => ShellError::PermissionDenied(path.to_string()),
        ResponseStatus::Timeout => ShellError::ServiceTimeout("fs_service".to_string()),
        ResponseStatus::InvalidRequest => ShellError::InvalidArguments(path.to_string()),
        ResponseStatus::Error => ShellError::InternalError(
            format!("FS service error for {}: {}", path, response.data),
        ),
        ResponseStatus::Success => {
            // Should not be called for success, but handle gracefully
            ShellError::InternalError("unexpected success status in error path".to_string())
        }
    }
}


// ══════════════════════════════════════════════════════════════════════════════
// Simulated FS Data (used until real IPC is wired up)
// ══════════════════════════════════════════════════════════════════════════════

/// Return simulated directory entries for well-known paths.
fn simulated_listing(path: &str) -> Option<Vec<FileEntry>> {
    match path {
        "/" => Some(alloc::vec![
            FileEntry { name: "bin".to_string(), file_type: FileType::Directory, size: 0, permissions: Permissions::all() },
            FileEntry { name: "dev".to_string(), file_type: FileType::Directory, size: 0, permissions: Permissions::all() },
            FileEntry { name: "etc".to_string(), file_type: FileType::Directory, size: 0, permissions: Permissions::all() },
            FileEntry { name: "home".to_string(), file_type: FileType::Directory, size: 0, permissions: Permissions::all() },
            FileEntry { name: "tmp".to_string(), file_type: FileType::Directory, size: 0, permissions: Permissions::all() },
            FileEntry { name: "var".to_string(), file_type: FileType::Directory, size: 0, permissions: Permissions::all() },
        ]),
        "/bin" => Some(alloc::vec![
            FileEntry { name: "shell".to_string(), file_type: FileType::File, size: 4096, permissions: Permissions::all() },
            FileEntry { name: "ls".to_string(), file_type: FileType::File, size: 2048, permissions: Permissions::all() },
            FileEntry { name: "cat".to_string(), file_type: FileType::File, size: 1024, permissions: Permissions::all() },
        ]),
        "/dev" => Some(alloc::vec![
            FileEntry { name: "console".to_string(), file_type: FileType::Device, size: 0, permissions: Permissions::read_only() },
            FileEntry { name: "keyboard".to_string(), file_type: FileType::Device, size: 0, permissions: Permissions::read_only() },
        ]),
        "/home" => Some(alloc::vec![
            FileEntry { name: "user".to_string(), file_type: FileType::Directory, size: 0, permissions: Permissions::all() },
        ]),
        "/home/user" => Some(alloc::vec![
            FileEntry { name: "readme.txt".to_string(), file_type: FileType::File, size: 256, permissions: Permissions::all() },
        ]),
        _ => None,
    }
}

/// Return simulated file content for well-known paths.
fn simulated_file_content(path: &str) -> Option<String> {
    match path {
        "/home/user/readme.txt" => Some("Welcome to Kosh OS!\n".to_string()),
        "/etc/hostname" => Some("kosh\n".to_string()),
        _ => None,
    }
}

/// Check whether a path is a known directory in the simulated FS.
fn simulated_path_exists(path: &str) -> bool {
    matches!(
        path,
        "/" | "/bin" | "/dev" | "/etc" | "/home" | "/home/user" | "/tmp" | "/var"
    )
}


// ══════════════════════════════════════════════════════════════════════════════
// FileSystemCommands
// ══════════════════════════════════════════════════════════════════════════════

/// High-level file system command handler.
///
/// Wraps a [`ServiceClient`] and tracks the current working directory.
/// Each method sends the appropriate request through the service client.
/// When the service returns real data it is used directly; otherwise the
/// implementation falls back to simulated data so the shell remains usable
/// before the IPC transport is fully wired up.
pub struct FileSystemCommands {
    client: ServiceClient,
    current_dir: String,
    previous_dir: String,
    home_dir: String,
}

impl FileSystemCommands {
    /// Create a new `FileSystemCommands` with the given service client.
    pub fn new(client: ServiceClient) -> Self {
        Self {
            client,
            current_dir: "/".to_string(),
            previous_dir: "/".to_string(),
            home_dir: "/home/user".to_string(),
        }
    }

    /// Create with a custom home directory.
    pub fn with_home(client: ServiceClient, home: &str) -> Self {
        Self {
            client,
            current_dir: "/".to_string(),
            previous_dir: "/".to_string(),
            home_dir: home.to_string(),
        }
    }

    /// Get the current working directory.
    pub fn current_dir(&self) -> &str {
        &self.current_dir
    }

    /// Get the previous working directory.
    pub fn previous_dir(&self) -> &str {
        &self.previous_dir
    }

    /// Get the home directory.
    pub fn home_dir(&self) -> &str {
        &self.home_dir
    }

    /// Get a mutable reference to the underlying service client.
    pub fn client_mut(&mut self) -> &mut ServiceClient {
        &mut self.client
    }

    // ── pwd ────────────────────────────────────────────────────────────

    /// Return the current working directory (Requirement 1.3).
    pub fn pwd(&self) -> ShellResult<String> {
        Ok(self.current_dir.clone())
    }

    // ── cd ─────────────────────────────────────────────────────────────

    /// Change the current working directory (Requirement 1.2).
    ///
    /// Supports:
    /// - Absolute paths (`/home/user`)
    /// - Relative paths (`../bin`)
    /// - `~` for home directory
    /// - `-` for previous directory
    /// - No argument defaults to home
    pub fn cd(&mut self, path: Option<&str>) -> ShellResult<String> {
        let target = match path {
            None | Some("") => self.home_dir.clone(),
            Some("-") => self.previous_dir.clone(),
            Some("~") => self.home_dir.clone(),
            Some(p) => resolve_path(p, &self.current_dir, &self.home_dir),
        };

        // Ask the FS service whether the target directory exists
        let response = self.client.send_fs_request(
            FileSystemRequest::Exists { path: target.clone() },
        );

        // Determine validity: prefer real service response, fall back to simulated check
        let path_valid = match &response {
            Ok(resp) if !resp.data.is_empty() => response_indicates_exists(resp),
            _ => simulated_path_exists(&target),
        };

        if !path_valid {
            return Err(ShellError::DirectoryNotFound(target));
        }

        let old_dir = self.current_dir.clone();
        self.previous_dir = old_dir;
        self.current_dir = target;

        Ok(self.current_dir.clone())
    }

    // ── ls ─────────────────────────────────────────────────────────────

    /// List directory contents with metadata (Requirement 1.1).
    ///
    /// When `long` is true, output includes type indicator, permissions, and size.
    pub fn ls(&mut self, path: Option<&str>, long: bool) -> ShellResult<String> {
        let resolved = match path {
            Some(p) => resolve_path(p, &self.current_dir, &self.home_dir),
            None => self.current_dir.clone(),
        };

        // Send listing request to the FS service
        let response = self.client.send_fs_request(
            FileSystemRequest::ListDir { path: resolved.clone() },
        );

        // Try to use real service data first
        let entries = match response {
            Ok(ref resp) if resp.is_success() && !resp.data.is_empty() => {
                parse_listing_response(&resp.data)
            }
            Ok(ref resp) if !resp.is_success() => {
                return Err(response_to_error(resp, &resolved));
            }
            _ => None,
        };

        // Fall back to simulated data when service returns empty/no data
        let entries = entries
            .or_else(|| simulated_listing(&resolved))
            .ok_or_else(|| ShellError::DirectoryNotFound(resolved.clone()))?;

        let mut output = String::new();
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            if long {
                output.push_str(&entry.format_long());
            } else {
                output.push_str(&entry.format_short());
            }
        }

        Ok(output)
    }

    // ── cat ────────────────────────────────────────────────────────────

    /// Read and return file contents (Requirement 1.8).
    pub fn cat(&mut self, path: &str) -> ShellResult<String> {
        if path.is_empty() {
            return Err(ShellError::InvalidArguments("cat: missing file operand".to_string()));
        }

        let resolved = resolve_path(path, &self.current_dir, &self.home_dir);

        // Request file content from the FS service
        let response = self.client.send_fs_request(
            FileSystemRequest::ReadFile { path: resolved.clone() },
        );

        // Use real data when available
        match response {
            Ok(ref resp) if resp.is_success() && !resp.data.is_empty() => {
                return Ok(resp.data.clone());
            }
            Ok(ref resp) if !resp.is_success() => {
                // Check simulated fallback before returning error
                if let Some(content) = simulated_file_content(&resolved) {
                    return Ok(content);
                }
                return Err(response_to_error(resp, &resolved));
            }
            _ => {}
        }

        // Fall back to simulated content
        simulated_file_content(&resolved)
            .ok_or_else(|| ShellError::FileNotFound(resolved))
    }

    // ── write ──────────────────────────────────────────────────────────

    /// Write data to a file through the FS service (Requirement 1.8 extended).
    ///
    /// Creates the file if it does not exist.
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> ShellResult<String> {
        if path.is_empty() {
            return Err(ShellError::InvalidArguments("write: missing file operand".to_string()));
        }

        let resolved = resolve_path(path, &self.current_dir, &self.home_dir);

        let response = self.client.send_fs_request(
            FileSystemRequest::WriteFile {
                path: resolved.clone(),
                data: data.to_vec(),
            },
        );

        match response {
            Ok(ref resp) if resp.is_success() => {
                Ok(format!("Wrote {} bytes to {}", data.len(), resolved))
            }
            Ok(ref resp) => Err(response_to_error(resp, &resolved)),
            Err(e) => Err(ShellError::from(e)),
        }
    }

    // ── mkdir ──────────────────────────────────────────────────────────

    /// Create a directory (Requirement 1.4).
    ///
    /// When `flags.parents` is true, create intermediate directories as needed.
    pub fn mkdir(&mut self, path: &str, flags: MkdirFlags) -> ShellResult<String> {
        if path.is_empty() {
            return Err(ShellError::InvalidArguments("mkdir: missing operand".to_string()));
        }

        let resolved = resolve_path(path, &self.current_dir, &self.home_dir);

        if flags.parents {
            // Create each component along the way
            let mut accumulated = String::new();
            for component in resolved.split('/').filter(|c| !c.is_empty()) {
                accumulated.push('/');
                accumulated.push_str(component);
                let response = self.client.send_fs_request(
                    FileSystemRequest::CreateDir {
                        path: accumulated.clone(),
                        recursive: false,
                    },
                );
                // Ignore "already exists" errors when creating parents
                if let Ok(ref resp) = response {
                    if !resp.is_success() && resp.status != ResponseStatus::Error {
                        return Err(response_to_error(resp, &accumulated));
                    }
                }
            }
        } else {
            let response = self.client.send_fs_request(
                FileSystemRequest::CreateDir {
                    path: resolved.clone(),
                    recursive: false,
                },
            );
            if let Ok(ref resp) = response {
                if !resp.is_success() {
                    return Err(response_to_error(resp, &resolved));
                }
            }
        }

        Ok(format!("Created directory: {}", resolved))
    }

    // ── rmdir ──────────────────────────────────────────────────────────

    /// Remove an empty directory (Requirement 1.5).
    pub fn rmdir(&mut self, path: &str) -> ShellResult<String> {
        if path.is_empty() {
            return Err(ShellError::InvalidArguments("rmdir: missing operand".to_string()));
        }

        let resolved = resolve_path(path, &self.current_dir, &self.home_dir);

        let response = self.client.send_fs_request(
            FileSystemRequest::DeleteDir {
                path: resolved.clone(),
                recursive: false,
            },
        );

        if let Ok(ref resp) = response {
            if !resp.is_success() {
                return Err(response_to_error(resp, &resolved));
            }
        }

        Ok(format!("Removed directory: {}", resolved))
    }

    // ── rm ─────────────────────────────────────────────────────────────

    /// Remove a file or directory (Requirement 1.6).
    ///
    /// With `flags.recursive`, removes directories and their contents.
    /// With `flags.force`, suppresses errors for non-existent files.
    pub fn rm(&mut self, path: &str, flags: RmFlags) -> ShellResult<String> {
        if path.is_empty() {
            return Err(ShellError::InvalidArguments("rm: missing operand".to_string()));
        }

        let resolved = resolve_path(path, &self.current_dir, &self.home_dir);

        if flags.recursive {
            let response = self.client.send_fs_request(
                FileSystemRequest::DeleteDir {
                    path: resolved.clone(),
                    recursive: true,
                },
            );
            if let Ok(ref resp) = response {
                if !resp.is_success() && !flags.force {
                    return Err(response_to_error(resp, &resolved));
                }
            }
        } else {
            let result = self.client.send_fs_request(
                FileSystemRequest::DeleteFile { path: resolved.clone() },
            );
            if !flags.force {
                match result {
                    Ok(ref resp) if !resp.is_success() => {
                        return Err(response_to_error(resp, &resolved));
                    }
                    Err(e) => return Err(ShellError::from(e)),
                    _ => {}
                }
            }
        }

        Ok(format!("Removed: {}", resolved))
    }

    // ── touch ──────────────────────────────────────────────────────────

    /// Create an empty file or update timestamps (Requirement 1.7).
    pub fn touch(&mut self, path: &str) -> ShellResult<String> {
        if path.is_empty() {
            return Err(ShellError::InvalidArguments("touch: missing file operand".to_string()));
        }

        let resolved = resolve_path(path, &self.current_dir, &self.home_dir);

        let response = self.client.send_fs_request(
            FileSystemRequest::Touch { path: resolved.clone() },
        );

        if let Ok(ref resp) = response {
            if !resp.is_success() {
                return Err(response_to_error(resp, &resolved));
            }
        }

        Ok(format!("Touched: {}", resolved))
    }
}


// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    // ── Path Resolution ───────────────────────────────────────────────

    #[test]
    fn test_resolve_absolute_path() {
        assert_eq!(resolve_path("/usr/bin", "/home", "/home/user"), "/usr/bin");
    }

    #[test]
    fn test_resolve_relative_path() {
        assert_eq!(resolve_path("docs", "/home/user", "/home/user"), "/home/user/docs");
    }

    #[test]
    fn test_resolve_relative_from_root() {
        assert_eq!(resolve_path("bin", "/", "/home/user"), "/bin");
    }

    #[test]
    fn test_resolve_dotdot() {
        assert_eq!(resolve_path("..", "/home/user", "/home/user"), "/home");
    }

    #[test]
    fn test_resolve_dot() {
        assert_eq!(resolve_path(".", "/home/user", "/home/user"), "/home/user");
    }

    #[test]
    fn test_resolve_complex_relative() {
        assert_eq!(
            resolve_path("../bin/../etc", "/home/user", "/home/user"),
            "/home/etc"
        );
    }

    #[test]
    fn test_resolve_tilde() {
        assert_eq!(resolve_path("~", "/tmp", "/home/user"), "/home/user");
    }

    #[test]
    fn test_resolve_tilde_subpath() {
        assert_eq!(resolve_path("~/docs", "/tmp", "/home/user"), "/home/user/docs");
    }

    #[test]
    fn test_resolve_dotdot_past_root() {
        assert_eq!(resolve_path("../../..", "/home", "/home/user"), "/");
    }

    #[test]
    fn test_normalize_trailing_slash() {
        assert_eq!(resolve_path("/home/user/", "/", "/home/user"), "/home/user");
    }

    #[test]
    fn test_normalize_double_slash() {
        assert_eq!(resolve_path("/home//user", "/", "/home/user"), "/home/user");
    }

    // ── FileEntry Formatting ──────────────────────────────────────────

    #[test]
    fn test_file_entry_format_short_dir() {
        let entry = FileEntry {
            name: "bin".to_string(),
            file_type: FileType::Directory,
            size: 0,
            permissions: Permissions::all(),
        };
        assert_eq!(entry.format_short(), "bin/");
    }

    #[test]
    fn test_file_entry_format_short_file() {
        let entry = FileEntry {
            name: "readme.txt".to_string(),
            file_type: FileType::File,
            size: 100,
            permissions: Permissions::read_only(),
        };
        assert_eq!(entry.format_short(), "readme.txt");
    }

    #[test]
    fn test_file_entry_format_long() {
        let entry = FileEntry {
            name: "test".to_string(),
            file_type: FileType::File,
            size: 1024,
            permissions: Permissions { read: true, write: true, execute: false },
        };
        let long = entry.format_long();
        assert!(long.contains("-rw-"));
        assert!(long.contains("1024"));
        assert!(long.contains("test"));
    }

    #[test]
    fn test_permissions_format() {
        assert_eq!(Permissions::all().format(), "rwx");
        assert_eq!(Permissions::read_only().format(), "r--");
        assert_eq!(
            (Permissions { read: false, write: true, execute: false }).format(),
            "-w-"
        );
    }

    #[test]
    fn test_file_type_indicator() {
        assert_eq!(FileType::File.indicator(), '-');
        assert_eq!(FileType::Directory.indicator(), 'd');
        assert_eq!(FileType::Symlink.indicator(), 'l');
        assert_eq!(FileType::Device.indicator(), 'c');
    }

    // ── Response Parsing ──────────────────────────────────────────────

    #[test]
    fn test_parse_listing_response_empty() {
        assert!(parse_listing_response("").is_none());
    }

    #[test]
    fn test_parse_listing_response_plain_names() {
        let data = "file1.txt\nsubdir/\nfile2.log";
        let entries = parse_listing_response(data).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "file1.txt");
        assert_eq!(entries[0].file_type, FileType::File);
        assert_eq!(entries[1].name, "subdir");
        assert_eq!(entries[1].file_type, FileType::Directory);
        assert_eq!(entries[2].name, "file2.log");
        assert_eq!(entries[2].file_type, FileType::File);
    }

    #[test]
    fn test_parse_listing_response_structured() {
        let data = "drwx 4096 mydir\n-r-- 256 readme.txt";
        let entries = parse_listing_response(data).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file_type, FileType::Directory);
        assert_eq!(entries[0].size, 4096);
        assert_eq!(entries[0].name, "mydir");
        assert!(entries[0].permissions.write);
        assert_eq!(entries[1].file_type, FileType::File);
        assert_eq!(entries[1].size, 256);
        assert!(!entries[1].permissions.write);
    }

    #[test]
    fn test_parse_structured_entry_invalid() {
        assert!(parse_structured_entry("short").is_none());
        assert!(parse_structured_entry("xy").is_none());
    }

    #[test]
    fn test_response_to_error_variants() {
        let resp = ServiceResponse {
            request_id: 1,
            status: ResponseStatus::NotFound,
            data: String::new(),
            raw_data: None,
        };
        assert!(matches!(response_to_error(&resp, "/x"), ShellError::FileNotFound(_)));

        let resp = ServiceResponse {
            request_id: 1,
            status: ResponseStatus::PermissionDenied,
            data: String::new(),
            raw_data: None,
        };
        assert!(matches!(response_to_error(&resp, "/x"), ShellError::PermissionDenied(_)));
    }

    // ── FileSystemCommands ────────────────────────────────────────────

    fn make_fs() -> FileSystemCommands {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();
        FileSystemCommands::new(client)
    }

    #[test]
    fn test_pwd_default() {
        let fs = make_fs();
        assert_eq!(fs.pwd().unwrap(), "/");
    }

    #[test]
    fn test_cd_absolute() {
        let mut fs = make_fs();
        let result = fs.cd(Some("/home/user"));
        assert!(result.is_ok());
        assert_eq!(fs.current_dir(), "/home/user");
    }

    #[test]
    fn test_cd_relative() {
        let mut fs = make_fs();
        fs.cd(Some("/home")).unwrap();
        fs.cd(Some("user")).unwrap();
        assert_eq!(fs.current_dir(), "/home/user");
    }

    #[test]
    fn test_cd_dash_returns_to_previous() {
        let mut fs = make_fs();
        fs.cd(Some("/home")).unwrap();
        fs.cd(Some("/tmp")).unwrap();
        fs.cd(Some("-")).unwrap();
        assert_eq!(fs.current_dir(), "/home");
    }

    #[test]
    fn test_cd_tilde() {
        let mut fs = make_fs();
        fs.cd(Some("/tmp")).unwrap();
        fs.cd(Some("~")).unwrap();
        assert_eq!(fs.current_dir(), "/home/user");
    }

    #[test]
    fn test_cd_none_goes_home() {
        let mut fs = make_fs();
        fs.cd(Some("/tmp")).unwrap();
        fs.cd(None).unwrap();
        assert_eq!(fs.current_dir(), "/home/user");
    }

    #[test]
    fn test_cd_dotdot() {
        let mut fs = make_fs();
        fs.cd(Some("/home/user")).unwrap();
        fs.cd(Some("..")).unwrap();
        assert_eq!(fs.current_dir(), "/home");
    }

    #[test]
    fn test_cd_nonexistent_returns_error() {
        let mut fs = make_fs();
        let result = fs.cd(Some("/nonexistent/path"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ShellError::DirectoryNotFound(_)));
    }

    #[test]
    fn test_ls_root() {
        let mut fs = make_fs();
        let output = fs.ls(None, false).unwrap();
        assert!(output.contains("bin/"));
        assert!(output.contains("home/"));
    }

    #[test]
    fn test_ls_long_format() {
        let mut fs = make_fs();
        let output = fs.ls(Some("/bin"), true).unwrap();
        assert!(output.contains("-rwx"));
        assert!(output.contains("shell"));
    }

    #[test]
    fn test_ls_nonexistent() {
        let mut fs = make_fs();
        let result = fs.ls(Some("/nonexistent"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_ls_relative_path() {
        let mut fs = make_fs();
        fs.cd(Some("/")).unwrap();
        let output = fs.ls(Some("bin"), false).unwrap();
        assert!(output.contains("shell"));
    }

    #[test]
    fn test_cat_existing_file() {
        let mut fs = make_fs();
        let content = fs.cat("/home/user/readme.txt").unwrap();
        assert!(content.contains("Welcome to Kosh OS!"));
    }

    #[test]
    fn test_cat_nonexistent_file() {
        let mut fs = make_fs();
        let result = fs.cat("/no/such/file");
        assert!(result.is_err());
    }

    #[test]
    fn test_cat_empty_path() {
        let mut fs = make_fs();
        let result = fs.cat("");
        assert!(result.is_err());
    }

    #[test]
    fn test_cat_relative_path() {
        let mut fs = make_fs();
        fs.cd(Some("/home/user")).unwrap();
        let content = fs.cat("readme.txt").unwrap();
        assert!(content.contains("Welcome"));
    }

    #[test]
    fn test_write_file_basic() {
        let mut fs = make_fs();
        let result = fs.write_file("/tmp/out.txt", b"hello");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("5 bytes"));
    }

    #[test]
    fn test_write_file_empty_path() {
        let mut fs = make_fs();
        let result = fs.write_file("", b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_mkdir_basic() {
        let mut fs = make_fs();
        let result = fs.mkdir("/tmp/newdir", MkdirFlags::default());
        assert!(result.is_ok());
        assert!(result.unwrap().contains("/tmp/newdir"));
    }

    #[test]
    fn test_mkdir_parents() {
        let mut fs = make_fs();
        let result = fs.mkdir("/tmp/a/b/c", MkdirFlags { parents: true });
        assert!(result.is_ok());
    }

    #[test]
    fn test_mkdir_empty_path() {
        let mut fs = make_fs();
        let result = fs.mkdir("", MkdirFlags::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_rmdir_basic() {
        let mut fs = make_fs();
        let result = fs.rmdir("/tmp/olddir");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("/tmp/olddir"));
    }

    #[test]
    fn test_rmdir_empty_path() {
        let mut fs = make_fs();
        let result = fs.rmdir("");
        assert!(result.is_err());
    }

    #[test]
    fn test_rm_basic() {
        let mut fs = make_fs();
        let result = fs.rm("/tmp/file.txt", RmFlags::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_rm_recursive() {
        let mut fs = make_fs();
        let result = fs.rm("/tmp/dir", RmFlags { recursive: true, force: false });
        assert!(result.is_ok());
    }

    #[test]
    fn test_rm_empty_path() {
        let mut fs = make_fs();
        let result = fs.rm("", RmFlags::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_touch_basic() {
        let mut fs = make_fs();
        let result = fs.touch("/tmp/newfile.txt");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("/tmp/newfile.txt"));
    }

    #[test]
    fn test_touch_empty_path() {
        let mut fs = make_fs();
        let result = fs.touch("");
        assert!(result.is_err());
    }

    #[test]
    fn test_touch_relative_path() {
        let mut fs = make_fs();
        fs.cd(Some("/tmp")).unwrap();
        let result = fs.touch("file.txt");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("/tmp/file.txt"));
    }

    #[test]
    fn test_home_dir_accessor() {
        let fs = make_fs();
        assert_eq!(fs.home_dir(), "/home/user");
    }

    #[test]
    fn test_with_home_constructor() {
        let client = ServiceClient::new();
        let fs = FileSystemCommands::with_home(client, "/root");
        assert_eq!(fs.home_dir(), "/root");
    }
}
