//! OpenAI Chat Completions (`POST /chat/completions`, streamed), the format most gateways and
//! non-Claude models speak.
//!
//! Tool results are text only in this format: images a tool returns (previews) follow the tool
//! messages as a user message. A model without vision gets a note instead of the image.

use crate::agent::{Call, Event, Turn, Usage};
use crate::config::ModelEntry;
use crate::msg::{Block, Message, Role};
use crate::tools::ToolDef;
use serde_json::{Value, json};

pub fn headers(key: &str) -> Vec<(&'static str, String)> {
    vec![("authorization", format!("Bearer {key}"))]
}

const NO_VISION: &str = "[image omitted: this model cannot read images]";

fn image_part(media_type: &str, data: &str) -> Value {
    json!({"type": "image_url", "image_url": {"url": format!("data:{media_type};base64,{data}")}})
}

/// Appends the wire messages for one conversation message.
fn push_message(out: &mut Vec<Value>, m: &Message, vision: bool) {
    match m.role {
        Role::Assistant => {
            let text = m.text();
            let calls: Vec<Value> = m
                .tool_uses()
                .map(|(id, name, input)| json!({"id": id, "type": "function", "function": {"name": name, "arguments": input.to_string()}}))
                .collect();
            let mut msg = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text) }});
            if !calls.is_empty()
                && let Value::Object(o) = &mut msg
            {
                o.insert("tool_calls".into(), Value::Array(calls));
            }
            out.push(msg);
        }
        Role::User => {
            // Tool results first: they must follow the assistant message that called the tools.
            let mut parts: Vec<Value> = Vec::new();
            for b in &m.content {
                if let Block::ToolResult { tool_use_id, content, is_error } = b {
                    let mut text: Vec<String> = Vec::new();
                    for c in content {
                        match c {
                            Block::Text { text: t } => text.push(t.clone()),
                            Block::Image { media_type, data } => {
                                if vision {
                                    parts.push(json!({"type": "text", "text": format!("Image returned by tool call {tool_use_id}:")}));
                                    parts.push(image_part(media_type, data));
                                    text.push("[image attached in the next user message]".into());
                                } else {
                                    text.push(NO_VISION.into());
                                }
                            }
                            _ => {}
                        }
                    }
                    let mut body = text.join("\n");
                    if *is_error {
                        body = format!("ERROR: {body}");
                    }
                    out.push(json!({"role": "tool", "tool_call_id": tool_use_id, "content": body}));
                }
            }
            for b in &m.content {
                match b {
                    Block::Text { text } => parts.push(json!({"type": "text", "text": text})),
                    Block::Image { media_type, data } => {
                        parts.push(if vision { image_part(media_type, data) } else { json!({"type": "text", "text": NO_VISION}) })
                    }
                    _ => {}
                }
            }
            if !parts.is_empty() {
                out.push(json!({"role": "user", "content": parts}));
            }
        }
    }
}

pub fn request(model: &ModelEntry, system: &str, tools: &[ToolDef], conversation: &[Message]) -> Value {
    let mut messages = vec![json!({"role": "system", "content": system})];
    for m in conversation {
        push_message(&mut messages, m, model.vision);
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}}))
        .collect();
    json!({
        "model": model.model,
        "max_tokens": model.max_tokens,
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": messages,
        "tools": tools,
    })
}

#[derive(Default)]
struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub struct Stream {
    text: String,
    calls: Vec<PendingCall>,
    finish_reason: String,
    usage: Usage,
}

impl Stream {
    pub fn event(&mut self, data: &str, emit: &mut dyn FnMut(Event)) -> crate::Result<()> {
        let data = data.trim();
        if data == "[DONE]" {
            return Ok(());
        }
        let v: Value = serde_json::from_str(data).map_err(|e| crate::Error::Protocol(format!("{e}: {}", crate::truncate(data, 200))))?;
        if let Some(e) = v.get("error") {
            let msg = e.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| e.to_string());
            return Err(crate::Error::Protocol(msg));
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage.add_openai(u);
        }
        let Some(choice) = v.get("choices").and_then(Value::as_array).and_then(|c| c.first()) else { return Ok(()) };
        if let Some(r) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = r.to_string();
        }
        let Some(delta) = choice.get("delta") else { return Ok(()) };
        if let Some(t) = delta.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            self.text.push_str(t);
            emit(Event::Text(t.to_string()));
        }
        for c in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
            let i = c.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
            if i > 256 {
                return Err(crate::Error::Protocol(format!("tool call index {i}")));
            }
            while self.calls.len() <= i {
                self.calls.push(PendingCall::default());
            }
            let Some(p) = self.calls.get_mut(i) else { continue };
            if let Some(id) = c.get("id").and_then(Value::as_str) {
                p.id = id.to_string();
            }
            if let Some(f) = c.get("function") {
                if let Some(n) = f.get("name").and_then(Value::as_str) {
                    p.name.push_str(n);
                }
                if let Some(a) = f.get("arguments").and_then(Value::as_str) {
                    p.arguments.push_str(a);
                }
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Turn {
        let mut content = Vec::new();
        if !self.text.is_empty() {
            content.push(Block::text(self.text));
        }
        for (n, c) in self.calls.into_iter().enumerate().filter(|(_, c)| !c.name.is_empty()) {
            let input = if c.arguments.trim().is_empty() { json!({}) } else { serde_json::from_str(&c.arguments).unwrap_or_else(|_| json!({ "_invalid_json": c.arguments })) };
            let id = if c.id.is_empty() { format!("call_{n}") } else { c.id };
            content.push(Block::ToolUse { id, name: c.name, input });
        }
        // Normalized to the Anthropic names the agent loop reads.
        let stop_reason = match self.finish_reason.as_str() {
            "tool_calls" | "function_call" => "tool_use",
            "length" => "max_tokens",
            "content_filter" => "refusal",
            "stop" | "" => "end_turn",
            other => other,
        }
        .to_string();
        Turn { message: Message::assistant(content), stop_reason, usage: self.usage }
    }
}

pub fn call(c: &Call<'_>, cancel: &std::sync::atomic::AtomicBool, emit: &mut dyn FnMut(Event)) -> crate::Result<Turn> {
    let body = request(c.model, c.system, c.tools, c.conversation);
    let mut stream = Stream::default();
    c.http.post_sse(&c.provider.endpoint(), &headers(c.key), &body, cancel, &mut |_event, data| stream.event(data, emit))?;
    Ok(stream.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn model(vision: bool) -> ModelEntry {
        ModelEntry { vision, ..Config::default().model("deepseek-v4-flash").unwrap().clone() }
    }

    fn conversation() -> Vec<Message> {
        vec![
            Message::user(vec![Block::text("fais un flyer"), Block::png(&[1])]),
            Message::assistant(vec![Block::text("ok"), Block::ToolUse { id: "c1".into(), name: "render_preview".into(), input: json!({}) }]),
            Message::user(vec![Block::ToolResult { tool_use_id: "c1".into(), content: vec![Block::png(&[2])], is_error: false }]),
        ]
    }

    #[test]
    fn tool_results_follow_the_call_and_images_come_after_them() {
        let body = request(&model(true), "sys", &crate::tools::definitions(true), &conversation());
        let m = body["messages"].as_array().unwrap();
        assert_eq!(m[0]["role"], "system");
        assert_eq!(m[1]["content"][1]["type"], "image_url");
        assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{}");
        assert_eq!(m[3]["role"], "tool");
        assert_eq!(m[3]["tool_call_id"], "c1");
        assert_eq!(m[4]["role"], "user");
        assert!(m[4]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(body["tools"][0]["type"], "function");
    }

    #[test]
    fn models_without_vision_get_notes_instead_of_images() {
        let body = request(&model(false), "sys", &crate::tools::definitions(false), &conversation());
        let text = body.to_string();
        assert!(!text.contains("image_url"));
        assert!(text.contains("cannot read images"));
    }

    #[test]
    fn stream_assembles_text_and_split_tool_calls() {
        let mut s = Stream::default();
        let mut emit = |_e: Event| {};
        for d in [
            r#"{"choices":[{"delta":{"content":"Je "}}]}"#,
            r#"{"choices":[{"delta":{"content":"commence"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"run_commands","arguments":"{\"comm"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ands\":[]}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":60}}}"#,
            "[DONE]",
        ] {
            s.event(d, &mut emit).unwrap();
        }
        let t = s.finish();
        assert_eq!(t.message.text(), "Je commence");
        assert_eq!(t.stop_reason, "tool_use");
        let calls: Vec<_> = t.message.tool_uses().collect();
        assert_eq!(calls[0].0, "x");
        assert_eq!(calls[0].2, &json!({"commands": []}));
        assert_eq!((t.usage.input, t.usage.output, t.usage.cache_read), (40, 7, 60));
    }

    #[test]
    fn errors_and_garbage_are_errors_not_panics() {
        let mut s = Stream::default();
        let mut emit = |_e: Event| {};
        assert!(s.event("{", &mut emit).is_err());
        assert!(s.event(r#"{"error":{"message":"quota"}}"#, &mut emit).is_err());
        assert!(s.event(r#"{"choices":[{"delta":{"tool_calls":[{"index":100000}]}}]}"#, &mut emit).is_err());
        s.event(r#"{"choices":"x"}"#, &mut emit).unwrap();
        assert_eq!(s.finish().stop_reason, "end_turn");
    }
}
