//! AI timeline building (`ai.*`): a prompt becomes a [`Plan`] (a title and a list of scenes), and a
//! plan becomes a sequence: one generated clip per scene on V1, the scene's caption over it and a
//! title card at the start, as one undo step.
//!
//! The engine never talks to the network. A frontend sends [`PLANNER_SYSTEM`] and the user's
//! prompt to a language model of the user's choosing, reads the answer with [`parse_plan`] and
//! runs `ai.buildTimeline` with it. Without a model (no API key, offline, a bad answer)
//! [`local_plan`] makes a plan from the prompt's own words, so the command always works.
//!
//! Scene footage comes from FilmCraft's procedural demo generators (original work, see
//! `ATTRIBUTION.md`); the model only picks which one fits each scene.

use filmcraft_media::DemoScene;
use filmcraft_media::generators::{Generator, GeneratorSource};
use filmcraft_project::Label;
use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, bad, bool_p, str_p};
use crate::{EngineError, Result, Session};

/// Most scenes in a plan.
pub const MAX_SCENES: usize = 12;
/// Longest caption or title kept (chars).
const MAX_TEXT: usize = 160;

/// The visuals a scene can use, by the names a model answers with.
pub const VISUALS: [(&str, DemoScene); 6] = [
    ("ocean_sunset", DemoScene::OceanSunset),
    ("aurora", DemoScene::Aurora),
    ("city_night", DemoScene::CityNight),
    ("dunes", DemoScene::Dunes),
    ("forest", DemoScene::Forest),
    ("neon", DemoScene::Plasma),
];

/// The instructions a frontend sends to a language model with the user's prompt.
pub const PLANNER_SYSTEM: &str = "You plan short videos for a video editor. Reply with one JSON object and nothing else, in this shape: \
{\"title\": string (at most 6 words), \"subtitle\": string (at most 10 words, may be empty), \"scenes\": [{\"visual\": one of \"ocean_sunset\", \"aurora\", \"city_night\", \"dunes\", \"forest\", \"neon\", \"seconds\": number from 2 to 10, \"text\": on-screen caption of at most 8 words, may be empty}]}. \
Use 3 to 8 scenes that tell the story the user asks for, in order, and pick the visual that fits each scene best.";

/// One scene of a plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    /// One of [`VISUALS`]' names (anything else falls back to keyword matching).
    pub visual: String,
    pub seconds: f64,
    /// Caption shown over the scene (may be empty).
    pub text: String,
}

/// What `ai.buildTimeline` builds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plan {
    pub title: String,
    pub subtitle: String,
    pub scenes: Vec<Scene>,
}

fn clip_text(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
    s.trim().to_string()
}

/// The generator for a scene: its named visual, else the first one whose keywords appear in `hint`.
pub fn visual_for(name: &str, hint: &str) -> DemoScene {
    let n = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    if let Some((_, v)) = VISUALS.iter().find(|(k, _)| *k == n) {
        return *v;
    }
    scene_for_words(&format!("{name} {hint}"))
}

/// The generator whose keywords appear in `text` (the ocean sunset when none do).
pub fn scene_for_words(text: &str) -> DemoScene {
    let l = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| l.contains(w));
    if has(&["aurora", "northern lights", "night sky", "stars", "space", "sky"]) {
        DemoScene::Aurora
    } else if has(&["city", "drive", "traffic", "highway", "street", "urban", "car", "night"]) {
        DemoScene::CityNight
    } else if has(&["desert", "dune", "sand", "sahara", "dry", "heat"]) {
        DemoScene::Dunes
    } else if has(&["forest", "tree", "pine", "mist", "nature", "wood", "hike", "mountain"]) {
        DemoScene::Forest
    } else if has(&["neon", "cyber", "retro", "glow", "tech", "party", "music", "club", "abstract", "game"]) {
        DemoScene::Plasma
    } else {
        DemoScene::OceanSunset
    }
}

fn visual_name(v: DemoScene) -> &'static str {
    VISUALS.iter().find(|(_, x)| *x == v).map_or("ocean_sunset", |(k, _)| k)
}

/// Upper-case first letter.
fn sentence_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A plan made from the prompt alone, without a language model: each sentence (or clause) is a
/// scene captioned with it, on the visual its words suggest. A one-line prompt gets three scenes.
pub fn local_plan(prompt: &str) -> Plan {
    let prompt = prompt.trim();
    let mut parts: Vec<String> = prompt
        .split(['.', '!', '?', ';', '\n'])
        .flat_map(|s| s.split(" then "))
        .map(|s| s.trim().trim_matches(',').trim().to_string())
        .filter(|s| !s.is_empty())
        .take(MAX_SCENES)
        .collect();
    if parts.len() == 1 {
        let commas: Vec<String> = parts[0].split(',').map(|s| s.trim().to_string()).filter(|s| s.split_whitespace().count() >= 2).collect();
        if commas.len() > 1 {
            parts = commas;
        }
    }
    let title_words: Vec<&str> = parts.first().map(|p| p.split_whitespace().take(6).collect()).unwrap_or_default();
    let title = if title_words.is_empty() { "Untitled Story".to_string() } else { sentence_case(&title_words.join(" ")) };
    let mut scenes: Vec<Scene> =
        parts.iter().map(|p| Scene { visual: visual_name(scene_for_words(p)).into(), seconds: 4.0, text: clip_text(&sentence_case(p)) }).collect();
    if scenes.len() <= 1 {
        // one idea: open on its visual, then two more of the set, the caption on the middle one
        let first = scene_for_words(prompt);
        let others: Vec<DemoScene> = DemoScene::ALL.into_iter().filter(|v| *v != first).take(2).collect();
        scenes = std::iter::once(first)
            .chain(others)
            .enumerate()
            .map(|(i, v)| Scene { visual: visual_name(v).into(), seconds: 4.0, text: if i == 1 { clip_text(&sentence_case(prompt)) } else { String::new() } })
            .collect();
    } else if let Some(first) = scenes.first_mut() {
        // the title card covers the first scene
        first.text.clear();
    }
    Plan { title: clip_text(&title), subtitle: String::new(), scenes }
}

/// Reads a model's answer: the first JSON object in `text` (models sometimes wrap it in prose or
/// a code fence), checked and clamped. None when there is no usable plan.
pub fn parse_plan(text: &str) -> Option<Plan> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let json = text.get(start..=end)?;
    let plan: Plan = serde_json::from_str(json).ok()?;
    let plan = sanitize(plan);
    (!plan.scenes.is_empty()).then_some(plan)
}

/// Clamps a plan to what the builder accepts.
pub fn sanitize(mut plan: Plan) -> Plan {
    plan.title = clip_text(&plan.title);
    plan.subtitle = clip_text(&plan.subtitle);
    plan.scenes.truncate(MAX_SCENES);
    for s in &mut plan.scenes {
        s.text = clip_text(&s.text);
        s.seconds = if s.seconds.is_finite() { s.seconds.clamp(1.0, 30.0) } else { 4.0 };
    }
    plan
}

/// Builds `plan` into a new sequence (or, with `new_sequence` false and a sequence open, after the
/// end of the active one), as one undo step. Returns the sequence and the clips made.
pub fn build(s: &mut Session, plan: &Plan, new_sequence: bool) -> Result<Value> {
    let plan = sanitize(plan.clone());
    if plan.scenes.is_empty() {
        return Err(bad("ai.buildTimeline", "the plan has no scenes"));
    }
    let name = if plan.title.is_empty() { "AI Sequence".to_string() } else { plan.title.clone() };
    s.edit_group("AI: Build Timeline", |s| {
        let fresh = new_sequence || s.state.active_sequence.is_none();
        if fresh {
            s.execute("file.newSequence", json!({"name": name}))?;
        }
        let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
        let (w, h, rate) = (q.settings.width, q.settings.height, q.settings.frame_rate);
        let start = if fresh { Tick::ZERO } else { q.duration() };
        let mut t = start;
        let mut clips = Vec::new();
        for (i, scene) in plan.scenes.iter().enumerate() {
            let dur = rate.snap_nearest(Tick::from_seconds_f64(scene.seconds)).max(rate.frame_duration());
            let palette: [[f32; 4]; 6] = [
                [0.10, 0.12, 0.18, 1.0],
                [0.14, 0.10, 0.16, 1.0],
                [0.08, 0.14, 0.14, 1.0],
                [0.16, 0.12, 0.08, 1.0],
                [0.12, 0.12, 0.16, 1.0],
                [0.15, 0.15, 0.15, 1.0],
            ];
            let color = palette[i % palette.len()];
            let src = GeneratorSource::new(Generator::ColorMatte { color }, w, h, rate, dur);
            let pool = s.media.clone();
            let label = format!("AI Scene {}", i + 1);
            let item = s.edit("AI Scene", |pr, _| Ok(crate::demo::add_generator(pr, &pool, src, &label, Label::Iris, None)))?;
            let placed = s.execute("timeline.place", json!({"item": item.0, "time": t.0, "track": "V1"}))?;
            clips.push(placed);
            if !scene.text.is_empty() {
                let size = (f64::from(h) * 0.06).clamp(18.0, 400.0);
                let at = [f64::from(w) / 2.0, f64::from(h) * 0.86];
                let cap = s.execute(
                    "graphics.newText",
                    json!({"text": scene.text, "newClip": true, "seconds": dur.seconds(), "time": t.0, "size": size, "position": at, "fontStyle": "SemiBold"}),
                )?;
                clips.push(cap);
            }
            t += dur;
        }
        if !plan.title.is_empty() {
            let title = s.execute(
                "graphics.template.apply",
                json!({"template": "builtin:title-centered", "time": start.0, "values": {"title": plan.title, "subtitle": plan.subtitle}}),
            )?;
            clips.push(title);
        }
        s.execute("playhead.set", json!({"time": start.0}))?;
        s.state.selection.clear();
        Ok(json!({"sequence": s.state.active_sequence.map(|q| q.0), "scenes": plan.scenes.len(), "clips": clips, "plan": plan}))
    })
}

pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "ai.buildTimeline",
            label: "Build Timeline from Prompt",
            menu: &[],
            shortcut: None,
            params: r#"{"plan":{"title":str,"subtitle":str?,"scenes":[{"visual":"ocean_sunset|aurora|city_night|dunes|forest|neon","seconds":f64,"text":str?}]}?,"prompt":str? (planned locally when there is no `plan`),"newSequence":bool=true}"#,
            enabled: always,
            run: |s, p| {
                let plan = match p.get("plan") {
                    Some(v) if !v.is_null() => serde_json::from_value::<Plan>(v.clone()).map_err(|e| bad("ai.buildTimeline", format!("bad `plan`: {e}")))?,
                    _ => {
                        let prompt = str_p(p, "prompt").filter(|x| !x.trim().is_empty()).ok_or_else(|| bad("ai.buildTimeline", "need `plan` or `prompt`"))?;
                        local_plan(prompt)
                    }
                };
                build(s, &plan, bool_p(p, "newSequence").unwrap_or(true))
            },
            journal: true,
            named: crate::commands::NamedKeys::NONE,
        },
        CommandSpec {
            id: "ai.planLocal",
            label: "Plan Timeline from Prompt (offline)",
            menu: &[],
            shortcut: None,
            params: r#"{"prompt":str}"#,
            enabled: always,
            run: |_, p| {
                let prompt = str_p(p, "prompt").ok_or_else(|| bad("ai.planLocal", "need `prompt`"))?;
                serde_json::to_value(local_plan(prompt)).map_err(|e| EngineError::Other(e.to_string()))
            },
            journal: false,
            named: crate::commands::NamedKeys::NONE,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_plan_splits_sentences_and_matches_visuals() {
        let p = local_plan("A road trip through the desert. Night falls over the city. We camp in a misty forest.");
        assert_eq!(p.scenes.len(), 3);
        let v: Vec<_> = p.scenes.iter().map(|s| s.visual.as_str()).collect();
        assert_eq!(v, ["dunes", "city_night", "forest"]);
        assert_eq!(p.title, "A road trip through the desert");
        assert!(p.scenes[0].text.is_empty(), "the title covers the first scene");
    }

    #[test]
    fn one_line_prompt_gets_three_scenes() {
        let p = local_plan("neon music video");
        assert_eq!(p.scenes.len(), 3);
        assert_eq!(p.scenes[0].visual, "neon");
    }

    #[test]
    fn model_answers_are_parsed_and_clamped() {
        let a = "Sure! ```json\n{\"title\":\"Waves\",\"scenes\":[{\"visual\":\"Ocean Sunset\",\"seconds\":900,\"text\":\"Hi\"},{\"visual\":\"x\",\"seconds\":-3}]}\n```";
        let p = parse_plan(a).unwrap();
        assert_eq!(p.scenes[0].seconds, 30.0);
        assert_eq!(p.scenes[1].seconds, 1.0);
        assert_eq!(visual_for(&p.scenes[0].visual, ""), DemoScene::OceanSunset);
        assert!(parse_plan("no json here").is_none());
        assert!(parse_plan("{\"scenes\": []}").is_none());
        assert!(parse_plan("{\"scenes\": \"oops\"}").is_none());
    }

    #[test]
    fn build_makes_a_sequence_in_one_undo_step() {
        let mut s = Session::default();
        let r = s.execute("ai.buildTimeline", json!({"prompt": "Sunrise at sea. A drive through the city. Stars over the desert."})).unwrap();
        assert_eq!(r["scenes"], 3);
        let q = s.active_sequence().unwrap();
        assert_eq!(q.video_tracks[0].items.len(), 3, "one clip per scene on V1");
        let graphics: usize = q.video_tracks.iter().skip(1).map(|t| t.items.len()).sum();
        assert_eq!(graphics, 3, "two captions and the title");
        assert_eq!(q.duration(), q.settings.frame_rate.snap_nearest(Tick::from_seconds_f64(12.0)));
        // one undo removes it all
        let before = s.project.items.len();
        s.execute("edit.undo", json!({})).unwrap();
        assert!(s.project.items.len() < before);
        // a hostile plan is clamped, an empty one refused
        assert!(s.execute("ai.buildTimeline", json!({"plan": {"title": "x", "scenes": []}})).is_err());
        assert!(s.execute("ai.buildTimeline", json!({"plan": "nope"})).is_err());
        let many: Vec<Value> = (0..100).map(|_| json!({"visual": "??", "seconds": f64::MAX})).collect();
        let r = s.execute("ai.buildTimeline", json!({"plan": {"title": "", "scenes": many}})).unwrap();
        assert_eq!(r["scenes"], MAX_SCENES);
    }
}
