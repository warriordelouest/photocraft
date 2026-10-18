//! Gateways and models. The defaults are the user's gateways (APIYI, LaoZhang) and the model IDs
//! both list (checked 2026-10-18 against docs.apiyi.com/model-registry.json and
//! docs.laozhang.ai/en/models). Everything is editable and saved as JSON next to the app's
//! preferences. API keys are not stored here: see [`crate::secrets`].
//!
//! The chat picks a gateway and a model separately: every gateway serves the same model IDs, and
//! each model says which wire format to use with it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// `POST {anthropic_url}/v1/messages` (Anthropic Messages API). Prompt caching is billed
    /// correctly only in this format on APIYI, and both gateways serve DeepSeek through it too.
    Anthropic,
    /// `POST {openai_url}/chat/completions` (OpenAI Chat Completions).
    OpenAi,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gateway {
    pub id: String,
    pub name: String,
    /// The root for the Anthropic format (`https://api.apiyi.com`, no `/v1`).
    pub anthropic_url: String,
    /// The base for the OpenAI format, up to and including `/v1`.
    pub openai_url: String,
    /// The credential store entry holding this gateway's API key.
    pub key_id: String,
}

/// A gateway route, resolved for one model.
#[derive(Clone, Debug, PartialEq)]
pub struct Provider {
    /// The gateway's name, for messages.
    pub name: String,
    pub base_url: String,
    pub protocol: Protocol,
    pub key_id: String,
}

impl Provider {
    /// The endpoint requests are posted to.
    pub fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        match self.protocol {
            Protocol::Anthropic => format!("{base}/v1/messages"),
            Protocol::OpenAi => format!("{base}/chat/completions"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Unique key in the list (the selector's value).
    pub id: String,
    pub label: String,
    /// The model ID the gateway expects.
    pub model: String,
    pub protocol: Protocol,
    /// Reads images: without it the assistant works without previews or reference images.
    pub vision: bool,
    pub max_tokens: u32,
    /// `output_config.effort` (Anthropic format, Claude 4.6 and later); `None` leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Dollars per million input / output tokens, for the chat's cost estimate (the public
    /// list price; gateways bill their own rates). `None`: no estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub gateways: Vec<Gateway>,
    pub models: Vec<ModelEntry>,
    /// [`Gateway::id`] of the selected gateway.
    pub gateway: String,
    /// [`ModelEntry::id`] of the selected model.
    pub selected: String,
    /// Tool round trips allowed for one request before the assistant stops and reports.
    pub max_steps: u32,
    /// Longest side of the previews sent to the model, in pixels.
    pub preview_side: u32,
    /// Layout revision of a saved file, for [`Config::from_json`]'s upgrades.
    #[serde(default)]
    pub version: u32,
}

/// The current [`Config::version`]. 2: list prices, 768 px previews, Opus at medium effort.
const VERSION: u32 = 2;

/// Public list price (dollars per million input / output tokens) of a known model ID.
pub fn list_price(model: &str) -> Option<(f64, f64)> {
    let m = model.trim_end_matches("-thinking");
    Some(match m {
        _ if m.starts_with("claude-opus-5-5") => (4.0, 20.0),
        _ if m.starts_with("claude-opus") => (5.0, 25.0),
        _ if m.starts_with("claude-sonnet-5") => (2.0, 10.0),
        _ if m.starts_with("claude-sonnet") => (3.0, 15.0),
        _ if m.starts_with("claude-haiku-5") => (0.1, 0.5),
        _ if m.starts_with("claude-haiku") => (1.0, 5.0),
        _ => return None,
    })
}

impl Default for Config {
    fn default() -> Self {
        let gateway = |id: &str, name: &str, root: &str| Gateway {
            id: id.into(),
            name: name.into(),
            anthropic_url: root.into(),
            openai_url: format!("{root}/v1"),
            key_id: id.into(),
        };
        let model = |id: &str, label: &str, model: &str, vision: bool, effort: Option<&str>, price: Option<(f64, f64)>| ModelEntry {
            id: id.into(),
            label: label.into(),
            model: model.into(),
            protocol: Protocol::Anthropic,
            vision,
            max_tokens: 32_000,
            effort: effort.map(Into::into),
            price,
        };
        Self {
            gateways: vec![gateway("apiyi", "APIYI", "https://api.apiyi.com"), gateway("laozhang", "LaoZhang", "https://api.laozhang.ai")],
            models: vec![
                // Opus at medium effort: the model's default, plenty for layout work and cheaper
                // in thinking tokens than high.
                model("opus-5-5", "Claude Opus 5.5", "claude-opus-5-5", true, Some("medium"), Some((4.0, 20.0))),
                model("sonnet-5-5", "Claude Sonnet 5.5", "claude-sonnet-5-5", true, Some("medium"), Some((2.0, 10.0))),
                model("haiku-5-5", "Claude Haiku 5.5", "claude-haiku-5-5", true, None, Some((0.1, 0.5))),
                model("sonnet-4-5", "Claude Sonnet 4.5", "claude-sonnet-4-5-20250929", true, None, Some((3.0, 15.0))),
                model("deepseek-v4-flash", "DeepSeek V4 Flash", "deepseek-v4-flash", false, None, None),
                model("deepseek-v4-flash-vision", "DeepSeek V4 Flash Vision (expérimental)", "deepseek-v4-flash-vision-exp", true, None, None),
            ],
            gateway: "apiyi".into(),
            selected: "opus-5-5".into(),
            max_steps: 80,
            // ~800 image tokens a preview (1024 px: ~1 400), enough to judge a layout.
            preview_side: 768,
            version: VERSION,
        }
    }
}

impl Config {
    pub fn model(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn gateway(&self, id: &str) -> Option<&Gateway> {
        self.gateways.iter().find(|g| g.id == id)
    }

    /// The selected model and the route to it, or why they can't be used. An unknown selection
    /// falls back to the first entry.
    pub fn selection(&self) -> crate::Result<(&ModelEntry, Provider)> {
        let model = self.model(&self.selected).or_else(|| self.models.first()).ok_or_else(|| crate::Error::Config("no model configured".into()))?;
        let gateway = self.gateway(&self.gateway).or_else(|| self.gateways.first()).ok_or_else(|| crate::Error::Config("no gateway configured".into()))?;
        let base_url = match model.protocol {
            Protocol::Anthropic => gateway.anthropic_url.clone(),
            Protocol::OpenAi => gateway.openai_url.clone(),
        };
        if base_url.trim().is_empty() {
            return Err(crate::Error::Config(format!("{} has no URL for this model's format", gateway.name)));
        }
        if model.model.trim().is_empty() {
            return Err(crate::Error::Config(format!("model `{}` has no identifier", model.label)));
        }
        Ok((model, Provider { name: gateway.name.clone(), base_url, protocol: model.protocol, key_id: gateway.key_id.clone() }))
    }

    /// Parses a saved configuration. A file from an older layout is an error (the caller then
    /// uses the defaults); limits are clamped.
    pub fn from_json(text: &str) -> crate::Result<Self> {
        let mut c: Self = serde_json::from_str(text).map_err(|e| crate::Error::Config(e.to_string()))?;
        if c.version < 2 {
            if c.preview_side == 1024 {
                c.preview_side = 768;
            }
            for m in &mut c.models {
                if m.price.is_none() {
                    m.price = list_price(&m.model);
                }
                if m.model == "claude-opus-5-5" && m.effort.as_deref() == Some("high") {
                    m.effort = Some("medium".into());
                }
            }
            c.version = VERSION;
        }
        c.max_steps = c.max_steps.clamp(1, 500);
        c.preview_side = c.preview_side.clamp(256, 2048);
        Ok(c)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Reads `path`, falling back to the defaults when it is missing or unreadable.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| Self::from_json(&t).ok()).unwrap_or_default()
    }

    /// Writes `path` through a temporary file, so a crash never leaves half a file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self, path: &std::path::Path) -> crate::Result<()> {
        let io = |e: std::io::Error| crate::Error::Config(e.to_string());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json()).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_select_opus_on_apiyi_in_the_anthropic_format() {
        let c = Config::default();
        let (m, p) = c.selection().unwrap();
        assert_eq!(m.model, "claude-opus-5-5");
        assert_eq!(p.protocol, Protocol::Anthropic);
        assert_eq!(p.endpoint(), "https://api.apiyi.com/v1/messages");
        assert_eq!(p.key_id, "apiyi");
    }

    #[test]
    fn the_gateway_and_the_model_format_pick_the_endpoint() {
        let mut c = Config { gateway: "laozhang".into(), ..Config::default() };
        assert_eq!(c.selection().unwrap().1.endpoint(), "https://api.laozhang.ai/v1/messages");
        c.models[0].protocol = Protocol::OpenAi;
        assert_eq!(c.selection().unwrap().1.endpoint(), "https://api.laozhang.ai/v1/chat/completions");
        c.gateways[1].openai_url = "https://x.test/v1/".into();
        assert_eq!(c.selection().unwrap().1.endpoint(), "https://x.test/v1/chat/completions");
    }

    #[test]
    fn json_round_trips_and_bad_or_old_json_is_an_error() {
        let c = Config::default();
        assert_eq!(Config::from_json(&c.to_json()).unwrap(), c);
        assert!(Config::from_json("{").is_err());
        assert!(Config::from_json("null").is_err());
        assert!(Config::from_json(r#"{"providers":[],"models":[],"selected":"x","max_steps":1,"preview_side":1}"#).is_err());
    }

    #[test]
    fn unknown_selections_fall_back_and_empty_or_blank_entries_are_errors() {
        let mut c = Config { selected: "gone".into(), gateway: "gone".into(), ..Config::default() };
        let (m, p) = c.selection().unwrap();
        assert_eq!((m.id.as_str(), p.name.as_str()), ("opus-5-5", "APIYI"));
        c.models[0].model = " ".into();
        assert!(c.selection().is_err());
        c.models.clear();
        assert!(c.selection().is_err());
        let mut c = Config::default();
        c.gateways[0].anthropic_url.clear();
        assert!(c.selection().is_err());
        c.gateways.clear();
        assert!(c.selection().is_err());
    }

    #[test]
    fn version_1_files_get_prices_smaller_previews_and_medium_opus() {
        let mut old = Config { preview_side: 1024, version: 1, ..Config::default() };
        for m in &mut old.models {
            m.price = None;
        }
        old.models[0].effort = Some("high".into());
        let c = Config::from_json(&old.to_json()).unwrap();
        assert_eq!((c.preview_side, c.version), (768, VERSION));
        assert_eq!(c.models[0].price, Some((4.0, 20.0)));
        assert_eq!(c.models[0].effort.as_deref(), Some("medium"));
        assert_eq!(c.model("deepseek-v4-flash").unwrap().price, None);
        assert_eq!(list_price("claude-sonnet-4-5-20250929-thinking"), Some((3.0, 15.0)));
    }

    #[test]
    fn out_of_range_limits_are_clamped() {
        let c = Config { max_steps: 0, preview_side: 1, ..Config::default() };
        let back = Config::from_json(&c.to_json()).unwrap();
        assert_eq!(back.max_steps, 1);
        assert_eq!(back.preview_side, 256);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("photocraft-assistant-{}", std::process::id()));
        let path = dir.join("assistant.json");
        let c = Config { selected: "sonnet-5-5".into(), ..Config::default() };
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path), c);
        assert_eq!(Config::load(&dir.join("missing.json")), Config::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
