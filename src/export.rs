//! Generates the static site (HTML/CSS/JS/JSON) from a `Project`.
//!
//! * HTML files are fully regenerated on every save.
//! * CSS and JS files contain a generated block between marker comments; anything the
//!   author writes outside the markers is preserved.
//! * JSON in `config/` is the editable source of truth that the editor re-opens.

use crate::model::*;
use crate::richtext::{self, Line, Span};
use crate::security;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const BEGIN: &str = "/* BEGIN GENERATED - managed by Willow; edit outside these markers */";
/// Marker written by versions before the project was renamed. Still recognised so that
/// custom code next to an old generated block is preserved.
const LEGACY_BEGIN: &str = "/* BEGIN GENERATED - managed by Site Builder; edit outside these markers */";
const END: &str = "/* END GENERATED */";

fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn href_for(p: &PageMeta) -> String {
    if p.slug.is_empty() {
        "./".to_string()
    } else {
        format!("{}/", p.slug)
    }
}

// ------------------------------------------------------------------ file helpers

/// Writes to a temporary sibling and renames it into place, so a crash or full disk
/// can never leave a half-written file behind.
fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".willow-tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn write_if_changed(path: &Path, content: &str) -> io::Result<()> {
    if fs::read_to_string(path).map(|s| s == content).unwrap_or(false) {
        return Ok(());
    }
    write_atomic(path, content)
}

/// Writes `generated` between marker comments, preserving any surrounding user code.
fn write_managed(path: &Path, generated: &str) -> io::Result<()> {
    let block = format!("{BEGIN}\n{generated}\n{END}\n");
    let content = match fs::read_to_string(path) {
        Ok(existing) => {
            let begin = existing.find(BEGIN).or_else(|| existing.find(LEGACY_BEGIN));
            match (begin, existing.find(END)) {
                (Some(b), Some(e)) if b < e => {
                    let after = e + END.len();
                    let tail = existing[after..].trim_start_matches('\n');
                    format!("{}{}{}", &existing[..b], block, tail)
                }
                _ if existing.trim().is_empty() => block,
                // Foreign file without markers: keep its content below the generated block.
                _ => format!("{block}\n{existing}"),
            }
        }
        Err(_) => block,
    };
    write_if_changed(path, &content)
}

fn write_json<T: serde::Serialize>(path: &Path, v: &T) -> io::Result<()> {
    let s = serde_json::to_string_pretty(v).map_err(io::Error::other)?;
    write_if_changed(path, &(s + "\n"))
}

// ------------------------------------------------------------------ element output

/// ` href="..."` (plus `rel` for external links) for a validated link, or `None` when the
/// link is empty or unsafe. Unsafe links (`javascript:` and friends) are never emitted.
fn link_attrs(link: &str) -> Option<String> {
    let href = security::safe_href(link).ok()?;
    let rel = if security::is_external(&href) { " rel=\"noopener noreferrer\"" } else { "" };
    Some(format!(" href=\"{}\"{rel}", esc(&href)))
}

/// ` src="..."` for a validated media path, or nothing.
fn media_attr(src: &str) -> String {
    if security::safe_media_path(src) {
        format!(" src=\"{}\"", esc(src))
    } else {
        String::new()
    }
}

fn spans_html(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| if s.alt { format!("<span class=\"alt\">{}</span>", esc(&s.text)) } else { esc(&s.text) })
        .collect()
}

/// Returns the inner HTML of a text element and whether it needs a block container
/// (true when it contains bullet lists, which cannot live inside `<p>` or `<h1>`).
fn text_inner(t: &TextProps) -> (String, bool) {
    let lines = richtext::parse(&t.text);
    if !lines.iter().any(|l| l.bullet) {
        let inline: Vec<String> = lines.iter().map(|l| spans_html(&l.spans)).collect();
        return (inline.join("\n"), false);
    }
    (blocks_html(&lines), true)
}

fn blocks_html(lines: &[Line]) -> String {
    let mut out = String::new();
    let mut depth: i32 = -1; // -1: no list open, otherwise nesting depth of the open <ul>
    let close_all = |out: &mut String, depth: &mut i32| {
        if *depth >= 0 {
            out.push_str("</li>");
            while *depth > 0 {
                out.push_str("</ul></li>");
                *depth -= 1;
            }
            out.push_str("</ul>");
            *depth = -1;
        }
    };
    for line in lines {
        if line.bullet {
            let level = (line.level as i32).min(depth + 1);
            if depth < 0 {
                out.push_str("<ul>");
                depth = 0;
            } else if level > depth {
                out.push_str("<ul>");
                depth += 1;
            } else {
                out.push_str("</li>");
                while depth > level {
                    out.push_str("</ul></li>");
                    depth -= 1;
                }
            }
            out.push_str("<li>");
            out.push_str(&spans_html(&line.spans));
        } else {
            close_all(&mut out, &mut depth);
            let inner = spans_html(&line.spans);
            out.push_str(&format!("<div class=\"ln\">{}</div>", if inner.is_empty() { "<br>".into() } else { inner }));
        }
    }
    close_all(&mut out, &mut depth);
    out
}

fn table_html(cls: &str, t: &TableProps) -> String {
    let cell =
        |tag: &str, scope: &str, text: &str| format!("<{tag}{scope}>{}</{tag}>", esc(text).replace('\n', "<br>"));
    let mut s = format!("<table class=\"{cls}\">");
    let body_start = usize::from(t.header_row);
    if t.header_row {
        s.push_str("<thead><tr>");
        for text in t.cells.first().into_iter().flatten() {
            s.push_str(&cell("th", " scope=\"col\"", text));
        }
        s.push_str("</tr></thead>");
    }
    s.push_str("<tbody>");
    for row in t.cells.iter().skip(body_start) {
        s.push_str("<tr>");
        for (c, text) in row.iter().enumerate() {
            if t.header_col && c == 0 {
                s.push_str(&cell("th", " scope=\"row\"", text));
            } else {
                s.push_str(&cell("td", "", text));
            }
        }
        s.push_str("</tr>");
    }
    s.push_str("</tbody></table>");
    s
}

fn element_html(e: &Element, site: &Site) -> String {
    let cls = format!("el e{}", e.id);
    let link = link_attrs(&e.link);
    // Wraps non-text elements in an anchor when a valid link is set.
    let linked = |inner: String| -> String {
        match &link {
            None => inner,
            Some(attrs) => format!("<a class=\"link\"{attrs}>{inner}</a>"),
        }
    };
    match &e.kind {
        Kind::Text(t) => {
            let (inner, block) = text_inner(t);
            match &link {
                None => {
                    let tag = if block { "div" } else { t.tag.name() };
                    format!("<{tag} class=\"{cls}\">{inner}</{tag}>")
                }
                Some(attrs) => {
                    let wrap = if block { "div" } else { "span" };
                    format!("<a class=\"{cls} link\"{attrs}><{wrap}>{inner}</{wrap}></a>")
                }
            }
        }
        Kind::Image { src, alt, .. } => {
            linked(format!("<img class=\"{}\"{} alt=\"{}\">", cls, media_attr(src), esc(alt)))
        }
        Kind::Video { src } => {
            linked(format!("<video class=\"{}\"{} controls preload=\"metadata\"></video>", cls, media_attr(src)))
        }
        Kind::Shape { .. } => linked(format!("<div class=\"{}\"></div>", cls)),
        Kind::Nav(_) => {
            let links: String =
                site.pages.iter().map(|p| format!("<a href=\"{}\">{}</a>", esc(&href_for(p)), esc(&p.title))).collect();
            format!("<nav class=\"{cls}\">{links}</nav>")
        }
        Kind::Button(b) => {
            let attrs = link.clone().unwrap_or_else(|| " href=\"#\"".to_string());
            format!("<a class=\"{cls} btn\"{attrs}><span>{}</span></a>", esc(&b.label))
        }
        Kind::Table(t) => table_html(&cls, t),
        Kind::Embed { url, title } => match security::normalize_embed(url) {
            Ok(src) => {
                let title = if title.trim().is_empty() { "Embedded content" } else { title.trim() };
                format!(
                    "<iframe class=\"{cls}\" src=\"{}\" title=\"{}\" loading=\"lazy\" allowfullscreen \
referrerpolicy=\"strict-origin-when-cross-origin\" \
sandbox=\"allow-scripts allow-same-origin allow-popups allow-presentation allow-forms\"></iframe>",
                    esc(&src),
                    esc(title)
                )
            }
            Err(_) => format!("<div class=\"{cls}\"></div>"),
        },
    }
}

fn table_css(id: u64, t: &TableProps) -> String {
    let filled = t.header_filled();
    let b = t.border.hex();
    let (collapse, outer) = match t.style {
        TableStyle::Card => ("separate", format!("border:1px solid {b};border-radius:8px;overflow:hidden;")),
        _ => ("collapse", String::new()),
    };
    let mut s = format!(
        "border-collapse:{collapse};border-spacing:0;table-layout:fixed;font-family:{};font-size:{}px;color:{};background:{};{outer}",
        t.font.css(),
        num(t.size),
        t.color.hex(),
        t.background.hex(),
    );
    s += "}\n";
    let cell_border = match t.style {
        TableStyle::Grid => format!("border:1px solid {b};"),
        TableStyle::Lines | TableStyle::Card => format!("border-bottom:1px solid {b};"),
        TableStyle::Striped | TableStyle::Borderless => String::new(),
    };
    s += &format!(
        ".e{id} th,.e{id} td{{padding:{}px;text-align:{};vertical-align:middle;overflow-wrap:anywhere;{cell_border}}}\n",
        num(t.padding),
        t.align.css()
    );
    s += &format!(".e{id} th{{font-weight:700}}\n");
    if filled {
        let (hb, hc) = (t.header_bg.hex(), t.header_color.hex());
        if t.header_row {
            s += &format!(".e{id} thead th{{background:{hb};color:{hc}}}\n");
        }
        if t.header_col {
            s += &format!(".e{id} tbody th{{background:{hb};color:{hc};text-align:left}}\n");
        }
    } else if t.header_col {
        s += &format!(".e{id} tbody th{{text-align:left}}\n");
    }
    match t.style {
        TableStyle::Striped => {
            s += &format!(".e{id} tbody tr:nth-child(even) td{{background:{}}}\n", t.stripe.hex());
        }
        TableStyle::Lines => {
            s += &format!(".e{id} thead th{{border-bottom:2px solid {b}}}\n");
        }
        TableStyle::Card => {
            s += &format!(".e{id} tbody tr:last-child td,.e{id} tbody tr:last-child th{{border-bottom:0}}\n");
        }
        _ => {}
    }
    s
}

fn element_css(e: &Element) -> String {
    let mut s = format!(
        ".e{id}{{left:{}px;top:{}px;width:{}px;height:{}px;",
        num(e.x),
        num(e.y),
        num(e.w),
        num(e.h),
        id = e.id
    );
    let mut extra = String::new();
    match &e.kind {
        Kind::Text(t) => {
            s += &format!(
                "font-family:{};font-size:{}px;color:{};font-weight:{};font-style:{};text-align:{};line-height:{};white-space:pre-wrap;",
                t.font.css(),
                num(t.size),
                t.color.hex(),
                if t.bold { 700 } else { 400 },
                if t.italic { "italic" } else { "normal" },
                t.align.css(),
                num(t.line_height)
            );
            if richtext::has_alt(&t.text) {
                extra += &format!(
                    ".e{} .alt{{font-family:{};font-size:{}px;color:{};font-weight:{};font-style:{}}}\n",
                    e.id,
                    t.alt.font.css(),
                    num(t.alt.size),
                    t.alt.color.hex(),
                    if t.alt.bold { 700 } else { 400 },
                    if t.alt.italic { "italic" } else { "normal" },
                );
            }
        }
        Kind::Image { fit, radius, .. } => {
            s += &format!("object-fit:{};border-radius:{}px;", fit.css(), num(*radius));
        }
        Kind::Video { .. } => s += "object-fit:contain;background:#000;",
        Kind::Shape { color, radius } => {
            s += &format!("background:{};border-radius:{}px;", color.hex(), num(*radius));
        }
        Kind::Nav(n) => {
            s += &format!(
                "display:flex;align-items:center;gap:{}px;justify-content:{};font-family:{};font-size:{}px;color:{};font-weight:{};",
                num(n.gap),
                n.align.flex(),
                n.font.css(),
                num(n.size),
                n.color.hex(),
                if n.bold { 700 } else { 400 }
            );
            extra += &format!(".e{} a{{color:inherit;text-decoration:none;white-space:nowrap}}\n", e.id);
        }
        Kind::Button(b) => {
            s += &format!(
                "display:flex;align-items:center;justify-content:center;text-align:center;text-decoration:none;font-family:{};font-size:{}px;color:{};background:{};border-radius:{}px;font-weight:{};",
                b.font.css(),
                num(b.size),
                b.color.hex(),
                b.fill.hex(),
                num(b.radius),
                if b.bold { 700 } else { 400 }
            );
            extra += &format!(".e{}:hover{{filter:brightness(.92)}}\n", e.id);
        }
        Kind::Table(t) => {
            // The table rules close the block themselves, so emit them and return.
            let rules = table_css(e.id, t);
            return format!("{s}{rules}");
        }
        Kind::Embed { .. } => s += "border:0;background:#f2f2f2;",
    }
    s += "}\n";
    s + &extra
}

fn canvas_html(r: &Region, site: &Site, indent: &str) -> String {
    let mut s = format!("{indent}<div class=\"canvas\" data-w=\"{}\" data-h=\"{}\">\n", num(site.width), num(r.height));
    for e in &r.elements {
        s += &format!("{indent}  {}\n", element_html(e, site));
    }
    s += &format!("{indent}</div>");
    s
}

fn region_css(selector: &str, r: &Region, site: &Site) -> String {
    let mut s = format!(
        "{sel}{{height:{h}px;background:{bg}}}\n{sel} .canvas{{width:{w}px;height:{h}px}}\n",
        sel = selector,
        h = num(r.height),
        bg = r.background.hex(),
        w = num(site.width)
    );
    for e in &r.elements {
        s += &element_css(e);
    }
    s
}

// ------------------------------------------------------------------ boilerplate

fn base_css(site: &Site) -> String {
    format!(
        "html,body{{margin:0;padding:0}}\n\
body{{background:{};overflow-x:hidden}}\n\
.region{{position:relative;display:block;width:100%;overflow:hidden}}\n\
.canvas{{position:relative;transform-origin:0 0}}\n\
.el{{position:absolute;box-sizing:border-box;margin:0;padding:0;overflow-wrap:break-word}}\n\
a.link{{color:inherit;text-decoration:none}}\n\
a.el>span,a.el>div{{display:block}}\n\
img.el,video.el{{display:block}}\n\
.el ul{{margin:0;padding-left:1.4em}}\n\
.el li{{margin:0;padding:0}}\n\
.el .ln{{min-height:1em}}\n",
        site.background.hex()
    )
}

const PAGE_JS: &str = r#"(function () {
  function fit() {
    document.querySelectorAll('.canvas').forEach(function (c) {
      var w = parseFloat(c.dataset.w), h = parseFloat(c.dataset.h);
      var box = c.parentElement;
      var s = Math.min(1, box.clientWidth / w);
      c.style.transform = 'scale(' + s + ')';
      c.style.marginLeft = Math.max(0, (box.clientWidth - w * s) / 2) + 'px';
      box.style.height = (h * s) + 'px';
    });
  }

  function include(id, dir) {
    var host = document.getElementById(id);
    return fetch(dir + '/' + dir + '.html')
      .then(function (r) { return r.ok ? r.text() : ''; })
      .then(function (html) {
        host.innerHTML = html;
        return new Promise(function (done) {
          var s = document.createElement('script');
          s.src = dir + '/' + dir + '.js';
          s.onload = done;
          s.onerror = done;
          document.body.appendChild(s);
        });
      })
      .catch(function () {});
  }

  window.addEventListener('resize', fit);
  fit();
  Promise.all([include('site-header', 'header'), include('site-footer', 'footer')]).then(fit);
})();"#;

/// Builds the Content-Security-Policy for every page of the site. Only frames from the
/// hosts that the site's embeds actually use are allowed.
fn build_csp(p: &Project) -> String {
    let hosts: BTreeSet<String> = p
        .all_elements()
        .filter_map(|e| match &e.kind {
            Kind::Embed { url, .. } => security::normalize_embed(url).ok().and_then(|u| security::origin_of(&u)),
            _ => None,
        })
        .collect();
    let frames = if hosts.is_empty() { "'none'".to_string() } else { hosts.into_iter().collect::<Vec<_>>().join(" ") };
    format!(
        "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; media-src 'self'; \
font-src 'self'; connect-src 'self'; frame-src {frames}; object-src 'none'; base-uri 'self'; form-action 'none'"
    )
}

fn page_html(site: &Site, page: &PageMeta, body: &Region, csp: Option<&str>) -> String {
    let prefix = if page.slug.is_empty() { String::new() } else { format!("{}/", page.slug) };
    let base = if page.slug.is_empty() { "./" } else { "../" };
    let title =
        if page.slug.is_empty() { esc(&site.title) } else { format!("{} | {}", esc(&page.title), esc(&site.title)) };
    let lang = if security::valid_lang(&site.lang) { site.lang.as_str() } else { "en" };
    let description =
        if page.description.trim().is_empty() { site.description.trim() } else { page.description.trim() };

    let mut head = String::from("  <meta charset=\"utf-8\">\n");
    if let Some(csp) = csp {
        head += &format!("  <meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n", esc(csp));
    }
    head += "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n";
    head += "  <meta name=\"referrer\" content=\"strict-origin-when-cross-origin\">\n";
    head += &format!("  <title>{title}</title>\n  <base href=\"{base}\">\n");
    if !description.is_empty() {
        head += &format!("  <meta name=\"description\" content=\"{}\">\n", esc(description));
        head += &format!("  <meta property=\"og:description\" content=\"{}\">\n", esc(description));
    }
    head += &format!(
        "  <meta property=\"og:title\" content=\"{title}\">\n  <meta property=\"og:type\" content=\"website\">\n"
    );
    if security::safe_media_path(&site.favicon) {
        head += &format!("  <link rel=\"icon\" href=\"{}\">\n", esc(&site.favicon));
    }
    head += &format!(
        "  <link rel=\"stylesheet\" href=\"{prefix}index.css\">\n  <link rel=\"stylesheet\" href=\"header/header.css\">\n  <link rel=\"stylesheet\" href=\"footer/footer.css\">\n"
    );
    format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n{head}</head>\n<body>\n  <header id=\"site-header\" class=\"region\"></header>\n  <main id=\"page\" class=\"region\">\n{canvas}\n  </main>\n  <footer id=\"site-footer\" class=\"region\"></footer>\n  <script src=\"{prefix}index.js\"></script>\n</body>\n</html>\n",
        canvas = canvas_html(body, site, "    "),
    )
}

// ------------------------------------------------------------------ save

pub fn save(p: &mut Project) -> io::Result<()> {
    let root = p.root.clone();
    for d in ["media", "config/pages", "header", "footer"] {
        fs::create_dir_all(root.join(d))?;
    }
    let csp = p.site.strict_csp.then(|| build_csp(p));
    // GitHub Pages: serve the folder as-is instead of running it through Jekyll.
    write_if_changed(&root.join(".nojekyll"), "")?;

    // --- JSON (source of truth)
    write_json(&root.join("config/site.json"), &p.site)?;
    write_json(&root.join("config/header.json"), &p.header)?;
    write_json(&root.join("config/footer.json"), &p.footer)?;
    for (meta, body) in p.site.pages.iter().zip(&p.contents) {
        let path = if meta.slug.is_empty() {
            root.join("config/home.json")
        } else {
            root.join("config/pages").join(format!("{}.json", meta.slug))
        };
        write_json(&path, body)?;
    }

    // --- header / footer
    for (dir, id, region) in [("header", "#site-header", &p.header), ("footer", "#site-footer", &p.footer)] {
        let d = root.join(dir);
        write_if_changed(&d.join(format!("{dir}.html")), &(canvas_html(region, &p.site, "") + "\n"))?;
        write_managed(&d.join(format!("{dir}.css")), region_css(id, region, &p.site).trim_end())?;
        write_managed(
            &d.join(format!("{dir}.js")),
            &format!("// {dir} script: add custom behaviour outside the generated markers."),
        )?;
    }

    // --- pages
    for (meta, body) in p.site.pages.iter().zip(&p.contents) {
        let dir = if meta.slug.is_empty() { root.clone() } else { root.join(&meta.slug) };
        fs::create_dir_all(&dir)?;
        write_if_changed(&dir.join("index.html"), &page_html(&p.site, meta, body, csp.as_deref()))?;
        let css = format!("{}{}", base_css(&p.site), region_css("#page", body, &p.site));
        write_managed(&dir.join("index.css"), css.trim_end())?;
        write_managed(&dir.join("index.js"), PAGE_JS)?;
    }

    // --- remove generated files of pages that were deleted or renamed
    let current: Vec<&str> = p.site.pages.iter().map(|m| m.slug.as_str()).collect();
    for old in p.on_disk_slugs.iter().filter(|s| !s.is_empty() && !current.contains(&s.as_str())) {
        let dir = root.join(old);
        for f in ["index.html", "index.css", "index.js"] {
            let _ = fs::remove_file(dir.join(f));
        }
        let _ = fs::remove_dir(&dir); // only succeeds if now empty
        let _ = fs::remove_file(root.join("config/pages").join(format!("{old}.json")));
    }
    p.on_disk_slugs = p.site.pages.iter().map(|m| m.slug.clone()).collect();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sb_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn roundtrip_and_structure() {
        let root = tmp("rt");
        let mut p = Project::scaffold(&root);
        p.add_page("About Us").unwrap();
        p.add_page("Contact").unwrap();
        save(&mut p).unwrap();

        for f in [
            "index.html",
            "index.css",
            "index.js",
            "media",
            "config/site.json",
            "config/home.json",
            "config/pages/about-us.json",
            "header/header.html",
            "header/header.css",
            "header/header.js",
            "footer/footer.html",
            "footer/footer.css",
            "footer/footer.js",
            "about-us/index.html",
            "about-us/index.css",
            "about-us/index.js",
            "contact/index.html",
        ] {
            assert!(root.join(f).exists(), "missing {f}");
        }
        assert!(Project::validate(&root).is_empty());

        let html = fs::read_to_string(root.join("about-us/index.html")).unwrap();
        assert!(html.contains("<base href=\"../\">") && html.contains("href=\"about-us/index.css\""));

        let loaded = Project::load(&root).unwrap();
        assert_eq!(loaded.snapshot(), p.snapshot());

        // user code outside markers survives
        let css = root.join("index.css");
        let mut s = fs::read_to_string(&css).unwrap();
        s.push_str("\n/* my custom */ .x{color:red}\n");
        fs::write(&css, s).unwrap();
        p.site.title = "Changed".into();
        p.header.background = Rgb([1, 2, 3]);
        save(&mut p).unwrap();
        assert!(fs::read_to_string(&css).unwrap().contains("my custom"));
        assert!(fs::read_to_string(root.join("header/header.css")).unwrap().contains("#010203"));

        // rename + delete clean up generated files
        p.rename_page(1, "Our Story").unwrap();
        p.delete_page(2);
        save(&mut p).unwrap();
        assert!(!root.join("about-us").exists());
        assert!(!root.join("contact").exists());
        assert!(root.join("our-story/index.html").exists());
        assert!(!root.join("config/pages/about-us.json").exists());
        assert!(Project::load(&root).is_ok());

        // an incomplete folder is rejected with a list of what is missing
        let bad = tmp("bad");
        fs::write(bad.join("index.html"), "x").unwrap();
        let err = Project::load(&bad).unwrap_err();
        assert!(err.contains("config/site.json") && err.contains("header/header.html"));
        assert!(p.rename_page(0, "x").is_err());
        assert!(p.add_page("Media").is_err());
    }

    // ------------------------------------------------------------ helpers for the tests below

    fn add(p: &mut Project, kind: Kind, link: &str) -> u64 {
        let id = p.next_id();
        p.contents[0].elements.push(Element { id, x: 10.0, y: 10.0, w: 200.0, h: 60.0, link: link.into(), kind });
        id
    }

    fn home_html(p: &mut Project) -> String {
        save(p).unwrap();
        fs::read_to_string(p.root.join("index.html")).unwrap()
    }

    #[test]
    fn text_is_escaped_and_unsafe_links_are_dropped() {
        let root = tmp("escape");
        let mut p = Project::scaffold(&root);
        let evil = TextProps { text: "<script>alert(1)</script> & \"quotes\"".into(), ..TextProps::default() };
        add(&mut p, Kind::Text(evil.clone()), "javascript:alert(1)");
        add(&mut p, Kind::Text(evil), "https://example.com/?a=1&b=\"2\"");
        add(&mut p, Kind::Shape { color: Rgb::WHITE, radius: 0.0 }, "data:text/html,<b>x</b>");
        add(&mut p, Kind::Button(ButtonProps::default()), "vbscript:x");
        add(
            &mut p,
            Kind::Image { src: "../../etc/passwd".into(), alt: "<x>".into(), fit: Fit::Cover, radius: 0.0 },
            "",
        );
        let html = home_html(&mut p);
        assert!(!html.contains("<script>alert"), "script tag must be escaped");
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;quotes&quot;"));
        for bad in ["javascript:", "vbscript:", "data:text/html", "etc/passwd"] {
            assert!(!html.contains(bad), "{bad} leaked into the page");
        }
        assert!(html.contains("href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\" rel=\"noopener noreferrer\""));
        // a button without a valid link still renders, pointing nowhere
        assert!(html.contains("class=\"el e") && html.contains("btn\" href=\"#\""));
    }

    #[test]
    fn bullets_and_secondary_style_are_exported() {
        let root = tmp("rich");
        let mut p = Project::scaffold(&root);
        let t = TextProps {
            text: "Intro {{small}} text\n- one\n  - nested <b>\n- two\nafter".into(),
            alt: AltStyle { font: FontChoice::Serif, size: 13.0, color: Rgb([1, 2, 3]), bold: true, italic: false },
            ..TextProps::default()
        };
        let id = add(&mut p, Kind::Text(t), "");
        let html = home_html(&mut p);
        assert!(html.contains(
            "<div class=\"el e{id}\"><div class=\"ln\">Intro <span class=\"alt\">small</span> text</div>\
<ul><li>one<ul><li>nested &lt;b&gt;</li></ul></li><li>two</li></ul><div class=\"ln\">after</div></div>"
                .replace("{id}", &id.to_string())
                .as_str()
        ));
        let css = fs::read_to_string(root.join("index.css")).unwrap();
        assert!(
            css.contains(&format!(".e{id} .alt{{font-family:Georgia"))
                && css.contains("font-size:13px;color:#010203;font-weight:700")
        );

        // plain text keeps its original, heading-safe markup
        let mut p2 = Project::scaffold(&tmp("rich2"));
        let h = TextProps { text: "Title\nline two".into(), tag: Tag::H1, ..TextProps::default() };
        let id2 = add(&mut p2, Kind::Text(h), "");
        assert!(home_html(&mut p2).contains(&format!("<h1 class=\"el e{id2}\">Title\nline two</h1>")));
    }

    #[test]
    fn tables_export_every_style() {
        let root = tmp("tables");
        let mut p = Project::scaffold(&root);
        let mut ids = Vec::new();
        for style in TableStyle::ALL {
            let mut t = TableProps::new(3, 2, style);
            t.header_col = true;
            t.cells[1][1] = "a < b\nsecond line".into();
            ids.push((style, add(&mut p, Kind::Table(t), "")));
        }
        let html = home_html(&mut p);
        assert!(
            html.contains("<thead><tr><th scope=\"col\">Heading 1</th><th scope=\"col\">Heading 2</th></tr></thead>")
        );
        assert!(html.contains("<tbody><tr><th scope=\"row\">Text</th><td>a &lt; b<br>second line</td></tr>"));
        let css = fs::read_to_string(root.join("index.css")).unwrap();
        for (style, id) in ids {
            assert!(css.contains(&format!(".e{id}{{left:10px")), "{style:?}");
            let rules: String = css.lines().filter(|l| l.starts_with(&format!(".e{id} "))).collect();
            match style {
                TableStyle::Grid => assert!(rules.contains("border:1px solid #d0d5dd")),
                TableStyle::Striped => assert!(rules.contains("nth-child(even) td{background:#f3f5f8}")),
                TableStyle::Lines => assert!(rules.contains("border-bottom:2px solid")),
                TableStyle::Borderless => assert!(!rules.contains("border")),
                TableStyle::Card => assert!(css.contains("border-radius:8px;overflow:hidden")),
            }
        }
    }

    #[test]
    fn embeds_are_sandboxed_and_csp_allows_only_used_hosts() {
        let root = tmp("embed");
        let mut p = Project::scaffold(&root);
        let yt = Kind::Embed { url: "https://youtu.be/dQw4w9WgXcQ".into(), title: "Tour".into() };
        add(&mut p, yt, "");
        add(&mut p, Kind::Embed { url: "https://evil.example/x".into(), title: String::new() }, "");
        let html = home_html(&mut p);
        assert!(html.contains("src=\"https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ\" title=\"Tour\""));
        assert!(
            html.contains("sandbox=\"allow-scripts allow-same-origin allow-popups allow-presentation allow-forms\"")
        );
        assert!(!html.contains("evil.example"), "rejected embeds must not reach the page");
        assert!(html.contains("frame-src https://www.youtube-nocookie.com;"));
        assert!(html.contains("default-src 'none'; script-src 'self'; style-src 'self'"));
        assert!(html.contains("object-src 'none'") && html.contains("form-action 'none'"));

        // no embeds at all: frames are forbidden outright
        let mut p2 = Project::scaffold(&tmp("embed2"));
        assert!(home_html(&mut p2).contains("frame-src 'none';"));
        // the policy can be switched off
        p2.site.strict_csp = false;
        assert!(!home_html(&mut p2).contains("Content-Security-Policy"));
    }

    #[test]
    fn seo_tags_are_written_and_escaped() {
        let root = tmp("seo");
        let mut p = Project::scaffold(&root);
        p.site.description = "Site \"desc\"".into();
        p.site.lang = "pt-BR".into();
        p.site.favicon = "media/icon.png".into();
        p.add_page("About").unwrap();
        p.site.pages[1].description = "About <us>".into();
        save(&mut p).unwrap();
        let home = fs::read_to_string(root.join("index.html")).unwrap();
        let about = fs::read_to_string(root.join("about/index.html")).unwrap();
        assert!(
            home.contains("<html lang=\"pt-BR\">")
                && home.contains("name=\"description\" content=\"Site &quot;desc&quot;\"")
        );
        assert!(home.contains("<link rel=\"icon\" href=\"media/icon.png\">"));
        assert!(about.contains("content=\"About &lt;us&gt;\"") && about.contains("property=\"og:title\""));
        // an invalid language or favicon never reaches the page
        p.site.lang = "en\"><script>".into();
        p.site.favicon = "https://evil.example/x.png".into();
        save(&mut p).unwrap();
        let home = fs::read_to_string(root.join("index.html")).unwrap();
        assert!(home.contains("<html lang=\"en\">") && !home.contains("evil.example") && !home.contains("<script>"));
    }

    #[test]
    fn writes_are_atomic_and_pages_get_nojekyll() {
        let root = tmp("atomic");
        let mut p = Project::scaffold(&root);
        save(&mut p).unwrap();
        assert!(root.join(".nojekyll").is_file());
        let mut leftovers = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(d).unwrap().flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.to_string_lossy().ends_with(".willow-tmp") {
                    leftovers.push(path);
                }
            }
        }
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn code_next_to_a_pre_rename_generated_block_is_kept() {
        let root = tmp("legacy");
        let mut p = Project::scaffold(&root);
        save(&mut p).unwrap();
        let css = root.join("index.css");
        let legacy = format!("{LEGACY_BEGIN}\n.old{{}}\n{END}\n/* mine */ .keep{{color:red}}\n");
        fs::write(&css, legacy).unwrap();
        save(&mut p).unwrap();
        let out = fs::read_to_string(&css).unwrap();
        assert!(out.contains("/* mine */ .keep{color:red}"));
        assert!(out.starts_with(BEGIN) && !out.contains(LEGACY_BEGIN) && !out.contains(".old{}"));
        assert_eq!(out.matches(END).count(), 1);
    }

    /// Removes every key named in `drop` from a JSON document, at any depth.
    fn strip_keys(v: &mut serde_json::Value, drop: &[&str]) {
        match v {
            serde_json::Value::Object(m) => {
                m.retain(|k, _| !drop.contains(&k.as_str()));
                m.values_mut().for_each(|c| strip_keys(c, drop));
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|c| strip_keys(c, drop)),
            _ => {}
        }
    }

    #[test]
    fn sites_saved_by_the_previous_version_still_open() {
        let root = tmp("compat");
        let mut p = Project::scaffold(&root);
        p.add_page("About Us").unwrap();
        save(&mut p).unwrap();
        for f in [
            "config/site.json",
            "config/header.json",
            "config/footer.json",
            "config/home.json",
            "config/pages/about-us.json",
        ] {
            let path = root.join(f);
            let mut v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            strip_keys(&mut v, &["alt", "description", "lang", "favicon", "strict_csp"]);
            fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        }
        let loaded = Project::load(&root).expect("old site files must load");
        assert_eq!(loaded.site.lang, "en");
        assert!(loaded.site.strict_csp, "the policy defaults to on");
        assert_eq!(loaded.site.pages[1].slug, "about-us");
        assert!(loaded
            .header
            .elements
            .iter()
            .any(|e| matches!(&e.kind, Kind::Text(t) if t.alt == AltStyle::default())));
    }

    fn rewrite_json(root: &Path, file: &str, edit: impl FnOnce(&mut serde_json::Value)) {
        let path = root.join(file);
        let mut v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        edit(&mut v);
        fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }

    #[test]
    fn a_tampered_config_cannot_reach_outside_the_site_folder() {
        for bad in ["../escape", "a/b", "..", "Media", "has space", "media", "-lead", ""] {
            let root = tmp("traversal");
            let mut p = Project::scaffold(&root);
            p.add_page("Safe").unwrap();
            save(&mut p).unwrap();
            rewrite_json(&root, "config/site.json", |v| v["pages"][1]["slug"] = bad.into());
            let err = Project::load(&root).expect_err(&format!("slug {bad:?} must be refused"));
            assert!(err.contains("invalid folder name"), "{err}");
        }
        // duplicates are refused as well
        let root = tmp("dupes");
        let mut p = Project::scaffold(&root);
        p.add_page("One").unwrap();
        p.add_page("Two").unwrap();
        save(&mut p).unwrap();
        rewrite_json(&root, "config/site.json", |v| v["pages"][2]["slug"] = "one".into());
        assert!(Project::load(&root).unwrap_err().contains("more than one page"));
    }

    #[test]
    fn loaded_content_is_sanitised() {
        let root = tmp("sanitise");
        let mut p = Project::scaffold(&root);
        // scaffold home page: [0] title, [1] intro; then [2] image, [3] video, [4] table
        let img_id = add(
            &mut p,
            Kind::Image { src: "media/ok.png".into(), alt: String::new(), fit: Fit::Cover, radius: 0.0 },
            "javascript:x",
        );
        let vid_id = add(&mut p, Kind::Video { src: "../../secret.mp4".into() }, "");
        let tbl_id = add(&mut p, Kind::Table(TableProps::new(2, 2, TableStyle::Grid)), "");
        let title_id = p.contents[0].elements[0].id;
        save(&mut p).unwrap();
        rewrite_json(&root, "config/home.json", |v| {
            let els = v["elements"].as_array_mut().unwrap();
            assert_eq!(els.len(), 5);
            els[0]["x"] = serde_json::json!(1e30);
            els[0]["w"] = serde_json::json!(-5);
            els[4]["kind"]["cells"] = serde_json::json!([["only one row", "ragged"], ["short"]]);
            els[3]["id"] = els[2]["id"].clone(); // duplicate id
        });
        let loaded = Project::load(&root).unwrap_or_else(|e| panic!("{e}"));
        let els = &loaded.contents[0].elements;
        let by_pos = |i: usize| &els[i];

        let title = by_pos(0);
        assert_eq!(title.id, title_id);
        assert!(title.x <= 5000.0 && title.w >= 10.0, "geometry is clamped: x={} w={}", title.x, title.w);
        assert!(by_pos(2).link.is_empty(), "unsafe link is removed");
        assert!(matches!(&by_pos(3).kind, Kind::Video { src } if src.is_empty()), "bad media path is cleared");
        let Kind::Table(t) = &by_pos(4).kind else { panic!("expected a table") };
        assert!(t.cells.iter().all(|r| r.len() == t.cols()) && t.rows() == 2, "ragged tables are repaired");

        let mut ids: Vec<u64> = loaded.all_elements().map(|e| e.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "element ids are unique");
        assert!(ids.iter().all(|i| *i < loaded.site.next_id));
        assert_eq!(by_pos(2).id, img_id, "the first holder of an id keeps it");
        assert_ne!(by_pos(3).id, img_id);
        let _ = (vid_id, tbl_id);
    }
}
