//! Codex CLI conversation export.
//!
//! Codex stores rollout JSONL files under ~/.codex/sessions/YYYY/MM/DD. This
//! exporter scans those files, filters by the session working directory, and
//! renders only user/assistant conversation messages to Markdown.

use crate::error::{AppError, Result};
use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug)]
struct CodexSession {
    path: PathBuf,
    id: Option<String>,
    timestamp: Option<DateTime<Local>>,
    last_activity: Option<DateTime<Local>>,
    modified: SystemTime,
    cwd: PathBuf,
    cli_version: Option<String>,
    turns: Vec<CodexTurn>,
}

#[derive(Debug)]
struct CodexTurn {
    role: CodexRole,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexRole {
    User,
    Assistant,
}

impl CodexRole {
    fn markdown_label(self) -> &'static str {
        match self {
            CodexRole::User => "You",
            CodexRole::Assistant => "Codex",
        }
    }
}

impl CodexSession {
    fn activity_timestamp(&self) -> DateTime<Local> {
        self.last_activity
            .or(self.timestamp)
            .unwrap_or_else(|| DateTime::<Local>::from(self.modified))
    }
}

#[derive(Debug, Deserialize)]
struct CodexEnvelope {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct SessionMetaPayload {
    id: Option<String>,
    timestamp: Option<String>,
    cwd: Option<PathBuf>,
    cli_version: Option<String>,
}

/// Export all Codex CLI conversations for a project directory to Markdown files.
pub fn export_project_markdown(project_dir: &Path, output_dir: &Path) -> Result<()> {
    let project_dir = project_dir.canonicalize().map_err(AppError::Io)?;
    let codex_root = get_codex_root()?;
    if !codex_root.exists() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Codex history root not found: {}", codex_root.display()),
        )));
    }

    let mut sessions = load_project_sessions(&codex_root, &project_dir, false)?;
    sessions.sort_by_key(|session| Reverse(session.activity_timestamp()));

    fs::create_dir_all(output_dir).map_err(AppError::Io)?;

    let mut written = 0usize;
    for (idx, session) in sessions.iter().enumerate() {
        let content = render_session_markdown(session);
        if content.trim().is_empty() {
            continue;
        }

        let title = session
            .turns
            .iter()
            .find(|turn| turn.role == CodexRole::User)
            .map(|turn| turn.text.as_str())
            .or(session.id.as_deref())
            .unwrap_or("session");
        let date = session
            .activity_timestamp()
            .format("%Y-%m-%d-%H%M%S")
            .to_string();
        let filename = format!(
            "{:03}-{}-{}.md",
            idx + 1,
            date,
            crate::sanitize_filename(title)
        );
        let output_path = output_dir.join(filename);
        fs::write(&output_path, content).map_err(AppError::Io)?;
        println!("{}", output_path.display());
        written += 1;
    }

    eprintln!(
        "Exported {} Markdown files from {}",
        written,
        codex_root.display()
    );
    Ok(())
}

fn get_codex_root() -> Result<PathBuf> {
    if let Ok(config_dir) = std::env::var("CODEX_HOME") {
        return Ok(PathBuf::from(config_dir));
    }

    let home_dir = home::home_dir().ok_or_else(|| {
        AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine home directory",
        ))
    })?;
    Ok(home_dir.join(".codex"))
}

fn load_project_sessions(
    codex_root: &Path,
    project_dir: &Path,
    include_archived: bool,
) -> Result<Vec<CodexSession>> {
    let mut files = Vec::new();
    collect_jsonl_files(&codex_root.join("sessions"), &mut files)?;
    if include_archived {
        collect_jsonl_files(&codex_root.join("archived_sessions"), &mut files)?;
    }
    files.sort();

    let mut sessions = Vec::new();
    for path in files {
        let metadata = fs::metadata(&path).map_err(AppError::Io)?;
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let Some(session) = parse_session_file(path, modified)? else {
            continue;
        };
        let Ok(cwd) = session.cwd.canonicalize() else {
            continue;
        };
        if cwd == project_dir {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(AppError::Io)? {
        let entry = entry.map_err(AppError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rollout-"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn parse_session_file(path: PathBuf, modified: SystemTime) -> Result<Option<CodexSession>> {
    let content = fs::read_to_string(&path).map_err(AppError::Io)?;
    parse_session_str(path, modified, &content)
}

fn parse_session_str(
    path: PathBuf,
    modified: SystemTime,
    content: &str,
) -> Result<Option<CodexSession>> {
    let mut id = None;
    let mut timestamp = None;
    let mut last_activity = None;
    let mut cwd = None;
    let mut cli_version = None;
    let mut turns = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let envelope: CodexEnvelope = serde_json::from_str(line).map_err(AppError::Json)?;
        let envelope_timestamp = envelope
            .timestamp
            .as_deref()
            .and_then(parse_codex_timestamp);
        match envelope.kind.as_str() {
            "session_meta" => {
                let meta: SessionMetaPayload =
                    serde_json::from_value(envelope.payload).map_err(AppError::Json)?;
                id = meta.id;
                timestamp = meta
                    .timestamp
                    .as_deref()
                    .and_then(parse_codex_timestamp)
                    .or(envelope_timestamp);
                cwd = meta.cwd;
                cli_version = meta.cli_version;
            }
            "response_item" => {
                if let Some(turn) = parse_response_item(&envelope.payload)
                    && !is_automatic_context_message(&turn.text)
                {
                    if let Some(timestamp) = envelope_timestamp {
                        last_activity = Some(timestamp);
                    }
                    turns.push(turn);
                }
            }
            _ => {}
        }
    }

    let Some(cwd) = cwd else {
        return Ok(None);
    };

    Ok(Some(CodexSession {
        path,
        id,
        timestamp,
        last_activity,
        modified,
        cwd,
        cli_version,
        turns,
    }))
}

fn parse_response_item(payload: &Value) -> Option<CodexTurn> {
    if payload.get("type")?.as_str()? != "message" {
        return None;
    }

    let role = match payload.get("role")?.as_str()? {
        "user" => CodexRole::User,
        "assistant" => CodexRole::Assistant,
        _ => return None,
    };

    let text = extract_message_text(payload.get("content")?)
        .trim()
        .to_string();
    if text.is_empty() {
        return None;
    }

    Some(CodexTurn { role, text })
}

fn extract_message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_owned(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn is_automatic_context_message(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("# AGENTS.md instructions for ")
        || trimmed.starts_with("<environment_context>")
        || trimmed.starts_with("<user_instructions>")
}

fn render_session_markdown(session: &CodexSession) -> String {
    let mut output = String::new();
    let _ = (&session.path, &session.cli_version);
    for turn in &session.turns {
        output.push_str(&format!(
            "## {}\n\n{}\n\n",
            turn.role.markdown_label(),
            turn.text
        ));
    }
    output
}

fn parse_codex_timestamp(timestamp: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Local))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session(cwd: &Path) -> String {
        format!(
            r##"{{"timestamp":"2026-04-30T21:00:00.000Z","type":"session_meta","payload":{{"id":"session-1","timestamp":"2026-04-30T21:00:00.000Z","cwd":{},"cli_version":"0.125.0"}}}}
{{"timestamp":"2026-04-30T21:00:01.000Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"# AGENTS.md instructions for {}\n\n<INSTRUCTIONS>ignored</INSTRUCTIONS>"}}]}}}}
{{"timestamp":"2026-04-30T21:00:02.000Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"<environment_context>\n  <cwd>{}</cwd>\n</environment_context>"}}]}}}}
{{"timestamp":"2026-04-30T21:00:03.000Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"Build the exporter"}}]}}}}
{{"timestamp":"2026-04-30T21:00:04.000Z","type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{{}}","call_id":"call_1"}}}}
{{"timestamp":"2026-04-30T21:00:05.000Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"call_1","output":"secret tool output"}}}}
{{"timestamp":"2026-04-30T21:00:06.000Z","type":"response_item","payload":{{"type":"reasoning","summary":[],"encrypted_content":"secret"}}}}
{{"timestamp":"2026-04-30T21:00:07.000Z","type":"turn_context","payload":{{"cwd":{}}}}}
{{"timestamp":"2026-04-30T21:00:08.000Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"Done."}}],"phase":"final"}}}}"##,
            serde_json::to_string(&cwd.display().to_string()).unwrap(),
            cwd.display(),
            cwd.display(),
            serde_json::to_string(&cwd.display().to_string()).unwrap()
        )
    }

    #[test]
    fn renders_only_conversation_messages() {
        let cwd = Path::new("/tmp/project");
        let session = parse_session_str(
            PathBuf::from("rollout.jsonl"),
            SystemTime::UNIX_EPOCH,
            &sample_session(cwd),
        )
        .unwrap()
        .unwrap();

        let markdown = render_session_markdown(&session);
        assert!(markdown.contains("## You\n\nBuild the exporter"));
        assert!(markdown.contains("## Codex\n\nDone."));
        assert!(!markdown.contains("AGENTS.md"));
        assert!(!markdown.contains("environment_context"));
        assert!(!markdown.contains("secret tool output"));
        assert!(!markdown.contains("encrypted_content"));
    }

    #[test]
    fn filters_sessions_by_exact_canonical_cwd() {
        let temp = tempfile::tempdir().unwrap();
        let project_dir = temp.path().join("project");
        let other_dir = temp.path().join("other");
        let sessions_dir = temp.path().join(".codex/sessions/2026/04/30");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&other_dir).unwrap();
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::write(
            sessions_dir.join("rollout-2026-04-30T00-00-00-match.jsonl"),
            sample_session(&project_dir),
        )
        .unwrap();
        fs::write(
            sessions_dir.join("rollout-2026-04-30T00-00-01-other.jsonl"),
            sample_session(&other_dir),
        )
        .unwrap();

        let sessions = load_project_sessions(
            &temp.path().join(".codex"),
            &project_dir.canonicalize().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].cwd, project_dir);
    }

    #[test]
    fn skips_empty_transcripts_when_exporting_content() {
        let session = CodexSession {
            path: PathBuf::from("rollout.jsonl"),
            id: Some("session-1".to_string()),
            timestamp: None,
            last_activity: None,
            modified: SystemTime::UNIX_EPOCH,
            cwd: PathBuf::from("/tmp/project"),
            cli_version: None,
            turns: Vec::new(),
        };

        assert!(render_session_markdown(&session).trim().is_empty());
    }
}
