//! The editor UI (egui).

use crate::export;
use crate::model::*;
use crate::presets::{self, Section};
use crate::richtext::{self, Line};
use crate::security;
use crate::server::Server;
use crate::snap;
use eframe::egui::{
    self, pos2, text::CCursor, text::CCursorRange, vec2, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId,
    Key, KeyboardShortcut, Modifiers, Rect, Sense, Stroke, StrokeKind, Vec2,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(46, 125, 246);
const GUIDE: Color32 = Color32::from_rgb(255, 64, 160);
const IMAGE_EXT: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
const VIDEO_EXT: [&str; 4] = ["mp4", "webm", "ogv", "mov"];
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");
const LOGO_BG: Color32 = Color32::from_rgb(248, 248, 237);
/// Prefix of element JSON on the system clipboard, so elements can be pasted across sites.
const CLIP_PREFIX: &str = "willow-element:";
const APP_NAME: &str = "Willow";

#[derive(Clone, Copy, PartialEq)]
enum Sel {
    None,
    Region(RegionId),
    Element(RegionId, u64),
}

#[derive(Clone, Copy, PartialEq)]
enum AlignTo {
    Left,
    HCenter,
    Right,
    Top,
    VMiddle,
    Bottom,
}

#[derive(Clone, Copy)]
enum ElemAction {
    Delete,
    Duplicate,
    Forward,
    Backward,
    Front,
    Back,
    Align(AlignTo),
}

#[derive(Clone, Copy, PartialEq)]
enum Cmd {
    New,
    Open,
    Save,
    Preview,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
}

/// Something added through the Insert menu or the toolbar.
#[derive(Clone, Copy, PartialEq)]
enum Insert {
    Text,
    Image,
    Video,
    Shape,
    Button,
    Nav,
    Embed,
    Table(usize),
    TableCustom,
    Section(Section),
}

/// Actions of the text-editing toolbar.
#[derive(Clone, Copy, PartialEq)]
enum EditTool {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Bullets,
    Secondary,
}

struct TableDialog {
    rows: usize,
    cols: usize,
    style: TableStyle,
}

pub struct App {
    project: Option<Project>,
    page: usize,
    active: RegionId,
    sel: Sel,
    editing: Option<u64>,
    focus_edit: bool,
    pending_insert: Option<Insert>,
    table_dialog: Option<TableDialog>,
    /// Text editor (on the page or in the panel) that was focused last; the editing tools act on it.
    last_text_widget: Option<egui::Id>,

    // snapping and guides
    snap_on: bool,
    snap_all: bool,
    snap_objects: bool,
    snap_grid_on: bool,
    grid_size: f32,
    show_grid: bool,
    /// Unsnapped geometry of the element being dragged: (id, x, y, w, h).
    drag_raw: Option<(u64, f32, f32, f32, f32)>,

    // screen rectangles from the previous frame, used to decide when editing ends
    edit_rect: Option<Rect>,
    bar_rect: Option<Rect>,
    props_rect: Option<Rect>,

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

struct RunStyle {
    font: FontId,
    color: Color32,
    italic: bool,
    bold: bool,
    size: f32,
}

fn egui_align(a: Align) -> egui::Align {
    match a {
        Align::Left => egui::Align::LEFT,
        Align::Center => egui::Align::Center,
        Align::Right => egui::Align::RIGHT,
    }
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
        let mut app = App::blank();
        app.serif_ok = install_fonts(&cc.egui_ctx);
        app.server = Server::start().ok();
        app
    }

    /// An empty editor with no window, preview server or custom fonts attached.
    fn blank() -> App {
        App {
            project: None,
            page: 0,
            active: RegionId::Body,
            sel: Sel::None,
            editing: None,
            focus_edit: false,
            pending_insert: None,
            table_dialog: None,
            last_text_widget: None,
            snap_on: false,
            snap_all: false,
            snap_objects: true,
            snap_grid_on: false,
            grid_size: 20.0,
            show_grid: false,
            drag_raw: None,
            edit_rect: None,
            bar_rect: None,
            props_rect: None,
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
            server: None,
            serif_ok: false,
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

    fn run_cmd(&mut self, ctx: &egui::Context, cmd: Cmd, proj: &mut Option<Project>) {
        match cmd {
            Cmd::Copy | Cmd::Cut => {
                if let Some(p) = proj.as_mut() {
                    self.copy_selected(ctx, p, cmd == Cmd::Cut);
                }
            }
            // The paste event arrives on the next frame and is handled by `handle_clipboard_events`.
            Cmd::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            Cmd::Duplicate | Cmd::Delete => {
                if let (Some(p), Sel::Element(rid, id)) = (proj.as_mut(), self.sel) {
                    let act = if cmd == Cmd::Duplicate { ElemAction::Duplicate } else { ElemAction::Delete };
                    self.apply_action(p, rid, id, act);
                }
            }
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
                let Some(dir) =
                    rfd::FileDialog::new().set_title("Choose an empty folder for the new site").pick_folder()
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
        *self.dims.entry(path.to_path_buf()).or_insert_with(|| image::image_dimensions(path).ok())
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
        let ink = contrast(bg);
        let mut props = TextProps { color: ink, ..TextProps::default() };
        props.alt.color = ink;
        self.add_element(p, Kind::Text(props), 300.0, 50.0);
    }

    fn add_button(&mut self, p: &mut Project) {
        let bg = p.region(self.active, self.page).background;
        let props = if contrast(bg) == Rgb::WHITE {
            ButtonProps { fill: Rgb([242, 169, 0]), color: Rgb::INK, ..ButtonProps::default() }
        } else {
            ButtonProps::default()
        };
        self.add_element(p, Kind::Button(props), 220.0, 56.0);
    }

    fn add_table(&mut self, p: &mut Project, props: TableProps) {
        let (w, h) = presets::table_size(&props, p.site.width);
        self.add_element(p, Kind::Table(props), w, h);
    }

    /// Inserts a ready-made section below the lowest element of the active region and
    /// grows the region if needed.
    fn add_section(&mut self, p: &mut Project, section: Section) {
        let (rid, page, width) = (self.active, self.page, p.site.width);
        let region = p.region(rid, page);
        let ink = contrast(region.background);
        let bottom = region.elements.iter().map(|e| e.y + e.h).fold(0.0_f32, f32::max);
        let y0 = if region.elements.is_empty() { 0.0 } else { (bottom + 20.0).round() };
        let (els, height) = presets::build_section(section, width, y0, ink, &mut || p.next_id());
        let region = p.region_mut(rid, page);
        region.height = region.height.max(y0 + height + 40.0);
        region.elements.extend(els);
        self.sel = Sel::Region(rid);
        self.editing = None;
        self.status = format!("Inserted: {}", section.label());
    }

    fn do_insert(&mut self, p: &mut Project, what: Insert) {
        match what {
            Insert::Text => self.add_text(p),
            Insert::Image => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Images", &IMAGE_EXT).pick_file() {
                    self.add_media_from_path(p, &path);
                }
            }
            Insert::Video => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Video", &VIDEO_EXT).pick_file() {
                    self.add_media_from_path(p, &path);
                }
            }
            Insert::Shape => {
                let bg = p.region(self.active, self.page).background;
                let color = if contrast(bg) == Rgb::WHITE { Rgb([70, 90, 120]) } else { Rgb([225, 232, 242]) };
                self.add_element(p, Kind::Shape { color, radius: 8.0 }, 300.0, 150.0);
            }
            Insert::Button => self.add_button(p),
            Insert::Nav => {
                let bg = p.region(self.active, self.page).background;
                let n = NavProps { color: contrast(bg), ..NavProps::default() };
                self.add_element(p, Kind::Nav(n), 400.0, 30.0);
            }
            Insert::Embed => {
                self.add_element(p, Kind::Embed { url: String::new(), title: String::new() }, 560.0, 315.0);
                self.status =
                    "Paste a YouTube, Vimeo, Google Maps, Calendar or Forms address in the properties panel.".into();
            }
            Insert::Table(i) => {
                if let Some(preset) = presets::table_presets().into_iter().nth(i) {
                    self.add_table(p, preset.props);
                }
            }
            Insert::TableCustom => self.table_dialog = Some(TableDialog { rows: 4, cols: 3, style: TableStyle::Grid }),
            Insert::Section(section) => self.add_section(p, section),
        }
    }

    // ------------------------------------------------------------ clipboard (elements on the page)

    fn copy_selected(&mut self, ctx: &egui::Context, p: &mut Project, cut: bool) {
        let Sel::Element(rid, id) = self.sel else {
            return;
        };
        let Some(el) = p.region(rid, self.page).elements.iter().find(|e| e.id == id).cloned() else {
            return;
        };
        match serde_json::to_string(&el) {
            Ok(json) => {
                ctx.copy_text(format!("{CLIP_PREFIX}{json}"));
                self.status = format!("{} {}", if cut { "Cut" } else { "Copied" }, el.kind_name().to_lowercase());
                if cut {
                    self.apply_action(p, rid, id, ElemAction::Delete);
                }
            }
            Err(e) => self.status = format!("Copy failed: {e}"),
        }
    }

    /// Handles text that arrives from the clipboard while no text field has focus: an element
    /// copied from Willow is re-created, any other text becomes a new text box.
    fn paste_text(&mut self, p: &mut Project, pasted: &str) {
        if let Some(json) = pasted.strip_prefix(CLIP_PREFIX) {
            match serde_json::from_str::<Element>(json) {
                Ok(mut el) => {
                    el.sanitize();
                    self.insert_pasted(p, el);
                }
                Err(_) => self.status = "The clipboard does not contain a valid Willow element.".into(),
            }
        } else if !pasted.trim().is_empty() {
            let bg = p.region(self.active, self.page).background;
            let ink = contrast(bg);
            let mut props = TextProps { color: ink, ..TextProps::default() };
            props.alt.color = ink;
            props.text = pasted.chars().take(security::MAX_TEXT_CHARS).collect();
            let lines = props.text.lines().count().max(1) as f32;
            let h = (lines * props.size * props.line_height + 10.0).clamp(50.0, 600.0);
            self.add_element(p, Kind::Text(props), 500.0, h.round());
            self.status = "Pasted text as a new text box.".into();
        }
    }

    fn insert_pasted(&mut self, p: &mut Project, mut el: Element) {
        let (rid, page, width) = (self.active, self.page, p.site.width);
        el.id = p.next_id();
        el.w = el.w.min(width);
        let region = p.region_mut(rid, page);
        el.h = el.h.min(region.height);
        let (mut x, mut y) = (el.x + 20.0, el.y + 20.0);
        while region.elements.iter().any(|o| (o.x - x).abs() < 1.0 && (o.y - y).abs() < 1.0) {
            x += 20.0;
            y += 20.0;
        }
        el.x = x.min(width - el.w).max(0.0);
        el.y = y.min(region.height - el.h).max(0.0);
        let id = el.id;
        region.elements.push(el);
        self.sel = Sel::Element(rid, id);
        self.editing = None;
        self.status = "Pasted.".into();
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
            ElemAction::Align(to) => {
                let (rw, rh) = (p.site.width, p.region(rid, page).height);
                let el = &mut p.region_mut(rid, page).elements[idx];
                match to {
                    AlignTo::Left => el.x = 0.0,
                    AlignTo::HCenter => el.x = ((rw - el.w) / 2.0).max(0.0).round(),
                    AlignTo::Right => el.x = (rw - el.w).max(0.0),
                    AlignTo::Top => el.y = 0.0,
                    AlignTo::VMiddle => el.y = ((rh - el.h) / 2.0).max(0.0).round(),
                    AlignTo::Bottom => el.y = (rh - el.h).max(0.0),
                }
            }
        }
    }

    // ------------------------------------------------------------ drawing

    /// Font, colour and weight of the primary or the secondary (`{{ }}`) text style.
    fn run_style(&self, t: &TextProps, alt: bool, z: f32) -> RunStyle {
        if alt {
            RunStyle {
                font: FontId::new(t.alt.size * z, self.family(t.alt.font)),
                color: c32(t.alt.color),
                italic: t.alt.italic,
                bold: t.alt.bold,
                size: t.alt.size,
            }
        } else {
            RunStyle {
                font: FontId::new(t.size * z, self.family(t.font)),
                color: c32(t.color),
                italic: t.italic,
                bold: t.bold,
                size: t.size,
            }
        }
    }

    /// Lays out one logical line (a paragraph or one bullet) of a text box. With `bold_overlay`
    /// only the bold runs are visible, so painting it again with a small offset fakes bold weight.
    fn layout_line(
        &self,
        ctx: &egui::Context,
        t: &TextProps,
        line: &Line,
        z: f32,
        wrap: f32,
        bold_overlay: bool,
    ) -> Arc<egui::Galley> {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wrap;
        job.halign = egui_align(t.align);
        for span in &line.spans {
            let st = self.run_style(t, span.alt, z);
            let color = if bold_overlay && !st.bold { Color32::TRANSPARENT } else { st.color };
            job.append(
                &span.text,
                0.0,
                egui::TextFormat {
                    font_id: st.font,
                    color,
                    italics: st.italic,
                    line_height: Some(st.size * t.line_height * z),
                    ..Default::default()
                },
            );
        }
        ctx.fonts(|f| f.layout_job(job))
    }

    /// Single-style text, used by buttons and table cells.
    fn simple_galley(
        &self,
        ctx: &egui::Context,
        text: &str,
        font: FontId,
        color: Color32,
        wrap: f32,
        align: Align,
    ) -> Arc<egui::Galley> {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wrap.max(4.0);
        job.halign = egui_align(align);
        job.append(text, 0.0, egui::TextFormat { font_id: font, color, ..Default::default() });
        ctx.fonts(|f| f.layout_job(job))
    }

    /// Paints a text box: paragraphs, nested bullets and the two text styles.
    fn draw_text(&self, ui: &egui::Ui, t: &TextProps, rect: Rect, z: f32) {
        let painter = ui.painter();
        let ctx = ui.ctx();
        let has_alt = richtext::has_alt(&t.text);
        let any_bold = t.bold || (has_alt && t.alt.bold);
        let blank_h = t.size * t.line_height * z;
        let indent_unit = t.size * 1.4 * z;
        let bottom = painter.clip_rect().max.y;
        let mut y = rect.min.y;
        for line in richtext::parse(&t.text) {
            if y > bottom {
                break;
            }
            if line.is_blank() {
                y += blank_h;
                continue;
            }
            let indent = if line.bullet { indent_unit * (line.level as f32 + 1.0) } else { 0.0 };
            let area_min = rect.min.x + indent;
            let wrap = (rect.max.x - area_min).max(10.0);
            let ax = match t.align {
                Align::Left => area_min,
                Align::Center => area_min + wrap / 2.0,
                Align::Right => rect.max.x,
            };
            let galley = self.layout_line(ctx, t, &line, z, wrap, false);
            let pos = pos2(ax, y);
            painter.galley(pos, galley.clone(), c32(t.color));
            if any_bold {
                let overlay = self.layout_line(ctx, t, &line, z, wrap, true);
                painter.galley(pos + vec2((t.size * z * 0.03).max(0.5), 0.0), overlay, c32(t.color));
            }
            if line.bullet {
                let first_alt = line.spans.first().is_some_and(|s| s.alt);
                let st = self.run_style(t, first_alt, z);
                let marker = self.simple_galley(ctx, "\u{2022}", st.font, st.color, indent_unit, Align::Right);
                // Right-aligned galleys are anchored at their right edge, just left of the text.
                painter.galley(pos2(area_min - st.size * z * 0.3, y), marker, st.color);
            }
            y += galley.size().y;
        }
    }

    fn draw_table(&self, ui: &egui::Ui, t: &TableProps, rect: Rect, z: f32) {
        let (rows, cols) = (t.rows(), t.cols());
        if rows == 0 || cols == 0 {
            return;
        }
        let ctx = ui.ctx();
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.painter().clip_rect()));
        let radius = if t.style == TableStyle::Card {
            CornerRadius::same((8.0 * z).round().clamp(0.0, 255.0) as u8)
        } else {
            CornerRadius::ZERO
        };
        painter.rect_filled(rect, radius, c32(t.background));
        let (cw, rh) = (rect.width() / cols as f32, rect.height() / rows as f32);
        let filled = t.header_filled();
        let body_start = usize::from(t.header_row);
        let pad = t.padding * z;
        let line = Stroke::new((1.0 * z).max(1.0), c32(t.border));

        for r in 0..rows {
            for c in 0..cols {
                let cell =
                    Rect::from_min_size(pos2(rect.min.x + c as f32 * cw, rect.min.y + r as f32 * rh), vec2(cw, rh));
                let header = t.is_header(r, c);
                if header && filled {
                    painter.rect_filled(cell, 0.0, c32(t.header_bg));
                } else if t.style == TableStyle::Striped && !header && r >= body_start && (r - body_start) % 2 == 1 {
                    painter.rect_filled(cell, 0.0, c32(t.stripe));
                }
                let text = &t.cells[r][c];
                if text.is_empty() {
                    continue;
                }
                let color = if header && filled { c32(t.header_color) } else { c32(t.color) };
                let align = if t.header_col && c == 0 && r >= body_start { Align::Left } else { t.align };
                let font = FontId::new(t.size * z, self.family(t.font));
                let galley = self.simple_galley(ctx, text, font, color, cw - 2.0 * pad, align);
                let ax = match align {
                    Align::Left => cell.min.x + pad,
                    Align::Center => cell.center().x,
                    Align::Right => cell.max.x - pad,
                };
                let pos = pos2(ax, (cell.center().y - galley.size().y / 2.0).max(cell.min.y));
                let cp = painter.with_clip_rect(cell.intersect(painter.clip_rect()));
                cp.galley(pos, galley.clone(), color);
                if header {
                    cp.galley(pos + vec2((t.size * z * 0.03).max(0.5), 0.0), galley, color);
                }
            }
        }
        match t.style {
            TableStyle::Grid => {
                for r in 0..=rows {
                    let y = rect.min.y + r as f32 * rh;
                    painter.line_segment([pos2(rect.min.x, y), pos2(rect.max.x, y)], line);
                }
                for c in 0..=cols {
                    let x = rect.min.x + c as f32 * cw;
                    painter.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], line);
                }
            }
            TableStyle::Lines | TableStyle::Card => {
                for r in 1..=rows {
                    if t.style == TableStyle::Card && r == rows {
                        continue;
                    }
                    let y = rect.min.y + r as f32 * rh;
                    let heavy = t.style == TableStyle::Lines && t.header_row && r == 1;
                    let stroke = if heavy { Stroke::new(line.width * 2.0, line.color) } else { line };
                    painter.line_segment([pos2(rect.min.x, y), pos2(rect.max.x, y)], stroke);
                }
                if t.style == TableStyle::Card {
                    painter.rect_stroke(rect, radius, line, StrokeKind::Inside);
                }
            }
            TableStyle::Striped | TableStyle::Borderless => {}
        }
    }

    fn draw_element(&mut self, ui: &egui::Ui, e: &Element, rect: Rect, z: f32, root: &Path, pages: &[PageMeta]) {
        let painter = ui.painter();
        match &e.kind {
            Kind::Text(t) => self.draw_text(ui, t, rect, z),
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
                let galleys: Vec<_> =
                    pages.iter().map(|p| painter.layout_no_wrap(p.title.clone(), font.clone(), color)).collect();
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
            Kind::Button(b) => {
                let r = (b.radius * z).round().clamp(0.0, 255.0) as u8;
                painter.rect_filled(rect, CornerRadius::same(r), c32(b.fill));
                let font = FontId::new(b.size * z, self.family(b.font));
                let galley =
                    self.simple_galley(ui.ctx(), &b.label, font, c32(b.color), rect.width() - 8.0, Align::Center);
                let pos = pos2(rect.center().x, rect.center().y - galley.size().y / 2.0);
                painter.galley(pos, galley.clone(), c32(b.color));
                if b.bold {
                    painter.galley(pos + vec2((b.size * z * 0.03).max(0.5), 0.0), galley, c32(b.color));
                }
            }
            Kind::Table(t) => self.draw_table(ui, t, rect, z),
            Kind::Embed { url, .. } => {
                painter.rect_filled(rect, 4.0, Color32::from_gray(232));
                let (label, color) = match security::normalize_embed(url) {
                    Ok(u) => {
                        (security::origin_of(&u).unwrap_or_default().replace("https://", ""), Color32::from_gray(70))
                    }
                    Err(e) if url.trim().is_empty() => (e, Color32::from_gray(110)),
                    Err(e) => (format!("Not allowed: {e}"), Color32::from_rgb(180, 40, 40)),
                };
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    format!("EMBED\n{label}"),
                    FontId::proportional(14.0),
                    color,
                );
            }
        }
    }

    /// Ends the text-editing session when the pointer is pressed anywhere except the text box,
    /// its floating toolbar or the properties panel, or when Escape is pressed.
    fn update_editing_session(&mut self, ctx: &egui::Context) {
        if self.editing.is_none() {
            self.edit_rect = None;
            self.bar_rect = None;
            return;
        }
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let pressed_at = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
        let outside = pressed_at.is_some_and(|pos| {
            ![self.edit_rect, self.bar_rect, self.props_rect].iter().flatten().any(|r| r.contains(pos))
        });
        if escape || outside {
            self.editing = None;
            self.edit_rect = None;
            self.bar_rect = None;
        }
    }

    fn canvas_ui(&mut self, ui: &mut egui::Ui, p: &mut Project) {
        let width = p.site.width;
        let pad = 28.0;
        if self.fit_zoom {
            self.zoom = ((ui.available_width() - 2.0 * pad - 18.0) / width).clamp(0.25, 2.0);
        }
        self.update_editing_session(ui.ctx());
        let z = self.zoom;
        let page = self.page;
        let root = p.root.clone();
        let pages = p.site.pages.clone();
        let order = [RegionId::Header, RegionId::Body, RegionId::Footer];
        let heights = [p.header.height, p.contents[page].height, p.footer.height];
        let total: f32 = heights.iter().sum();
        let mut actions: Vec<(RegionId, u64, ElemAction)> = Vec::new();
        // Holding Alt turns snapping off for the current drag.
        let alt_held = ui.input(|i| i.modifiers.alt);
        let snap_cfg = snap::Config {
            threshold: 6.0 / z,
            grid: self.snap_grid_on.then_some(self.grid_size),
            objects: self.snap_objects,
        };
        let snap_active = self.snap_on && !alt_held && (self.snap_objects || self.snap_grid_on);

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
                let region_bg = region.background;
                // Geometry of every element, so a dragged element can snap to the others.
                let others: Vec<(u64, snap::Box4)> =
                    region.elements.iter().map(|e| (e.id, snap::Box4 { x: e.x, y: e.y, w: e.w, h: e.h })).collect();
                let targets_without = |id: u64| {
                    let boxes: Vec<snap::Box4> = others.iter().filter(|(i, _)| *i != id).map(|(_, b)| *b).collect();
                    snap::targets(width, region_h, &boxes)
                };
                let mut guides: Vec<(bool, f32)> = Vec::new(); // (vertical, design position)

                ui.painter().rect_filled(band, 0.0, c32(region_bg));
                let band_resp = ui.interact(band, egui::Id::new(("band", ri)), Sense::click());
                if band_resp.clicked() {
                    self.sel = Sel::Region(rid);
                    self.active = rid;
                    self.editing = None;
                }

                let old_clip = ui.clip_rect();
                ui.set_clip_rect(band.intersect(old_clip));

                if self.show_grid && self.grid_size * z >= 4.0 {
                    let ink = contrast(region_bg).0;
                    let stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(ink[0], ink[1], ink[2], 30));
                    let mut gx = 0.0;
                    while gx <= width {
                        let sx = origin.x + gx * z;
                        ui.painter().line_segment([pos2(sx, band.min.y), pos2(sx, band.max.y)], stroke);
                        gx += self.grid_size;
                    }
                    let mut gy = 0.0;
                    while gy <= region_h {
                        let sy = origin.y + gy * z;
                        ui.painter().line_segment([pos2(band.min.x, sy), pos2(band.max.x, sy)], stroke);
                        gy += self.grid_size;
                    }
                }

                for i in 0..region.elements.len() {
                    let el = &mut region.elements[i];
                    let rect = Rect::from_min_size(origin + vec2(el.x, el.y) * z, vec2(el.w, el.h) * z);
                    let editing_this = self.editing == Some(el.id);
                    let selected = self.sel == Sel::Element(rid, el.id);
                    let eid = egui::Id::new(("el", el.id));
                    let snapping = snap_active && (self.snap_all || matches!(el.kind, Kind::Text(_)));

                    if editing_this {
                        let fam = self.family(match &el.kind {
                            Kind::Text(t) => t.font,
                            _ => FontChoice::Sans,
                        });
                        if let Kind::Text(t) = &mut el.kind {
                            let widget_id = egui::Id::new(("edit", el.id));
                            // Focus is requested before the widget runs so that a pending paste
                            // event (from the toolbar or a menu) reaches it on the same frame.
                            if self.focus_edit {
                                ui.memory_mut(|m| m.request_focus(widget_id));
                                self.focus_edit = false;
                            }
                            let te = egui::TextEdit::multiline(&mut t.text)
                                .id(widget_id)
                                .font(FontId::new(t.size * z, fam))
                                .text_color(c32(t.color))
                                .horizontal_align(egui_align(t.align))
                                .frame(false)
                                .margin(egui::Margin::ZERO)
                                .desired_rows(1)
                                .desired_width(rect.width());
                            let r = ui.put(rect, te);
                            if r.has_focus() {
                                self.last_text_widget = Some(widget_id);
                            }
                            self.edit_rect = Some(rect);
                        }
                        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.5_f32, ACCENT), StrokeKind::Outside);
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
                    if resp.drag_started() {
                        self.drag_raw = Some((el.id, el.x, el.y, el.w, el.h));
                    }
                    if resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                        let d = resp.drag_delta() / z;
                        // The unsnapped position keeps accumulating, so the box can pull free of a snap.
                        let (mut nx, mut ny) = match &mut self.drag_raw {
                            Some((id, rx, ry, _, _)) if *id == el.id => {
                                *rx = (*rx + d.x).clamp(0.0, (width - el.w).max(0.0));
                                *ry = (*ry + d.y).clamp(0.0, (region_h - el.h).max(0.0));
                                (*rx, *ry)
                            }
                            _ => (el.x + d.x, el.y + d.y),
                        };
                        if snapping {
                            let s = snap::snap_move(
                                snap::Box4 { x: nx, y: ny, w: el.w, h: el.h },
                                &targets_without(el.id),
                                &snap_cfg,
                            );
                            (nx, ny) = (s.x, s.y);
                            guides.extend(s.guide_x.map(|g| (true, g)));
                            guides.extend(s.guide_y.map(|g| (false, g)));
                        }
                        el.x = nx.clamp(0.0, (width - el.w).max(0.0));
                        el.y = ny.clamp(0.0, (region_h - el.h).max(0.0));
                    }
                    if resp.drag_stopped() {
                        el.x = el.x.round();
                        el.y = el.y.round();
                        self.drag_raw = None;
                    }
                    if resp.double_clicked() && matches!(el.kind, Kind::Text(_)) {
                        self.editing = Some(el.id);
                        self.focus_edit = true;
                        self.sel = Sel::Element(rid, el.id);
                    }
                    let id = el.id;
                    resp.context_menu(|ui| {
                        if ui.button("Copy").clicked() {
                            self.cmd = Some(Cmd::Copy);
                            ui.close_menu();
                        }
                        if ui.button("Cut").clicked() {
                            self.cmd = Some(Cmd::Cut);
                            ui.close_menu();
                        }
                        if ui.button("Paste").clicked() {
                            self.cmd = Some(Cmd::Paste);
                            ui.close_menu();
                        }
                        ui.separator();
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
                        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.5_f32, ACCENT), StrokeKind::Outside);
                        let hr = Rect::from_center_size(rect.right_bottom(), Vec2::splat(14.0));
                        let hresp = ui.interact(hr, eid.with("resize"), Sense::drag());
                        if hresp.hovered() || hresp.dragged() {
                            ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
                        }
                        if hresp.drag_started() {
                            self.drag_raw = Some((el.id, el.x, el.y, el.w, el.h));
                        }
                        if hresp.dragged() {
                            let d = hresp.drag_delta() / z;
                            let max_w = (width - el.x).max(10.0);
                            let (mut nw, mut nh) = match &mut self.drag_raw {
                                Some((id, _, _, rw, rh)) if *id == el.id => {
                                    *rw = (*rw + d.x).clamp(10.0, max_w);
                                    *rh = (*rh + d.y).max(10.0);
                                    (*rw, *rh)
                                }
                                _ => (el.w + d.x, el.h + d.y),
                            };
                            if snapping {
                                let s = snap::snap_resize(
                                    snap::Box4 { x: el.x, y: el.y, w: nw, h: nh },
                                    &targets_without(el.id),
                                    &snap_cfg,
                                );
                                (nw, nh) = (s.w, s.h);
                                guides.extend(s.guide_x.map(|g| (true, g)));
                                guides.extend(s.guide_y.map(|g| (false, g)));
                            }
                            el.w = nw.clamp(10.0, max_w);
                            el.h = nh.max(10.0);
                        }
                        if hresp.drag_stopped() {
                            el.w = el.w.round();
                            el.h = el.h.round();
                            self.drag_raw = None;
                        }
                        ui.painter().rect_filled(
                            Rect::from_center_size(rect.right_bottom(), Vec2::splat(9.0)),
                            2.0,
                            ACCENT,
                        );
                    } else if resp.hovered() {
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.5)),
                            StrokeKind::Outside,
                        );
                    }
                }

                for (vertical, pos) in &guides {
                    let (a, b) = if *vertical {
                        let x = origin.x + pos * z;
                        (pos2(x, band.min.y), pos2(x, band.max.y))
                    } else {
                        let gy = origin.y + pos * z;
                        (pos2(band.min.x, gy), pos2(band.max.x, gy))
                    };
                    ui.painter().line_segment([a, b], Stroke::new(1.0_f32, GUIDE));
                }
                ui.set_clip_rect(old_clip);

                // Boundary line and active-region marker.
                ui.painter().line_segment(
                    [band.left_bottom(), band.right_bottom()],
                    Stroke::new(1.0_f32, Color32::from_gray(150).gamma_multiply(0.6)),
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
                    let items = [
                        ("Undo", "Ctrl+Z", Cmd::Undo),
                        ("Redo", "Ctrl+Shift+Z", Cmd::Redo),
                        ("", "", Cmd::Undo),
                        ("Cut", "Ctrl+X", Cmd::Cut),
                        ("Copy", "Ctrl+C", Cmd::Copy),
                        ("Paste", "Ctrl+V", Cmd::Paste),
                        ("Duplicate", "Ctrl+D", Cmd::Duplicate),
                        ("Delete", "Del", Cmd::Delete),
                    ];
                    for (label, shortcut, cmd) in items {
                        if label.is_empty() {
                            ui.separator();
                        } else if ui.add(egui::Button::new(label).shortcut_text(shortcut)).clicked() {
                            self.cmd = Some(cmd);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Insert", |ui| self.insert_menu(ui));
                ui.menu_button("View", |ui| {
                    if ui.checkbox(&mut self.fit_zoom, "Fit page to window").clicked() {
                        ui.close_menu();
                    }
                    ui.separator();
                    self.snap_options(ui);
                });
            });
        });
    }

    fn table_menu(&mut self, ui: &mut egui::Ui) {
        for (i, preset) in presets::table_presets().iter().enumerate() {
            if ui.button(preset.name).clicked() {
                self.pending_insert = Some(Insert::Table(i));
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button("Custom table...").clicked() {
            self.pending_insert = Some(Insert::TableCustom);
            ui.close_menu();
        }
    }

    fn section_menu(&mut self, ui: &mut egui::Ui) {
        for section in Section::ALL {
            if ui.button(section.label()).clicked() {
                self.pending_insert = Some(Insert::Section(section));
                ui.close_menu();
            }
        }
    }

    fn insert_menu(&mut self, ui: &mut egui::Ui) {
        let items = [
            ("Text", Insert::Text),
            ("Image...", Insert::Image),
            ("Video...", Insert::Video),
            ("Shape", Insert::Shape),
            ("Button", Insert::Button),
            ("Navigation", Insert::Nav),
            ("Embed (video, map, calendar, form)", Insert::Embed),
        ];
        for (label, what) in items {
            if ui.button(label).clicked() {
                self.pending_insert = Some(what);
                ui.close_menu();
            }
        }
        ui.separator();
        ui.menu_button("Table", |ui| self.table_menu(ui));
        ui.menu_button("Section", |ui| self.section_menu(ui));
    }

    fn snap_options(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.snap_on, "Snap text boxes while dragging and resizing");
        ui.add_enabled_ui(self.snap_on, |ui| {
            ui.checkbox(&mut self.snap_all, "Also snap images, shapes, tables and other elements");
            ui.checkbox(&mut self.snap_objects, "Snap to other elements and page edges");
            ui.checkbox(&mut self.snap_grid_on, "Snap to grid");
            ui.horizontal(|ui| {
                ui.label("Grid size");
                ui.add(egui::DragValue::new(&mut self.grid_size).range(5.0..=200.0).speed(1.0).suffix(" px"));
            });
        });
        ui.checkbox(&mut self.show_grid, "Show grid");
        ui.weak("Hold Alt while dragging to turn snapping off temporarily.");
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, p: &mut Project) {
        ui.horizontal_wrapped(|ui| {
            let region_name = match self.active {
                RegionId::Header => "Header".to_string(),
                RegionId::Footer => "Footer".to_string(),
                RegionId::Body => format!("Page: {}", p.site.pages[self.page].title),
            };
            ui.label(format!("Add to {region_name}:"));
            let quick = [
                ("Text", Insert::Text),
                ("Image...", Insert::Image),
                ("Video...", Insert::Video),
                ("Shape", Insert::Shape),
                ("Button", Insert::Button),
                ("Navigation", Insert::Nav),
                ("Embed", Insert::Embed),
            ];
            for (label, what) in quick {
                if ui.button(label).clicked() {
                    self.pending_insert = Some(what);
                }
            }
            ui.menu_button("Table", |ui| self.table_menu(ui));
            ui.menu_button("Section", |ui| self.section_menu(ui));
            ui.separator();
            ui.toggle_value(&mut self.snap_on, "Snap")
                .on_hover_text("Snap text boxes to other elements, page edges and the grid. Hold Alt to bypass.");
            ui.menu_button("Grid", |ui| self.snap_options(ui));
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
                        .set_description(format!(
                            "Delete '{}'? Its folder is removed on save.",
                            p.site.pages[self.page].title
                        ))
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
        egui::CollapsingHeader::new("Page settings").default_open(false).show(ui, |ui| {
            ui.label("Search description");
            ui.add(
                egui::TextEdit::multiline(&mut p.site.pages[page].description)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("One or two sentences shown by search engines"),
            );
            ui.weak("Falls back to the site description when empty.");
        });
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
                ui.label("Language");
                ui.add(egui::TextEdit::singleline(&mut p.site.lang).desired_width(60.0).hint_text("en"));
                ui.end_row();
                ui.label("Site icon");
                ui.horizontal(|ui| {
                    media_picker(ui, "favicon", &mut p.site.favicon, &self.media, is_image);
                    if ui.small_button("Clear").clicked() {
                        p.site.favicon.clear();
                    }
                });
                ui.end_row();
            });
            ui.label("Default description");
            ui.add(
                egui::TextEdit::multiline(&mut p.site.description)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text("Used by search engines when a page has none"),
            );
            ui.checkbox(&mut p.site.strict_csp, "Strict Content-Security-Policy").on_hover_text(
                "Blocks scripts, styles, images and frames from anywhere except this site and the embeds you add. \
Turn off only if you add your own external resources by hand.",
            );
            if !security::valid_lang(&p.site.lang) {
                ui.colored_label(
                    Color32::from_rgb(200, 60, 60),
                    "Language must be letters, digits and dashes, e.g. en or pt-BR.",
                );
            }
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
                let canvas_id = egui::Id::new(("edit", el.id));
                let panel_id = egui::Id::new(("panel_text", el.id));
                let r = ui.add(
                    egui::TextEdit::multiline(&mut t.text).id(panel_id).desired_rows(4).desired_width(f32::INFINITY),
                );
                if r.has_focus() {
                    self.last_text_widget = Some(panel_id);
                }
                // Tools act on whichever of the two editors (page or panel) was used last.
                let target = match self.last_text_widget {
                    Some(id) if id == canvas_id || id == panel_id => id,
                    _ if self.editing == Some(el.id) => canvas_id,
                    _ => panel_id,
                };
                let mut tool = None;
                ui.horizontal_wrapped(|ui| tool = edit_tool_buttons(ui));
                if let Some(tool) = tool {
                    run_edit_tool(ui.ctx(), target, &mut t.text, tool);
                }
                ui.weak("Start a line with \"- \" for a bullet; indent two spaces per sub-level.");
                ui.add_space(4.0);
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
                egui::CollapsingHeader::new("Secondary style").default_open(richtext::has_alt(&t.text)).show(ui, |ui| {
                    ui.weak("Text inside {{double braces}} uses this style, so one box can mix two sizes or fonts. Select text and press \"2nd style\" to apply it.");
                    egui::Grid::new("alt_props").num_columns(2).show(ui, |ui| {
                        ui.label("Font");
                        egui::ComboBox::from_id_salt("altfont").selected_text(t.alt.font.label()).show_ui(ui, |ui| {
                            for f in FontChoice::ALL {
                                ui.selectable_value(&mut t.alt.font, f, f.label());
                            }
                        });
                        ui.end_row();
                        ui.label("Size");
                        ui.add(egui::DragValue::new(&mut t.alt.size).range(6.0..=300.0).suffix(" px"));
                        ui.end_row();
                        ui.label("Color");
                        ui.color_edit_button_srgb(&mut t.alt.color.0);
                        ui.end_row();
                        ui.label("Emphasis");
                        ui.horizontal(|ui| {
                            ui.toggle_value(&mut t.alt.bold, "Bold");
                            ui.toggle_value(&mut t.alt.italic, "Italic");
                        });
                        ui.end_row();
                    });
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
            Kind::Button(b) => {
                egui::Grid::new("btn_props").num_columns(2).show(ui, |ui| {
                    ui.label("Label");
                    ui.text_edit_singleline(&mut b.label);
                    ui.end_row();
                    ui.label("Font");
                    egui::ComboBox::from_id_salt("btnfont").selected_text(b.font.label()).show_ui(ui, |ui| {
                        for f in FontChoice::ALL {
                            ui.selectable_value(&mut b.font, f, f.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Size");
                    ui.add(egui::DragValue::new(&mut b.size).range(6.0..=120.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Text color");
                    ui.color_edit_button_srgb(&mut b.color.0);
                    ui.end_row();
                    ui.label("Fill");
                    ui.color_edit_button_srgb(&mut b.fill.0);
                    ui.end_row();
                    ui.label("Corner radius");
                    ui.add(egui::DragValue::new(&mut b.radius).range(0.0..=500.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Bold");
                    ui.checkbox(&mut b.bold, "");
                    ui.end_row();
                });
                ui.weak("Set the link below to choose where the button goes.");
            }
            Kind::Table(t) => {
                let (mut rows, mut cols) = (t.rows(), t.cols());
                egui::Grid::new("tbl_props").num_columns(2).show(ui, |ui| {
                    ui.label("Type");
                    egui::ComboBox::from_id_salt("tblstyle").selected_text(t.style.label()).show_ui(ui, |ui| {
                        for st in TableStyle::ALL {
                            ui.selectable_value(&mut t.style, st, st.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Rows");
                    ui.add(egui::DragValue::new(&mut rows).range(1..=MAX_TABLE_ROWS));
                    ui.end_row();
                    ui.label("Columns");
                    ui.add(egui::DragValue::new(&mut cols).range(1..=MAX_TABLE_COLS));
                    ui.end_row();
                    ui.label("Header row");
                    ui.checkbox(&mut t.header_row, "");
                    ui.end_row();
                    ui.label("Header column");
                    ui.checkbox(&mut t.header_col, "");
                    ui.end_row();
                    ui.label("Font");
                    egui::ComboBox::from_id_salt("tblfont").selected_text(t.font.label()).show_ui(ui, |ui| {
                        for f in FontChoice::ALL {
                            ui.selectable_value(&mut t.font, f, f.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Size");
                    ui.add(egui::DragValue::new(&mut t.size).range(6.0..=120.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Cell padding");
                    ui.add(egui::DragValue::new(&mut t.padding).range(0.0..=80.0).suffix(" px"));
                    ui.end_row();
                    ui.label("Align");
                    ui.horizontal(|ui| {
                        for a in Align::ALL {
                            ui.selectable_value(&mut t.align, a, a.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Text color");
                    ui.color_edit_button_srgb(&mut t.color.0);
                    ui.end_row();
                    ui.label("Background");
                    ui.color_edit_button_srgb(&mut t.background.0);
                    ui.end_row();
                    if t.header_filled() {
                        ui.label("Header fill");
                        ui.color_edit_button_srgb(&mut t.header_bg.0);
                        ui.end_row();
                        ui.label("Header text");
                        ui.color_edit_button_srgb(&mut t.header_color.0);
                        ui.end_row();
                    }
                    ui.label("Border");
                    ui.color_edit_button_srgb(&mut t.border.0);
                    ui.end_row();
                    if t.style == TableStyle::Striped {
                        ui.label("Stripe");
                        ui.color_edit_button_srgb(&mut t.stripe.0);
                        ui.end_row();
                    }
                });
                if (rows, cols) != (t.rows(), t.cols()) {
                    t.resize(rows, cols);
                }
                ui.add_space(4.0);
                ui.label("Cells");
                egui::ScrollArea::both().id_salt(("tbl_scroll", el.id)).max_height(240.0).show(ui, |ui| {
                    egui::Grid::new(("tbl_cells", el.id)).spacing([4.0, 4.0]).show(ui, |ui| {
                        for row in t.cells.iter_mut() {
                            for cell in row.iter_mut() {
                                ui.add(egui::TextEdit::singleline(cell).desired_width(90.0));
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.weak("Table height is a minimum: rows grow to fit their text on the published page.");
            }
            Kind::Embed { url, title } => {
                ui.label("Address");
                ui.add(
                    egui::TextEdit::singleline(url)
                        .desired_width(f32::INFINITY)
                        .hint_text("https://www.youtube.com/watch?v=..."),
                );
                ui.label("Title (read by screen readers)");
                ui.add(egui::TextEdit::singleline(title).desired_width(f32::INFINITY));
                match security::normalize_embed(url) {
                    Ok(u) => {
                        ui.weak(format!("Embeds: {u}"));
                    }
                    Err(e) if url.trim().is_empty() => {
                        ui.weak(e);
                    }
                    Err(e) => {
                        ui.colored_label(Color32::from_rgb(200, 60, 60), format!("Not allowed: {e}"));
                    }
                }
                ui.weak("You can also paste the <iframe> code that YouTube, Google Maps or Google Forms provide.");
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
        if !matches!(el.kind, Kind::Nav(_) | Kind::Table(_) | Kind::Embed { .. }) {
            ui.add_space(4.0);
            ui.label("Link (URL or page)");
            ui.add(egui::TextEdit::singleline(&mut el.link).desired_width(f32::INFINITY).hint_text("https://..."));
            if !el.link.trim().is_empty() {
                if let Err(e) = security::safe_href(&el.link) {
                    ui.colored_label(Color32::from_rgb(200, 60, 60), format!("Not published: {e}"));
                }
            }
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
        ui.add_space(4.0);
        ui.label("Align to page");
        ui.horizontal_wrapped(|ui| {
            let buttons = [
                ("Left", AlignTo::Left),
                ("Center", AlignTo::HCenter),
                ("Right", AlignTo::Right),
                ("Top", AlignTo::Top),
                ("Middle", AlignTo::VMiddle),
                ("Bottom", AlignTo::Bottom),
            ];
            for (label, to) in buttons {
                if ui.small_button(label).clicked() {
                    act = Some(ElemAction::Align(to));
                }
            }
        });
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

    /// Floating toolbar above the text box being edited on the page.
    fn edit_bar(&mut self, ctx: &egui::Context, p: &mut Project) {
        let (Some(editing), Some(rect), Sel::Element(rid, _)) = (self.editing, self.edit_rect, self.sel) else {
            self.bar_rect = None;
            return;
        };
        let top = rect.min.y - 40.0;
        let pos = if top > ctx.screen_rect().min.y + 70.0 {
            pos2(rect.min.x, top)
        } else {
            pos2(rect.min.x, rect.max.y + 8.0)
        };
        let mut picked = None;
        let out =
            egui::Area::new(egui::Id::new("edit_bar")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| picked = edit_tool_buttons(ui));
                });
            });
        self.bar_rect = Some(out.response.rect);
        if let Some(tool) = picked {
            let page = self.page;
            if let Some(el) = p.region_mut(rid, page).elements.iter_mut().find(|e| e.id == editing) {
                if let Kind::Text(t) = &mut el.kind {
                    run_edit_tool(ctx, egui::Id::new(("edit", editing)), &mut t.text, tool);
                }
            }
        }
    }

    fn table_dialog_ui(&mut self, ctx: &egui::Context, p: &mut Project) {
        let mut create = None;
        let mut cancel = false;
        if let Some(d) = &mut self.table_dialog {
            egui::Window::new("Insert table")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("table_dialog").num_columns(2).show(ui, |ui| {
                        ui.label("Type");
                        egui::ComboBox::from_id_salt("dlg_style").selected_text(d.style.label()).show_ui(ui, |ui| {
                            for st in TableStyle::ALL {
                                ui.selectable_value(&mut d.style, st, st.label());
                            }
                        });
                        ui.end_row();
                        ui.label("Rows");
                        ui.add(egui::DragValue::new(&mut d.rows).range(1..=MAX_TABLE_ROWS));
                        ui.end_row();
                        ui.label("Columns");
                        ui.add(egui::DragValue::new(&mut d.cols).range(1..=MAX_TABLE_COLS));
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Insert").clicked() {
                            create = Some((d.rows, d.cols, d.style));
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        }
        if let Some((rows, cols, style)) = create {
            self.table_dialog = None;
            self.add_table(p, TableProps::new(rows, cols, style));
        } else if cancel {
            self.table_dialog = None;
        }
    }

    fn welcome(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.15).max(8.0));
                egui::Frame::new()
                    .fill(LOGO_BG)
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.add(egui::Image::from_bytes("bytes://willow-logo.png", LOGO_PNG).max_width(340.0));
                    });
                ui.add_space(14.0);
                ui.heading(APP_NAME);
                ui.add_space(6.0);
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

    /// Copy, cut and paste of whole elements while no text field has keyboard focus.
    /// (Inside a focused text field, egui handles these keys on the text itself.)
    fn handle_clipboard_events(&mut self, ctx: &egui::Context, p: &mut Project) {
        let events: Vec<egui::Event> = ctx.input(|i| {
            i.events
                .iter()
                .filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)))
                .cloned()
                .collect()
        });
        for ev in events {
            match ev {
                egui::Event::Copy => self.copy_selected(ctx, p, false),
                egui::Event::Cut => self.copy_selected(ctx, p, true),
                egui::Event::Paste(text) => self.paste_text(p, &text),
                _ => {}
            }
        }
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
            self.handle_clipboard_events(ctx, p);
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::D))) {
                self.cmd = Some(Cmd::Duplicate);
            }
            if ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
            }) || ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y)))
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
        let props = egui::SidePanel::right("props").default_width(290.0).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.props_panel(ui, p));
        });
        self.props_rect = Some(props.response.rect);
        if let Some(what) = self.pending_insert.take() {
            self.do_insert(p, what);
            ctx.request_repaint();
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_gray(60))).show(ctx, |ui| {
            self.canvas_ui(ui, p);
        });
        self.edit_bar(ctx, p);
        self.table_dialog_ui(ctx, p);
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

/// The row of text-editing buttons shown above a text box being edited and in the panel.
fn edit_tool_buttons(ui: &mut egui::Ui) -> Option<EditTool> {
    let buttons = [
        ("Cut", EditTool::Cut, "Cut the selected text (Ctrl+X)"),
        ("Copy", EditTool::Copy, "Copy the selected text (Ctrl+C)"),
        ("Paste", EditTool::Paste, "Paste at the cursor (Ctrl+V)"),
        ("Select all", EditTool::SelectAll, "Select all text in this box (Ctrl+A)"),
        ("Bullets", EditTool::Bullets, "Turn the selected lines into bullets, or back into plain lines"),
        ("2nd style", EditTool::Secondary, "Apply the secondary size, font and color to the selected text"),
    ];
    let mut picked = None;
    for (label, tool, hint) in buttons {
        if ui.button(label).on_hover_text(hint).clicked() {
            picked = Some(tool);
        }
    }
    picked
}

/// Runs an editing tool on the text field `widget_id`, using the selection that field remembers.
fn run_edit_tool(ctx: &egui::Context, widget_id: egui::Id, text: &mut String, tool: EditTool) {
    let mut state = egui::TextEdit::load_state(ctx, widget_id).unwrap_or_default();
    let len = text.chars().count();
    let (a, b) = state
        .cursor
        .char_range()
        .map(|r| {
            let [lo, hi] = r.sorted();
            (lo.index.min(len), hi.index.min(len))
        })
        .unwrap_or((len, len));
    let select = |state: &mut egui::text_edit::TextEditState, from: usize, to: usize| {
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(from), CCursor::new(to))));
    };
    match tool {
        EditTool::Copy => {
            if a < b {
                ctx.copy_text(text.chars().skip(a).take(b - a).collect());
            }
        }
        EditTool::Cut => {
            if a < b {
                ctx.copy_text(text.chars().skip(a).take(b - a).collect());
                let mut chars: Vec<char> = text.chars().collect();
                chars.drain(a..b);
                *text = chars.into_iter().collect();
                select(&mut state, a, a);
            }
        }
        // The pasted text arrives as an event on the next frame, once the field has focus again.
        EditTool::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
        EditTool::SelectAll => select(&mut state, 0, len),
        EditTool::Bullets => {
            let (t, from, to) = richtext::toggle_bullets(text, a, b);
            *text = t;
            select(&mut state, from, to);
        }
        EditTool::Secondary => {
            let (t, from, to) = richtext::toggle_alt(text, a, b);
            *text = t;
            select(&mut state, from, to);
        }
    }
    state.store(ctx, widget_id);
    ctx.memory_mut(|m| m.request_focus(widget_id));
    ctx.request_repaint();
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
            self.run_cmd(ctx, cmd, &mut proj);
        }

        if let Some(p) = proj.as_ref() {
            self.track_changes(ctx, p);
        }

        // Window title with unsaved marker.
        let title = match proj.as_ref() {
            Some(p) => format!("{}{} - {APP_NAME}", p.site.title, if self.is_dirty(p) { " *" } else { "" }),
            None => APP_NAME.to_string(),
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

#[cfg(test)]
mod tests {
    //! Headless UI tests: the real canvas code runs inside an `egui::Context` that is fed
    //! synthetic pointer and clipboard events, so interaction logic is checked without a window.

    use super::*;
    use egui::{Event, PointerButton, RawInput};

    const SCREEN: Vec2 = vec2(1400.0, 1300.0);

    struct Harness {
        ctx: egui::Context,
        app: App,
        p: Project,
        time: f64,
        mods: Modifiers,
        commands: Vec<egui::OutputCommand>,
    }

    impl Harness {
        fn new() -> Harness {
            let mut p = Project::scaffold(&std::env::temp_dir().join("willow_ui_test_unused"));
            p.contents[0].elements.clear();
            let mut app = App::blank();
            app.fit_zoom = false;
            app.zoom = 1.0;
            let mut h =
                Harness { ctx: egui::Context::default(), app, p, time: 0.0, mods: Modifiers::NONE, commands: vec![] };
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.05;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), SCREEN)),
                time: Some(self.time),
                modifiers: self.mods,
                events,
                ..Default::default()
            };
            let (app, p) = (&mut self.app, &mut self.p);
            let out = self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| app.canvas_ui(ui, p));
                app.handle_clipboard_events(ctx, p);
            });
            self.commands.extend(out.platform_output.commands);
        }

        /// One frame of the complete project UI (menus, panels, canvas, clipboard handling).
        fn ui_frame(&mut self, events: Vec<Event>) {
            self.time += 0.05;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), SCREEN)),
                time: Some(self.time),
                modifiers: self.mods,
                events,
                ..Default::default()
            };
            let (app, p) = (&mut self.app, &mut self.p);
            let out = self.ctx.run(raw, |ctx| app.project_ui(ctx, p));
            self.commands.extend(out.platform_output.commands);
        }

        fn add(&mut self, x: f32, y: f32, w: f32, h: f32, kind: Kind) -> u64 {
            let id = self.p.next_id();
            self.p.contents[0].elements.push(Element { id, x, y, w, h, link: String::new(), kind });
            id
        }

        fn add_text(&mut self, x: f32, y: f32, w: f32, h: f32) -> u64 {
            self.add(x, y, w, h, Kind::Text(TextProps::default()))
        }

        fn shape() -> Kind {
            Kind::Shape { color: Rgb::WHITE, radius: 0.0 }
        }

        fn get(&self, id: u64) -> &Element {
            self.p.contents[0].elements.iter().find(|e| e.id == id).expect("element exists")
        }

        /// Screen position of a point given in page-body design coordinates (zoom 1).
        fn body(&self, x: f32, y: f32) -> egui::Pos2 {
            let origin_x = ((SCREEN.x - self.p.site.width) / 2.0).max(28.0);
            let origin_y = 28.0 + self.p.header.height;
            pos2(origin_x + x, origin_y + y)
        }

        fn pointer(&mut self, pos: egui::Pos2, pressed: Option<bool>) {
            let mut ev = vec![Event::PointerMoved(pos)];
            if let Some(pressed) = pressed {
                ev.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: self.mods });
            }
            self.frame(ev);
        }

        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.pointer(from, None);
            self.pointer(from, Some(true));
            let steps = 8;
            for i in 1..=steps {
                self.pointer(from + (to - from) * (i as f32 / steps as f32), None);
            }
            self.pointer(to, Some(false));
            self.frame(vec![]);
        }

        fn selected_element(&mut self, id: u64) {
            self.app.sel = Sel::Element(RegionId::Body, id);
            self.app.active = RegionId::Body;
            self.frame(vec![]);
        }
    }

    #[test]
    fn dragging_moves_the_box_and_snapping_is_off_by_default() {
        let mut h = Harness::new();
        let a = h.add_text(100.0, 200.0, 200.0, 50.0);
        let b = h.add_text(400.0, 200.0, 200.0, 50.0);
        h.frame(vec![]);
        h.drag(h.body(500.0, 225.0), h.body(203.0, 225.0));
        assert_eq!(h.get(b).x, 103.0, "free drag keeps the exact position");
        assert_eq!(h.get(b).y, 200.0);
        assert_eq!(h.get(a).x, 100.0, "other boxes stay put");
    }

    #[test]
    fn text_boxes_snap_to_neighbours_when_snapping_is_on() {
        for (alt, expected) in [(false, 100.0), (true, 103.0)] {
            let mut h = Harness::new();
            h.app.snap_on = true;
            h.mods = if alt { Modifiers::ALT } else { Modifiers::NONE };
            h.add_text(100.0, 200.0, 200.0, 50.0);
            let b = h.add_text(400.0, 200.0, 200.0, 50.0);
            h.frame(vec![]);
            h.drag(h.body(500.0, 225.0), h.body(203.0, 225.0));
            assert_eq!(h.get(b).x, expected, "alt held: {alt}");
        }
    }

    #[test]
    fn other_elements_only_snap_when_asked() {
        for (snap_all, expected) in [(false, 103.0), (true, 100.0)] {
            let mut h = Harness::new();
            h.app.snap_on = true;
            h.app.snap_all = snap_all;
            h.add_text(100.0, 200.0, 200.0, 50.0);
            let b = h.add(400.0, 200.0, 200.0, 50.0, Harness::shape());
            h.frame(vec![]);
            h.drag(h.body(500.0, 225.0), h.body(203.0, 225.0));
            assert_eq!(h.get(b).x, expected, "snap_all: {snap_all}");
        }
    }

    #[test]
    fn snapping_to_the_grid_rounds_positions() {
        let mut h = Harness::new();
        h.app.snap_on = true;
        h.app.snap_objects = false;
        h.app.snap_grid_on = true;
        h.app.grid_size = 25.0;
        let b = h.add_text(400.0, 200.0, 200.0, 50.0);
        h.frame(vec![]);
        // moves the box by (-60, +16); the first step is large enough to register as a drag at once
        h.drag(h.body(500.0, 225.0), h.body(440.0, 241.0));
        let el = h.get(b);
        assert_eq!((el.x % 25.0, el.y % 25.0), (0.0, 0.0), "x={} y={}", el.x, el.y);
        assert_eq!((el.x, el.y), (350.0, 225.0));
    }

    #[test]
    fn resizing_snaps_the_far_edge() {
        let mut h = Harness::new();
        h.app.snap_on = true;
        h.add_text(100.0, 200.0, 200.0, 50.0); // right edge at 300
        let b = h.add_text(100.0, 300.0, 190.0, 50.0); // right edge at 290
        h.selected_element(b);
        let handle = h.body(290.0, 350.0);
        h.drag(handle, handle + vec2(8.0, 0.0)); // raw right edge 298 -> snaps to 300
        assert_eq!(h.get(b).w, 200.0);
        assert_eq!(h.get(b).h, 50.0);
    }

    fn json_of(el: &Element) -> String {
        format!("{CLIP_PREFIX}{}", serde_json::to_string(el).unwrap())
    }

    #[test]
    fn copy_then_paste_recreates_the_element_with_a_new_id() {
        let mut h = Harness::new();
        let t = h.add(100.0, 100.0, 300.0, 60.0, Kind::Table(TableProps::new(2, 2, TableStyle::Striped)));
        h.selected_element(t);
        let (app, p) = (&mut h.app, &mut h.p);
        let out = h.ctx.run(RawInput::default(), |ctx| app.copy_selected(ctx, p, false));
        let copied = out
            .platform_output
            .commands
            .iter()
            .find_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None })
            .expect("copy puts text on the clipboard");
        assert!(copied.starts_with(CLIP_PREFIX));
        assert_eq!(h.p.contents[0].elements.len(), 1, "copy keeps the original");

        h.frame(vec![Event::Paste(copied.clone())]);
        let els = &h.p.contents[0].elements;
        assert_eq!(els.len(), 2);
        assert_ne!(els[1].id, els[0].id);
        assert_eq!(els[1].kind, els[0].kind);
        assert_eq!((els[1].x, els[1].y), (120.0, 120.0), "pasted copy is offset so it is visible");
        h.frame(vec![Event::Paste(copied)]);
        assert_eq!((h.p.contents[0].elements[2].x, h.p.contents[0].elements[2].y), (140.0, 140.0));
    }

    #[test]
    fn cut_removes_the_element_and_paste_brings_it_back() {
        let mut h = Harness::new();
        let t = h.add_text(100.0, 100.0, 300.0, 60.0);
        h.selected_element(t);
        let (app, p) = (&mut h.app, &mut h.p);
        let out = h.ctx.run(RawInput::default(), |ctx| app.copy_selected(ctx, p, true));
        assert!(h.p.contents[0].elements.is_empty());
        let text = out
            .platform_output
            .commands
            .iter()
            .find_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None })
            .unwrap();
        h.frame(vec![Event::Paste(text)]);
        assert_eq!(h.p.contents[0].elements.len(), 1);
    }

    #[test]
    fn pasted_plain_text_becomes_a_text_box() {
        let mut h = Harness::new();
        h.frame(vec![Event::Paste("hello\nworld".into())]);
        let els = &h.p.contents[0].elements;
        assert_eq!(els.len(), 1);
        assert!(matches!(&els[0].kind, Kind::Text(t) if t.text == "hello\nworld"));
        h.frame(vec![Event::Paste("   ".into())]);
        assert_eq!(h.p.contents[0].elements.len(), 1, "whitespace is ignored");
    }

    #[test]
    fn pasted_elements_are_sanitised_and_garbage_is_ignored() {
        let mut h = Harness::new();
        let hostile = Element {
            id: 1,
            x: 1e12,
            y: -50.0,
            w: 1e9,
            h: -1e9,
            link: "javascript:alert(1)".into(),
            kind: Kind::Image { src: "../../etc/passwd".into(), alt: String::new(), fit: Fit::Cover, radius: 0.0 },
        };
        h.frame(vec![Event::Paste(json_of(&hostile))]);
        let el = &h.p.contents[0].elements[0];
        assert!(el.link.is_empty());
        assert!(matches!(&el.kind, Kind::Image { src, .. } if src.is_empty()));
        assert!(el.w <= h.p.site.width && el.x + el.w <= h.p.site.width + 0.5 && el.x >= 0.0 && el.y >= 0.0);

        let before = h.p.contents[0].elements.len();
        h.frame(vec![Event::Paste(format!("{CLIP_PREFIX}{{not json"))]);
        assert_eq!(h.p.contents[0].elements.len(), before, "invalid element data adds nothing");
        assert!(h.app.status.contains("not contain a valid"));
    }

    #[test]
    fn pasting_while_editing_goes_into_the_text_box_not_onto_the_page() {
        let mut h = Harness::new();
        let t = h.add(100.0, 100.0, 300.0, 60.0, Kind::Text(TextProps { text: "abc".into(), ..TextProps::default() }));
        h.app.sel = Sel::Element(RegionId::Body, t);
        h.app.active = RegionId::Body;
        h.app.editing = Some(t);
        h.app.focus_edit = true;
        h.ui_frame(vec![]);
        h.ui_frame(vec![]);
        assert!(h.ctx.wants_keyboard_input(), "the page editor has keyboard focus");
        h.ui_frame(vec![Event::Paste("XYZ".into())]);
        let els = &h.p.contents[0].elements;
        assert_eq!(els.len(), 1, "no new element is created while a text field has focus");
        let Kind::Text(text) = &els[0].kind else { panic!("expected text") };
        assert!(text.text.contains("XYZ") && text.text.len() == 6, "pasted into the text: {:?}", text.text);
    }

    #[test]
    fn editing_tools_change_text_and_selection() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("tool_test");
        let select = |from: usize, to: usize| {
            let mut st = egui::text_edit::TextEditState::default();
            st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(from), CCursor::new(to))));
            st.store(&ctx, id);
        };
        let range = || {
            let st = egui::TextEdit::load_state(&ctx, id).unwrap();
            let [a, b] = st.cursor.char_range().unwrap().sorted();
            (a.index, b.index)
        };

        let mut text = "one\ntwo\nthree".to_string();
        select(0, 7);
        run_edit_tool(&ctx, id, &mut text, EditTool::Bullets);
        assert_eq!((text.as_str(), range()), ("- one\n- two\nthree", (2, 11)));
        run_edit_tool(&ctx, id, &mut text, EditTool::Bullets);
        assert_eq!((text.as_str(), range()), ("one\ntwo\nthree", (0, 7)));

        let mut text = "Big small".to_string();
        select(4, 9);
        run_edit_tool(&ctx, id, &mut text, EditTool::Secondary);
        assert_eq!((text.as_str(), range()), ("Big {{small}}", (6, 11)));
        run_edit_tool(&ctx, id, &mut text, EditTool::Secondary);
        assert_eq!((text.as_str(), range()), ("Big small", (4, 9)));

        let mut text = "cut me out".to_string();
        select(4, 6);
        run_edit_tool(&ctx, id, &mut text, EditTool::Cut);
        assert_eq!((text.as_str(), range()), ("cut  out", (4, 4)));
        run_edit_tool(&ctx, id, &mut text, EditTool::SelectAll);
        assert_eq!(range(), (0, 8));
        run_edit_tool(&ctx, id, &mut text, EditTool::Copy);
        assert_eq!(text, "cut  out", "copy never changes the text");
        select(100, 200); // stale, out-of-range selections are clamped
        run_edit_tool(&ctx, id, &mut text, EditTool::Bullets);
        assert_eq!(text, "- cut  out");
    }

    #[test]
    fn editing_ends_on_an_outside_click_but_not_on_the_tools() {
        let inside = Rect::from_min_size(pos2(300.0, 400.0), vec2(200.0, 50.0));
        let bar = Rect::from_min_size(pos2(300.0, 350.0), vec2(300.0, 30.0));
        let panel = Rect::from_min_size(pos2(1100.0, 0.0), vec2(300.0, 900.0));
        for (click, still_editing) in [
            (pos2(350.0, 420.0), true),  // on the text box
            (pos2(400.0, 360.0), true),  // on the floating toolbar
            (pos2(1200.0, 300.0), true), // in the properties panel
            (pos2(700.0, 700.0), false), // anywhere else
        ] {
            let mut h = Harness::new();
            h.app.editing = Some(42);
            h.app.edit_rect = Some(inside);
            h.app.bar_rect = Some(bar);
            h.app.props_rect = Some(panel);
            h.pointer(click, None);
            h.pointer(click, Some(true));
            assert_eq!(h.app.editing.is_some(), still_editing, "click at {click:?}");
        }
        let mut h = Harness::new();
        h.app.editing = Some(42);
        h.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        assert!(h.app.editing.is_none(), "Escape ends editing");
    }

    #[test]
    fn every_table_preset_and_section_can_be_inserted() {
        let mut h = Harness::new();
        for i in 0..presets::table_presets().len() {
            h.app.do_insert(&mut h.p, Insert::Table(i));
        }
        assert_eq!(h.p.contents[0].elements.len(), presets::table_presets().len());
        assert!(h.p.contents[0].elements.iter().all(|e| matches!(e.kind, Kind::Table(_)) && e.x + e.w <= 1000.0));

        let mut h = Harness::new();
        for s in Section::ALL {
            h.app.do_insert(&mut h.p, Insert::Section(s));
        }
        let region = &h.p.contents[0];
        assert!(region.elements.len() > 10);
        assert!(region.elements.iter().all(|e| e.y + e.h <= region.height), "the page grew to hold every section");
        h.app.do_insert(&mut h.p, Insert::Button);
        h.app.do_insert(&mut h.p, Insert::Embed);
        h.app.do_insert(&mut h.p, Insert::TableCustom);
        assert!(h.app.table_dialog.is_some());
        // inserted content survives a save and a reload
        let root = std::env::temp_dir().join(format!("willow_ui_insert_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        h.p.root = root.clone();
        export::save(&mut h.p).unwrap();
        assert_eq!(Project::load(&root).unwrap().snapshot(), h.p.snapshot());
    }

    #[test]
    fn align_actions_place_elements_on_the_page() {
        let mut h = Harness::new();
        let id = h.add_text(10.0, 10.0, 200.0, 50.0);
        let page = h.p.contents[0].height;
        let cases = [
            (AlignTo::HCenter, 400.0, 10.0),
            (AlignTo::Right, 800.0, 10.0),
            (AlignTo::Left, 0.0, 10.0),
            (AlignTo::VMiddle, 0.0, ((page - 50.0) / 2.0).round()),
            (AlignTo::Bottom, 0.0, page - 50.0),
            (AlignTo::Top, 0.0, 0.0),
        ];
        for (to, x, y) in cases {
            h.app.apply_action(&mut h.p, RegionId::Body, id, ElemAction::Align(to));
            let el = h.get(id);
            let got = (el.x, el.y);
            let want = match to {
                AlignTo::Left | AlignTo::HCenter | AlignTo::Right => (x, got.1),
                _ => (got.0, y),
            };
            assert_eq!(got, want);
        }
    }
}
