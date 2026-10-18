//! The tool loop: send the conversation, run the tools the model asks for, send their results,
//! until the model answers without a tool (or the step limit, an error, or Stop).

use crate::config::{Config, ModelEntry, Protocol, Provider};
use crate::http::Http;
use crate::msg::{Block, Message};
use crate::tools::{ToolDef, ToolHost, ToolOutput};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

/// Token counts, summed over a request's round trips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Uncached input.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    pub fn add(&mut self, o: Usage) {
        self.input = self.input.saturating_add(o.input);
        self.output = self.output.saturating_add(o.output);
        self.cache_read = self.cache_read.saturating_add(o.cache_read);
        self.cache_write = self.cache_write.saturating_add(o.cache_write);
    }

    /// Estimated cost in dollars at `price_in` / `price_out` per million tokens, with cache
    /// reads at a tenth of the input price and cache writes at 1.25× (Anthropic's rates).
    pub fn cost(&self, price_in: f64, price_out: f64) -> f64 {
        let m = |n: u64| n as f64 / 1_000_000.0;
        m(self.input) * price_in + m(self.cache_read) * price_in * 0.1 + m(self.cache_write) * price_in * 1.25 + m(self.output) * price_out
    }

    pub(crate) fn add_anthropic(&mut self, u: &Value) {
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
        self.add(Usage { input: n("input_tokens"), output: n("output_tokens"), cache_read: n("cache_read_input_tokens"), cache_write: n("cache_creation_input_tokens") });
    }

    pub(crate) fn add_openai(&mut self, u: &Value) {
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
        let cached = u.get("prompt_tokens_details").and_then(|d| d.get("cached_tokens")).and_then(Value::as_u64).unwrap_or(0);
        self.add(Usage { input: n("prompt_tokens").saturating_sub(cached), output: n("completion_tokens"), cache_read: cached, cache_write: 0 });
    }
}

/// What the chat shows while the agent works.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A piece of the model's answer.
    Text(String),
    /// A tool is about to run.
    ToolStart { name: String, summary: String },
    /// It ran.
    ToolEnd { name: String, ok: bool, summary: String },
    /// Tokens of the round trip just finished.
    Usage(Usage),
    /// A round trip ended (the next text starts a new paragraph).
    StepDone,
}

/// One round trip's answer.
pub struct Turn {
    pub message: Message,
    /// Anthropic names: `end_turn`, `tool_use`, `max_tokens`, `refusal`, …
    pub stop_reason: String,
    pub usage: Usage,
}

/// Everything one round trip needs.
pub struct Call<'a> {
    pub http: &'a Http,
    pub provider: &'a Provider,
    pub key: &'a str,
    pub model: &'a ModelEntry,
    pub system: &'a str,
    pub tools: &'a [ToolDef],
    pub conversation: &'a [Message],
}

/// Sends one round trip. Swappable in tests.
pub trait Transport {
    fn send(&mut self, call: &Call<'_>, cancel: &AtomicBool, emit: &mut dyn FnMut(Event)) -> crate::Result<Turn>;
}

/// The real network transport.
pub struct Network;

impl Transport for Network {
    fn send(&mut self, call: &Call<'_>, cancel: &AtomicBool, emit: &mut dyn FnMut(Event)) -> crate::Result<Turn> {
        match call.provider.protocol {
            Protocol::Anthropic => crate::anthropic::call(call, cancel, emit),
            Protocol::OpenAi => crate::openai::call(call, cancel, emit),
        }
    }
}

/// Runs the user's latest request (already the last message of `conversation`) to the end with
/// the selected model, over the network, reading its key from the credential store.
/// `reference` (command parameters, installed fonts) ends the system prompt; keep it identical
/// between requests so it stays cached.
pub fn run(config: &Config, reference: &str, conversation: &mut Vec<Message>, host: &mut dyn ToolHost, cancel: &AtomicBool, emit: &mut dyn FnMut(Event)) -> crate::Result<()> {
    let (model, provider) = config.selection()?;
    let key = crate::secrets::get(&provider.key_id)?.ok_or_else(|| crate::Error::MissingKey(provider.name.clone()))?;
    let http = Http::new()?;
    let system = crate::prompt::system(model.vision, reference);
    run_with(&mut Network, &http, config, model, &provider, &key, &system, conversation, host, cancel, emit)
}

#[allow(clippy::too_many_arguments)]
pub fn run_with(
    transport: &mut dyn Transport,
    http: &Http,
    config: &Config,
    model: &ModelEntry,
    provider: &Provider,
    key: &str,
    system: &str,
    conversation: &mut Vec<Message>,
    host: &mut dyn ToolHost,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Event),
) -> crate::Result<()> {
    let tools = crate::tools::definitions(model.vision);
    for _ in 0..config.max_steps.max(1) {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::Error::Cancelled);
        }
        let call = Call { http, provider, key, model, system, tools: &tools, conversation };
        let turn = transport.send(&call, cancel, emit)?;
        emit(Event::Usage(turn.usage));
        emit(Event::StepDone);
        let calls: Vec<(String, String, Value)> = turn.message.tool_uses().map(|(id, name, input)| (id.to_string(), name.to_string(), input.clone())).collect();
        let stop = turn.stop_reason.clone();
        if !turn.message.content.is_empty() || turn.message.raw.as_ref().is_some_and(|r| r.as_array().is_some_and(|a| !a.is_empty())) {
            conversation.push(turn.message);
        }
        if stop == "refusal" {
            return Err(crate::Error::Protocol("the model declined this request".into()));
        }
        if calls.is_empty() {
            if stop == "max_tokens" {
                return Err(crate::Error::Protocol("the answer hit the output limit".into()));
            }
            return Ok(());
        }
        // Every call gets a result, even after Stop: an unanswered tool call would make the
        // conversation unusable for the next request.
        let mut results = Vec::with_capacity(calls.len());
        for (id, name, input) in calls {
            let out = if cancel.load(Ordering::Relaxed) {
                ToolOutput::error("Stopped by the user before this tool ran.")
            } else if input.get("_invalid_json").is_some() {
                ToolOutput::error("Your tool input was not valid JSON (possibly cut off). Send it again, smaller.")
            } else {
                emit(Event::ToolStart { name: name.clone(), summary: crate::tools::summarize_call(&name, &input) });
                let out = host.call(&name, &input);
                emit(Event::ToolEnd { name: name.clone(), ok: !out.is_error, summary: result_summary(&out) });
                out
            };
            results.push(Block::ToolResult { tool_use_id: id, content: out.content, is_error: out.is_error });
        }
        conversation.push(Message::user(results));
    }
    Err(crate::Error::Protocol(format!("stopped after {} steps; ask me to continue", config.max_steps)))
}

fn result_summary(out: &ToolOutput) -> String {
    let text: String = out
        .content
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            Block::Image { .. } => Some("[image]"),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    crate::truncate(&text, 160)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Plays back scripted turns.
    struct Script(Vec<Turn>);

    impl Transport for Script {
        fn send(&mut self, call: &Call<'_>, _cancel: &AtomicBool, emit: &mut dyn FnMut(Event)) -> crate::Result<Turn> {
            assert!(!call.conversation.is_empty());
            if self.0.is_empty() {
                return Err(crate::Error::Protocol("script ended".into()));
            }
            let t = self.0.remove(0);
            emit(Event::Text(t.message.text()));
            Ok(t)
        }
    }

    #[derive(Default)]
    struct Host(Vec<String>);

    impl ToolHost for Host {
        fn call(&mut self, name: &str, _input: &Value) -> ToolOutput {
            self.0.push(name.to_string());
            if name == "boom" { ToolOutput::error("failed") } else { ToolOutput::json(&json!({"ok": true})) }
        }
    }

    fn turn(content: Vec<Block>, stop: &str) -> Turn {
        Turn { message: Message::assistant(content), stop_reason: stop.into(), usage: Usage { input: 1, output: 1, ..Usage::default() } }
    }

    fn tool(id: &str, name: &str) -> Block {
        Block::ToolUse { id: id.into(), name: name.into(), input: json!({}) }
    }

    fn go(script: Vec<Turn>, cancel: bool) -> (crate::Result<()>, Vec<Message>, Vec<String>, Vec<Event>) {
        let config = Config::default();
        let (model, provider) = config.selection().unwrap();
        let mut conv = vec![Message::user(vec![Block::text("go")])];
        let mut host = Host::default();
        let mut events = Vec::new();
        let flag = AtomicBool::new(cancel);
        let http = Http::new().unwrap();
        let r = run_with(&mut Script(script), &http, &config, model, &provider, "k", "sys", &mut conv, &mut host, &flag, &mut |e| events.push(e));
        (r, conv, host.0, events)
    }

    #[test]
    fn tools_run_and_results_go_back_until_the_model_is_done() {
        let (r, conv, ran, events) = go(vec![turn(vec![Block::text("je fais"), tool("a", "inspect_document"), tool("b", "boom")], "tool_use"), turn(vec![Block::text("fini")], "end_turn")], false);
        r.unwrap();
        assert_eq!(ran, ["inspect_document", "boom"]);
        assert_eq!(conv.len(), 4);
        let Block::ToolResult { tool_use_id, is_error, .. } = &conv[2].content[1] else { panic!("a tool result") };
        assert_eq!((tool_use_id.as_str(), *is_error), ("b", true));
        assert!(events.iter().any(|e| matches!(e, Event::ToolEnd { ok: false, .. })));
        assert_eq!(conv[3].text(), "fini");
    }

    #[test]
    fn stop_answers_pending_calls_without_running_them() {
        let (r, ..) = go(vec![turn(vec![tool("a", "inspect_document")], "tool_use")], true);
        assert!(matches!(r, Err(crate::Error::Cancelled)));
    }

    #[test]
    fn invalid_tool_json_is_reported_to_the_model_not_run() {
        let bad = Block::ToolUse { id: "a".into(), name: "run_commands".into(), input: json!({"_invalid_json": "{"}) };
        let (r, conv, ran, _) = go(vec![turn(vec![bad], "tool_use"), turn(vec![Block::text("ok")], "end_turn")], false);
        r.unwrap();
        assert!(ran.is_empty());
        let Block::ToolResult { is_error, .. } = &conv[2].content[0] else { panic!("a tool result") };
        assert!(is_error);
    }

    #[test]
    fn refusals_truncations_and_endless_loops_end_with_errors() {
        assert!(go(vec![turn(vec![Block::text("non")], "refusal")], false).0.is_err());
        assert!(go(vec![turn(vec![Block::text("…")], "max_tokens")], false).0.is_err());
        let endless: Vec<Turn> = (0..100).map(|i| turn(vec![tool(&i.to_string(), "inspect_document")], "tool_use")).collect();
        let (r, _, ran, _) = go(endless, false);
        assert!(r.is_err());
        assert_eq!(ran.len(), Config::default().max_steps as usize);
    }

    #[test]
    fn cost_weights_cache_reads_and_writes() {
        let u = Usage { input: 1_000_000, output: 1_000_000, cache_read: 1_000_000, cache_write: 1_000_000 };
        assert!((u.cost(4.0, 20.0) - (4.0 + 20.0 + 0.4 + 5.0)).abs() < 1e-9);
    }

    #[test]
    fn usage_sums_saturate() {
        let mut u = Usage { input: u64::MAX, ..Usage::default() };
        u.add(Usage { input: 5, ..Usage::default() });
        assert_eq!(u.input, u64::MAX);
    }
}
