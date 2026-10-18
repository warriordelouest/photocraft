//! A provider-neutral conversation. Each adapter ([`crate::anthropic`], [`crate::openai`]) maps it
//! to its wire format.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text { text: String },
    /// Base64 data, without a `data:` prefix.
    Image { media_type: String, data: String },
    ToolUse { id: String, name: String, input: Value },
    /// `content` holds text and image blocks only.
    ToolResult { tool_use_id: String, content: Vec<Block>, is_error: bool },
}

impl Block {
    pub fn text(s: impl Into<String>) -> Self {
        Block::Text { text: s.into() }
    }

    pub fn image(media_type: &str, bytes: &[u8]) -> Self {
        Block::Image { media_type: media_type.to_string(), data: base64::engine::general_purpose::STANDARD.encode(bytes) }
    }

    pub fn png(bytes: &[u8]) -> Self {
        Self::image("image/png", bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Block>,
    /// An assistant turn's content exactly as an Anthropic-format gateway returned it (thinking
    /// blocks and their signatures included), resent unchanged to the same model: the API
    /// requires thinking blocks to come back as they were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    /// The model that produced `raw`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_model: Option<String>,
}

impl Message {
    pub fn user(content: Vec<Block>) -> Self {
        Self { role: Role::User, content, raw: None, raw_model: None }
    }

    pub fn assistant(content: Vec<Block>) -> Self {
        Self { role: Role::Assistant, content, raw: None, raw_model: None }
    }

    /// The tool calls in this message, in order.
    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.content.iter().filter_map(|b| match b {
            Block::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        })
    }

    /// The message's text blocks, joined.
    pub fn text(&self) -> String {
        let parts: Vec<&str> = self
            .content
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn png_blocks_are_base64_without_prefix() {
        let Block::Image { media_type, data } = Block::png(&[1, 2, 3]) else { panic!("an image") };
        assert_eq!(media_type, "image/png");
        assert_eq!(data, "AQID");
    }

    #[test]
    fn tool_uses_and_text_are_read_in_order() {
        let m = Message::assistant(vec![
            Block::text("a"),
            Block::ToolUse { id: "1".into(), name: "x".into(), input: json!({}) },
            Block::text("b"),
            Block::ToolUse { id: "2".into(), name: "y".into(), input: json!({"k": 1}) },
        ]);
        let ids: Vec<&str> = m.tool_uses().map(|(id, ..)| id).collect();
        assert_eq!(ids, ["1", "2"]);
        assert_eq!(m.text(), "a\nb");
    }

    #[test]
    fn messages_round_trip_through_json() {
        let m = Message::user(vec![Block::ToolResult { tool_use_id: "t".into(), content: vec![Block::text("ok")], is_error: false }]);
        let back: Message = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }
}
