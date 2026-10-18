//! Posting a request and reading its event stream. Blocking: the agent runs on its own thread.

use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

/// Longest error body kept for the chat.
const ERROR_BODY_CHARS: usize = 600;

#[cfg(not(target_arch = "wasm32"))]
pub struct Http {
    client: reqwest::blocking::Client,
}

#[cfg(not(target_arch = "wasm32"))]
impl Http {
    pub fn new() -> crate::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            // A long answer streams for minutes: no overall deadline, only the connect one.
            .timeout(None::<std::time::Duration>)
            .user_agent("Photocraft-assistant")
            .build()
            .map_err(|e| crate::Error::Network(e.to_string()))?;
        Ok(Self { client })
    }

    /// Posts `body` as JSON and calls `on_event(event, data)` for each server-sent event, until
    /// the stream ends, `on_event` fails, or `cancel` is set (checked between events).
    pub fn post_sse(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(&str, &str) -> crate::Result<()>,
    ) -> crate::Result<()> {
        use std::io::BufRead as _;
        let mut req = self.client.post(url).json(body).header("accept", "text/event-stream");
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let resp = req.send().map_err(|e| crate::Error::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            return Err(crate::Error::Http { status: status.as_u16(), body: crate::truncate(text.trim(), ERROR_BODY_CHARS) });
        }
        let mut parser = crate::sse::Parser::default();
        let mut reader = std::io::BufReader::new(resp);
        let mut line = String::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::Error::Cancelled);
            }
            line.clear();
            let n = reader.read_line(&mut line).map_err(|e| crate::Error::Network(e.to_string()))?;
            if n == 0 {
                if let Some((event, data)) = parser.finish() {
                    on_event(&event, &data)?;
                }
                return Ok(());
            }
            if let Some((event, data)) = parser.line(line.trim_end_matches('\n')) {
                on_event(&event, &data)?;
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub struct Http;

#[cfg(target_arch = "wasm32")]
impl Http {
    pub fn new() -> crate::Result<Self> {
        Err(crate::Error::Unsupported("network requests"))
    }

    pub fn post_sse(
        &self,
        _url: &str,
        _headers: &[(&str, String)],
        _body: &Value,
        _cancel: &AtomicBool,
        _on_event: &mut dyn FnMut(&str, &str) -> crate::Result<()>,
    ) -> crate::Result<()> {
        let _ = Ordering::Relaxed;
        Err(crate::Error::Unsupported("network requests"))
    }
}
