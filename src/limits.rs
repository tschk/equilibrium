//! Shared read-size guards for headers, sources, and config.

use std::path::Path;

/// C header inputs (bindings, imports).
pub const MAX_HEADER_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_HEADER_LINES: usize = 200_000;

/// Source files scanned for export discovery (aligned with compiler cap).
pub const MAX_DISCOVERY_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

/// `equilibrium.toml` and similar config.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

pub fn check_file_size(path: &Path, max_bytes: u64) -> Result<u64, String> {
    if !path.is_file() {
        return Err(format!("Not a regular file: {}", path.display()));
    }
    let len = std::fs::metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?
        .len();
    if len > max_bytes {
        return Err(format!(
            "File too large ({} bytes; max {} bytes): {}",
            len,
            max_bytes,
            path.display()
        ));
    }
    Ok(len)
}

pub fn read_string_limited(path: &Path, max_bytes: u64) -> Result<String, String> {
    check_file_size(path, max_bytes)?;
    std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))
}

pub fn count_lines(content: &str) -> usize {
    content.lines().count()
}

pub fn ensure_line_limit(content: &str, max_lines: usize, path: &Path) -> Result<(), String> {
    let n = count_lines(content);
    if n > max_lines {
        return Err(format!(
            "File has too many lines ({n}; max {max_lines}): {}",
            path.display()
        ));
    }
    Ok(())
}

pub fn read_header_content(path: &Path) -> Result<String, String> {
    let content = read_string_limited(path, MAX_HEADER_BYTES)?;
    ensure_line_limit(&content, MAX_HEADER_LINES, path)?;
    Ok(content)
}

pub fn read_discovery_source(path: &Path) -> Result<String, String> {
    read_string_limited(path, MAX_DISCOVERY_SOURCE_BYTES)
}

pub fn read_config_text(path: &Path) -> Result<String, String> {
    read_string_limited(path, MAX_CONFIG_BYTES)
}
