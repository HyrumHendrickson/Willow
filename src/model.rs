//! Data model, JSON (de)serialisation and on-disk structure validation.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- colours

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const WHITE: Rgb = Rgb([255, 255, 255]);
    pub const INK: Rgb = Rgb([34, 34, 34]);

    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }

    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]))
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).ok_or_else(|| de::Error::custom(format!("invalid colour '{s}'")))
    }
}

// ---------------------------------------------------------------- enums

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum FontChoice {
    #[default]
    Sans,
    Serif,
    Mono,
}

impl FontChoice {
    pub const ALL: [FontChoice; 3] = [FontChoice::Sans, FontChoice::Serif, FontChoice::Mono];

    pub fn label(self) -> &'static str {
        match self {
            FontChoice::Sans => "Sans-serif",
            FontChoice::Serif => "Serif",
            FontChoice::Mono => "Monospace",
        }
    }

    pub fn css(self) -> &'static str {
        match self {
            FontChoice::Sans => "system-ui,-apple-system,\"Segoe UI\",Roboto,Helvetica,Arial,sans-serif",
            FontChoice::Serif => "Georgia,\"Times New Roman\",Times,serif",
            FontChoice::Mono => "\"Courier New\",Courier,monospace",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub const ALL: [Align; 3] = [Align::Left, Align::Center, Align::Right];

    pub fn label(self) -> &'static str {
        match self {
            Align::Left => "Left",
            Align::Center => "Center",
            Align::Right => "Right",
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
    pub fn flex(self) -> &'static str {
        match self {
            Align::Left => "flex-start",
            Align::Center => "center",
            Align::Right => "flex-end",
        }
    }
}

/// Semantic HTML tag used for a text element.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    #[default]
    P,
    H1,
    H2,
    H3,
}

impl Tag {
    pub const ALL: [Tag; 4] = [Tag::P, Tag::H1, Tag::H2, Tag::H3];
    pub fn name(self) -> &'static str {
        match self {
            Tag::P => "p",
            Tag::H1 => "h1",
            Tag::H2 => "h2",
            Tag::H3 => "h3",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tag::P => "Paragraph",
            Tag::H1 => "Heading 1",
            Tag::H2 => "Heading 2",
            Tag::H3 => "Heading 3",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    #[default]
    Cover,
    Contain,
}

impl Fit {
    pub fn css(self) -> &'static str {
        match self {
            Fit::Cover => "cover",
            Fit::Contain => "contain",
        }
    }
}

// ---------------------------------------------------------------- elements

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TextProps {
    pub text: String,
    pub font: FontChoice,
    pub size: f32,
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    pub line_height: f32,
    pub tag: Tag,
}

impl Default for TextProps {
    fn default() -> Self {
        TextProps {
            text: "New text".into(),
            font: FontChoice::Sans,
            size: 24.0,
            color: Rgb::INK,
            bold: false,
            italic: false,
            align: Align::Left,
            line_height: 1.4,
            tag: Tag::P,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct NavProps {
    pub font: FontChoice,
    pub size: f32,
    pub color: Rgb,
    pub bold: bool,
    pub gap: f32,
    pub align: Align,
}

impl Default for NavProps {
    fn default() -> Self {
        NavProps {
            font: FontChoice::Sans,
            size: 18.0,
            color: Rgb::INK,
            bold: false,
            gap: 24.0,
            align: Align::Left,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Text(TextProps),
    Image {
        src: String,
        alt: String,
        fit: Fit,
        radius: f32,
    },
    Video {
        src: String,
    },
    Shape {
        color: Rgb,
        radius: f32,
    },
    /// Automatically generated list of links to every page.
    Nav(NavProps),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Element {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub link: String,
    pub kind: Kind,
}

impl Element {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Text(_) => "Text",
            Kind::Image { .. } => "Image",
            Kind::Video { .. } => "Video",
            Kind::Shape { .. } => "Shape",
            Kind::Nav(_) => "Navigation",
        }
    }
}

// ---------------------------------------------------------------- regions

/// A vertical band of absolutely positioned elements (header, footer, or a page body).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Region {
    pub height: f32,
    pub background: Rgb,
    pub elements: Vec<Element>,
}

impl Region {
    pub fn empty(height: f32, background: Rgb) -> Region {
        Region { height, background, elements: Vec::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PageMeta {
    pub title: String,
    /// Folder name. Empty for the home page (which lives in the repository root).
    pub slug: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Site {
    pub title: String,
    /// Design width of every page, in CSS pixels. Narrower screens scale down.
    pub width: f32,
    pub background: Rgb,
    pub next_id: u64,
    pub pages: Vec<PageMeta>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionId {
    Header,
    Body,
    Footer,
}

// ---------------------------------------------------------------- project

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    #[serde(skip)]
    pub root: PathBuf,
    /// Slugs that exist on disk as of the last load/save (used to clean up renamed or deleted pages).
    #[serde(skip)]
    pub on_disk_slugs: Vec<String>,
    pub site: Site,
    pub header: Region,
    pub footer: Region,
    /// Page bodies, parallel to `site.pages`.
    pub contents: Vec<Region>,
}

pub const RESERVED_SLUGS: [&str; 7] = ["header", "footer", "media", "config", "index", "assets", "css"];

pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in title.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

impl Project {
    pub fn next_id(&mut self) -> u64 {
        let id = self.site.next_id;
        self.site.next_id += 1;
        id
    }

    pub fn snapshot(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Restores content from a snapshot string, keeping `root` and `on_disk_slugs`.
    pub fn restore(&mut self, snap: &str) {
        if let Ok(p) = serde_json::from_str::<Project>(snap) {
            self.site = p.site;
            self.header = p.header;
            self.footer = p.footer;
            self.contents = p.contents;
        }
    }

    pub fn region(&self, r: RegionId, page: usize) -> &Region {
        match r {
            RegionId::Header => &self.header,
            RegionId::Footer => &self.footer,
            RegionId::Body => &self.contents[page],
        }
    }

    pub fn region_mut(&mut self, r: RegionId, page: usize) -> &mut Region {
        match r {
            RegionId::Header => &mut self.header,
            RegionId::Footer => &mut self.footer,
            RegionId::Body => &mut self.contents[page],
        }
    }

    pub fn add_page(&mut self, title: &str) -> Result<usize, String> {
        let slug = self.check_slug(title, None)?;
        self.site.pages.push(PageMeta { title: title.trim().to_string(), slug });
        self.contents.push(Region::empty(600.0, Rgb::WHITE));
        Ok(self.site.pages.len() - 1)
    }

    pub fn rename_page(&mut self, idx: usize, title: &str) -> Result<(), String> {
        if idx == 0 {
            return Err("The home page cannot be renamed.".into());
        }
        let slug = self.check_slug(title, Some(idx))?;
        self.site.pages[idx] = PageMeta { title: title.trim().to_string(), slug };
        Ok(())
    }

    pub fn delete_page(&mut self, idx: usize) {
        if idx > 0 && idx < self.site.pages.len() {
            self.site.pages.remove(idx);
            self.contents.remove(idx);
        }
    }

    pub fn move_page(&mut self, idx: usize, up: bool) -> Option<usize> {
        let to = if up { idx.checked_sub(1)? } else { idx + 1 };
        if idx == 0 || to == 0 || to >= self.site.pages.len() {
            return None;
        }
        self.site.pages.swap(idx, to);
        self.contents.swap(idx, to);
        Some(to)
    }

    fn check_slug(&self, title: &str, ignore: Option<usize>) -> Result<String, String> {
        let slug = slugify(title);
        if slug.is_empty() {
            return Err("Page title must contain letters or digits.".into());
        }
        if RESERVED_SLUGS.contains(&slug.as_str()) {
            return Err(format!("'{slug}' is reserved for the site structure."));
        }
        for (i, p) in self.site.pages.iter().enumerate() {
            if Some(i) != ignore && p.slug == slug {
                return Err(format!("A page with folder name '{slug}' already exists."));
            }
        }
        Ok(slug)
    }

    // ------------------------------------------------------------ defaults

    pub fn scaffold(root: &Path) -> Project {
        let mut p = Project {
            root: root.to_path_buf(),
            on_disk_slugs: Vec::new(),
            site: Site {
                title: "My Microschool".into(),
                width: 1000.0,
                background: Rgb::WHITE,
                next_id: 1,
                pages: vec![PageMeta { title: "Home".into(), slug: String::new() }],
            },
            header: Region::empty(80.0, Rgb([31, 58, 95])),
            footer: Region::empty(100.0, Rgb([240, 240, 240])),
            contents: vec![Region::empty(700.0, Rgb::WHITE)],
        };

        let id = p.next_id();
        p.header.elements.push(Element {
            id,
            x: 40.0,
            y: 20.0,
            w: 420.0,
            h: 40.0,
            link: String::new(),
            kind: Kind::Text(TextProps {
                text: "My Microschool".into(),
                font: FontChoice::Serif,
                size: 30.0,
                color: Rgb::WHITE,
                bold: true,
                ..TextProps::default()
            }),
        });
        let id = p.next_id();
        p.header.elements.push(Element {
            id,
            x: 500.0,
            y: 26.0,
            w: 460.0,
            h: 30.0,
            link: String::new(),
            kind: Kind::Nav(NavProps { color: Rgb::WHITE, align: Align::Right, ..NavProps::default() }),
        });

        let id = p.next_id();
        p.footer.elements.push(Element {
            id,
            x: 0.0,
            y: 36.0,
            w: 1000.0,
            h: 30.0,
            link: String::new(),
            kind: Kind::Text(TextProps {
                text: "\u{a9} My Microschool. All rights reserved.".into(),
                size: 15.0,
                color: Rgb([90, 90, 90]),
                align: Align::Center,
                ..TextProps::default()
            }),
        });

        let id = p.next_id();
        p.contents[0].elements.push(Element {
            id,
            x: 100.0,
            y: 120.0,
            w: 800.0,
            h: 70.0,
            link: String::new(),
            kind: Kind::Text(TextProps {
                text: "Welcome to Our Microschool".into(),
                font: FontChoice::Serif,
                size: 52.0,
                bold: true,
                align: Align::Center,
                tag: Tag::H1,
                ..TextProps::default()
            }),
        });
        let id = p.next_id();
        p.contents[0].elements.push(Element {
            id,
            x: 150.0,
            y: 230.0,
            w: 700.0,
            h: 90.0,
            link: String::new(),
            kind: Kind::Text(TextProps {
                text: "Double-click this text to edit it. Drag to move, drag the corner to resize, and use the Add menu for images, video and more.".into(),
                size: 22.0,
                align: Align::Center,
                ..TextProps::default()
            }),
        });
        p
    }

    // ------------------------------------------------------------ disk I/O

    /// Lists everything required by the directory layout that is missing.
    pub fn validate(root: &Path) -> Vec<String> {
        let mut missing = Vec::new();
        let need_file = |rel: &str, missing: &mut Vec<String>| {
            if !root.join(rel).is_file() {
                missing.push(rel.to_string());
            }
        };
        let need_dir = |rel: &str, missing: &mut Vec<String>| {
            if !root.join(rel).is_dir() {
                missing.push(format!("{rel}/"));
            }
        };
        for f in ["index.html", "index.css", "index.js", "config/site.json"] {
            need_file(f, &mut missing);
        }
        for d in ["media", "config", "header", "footer"] {
            need_dir(d, &mut missing);
        }
        for f in ["header/header.html", "header/header.css", "header/header.js"] {
            need_file(f, &mut missing);
        }
        for f in ["footer/footer.html", "footer/footer.css", "footer/footer.js"] {
            need_file(f, &mut missing);
        }
        // Page folders are checked only if the site config is readable.
        if let Ok(site) = read_json::<Site>(&root.join("config/site.json")) {
            for p in site.pages.iter().filter(|p| !p.slug.is_empty()) {
                for f in ["index.html", "index.css", "index.js"] {
                    need_file(&format!("{}/{}", p.slug, f), &mut missing);
                }
            }
        }
        missing
    }

    pub fn load(root: &Path) -> Result<Project, String> {
        let missing = Project::validate(root);
        if !missing.is_empty() {
            return Err(format!(
                "This folder is not a site created by this editor. Missing:\n  {}",
                missing.join("\n  ")
            ));
        }
        let site: Site = read_json(&root.join("config/site.json"))?;
        if site.pages.is_empty() || !site.pages[0].slug.is_empty() {
            return Err("config/site.json: the first page must be the home page (empty slug).".into());
        }
        let header = read_json_or(&root.join("config/header.json"), Region::empty(80.0, Rgb::WHITE))?;
        let footer = read_json_or(&root.join("config/footer.json"), Region::empty(100.0, Rgb::WHITE))?;
        let mut contents = Vec::new();
        for p in &site.pages {
            let path = if p.slug.is_empty() {
                root.join("config/home.json")
            } else {
                root.join("config/pages").join(format!("{}.json", p.slug))
            };
            contents.push(read_json_or(&path, Region::empty(600.0, Rgb::WHITE))?);
        }
        let on_disk_slugs = site.pages.iter().map(|p| p.slug.clone()).collect();
        Ok(Project { root: root.to_path_buf(), on_disk_slugs, site, header, footer, contents })
    }

    pub fn media_files(&self) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(self.root.join("media"))
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| !n.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by_key(|s| s.to_lowercase());
        out
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let s = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_json_or<T: for<'de> Deserialize<'de>>(path: &Path, default: T) -> Result<T, String> {
    if path.is_file() {
        read_json(path)
    } else {
        Ok(default)
    }
}
