//! Window › AI Assistant: a chat panel that builds and edits the document with an AI model
//! (`photocraft-assistant`). The agent loop runs on its own thread; every tool call comes back
//! here and runs on the UI thread, on the live session, so the user sees the document change and
//! can undo the whole request as one history step.

use crate::PhotocraftApp;
use crate::theme::Tokens;
use photocraft_assistant::{Block, Config, Event, Message, ToolHost, ToolOutput, Usage, tools};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Commands the assistant may not run: they write or close files, or quit.
const DENIED_PREFIXES: &[&str] = &["file.save", "file.export", "file.close", "file.revert", "file.print", "file.scripts", "app.", "prefs."];

/// Commands whose parameters go into the system prompt (cached), so the model rarely needs a
/// `find_commands` round trip for flyer work.
const REFERENCE_COMMANDS: &[&str] = &[
    "file.new",
    "layer.new.layer",
    "layer.new.group",
    "layer.groupLayers",
    "layer.select",
    "layer.setProps",
    "layer.moveTo",
    "layer.duplicate",
    "layer.createClippingMask",
    "layer.linkLayers",
    "shape.create",
    "shape.edit",
    "type.create",
    "type.setStyle",
    "type.edit",
    "layer.newFillLayer.solidColor",
    "layer.newFillLayer.gradient",
    "layer.layerStyle.gradientOverlay",
    "layer.layerStyle.colorOverlay",
    "layer.layerStyle.dropShadow",
    "layer.layerStyle.outerGlow",
    "layer.layerStyle.stroke",
    "layer.newAdjustmentLayer.hueSaturation",
    "layer.newAdjustmentLayer.colorBalance",
    "layer.layerMask.revealAll",
    "paint.gradient",
    "file.placeEmbedded",
    "layer.align.horizontalCenters",
    "layer.align.verticalCenters",
    "layer.distribute.vertically",
];

/// Longest text a tool result may send back, in chars: past it the model re-reads megabytes
/// on every round trip.
const MAX_RESULT_CHARS: usize = 12_000;
/// Longest single command result inside a `run_commands` batch.
const MAX_COMMAND_RESULT_CHARS: usize = 1_500;
/// Most font families listed in the reference.
const MAX_FONT_CHARS: usize = 7_000;

/// What the chat shows.
enum Entry {
    User(String),
    Assistant(String),
    Tool { summary: String, ok: Option<bool>, detail: String },
    Error(String),
}

/// Messages from the agent thread.
enum FromAgent {
    Event(Event),
    Tool { name: String, input: Value, reply: Sender<ToolOutput> },
    Done { conversation: Vec<Message>, error: Option<String> },
}

struct Running {
    cancel: Arc<AtomicBool>,
    rx: Receiver<FromAgent>,
    /// History key shared by every command of this request: one undo step.
    coalesce: String,
}

/// Closed by default (tests, embedders); the desktop app opens it at launch.
#[derive(Default)]
pub struct Assistant {
    pub open: bool,
    config: Config,
    config_path: Option<std::path::PathBuf>,
    input: String,
    entries: Vec<Entry>,
    conversation: Vec<Message>,
    running: Option<Running>,
    usage: Usage,
    requests: u64,
    /// The model's next text starts a new bubble (a round trip ended).
    new_bubble: bool,
    /// The system prompt's command reference, built once (stable, so it stays cached).
    reference: Option<String>,
    settings_open: bool,
    /// Key being typed per credential entry, and the masked stored key (`None`: none stored).
    key_edit: std::collections::BTreeMap<String, String>,
    key_status: std::collections::BTreeMap<String, Option<String>>,
}

impl Assistant {
    /// Loads the gateway and model settings from the app's config directory.
    pub fn with_config_dir(dir: Option<std::path::PathBuf>) -> Self {
        let path = dir.map(|d| d.join("assistant.json"));
        #[cfg(not(target_arch = "wasm32"))]
        let config = path.as_deref().map(Config::load).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let config = Config::default();
        Self { config, config_path: path, ..Self::default() }
    }

    fn save_config(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            self.entries.push(Entry::Error(format!("Réglages non enregistrés : {e}")));
        }
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }
}

/// Runs tools by asking the UI thread, then waiting for its answer.
struct ChannelHost {
    tx: Sender<FromAgent>,
    ctx: egui::Context,
}

impl ToolHost for ChannelHost {
    fn call(&mut self, name: &str, input: &Value) -> ToolOutput {
        let (reply, rx) = channel();
        if self.tx.send(FromAgent::Tool { name: name.to_string(), input: input.clone(), reply }).is_err() {
            return ToolOutput::error("the editor is closing");
        }
        self.ctx.request_repaint();
        rx.recv().unwrap_or_else(|_| ToolOutput::error("the editor did not answer"))
    }
}

/// The exact parameters of [`REFERENCE_COMMANDS`] and the installed font families.
fn build_reference(app: &mut PhotocraftApp) -> String {
    let mut out = String::from("Parameters of the common commands (JSON-ish; `?` marks optional keys, `=x` a default):\n");
    if let Ok(Value::Array(all)) = app.run("command.list", json!({})) {
        for id in REFERENCE_COMMANDS {
            if let Some(params) = all.iter().find(|c| c.get("id").and_then(Value::as_str) == Some(id)).and_then(|c| c.get("params")).and_then(Value::as_str) {
                out.push_str(&format!("- {id} {}\n", clip(params, 1_200)));
            }
        }
    }
    out.push_str("Other alignments: layer.align.leftEdges|rightEdges|topEdges|bottomEdges, layer.distribute.horizontally (same parameters).\n");
    if let Ok(v) = app.run("type.fonts", json!({})) {
        let families: Vec<&str> = v.get("families").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        if !families.is_empty() {
            out.push_str(&format!("\nInstalled font families ({}): {}\n", families.len(), clip(&families.join(", "), MAX_FONT_CHARS)));
            out.push_str("Use only these family names; type.fonts {\"family\": name} lists a family's styles.\n");
        }
    }
    out
}

/// `s` cut to `max` chars, saying how much was left out.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}… [truncated: {} more chars]", s.get(..i).unwrap_or(s), s.chars().count().saturating_sub(max)),
        None => s.to_string(),
    }
}

/// A tool result capped to [`MAX_RESULT_CHARS`].
fn capped(out: ToolOutput) -> ToolOutput {
    let content = out
        .content
        .into_iter()
        .map(|b| match b {
            Block::Text { text } if text.chars().count() > MAX_RESULT_CHARS => Block::text(clip(&text, MAX_RESULT_CHARS)),
            other => other,
        })
        .collect();
    ToolOutput { content, is_error: out.is_error }
}

/// Sends the typed request.
fn send(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if app.assistant.reference.is_none() {
        let r = build_reference(app);
        app.assistant.reference = Some(r);
    }
    let a = &mut app.assistant;
    let text = a.input.trim().to_string();
    if text.is_empty() || a.running.is_some() {
        return;
    }
    a.input.clear();
    a.entries.push(Entry::User(text.clone()));
    a.conversation.push(Message::user(vec![Block::text(text)]));
    a.requests = a.requests.wrapping_add(1);
    start(a, ctx);
}

#[cfg(not(target_arch = "wasm32"))]
fn start(a: &mut Assistant, ctx: &egui::Context) {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let config = a.config.clone();
    let reference = a.reference.clone().unwrap_or_default();
    let mut conversation = a.conversation.clone();
    let (flag, ctx2) = (cancel.clone(), ctx.clone());
    let spawned = std::thread::Builder::new().name("assistant".into()).spawn(move || {
        let mut host = ChannelHost { tx: tx.clone(), ctx: ctx2.clone() };
        let events = tx.clone();
        let wake = ctx2.clone();
        let mut emit = move |e: Event| {
            let _ = events.send(FromAgent::Event(e));
            wake.request_repaint();
        };
        let result = photocraft_assistant::run(&config, &reference, &mut conversation, &mut host, &flag, &mut emit);
        let error = result.err().map(|e| e.to_string());
        let _ = tx.send(FromAgent::Done { conversation, error });
        ctx2.request_repaint();
    });
    match spawned {
        Ok(_) => a.running = Some(Running { cancel, rx, coalesce: format!("assistant-{}", a.requests) }),
        Err(e) => a.entries.push(Entry::Error(format!("Impossible de démarrer l'assistant : {e}"))),
    }
}

#[cfg(target_arch = "wasm32")]
fn start(a: &mut Assistant, _ctx: &egui::Context) {
    a.entries.push(Entry::Error("L'assistant n'est disponible que dans l'application de bureau.".into()));
}

/// Drains the agent's messages and runs the tools it asks for. Called every frame.
pub fn poll(app: &mut PhotocraftApp, ctx: &egui::Context) {
    loop {
        let Some(r) = app.assistant.running.as_ref() else { return };
        let Ok(msg) = r.rx.try_recv() else { break };
        let coalesce = r.coalesce.clone();
        match msg {
            FromAgent::Event(e) => on_event(&mut app.assistant, e),
            FromAgent::Tool { name, input, reply } => {
                let out = capped(run_tool(app, &name, &input, &coalesce));
                let _ = reply.send(out);
            }
            FromAgent::Done { conversation, error } => {
                let a = &mut app.assistant;
                a.conversation = conversation;
                if let Some(e) = error {
                    a.entries.push(Entry::Error(if e == "stopped" { "Arrêté.".into() } else { e }));
                }
                a.running = None;
            }
        }
    }
    if app.assistant.running.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn on_event(a: &mut Assistant, e: Event) {
    match e {
        Event::Text(t) => match a.entries.last_mut() {
            Some(Entry::Assistant(s)) if !a.new_bubble => s.push_str(&t),
            _ => {
                a.new_bubble = false;
                a.entries.push(Entry::Assistant(t));
            }
        },
        Event::ToolStart { summary, .. } => a.entries.push(Entry::Tool { summary, ok: None, detail: String::new() }),
        Event::ToolEnd { ok, summary, .. } => {
            if let Some(Entry::Tool { ok: slot, detail, .. }) = a.entries.iter_mut().rev().find(|e| matches!(e, Entry::Tool { ok: None, .. })) {
                *slot = Some(ok);
                *detail = summary;
            }
        }
        Event::Usage(u) => a.usage.add(u),
        // The next answer starts its own bubble.
        Event::StepDone => a.new_bubble = true,
    }
}

/// Runs one tool on the live session.
fn run_tool(app: &mut PhotocraftApp, name: &str, input: &Value, coalesce: &str) -> ToolOutput {
    match name {
        tools::INSPECT_DOCUMENT => {
            if app.session.active().is_none() {
                return ToolOutput::json(&json!({"document": null, "hint": "No document is open: create one with file.new."}));
            }
            match app.run("document.inspect", json!({})) {
                Ok(v) => ToolOutput::json(&v),
                Err(e) => ToolOutput::error(e),
            }
        }
        tools::RENDER_PREVIEW => {
            let side = input.get("max_side").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(app.assistant.config.preview_side).clamp(256, 2048);
            match preview_png(app, side) {
                Ok((png, w, h, dw, dh)) => ToolOutput {
                    content: vec![Block::png(&png), Block::text(format!("Preview {w}×{h} px of a {dw}×{dh} px document."))],
                    is_error: false,
                },
                Err(e) => ToolOutput::error(e),
            }
        }
        tools::FIND_COMMANDS => find_commands(app, input),
        tools::RUN_COMMANDS => run_commands(app, input, coalesce),
        other => ToolOutput::error(format!("unknown tool `{other}`")),
    }
}

/// The flattened active document as PNG: (bytes, width, height, document width, height).
fn preview_png(app: &PhotocraftApp, side: u32) -> Result<(Vec<u8>, u32, u32, u32, u32), String> {
    let st = app.session.active().ok_or("No document is open.")?;
    let doc = &st.doc;
    let img = photocraft_compose::thumbnail(doc, side.max(1));
    let (w, h) = (img.width, img.height);
    let image = photocraft_codecs::Image::from_u8(w, h, photocraft_codecs::ChannelLayout::Rgba, img.pixels).map_err(|e| e.to_string())?;
    let png = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).map_err(|e| e.to_string())?;
    Ok((png, w, h, doc.size.width, doc.size.height))
}

fn find_commands(app: &mut PhotocraftApp, input: &Value) -> ToolOutput {
    let query = input.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let limit = input.get("limit").and_then(Value::as_u64).map_or(15, |l| l.clamp(1, 50)) as usize;
    let all = match app.run("command.list", json!({})) {
        Ok(Value::Array(a)) => a,
        Ok(_) => Vec::new(),
        Err(e) => return ToolOutput::error(e),
    };
    let hay = |c: &Value| {
        let menu: Vec<&str> = c.get("menu").and_then(Value::as_array).map(|m| m.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        format!("{} {} {}", c.get("id").and_then(Value::as_str).unwrap_or(""), c.get("label").and_then(Value::as_str).unwrap_or(""), menu.join(" ")).to_lowercase()
    };
    let matches: Vec<Value> = all
        .iter()
        .filter(|c| {
            let h = hay(c);
            !words.is_empty() && words.iter().all(|w| h.contains(w))
        })
        .filter(|c| !denied(c.get("id").and_then(Value::as_str).unwrap_or("")))
        .take(limit)
        .map(|c| {
            json!({
                "id": c.get("id"),
                "label": c.get("label"),
                "menu": c.get("menu"),
                "params": c.get("params"),
                "enabled": c.get("enabled"),
                "disabledReason": c.get("disabledReason"),
            })
        })
        .collect();
    if matches.is_empty() {
        return ToolOutput::json(&json!({"matches": [], "hint": "No command matched every word; try fewer or other words (English labels, e.g. \"gradient\", \"align\", \"shape\")."}));
    }
    ToolOutput::json(&json!({ "matches": matches }))
}

fn denied(id: &str) -> bool {
    DENIED_PREFIXES.iter().any(|p| id.starts_with(p))
}

fn run_commands(app: &mut PhotocraftApp, input: &Value, coalesce: &str) -> ToolOutput {
    let Some(cmds) = input.get("commands").and_then(Value::as_array).filter(|c| !c.is_empty()) else {
        return ToolOutput::error("`commands` must be a non-empty array of {\"id\", \"params\"}");
    };
    let stop_on_error = input.get("stop_on_error").and_then(Value::as_bool).unwrap_or(true);
    let mut results = Vec::with_capacity(cmds.len());
    let mut failed = false;
    for c in cmds.iter().take(60) {
        let id = c.get("id").and_then(Value::as_str).unwrap_or("");
        if failed && stop_on_error {
            results.push(json!({"id": id, "skipped": true}));
            continue;
        }
        let r = if id.is_empty() {
            Err("missing command `id`".to_string())
        } else if denied(id) {
            Err(format!("`{id}` is not available to the assistant (it writes, closes or quits)"))
        } else {
            let params = c.get("params").cloned().unwrap_or_else(|| json!({}));
            match params {
                Value::Object(plain) => {
                    // One undo step for the whole request: every command shares the history key.
                    // Commands with strictly checked params (file.new…) refuse the key; they run
                    // without it as their own step.
                    let mut keyed = plain.clone();
                    keyed.insert("coalesce".into(), Value::String(coalesce.to_string()));
                    match app.run(id, Value::Object(keyed)) {
                        Err(e) if e.contains("unknown parameter `coalesce`") => app.run(id, Value::Object(plain)),
                        r => r,
                    }
                }
                _ => Err("`params` must be an object".to_string()),
            }
        };
        match r {
            Ok(v) => {
                // Big results (layer info with every style, font lists) are cut: the model
                // mostly needs the new layer's id.
                let text = v.to_string();
                let v = if text.chars().count() > MAX_COMMAND_RESULT_CHARS { Value::String(clip(&text, MAX_COMMAND_RESULT_CHARS)) } else { v };
                results.push(json!({"id": id, "ok": true, "result": v}));
            }
            Err(e) => {
                failed = true;
                results.push(json!({"id": id, "ok": false, "error": e}));
            }
        }
    }
    let out = json!({ "results": results });
    if failed { ToolOutput { content: vec![Block::text(out.to_string())], is_error: true } } else { ToolOutput::json(&out) }
}

/// The panel, on the dock's side of the canvas.
/// The chat, as a floating window over the workspace: movable, resizable, never squeezed by
/// the dock or the toolbars. Starts at the right, below the options bar.
pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.assistant.open {
        settings_window(app, ctx);
        return;
    }
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let w = 420.0_f32.min(screen.width() - 40.0).max(320.0);
    let h = (screen.height() - 140.0).clamp(420.0, 900.0);
    let frame = egui::Frame::NONE
        .fill(t.dock)
        .stroke(egui::Stroke::new(1.0, t.card_border))
        .corner_radius(t.radius_lg)
        .inner_margin(egui::Margin::same(12))
        .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: t.shadow });
    egui::Window::new("Assistant IA")
        .id(egui::Id::new("assistant-window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .default_size([w, h])
        .min_width(320.0)
        .min_height(380.0)
        .default_pos(egui::pos2(screen.right() - w - 64.0, screen.top() + 96.0))
        .frame(frame)
        .show(ctx, |ui| {
            glow(ui, &t);
            header(app, ui, &t);
            ui.add_space(8.0);
            let input_h = 118.0;
            let list_h = (ui.available_height() - input_h).max(80.0);
            egui::ScrollArea::vertical().id_salt("assistant-chat").max_height(list_h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                if app.assistant.entries.is_empty() {
                    suggestions(app, ui, &t);
                } else {
                    for e in &app.assistant.entries {
                        entry(ui, e, &t);
                    }
                    if app.assistant.busy() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("L'assistant travaille…").color(t.text_dim));
                        });
                    }
                }
            });
            ui.add_space(6.0);
            composer(app, ui, ctx, &t);
        });
    settings_window(app, ctx);
}

/// The soft accent glow at the top of the panel (the mockup's blue halo).
fn glow(ui: &egui::Ui, t: &Tokens) {
    let r = ui.max_rect();
    let c = egui::pos2(r.center().x, r.top() + 10.0);
    for (radius, alpha) in [(r.width() * 0.75, 10u8), (r.width() * 0.5, 16), (r.width() * 0.28, 24)] {
        let [cr, cg, cb, _] = t.accent.to_array();
        ui.painter().circle_filled(c, radius, egui::Color32::from_rgba_unmultiplied(cr, cg, cb, alpha));
    }
}

fn header(app: &mut PhotocraftApp, ui: &mut egui::Ui, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("✦ Assistant IA").strong().size(16.0).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("✕").on_hover_text("Masquer (Fenêtre › AI Assistant pour le rouvrir)").clicked() {
                app.assistant.open = false;
            }
            if ui.button("⚙").on_hover_text("Passerelles, clés API et modèles").clicked() {
                let a = &mut app.assistant;
                a.settings_open = !a.settings_open;
                if a.settings_open {
                    refresh_key_status(a);
                }
            }
            let a = &mut app.assistant;
            if ui.add_enabled(!a.busy() && !a.entries.is_empty(), egui::Button::new("＋")).on_hover_text("Nouvelle conversation").clicked() {
                a.entries.clear();
                a.conversation.clear();
                a.usage = Usage::default();
            }
        });
    });
    let a = &mut app.assistant;
    let model = a.config.model(&a.config.selected).map(|m| m.label.clone()).unwrap_or_else(|| "—".into());
    let gateway = a.config.gateway(&a.config.gateway).map(|g| g.name.clone()).unwrap_or_else(|| "—".into());
    let mut changed = false;
    ui.add_enabled_ui(!a.busy(), |ui| {
        ui.horizontal(|ui| {
            let gw_w = 110.0;
            egui::ComboBox::from_id_salt("assistant-gateway").selected_text(gateway).width(gw_w).show_ui(ui, |ui| {
                for g in &a.config.gateways {
                    changed |= ui.selectable_value(&mut a.config.gateway, g.id.clone(), &g.name).changed();
                }
            })
            .response
            .on_hover_text("Passerelle");
            egui::ComboBox::from_id_salt("assistant-model").selected_text(model).width(ui.available_width()).show_ui(ui, |ui| {
                for m in &a.config.models {
                    let label = if m.vision { m.label.clone() } else { format!("{} (sans vision)", m.label) };
                    changed |= ui.selectable_value(&mut a.config.selected, m.id.clone(), label).changed();
                }
            })
            .response
            .on_hover_text("Modèle");
        });
    });
    if changed {
        a.save_config();
    }
}

fn suggestions(app: &mut PhotocraftApp, ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("Que créons-nous aujourd'hui ?").size(18.0).strong().color(t.text));
        ui.label(egui::RichText::new("Décris ton flyer : je monte le document en calques éditables.").color(t.text_dim));
    });
    ui.add_space(16.0);
    let cards = [
        ("🎤", "Flyer d'événement", "Crée un flyer carré 2000×2000 pour un atelier : fond bleu nuit avec un halo lumineux, titre en deux lignes avec un mot sur un bandeau rouge, carte d'infos arrondie (lieu, date, heure) et une zone pour la photo de l'intervenant à droite."),
        ("🧾", "Grille d'offres", "Crée un flyer portrait 1600×2000 « Offres de participation » : fond lavande clair, 4 cartes arrondies en dégradé (rouge, bleu, violet, orange) avec titre, prix en pilule et 3 avantages cochés chacune."),
        ("🔁", "Mettre à jour un flyer", "Inspecte le document ouvert et dis-moi quels textes et quelles photos changer pour la prochaine édition."),
    ];
    for (icon, title, prompt) in cards {
        let resp = egui::Frame::NONE
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0, t.card_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(18.0));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(title).strong().color(t.text));
                        ui.label(egui::RichText::new(prompt).small().color(t.text_dim));
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
        if resp.clicked() {
            app.assistant.input = prompt.to_string();
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.add_space(8.0);
    }
}

fn entry(ui: &mut egui::Ui, e: &Entry, t: &Tokens) {
    match e {
        Entry::User(s) => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                egui::Frame::NONE.fill(t.accent_soft).corner_radius(t.radius_lg).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                    ui.set_max_width(ui.available_width() * 0.85);
                    ui.label(egui::RichText::new(s).color(t.text));
                });
            });
        }
        Entry::Assistant(s) => {
            ui.label(egui::RichText::new(s).color(t.text));
        }
        Entry::Tool { summary, ok, detail } => {
            let (mark, color) = match ok {
                None => ("⏳", t.text_dim),
                Some(true) => ("✓", t.text_faint),
                Some(false) => ("✗", t.warning),
            };
            let r = ui.label(egui::RichText::new(format!("{mark} {summary}")).small().color(color));
            if !detail.is_empty() {
                r.on_hover_text(detail);
            }
        }
        Entry::Error(s) => {
            ui.label(egui::RichText::new(format!("⚠ {s}")).color(t.danger));
        }
    }
    ui.add_space(6.0);
}

fn composer(app: &mut PhotocraftApp, ui: &mut egui::Ui, ctx: &egui::Context, t: &Tokens) {
    let busy = app.assistant.busy();
    let mut submit = false;
    egui::Frame::NONE.fill(t.field).stroke(egui::Stroke::new(1.0, t.field_border)).corner_radius(t.radius_lg).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        let edit = egui::TextEdit::multiline(&mut app.assistant.input)
            .hint_text("Décris ton flyer ou la modification à faire…")
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .frame(egui::Frame::NONE);
        let r = ui.add_enabled(!busy, edit);
        // Enter sends, Shift+Enter starts a new line.
        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
            let a = &mut app.assistant;
            while a.input.ends_with('\n') {
                a.input.pop();
            }
            submit = true;
        }
        ui.horizontal(|ui| {
            let u = app.assistant.usage;
            let total = u.input.saturating_add(u.cache_read).saturating_add(u.cache_write);
            if total > 0 {
                let price = app.assistant.config.model(&app.assistant.config.selected).and_then(|m| m.price.or_else(|| photocraft_assistant::config::list_price(&m.model)));
                let cost = price.map(|(i, o)| u.cost(i, o));
                let head = match cost {
                    Some(c) => format!("≈ {c:.2} $ · {} k tokens", (total + u.output) / 1000),
                    None => format!("{} k tokens", (total + u.output) / 1000),
                };
                let detail = format!(
                    "Conversation en cours (tarif public estimé ; la passerelle applique le sien) :\n\
                     • {} tokens d'entrée plein tarif\n• {} relus depuis le cache (≈ 10 % du tarif)\n• {} mis en cache (≈ 125 %)\n• {} tokens de sortie\n\n\
                     Chaque aller-retour renvoie toute la conversation : cliquez ＋ pour repartir à zéro entre deux flyers.",
                    u.input, u.cache_read, u.cache_write, u.output
                );
                ui.label(egui::RichText::new(head).small().color(t.text_faint)).on_hover_text(detail);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if busy {
                    if ui.button("■ Stop").clicked()
                        && let Some(r) = &app.assistant.running
                    {
                        r.cancel.store(true, Ordering::Relaxed);
                    }
                } else {
                    let send_btn = egui::Button::new(egui::RichText::new("➤").color(t.primary_text)).fill(t.primary_bg).corner_radius(14.0).min_size(egui::vec2(28.0, 28.0));
                    if ui.add_enabled(!app.assistant.input.trim().is_empty(), send_btn).on_hover_text("Envoyer (Entrée)").clicked() {
                        submit = true;
                    }
                }
            });
        });
    });
    if submit && !busy {
        send(app, ctx);
    }
}

fn refresh_key_status(a: &mut Assistant) {
    a.key_status.clear();
    let ids: std::collections::BTreeSet<String> = a.config.gateways.iter().map(|g| g.key_id.clone()).collect();
    for id in ids {
        let status = photocraft_assistant::secrets::get(&id).ok().flatten().map(|k| photocraft_assistant::secrets::masked(&k));
        a.key_status.insert(id, status);
    }
}

fn settings_window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let a = &mut app.assistant;
    if !a.settings_open {
        return;
    }
    let mut open = true;
    let mut dirty = false;
    let mut notice: Option<String> = None;
    // Fields get explicit sizes: inside a grid, a text field otherwise shrinks to a few letters.
    let row_h = 24.0;
    let field = |ui: &mut egui::Ui, w: f32, text: &mut String| ui.add_sized([w, row_h], egui::TextEdit::singleline(text)).lost_focus();
    egui::Window::new("Réglages de l'assistant").open(&mut open).default_width(760.0).min_width(700.0).resizable(true).collapsible(false).show(ctx, |ui| {
        ui.label(egui::RichText::new("Clés API").strong());
        ui.label(egui::RichText::new("Stockées dans le coffre d'identifiants du système, jamais dans un fichier.").small());
        let ids: Vec<String> = a.key_status.keys().cloned().collect();
        for id in ids {
            ui.horizontal(|ui| {
                ui.add_sized([90.0, row_h], egui::Label::new(egui::RichText::new(&id).strong()));
                let status = a.key_status.get(&id).cloned().flatten();
                ui.add_sized([110.0, row_h], egui::Label::new(status.clone().unwrap_or_else(|| "aucune clé".into())));
                let buf = a.key_edit.entry(id.clone()).or_default();
                ui.add_sized([260.0, row_h], egui::TextEdit::singleline(buf).password(true).hint_text("coller la clé"));
                if ui.button("Enregistrer").clicked() {
                    let key = std::mem::take(buf);
                    match photocraft_assistant::secrets::set(&id, &key) {
                        Ok(()) => notice = Some(format!("Clé « {id} » enregistrée.")),
                        Err(e) => notice = Some(format!("Erreur : {e}")),
                    }
                }
                if status.is_some() && ui.button("Supprimer").clicked() {
                    notice = Some(match photocraft_assistant::secrets::remove(&id) {
                        Ok(()) => format!("Clé « {id} » supprimée."),
                        Err(e) => format!("Erreur : {e}"),
                    });
                }
            });
        }
        ui.separator();
        ui.label(egui::RichText::new("Passerelles").strong());
        egui::Grid::new("assistant-gateways").num_columns(3).striped(true).show(ui, |ui| {
            ui.label("Nom");
            ui.label("URL format Anthropic (sans /v1)");
            ui.label("URL format OpenAI (avec /v1)");
            ui.end_row();
            for g in &mut a.config.gateways {
                ui.label(egui::RichText::new(&g.name).strong());
                dirty |= field(ui, 260.0, &mut g.anthropic_url);
                dirty |= field(ui, 260.0, &mut g.openai_url);
                ui.end_row();
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("Modèles").strong());
        ui.label(egui::RichText::new("« Identifiant » est le nom exact du modèle chez la passerelle (le même chez APIYI et LaoZhang).").small());
        egui::Grid::new("assistant-models").num_columns(4).striped(true).show(ui, |ui| {
            ui.label("Nom");
            ui.label("Identifiant");
            ui.label("Format");
            ui.label("Vision");
            ui.end_row();
            for (i, m) in a.config.models.iter_mut().enumerate() {
                dirty |= field(ui, 240.0, &mut m.label);
                dirty |= field(ui, 240.0, &mut m.model);
                use photocraft_assistant::Protocol;
                let name = |p: Protocol| if p == Protocol::Anthropic { "Anthropic" } else { "OpenAI" };
                egui::ComboBox::from_id_salt(("assistant-model-format", i)).selected_text(name(m.protocol)).width(100.0).show_ui(ui, |ui| {
                    for p in [Protocol::Anthropic, Protocol::OpenAi] {
                        dirty |= ui.selectable_value(&mut m.protocol, p, name(p)).changed();
                    }
                });
                dirty |= ui.checkbox(&mut m.vision, "").changed();
                ui.end_row();
            }
        });
        if ui.button("Ajouter un modèle").clicked() {
            let n = a.config.models.len() + 1;
            a.config.models.push(photocraft_assistant::ModelEntry {
                id: format!("custom-{n}"),
                label: format!("Modèle {n}"),
                protocol: photocraft_assistant::Protocol::Anthropic,
                model: String::new(),
                vision: true,
                max_tokens: 32_000,
                effort: None,
                price: None,
            });
            dirty = true;
        }
        if let Some(n) = &notice {
            ui.label(n);
        }
    });
    if notice.is_some() {
        refresh_key_status(a);
    }
    if dirty {
        a.save_config();
    }
    a.settings_open = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_writing_and_quitting_commands_are_denied() {
        assert!(denied("file.saveAs"));
        assert!(denied("file.export.exportAs"));
        assert!(denied("file.close"));
        assert!(denied("app.quit"));
        assert!(!denied("file.new"));
        assert!(!denied("file.placeEmbedded"));
        assert!(!denied("shape.create"));
    }

    #[test]
    fn the_reference_lists_every_reference_command_and_stays_small() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        let r = build_reference(&mut app);
        let missing: Vec<&str> = REFERENCE_COMMANDS.iter().copied().filter(|id| !r.contains(&format!("- {id} "))).collect();
        assert!(missing.is_empty(), "missing from the reference: {missing:?}");
        // About 4 chars a token: the reference is re-read (cached) on every round trip.
        println!("reference: {} chars", r.chars().count());
        assert!(r.chars().count() < 30_000, "{} chars", r.chars().count());
    }

    #[test]
    fn oversized_results_are_clipped_with_a_note() {
        let out = capped(ToolOutput::json(&json!({"x": "a".repeat(MAX_RESULT_CHARS * 2)})));
        let Block::Text { text } = &out.content[0] else { panic!("text") };
        assert!(text.chars().count() < MAX_RESULT_CHARS + 60);
        assert!(text.contains("truncated"));
        assert_eq!(clip("abc", 5), "abc");
    }

    #[test]
    fn tool_events_update_the_last_pending_tool_line() {
        let mut a = Assistant::default();
        on_event(&mut a, Event::Text("Je ".into()));
        on_event(&mut a, Event::Text("commence".into()));
        on_event(&mut a, Event::ToolStart { name: "x".into(), summary: "s".into() });
        on_event(&mut a, Event::ToolEnd { name: "x".into(), ok: false, summary: "err".into() });
        assert!(matches!(&a.entries[0], Entry::Assistant(s) if s == "Je commence"));
        assert!(matches!(&a.entries[1], Entry::Tool { ok: Some(false), detail, .. } if detail == "err"));
    }
}
