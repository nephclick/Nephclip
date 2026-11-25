//! Requests to the user's AI provider (Settings ▸ AI Generation): Claude (Anthropic), Google
//! Gemini or OpenRouter, with the user's own API key.
//!
//! A request runs on a background thread (under `catch_unwind`) and the UI polls its [`AiJob`]
//! each frame, so the window never freezes while a model answers. The HTTPS call goes through the
//! system `curl`; the key and body are passed on curl's standard input as a config file, never on
//! its command line, so other processes cannot read the key from the process list. Nothing is
//! sent anywhere until the user asks for a generation with a key configured.

use std::sync::{Arc, Mutex, PoisonError};

use filmcraft_engine::settings::AiPrefs;

/// What a request returns: the model's text, or an error to show.
pub type Answer = Result<String, String>;

/// A request in flight.
#[derive(Clone, Default)]
pub struct AiJob {
    slot: Arc<Mutex<Option<Answer>>>,
}

impl AiJob {
    /// The answer, once (None while the model is still working).
    pub fn poll(&self) -> Option<Answer> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// The providers, as stored in `ai.provider`, with their display names.
pub const PROVIDERS: [(&str, &str); 5] = [
    ("google", "Google Gemini"),
    ("openai", "OpenAI"),
    ("claude", "Claude"),
    ("openrouter", "OpenRouter"),
    ("custom", "Custom Server API"),
];

pub fn provider_label(id: &str) -> &'static str {
    PROVIDERS.iter().find(|(k, _)| *k == id).map_or("Google Gemini", |(_, l)| l)
}

/// Default free model on OpenRouter (automatically routes to available free models).
pub const OPENROUTER_FREE_MODEL: &str = "openrouter/free";

/// The model used when Settings ▸ AI Generation ▸ Model is blank.
pub fn default_model(provider: &str) -> &'static str {
    match provider {
        "claude" => "claude-opus-5-5",
        "openrouter" => OPENROUTER_FREE_MODEL,
        "openai" => "gpt-4o",
        "custom" => "default",
        _ => "gemini-2.5-flash",
    }
}

/// The API key the selected provider uses (may be blank).
pub fn key_of<'a>(prefs: &'a AiPrefs, provider: &str) -> &'a str {
    match provider {
        "claude" => &prefs.claude_key,
        "openrouter" => &prefs.openrouter_key,
        "openai" => &prefs.openai_key,
        "custom" => if !prefs.server_key.is_empty() { &prefs.server_key } else { &prefs.server_url },
        _ => &prefs.google_key,
    }
}

/// Whether the selected provider has a key.
pub fn configured(prefs: &AiPrefs) -> bool {
    !key_of(prefs, &prefs.provider).trim().is_empty()
}

/// One request: a system instruction and the user's prompt.
#[derive(Clone, Debug)]
pub struct Request {
    pub provider: String,
    pub key: String,
    pub model: String,
    pub server_url: String,
    pub system: String,
    pub prompt: String,
    /// Ask for a JSON answer (where the provider has a switch for it).
    pub json: bool,
}

impl Request {
    /// A request to the provider chosen in `prefs`; Err when it has no key.
    pub fn from_prefs(prefs: &AiPrefs, system: &str, prompt: &str, json: bool) -> Result<Request, String> {
        let provider = prefs.provider.clone();
        let key = key_of(prefs, &provider).trim().to_string();
        if key.is_empty() {
            return Err(format!("No API key for {}: add one in the AI window's Settings tab.", provider_label(&provider)));
        }
        let model = if prefs.model.trim().is_empty() { default_model(&provider).to_string() } else { prefs.model.trim().to_string() };
        let server_url = prefs.server_url.clone();
        Ok(Request { provider, key, model, server_url, system: system.into(), prompt: prompt.into(), json })
    }
}

/// Starts `req` on a background thread; `ctx` is repainted when the answer arrives.
pub fn spawn(ctx: &egui::Context, req: Request) -> AiJob {
    let job = AiJob::default();
    let slot = job.slot.clone();
    let ctx = ctx.clone();
    let run = move || {
        let answer = std::panic::catch_unwind(|| call(&req)).unwrap_or_else(|_| Err("the AI request failed unexpectedly".into()));
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
        ctx.request_repaint();
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Err(e) = std::thread::Builder::new().name("ai-request".into()).spawn(run) {
            *job.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err(format!("could not start the request: {e}")));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(run);
        *job.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err("AI generation needs the desktop app".into()));
    }
    job
}

/// A value for a curl config file: quoted, with `\` and `"` escaped and no raw line breaks.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The endpoint, headers and JSON body of `req`.
fn http_parts(req: &Request) -> (String, Vec<String>, serde_json::Value) {
    use serde_json::json;
    match req.provider.as_str() {
        "claude" => (
            "https://api.anthropic.com/v1/messages".into(),
            vec![
                format!("x-api-key: {}", req.key),
                "anthropic-version: 2026-10-21".into(),
                // on a safety decline the API retries the request on a fallback model it picks
                "anthropic-beta: server-side-fallback-2026-07-01".into(),
            ],
            json!({
                "model": req.model,
                "max_tokens": 16000,
                "system": req.system,
                "messages": [{"role": "user", "content": req.prompt}],
                "output_config": {"effort": "low"},
                "fallbacks": "default",
            }),
        ),
        "openrouter" => (
            "https://openrouter.ai/api/v1/chat/completions".into(),
            vec![format!("Authorization: Bearer {}", req.key)],
            json!({
                "model": req.model,
                "messages": [{"role": "system", "content": req.system}, {"role": "user", "content": req.prompt}],
            }),
        ),
        "openai" => (
            "https://api.openai.com/v1/chat/completions".into(),
            vec![format!("Authorization: Bearer {}", req.key)],
            json!({
                "model": req.model,
                "messages": [{"role": "system", "content": req.system}, {"role": "user", "content": req.prompt}],
            }),
        ),
        "custom" => {
            let url = if req.server_url.is_empty() {
                "http://localhost:8080/v1/chat/completions".to_string()
            } else {
                req.server_url.clone()
            };
            (
                url,
                if req.key.is_empty() { vec![] } else { vec![format!("Authorization: Bearer {}", req.key)] },
                json!({
                    "model": req.model,
                    "messages": [{"role": "system", "content": req.system}, {"role": "user", "content": req.prompt}],
                }),
            )
        }
        _ => {
            let mut body = json!({
                "systemInstruction": {"parts": [{"text": req.system}]},
                "contents": [{"role": "user", "parts": [{"text": req.prompt}]}],
            });
            if req.json {
                body["generationConfig"] = json!({"responseMimeType": "application/json"});
            }
            let model: String = req.model.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')).collect();
            (format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"), vec![format!("x-goog-api-key: {}", req.key)], body)
        }
    }
}

/// The text of a provider's answer, or its error.
pub fn read_answer(provider: &str, resp: &serde_json::Value) -> Answer {
    let s = |v: Option<&serde_json::Value>| v.and_then(serde_json::Value::as_str).map(str::to_string);
    if let Some(msg) = s(resp.pointer("/error/message")) {
        return Err(format!("{}: {msg}", provider_label(provider)));
    }
    let text = match provider {
        "claude" => {
            if s(resp.get("stop_reason")).as_deref() == Some("refusal") {
                return Err("Claude declined this request. Try rewording the prompt.".into());
            }
            // thinking blocks may come first: keep only the text blocks
            let parts: Vec<String> = resp
                .get("content")
                .and_then(serde_json::Value::as_array)
                .map(|a| a.iter().filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text")).filter_map(|b| s(b.get("text"))).collect())
                .unwrap_or_default();
            (!parts.is_empty()).then(|| parts.join(""))
        }
        "openrouter" | "openai" | "custom" => s(resp.pointer("/choices/0/message/content")),
        _ => resp
            .pointer("/candidates/0/content/parts")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(|p| s(p.get("text"))).collect::<Vec<_>>().join(""))
            .filter(|t| !t.is_empty()),
    };
    text.map(|t| t.trim().to_string()).ok_or_else(|| format!("{} sent an answer NephClip could not read", provider_label(provider)))
}

#[cfg(not(target_arch = "wasm32"))]
fn call(req: &Request) -> Answer {
    use std::io::Write;
    let (url, headers, body) = http_parts(req);
    let mut config = format!("url = {}\nrequest = \"POST\"\nsilent\nshow-error\nmax-time = 120\nheader = \"Content-Type: application/json\"\n", quoted(&url));
    for h in &headers {
        config.push_str(&format!("header = {}\n", quoted(h)));
    }
    config.push_str(&format!("data-binary = {}\n", quoted(&body.to_string())));
    let mut child = std::process::Command::new("curl")
        .args(["--config", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run curl ({e}); AI generation needs curl installed"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes()).map_err(|e| format!("could not send the request: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("the request failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Network error: {}", err.trim()));
    }
    let resp: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|_| format!("{} sent a reply that is not JSON", provider_label(&req.provider)))?;
    read_answer(&req.provider, &resp)
}

#[cfg(target_arch = "wasm32")]
fn call(_: &Request) -> Answer {
    Err("AI generation needs the desktop app".into())
}

/// The AI director shared by Home and the AI window: a prompt becomes a planned sequence on the
/// timeline (`ai.buildTimeline`). With a key, the provider plans the scenes; without one, or when
/// its answer is unusable, the engine's offline planner does, and the status says so.
#[derive(Clone, Default)]
pub struct Director {
    pub prompt: String,
    /// Build into a new sequence (else after the end of the active one).
    pub new_sequence: bool,
    pub job: Option<AiJob>,
    pub status: String,
    pub failed: bool,
}

impl Director {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts building from `self.prompt`. Returns true when the timeline was built right away.
    pub fn start(&mut self, app: &mut crate::FilmcraftApp, ctx: &egui::Context) -> bool {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            self.set_status("Describe the video you want first.", true);
            return false;
        }
        match Request::from_prefs(&app.session.prefs.ai, filmcraft_engine::ai_timeline::PLANNER_SYSTEM, &prompt, true) {
            Ok(req) => {
                self.set_status(&format!("Planning with {}…", provider_label(&req.provider)), false);
                self.job = Some(spawn(ctx, req));
                false
            }
            Err(e) => {
                self.set_status(&format!("Online server API required: {e}"), true);
                false
            }
        }
    }

    /// Picks up the provider's answer. Returns true when the timeline was built this frame.
    pub fn poll(&mut self, app: &mut crate::FilmcraftApp) -> bool {
        let Some(answer) = self.job.as_ref().and_then(AiJob::poll) else { return false };
        self.job = None;
        match answer.map(|text| filmcraft_engine::ai_timeline::parse_plan(&text)) {
            Ok(Some(plan)) => self.build(app, plan, "Timeline built from your prompt."),
            Ok(None) => {
                self.set_status("The online server's plan was unusable. Try rewording your prompt.", true);
                false
            }
            Err(e) => {
                self.set_status(&format!("Online server API error: {e}"), true);
                false
            }
        }
    }

    fn build(&mut self, app: &mut crate::FilmcraftApp, plan: filmcraft_engine::ai_timeline::Plan, done: &str) -> bool {
        let plan = match serde_json::to_value(&plan) {
            Ok(v) => v,
            Err(e) => {
                self.set_status(&e.to_string(), true);
                return false;
            }
        };
        match app.session.execute("ai.buildTimeline", serde_json::json!({"plan": plan, "newSequence": self.new_sequence})) {
            Ok(r) => {
                let n = r["scenes"].as_u64().unwrap_or(0);
                self.set_status(&format!("{done} {n} scenes."), false);
                app.ui.status = self.status.clone();
                true
            }
            Err(e) => {
                self.set_status(&e.to_string(), true);
                false
            }
        }
    }

    fn set_status(&mut self, s: &str, failed: bool) {
        self.status = s.to_string();
        self.failed = failed;
    }
}

/// Online video generation request to the server API.
pub fn generate_video(ctx: &egui::Context, prefs: &AiPrefs, prompt: &str, seconds: f64) -> AiJob {
    let provider = prefs.provider.clone();
    let key = key_of(prefs, &provider).trim().to_string();
    let prompt = prompt.to_string();
    let server_url = prefs.server_url.clone();
    let job = AiJob::default();
    let slot = job.slot.clone();
    let ctx = ctx.clone();
    let run = move || {
        let answer = std::panic::catch_unwind(|| {
            if key.is_empty() {
                return Err("No online server API key configured. Please set your API key in Settings.".into());
            }
            call_online_video_api(&provider, &key, &server_url, &prompt, seconds)
        })
        .unwrap_or_else(|_| Err("The video generation request failed unexpectedly".into()));
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
        ctx.request_repaint();
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Err(e) = std::thread::Builder::new().name("ai-video-gen".into()).spawn(run) {
            *job.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err(format!("Could not start video generation: {e}")));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(run);
        *job.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err("AI video generation needs the desktop app".into()));
    }
    job
}

#[cfg(not(target_arch = "wasm32"))]
fn call_online_video_api(provider: &str, key: &str, server_url: &str, prompt: &str, seconds: f64) -> Answer {
    use std::io::Write;
    let url = if !server_url.is_empty() {
        server_url.to_string()
    } else {
        match provider {
            "openai" => "https://api.openai.com/v1/videos/generations".into(),
            "openrouter" => "https://openrouter.ai/api/v1/videos/generations".into(),
            _ => "https://api.openai.com/v1/videos/generations".into(),
        }
    };
    let body = serde_json::json!({
        "prompt": prompt,
        "seconds": seconds,
    });
    let mut config = format!(
        "url = {}\nrequest = \"POST\"\nsilent\nshow-error\nmax-time = 180\nheader = \"Content-Type: application/json\"\nheader = \"Authorization: Bearer {}\"\n",
        quoted(&url),
        quoted(key)
    );
    config.push_str(&format!("data-binary = {}\n", quoted(&body.to_string())));
    let mut child = std::process::Command::new("curl")
        .args(["--config", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run curl ({e})"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes()).map_err(|e| format!("could not send the request: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("the request failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Network error: {}", err.trim()));
    }
    let resp: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| "The video server sent an invalid response.".to_string())?;
    if let Some(err) = resp.pointer("/error/message").and_then(serde_json::Value::as_str) {
        return Err(format!("Video API Error: {err}"));
    }
    let video_url = resp.pointer("/data/0/url").or_else(|| resp.get("url")).and_then(serde_json::Value::as_str);
    if let Some(v_url) = video_url {
        let dest = std::env::temp_dir().join(format!(
            "ai_video_{}.mp4",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
        ));
        let dest_str = dest.to_string_lossy().to_string();
        let dl = std::process::Command::new("curl")
            .args(["-s", "-o", &dest_str, v_url])
            .status();
        if dl.map(|s| s.success()).unwrap_or(false) && dest.exists() {
            return Ok(dest_str);
        }
    }
    Err("Video generation completed, but server response did not include a downloadable video URL.".into())
}

#[cfg(target_arch = "wasm32")]
fn call_online_video_api(_: &str, _: &str, _: &str, _: &str, _: f64) -> Answer {
    Err("AI video generation needs the desktop app".into())
}

/// Structured performance & proxy recommendation returned by the AI hardware analyzer.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PerfRecommendation {
    pub summary: String,
    pub enable_auto_proxy: bool,
    pub proxy_preset: String,
    pub playback_res: String,
    pub draft_decode: bool,
}

/// Parse the JSON recommendation from the model's answer.
pub fn parse_perf_recommendation(text: &str) -> Option<PerfRecommendation> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let json_str = text.get(start..=end)?;
    serde_json::from_str::<PerfRecommendation>(json_str).ok()
}

/// Collects local hardware specifications and current project media stats for AI optimization.
pub fn system_specs_summary(session: &filmcraft_engine::Session) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let ram_gb: Option<u64> = {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("sysctl")
                .args(["-n", "hw.memsize"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|b| b / (1024 * 1024 * 1024))
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    };
    let ram_str = ram_gb.map(|g| format!("{g} GB")).unwrap_or_else(|| "Unknown".into());
    let mut movies = 0usize;
    let mut high_res = 0usize;
    let mut proxied = 0usize;
    let mut max_w = 0u32;
    let mut max_h = 0u32;
    for item in session.project.items.values() {
        if let Some(m) = item.as_media()
            && let Some(v) = &m.info.video
        {
            movies += 1;
            max_w = max_w.max(v.width);
            max_h = max_h.max(v.height);
            if v.width > 1920 || v.height > 1080 {
                high_res += 1;
            }
            if m.proxy.is_some() {
                proxied += 1;
            }
        }
    }
    format!(
        "OS: {os} ({arch}), CPU threads: {cores}, RAM: {ram_str}. Project video clips: {movies} (4K/high-res: {high_res}, max resolution: {max_w}x{max_h}, proxies attached: {proxied})."
    )
}

/// Analyze laptop hardware and media performance using OpenRouter's free model (or configured provider).
pub fn analyze_performance(ctx: &egui::Context, prefs: &AiPrefs, specs: &str) -> Result<AiJob, String> {
    let system = "You are a video editing performance engineer for NephClip. \
        Analyze the user's laptop hardware and project video resolution. \
        Return ONLY a single valid JSON object with this exact shape: \
        {\"summary\": string (2-3 concise sentences diagnosing playback lag and explaining why proxies help while export keeps original 4K), \
         \"enableAutoProxy\": true|false, \
         \"proxyPreset\": \"prores_proxy_quarter\"|\"h264_quarter\"|\"prores_proxy_half\", \
         \"playbackRes\": \"full\"|\"half\"|\"quarter\", \
         \"draftDecode\": true|false}";
    let req = if !prefs.openrouter_key.trim().is_empty() {
        Request {
            provider: "openrouter".into(),
            key: prefs.openrouter_key.trim().to_string(),
            model: OPENROUTER_FREE_MODEL.into(),
            server_url: String::new(),
            system: system.into(),
            prompt: specs.into(),
            json: true,
        }
    } else {
        Request::from_prefs(prefs, system, specs, true)?
    };
    Ok(spawn(ctx, req))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_values_are_quoted() {
        assert_eq!(quoted("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }

    #[test]
    fn keys_go_in_headers_not_urls() {
        let prefs = AiPrefs { google_key: "SECRET".into(), provider: "google".into(), ..Default::default() };
        let r = Request::from_prefs(&prefs, "sys", "hi", true).unwrap();
        let (url, headers, body) = http_parts(&r);
        assert!(!url.contains("SECRET"));
        assert!(headers.iter().any(|h| h.contains("SECRET")));
        assert_eq!(body["generationConfig"]["responseMimeType"], "application/json");
        assert!(Request::from_prefs(&AiPrefs { provider: "claude".into(), ..Default::default() }, "", "", false).is_err());
    }

    #[test]
    fn answers_are_read_per_provider() {
        let claude = json!({"content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": " Hello "}], "stop_reason": "end_turn"});
        assert_eq!(read_answer("claude", &claude).unwrap(), "Hello");
        assert!(read_answer("claude", &json!({"content": [], "stop_reason": "refusal"})).is_err());
        assert_eq!(read_answer("openrouter", &json!({"choices": [{"message": {"content": "x"}}]})).unwrap(), "x");
        assert_eq!(read_answer("google", &json!({"candidates": [{"content": {"parts": [{"text": "g"}]}}]})).unwrap(), "g");
        assert!(read_answer("google", &json!({"error": {"message": "bad key"}})).unwrap_err().contains("bad key"));
        assert!(read_answer("google", &json!({})).is_err());
    }
}
