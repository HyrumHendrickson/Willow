//! The editor UI (egui).

use crate::export;
use crate::model::*;
use crate::server::Server;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Key, KeyboardShortcut,
    Modifiers, Rect, Sense, Stroke, StrokeKind, Vec2,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(46, 125, 246);
const IMAGE_EXT: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
const VIDEO_EXT: [&str; 4] = ["mp4", "webm", "ogv", "mov"];

#[derive(Clone, Copy, PartialEq)]
enum Sel {
    None,
    Region(RegionId),
    Element(RegionId, u64),
}

#[derive(Clone, Copy)]
enum ElemAction {
    Delete,
    Duplicate,
    Forward,
    Backward,
    Front,
    Back,
}

#[derive(Clone, Copy, PartialEq)]
enum Cmd {
    New,
    Open,
    Save,
    Preview,
    Undo,
    Redo,
}

pub struct App {
    project: Option<Project>,
    page: usize,
    active: RegionId,
    sel: Sel,
    editing: Option<u64>,
    focus_edit: bool,
    zoom: f32,
    fit_zoom: bool,
    status: String,
    error: Option<String>,
    cmd: Option<Cmd>,

    // change tracking / undo
    saved_json: String,
    last_json: String,
    pending_before: Option<String>,
    last_change: Instant,
    undo: Vec<String>,
    redo: Vec<String>,

    // helpers
    server: Option<Server>,
    serif_ok: bool,
    dims: HashMap<PathBuf, Option<(u32, u32)>>,
    media: Vec<String>,
    media_stale: bool,
    new_page_title: String,
    rename_buf: String,
    rename_for: usize,
    closing_ok: bool,
    last_title: String,
}

fn c32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0[0], c.0[1], c.0[2])
}

fn contrast(bg: Rgb) -> Rgb {
    let l = 0.299 * bg.0[0] as f32 + 0.587 * bg.0[1] as f32 + 0.114 * bg.0[2] as f32;
    if l < 140.0 {
        Rgb::WHITE
    } else {
        Rgb::INK
    }
}

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

fn is_image(name: &str) -> bool {
    IMAGE_EXT.contains(&ext_of(Path::new(name)).as_str())
}

fn is_video(name: &str) -> bool {
    VIDEO_EXT.contains(&ext_of(Path::new(name)).as_str())
}

fn install_fonts(ctx: &egui::Context) -> bool {
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSerif.ttf",
        "/usr/share/fonts/dejavu/DejaVuSerif.ttf",
        "/System/Library/Fonts/Supplemental/Georgia.ttf",
        "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
        "/Library/Fonts/Georgia.ttf",
        "C:\\Windows\\Fonts\\georgia.ttf",
        "C:\\Windows\\Fonts\\times.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("serif".into(), Arc::new(egui::FontData::from_owned(bytes)));
            let mut list = vec!["serif".to_string()];
            list.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
            fonts.families.insert(FontFamily::Name("serif".into()), list);
            ctx.set_fonts(fonts);
            return true;
        }
    }
    false
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let serif_ok = install_fonts(&cc.egui_ctx);
        App {
            project: None,
            page: 0,
            active: RegionId::Body,
            sel: Sel::None,
            editing: None,
            focus_edit: false,
            zoom: 1.0,
            fit_zoom: true,
            status: String::new(),
            error: None,
            cmd: None,
            saved_json: String::new(),
            last_json: String::new(),
            pending_before: None,
            last_change: Instant::now(),
            undo: Vec::new(),
            redo: Vec::new(),
            server: Server::start().ok(),
            serif_ok,
            dims: HashMap::new(),
            media: Vec::new(),
            media_stale: true,
            new_page_title: String::new(),
            rename_buf: String::new(),
            rename_for: usize::MAX,
            closing_ok: false,
            last_title: String::new(),
        }
    }

    fn family(&self, f: FontChoice) -> FontFamily {
        match f {
            FontChoice::Sans => FontFamily::Proportional,
            FontChoice::Mono => FontFamily::Monospace,
            FontChoice::Serif if self.serif_ok => FontFamily::Name("serif".into()),
            FontChoice::Serif => FontFamily::Proportional,
        }
    }

    fn activate(&mut self, p: Project) {
        if let Some(s) = &self.server {
            s.set_root(&p.root);
        }
        let snap = p.snapshot();
        self.saved_json = snap.clone();
        self.last_json = snap;
        self.pending_before = None;
        self.undo.clear();
        self.redo.clear();
        self.page = 0;
        self.active = RegionId::Body;
        self.sel = Sel::None;
        self.editing = None;
        self.rename_for = usize::MAX;
        self.dims.clear();
        self.media_stale = true;
        self.project = Some(p);
    }

    fn is_dirty(&self, p: &Project) -> bool {
        p.snapshot() != self.saved_json
    }

    // ------------------------------------------------------------ commands

    /// Returns false if the user cancelled.
    fn confirm_unsaved(&mut self, p: &mut Project) -> bool {
        if !self.is_dirty(p) {
            return true;
        }
        match rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("Save changes before continuing?")
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show()
        {
            rfd::MessageDialogResult::Yes => self.save(p),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn save(&mut self, p: &mut Project) -> bool {
        match export::save(p) {
            Ok(()) => {
                self.saved_json = p.snapshot();
                self.status = format!("Saved to {}", p.root.display());
                true
            }
            Err(e) => {
                self.error = Some(format!("Save failed: {e}"));
                false
            }
        }
    }

    fn run_cmd(&mut self, cmd: Cmd, proj: &mut Option<Project>) {
        match cmd {
            Cmd::Save => {
                if let Some(p) = proj.as_mut() {
                    self.save(p);
                }
            }
            Cmd::Preview => {
                if let Some(p) = proj.as_mut() {
                    if self.save(p) {
                        if let Some(s) = &self.server {
                            s.set_root(&p.root);
                            let url = format!("http://127.0.0.1:{}/", s.port);
                            if let Err(e) = open::that(&url) {
                                self.status = format!("Could not open browser ({e}). Visit {url}");
                            } else {
                                self.status = format!("Preview running at {url}");
                            }
                        } else {
                            self.error = Some("The preview server could not be started.".into());
                        }
                    }
                }
            }
            Cmd::Undo => {
                if let Some(p) = proj.as_mut() {
                    self.commit_pending();
                    if let Some(prev) = self.undo.pop() {
                        self.redo.push(p.snapshot());
                        p.restore(&prev);
                        self.after_restore(p, prev);
                    }
                }
            }
            Cmd::Redo => {
                if let Some(p) = proj.as_mut() {
                    self.commit_pending();
                    if let Some(next) = self.redo.pop() {
                        self.undo.push(p.snapshot());
                        p.restore(&next);
                        self.after_restore(p, next);
                    }
                }
            }
            Cmd::New => {
                if let Some(p) = proj.as_mut() {
                    if !self.confirm_unsaved(p) {
                        return;
                    }
                }
                let Some(dir) = rfd::FileDialog::new().set_title("Choose an empty folder for the new site").pick_folder()
                else {
                    return;
                };
                if dir.join("config/site.json").exists() {
                    self.error = Some("That folder already contains a site. Use File > Open instead.".into());
                    return;
                }
                let ignorable = [".git", ".gitignore", "README.md", "readme.md", "LICENSE", "CNAME", ".nojekyll"];
                let has_other = fs::read_dir(&dir)
                    .map(|rd| {
                        rd.filter_map(|e| e.ok())
                            .any(|e| !ignorable.contains(&e.file_name().to_string_lossy().as_ref()))
                    })
                    .unwrap_or(false);
                if has_other {
                    let ok = rfd::MessageDialog::new()
                        .set_title("Folder is not empty")
                        .set_description(
                            "This folder already contains files. Existing index.html, index.css and index.js will be overwritten. Continue?",
                        )
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show();
                    if ok != rfd::MessageDialogResult::Yes {
                        return;
                    }
                }
                let mut np = Project::scaffold(&dir);
                match export::save(&mut np) {
                    Ok(()) => {
                        self.status = format!("Created new site in {}", dir.display());
                        self.activate(np);
                        *proj = self.project.take();
                    }
                    Err(e) => self.error = Some(format!("Could not create site: {e}")),
                }
            }
            Cmd::Open => {
                if let Some(p) = proj.as_mut() {
                    if !self.confirm_unsaved(p) {
                        return;
                    }
                }
                let Some(dir) = rfd::FileDialog::new().set_title("Open a site folder").pick_folder() else {
                    return;
                };
                match Project::load(&dir) {
                    Ok(np) => {
                        self.status = format!("Opened {}", dir.display());
                        self.activate(np);
                        *proj = self.project.take();
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    fn after_restore(&mut self, p: &Project, snap: String) {
        self.last_json = snap;
        self.pending_before = None;
        self.page = self.page.min(p.site.pages.len().saturating_sub(1));
        self.sel = Sel::None;
        self.editing = None;
        self.rename_for = usize::MAX;
    }

    fn commit_pending(&mut self) {
        if let Some(before) = self.pending_before.take() {
            self.undo.push(before);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn track_changes(&mut self, ctx: &egui::Context, p: &Project) {
        let cur = p.snapshot();
        if cur != self.last_json {
            if self.pending_before.is_none() {
                self.pending_before = Some(self.last_json.clone());
            }
            self.last_json = cur;
            self.last_change = Instant::now();
        }
        if self.pending_before.is_some() {
            let down = ctx.input(|i| i.pointer.any_down());
            if !down && self.last_change.elapsed() > Duration::from_millis(500) {
                self.commit_pending();
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    // ------------------------------------------------------------ media

    fn import_media(&mut self, root: &Path, src: &Path) -> Result<String, String> {
        let name = src.file_name().and_then(|n| n.to_str()).ok_or("Invalid file name")?;
        let clean: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '-' })
            .collect();
        let dir = root.join("media");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let (stem, ext) = match clean.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (clean.clone(), String::new()),
        };
        let src_len = fs::metadata(src).map(|m| m.len()).map_err(|e| e.to_string())?;
        let mut candidate = clean.clone();
        let mut n = 1;
        loop {
            let target = dir.join(&candidate);
            match fs::metadata(&target) {
                Err(_) => {
                    fs::copy(src, &target).map_err(|e| format!("Copy failed: {e}"))?;
                    break;
                }
                Ok(m) if m.len() == src_len => break, // same file already imported
                Ok(_) => {
                    candidate = format!("{stem}-{n}{ext}");
                    n += 1;
                }
            }
        }
        self.dims.remove(&dir.join(&candidate));
        self.media_stale = true;
        Ok(format!("media/{candidate}"))
    }

    fn image_dims(&mut self, path: &Path) -> Option<(u32, u32)> {
        *self
            .dims
            .entry(path.to_path_buf())
            .or_insert_with(|| image::image_dimensions(path).ok())
    }

    // ------------------------------------------------------------ adding elements

    fn add_element(&mut self, p: &mut Project, kind: Kind, w: f32, h: f32) {
        let id = p.next_id();
        let page = self.page;
        let rid = self.active;
        let width = p.site.width;
        let region = p.region_mut(rid, page);
        let off = (region.elements.len() % 8) as f32 * 20.0;
        let x = (40.0 + off).min(width - w).max(0.0);
        let y = (40.0 + off).min(region.height - h).max(0.0);
        region.elements.push(Element { id, x, y, w, h, link: String::new(), kind });
        self.sel = Sel::Element(rid, id);
        self.editing = None;
    }

    fn add_text(&mut self, p: &mut Project) {
        let bg = p.region(self.active, self.page).background;
        let props = TextProps { color: contrast(bg), ..TextProps::default() };
        self.add_element(p, Kind::Text(props), 300.0, 50.0);
    }

    fn add_media_from_path(&mut self, p: &mut Project, path: &Path) {
        let ext = ext_of(path);
        let root = p.root.clone();
        let rel = match self.import_media(&root, path) {
            Ok(r) => r,
            Err(e) => {
                self.status = format!("Import failed: {e}");
                return;
            }
        };
        if IMAGE_EXT.contains(&ext.as_str()) {
            let (w, h) = match self.image_dims(&root.join(&rel)) {
                Some((iw, ih)) if iw > 0 => {
                    let w = (iw as f32).min(400.0);
                    (w, (w * ih as f32 / iw as f32).max(20.0))
                }
                _ => (300.0, 200.0),
            };
            self.add_element(p, Kind::Image { src: rel, alt: String::new(), fit: Fit::Cover, radius: 0.0 }, w, h);
        } else if VIDEO_EXT.contains(&ext.as_str()) {
            self.add_element(p, Kind::Video { src: rel }, 480.0, 270.0);
        } else {
            self.status = format!("Unsupported file type: .{ext}");
        }
    }

    fn apply_action(&mut self, p: &mut Project, rid: RegionId, id: u64, act: ElemAction) {
        let page = self.page;
        let Some(idx) = p.region(rid, page).elements.iter().position(|e| e.id == id) else {
            return;
        };
        match act {
            ElemAction::Delete => {
                p.region_mut(rid, page).elements.remove(idx);
                self.sel = Sel::None;
                self.editing = None;
            }
            ElemAction::Duplicate => {
                let mut copy = p.region(rid, page).elements[idx].clone();
                copy.id = p.next_id();
                copy.x += 20.0;
                copy.y += 20.0;
                let new_id = copy.id;
                p.region_mut(rid, page).elements.insert(idx + 1, copy);
                self.sel = Sel::Element(rid, new_id);
            }
            ElemAction::Forward => {
                let els = &mut p.region_mut(rid, page).elements;
                if idx + 1 < els.len() {
                    els.swap(idx, idx + 1);
                }
            }
            ElemAction::Backward => {
                let els = &mut p.region_mut(rid, page).elements;
                if idx > 0 {
                    els.swap(idx, idx - 1);
                }
            }
            ElemAction::Front => {
                let els = &mut p.region_mut(rid, page).elements;
                let e = els.remove(idx);
                els.push(e);
            }
            ElemAction::Back => {
                let els = &mut p.region_mut(rid, page).elements;
                let e = els.remove(idx);
                els.insert(0, e);
            }
        }
    }

    // ------------------------------------------------------------ drawing

    fn layout_text(&self, ctx: &egui::Context, t: &TextProps, z: f32, wrap: f32) -> Arc<egui::Galley> {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wrap;
        job.halign = match t.align {
            Align::Left => egui::Align::LEFT,
            Align::Center => egui::Align::Center,
            Align::Right => egui::Align::RIGHT,
        };
        job.append(
            &t.text,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(t.size * z, self.family(t.font)),
                color: c32(t.color),
                italics: t.italic,
                line_height: Some(t.size * t.line_height * z),
                ..Default::default()
            },
        );
        ctx.fonts(|f| f.layout_job(job))
    }

    fn draw_element(&mut self, ui: &egui::Ui, e: &Element, rect: Rect, z: f32, root: &Path, pages: &[PageMeta]) {
        let painter = ui.painter();
        match &e.kind {
            Kind::Text(t) => {
                let galley = self.layout_text(ui.ctx(), t, z, rect.width());
                let ax = match t.align {
                    Align::Left => rect.min.x,
                    Align::Center => rect.center().x,
                    Align::Right => rect.max.x,
                };
                let pos = pos2(ax, rect.min.y);
                painter.galley(pos, galley.clone(), c32(t.color));
                if t.bold {
                    let d = (t.size * z * 0.03).max(0.5);
                    painter.galley(pos + vec2(d, 0.0), galley, c32(t.color));
                }
            }
            Kind::Shape { color, radius } => {
                let r = (radius * z).round().clamp(0.0, 255.0) as u8;
                painter.rect_filled(rect, CornerRadius::same(r), c32(*color));
            }
            Kind::Image { src, fit, radius, .. } => {
                let path = root.join(src);
                match self.image_dims(&path) {
                    None => {
                        painter.rect_filled(rect, 0.0, Color32::from_gray(225));
                        painter.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            format!("Missing image\n{src}"),
                            FontId::proportional(13.0),
                            Color32::from_gray(90),
                        );
                    }
                    Some((iw, ih)) => {
                        let img_aspect = iw as f32 / ih.max(1) as f32;
                        let box_aspect = rect.width() / rect.height().max(1.0);
                        let full_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        let (dest, uv) = match fit {
                            Fit::Cover => {
                                let uv = if img_aspect > box_aspect {
                                    let f = box_aspect / img_aspect;
                                    Rect::from_min_max(pos2((1.0 - f) / 2.0, 0.0), pos2(1.0 - (1.0 - f) / 2.0, 1.0))
                                } else {
                                    let f = img_aspect / box_aspect;
                                    Rect::from_min_max(pos2(0.0, (1.0 - f) / 2.0), pos2(1.0, 1.0 - (1.0 - f) / 2.0))
                                };
                                (rect, uv)
                            }
                            Fit::Contain => {
                                let size = if img_aspect > box_aspect {
                                    vec2(rect.width(), rect.width() / img_aspect)
                                } else {
                                    vec2(rect.height() * img_aspect, rect.height())
                                };
                                (Rect::from_center_size(rect.center(), size), full_uv)
                            }
                        };
                        let uri = format!("file://{}", path.to_string_lossy().replace('\\', "/"));
                        let r = (radius * z).round().clamp(0.0, 255.0) as u8;
                        egui::Image::from_uri(uri)
                            .maintain_aspect_ratio(false)
                            .fit_to_exact_size(dest.size())
                            .uv(uv)
                            .corner_radius(CornerRadius::same(r))
                            .paint_at(ui, dest);
                    }
                }
            }
            Kind::Video { src } => {
                painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 24));
                let c = rect.center();
                let s = (rect.height().min(rect.width()) * 0.18).clamp(8.0, 40.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![pos2(c.x - s * 0.6, c.y - s), pos2(c.x - s * 0.6, c.y + s), pos2(c.x + s, c.y)],
                    Color32::WHITE,
                    Stroke::NONE,
                ));
                painter.text(
                    rect.left_bottom() + vec2(8.0, -6.0),
                    Align2::LEFT_BOTTOM,
                    format!("VIDEO  {src}"),
                    FontId::proportional(12.0),
                    Color32::from_gray(190),
                );
            }
            Kind::Nav(n) => {
                let font = FontId::new(n.size * z, self.family(n.font));
                let color = c32(n.color);
                let galleys: Vec<_> = pages
                    .iter()
                    .map(|p| painter.layout_no_wrap(p.title.clone(), font.clone(), color))
                    .collect();
                let gap = n.gap * z;
                let total: f32 =
                    galleys.iter().map(|g| g.size().x).sum::<f32>() + gap * galleys.len().saturating_sub(1) as f32;
                let mut x = match n.align {
                    Align::Left => rect.min.x,
                    Align::Center => rect.center().x - total / 2.0,
                    Align::Right => rect.max.x - total,
                };
                for g in galleys {
                    let w = g.size().x;
                    let pos = pos2(x, rect.center().y - g.size().y / 2.0);
                    painter.galley(pos, g.clone(), color);
                    if n.bold {
                        painter.galley(pos + vec2((n.size * z * 0.03).max(0.5), 0.0), g, color);
                    }
                    x += w + gap;
                }
            }
        }
    }

    fn canvas_ui(&mut self, ui: &mut egui::Ui, p: &mut Project) {
        let width = p.site.width;
        let pad = 28.0;
        if self.fit_zoom {
            self.zoom = ((ui.available_width() - 2.0 * pad - 18.0) / width).clamp(0.25, 2.0);
        }
        let z = self.zoom;
        let page = self.page;
        let root = p.root.clone();
        let pages = p.site.pages.clone();
        let order = [RegionId::Header, RegionId::Body, RegionId::Footer];
        let heights = [p.header.height, p.contents[page].height, p.footer.height];
        let total: f32 = heights.iter().sum();
        let mut actions: Vec<(RegionId, u64, ElemAction)> = Vec::new();

        egui::ScrollArea::both().auto_shrink(false).drag_to_scroll(false).show(ui, |ui| {
            let size = vec2((width * z + 2.0 * pad).max(ui.available_width()), total * z + 2.0 * pad);
            let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
            let cx0 = outer.min.x + ((outer.width() - width * z) / 2.0).max(pad);
            let mut y = outer.min.y + pad;

            for (ri, rid) in order.iter().copied().enumerate() {
                let h = heights[ri] * z;
                let band = Rect::from_min_size(pos2(outer.min.x, y), vec2(outer.width(), h));
                let origin = pos2(cx0, y);
                let region_h = heights[ri];
                let region = p.region_mut(rid, page);

                ui.painter().rect_filled(band, 0.0, c32(region.background));
                let band_resp = ui.interact(band, egui::Id::new(("band", ri)), Sense::click());
                if band_resp.clicked() {
                    self.sel = Sel::Region(rid);
                    self.active = rid;
                    self.editing = None;
                }

                let old_clip = ui.clip_rect();
                ui.set_clip_rect(band.intersect(old_clip));

                for i in 0..region.elements.len() {
                    let el = &mut region.elements[i];
                    let rect = Rect::from_min_size(origin + vec2(el.x, el.y) * z, vec2(el.w, el.h) * z);
                    let editing_this = self.editing == Some(el.id);
                    let selected = self.sel == Sel::Element(rid, el.id);
                    let eid = egui::Id::new(("el", el.id));

                    if editing_this {
                        let fam = self.family(match &el.kind {
                            Kind::Text(t) => t.font,
                            _ => FontChoice::Sans,
                        });
                        if let Kind::Text(t) = &mut el.kind {
                            let te = egui::TextEdit::multiline(&mut t.text)
                                .font(FontId::new(t.size * z, fam))
                                .text_color(c32(t.color))
                                .horizontal_align(match t.align {
                                    Align::Left => egui::Align::LEFT,
                                    Align::Center => egui::Align::Center,
                                    Align::Right => egui::Align::RIGHT,
                                })
                                .frame(false)
                                .margin(egui::Margin::ZERO)
                                .desired_rows(1)
                                .desired_width(rect.width());
                            let r = ui.put(rect, te);
                            if self.focus_edit {
                                r.request_focus();
                                self.focus_edit = false;
                            }
                            if r.lost_focus() || ui.input(|i| i.key_pressed(Key::Escape)) {
                                self.editing = None;
                            }
                        }
                        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
                        continue;
                    }

                    let snapshot = el.clone();
                    self.draw_element(ui, &snapshot, rect, z, &root, &pages);

                    let resp = ui.interact(rect, eid, Sense::click_and_drag());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::Grab);
                    }
                    if resp.clicked() || resp.drag_started() {
                        self.sel = Sel::Element(rid, el.id);
                        self.active = rid;
                    }
                    if resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                        let d = resp.drag_delta() / z;
                        el.x = (el.x + d.x).clamp(0.0, (width - el.w).max(0.0));
                        el.y = (el.y + d.y).clamp(0.0, (region_h - el.h).max(0.0));
                    }
                    if resp.drag_stopped() {
                        el.x = el.x.round();
                        el.y = el.y.round();
                    }
                    if resp.double_clicked() && matches!(el.kind, Kind::Text(_)) {
                        self.editing = Some(el.id);
                        self.focus_edit = true;
                        self.sel = Sel::Element(rid, el.id);
                    }
                    let id = el.id;
                    resp.context_menu(|ui| {
                        if ui.button("Duplicate").clicked() {
                            actions.push((rid, id, ElemAction::Duplicate));
                            ui.close_menu();
                        }
                        if ui.button("Bring to front").clicked() {
                            actions.push((rid, id, ElemAction::Front));
                            ui.close_menu();
                        }
                        if ui.button("Send to back").clicked() {
                            actions.push((rid, id, ElemAction::Back));
                            ui.close_menu();
                        }
                        if ui.button("Delete").clicked() {
                            actions.push((rid, id, ElemAction::Delete));
                            ui.close_menu();
                        }
                    });

                    if selected {
                        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
                        let hr = Rect::from_center_size(rect.right_bottom(), Vec2::splat(14.0));
                        let hresp = ui.interact(hr, eid.with("resize"), Sense::drag());
                        if hresp.hovered() || hresp.dragged() {
                            ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
                        }
                        if hresp.dragged() {
                            let d = hresp.drag_delta() / z;
                            el.w = (el.w + d.x).clamp(10.0, (width - el.x).max(10.0));
                            el.h = (el.h + d.y).max(10.0);
                        }
                        if hresp.drag_stopped() {
                            el.w = el.w.round();
                            el.h = el.h.round();
                        }
                        ui.painter().rect_filled(Rect::from_center_size(rect.right_bottom(), Vec2::splat(9.0)), 2.0, ACCENT);
                    } else if resp.hovered() {
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(1.0, ACCENT.gamma_multiply(0.5)),
                            StrokeKind::Outside,
                        );
                    }
                }
                ui.set_clip_rect(old_clip);

                // Boundary line and active-region marker.
                ui.painter().line_segment(
                    [band.left_bottom(), band.right_bottom()],
                    Stroke::new(1.0, Color32::from_gray(150).gamma_multiply(0.6)),
                );
                if self.active == rid {
                    ui.painter().rect_filled(Rect::from_min_size(band.min, vec2(4.0, h)), 0.0, ACCENT);
                }
                y += h;
            }
        });

        for (rid, id, act) in actions {
            self.apply_action(p, rid, id, act);
        }
    }

    // ------------------------------------------------------------ panels

    fn menu_bar(&mut self, ui: &mut egui::Ui, has_project: bool) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New site...").clicked() {
                    self.cmd = Some(Cmd::New);
                    ui.close_menu();
                }
                if ui.button("Open site folder...").clicked() {
                    self.cmd = Some(Cmd::Open);
                    ui.close_menu();
                }
                ui.add_enabled_ui(has_project, |ui| {
                    if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                        self.cmd = Some(Cmd::Save);
                        ui.close_menu();
                    }
                    if ui.button("Save and preview in browser").clicked() {
                        self.cmd = Some(Cmd::Preview);
                        ui.close_menu();
                    }
                });
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.add_enabled_ui(has_project, |ui| {
                ui.menu_button("Edit", |ui| {
                    if ui.add(egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                        self.cmd = Some(Cmd::Undo);
                        ui.close_menu();
                    }
                    if ui.add(egui::Button::new("Redo").shortcut_text("Ctrl+Shift+Z")).clicked() {
                        self.cmd = Some(Cmd::Redo);
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, p: &mut Project) {
        ui.horizontal(|ui| {
            let region_name = match self.active {
                RegionId::Header => "Header".to_string(),
                RegionId::Footer => "Footer".to_string(),
                RegionId::Body => format!("Page: {}", p.site.pages[self.page].title),
            };
            ui.label(format!("Add to {region_name}:"));
            if ui.button("Text").clicked() {
                self.add_text(p);
            }
            if ui.button("Image...").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Images", &IMAGE_EXT).pick_file() {
                    self.add_media_from_path(p, &path);
                }
            }
            if ui.button("Video...").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Video", &VIDEO_EXT).pick_file() {
                    self.add_media_from_path(p, &path);
                }
            }
            if ui.button("Shape").clicked() {
                let bg = p.region(self.active, self.page).background;
                let color = if contrast(bg) == Rgb::WHITE { Rgb([70, 90, 120]) } else { Rgb([225, 232, 242]) };
                self.add_element(p, Kind::Shape { color, radius: 8.0 }, 300.0, 150.0);
            }
            if ui.button("Navigation").clicked() {
                let bg = p.region(self.active, self.page).background;
                let n = NavProps { color: contrast(bg), ..NavProps::default() };
                self.add_element(p, Kind::Nav(n), 400.0, 30.0);
            }
            ui.separator();
            ui.label("Zoom");
            let mut pct = self.zoom * 100.0;
            if ui.add(egui::Slider::new(&mut pct, 25.0..=200.0).suffix("%").fixed_decimals(0)).changed() {
                self.zoom = pct / 100.0;
                self.fit_zoom = false;
            }
            if ui.selectable_label(self.fit_zoom, "Fit").clicked() {
                self.fit_zoom = !self.fit_zoom;
            }
        });
    }

    fn pages_panel(&mut self, ui: &mut egui::Ui, p: &mut Project) {
        ui.heading("Pages");
        ui.add_space(4.0);
        for i in 0..p.site.pages.len() {
            let title = p.site.pages[i].title.clone();
            if ui.selectable_label(self.page == i, title).clicked() && self.page != i {
                self.page = i;
                self.sel = Sel::None;
                self.editing = None;
                self.active = RegionId::Body;
            }
        }
        ui.separator();
        ui.label("New page");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_page_title).desired_width(130.0).hint_text("Title"));
            if ui.button("Add").clicked() {
                match p.add_page(&self.new_page_title) {
                    Ok(idx) => {
                        self.new_page_title.clear();
                        self.page = idx;
                        self.sel = Sel::None;
                        self.active = RegionId::Body;
                        self.status = "Page added. Add a Navigation element to show it in the header.".into();
                    }
                    Err(e) => self.status = e,
                }
            }
        });

        if self.rename_for != self.page {
            self.rename_buf = p.site.pages[self.page].title.clone();
            self.rename_for = self.page;
        }
        if self.page > 0 {
            ui.add_space(8.0);
            ui.label("Selected page");
            ui.add(egui::TextEdit::singleline(&mut self.rename_buf).desired_width(f32::INFINITY));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Rename").clicked() {
                    match p.rename_page(self.page, &self.rename_buf) {
                        Ok(()) => self.status = "Page renamed. Its folder is renamed on save.".into(),
                        Err(e) => self.status = e,
                    }
                }
                if ui.button("Up").clicked() {
                    if let Some(n) = p.move_page(self.page, true) {
                        self.page = n;
                        self.rename_for = usize::MAX;
                    }
                }
                if ui.button("Down").clicked() {
                    if let Some(n) = p.move_page(self.page, false) {
                        self.page = n;
                        self.rename_for = usize::MAX;
                    }
                }
                if ui.button("Delete").clicked() {
                    let ok = rfd::MessageDialog::new()
                        .set_title("Delete page")
                        .set_description(format!("Delete '{}'? Its folder is removed on save.", p.site.pages[self.page].title))
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show();
                    if ok == rfd::MessageDialogResult::Yes {
                        p.delete_page(self.page);
                        self.page = self.page.saturating_sub(1);
                        self.sel = Sel::None;
                        self.rename_for = usize::MAX;
                    }
                }
            });
        } else {
            ui.add_space(8.0);
            ui.weak("The home page lives in the repository root.");
        }
        ui.add_space(12.0);
        ui.separator();
        ui.weak("Click empty space in the header, page or footer to choose where new items go. Drop image or video files onto the canvas to add them.");
    }

    fn props_panel(&mut self, ui: &mut egui::Ui, p: &mut Project) {
        let page = self.page;
        match self.sel {
            Sel::Element(rid, id) => {
                let pages = p.site.pages.clone();
                let media = self.media.clone();
                let root = p.root.clone();
                let mut act = None;
                let mut found = false;
                if let Some(el) = p.region_mut(rid, page).elements.iter_mut().find(|e| e.id == id) {
                    found = true;
                    act = self.element_props(ui, el, &pages, &media, &root);
                }
                if !found {
                    self.sel = Sel::None;
                }
                if let Some(a) = act {
                    self.apply_action(p, rid, id, a);
                }
            }
            Sel::Region(rid) => {
                ui.heading(match rid {
                    RegionId::Header => "Header",
                    RegionId::Footer => "Footer",
                    RegionId::Body => "Page body",
                });
                let r = p.region_mut(rid, page);
                egui::Grid::new("region_props").num_columns(2).show(ui, |ui| {
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut r.height).range(20.0..=5000.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Background");
                    ui.color_edit_button_srgb(&mut r.background.0);
                    ui.end_row();
                });
                ui.weak(match rid {
                    RegionId::Body => "Content here is specific to this page.",
                    _ => "Shown on every page.",
                });
            }
            Sel::None => {
                ui.heading("Properties");
                ui.weak("Select an element or a region (header, page, footer).");
            }
        }
        ui.add_space(10.0);
        ui.separator();
        egui::CollapsingHeader::new("Site settings").default_open(self.sel == Sel::None).show(ui, |ui| {
            egui::Grid::new("site_props").num_columns(2).show(ui, |ui| {
                ui.label("Site name");
                ui.text_edit_singleline(&mut p.site.title);
                ui.end_row();
                ui.label("Design width");
                ui.add(egui::DragValue::new(&mut p.site.width).range(320.0..=2400.0).suffix(" px"));
                ui.end_row();
                ui.label("Page background");
                ui.color_edit_button_srgb(&mut p.site.background.0);
                ui.end_row();
            });
            ui.weak("Pages scale down automatically on narrower screens.");
        });
    }

    fn element_props(
        &mut self,
        ui: &mut egui::Ui,
        el: &mut Element,
        pages: &[PageMeta],
        media: &[String],
        root: &Path,
    ) -> Option<ElemAction> {
        let mut act = None;
        ui.heading(el.kind_name());
        ui.add_space(4.0);
        let mut import: Option<(bool, PathBuf)> = None; // (is_video, path)

        match &mut el.kind {
            Kind::Text(t) => {
                ui.add(egui::TextEdit::multiline(&mut t.text).desired_rows(4).desired_width(f32::INFINITY));
                egui::Grid::new("text_props").num_columns(2).show(ui, |ui| {
                    ui.label("Style");
                    egui::ComboBox::from_id_salt("tag").selected_text(t.tag.label()).show_ui(ui, |ui| {
                        for tag in Tag::ALL {
                            ui.selectable_value(&mut t.tag, tag, tag.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Font");
                    egui::ComboBox::from_id_salt("font").selected_text(t.font.label()).show_ui(ui, |ui| {
                        for f in FontChoice::ALL {
                            ui.selectable_value(&mut t.font, f, f.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Size");
                    ui.add(egui::DragValue::new(&mut t.size).range(6.0..=300.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Color");
                    ui.color_edit_button_srgb(&mut t.color.0);
                    ui.end_row();
                    ui.label("Line height");
                    ui.add(egui::DragValue::new(&mut t.line_height).range(0.8..=4.0).speed(0.02));
                    ui.end_row();
                    ui.label("Emphasis");
                    ui.horizontal(|ui| {
                        ui.toggle_value(&mut t.bold, "Bold");
                        ui.toggle_value(&mut t.italic, "Italic");
                    });
                    ui.end_row();
                    ui.label("Align");
                    ui.horizontal(|ui| {
                        for a in Align::ALL {
                            ui.selectable_value(&mut t.align, a, a.label());
                        }
                    });
                    ui.end_row();
                });
                if !self.serif_ok {
                    ui.weak("Serif preview uses the default font here; the published site uses Georgia/Times.");
                }
            }
            Kind::Image { src, alt, fit, radius } => {
                media_picker(ui, "img_src", src, media, is_image);
                if ui.button("Import image...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Images", &IMAGE_EXT).pick_file() {
                        import = Some((false, path));
                    }
                }
                egui::Grid::new("img_props").num_columns(2).show(ui, |ui| {
                    ui.label("Alt text");
                    ui.text_edit_singleline(alt);
                    ui.end_row();
                    ui.label("Fit");
                    ui.horizontal(|ui| {
                        ui.selectable_value(fit, Fit::Cover, "Cover");
                        ui.selectable_value(fit, Fit::Contain, "Contain");
                    });
                    ui.end_row();
                    ui.label("Corner radius");
                    ui.add(egui::DragValue::new(radius).range(0.0..=500.0).suffix(" px"));
                    ui.end_row();
                });
            }
            Kind::Video { src } => {
                media_picker(ui, "vid_src", src, media, is_video);
                if ui.button("Import video...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Video", &VIDEO_EXT).pick_file() {
                        import = Some((true, path));
                    }
                }
                ui.weak("Large videos make the repository heavy; GitHub rejects files over 100 MB.");
            }
            Kind::Shape { color, radius } => {
                egui::Grid::new("shape_props").num_columns(2).show(ui, |ui| {
                    ui.label("Fill");
                    ui.color_edit_button_srgb(&mut color.0);
                    ui.end_row();
                    ui.label("Corner radius");
                    ui.add(egui::DragValue::new(radius).range(0.0..=500.0).suffix(" px"));
                    ui.end_row();
                });
            }
            Kind::Nav(n) => {
                egui::Grid::new("nav_props").num_columns(2).show(ui, |ui| {
                    ui.label("Font");
                    egui::ComboBox::from_id_salt("navfont").selected_text(n.font.label()).show_ui(ui, |ui| {
                        for f in FontChoice::ALL {
                            ui.selectable_value(&mut n.font, f, f.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Size");
                    ui.add(egui::DragValue::new(&mut n.size).range(6.0..=100.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Color");
                    ui.color_edit_button_srgb(&mut n.color.0);
                    ui.end_row();
                    ui.label("Spacing");
                    ui.add(egui::DragValue::new(&mut n.gap).range(0.0..=200.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Bold");
                    ui.checkbox(&mut n.bold, "");
                    ui.end_row();
                    ui.label("Align");
                    ui.horizontal(|ui| {
                        for a in Align::ALL {
                            ui.selectable_value(&mut n.align, a, a.label());
                        }
                    });
                    ui.end_row();
                });
                ui.weak("Links to every page automatically.");
            }
        }

        if let Some((video, path)) = import {
            match self.import_media(root, &path) {
                Ok(rel) => match &mut el.kind {
                    Kind::Image { src, .. } if !video => *src = rel,
                    Kind::Video { src } if video => *src = rel,
                    _ => {}
                },
                Err(e) => self.status = format!("Import failed: {e}"),
            }
        }

        ui.add_space(6.0);
        ui.separator();
        egui::Grid::new("geom").num_columns(4).show(ui, |ui| {
            ui.label("X");
            ui.add(egui::DragValue::new(&mut el.x).range(0.0..=5000.0));
            ui.label("Y");
            ui.add(egui::DragValue::new(&mut el.y).range(0.0..=5000.0));
            ui.end_row();
            ui.label("W");
            ui.add(egui::DragValue::new(&mut el.w).range(10.0..=5000.0));
            ui.label("H");
            ui.add(egui::DragValue::new(&mut el.h).range(10.0..=5000.0));
            ui.end_row();
        });
        if !matches!(el.kind, Kind::Nav(_)) {
            ui.add_space(4.0);
            ui.label("Link (URL or page)");
            ui.add(egui::TextEdit::singleline(&mut el.link).desired_width(f32::INFINITY).hint_text("https://..."));
            egui::ComboBox::from_id_salt("linkpage").selected_text("Link to page...").show_ui(ui, |ui| {
                if ui.selectable_label(false, "(no link)").clicked() {
                    el.link.clear();
                }
                for pg in pages {
                    let href = if pg.slug.is_empty() { "./".to_string() } else { format!("{}/", pg.slug) };
                    if ui.selectable_label(el.link == href, &pg.title).clicked() {
                        el.link = href;
                    }
                }
            });
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Forward").clicked() {
                act = Some(ElemAction::Forward);
            }
            if ui.button("Backward").clicked() {
                act = Some(ElemAction::Backward);
            }
            if ui.button("Duplicate").clicked() {
                act = Some(ElemAction::Duplicate);
            }
            if ui.button("Delete").clicked() {
                act = Some(ElemAction::Delete);
            }
        });
        act
    }

    fn welcome(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.25);
                ui.heading("Microschool Site Builder");
                ui.add_space(8.0);
                ui.label("Create a static website for GitHub Pages, or open an existing one.");
                ui.add_space(16.0);
                if ui.add_sized([220.0, 36.0], egui::Button::new("New site...")).clicked() {
                    self.cmd = Some(Cmd::New);
                }
                ui.add_space(6.0);
                if ui.add_sized([220.0, 36.0], egui::Button::new("Open site folder...")).clicked() {
                    self.cmd = Some(Cmd::Open);
                }
            });
        });
    }

    fn project_ui(&mut self, ctx: &egui::Context, p: &mut Project) {
        if self.media_stale {
            self.media = p.media_files();
            self.media_stale = false;
        }

        // Dropped files become media elements.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for path in dropped {
            self.add_media_from_path(p, &path);
        }

        // Keyboard shortcuts.
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S))) {
            self.cmd = Some(Cmd::Save);
        }
        if !ctx.wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)))
                || ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y)))
            {
                self.cmd = Some(Cmd::Redo);
            } else if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z))) {
                self.cmd = Some(Cmd::Undo);
            }
            if let Sel::Element(rid, id) = self.sel {
                let (delete, dx, dy, big) = ctx.input(|i| {
                    (
                        i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                        i.key_pressed(Key::ArrowRight) as i32 - i.key_pressed(Key::ArrowLeft) as i32,
                        i.key_pressed(Key::ArrowDown) as i32 - i.key_pressed(Key::ArrowUp) as i32,
                        i.modifiers.shift,
                    )
                });
                if delete {
                    self.apply_action(p, rid, id, ElemAction::Delete);
                } else if dx != 0 || dy != 0 {
                    let step = if big { 10.0 } else { 1.0 };
                    let (width, page) = (p.site.width, self.page);
                    let region = p.region_mut(rid, page);
                    let h = region.height;
                    if let Some(el) = region.elements.iter_mut().find(|e| e.id == id) {
                        el.x = (el.x + dx as f32 * step).clamp(0.0, (width - el.w).max(0.0));
                        el.y = (el.y + dy as f32 * step).clamp(0.0, (h - el.h).max(0.0));
                    }
                }
            }
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            self.menu_bar(ui, true);
        });
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(2.0);
            self.toolbar(ui, p);
            ui.add_space(2.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(p.root.display().to_string());
                });
            });
        });
        egui::SidePanel::left("pages").default_width(190.0).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.pages_panel(ui, p));
        });
        egui::SidePanel::right("props").default_width(290.0).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.props_panel(ui, p));
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_gray(60))).show(ctx, |ui| {
            self.canvas_ui(ui, p);
        });
    }
}

fn media_picker(ui: &mut egui::Ui, salt: &str, src: &mut String, media: &[String], filter: fn(&str) -> bool) {
    let shown = src.strip_prefix("media/").unwrap_or(src).to_string();
    egui::ComboBox::from_id_salt(salt)
        .selected_text(if shown.is_empty() { "Choose file..." } else { shown.as_str() })
        .width(220.0)
        .show_ui(ui, |ui| {
            for m in media.iter().filter(|m| filter(m)) {
                let rel = format!("media/{m}");
                if ui.selectable_label(*src == rel, m).clicked() {
                    *src = rel;
                }
            }
        });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut proj = self.project.take();

        match proj.as_mut() {
            Some(p) => self.project_ui(ctx, p),
            None => {
                egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui, false));
                self.welcome(ctx);
            }
        }

        if let Some(cmd) = self.cmd.take() {
            self.run_cmd(cmd, &mut proj);
        }

        if let Some(p) = proj.as_ref() {
            self.track_changes(ctx, p);
        }

        // Window title with unsaved marker.
        let title = match proj.as_ref() {
            Some(p) => format!(
                "{}{} - Microschool Site Builder",
                p.site.title,
                if self.is_dirty(p) { " *" } else { "" }
            ),
            None => "Microschool Site Builder".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        // Confirm close when there are unsaved changes.
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing_ok {
            if let Some(p) = proj.as_mut() {
                if self.is_dirty(p) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    match rfd::MessageDialog::new()
                        .set_title("Unsaved changes")
                        .set_description("Save changes before closing?")
                        .set_buttons(rfd::MessageButtons::YesNoCancel)
                        .show()
                    {
                        rfd::MessageDialogResult::Yes => {
                            if self.save(p) {
                                self.closing_ok = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        rfd::MessageDialogResult::No => {
                            self.closing_ok = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some(msg) = self.error.clone() {
            let mut open = true;
            egui::Window::new("Problem")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(msg);
                    ui.add_space(8.0);
                    if ui.button("OK").clicked() {
                        self.error = None;
                    }
                });
            if !open {
                self.error = None;
            }
        }

        self.project = proj;
    }
}
