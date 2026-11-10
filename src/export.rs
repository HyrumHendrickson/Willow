//! Generates the static site (HTML/CSS/JS/JSON) from a `Project`.
//!
//! * HTML files are fully regenerated on every save.
//! * CSS and JS files contain a generated block between marker comments; anything the
//!   author writes outside the markers is preserved.
//! * JSON in `config/` is the editable source of truth that the editor re-opens.

use crate::model::*;
use std::fs;
use std::io;
use std::path::Path;

const BEGIN: &str = "/* BEGIN GENERATED - managed by Site Builder; edit outside these markers */";
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

fn write_if_changed(path: &Path, content: &str) -> io::Result<()> {
    if fs::read_to_string(path).map(|s| s == content).unwrap_or(false) {
        return Ok(());
    }
    fs::write(path, content)
}

/// Writes `generated` between marker comments, preserving any surrounding user code.
fn write_managed(path: &Path, generated: &str) -> io::Result<()> {
    let block = format!("{BEGIN}\n{generated}\n{END}\n");
    let content = match fs::read_to_string(path) {
        Ok(existing) => match (existing.find(BEGIN), existing.find(END)) {
            (Some(b), Some(e)) if b < e => {
                let after = e + END.len();
                let tail = existing[after..].trim_start_matches('\n');
                format!("{}{}{}", &existing[..b], block, tail)
            }
            _ if existing.trim().is_empty() => block,
            // Foreign file without markers: keep its content below the generated block.
            _ => format!("{block}\n{existing}"),
        },
        Err(_) => block,
    };
    write_if_changed(path, &content)
}

fn write_json<T: serde::Serialize>(path: &Path, v: &T) -> io::Result<()> {
    let s = serde_json::to_string_pretty(v).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    write_if_changed(path, &(s + "\n"))
}

// ------------------------------------------------------------------ element output

fn element_html(e: &Element, site: &Site) -> String {
    let cls = format!("el e{}", e.id);
    let link = e.link.trim();
    // Wraps non-text elements in an anchor when a link is set.
    let linked = |inner: String| -> String {
        if link.is_empty() {
            inner
        } else {
            format!("<a class=\"link\" href=\"{}\">{}</a>", esc(link), inner)
        }
    };
    match &e.kind {
        Kind::Text(t) if link.is_empty() => {
            format!("<{0} class=\"{1}\">{2}</{0}>", t.tag.name(), cls, esc(&t.text))
        }
        Kind::Text(t) => {
            format!("<a class=\"{} link\" href=\"{}\"><span>{}</span></a>", cls, esc(link), esc(&t.text))
        }
        Kind::Image { src, alt, .. } => {
            linked(format!("<img class=\"{}\" src=\"{}\" alt=\"{}\">", cls, esc(src), esc(alt)))
        }
        Kind::Video { src } => linked(format!(
            "<video class=\"{}\" src=\"{}\" controls preload=\"metadata\"></video>",
            cls,
            esc(src)
        )),
        Kind::Shape { .. } => linked(format!("<div class=\"{}\"></div>", cls)),
        Kind::Nav(_) => {
            let links: String = site
                .pages
                .iter()
                .map(|p| format!("<a href=\"{}\">{}</a>", esc(&href_for(p)), esc(&p.title)))
                .collect();
            format!("<nav class=\"{cls}\">{links}</nav>")
        }
    }
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
        }
    }
    s += "}\n";
    if matches!(e.kind, Kind::Nav(_)) {
        s += &format!(".e{} a{{color:inherit;text-decoration:none;white-space:nowrap}}\n", e.id);
    }
    s
}

fn canvas_html(r: &Region, site: &Site, indent: &str) -> String {
    let mut s = format!(
        "{indent}<div class=\"canvas\" data-w=\"{}\" data-h=\"{}\">\n",
        num(site.width),
        num(r.height)
    );
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
a.el span{{display:block}}\n\
img.el,video.el{{display:block}}\n",
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

fn page_html(site: &Site, page: &PageMeta, body: &Region) -> String {
    let prefix = if page.slug.is_empty() { String::new() } else { format!("{}/", page.slug) };
    let base = if page.slug.is_empty() { "./" } else { "../" };
    let title = if page.slug.is_empty() {
        esc(&site.title)
    } else {
        format!("{} | {}", esc(&page.title), esc(&site.title))
    };
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  \
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{title}</title>\n  \
<base href=\"{base}\">\n  <link rel=\"stylesheet\" href=\"{prefix}index.css\">\n  \
<link rel=\"stylesheet\" href=\"header/header.css\">\n  <link rel=\"stylesheet\" href=\"footer/footer.css\">\n</head>\n<body>\n  \
<header id=\"site-header\" class=\"region\"></header>\n  <main id=\"page\" class=\"region\">\n{canvas}\n  </main>\n  \
<footer id=\"site-footer\" class=\"region\"></footer>\n  <script src=\"{prefix}index.js\"></script>\n</body>\n</html>\n",
        canvas = canvas_html(body, site, "    "),
    )
}

// ------------------------------------------------------------------ save

pub fn save(p: &mut Project) -> io::Result<()> {
    let root = p.root.clone();
    for d in ["media", "config/pages", "header", "footer"] {
        fs::create_dir_all(root.join(d))?;
    }

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
        write_if_changed(&dir.join("index.html"), &page_html(&p.site, meta, body))?;
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
            "index.html", "index.css", "index.js", "media", "config/site.json", "config/home.json",
            "config/pages/about-us.json", "header/header.html", "header/header.css", "header/header.js",
            "footer/footer.html", "footer/footer.css", "footer/footer.js", "about-us/index.html",
            "about-us/index.css", "about-us/index.js", "contact/index.html",
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
}
