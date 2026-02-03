use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum LogEntry {
    Summary {
        summary: String,
    },
    User {
        message: UserMessage,
        #[allow(dead_code)]
        timestamp: String,
        /// The working directory when this message was sent
        cwd: Option<String>,
    },
    Assistant {
        message: AssistantMessage,
        #[allow(dead_code)]
        timestamp: String,
    },
    #[serde(rename = "file-history-snapshot")]
    #[allow(dead_code)]
    FileHistorySnapshot {
        #[serde(rename = "messageId")]
        message_id: String,
        snapshot: serde_json::Value,
        #[serde(rename = "isSnapshotUpdate")]
        is_snapshot_update: bool,
    },
    Progress {
        data: serde_json::Value,
        #[allow(dead_code)]
        #[serde(flatten)]
        extra: serde_json::Value,
    },
    #[allow(dead_code)]
    System {
        subtype: String,
        level: Option<String>,
        #[serde(flatten)]
        extra: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
pub struct UserMessage {
    #[allow(dead_code)]
    pub role: String,
    pub content: UserContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    String(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Deserialize)]
pub struct AssistantMessage {
    #[allow(dead_code)]
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        #[allow(dead_code)]
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        #[allow(dead_code)]
        tool_use_id: String,
        #[serde(default)]
        content: Option<serde_json::Value>, // Optional in some user tool result entries
    },
    Thinking {
        thinking: String,
        #[allow(dead_code)]
        signature: String,
    },
    #[allow(dead_code)]
    Image {
        source: serde_json::Value,
    },
}

/// Extract text from content blocks, used for both user and assistant messages
pub fn extract_text_from_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn extract_text_from_user(message: &UserMessage) -> String {
    match &message.content {
        UserContent::String(text) => text.clone(),
        UserContent::Blocks(blocks) => extract_text_from_blocks(blocks),
    }
}

pub fn extract_text_from_assistant(message: &AssistantMessage) -> String {
    extract_text_from_blocks(&message.content)
}

/// Agent progress data from subagent conversations
#[derive(Debug, Deserialize)]
pub struct AgentProgressData {
    #[allow(dead_code)]
    #[serde(rename = "type")]
    pub progress_type: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
    pub message: AgentMessage,
    #[allow(dead_code)]
    pub prompt: Option<String>,
}

/// Individual message within an agent conversation
#[derive(Debug, Deserialize)]
pub struct AgentMessage {
    #[serde(rename = "type")]
    pub message_type: String, // "user" or "assistant"
    pub message: AgentMessageContent,
}

/// Content of an agent message (mirrors UserMessage/AssistantMessage structure)
#[derive(Debug, Deserialize)]
pub struct AgentMessageContent {
    #[allow(dead_code)]
    pub role: String,
    pub content: AgentContent,
}

/// Agent message content is always an array of content blocks
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AgentContent {
    Blocks(Vec<ContentBlock>),
}

/// Attempt to parse agent progress data from a Progress entry
pub fn parse_agent_progress(data: &serde_json::Value) -> Option<AgentProgressData> {
    // Check if this is an agent_progress type
    if data.get("type").and_then(|t| t.as_str()) != Some("agent_progress") {
        return None;
    }
    serde_json::from_value(data.clone()).ok()
}
