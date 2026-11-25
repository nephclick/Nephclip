//! The AI Generation window (header ▸ sparkle): Create (a prompt becomes a whole sequence), Titles,
//! Graphics (any graphics template, imported `.mogrt` packages included), Video (a generated
//! clip) and Settings (provider, API keys, model).
//!
//! Requests run in the background ([`crate::ai`]); the window stays responsive and shows a spinner
//! while a model answers. Without an API key every tab still works with offline generators.

use egui::{Align2, Color32, Rect, RichText, Sense, pos2, vec2};
use filmcraft_project::gtemplate::ControlKind;
use serde_json::{Value, json};

use crate::FilmcraftApp;
use crate::ai::{self, AiJob, Director};
use crate::icons::{self, Icon};
use crate::theme::Tokens;

const WIDTH: f32 = 620.0;
const GAP: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiTab {
    Edit,
    Create,
    Titles,
    Graphics,
    Video,
    Performance,
    Settings,
}

#[derive(Clone)]
struct Draft {
    tab: AiTab,
    edit_prompt: String,
    director: Director,
    title_prompt: String,
    title_style: &'static str,
    title_lines: String,
    graphic_prompt: String,
    graphic_template: String,
    graphic_values: Vec<(String, String, String)>,
    video_prompt: String,
    video_seconds: f64,
    perf_rec: Option<ai::PerfRecommendation>,
    job: Option<(AiJob, Want)>,
    status: String,
    failed: bool,
}

/// What a running request is for.
#[derive(Clone, PartialEq)]
enum Want {
    Title,
    Graphic,
    Test,
    Edit(String),
    Video(String, f64),
    Performance,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            tab: AiTab::Edit,
            edit_prompt: String::new(),
            director: Director { new_sequence: true, ..Default::default() },
            title_prompt: String::new(),
            title_style: "title",
            title_lines: String::new(),
            graphic_prompt: String::new(),
            graphic_template: "builtin:title-centered".into(),
            graphic_values: Vec::new(),
            video_prompt: String::new(),
            video_seconds: 5.0,
            perf_rec: None,
            job: None,
            status: String::new(),
            failed: false,
        }
    }
}

impl Draft {
    fn say(&mut self, s: impl Into<String>, failed: bool) {
        self.status = s.into();
        self.failed = failed;
    }
}

// ------------------------------------------------------------------------------------------------
// Widgets in the app's dialog style: pill buttons, dim 12 pt helper text, 12 pt rhythm
// ------------------------------------------------------------------------------------------------

fn heading(ui: &mut egui::Ui, t: &Tokens, title: &str, sub: &str) {
    ui.label(RichText::new(title).size(14.0).strong().color(t.text));
    ui.add_space(2.0);
    ui.label(RichText::new(sub).size(12.0).color(t.text_dim));
    ui.add_space(GAP);
}

fn field_label(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(t.text_dim));
    ui.add_space(4.0);
}

fn primary(ui: &mut egui::Ui, t: &Tokens, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(13.0).color(Color32::WHITE)).fill(t.accent).min_size(vec2(120.0, 28.0)).corner_radius(14.0),
    )
}

fn secondary(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).size(13.0)).min_size(vec2(84.0, 28.0)).corner_radius(14.0))
}

/// A row of single-choice pills; returns the clicked value.
fn choices<T: Copy + PartialEq>(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, current: T, items: &[(T, &str)]) -> Option<T> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (i, (v, label)) in items.iter().enumerate() {
            let r = ui.selectable_label(*v == current, RichText::new(*label).size(12.5));
            app.auto.add(&format!("aiGen.{id}.{i}"), r.rect, label);
            if r.clicked() {
                picked = Some(*v);
            }
        }
    });
    picked
}

fn multiline(app: &mut FilmcraftApp, ui: &mut egui::Ui, auto: &str, text: &mut String, hint: &str) {
    let r = ui.add(egui::TextEdit::multiline(text).hint_text(hint).desired_rows(3).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
    app.auto.add(auto, r.rect, hint);
}

/// The tab strip: icon and label pills, evenly padded.
fn tabs(app: &mut FilmcraftApp, ui: &mut egui::Ui, t: &Tokens, d: &mut Draft) {
    let items = [
        (AiTab::Edit, tl!("Edit"), Icon::Sparkle, "edit"),
        (AiTab::Create, tl!("Create"), Icon::Film, "create"),
        (AiTab::Titles, tl!("Titles"), Icon::Type, "titles"),
        (AiTab::Graphics, tl!("Graphics"), Icon::Grid, "graphics"),
        (AiTab::Video, tl!("Video"), Icon::Film, "video"),
        (AiTab::Performance, tl!("Optimize"), Icon::Wrench, "performance"),
        (AiTab::Settings, tl!("Settings"), Icon::Gear, "settings"),
    ];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (tab, label, icon, key) in items {
            let font = Tokens::ui(13.0);
            let g = ui.painter().layout_no_wrap(label.to_string(), font, t.text);
            let size = vec2(12.0 + 14.0 + 6.0 + g.size().x + 12.0, 28.0);
            let (r, resp) = ui.allocate_exact_size(size, Sense::click());
            let active = d.tab == tab;
            let fill = if active {
                t.pill_active_bg
            } else if resp.hovered() {
                t.hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(r, 14.0, fill);
            let col = if active || resp.hovered() { t.text } else { t.text_dim };
            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 19.0, r.center().y), vec2(14.0, 14.0)), icon, col);
            ui.painter().galley_with_override_text_color(pos2(r.min.x + 32.0, r.center().y - g.size().y / 2.0), g, col);
            app.auto.add(&format!("aiGen.tab.{key}"), r, label);
            if resp.clicked() {
                d.tab = tab;
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------
// Placement helpers
// ------------------------------------------------------------------------------------------------

fn need_sequence(app: &FilmcraftApp) -> Result<(), String> {
    if app.session.active_sequence().is_some() { Ok(()) } else { Err(tl!("Open or create a sequence first (or use Create to build one).").into()) }
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(|l| l.trim().trim_matches('"').trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// Offline text: the prompt, shaped for the style.
fn offline_text(prompt: &str, style: &str) -> String {
    let p = prompt.trim();
    match style {
        "title" => p.split_whitespace().take(6).collect::<Vec<_>>().join(" ").to_uppercase(),
        "lower_third" => {
            let mut parts = p.splitn(2, [',', '-', '–']);
            let name = parts.next().unwrap_or("").trim().to_string();
            let role = parts.next().unwrap_or("").trim().to_string();
            if role.is_empty() { name } else { format!("{name}\n{role}") }
        }
        _ => {
            let mut c = p.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

fn place_title(app: &mut FilmcraftApp, style: &str, text: &str) -> Result<Value, String> {
    need_sequence(app)?;
    let l = lines(text);
    let first = l.first().cloned().unwrap_or_default();
    if first.is_empty() {
        return Err(tl!("Write or generate the text first.").into());
    }
    let second = l.get(1).cloned().unwrap_or_default();
    let r = match style {
        "title" => {
            app.session.execute("graphics.template.apply", json!({"template": "builtin:title-centered", "values": {"title": first, "subtitle": second}}))
        }
        "lower_third" => app.session.execute(
            "graphics.template.apply",
            json!({"template": "builtin:lower-third-slab", "values": {"name": first, "role": second, "show_role": !second.is_empty()}}),
        ),
        _ => {
            let (w, h) = app.session.active_sequence().map(|q| (f64::from(q.settings.width), f64::from(q.settings.height))).unwrap_or((1920.0, 1080.0));
            app.session.execute(
                "graphics.newText",
                json!({"text": l.join("\n"), "newClip": true, "seconds": 5.0, "size": (h * 0.055).max(18.0), "position": [w / 2.0, h * 0.86]}),
            )
        }
    };
    r.map_err(|e| e.to_string())
}

/// The text properties of a library template: `(control id, label, default text)`.
fn text_controls(app: &FilmcraftApp, id: &str) -> Vec<(String, String, String)> {
    let lib = filmcraft_engine::graphic_templates::library(&app.session);
    let Some(e) = lib.iter().find(|e| e.template.id == id) else { return Vec::new() };
    e.template
        .controls
        .iter()
        .filter(|c| c.kind == ControlKind::Text)
        .map(|c| {
            let default = e
                .template
                .layers
                .iter()
                .find(|l| filmcraft_project::gtemplate::layer_uid(l) == c.layer)
                .and_then(|l| l.params.get(&c.param))
                .and_then(|p| match p.value_at(filmcraft_time::Tick::ZERO) {
                    filmcraft_project::ParamValue::Text(s) => Some(s),
                    _ => None,
                })
                .unwrap_or_default();
            (c.id.clone(), c.name.clone(), default)
        })
        .collect()
}

fn place_imported_video(app: &mut FilmcraftApp, video_path: &str, prompt: &str) -> Result<String, String> {
    let r = app.session.execute("file.import", json!({"paths": [video_path]})).map_err(|e| e.to_string())?;
    let item_id = r.get("items").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_u64)
        .or_else(|| r.get("imported").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_u64));
    if let Some(id) = item_id {
        if app.session.active_sequence().is_none() {
            app.session.execute("file.newSequence", json!({"name": prompt})).map_err(|e| e.to_string())?;
        }
        let at = app.session.playhead();
        app.session.execute("timeline.place", json!({"item": id, "time": at.0})).map_err(|e| e.to_string())?;
        Ok(tlf!("Added generated video \"{prompt}\" at playhead.", prompt = prompt))
    } else {
        Ok(tl!("Generated video saved and imported into project.").into())
    }
}

fn animate_clip(app: &mut FilmcraftApp, prompt: &str) -> Result<String, String> {
    need_sequence(app)?;
    let seq = app.session.active_sequence().ok_or_else(|| tl!("Open or create a sequence first.").to_string())?.clone();
    let ph = app.session.playhead();

    let target = app.session.state.selection.first().copied().or_else(|| {
        for tr in seq.video_tracks.iter().rev() {
            if let Some(it) = tr.item_at(ph) {
                return Some(it.id);
            }
        }
        for tr in seq.video_tracks.iter().rev() {
            if let Some(it) = tr.items.first() {
                return Some(it.id);
            }
        }
        None
    });

    let Some(clip_id) = target else {
        return Err(tl!("No clip found to animate. Place or select a clip first.").into());
    };

    let (_, it) = seq.find_item(clip_id).ok_or_else(|| tl!("Clip not found in sequence.").to_string())?;
    let start = it.start;
    let anim_dur = filmcraft_time::Tick::from_seconds_f64(0.6).min(it.duration / 2);
    let t0 = start;
    let t1 = start + anim_dur;
    let (w, h) = (f64::from(seq.settings.width), f64::from(seq.settings.height));
    let center_x = w / 2.0;
    let center_y = h / 2.0;

    let p_lower = prompt.to_lowercase();
    if p_lower.contains("zoom") || p_lower.contains("scale") {
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "motion",
            "param": "scale",
            "value": 30.0,
            "time": t0.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "motion",
            "param": "scale",
            "value": 100.0,
            "time": t1.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "opacity",
            "param": "opacity",
            "value": 0.0,
            "time": t0.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "opacity",
            "param": "opacity",
            "value": 100.0,
            "time": t1.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        Ok(tl!("Animated zoom-in with smooth opacity ease-in.").into())
    } else if p_lower.contains("fade") {
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "opacity",
            "param": "opacity",
            "value": 0.0,
            "time": t0.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "opacity",
            "param": "opacity",
            "value": 100.0,
            "time": t1.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        Ok(tl!("Animated smooth opacity fade-in.").into())
    } else if p_lower.contains("bounce") || p_lower.contains("pop") {
        let t_mid = t0 + anim_dur * 7 / 10;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "motion",
            "param": "scale",
            "value": 10.0,
            "time": t0.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "motion",
            "param": "scale",
            "value": 115.0,
            "time": t_mid.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "motion",
            "param": "scale",
            "value": 100.0,
            "time": t1.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        Ok(tl!("Animated pop-and-bounce scale entrance.").into())
    } else {
        let offset_y = 120.0;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "motion",
            "param": "position",
            "value": [center_x, center_y + offset_y],
            "time": t0.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "motion",
            "param": "position",
            "value": [center_x, center_y],
            "time": t1.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "opacity",
            "param": "opacity",
            "value": 0.0,
            "time": t0.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        app.session.execute("effects.setParam", json!({
            "clip": clip_id.0,
            "effect": "opacity",
            "param": "opacity",
            "value": 100.0,
            "time": t1.0,
            "keyframe": true
        })).map_err(|e| e.to_string())?;
        Ok(tl!("Animated smooth ease-in slide with position & opacity.").into())
    }
}

fn apply_ai_transition(app: &mut FilmcraftApp, prompt: &str) -> Result<String, String> {
    need_sequence(app)?;
    let p_lower = prompt.to_lowercase();
    let eff = if p_lower.contains("black") {
        "dip_to_black"
    } else if p_lower.contains("white") {
        "dip_to_white"
    } else if p_lower.contains("wipe") {
        "wipe"
    } else if p_lower.contains("push") {
        "push"
    } else if p_lower.contains("slide") {
        "slide"
    } else {
        "cross_dissolve"
    };

    let clip_param = if app.session.state.selection.is_empty() {
        let ph = app.session.playhead();
        let seq = app.session.active_sequence().ok_or_else(|| tl!("Open or create a sequence first.").to_string())?;
        seq.video_tracks.first().and_then(|t| t.item_at(ph)).map(|it| it.id)
    } else {
        None
    };

    let params = if let Some(cid) = clip_param {
        json!({"clip": cid.0, "effect": eff, "frames": 24})
    } else {
        json!({"effect": eff, "frames": 24})
    };

    app.session.execute("sequence.applyVideoTransition", params).map_err(|e| e.to_string())?;
    Ok(tlf!("Applied \"{eff}\" transition (24 frames).", eff = eff))
}

fn apply_local_ai_edit(app: &mut FilmcraftApp, prompt: &str) -> Result<String, String> {
    let p_trim = prompt.trim();
    if p_trim.is_empty() {
        return Err(tl!("Please enter an editing instruction.").into());
    }
    let p_lower = p_trim.to_lowercase();

    if p_lower.contains("animat")
        || p_lower.contains("motion")
        || p_lower.contains("slide in")
        || p_lower.contains("zoom in")
        || p_lower.contains("fade in")
        || p_lower.contains("bounce")
        || p_lower.contains("pop")
    {
        return animate_clip(app, &p_lower);
    }

    if p_lower.contains("transition")
        || p_lower.contains("dissolve")
        || p_lower.contains("dip to")
        || p_lower.contains("fade to black")
        || p_lower.contains("wipe")
        || p_lower.contains("push")
    {
        return apply_ai_transition(app, &p_lower);
    }

    if p_lower.contains("cut") || p_lower.contains("split") || p_lower.contains("razor") || p_lower.contains("slice") {
        need_sequence(app)?;
        app.session.execute("timeline.razor", json!({})).map_err(|e| e.to_string())?;
        return Ok(tl!("Split clip at the playhead.").into());
    }

    if p_lower.contains("ripple") || p_lower.contains("delete") || p_lower.contains("remove") || p_lower.contains("close gap") {
        need_sequence(app)?;
        app.session.execute("edit.clear", json!({"ripple": true})).map_err(|e| e.to_string())?;
        return Ok(tl!("Ripple deleted selection and closed the gap.").into());
    }

    if p_lower.contains("generate video")
        || p_lower.contains("generate scene")
        || p_lower.contains("create scene")
        || p_lower.contains("footage")
    {
        return Err(tl!("AI video generation requires the online server API. Switch to the Video tab or configure your API key in Settings.").into());
    }

    if p_lower.contains("title") || p_lower.contains("text") || p_lower.contains("caption") || p_lower.contains("lower third") {
        let style = if p_lower.contains("lower third") { "lower_third" } else if p_lower.contains("caption") { "caption" } else { "title" };
        let clean = p_trim
            .trim_start_matches("add ")
            .trim_start_matches("create ")
            .trim_start_matches("generate ")
            .trim_start_matches("title:")
            .trim_start_matches("text:")
            .trim();
        let text = if clean.is_empty() { "NephClip AI" } else { clean };
        place_title(app, style, text)?;
        return Ok(tlf!("Added {style} text to the timeline.", style = style));
    }

    Err(tl!("Configure your online server API key in Settings to execute custom AI editing commands.").into())
}

fn apply_llm_edit_response(app: &mut FilmcraftApp, response: &str, prompt: &str) -> Result<String, String> {
    if let Some(start) = response.find('{')
        && let Some(end) = response.rfind('}')
        && let Some(json_str) = response.get(start..=end)
        && let Ok(val) = serde_json::from_str::<Value>(json_str)
        && let Some(action) = val.get("action").and_then(Value::as_str)
    {
        match action {
            "animate" => {
                let style = val.get("style").and_then(Value::as_str).unwrap_or("slide");
                animate_clip(app, style)
            }
            "transition" => {
                let effect = val.get("effect").and_then(Value::as_str).unwrap_or("cross_dissolve");
                apply_ai_transition(app, effect)
            }
            "split" | "cut" => {
                need_sequence(app)?;
                app.session.execute("timeline.razor", json!({})).map_err(|e| e.to_string())?;
                Ok(tl!("Split clip at the playhead.").into())
            }
            "ripple_delete" | "delete" => {
                need_sequence(app)?;
                app.session.execute("edit.clear", json!({"ripple": true})).map_err(|e| e.to_string())?;
                Ok(tl!("Ripple deleted selection and closed the gap.").into())
            }
            "generate_video" => {
                Err(tl!("Video generation requested. Use the Video tab to generate with the online server API.").into())
            }
            "add_title" => {
                let style = val.get("style").and_then(Value::as_str).unwrap_or("title");
                let text = val.get("text").and_then(Value::as_str).unwrap_or(prompt);
                place_title(app, style, text)?;
                Ok(tlf!("Added {style} text to the timeline.", style = style))
            }
            _ => apply_local_ai_edit(app, prompt),
        }
    } else {
        apply_local_ai_edit(app, prompt)
    }
}

// ------------------------------------------------------------------------------------------------
// The window
// ------------------------------------------------------------------------------------------------

/// Draws the AI Generation window. Returns whether it stays open.
pub fn show(app: &mut FilmcraftApp, ctx: &egui::Context) -> bool {
    let draft_id = egui::Id::unique("ai-generation-draft");
    let mut d: Draft = ctx.data(|x| x.get_temp(draft_id)).unwrap_or_default();
    let mut open = true;
    let t = app.tokens;

    // answers that arrived since the last frame
    if d.director.poll(app) {
        let (s, f) = (d.director.status.clone(), d.director.failed);
        d.say(s, f);
    } else if d.director.busy() {
        let s = d.director.status.clone();
        d.say(s, false);
    }
    if let Some((job, want)) = d.job.clone()
        && let Some(answer) = job.poll()
    {
        d.job = None;
        match (want, answer) {
            (Want::Title, Ok(text)) => {
                d.title_lines = lines(&text).join("\n");
                d.say(tl!("Text generated. Edit it if you like, then add it to the timeline."), false);
            }
            (Want::Graphic, Ok(text)) => {
                for (slot, line) in d.graphic_values.iter_mut().zip(lines(&text)) {
                    slot.2 = line;
                }
                d.say(tl!("Text generated for the template's fields."), false);
            }
            (Want::Edit(prompt), Ok(text)) => match apply_llm_edit_response(app, &text, &prompt) {
                Ok(msg) => d.say(msg, false),
                Err(e) => d.say(e, true),
            },
            (Want::Video(prompt, _), Ok(video_path)) => {
                match place_imported_video(app, &video_path, &prompt) {
                    Ok(msg) => d.say(msg, false),
                    Err(e) => d.say(e, true),
                }
            }
            (Want::Performance, Ok(text)) => {
                if let Some(rec) = ai::parse_perf_recommendation(&text) {
                    d.perf_rec = Some(rec);
                    d.say(tl!("Hardware & media analysis complete. Review and apply below."), false);
                } else {
                    d.perf_rec = Some(ai::PerfRecommendation {
                        summary: text,
                        enable_auto_proxy: true,
                        proxy_preset: "prores_proxy_quarter".into(),
                        playback_res: "half".into(),
                        draft_decode: true,
                    });
                    d.say(tl!("Analysis complete."), false);
                }
            }
            (Want::Test, Ok(_)) => d.say(tlf!("{provider} answered: the key works.", provider = ai::provider_label(&app.session.prefs.ai.provider)), false),
            (_, Err(e)) => d.say(e, true),
        }
    }
    let busy = d.director.busy() || d.job.is_some();
    if busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    egui::Window::new(tl!("AI Generation"))
        .id(egui::Id::unique("NephClip AI Generation"))
        .collapsible(false)
        .resizable(false)
        .pivot(Align2::CENTER_CENTER)
        .default_pos(crate::widgets::dialog_center(ctx))
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 6.0;
            tabs(app, ui, &t, &mut d);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(GAP);
            let prefs = app.session.prefs.ai.clone();
            let keyed = ai::configured(&prefs);
            match d.tab {
                AiTab::Edit => {
                    heading(
                        ui,
                        &t,
                        tl!("Edit timeline with AI"),
                        tl!("Instruct AI to animate text, add transitions, cut/delete clips, or generate content."),
                    );
                    field_label(ui, &t, tl!("Quick actions"));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        let quick_actions = [
                            ("✨ Animate Text (Slide In)", "Animate the text to slide in from bottom with smooth ease"),
                            ("🔍 Animate Text (Zoom In)", "Animate the text to zoom in"),
                            ("🌫 Animate Text (Fade In)", "Animate text with smooth fade in"),
                            ("🎬 Add Cross Dissolve", "Add cross dissolve transition"),
                            ("⬛ Add Dip to Black", "Add dip to black transition"),
                            ("✂️ Split at Playhead", "Split clip at playhead"),
                            ("🧲 Ripple Delete", "Ripple delete selection and close gap"),
                            ("✨ Auto-Generate Captions", "Create captions for selected audio"),
                        ];
                        for (label, prompt_text) in quick_actions {
                            let btn = ui.button(RichText::new(label).size(12.0));
                            if btn.clicked() {
                                d.edit_prompt = prompt_text.to_string();
                                if keyed {
                                    let system = "You are an intelligent video editing assistant for NephClip. \
                                    Return ONLY a single valid JSON object representing the action to perform: \
                                    {\"action\": \"animate\", \"style\": \"slide\"|\"zoom\"|\"fade\"|\"bounce\"} \
                                    {\"action\": \"transition\", \"effect\": \"cross_dissolve\"|\"dip_to_black\"|\"dip_to_white\"|\"wipe\"|\"slide\"|\"push\"} \
                                    {\"action\": \"split\"} \
                                    {\"action\": \"ripple_delete\"} \
                                    {\"action\": \"add_title\", \"style\": \"title\"|\"lower_third\"|\"caption\", \"text\": string}";
                                    if let Ok(req) = ai::Request::from_prefs(&prefs, system, &d.edit_prompt, true) {
                                        d.job = Some((ai::spawn(ctx, req), Want::Edit(d.edit_prompt.clone())));
                                        d.say(tlf!("Editing with {provider}…", provider = ai::provider_label(&prefs.provider)), false);
                                    } else {
                                        match apply_local_ai_edit(app, &d.edit_prompt) {
                                            Ok(msg) => d.say(msg, false),
                                            Err(e) => d.say(e, true),
                                        }
                                    }
                                } else {
                                    match apply_local_ai_edit(app, &d.edit_prompt) {
                                        Ok(msg) => d.say(msg, false),
                                        Err(e) => d.say(e, true),
                                    }
                                }
                            }
                        }
                    });
                    ui.add_space(GAP);
                    field_label(ui, &t, tl!("Instruction"));
                    multiline(
                        app,
                        ui,
                        "aiGen.editPrompt",
                        &mut d.edit_prompt,
                        tl!("e.g. Animate the text to slide in from bottom, or Add a cross dissolve transition"),
                    );
                    ui.add_space(GAP);
                    ui.horizontal(|ui| {
                        let can_apply = !busy && !d.edit_prompt.trim().is_empty();
                        let r = primary(ui, &t, tl!("Apply with AI"), can_apply);
                        app.auto.add("aiGen.applyEdit", r.rect, "Apply with AI");
                        if busy {
                            ui.spinner();
                        }
                        if r.clicked() {
                            if keyed {
                                let system = "You are an intelligent video editing assistant for NephClip. \
                                Return ONLY a single valid JSON object representing the action to perform, matching one of these shapes: \
                                {\"action\": \"animate\", \"style\": \"slide\"|\"zoom\"|\"fade\"|\"bounce\"} \
                                {\"action\": \"transition\", \"effect\": \"cross_dissolve\"|\"dip_to_black\"|\"dip_to_white\"|\"wipe\"|\"slide\"|\"push\"} \
                                {\"action\": \"split\"} \
                                {\"action\": \"ripple_delete\"} \
                                {\"action\": \"add_title\", \"style\": \"title\"|\"lower_third\"|\"caption\", \"text\": string}";
                                match ai::Request::from_prefs(&prefs, system, &d.edit_prompt, true) {
                                    Ok(req) => {
                                        d.job = Some((ai::spawn(ctx, req), Want::Edit(d.edit_prompt.clone())));
                                        d.say(tlf!("Editing with {provider}…", provider = ai::provider_label(&prefs.provider)), false);
                                    }
                                    Err(e) => {
                                        d.say(e, true);
                                    }
                                }
                            } else {
                                d.say(tl!("Configure your online server API key in Settings to edit with AI."), true);
                            }
                        }
                    });
                }
                AiTab::Create => {
                    heading(
                        ui,
                        &t,
                        tl!("Create a video from a prompt"),
                        tl!("Describe the video. AI plans the scenes, then builds them on the timeline with captions and a title card."),
                    );
                    field_label(ui, &t, tl!("What should the video be about?"));
                    multiline(app, ui, "aiGen.createPrompt", &mut d.director.prompt, tl!("A road trip from the desert to the city at night, ending under the northern lights"));
                    ui.add_space(4.0);
                    let r = ui.checkbox(&mut d.director.new_sequence, RichText::new(tl!("Build in a new sequence")).size(12.5));
                    app.auto.add("aiGen.createNewSequence", r.rect, "Build in a new sequence");
                    ui.add_space(GAP);
                    ui.horizontal(|ui| {
                        let r = primary(ui, &t, tl!("Build Timeline"), !busy);
                        app.auto.add("aiGen.build", r.rect, "Build Timeline");
                        if busy {
                            ui.spinner();
                        }
                        if r.clicked() && d.director.start(app, ctx) {
                            app.ui.mode = crate::state::Mode::Edit;
                        }
                    });
                    if !d.director.status.is_empty() {
                        let (s, f) = (d.director.status.clone(), d.director.failed);
                        d.say(s, f);
                    }
                }
                AiTab::Titles => {
                    heading(ui, &t, tl!("Titles and text"), tl!("Write a headline, lower third or caption with AI and add it at the playhead."));
                    field_label(ui, &t, tl!("Prompt"));
                    multiline(app, ui, "aiGen.titlePrompt", &mut d.title_prompt, tl!("Opening title for a travel documentary about Iceland"));
                    ui.add_space(4.0);
                    field_label(ui, &t, tl!("Style"));
                    if let Some(s) = choices(app, ui, "titleStyle", d.title_style, &[("title", tl!("Title")), ("lower_third", tl!("Lower Third")), ("caption", tl!("Caption"))]) {
                        d.title_style = s;
                    }
                    ui.add_space(GAP);
                    ui.horizontal(|ui| {
                        let r = secondary(ui, tl!("Generate"), !busy && !d.title_prompt.trim().is_empty());
                        app.auto.add("aiGen.generateTitle", r.rect, "Generate");
                        if busy {
                            ui.spinner();
                        }
                        if r.clicked() {
                            let style = match d.title_style {
                                "title" => "a short, punchy title (at most 6 words) on the first line and an optional subtitle (at most 8 words) on the second",
                                "lower_third" => "a lower third: a person's or place's name on the first line and their role or a short description on the second",
                                _ => "one caption line of at most 10 words",
                            };
                            let system = format!("You write on-screen text for a video editor. Reply with only the text, no quotes, labels or explanation. Write {style}.");
                            match ai::Request::from_prefs(&prefs, &system, &d.title_prompt, false) {
                                Ok(req) => {
                                    d.job = Some((ai::spawn(ctx, req), Want::Title));
                                    d.say(tlf!("Writing with {provider}…", provider = ai::provider_label(&prefs.provider)), false);
                                }
                                Err(_) => {
                                    d.title_lines = offline_text(&d.title_prompt, d.title_style);
                                    d.say(tl!("Written offline from your prompt. Add an API key in Settings for AI writing."), false);
                                }
                            }
                        }
                    });
                    ui.add_space(GAP);
                    field_label(ui, &t, tl!("Text (one line per field)"));
                    let r = ui.add(egui::TextEdit::multiline(&mut d.title_lines).desired_rows(2).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                    app.auto.add("aiGen.titleText", r.rect, "Text");
                    ui.add_space(GAP);
                    let r = primary(ui, &t, tl!("Add to Timeline"), !d.title_lines.trim().is_empty());
                    app.auto.add("aiGen.placeTitle", r.rect, "Add to Timeline");
                    if r.clicked() {
                        match place_title(app, d.title_style, &d.title_lines) {
                            Ok(_) => d.say(tl!("Added at the playhead."), false),
                            Err(e) => d.say(e, true),
                        }
                    }
                }
                AiTab::Graphics => {
                    heading(ui, &t, tl!("Graphics templates"), tl!("Fill any graphics template, including imported .mogrt packages, with AI-written text."));
                    let lib = filmcraft_engine::graphic_templates::library(&app.session);
                    let current = lib.iter().find(|e| e.template.id == d.graphic_template).map(|e| e.template.name.clone()).unwrap_or_default();
                    field_label(ui, &t, tl!("Template"));
                    let mut picked = None;
                    let cb = craft_ui::ComboBox::from_id_salt("ai-graphic-template").width(WIDTH).selected_text(current).show_ui(ui, |ui| {
                        for e in &lib {
                            let label = format!("{}  ·  {}", e.template.name, e.template.category);
                            if ui.selectable_label(e.template.id == d.graphic_template, label).clicked() {
                                picked = Some(e.template.id.clone());
                            }
                        }
                    });
                    app.auto.add("aiGen.graphicTemplate", cb.response.rect, "Template");
                    if let Some(id) = picked {
                        d.graphic_template = id;
                        d.graphic_values.clear();
                    }
                    if d.graphic_values.is_empty() {
                        d.graphic_values = text_controls(app, &d.graphic_template);
                    }
                    ui.add_space(4.0);
                    field_label(ui, &t, tl!("Prompt"));
                    multiline(app, ui, "aiGen.graphicPrompt", &mut d.graphic_prompt, tl!("Lower third for our guest, a marine biologist from Lisbon"));
                    ui.add_space(GAP);
                    ui.horizontal(|ui| {
                        let r = secondary(ui, tl!("Write Fields"), !busy && !d.graphic_prompt.trim().is_empty() && !d.graphic_values.is_empty());
                        app.auto.add("aiGen.generateGraphic", r.rect, "Write Fields");
                        if busy {
                            ui.spinner();
                        }
                        if r.clicked() {
                            let fields: Vec<String> = d.graphic_values.iter().map(|(_, n, _)| n.clone()).collect();
                            let system = format!(
                                "You fill the text fields of a video title template. Reply with exactly {} lines and nothing else, one per field, in this order: {}. Keep each line short.",
                                fields.len(),
                                fields.join(", ")
                            );
                            match ai::Request::from_prefs(&prefs, &system, &d.graphic_prompt, false) {
                                Ok(req) => {
                                    d.job = Some((ai::spawn(ctx, req), Want::Graphic));
                                    d.say(tlf!("Writing with {provider}…", provider = ai::provider_label(&prefs.provider)), false);
                                }
                                Err(_) => {
                                    let words = offline_text(&d.graphic_prompt, "lower_third");
                                    for (slot, line) in d.graphic_values.iter_mut().zip(lines(&words)) {
                                        slot.2 = line;
                                    }
                                    d.say(tl!("Filled offline from your prompt. Add an API key in Settings for AI writing."), false);
                                }
                            }
                        }
                    });
                    ui.add_space(GAP);
                    egui::Grid::new("ai-graphic-fields").num_columns(2).spacing(vec2(GAP, 6.0)).min_col_width(96.0).show(ui, |ui| {
                        for (i, (_, name, value)) in d.graphic_values.iter_mut().enumerate() {
                            ui.label(RichText::new(name.as_str()).size(12.0).color(t.text_dim));
                            let r = ui.add(egui::TextEdit::singleline(value).desired_width(WIDTH - 120.0));
                            app.auto.add(&format!("aiGen.graphicField.{i}"), r.rect, name);
                            ui.end_row();
                        }
                    });
                    ui.add_space(GAP);
                    let r = primary(ui, &t, tl!("Add to Timeline"), true);
                    app.auto.add("aiGen.placeGraphic", r.rect, "Add to Timeline");
                    if r.clicked() {
                        let values: serde_json::Map<String, Value> =
                            d.graphic_values.iter().filter(|(_, _, v)| !v.trim().is_empty()).map(|(id, _, v)| (id.clone(), json!(v))).collect();
                        let r = need_sequence(app).and_then(|()| {
                            app.session.execute("graphics.template.apply", json!({"template": d.graphic_template, "values": values})).map_err(|e| e.to_string())
                        });
                        match r {
                            Ok(_) => d.say(tl!("Graphic added at the playhead."), false),
                            Err(e) => d.say(e, true),
                        }
                    }
                }
                AiTab::Video => {
                    heading(ui, &t, tl!("AI Video Generation"), tl!("Generate video clips using your online server API."));
                    field_label(ui, &t, tl!("Prompt"));
                    multiline(app, ui, "aiGen.videoPrompt", &mut d.video_prompt, tl!("Golden hour over rolling ocean waves, cinematic lighting"));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tl!("Duration")).size(12.0).color(t.text_dim));
                        let r = ui.add_sized(vec2(72.0, 22.0), crate::widgets::number_drag(&mut d.video_seconds).range(1.0..=60.0).speed(0.2).suffix(" s"));
                        app.auto.add("aiGen.videoSeconds", r.rect, "Duration");
                    });
                    ui.add_space(GAP);
                    let can_gen = keyed && !busy && !d.video_prompt.trim().is_empty();
                    let r = primary(ui, &t, tl!("Generate Video"), can_gen);
                    app.auto.add("aiGen.placeVideo", r.rect, "Generate Video");
                    if busy {
                        ui.spinner();
                    }
                    if r.clicked() {
                        if keyed {
                            d.job = Some((ai::generate_video(ctx, &prefs, &d.video_prompt, d.video_seconds), Want::Video(d.video_prompt.clone(), d.video_seconds)));
                            d.say(tlf!("Generating video with {provider} online server…", provider = ai::provider_label(&prefs.provider)), false);
                        } else {
                            d.say(tl!("Configure your online server API key in Settings to generate AI videos."), true);
                        }
                    }
                }
                AiTab::Performance => {
                    heading(
                        ui,
                        &t,
                        tl!("Smooth 4K Playback & Hardware AI"),
                        tl!("Analyze your laptop and automatically create low-res proxies for lag-free editing. Export always uses original full-res footage."),
                    );
                    let specs = ai::system_specs_summary(&app.session);
                    ui.group(|ui| {
                        ui.set_width(WIDTH - 16.0);
                        ui.label(RichText::new(tl!("Detected System & Project Media")).size(11.5).strong().color(t.text));
                        ui.add(egui::Label::new(RichText::new(&specs).size(11.5).color(t.text_dim)).wrap());
                    });
                    ui.add_space(6.0);
                    let mut auto_proxy = app.session.prefs.media.auto_proxy_4k;
                    if ui.checkbox(&mut auto_proxy, RichText::new(tl!("Auto-generate low-res proxies for 4K videos (Export uses original 4K)")).size(12.0)).changed() {
                        let _ = app.session.execute("prefs.set", json!({"key": "media.autoProxy4k", "value": auto_proxy}));
                    }
                    let mut use_proxies = app.session.media.use_proxies();
                    if ui.checkbox(&mut use_proxies, RichText::new(tl!("Use lightweight proxies during timeline playback")).size(12.0)).changed() {
                        let _ = app.session.execute("media.toggleProxies", json!({"on": use_proxies}));
                    }
                    ui.add_space(6.0);
                    // Direct OpenRouter API key input if not yet set
                    if prefs.openrouter_key.trim().is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tl!("OpenRouter API Key (Free models):")).size(12.0).color(t.text_dim));
                            let mut k = prefs.openrouter_key.clone();
                            let r = ui.add(egui::TextEdit::singleline(&mut k).password(true).hint_text("sk-or-v1-...").desired_width(WIDTH - 220.0));
                            if r.changed() {
                                let _ = app.session.execute("prefs.set", json!({"key": "ai.openrouterKey", "value": k}));
                            }
                        });
                        ui.add_space(4.0);
                    }
                    ui.horizontal(|ui| {
                        let can_analyze = !busy && (!prefs.openrouter_key.trim().is_empty() || keyed);
                        let r = primary(ui, &t, tl!("Analyze Laptop (OpenRouter Free AI)"), can_analyze);
                        app.auto.add("aiGen.analyzePerf", r.rect, "Analyze Laptop");
                        if r.clicked() {
                            match ai::analyze_performance(ctx, &prefs, &specs) {
                                Ok(job) => {
                                    d.job = Some((job, Want::Performance));
                                    d.say(tl!("Analyzing hardware & 4K playback needs via OpenRouter…"), false);
                                }
                                Err(e) => d.say(e, true),
                            }
                        }
                        let quick = secondary(ui, tl!("Convert All Video to Low-Res Proxies Now"), !busy);
                        app.auto.add("aiGen.proxyAllNow", quick.rect, "Convert All Video to Low-Res Proxies Now");
                        if quick.clicked() {
                            let video_items: Vec<u64> = app
                                .session
                                .project
                                .items
                                .values()
                                .filter_map(|it| {
                                    let m = it.as_media()?;
                                    (m.info.video.is_some() && m.proxy.is_none()).then_some(it.id.0)
                                })
                                .collect();
                            let _ = app.session.execute("media.toggleProxies", json!({"on": true}));
                            let _ = app.session.execute("prefs.set", json!({"key": "playback.draftDecode", "value": true}));
                            app.ui.program.res = crate::state::PlaybackRes::Half;
                            if video_items.is_empty() {
                                d.say(tl!("Playback optimized (1/2 res + draft decode). All video clips already have proxies or none imported."), false);
                            } else {
                                match app.session.execute("media.createProxies", json!({"items": video_items, "preset": "prores_proxy_quarter"})) {
                                    Ok(_) => d.say(tl!("Started background low-res proxy conversion! Timeline will use proxies; Export will use original footage."), false),
                                    Err(e) => d.say(e.to_string(), true),
                                }
                            }
                        }
                    });
                    if let Some(rec) = d.perf_rec.clone() {
                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.set_width(WIDTH - 16.0);
                            ui.label(RichText::new(tl!("AI Recommendation")).size(12.0).strong().color(t.accent));
                            ui.add(egui::Label::new(RichText::new(&rec.summary).size(12.0).color(t.text)).wrap());
                            ui.add_space(4.0);
                            let apply_btn = primary(ui, &t, tl!("Apply AI Settings & Generate Proxies"), !busy);
                            if apply_btn.clicked() {
                                let _ = app.session.execute("prefs.set", json!({"key": "media.autoProxy4k", "value": rec.enable_auto_proxy}));
                                let _ = app.session.execute("prefs.set", json!({"key": "playback.draftDecode", "value": rec.draft_decode}));
                                app.ui.program.res = match rec.playback_res.as_str() {
                                    "quarter" => crate::state::PlaybackRes::Quarter,
                                    "full" => crate::state::PlaybackRes::Full,
                                    _ => crate::state::PlaybackRes::Half,
                                };
                                if rec.enable_auto_proxy {
                                    let _ = app.session.execute("media.toggleProxies", json!({"on": true}));
                                    let preset = if rec.proxy_preset.is_empty() { "prores_proxy_quarter" } else { rec.proxy_preset.as_str() };
                                    let video_items: Vec<u64> = app
                                        .session
                                        .project
                                        .items
                                        .values()
                                        .filter_map(|it| {
                                            let m = it.as_media()?;
                                            (m.info.video.is_some() && m.proxy.is_none()).then_some(it.id.0)
                                        })
                                        .collect();
                                    if !video_items.is_empty() {
                                        let _ = app.session.execute("media.createProxies", json!({"items": video_items, "preset": preset}));
                                    }
                                }
                                d.say(tl!("Applied AI optimization settings and started proxy generation!"), false);
                            }
                        });
                    }
                }
                AiTab::Settings => {
                    heading(ui, &t, tl!("Provider and API keys"), tl!("Keys are stored in your NephClip preferences on this computer and sent only to the provider you choose. OpenRouter supports free models (default: openrouter/free)."));
                    field_label(ui, &t, tl!("Provider"));
                    let cur = prefs.provider.as_str();
                    let items: Vec<(&str, &str)> = ai::PROVIDERS.to_vec();
                    if let Some(p) = choices(app, ui, "provider", cur, &items) {
                        let _ = app.session.execute("prefs.set", json!({"key": "ai.provider", "value": p}));
                    }
                    ui.add_space(GAP);
                    egui::Grid::new("ai-keys").num_columns(2).spacing(vec2(GAP, 8.0)).min_col_width(140.0).show(ui, |ui| {
                        for (key, label, value) in [
                            ("ai.openrouterKey", tl!("OpenRouter API key"), prefs.openrouter_key.clone()),
                            ("ai.googleKey", tl!("Gemini API key"), prefs.google_key.clone()),
                            ("ai.openaiKey", tl!("OpenAI API key"), prefs.openai_key.clone()),
                            ("ai.claudeKey", tl!("Claude API key"), prefs.claude_key.clone()),
                            ("ai.serverUrl", tl!("Custom Server URL"), prefs.server_url.clone()),
                            ("ai.serverKey", tl!("Custom Server key"), prefs.server_key.clone()),
                        ] {
                            ui.label(RichText::new(label).size(12.0).color(t.text_dim));
                            let mut v = value;
                            let is_pwd = !key.contains("serverUrl");
                            let r = ui.add(egui::TextEdit::singleline(&mut v).password(is_pwd).desired_width(WIDTH - 170.0));
                            app.auto.add(&format!("aiGen.{key}"), r.rect, label);
                            if r.changed() {
                                let _ = app.session.execute("prefs.set", json!({"key": key, "value": v}));
                            }
                            ui.end_row();
                        }
                        ui.label(RichText::new(tl!("Model")).size(12.0).color(t.text_dim));
                        let mut m = prefs.model.clone();
                        let r = ui.add(egui::TextEdit::singleline(&mut m).hint_text(ai::default_model(&prefs.provider)).desired_width(WIDTH - 170.0));
                        app.auto.add("aiGen.model", r.rect, "Model");
                        if r.changed() {
                            let _ = app.session.execute("prefs.set", json!({"key": "ai.model", "value": m}));
                        }
                        ui.end_row();
                    });
                    ui.add_space(GAP);
                    ui.horizontal(|ui| {
                        let r = secondary(ui, tl!("Test Connection"), keyed && !busy);
                        app.auto.add("aiGen.test", r.rect, "Test Connection");
                        if busy {
                            ui.spinner();
                        }
                        if r.clicked()
                            && let Ok(req) = ai::Request::from_prefs(&prefs, "Reply with the single word OK.", "Connection test", false)
                        {
                            d.job = Some((ai::spawn(ctx, req), Want::Test));
                            d.say(tl!("Contacting the provider…"), false);
                        }
                    });
                }
            }

            // footer: status on the left, provider and Close on the right
            ui.add_space(GAP);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let (dot, note) = if keyed {
                    (t.accent, tlf!("{provider} · key set", provider = ai::provider_label(&prefs.provider)))
                } else {
                    (t.text_faint, tl!("Offline · no API key").to_string())
                };
                let (r, _) = ui.allocate_exact_size(vec2(8.0, 16.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, dot);
                ui.label(RichText::new(note).size(11.5).color(t.text_dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = secondary(ui, tl!("Close"), true);
                    app.auto.add("aiGen.close", r.rect, "Close");
                    if r.clicked() || crate::widgets::escape_closes(ctx) {
                        open = false;
                    }
                });
            });
            if !d.status.is_empty() {
                ui.add_space(4.0);
                let col = if d.failed { t.danger } else { t.text_dim };
                let r = ui.add(egui::Label::new(RichText::new(&d.status).size(12.0).color(col)).wrap());
                app.auto.add("aiGen.status", r.rect, &d.status);
            }
        });

    ctx.data_mut(|x| x.insert_temp(draft_id, d));
    if !open {
        app.dialog = None;
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_text_shapes_by_style() {
        assert_eq!(offline_text("the long way home tonight in the rain", "title"), "THE LONG WAY HOME TONIGHT IN");
        assert_eq!(offline_text("Ada Lovelace, mathematician", "lower_third"), "Ada Lovelace\nmathematician");
        assert_eq!(offline_text("hello there", "caption"), "Hello there");
        assert_eq!(lines(" \"A\" \n\n b "), ["A", "b"]);
    }

    #[test]
    fn local_ai_edit_handles_animate_transitions_and_split() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        app.session.execute("file.openDemoProject", json!({})).unwrap();
        let res = apply_local_ai_edit(&mut app, "Animate the text to slide in from bottom");
        assert!(res.is_ok(), "animate prompt should succeed: {:?}", res);
        let res_tr = apply_local_ai_edit(&mut app, "Add a cross dissolve transition");
        assert!(res_tr.is_ok(), "transition prompt should succeed: {:?}", res_tr);
        let res_split = apply_local_ai_edit(&mut app, "Split clip at playhead");
        assert!(res_split.is_ok(), "split prompt should succeed: {:?}", res_split);
    }
}
