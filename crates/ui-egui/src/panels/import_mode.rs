//! Import mode (header "Import"): centered action cards and an integrated live folder thumbnail
//! browser where users can set a folder, preview footage with hover-scrub thumbnails, and import.

use std::collections::BTreeSet;
use std::sync::Arc;

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use filmcraft_engine::media_browser::{self as mb, Entry};
use serde_json::json;

use crate::FilmcraftApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

#[derive(Clone, Default)]
struct ImportBrowserState {
    folder: String,
    entries: Arc<Vec<Entry>>,
    loaded_folder: String,
    selected: BTreeSet<String>,
}

fn default_import_folder() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let h = std::path::PathBuf::from(home);
        for sub in ["Movies", "Videos", "Desktop", "Downloads"] {
            let candidate = h.join(sub);
            if candidate.is_dir() {
                return candidate.to_string_lossy().into_owned();
            }
        }
        return h.to_string_lossy().into_owned();
    }
    ".".into()
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    ui.painter().rect_filled(rect, t.radius, t.panel_bg);

    let pad = 24.0;
    let mut y = rect.min.y + 28.0;
    ui.painter().text(pos2(rect.min.x + pad, y), Align2::LEFT_CENTER, tl!("Import"), Tokens::semibold(22.0), t.text);
    y += 26.0;
    ui.painter().text(
        pos2(rect.min.x + pad, y),
        Align2::LEFT_CENTER,
        tl!("Select footage from your media folder below, drop files anywhere, or choose a quick action."),
        Tokens::ui(13.0),
        t.text_dim,
    );
    y += 24.0;

    // ---- centered-icon quick action cards
    let cards = [
        (tl!("Browse Files…"), "file.import", tl!("Pick files from disk"), Icon::Folder),
        (tl!("Set Folder…"), "import.setFolder", tl!("Choose media folder below"), Icon::Import),
        (tl!("New Sequence"), "file.newSequence", tl!("Create an empty timeline"), Icon::Sequence),
        (tl!("Color Matte"), "file.newColorMatte", tl!("Solid background clip"), Icon::Film),
    ];
    let gap = 14.0;
    let avail_w = (rect.width() - pad * 2.0).max(240.0);
    let cw = ((avail_w - gap * (cards.len() as f32 - 1.0)) / cards.len() as f32).clamp(140.0, 240.0);
    let ch = 124.0;

    let state_id = egui::Id::unique("import-mode-browser");
    let mut st: ImportBrowserState = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
    if st.folder.is_empty() {
        st.folder = default_import_folder();
    }

    let mut pick_folder = false;
    for (i, (title, cmd, sub, icon)) in cards.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.min.x + pad + i as f32 * (cw + gap), y), vec2(cw, ch));
        let resp = ui.interact(r, egui::Id::unique(("imp", *cmd)), Sense::click());
        app.auto.add(&format!("import.{cmd}"), r, title);
        ui.painter().rect_filled(r, 10.0, if resp.hovered() { t.hover } else { t.tl_header_bg });
        if resp.hovered() {
            ui.painter().rect_stroke(r, 10.0, Stroke::new(1.2, t.accent), StrokeKind::Inside);
        }
        // Centered icon inside a soft accent pill
        let icon_box = Rect::from_center_size(pos2(r.center().x, r.min.y + 40.0), vec2(46.0, 46.0));
        ui.painter().rect_filled(icon_box, 12.0, t.accent.gamma_multiply(0.18));
        icons::paint(ui.painter(), icon_box.shrink(11.0), *icon, t.accent);
        // Centered title & subtitle
        ui.painter().text(pos2(r.center().x, r.min.y + 82.0), Align2::CENTER_CENTER, *title, Tokens::semibold(13.5), t.text);
        ui.painter().text(pos2(r.center().x, r.min.y + 102.0), Align2::CENTER_CENTER, *sub, Tokens::ui(11.0), t.text_dim);
        if resp.clicked() {
            if *cmd == "import.setFolder" {
                pick_folder = true;
            } else {
                let _ = crate::menus::invoke(app, &ctx, cmd, json!({}));
                app.ui.mode = crate::state::Mode::Edit;
            }
        }
    }
    y += ch + 22.0;

    // ---- folder browser header bar
    let bar = Rect::from_min_size(pos2(rect.min.x + pad, y), vec2(avail_w, 32.0));
    ui.painter().rect_filled(bar, 6.0, t.tl_header_bg);
    let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    bar_ui.label(egui::RichText::new(tl!("Media Folder:")).size(12.0).strong().color(t.text));
    bar_ui.label(egui::RichText::new(&st.folder).size(12.0).color(t.text_dim));
    bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let sel_count = st.selected.len();
        let import_label = if sel_count > 0 { format!("Import Selected ({sel_count})") } else { tl!("Import Selected").to_string() };
        let imp_btn = ui.add_enabled(sel_count > 0, egui::Button::new(egui::RichText::new(import_label).size(12.0).color(Color32::WHITE)).fill(t.accent));
        app.auto.add("import.importSelected", imp_btn.rect, "Import Selected");
        if imp_btn.clicked() {
            let paths: Vec<String> = st.selected.iter().cloned().collect();
            if crate::menus::invoke(app, &ctx, "file.import", json!({"paths": paths})).is_ok() {
                st.selected.clear();
                app.ui.mode = crate::state::Mode::Edit;
            }
        }
        let set_btn = ui.button(egui::RichText::new(tl!("Set Folder…")).size(12.0));
        app.auto.add("import.setFolderBtn", set_btn.rect, "Set Folder");
        if set_btn.clicked() {
            pick_folder = true;
        }
        if ui.button(egui::RichText::new(tl!("Refresh")).size(12.0)).clicked() {
            st.loaded_folder.clear();
        }
    });

    if pick_folder {
        let ctx_cb = ctx.clone();
        let current = Some(st.folder.clone());
        app.pick_ui(crate::FileDialog::Folder { at: current }, move |_app, paths| {
            if let Some(dir) = paths.into_iter().next() {
                ctx_cb.data_mut(|d| {
                    let s = d.get_temp_mut_or_default::<ImportBrowserState>(state_id);
                    s.folder = dir;
                    s.loaded_folder.clear();
                    s.selected.clear();
                });
                ctx_cb.request_repaint();
            }
        });
    }

    // Load media files from selected folder automatically
    if st.loaded_folder != st.folder {
        st.loaded_folder = st.folder.clone();
        match mb::list(&*app.session.services, &st.folder, "all") {
            Ok(list) => {
                // Keep media files first (videos, images, audio), then folders
                let filtered: Vec<Entry> = list.into_iter().filter(|e| e.is_dir || matches!(e.kind, "video" | "image" | "audio")).collect();
                st.entries = Arc::new(filtered);
            }
            Err(_) => {
                st.entries = Arc::new(Vec::new());
            }
        }
    }

    y += 42.0;

    // ---- live folder thumbnail grid
    let grid_h = (rect.max.y - y - 16.0).max(120.0);
    let grid_rect = Rect::from_min_size(pos2(rect.min.x + pad, y), vec2(avail_w, grid_h));
    let entries = st.entries.clone();
    if entries.is_empty() {
        ui.painter().text(
            grid_rect.center(),
            Align2::CENTER_CENTER,
            tl!("No media files found in this folder. Click \"Set Folder…\" to choose another folder."),
            Tokens::ui(13.0),
            t.text_dim,
        );
    } else {
        let thumb_w = 188.0_f32;
        let thumb_h = thumb_w * 9.0 / 16.0;
        let cell = vec2(thumb_w + 16.0, thumb_h + 36.0);
        let per_row = (avail_w / cell.x).floor().max(1.0) as usize;
        let mut navigate_to: Option<String> = None;
        let mut import_now: Option<String> = None;

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid_rect).id_salt("import-thumb-grid"));
        child.set_clip_rect(grid_rect.intersect(ui.clip_rect()));
        egui::ScrollArea::vertical().id_salt("import-thumb-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            for chunk in entries.chunks(per_row) {
                let (row, _) = ui.allocate_exact_size(vec2(avail_w, cell.y), Sense::hover());
                for (k, e) in chunk.iter().enumerate() {
                    let tr = Rect::from_min_size(pos2(row.min.x + k as f32 * cell.x, row.min.y + 4.0), vec2(thumb_w, thumb_h));
                    let card_r = Rect::from_min_max(tr.min - vec2(4.0, 4.0), pos2(tr.max.x + 4.0, tr.max.y + 26.0));
                    let resp = ui.interact(card_r, egui::Id::unique(("imp-file", &e.path)), Sense::click());
                    let is_sel = st.selected.contains(&e.path);
                    ui.painter().rect_filled(
                        card_r,
                        6.0,
                        if is_sel {
                            t.row_selected
                        } else if resp.hovered() {
                            t.hover
                        } else {
                            t.tl_header_bg
                        },
                    );
                    if is_sel {
                        ui.painter().rect_stroke(card_r, 6.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
                    }
                    ui.painter().rect_filled(tr, 4.0, Color32::from_gray(14));
                    if e.is_dir {
                        icons::paint(ui.painter(), Rect::from_center_size(tr.center(), vec2(42.0, 42.0)), Icon::Folder, Color32::from_rgb(237, 150, 58));
                    } else {
                        let visible = ui.is_rect_visible(tr);
                        if visible && matches!(e.kind, "video" | "image") && mb::probe_background(&mut app.session, &e.path).is_none() {
                            ctx.request_repaint_after(std::time::Duration::from_millis(100));
                        }
                        let hovered = resp.hovered() || ui.rect_contains_pointer(tr);
                        let frac = if hovered {
                            ctx.pointer_hover_pos().map(|p| ((p.x - tr.min.x) / tr.width().max(1.0)).clamp(0.0, 1.0)).unwrap_or(0.3)
                        } else {
                            0.3
                        };
                        let drawn = visible && crate::panels::media_browser::draw_preview(app, ui, &ctx, e, tr, frac);
                        if !drawn {
                            let icon = match e.kind {
                                "audio" => Icon::Audio,
                                "image" => Icon::Image,
                                _ => Icon::Film,
                            };
                            icons::paint(ui.painter(), Rect::from_center_size(tr.center(), vec2(30.0, 30.0)), icon, t.text_faint);
                        }
                        if hovered && e.kind == "video" {
                            let x = tr.min.x + tr.width() * frac;
                            ui.painter().line_segment([pos2(x, tr.min.y), pos2(x, tr.max.y)], Stroke::new(1.5, Color32::from_rgb(0x5a, 0x9b, 0xf0)));
                        }
                        if is_sel {
                            let chk = Rect::from_min_size(pos2(tr.max.x - 22.0, tr.min.y + 4.0), vec2(18.0, 18.0));
                            ui.painter().rect_filled(chk, 9.0, t.accent);
                            ui.painter().text(chk.center(), Align2::CENTER_CENTER, "✓", Tokens::semibold(11.0), Color32::WHITE);
                        }
                    }
                    let label_clip = ui.painter().with_clip_rect(Rect::from_min_max(pos2(tr.min.x, tr.max.y + 2.0), pos2(tr.max.x, tr.max.y + 24.0)));
                    label_clip.text(pos2(tr.min.x + 2.0, tr.max.y + 13.0), Align2::LEFT_CENTER, &e.name, Tokens::ui(11.5), t.text);
                    if resp.double_clicked() {
                        if e.is_dir {
                            navigate_to = Some(e.path.clone());
                        } else {
                            import_now = Some(e.path.clone());
                        }
                    } else if resp.clicked() {
                        if e.is_dir {
                            navigate_to = Some(e.path.clone());
                        } else if !st.selected.remove(&e.path) {
                            st.selected.insert(e.path.clone());
                        }
                    }
                }
            }
        });
        if let Some(dir) = navigate_to {
            st.folder = dir;
            st.loaded_folder.clear();
            st.selected.clear();
        }
        if let Some(file) = import_now {
            if crate::menus::invoke(app, &ctx, "file.import", json!({"paths": [file]})).is_ok() {
                app.ui.mode = crate::state::Mode::Edit;
            }
        }
    }

    ctx.data_mut(|d| d.insert_temp(state_id, st));
}
