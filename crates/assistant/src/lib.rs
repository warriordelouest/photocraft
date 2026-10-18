//! # photocraft-assistant
//!
//! The AI design assistant's core, independent of the UI and of the document model:
//!
//! * [`config`]: model gateways ([`config::Gateway`], each with an Anthropic-native and an
//!   OpenAI-compatible route) and the model list the chat's selectors offer.
//! * [`msg`]: a provider-neutral conversation (text, images, tool calls and results).
//! * [`anthropic`] / [`openai`]: request builders and streaming parsers for the two wire formats.
//! * [`agent`]: the tool loop. The model asks for a tool, the [`tools::ToolHost`] (the app) runs it
//!   on the document, the result goes back, until the model answers without a tool.
//! * [`secrets`]: API keys in the OS credential store.
//!
//! Standalone (no workspace dependencies), so the UI decides how tools touch the document.
//! Networking is native only; on the web every request fails with [`Error::Unsupported`].
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod agent;
pub mod anthropic;
pub mod config;
pub mod http;
pub mod msg;
pub mod openai;
pub mod prompt;
pub mod secrets;
pub mod sse;
pub mod tools;

pub use agent::{Event, Usage, run};
pub use config::{Config, Gateway, ModelEntry, Protocol, Provider};
pub use msg::{Block, Message, Role};
pub use tools::{ToolHost, ToolOutput};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The gateway answered with an error status; the body is kept (truncated) for the user.
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
    /// The stream carried something we can't read, or an `error` event.
    #[error("response: {0}")]
    Protocol(String),
    #[error("no API key for {0}: add it in the assistant settings")]
    MissingKey(String),
    #[error("configuration: {0}")]
    Config(String),
    #[error("credential store: {0}")]
    Secrets(String),
    #[error("stopped")]
    Cancelled,
    #[error("not available in this build: {0}")]
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Keeps a gateway error body short enough to show in the chat, cut at a char boundary.
pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => format!("{}…", s.get(..i).unwrap_or(s)),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_strings_and_cuts_at_char_boundaries() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("ééééé", 2), "éé…");
        assert_eq!(truncate("", 0), "");
    }
}
