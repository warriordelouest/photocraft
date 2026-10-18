//! The tools the model may call. Deliberately few and generic: the ~700 engine commands are
//! reached through `find_commands` and `run_commands`, so their schemas aren't all in context.

use crate::msg::Block;
use serde_json::{Value, json};

pub const INSPECT_DOCUMENT: &str = "inspect_document";
pub const RENDER_PREVIEW: &str = "render_preview";
pub const FIND_COMMANDS: &str = "find_commands";
pub const RUN_COMMANDS: &str = "run_commands";

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

/// Runs a tool on the document. Implemented by the app; called on the agent's thread.
pub trait ToolHost {
    fn call(&mut self, name: &str, input: &Value) -> ToolOutput;
}

pub struct ToolOutput {
    /// Text and image blocks.
    pub content: Vec<Block>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn json(v: &Value) -> Self {
        Self { content: vec![Block::text(v.to_string())], is_error: false }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self { content: vec![Block::text(msg)], is_error: true }
    }
}

/// The tool list. `vision` adds `render_preview`, useless to a model that can't see images.
pub fn definitions(vision: bool) -> Vec<ToolDef> {
    let mut v = vec![
        ToolDef {
            name: INSPECT_DOCUMENT,
            description: "Describe the active document as JSON: size, resolution, colour mode, and the layer tree \
                (ids, names, kinds, bounds, visibility, opacity, blend mode, text content and style, shapes, masks, \
                effects). Call it before editing, and whenever the user may have changed the document by hand. \
                Returns {\"document\": null} when no document is open.",
            schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        },
        ToolDef {
            name: FIND_COMMANDS,
            description: "Search the editor's command catalogue (every menu item, tool action and option). Each match \
                gives the command id, its label and menu path, a parameter description, and whether it can run now \
                (with the reason when not). Search before using a command whose parameters you are not sure of. \
                Words are matched against id, label and menu path (all words must match).",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words to look for, e.g. \"gradient overlay\", \"shape.create\", \"align\"."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "Most matches returned (default 15)."}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolDef {
            name: RUN_COMMANDS,
            description: "Run editor commands in order, as one undo step for the user. Each command is {\"id\", \"params\"} \
                with params as documented by find_commands. Returns each command's JSON result (new layer ids, \
                info) or its error. By default the batch stops at the first error; later commands are not run.",
            schema: json!({
                "type": "object",
                "properties": {
                    "commands": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 60,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string"},
                                "params": {"type": "object"}
                            },
                            "required": ["id"],
                            "additionalProperties": false
                        }
                    },
                    "stop_on_error": {"type": "boolean", "description": "Default true."}
                },
                "required": ["commands"],
                "additionalProperties": false
            }),
        },
    ];
    if vision {
        v.push(ToolDef {
            name: RENDER_PREVIEW,
            description: "Render the active document, flattened, and return it as an image. Use it to check your work \
                visually after building or changing a part of the design, and before telling the user you are done.",
            schema: json!({
                "type": "object",
                "properties": {
                    "max_side": {"type": "integer", "minimum": 256, "maximum": 2048, "description": "Longest side in pixels (default from the settings)."}
                },
                "additionalProperties": false
            }),
        });
    }
    v
}

/// A one-line description of a call for the chat ("Running 4 commands: shape.create, …").
pub fn summarize_call(name: &str, input: &Value) -> String {
    match name {
        INSPECT_DOCUMENT => "Lecture du document".into(),
        RENDER_PREVIEW => "Aperçu du rendu".into(),
        FIND_COMMANDS => format!("Recherche de commandes : {}", input.get("query").and_then(Value::as_str).unwrap_or("")),
        RUN_COMMANDS => {
            let ids: Vec<&str> = input
                .get("commands")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|c| c.get("id").and_then(Value::as_str)).collect())
                .unwrap_or_default();
            let mut shown: Vec<&str> = ids.iter().take(4).copied().collect();
            if ids.len() > 4 {
                shown.push("…");
            }
            format!("{} commande(s) : {}", ids.len(), shown.join(", "))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_tool_only_with_vision() {
        let names = |v: bool| definitions(v).iter().map(|d| d.name).collect::<Vec<_>>();
        assert!(names(true).contains(&RENDER_PREVIEW));
        assert!(!names(false).contains(&RENDER_PREVIEW));
        assert!(names(false).contains(&RUN_COMMANDS));
    }

    #[test]
    fn schemas_are_objects() {
        for d in definitions(true) {
            assert_eq!(d.schema["type"], "object", "{}", d.name);
        }
    }

    #[test]
    fn summaries_survive_bad_input() {
        assert_eq!(summarize_call(RUN_COMMANDS, &json!({"commands": "nope"})), "0 commande(s) : ");
        let many = json!({"commands": [{"id":"a"},{"id":"b"},{"id":"c"},{"id":"d"},{"id":"e"}]});
        assert_eq!(summarize_call(RUN_COMMANDS, &many), "5 commande(s) : a, b, c, d, …");
        assert_eq!(summarize_call("other", &Value::Null), "other");
    }
}
