//! Anthropic Messages API (`POST /v1/messages`, streamed).
//!
//! Caching: the system prompt and the tool list carry `cache_control` breakpoints, and so does the
//! last block of the conversation, so each round trip re-reads the stable prefix from the cache.
//! Assistant turns are resent exactly as received (`Message::raw`), thinking blocks included.

use crate::agent::{Call, Event, Turn, Usage};
use crate::config::ModelEntry;
use crate::msg::{Block, Message, Role};
use crate::tools::ToolDef;
use serde_json::{Map, Value, json};

pub const VERSION: &str = "2023-06-01";

pub fn headers(key: &str) -> Vec<(&'static str, String)> {
    // Gateways differ on which header they read; both carry the same key.
    vec![("x-api-key", key.to_string()), ("authorization", format!("Bearer {key}")), ("anthropic-version", VERSION.to_string())]
}

fn ephemeral() -> Value {
    json!({"type": "ephemeral"})
}

fn block_json(b: &Block) -> Value {
    match b {
        Block::Text { text } => json!({"type": "text", "text": text}),
        Block::Image { media_type, data } => json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}}),
        Block::ToolUse { id, name, input } => json!({"type": "tool_use", "id": id, "name": name, "input": input}),
        Block::ToolResult { tool_use_id, content, is_error } => json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": content.iter().map(block_json).collect::<Vec<_>>(),
            "is_error": is_error,
        }),
    }
}

fn message_json(m: &Message, model: &str) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content = match (&m.raw, &m.raw_model) {
        (Some(raw), Some(from)) if m.role == Role::Assistant && from == model => raw.clone(),
        _ => Value::Array(m.content.iter().map(block_json).collect()),
    };
    json!({"role": role, "content": content})
}

/// The request body.
pub fn request(model: &ModelEntry, system: &str, tools: &[ToolDef], conversation: &[Message]) -> Value {
    let mut tools_json: Vec<Value> = tools.iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema})).collect();
    if let Some(Value::Object(last)) = tools_json.last_mut() {
        last.insert("cache_control".into(), ephemeral());
    }
    let mut messages: Vec<Value> = conversation.iter().map(|m| message_json(m, &model.model)).collect();
    if let Some(Value::Array(blocks)) = messages.last_mut().and_then(|m| m.get_mut("content"))
        && let Some(Value::Object(last)) = blocks.last_mut()
    {
        last.insert("cache_control".into(), ephemeral());
    }
    let mut body = json!({
        "model": model.model,
        "max_tokens": model.max_tokens,
        "stream": true,
        "system": [{"type": "text", "text": system, "cache_control": ephemeral()}],
        "tools": tools_json,
        "messages": messages,
    });
    if let (Some(effort), Value::Object(o)) = (&model.effort, &mut body) {
        o.insert("output_config".into(), json!({"effort": effort}));
    }
    body
}

/// Builds the turn from stream events.
#[derive(Default)]
pub struct Stream {
    /// Content blocks as received, by index.
    blocks: Vec<Value>,
    /// Partial JSON of tool inputs, by block index.
    partial: Vec<String>,
    stop_reason: String,
    usage: Usage,
}

impl Stream {
    fn slot(&mut self, index: usize) -> crate::Result<&mut Value> {
        if index > 4096 {
            return Err(crate::Error::Protocol(format!("content block index {index}")));
        }
        while self.blocks.len() <= index {
            self.blocks.push(Value::Null);
            self.partial.push(String::new());
        }
        self.blocks.get_mut(index).ok_or_else(|| crate::Error::Protocol("content block".into()))
    }

    /// Handles one event. Text deltas are emitted as they arrive.
    pub fn event(&mut self, data: &str, emit: &mut dyn FnMut(Event)) -> crate::Result<()> {
        let v: Value = serde_json::from_str(data).map_err(|e| crate::Error::Protocol(format!("{e}: {}", crate::truncate(data, 200))))?;
        let index = || v.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                if let Some(u) = v.get("message").and_then(|m| m.get("usage")) {
                    self.usage.add_anthropic(u);
                }
            }
            "content_block_start" => {
                let i = index();
                let block = v.get("content_block").cloned().unwrap_or(Value::Null);
                *self.slot(i)? = block;
            }
            "content_block_delta" => {
                let i = index();
                let delta = v.get("delta").cloned().unwrap_or(Value::Null);
                let s = |k: &str| delta.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text_delta" => {
                        let t = s("text");
                        append(self.slot(i)?, "text", &t);
                        emit(Event::Text(t));
                    }
                    "input_json_delta" => {
                        self.slot(i)?;
                        if let Some(p) = self.partial.get_mut(i) {
                            p.push_str(&s("partial_json"));
                        }
                    }
                    "thinking_delta" => append(self.slot(i)?, "thinking", &s("thinking")),
                    "signature_delta" => append(self.slot(i)?, "signature", &s("signature")),
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = index();
                let partial = self.partial.get(i).cloned().unwrap_or_default();
                let slot = self.slot(i)?;
                if slot.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let input = if partial.trim().is_empty() { json!({}) } else { serde_json::from_str(&partial).unwrap_or_else(|_| json!({ "_invalid_json": partial })) };
                    if let Value::Object(o) = slot {
                        o.insert("input".into(), input);
                    }
                }
            }
            "message_delta" => {
                if let Some(r) = v.get("delta").and_then(|d| d.get("stop_reason")).and_then(Value::as_str) {
                    self.stop_reason = r.to_string();
                }
                if let Some(u) = v.get("usage") {
                    self.usage.add_anthropic(u);
                }
            }
            "error" => {
                let msg = v.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or(data);
                return Err(crate::Error::Protocol(msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn finish(self, model: &str) -> Turn {
        let raw: Vec<Value> = self.blocks.into_iter().filter(|b| !b.is_null()).collect();
        let content = raw
            .iter()
            .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => Some(Block::text(b.get("text").and_then(Value::as_str).unwrap_or(""))),
                Some("tool_use") => Some(Block::ToolUse {
                    id: b.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                    name: b.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    input: b.get("input").cloned().unwrap_or_else(|| json!({})),
                }),
                _ => None,
            })
            .collect();
        let mut message = Message::assistant(content);
        message.raw = Some(Value::Array(raw));
        message.raw_model = Some(model.to_string());
        Turn { message, stop_reason: self.stop_reason, usage: self.usage }
    }
}

fn append(slot: &mut Value, key: &str, s: &str) {
    if let Value::Object(o) = slot {
        let cur = o.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        o.insert(key.into(), Value::String(cur + s));
    } else {
        let mut o = Map::new();
        o.insert(key.into(), Value::String(s.to_string()));
        *slot = Value::Object(o);
    }
}

/// One round trip.
pub fn call(c: &Call<'_>, cancel: &std::sync::atomic::AtomicBool, emit: &mut dyn FnMut(Event)) -> crate::Result<Turn> {
    let body = request(c.model, c.system, c.tools, c.conversation);
    let mut stream = Stream::default();
    c.http.post_sse(&c.provider.endpoint(), &headers(c.key), &body, cancel, &mut |_event, data| stream.event(data, emit))?;
    Ok(stream.finish(&c.model.model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn model() -> ModelEntry {
        Config::default().model("opus-5-5").unwrap().clone()
    }

    #[test]
    fn request_caches_system_tools_and_the_conversation_tail() {
        let tools = crate::tools::definitions(true);
        let conv = vec![Message::user(vec![Block::text("hello"), Block::png(&[0])])];
        let body = request(&model(), "sys", &tools, &conv);
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        let tl = body["tools"].as_array().unwrap();
        assert!(tl.last().unwrap().get("cache_control").is_some());
        assert!(tl.first().unwrap().get("cache_control").is_none());
        let blocks = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["output_config"]["effort"], "medium");
    }

    #[test]
    fn raw_assistant_content_is_resent_only_to_its_model() {
        let mut m = Message::assistant(vec![Block::text("visible")]);
        m.raw = Some(json!([{"type": "thinking", "thinking": "", "signature": "s"}, {"type": "text", "text": "visible"}]));
        m.raw_model = Some("claude-opus-5-5".into());
        assert_eq!(message_json(&m, "claude-opus-5-5")["content"][0]["type"], "thinking");
        assert_eq!(message_json(&m, "claude-sonnet-5-5")["content"][0]["type"], "text");
    }

    #[test]
    fn stream_assembles_text_thinking_and_tool_calls() {
        let mut s = Stream::default();
        let mut text = String::new();
        let mut emit = |e: Event| {
            if let Event::Text(t) = e {
                text.push_str(&t);
            }
        };
        for d in [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":90}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Bon"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"jour"}}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"t1","name":"run_commands","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"commands\":"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"[]}"}}"#,
            r#"{"type":"content_block_stop","index":2}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}"#,
            r#"{"type":"message_stop"}"#,
        ] {
            s.event(d, &mut emit).unwrap();
        }
        let turn = s.finish("claude-opus-5-5");
        assert_eq!(text, "Bonjour");
        assert_eq!(turn.stop_reason, "tool_use");
        assert_eq!(turn.usage.input, 10);
        assert_eq!(turn.usage.cache_read, 90);
        assert_eq!(turn.usage.output, 42);
        assert_eq!(turn.message.text(), "Bonjour");
        let calls: Vec<_> = turn.message.tool_uses().collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].2, &json!({"commands": []}));
        let raw = turn.message.raw.unwrap();
        assert_eq!(raw[0]["signature"], "sig");
        assert_eq!(raw[2]["input"], json!({"commands": []}));
    }

    #[test]
    fn bad_events_are_errors_not_panics() {
        let mut s = Stream::default();
        let mut emit = |_e: Event| {};
        assert!(s.event("not json", &mut emit).is_err());
        assert!(s.event(r#"{"type":"error","error":{"message":"overloaded"}}"#, &mut emit).is_err());
        assert!(s.event(r#"{"type":"content_block_start","index":99999999}"#, &mut emit).is_err());
        // A delta for a block that never started, and a truncated tool input.
        s.event(r#"{"type":"content_block_delta","index":3,"delta":{"type":"text_delta","text":"x"}}"#, &mut emit).unwrap();
        s.event(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"a","name":"n"}}"#, &mut emit).unwrap();
        s.event(r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"x\":"}}"#, &mut emit).unwrap();
        s.event(r#"{"type":"content_block_stop","index":0}"#, &mut emit).unwrap();
        let turn = s.finish("m");
        let calls: Vec<_> = turn.message.tool_uses().collect();
        assert!(calls[0].2.get("_invalid_json").is_some());
    }
}
