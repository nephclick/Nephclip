//! Home (the header's house button): an AI prompt bar that builds a whole sequence from a
//! description, New Project / Open Project / Import Media, and the recent projects.

use egui::{Align2, Color32, Rect, Sense, pos2, vec2};
use serde_json::json;

use crate::FilmcraftApp;
use crate::ai::Director;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// Width of the centred column.
const COLUMN: f32 = 820.0;
const RECENT_MAX: usize = 9;

fn thumb_cache_path(project_path: &str) -> Option<std::path::PathBuf> {
    let dir = filmcraft_engine::autosave::default_data_dir()?.join("recent_thumbs");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in project_path.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    Some(dir.join(format!("{h:016x}.thumb")))
}

/// Snapshot the currently open project's video frame into the recent-thumbnails cache so Home
/// displays a real video thumbnail for it.
fn maybe_capture_active_thumb(app: &mut FilmcraftApp) {
    let Some(path) = app.session.path.clone() else { return };
    let Some(out_path) = thumb_cache_path(&path) else { return };
    // Try active sequence's last rendered frame or first video clip thumbnail in frame cache
    let img = app.program_shown.and_then(|k| app.frames.get(&k)).or_else(|| {
        let seq_id = app.session.state.active_sequence?;
        let key = crate::frames::FrameKey {
            target: crate::frames::Target::Sequence(seq_id),
            frame: 0,
            size: 240,
            revision: app.session.revision,
            draft: false,
        };
        app.frames.get(&key)
    });
    let Some(img) = img else { return };
    if img.w == 0 || img.h == 0 {
        return;
    }
    // Downsample to max 240px wide RGBA for a tiny, instant-loading thumbnail file
    let tw = img.w.min(240);
    let th = ((img.h as u64 * tw as u64) / img.w.max(1) as u64).max(1) as usize;
    let mut bytes = Vec::with_capacity(8 + tw * th * 4);
    bytes.extend_from_slice(&(tw as u32).to_le_bytes());
    bytes.extend_from_slice(&(th as u32).to_le_bytes());
    for y in 0..th {
        let sy = (y * img.h) / th;
        for x in 0..tw {
            let sx = (x * img.w) / tw;
            let off = (sy * img.w + sx) * 4;
            if let Some(px) = img.px.get(off..off + 4) {
                bytes.extend_from_slice(px);
            } else {
                bytes.extend_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(out_path, bytes);
}

#[derive(Clone, Default)]
struct RecentThumbCache {
    loaded: std::collections::BTreeMap<String, Option<egui::TextureHandle>>,
}

fn load_recent_thumb(ctx: &egui::Context, project_path: &str) -> Option<egui::TextureId> {
    let cache_id = egui::Id::unique("home-recent-thumbs");
    if let Some(existing) = ctx.data(|d| d.get_temp::<RecentThumbCache>(cache_id)).and_then(|c| c.loaded.get(project_path).cloned()) {
        return existing.map(|h| h.id());
    }
    let handle = thumb_cache_path(project_path)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| {
            let w = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize;
            let h = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
            let rgba = bytes.get(8..)?;
            if w == 0 || h == 0 || w > 1024 || h > 1024 || rgba.len() != w * h * 4 {
                return None;
            }
            let color_img = egui::ColorImage::from_rgba_unmultiplied([w, h], rgba);
            Some(ctx.load_texture(format!("home-recent-thumb-{project_path}"), color_img, egui::TextureOptions::LINEAR))
        });
    let tid = handle.as_ref().map(|h| h.id());
    ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<RecentThumbCache>(cache_id);
        c.loaded.insert(project_path.to_string(), handle);
    });
    tid
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    ui.painter().rect_filled(rect, t.radius, t.panel_bg);
    let w = COLUMN.min(rect.width() - 48.0).max(240.0);
    let x0 = rect.center().x - w / 2.0;
    let mut y = rect.min.y + 48.0;

    // ----- welcome
    ui.painter().text(pos2(x0, y), Align2::LEFT_CENTER, tl!("Welcome to NephClip"), Tokens::semibold(24.0), t.text);
    y += 30.0;
    ui.painter().text(pos2(x0, y), Align2::LEFT_CENTER, tl!("Describe a video and let AI build it, or start from a project."), Tokens::ui(13.0), t.text_dim);
    y += 32.0;

    // ----- AI prompt bar
    let id = egui::Id::unique("home-director");
    let mut d: Director = ctx.data(|x| x.get_temp(id)).unwrap_or_else(|| Director { new_sequence: true, ..Default::default() });
    if d.poll(app) {
        app.ui.mode = crate::state::Mode::Edit;
    }
    let busy = d.busy();
    if busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let bar = Rect::from_min_size(pos2(x0, y), vec2(w, 52.0));
    ui.painter().rect_filled(bar, 26.0, t.field_bg);
    ui.painter().rect_stroke(bar, 26.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(bar.min.x + 28.0, bar.center().y), vec2(18.0, 18.0)), Icon::Sparkle, t.accent);
    let btn = Rect::from_min_size(pos2(bar.max.x - 8.0 - 112.0, bar.min.y + 8.0), vec2(112.0, 36.0));
    let field = Rect::from_min_max(pos2(bar.min.x + 48.0, bar.min.y + 8.0), pos2(btn.min.x - 12.0, bar.max.y - 8.0));
    let mut submit = false;
    let mut fu = ui.new_child(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let edit = fu.add_enabled(
        !busy,
        egui::TextEdit::singleline(&mut d.prompt)
            .hint_text(tl!("What should we create? e.g. a 30-second travel teaser from the desert to the northern lights"))
            .frame(egui::Frame::NONE)
            .font(Tokens::ui(14.0))
            .desired_width(field.width()),
    );
    app.auto.add("home.aiPrompt", field, "AI prompt");
    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        submit = true;
    }
    let bresp = ui.interact(btn, egui::Id::unique("home-ai-create"), Sense::click());
    app.auto.add("home.aiCreate", btn, "Create");
    let enabled = !busy && !d.prompt.trim().is_empty();
    let fill = if !enabled {
        t.accent.gamma_multiply(0.45)
    } else if bresp.hovered() {
        t.accent_hover
    } else {
        t.accent
    };
    ui.painter().rect_filled(btn, 18.0, fill);
    if busy {
        let sp = Rect::from_center_size(btn.center(), vec2(16.0, 16.0));
        ui.put(sp, egui::Spinner::new().color(Color32::WHITE));
    } else {
        ui.painter().text(btn.center(), Align2::CENTER_CENTER, tl!("Create"), Tokens::semibold(13.5), Color32::WHITE);
    }
    if (bresp.clicked() || submit) && enabled && d.start(app, &ctx) {
        app.ui.mode = crate::state::Mode::Edit;
    }
    y = bar.max.y + 10.0;
    // status, or what will happen
    let (note, col) = if !d.status.is_empty() {
        (d.status.clone(), if d.failed { t.danger } else { t.text_dim })
    } else if crate::ai::configured(&app.session.prefs.ai) {
        (
            tlf!(
                "Builds a new sequence with {provider}: scenes, captions and a title card.",
                provider = crate::ai::provider_label(&app.session.prefs.ai.provider)
            ),
            t.text_dim,
        )
    } else {
        (tl!("Builds a new sequence with scenes, captions and a title card. Add an API key in AI Generation for smarter plans.").to_string(), t.text_dim)
    };
    let g = ui.painter().layout(note, Tokens::ui(12.0), col, w - 24.0);
    let gh = g.size().y;
    ui.painter().galley(pos2(x0 + 24.0, y), g, col);
    ctx.data_mut(|x| x.insert_temp(id, d));
    y += gh + 32.0;

    // ----- actions
    let actions = [
        (Icon::Plus, tl!("New Project"), tl!("Start an empty project"), "newProject"),
        (Icon::Folder, tl!("Open Project…"), tl!("Open a project from disk"), "openProject"),
        (Icon::Import, tl!("Import Media"), tl!("Bring in footage, audio, stills and .mogrt"), "import"),
    ];
    let gap = 16.0;
    let cw = (w - gap * 2.0) / 3.0;
    for (i, (icon, title, sub, key)) in actions.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(x0 + i as f32 * (cw + gap), y), vec2(cw, 76.0));
        let resp = ui.interact(r, egui::Id::unique(("home-action", key)), Sense::click());
        app.auto.add(&format!("home.{key}"), r, title);
        ui.painter().rect_filled(r, 10.0, if resp.hovered() { t.hover } else { t.tl_header_bg });
        let ic = Rect::from_center_size(pos2(r.min.x + 30.0, r.center().y), vec2(36.0, 36.0));
        ui.painter().rect_filled(ic, 8.0, t.accent.gamma_multiply(0.18));
        icons::paint(ui.painter(), ic.shrink(9.0), icon, t.accent);
        let tx = ic.max.x + 14.0;
        let clip = ui.painter().with_clip_rect(r.shrink(4.0));
        clip.text(pos2(tx, r.center().y - 9.0), Align2::LEFT_CENTER, title, Tokens::semibold(14.0), t.text);
        clip.text(pos2(tx, r.center().y + 11.0), Align2::LEFT_CENTER, sub, Tokens::ui(11.5), t.text_dim);
        if resp.clicked() {
            match key {
                "newProject" => {
                    if crate::menus::invoke(app, &ctx, "file.newProject", json!({})).is_ok() && app.ui.clip_dialog.is_none() {
                        app.ui.mode = crate::state::Mode::Edit;
                    }
                }
                "openProject" => {
                    if let Err(e) = crate::menus::invoke(app, &ctx, "file.open", json!({})) {
                        app.ui.status = e;
                    }
                }
                _ => app.ui.mode = crate::state::Mode::Import,
            }
        }
    }
    y += 76.0 + 36.0;

    // ----- recent projects
    ui.painter().text(pos2(x0, y), Align2::LEFT_CENTER, tl!("Recent Projects"), Tokens::semibold(15.0), t.text);
    y += 16.0;
    recent(app, ui, x0, y, w, rect.max.y - 16.0);
}

/// The recent projects (newest first) displayed as a video thumbnail card grid; a click opens one.
fn recent(app: &mut FilmcraftApp, ui: &mut egui::Ui, x0: f32, y: f32, w: f32, bottom: f32) {
    maybe_capture_active_thumb(app);
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    let paths: Vec<String> = app.session.prefs.general.recent_projects.iter().take(RECENT_MAX).cloned().collect();
    if paths.is_empty() {
        ui.painter().text(pos2(x0, y + 18.0), Align2::LEFT_CENTER, tl!("No recent projects yet. Saved projects appear here."), Tokens::ui(12.5), t.text_dim);
        return;
    }
    let cols = if w >= 660.0 { 3 } else if w >= 420.0 { 2 } else { 1 };
    let gap = 14.0;
    let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
    let thumb_h = (cw * 9.0 / 16.0).clamp(80.0, 148.0);
    let card_h = thumb_h + 44.0;
    let avail_h = (bottom - y).max(120.0);
    let grid_rect = Rect::from_min_size(pos2(x0, y), vec2(w, avail_h));
    let mut open = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid_rect).id_salt("home-recent-grid"));
    child.set_clip_rect(grid_rect.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical().id_salt("home-recent-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
        for (row_idx, chunk) in paths.chunks(cols).enumerate() {
            let (row_rect, _) = ui.allocate_exact_size(vec2(w, card_h + gap), Sense::hover());
            for (col_idx, path) in chunk.iter().enumerate() {
                let i = row_idx * cols + col_idx;
                let card = Rect::from_min_size(pos2(row_rect.min.x + col_idx as f32 * (cw + gap), row_rect.min.y), vec2(cw, card_h));
                let name = crate::menus::recent_name(path);
                let resp = ui.interact(card, egui::Id::unique(("home-recent", i)), Sense::click()).on_hover_text(path.as_str());
                app.auto.add(&format!("home.recent.{i}"), card, &name);
                ui.painter().rect_filled(card, 8.0, if resp.hovered() { t.hover } else { t.tl_header_bg });
                if resp.hovered() {
                    ui.painter().rect_stroke(card, 8.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                }
                let tr = Rect::from_min_size(card.min, vec2(cw, thumb_h));
                ui.painter().rect_filled(tr, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, Color32::from_rgb(16, 18, 26));
                // Show real video thumbnail if available (live sequence frame for current project, or cached .thumb)
                let live_tid = app
                    .session
                    .path
                    .as_ref()
                    .filter(|p| p.as_str() == path.as_str())
                    .and_then(|_| app.session.state.active_sequence)
                    .and_then(|seq_id| app.thumbnail(&ctx, seq_id, app.session.playhead(), 240))
                    .map(|(tid, _)| tid);
                let tid = live_tid.or_else(|| load_recent_thumb(&ctx, path));
                if let Some(tex_id) = tid {
                    ui.painter().image(tex_id, tr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                } else {
                    // Sleek cinema preview placeholder with centered play badge
                    let badge = Rect::from_center_size(tr.center(), vec2(38.0, 38.0));
                    ui.painter().rect_filled(badge, 19.0, t.accent.gamma_multiply(0.22));
                    icons::paint(ui.painter(), badge.shrink(10.0), Icon::Play, t.accent);
                }
                // Title and parent folder below thumbnail
                let info = ui.painter().with_clip_rect(Rect::from_min_max(pos2(card.min.x + 10.0, tr.max.y + 4.0), pos2(card.max.x - 10.0, card.max.y - 4.0)));
                info.text(pos2(card.min.x + 10.0, tr.max.y + 14.0), Align2::LEFT_CENTER, &name, Tokens::semibold(13.0), t.text);
                if let Some(folder) = std::path::Path::new(path).parent() {
                    info.text(pos2(card.min.x + 10.0, tr.max.y + 31.0), Align2::LEFT_CENTER, folder.to_string_lossy(), Tokens::ui(11.0), t.text_dim);
                }
                if resp.clicked() {
                    open = Some(path.clone());
                }
            }
        }
    });
    if let Some(path) = open {
        // unsaved changes: Save / Don't Save / Cancel first; Home switches to Edit once the
        // chosen project opens, and stays after Cancel
        if crate::panels::clip_dialogs::ask_before_open(app, &path).is_some() {
            if let Some(info) = app.ui.clip_dialog.as_mut().and_then(|d| d.info.as_object_mut()) {
                info.insert("editAfterOpen".into(), json!(true));
            }
        } else if crate::menus::invoke(app, &ctx, "file.open", json!({"path": path})).is_ok() {
            app.ui.mode = crate::state::Mode::Edit;
        }
    }
}
