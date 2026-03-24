//! Per-project binary cache for parsed conversation metadata.
//!
//! Stores parsed conversation data in bincode format, keyed by session filename
//! and validated by mtime + file size. Eliminates redundant JSONL parsing and
//! search text normalization on startup for unchanged files.

use super::{Conversation, ParseError};
use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_MAGIC: [u8; 8] = *b"CLHIST01";
const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct ProjectCache {
    magic: [u8; 8],
    schema_version: u32,
    entries: HashMap<String, CacheEntry>,
}

/// Cached conversation data — a dedicated DTO separate from Conversation
/// to avoid schema churn from UI/runtime field changes.
#[derive(Serialize, Deserialize, Clone)]
pub struct CacheEntry {
    pub file_size: u64,
    pub mtime_secs: u64,
    pub mtime_nsecs: u32,
    /// If true, this file was parsed but yielded no conversation (empty/clear-only).
    /// Avoids re-parsing known-empty files on every startup.
    #[serde(default)]
    pub is_empty: bool,
    pub preview_first: String,
    pub preview_last: String,
    pub full_text: String,
    pub search_text_lower: String,
    pub cwd: Option<PathBuf>,
    pub message_count: usize,
    pub parse_errors: Vec<CachedParseError>,
    pub summary: Option<String>,
    pub custom_title: Option<String>,
    pub model: Option<String>,
    pub total_tokens: u64,
    pub duration_minutes: Option<u64>,
    pub timestamp_epoch_ms: i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CachedParseError {
    pub line_number: usize,
    pub line_content: String,
    pub error_message: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

/// Get the cache directory for per-project cache files.
/// Respects CLAUDE_CONFIG_DIR to namespace caches per config root.
fn cache_dir() -> Option<PathBuf> {
    let base = home::home_dir()?.join(".cache").join("claude-history");
    if let Ok(config_dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        // Namespace by config dir to avoid cross-config cache collisions
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&config_dir, &mut hasher);
        let hash = std::hash::Hasher::finish(&hasher);
        Some(base.join(format!("config-{:016x}", hash)).join("projects"))
    } else {
        Some(base.join("projects"))
    }
}

/// Get the cache file path for a specific project
fn cache_path_for_project(project_dir_name: &str) -> Option<PathBuf> {
    cache_dir().map(|d| d.join(format!("{}.bin", project_dir_name)))
}

/// Read a project's cache file, returning entries keyed by session filename.
/// Returns None on any failure (missing, corrupt, version mismatch).
pub fn read_project_cache(project_dir_name: &str) -> Option<HashMap<String, CacheEntry>> {
    let path = cache_path_for_project(project_dir_name)?;
    let data = std::fs::read(&path).ok()?;
    if data.len() < 12 {
        return None;
    }
    if data[..8] != CACHE_MAGIC {
        return None;
    }
    let cache: ProjectCache = bincode::deserialize(&data).ok()?;
    if cache.schema_version != SCHEMA_VERSION {
        return None;
    }
    Some(cache.entries)
}

/// Write a project's cache file atomically (temp file + rename).
/// Uses tempfile for safe concurrent writes. Silently ignores failures.
pub fn write_project_cache(project_dir_name: &str, entries: HashMap<String, CacheEntry>) {
    let Some(path) = cache_path_for_project(project_dir_name) else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    let _ = std::fs::create_dir_all(parent);
    let cache = ProjectCache {
        magic: CACHE_MAGIC,
        schema_version: SCHEMA_VERSION,
        entries,
    };
    let Ok(data) = bincode::serialize(&cache) else {
        return;
    };
    // Use tempfile in the same directory for safe atomic rename
    let Ok(mut tmp) = tempfile::NamedTempFile::new_in(parent) else {
        return;
    };
    if tmp.write_all(&data).is_err() {
        return;
    }
    let _ = tmp.persist(&path);
}

/// Create a negative cache entry for files that parsed to no conversation
pub fn empty_entry(file_size: u64, mtime: SystemTime) -> CacheEntry {
    let duration_since_epoch = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
    CacheEntry {
        file_size,
        mtime_secs: duration_since_epoch.as_secs(),
        mtime_nsecs: duration_since_epoch.subsec_nanos(),
        is_empty: true,
        preview_first: String::new(),
        preview_last: String::new(),
        full_text: String::new(),
        search_text_lower: String::new(),
        cwd: None,
        message_count: 0,
        parse_errors: Vec::new(),
        summary: None,
        custom_title: None,
        model: None,
        total_tokens: 0,
        duration_minutes: None,
        timestamp_epoch_ms: 0,
    }
}

/// Create a CacheEntry from a parsed Conversation
pub fn entry_from_conversation(
    conv: &Conversation,
    file_size: u64,
    mtime: SystemTime,
) -> CacheEntry {
    let duration_since_epoch = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
    CacheEntry {
        file_size,
        mtime_secs: duration_since_epoch.as_secs(),
        mtime_nsecs: duration_since_epoch.subsec_nanos(),
        is_empty: false,
        preview_first: conv.preview_first.clone(),
        preview_last: conv.preview_last.clone(),
        full_text: conv.full_text.clone(),
        search_text_lower: conv.search_text_lower.clone(),
        cwd: conv.cwd.clone(),
        message_count: conv.message_count,
        parse_errors: conv
            .parse_errors
            .iter()
            .map(|e| CachedParseError {
                line_number: e.line_number,
                line_content: e.line_content.clone(),
                error_message: e.error_message.clone(),
                context_before: e.context_before.clone(),
                context_after: e.context_after.clone(),
            })
            .collect(),
        summary: conv.summary.clone(),
        custom_title: conv.custom_title.clone(),
        model: conv.model.clone(),
        total_tokens: conv.total_tokens,
        duration_minutes: conv.duration_minutes,
        timestamp_epoch_ms: conv.timestamp.timestamp_millis(),
    }
}

/// Reconstruct a Conversation from a CacheEntry
pub fn conversation_from_entry(entry: &CacheEntry, path: PathBuf, show_last: bool) -> Conversation {
    let timestamp = Local
        .timestamp_millis_opt(entry.timestamp_epoch_ms)
        .single()
        .unwrap_or_else(Local::now);
    let preview = if show_last {
        entry.preview_last.clone()
    } else {
        entry.preview_first.clone()
    };
    Conversation {
        path,
        index: 0,
        timestamp,
        preview,
        preview_first: entry.preview_first.clone(),
        preview_last: entry.preview_last.clone(),
        full_text: entry.full_text.clone(),
        search_text_lower: entry.search_text_lower.clone(),
        project_name: None,
        project_path: None,
        cwd: entry.cwd.clone(),
        message_count: entry.message_count,
        parse_errors: entry
            .parse_errors
            .iter()
            .map(|e| ParseError {
                line_number: e.line_number,
                line_content: e.line_content.clone(),
                error_message: e.error_message.clone(),
                context_before: e.context_before.clone(),
                context_after: e.context_after.clone(),
            })
            .collect(),
        summary: entry.summary.clone(),
        custom_title: entry.custom_title.clone(),
        model: entry.model.clone(),
        total_tokens: entry.total_tokens,
        duration_minutes: entry.duration_minutes,
    }
}

/// Check if a CacheEntry matches the given file metadata
pub fn entry_matches(entry: &CacheEntry, file_size: u64, mtime: SystemTime) -> bool {
    let duration_since_epoch = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
    entry.file_size == file_size
        && entry.mtime_secs == duration_since_epoch.as_secs()
        && entry.mtime_nsecs == duration_since_epoch.subsec_nanos()
}
