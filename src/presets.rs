//! Ready-made tables and page sections for the Insert menu.
//!
//! Sections are built only from ordinary elements, so after inserting one the author can
//! move, restyle or delete every piece individually.

use crate::model::*;

pub struct TablePreset {
    pub name: &'static str,
    pub props: TableProps,
}

fn table(rows: &[&[&str]], style: TableStyle, header_row: bool, header_col: bool) -> TableProps {
    let mut t = TableProps::new(rows.len(), rows.first().map_or(1, |r| r.len()), style);
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            t.cells[r][c] = (*text).to_string();
        }
    }
    t.header_row = header_row;
    t.header_col = header_col;
    t
}

/// The table types offered under Insert > Table.
pub fn table_presets() -> Vec<TablePreset> {
    vec![
        TablePreset { name: "Basic grid", props: TableProps::new(3, 3, TableStyle::Grid) },
        TablePreset {
            name: "Striped list",
            props: table(
                &[&["Item", "Details", "Notes"], &["", "", ""], &["", "", ""], &["", "", ""], &["", "", ""]],
                TableStyle::Striped,
                true,
                false,
            ),
        },
        TablePreset {
            name: "Simple list (horizontal lines)",
            props: table(
                &[&["Topic", "Description"], &["", ""], &["", ""], &["", ""], &["", ""]],
                TableStyle::Lines,
                true,
                false,
            ),
        },
        TablePreset {
            name: "Comparison (header row and column)",
            props: table(
                &[
                    &["", "Option A", "Option B", "Option C"],
                    &["Price", "", "", ""],
                    &["Schedule", "", "", ""],
                    &["Ages", "", "", ""],
                ],
                TableStyle::Grid,
                true,
                true,
            ),
        },
        TablePreset {
            name: "Weekly schedule",
            props: table(
                &[
                    &["Day", "Time", "Activity"],
                    &["Monday", "9:00 AM", ""],
                    &["Tuesday", "9:00 AM", ""],
                    &["Wednesday", "9:00 AM", ""],
                    &["Thursday", "9:00 AM", ""],
                    &["Friday", "9:00 AM", ""],
                ],
                TableStyle::Striped,
                true,
                false,
            ),
        },
        TablePreset {
            name: "Tuition and pricing (rounded card)",
            props: table(
                &[
                    &["Plan", "Price", "Includes"],
                    &["Half day", "$0", "Mornings, Monday to Friday"],
                    &["Full day", "$0", "All day, Monday to Friday"],
                    &["Drop-in", "$0", "Single day"],
                ],
                TableStyle::Card,
                true,
                false,
            ),
        },
        TablePreset {
            name: "Contact details (no borders)",
            props: table(
                &[
                    &["Address", "123 Main Street"],
                    &["Phone", "(555) 010-0000"],
                    &["Email", "hello@example.com"],
                    &["Hours", "Monday to Friday, 8 AM to 3 PM"],
                ],
                TableStyle::Borderless,
                false,
                true,
            ),
        },
    ]
}

/// Default element size for a freshly inserted table, in design pixels.
pub fn table_size(t: &TableProps, site_width: f32) -> (f32, f32) {
    let w = (t.cols() as f32 * 170.0).clamp(300.0, 900.0).min((site_width - 80.0).max(200.0));
    let h = t.rows() as f32 * (t.size * 1.4 + 2.0 * t.padding);
    (w.round(), h.round())
}

// ---------------------------------------------------------------- sections

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Section {
    Hero,
    Cards,
    CallToAction,
    Contact,
}

impl Section {
    pub const ALL: [Section; 4] = [Section::Hero, Section::Cards, Section::CallToAction, Section::Contact];

    pub fn label(self) -> &'static str {
        match self {
            Section::Hero => "Hero banner",
            Section::Cards => "Three feature cards",
            Section::CallToAction => "Call-to-action band",
            Section::Contact => "Contact block",
        }
    }
}

const NAVY: Rgb = Rgb([31, 58, 95]);
const AMBER: Rgb = Rgb([242, 169, 0]);
const PALE: Rgb = Rgb([243, 245, 248]);

fn el(id: u64, x: f32, y: f32, w: f32, h: f32, kind: Kind) -> Element {
    Element { id, x: x.round(), y: y.round(), w: w.round(), h: h.round(), link: String::new(), kind }
}

#[allow(clippy::too_many_arguments)]
fn text(
    id: u64,
    (x, y, w, h): (f32, f32, f32, f32),
    s: &str,
    size: f32,
    color: Rgb,
    align: Align,
    tag: Tag,
    bold: bool,
    serif: bool,
) -> Element {
    let font = if serif { FontChoice::Serif } else { FontChoice::Sans };
    let props = TextProps { text: s.into(), font, size, color, bold, align, tag, ..TextProps::default() };
    el(id, x, y, w, h, Kind::Text(props))
}

fn button(id: u64, (x, y, w, h): (f32, f32, f32, f32), label: &str, fill: Rgb, color: Rgb) -> Element {
    let props = ButtonProps { label: label.into(), fill, color, ..ButtonProps::default() };
    el(id, x, y, w, h, Kind::Button(props))
}

/// Builds a section whose top edge is at `y0`. `heading` is the colour for text that sits
/// directly on the region background. Returns the elements and the section height.
pub fn build_section(
    section: Section,
    width: f32,
    y0: f32,
    heading: Rgb,
    ids: &mut dyn FnMut() -> u64,
) -> (Vec<Element>, f32) {
    let cx = width / 2.0;
    match section {
        Section::Hero => {
            let h = 340.0;
            let v = vec![
                el(ids(), 0.0, y0, width, h, Kind::Shape { color: NAVY, radius: 0.0 }),
                text(
                    ids(),
                    (100.0, y0 + 70.0, width - 200.0, 80.0),
                    "Welcome to our school",
                    52.0,
                    Rgb::WHITE,
                    Align::Center,
                    Tag::H1,
                    true,
                    true,
                ),
                text(
                    ids(),
                    (150.0, y0 + 160.0, width - 300.0, 70.0),
                    "Tell visitors who you are and what makes your school different.",
                    22.0,
                    Rgb([225, 232, 242]),
                    Align::Center,
                    Tag::P,
                    false,
                    false,
                ),
                button(ids(), (cx - 120.0, y0 + 250.0, 240.0, 56.0), "Schedule a visit", AMBER, Rgb::INK),
            ];
            (v, h)
        }
        Section::Cards => {
            let h = 380.0;
            let (margin, gap) = (60.0, 30.0);
            let card_w = (width - 2.0 * margin - 2.0 * gap) / 3.0;
            let mut v = vec![text(
                ids(),
                (100.0, y0, width - 200.0, 50.0),
                "Why families choose us",
                38.0,
                heading,
                Align::Center,
                Tag::H2,
                true,
                true,
            )];
            let cards = [
                ("Small classes", "- Personal attention\n- Mixed-age learning\n- Flexible pace"),
                ("Hands-on projects", "Describe this benefit in a sentence or two."),
                ("Caring community", "Describe this benefit in a sentence or two."),
            ];
            for (i, (title, body)) in cards.iter().enumerate() {
                let x = margin + i as f32 * (card_w + gap);
                v.push(el(ids(), x, y0 + 80.0, card_w, 260.0, Kind::Shape { color: PALE, radius: 14.0 }));
                v.push(text(
                    ids(),
                    (x + 24.0, y0 + 104.0, card_w - 48.0, 36.0),
                    title,
                    24.0,
                    Rgb::INK,
                    Align::Left,
                    Tag::H3,
                    true,
                    false,
                ));
                v.push(text(
                    ids(),
                    (x + 24.0, y0 + 152.0, card_w - 48.0, 160.0),
                    body,
                    18.0,
                    Rgb([70, 70, 70]),
                    Align::Left,
                    Tag::P,
                    false,
                    false,
                ));
            }
            (v, h)
        }
        Section::CallToAction => {
            let h = 200.0;
            let v = vec![
                el(ids(), 0.0, y0, width, h, Kind::Shape { color: AMBER, radius: 0.0 }),
                text(
                    ids(),
                    (100.0, y0 + 36.0, width - 200.0, 56.0),
                    "Ready to learn more?",
                    38.0,
                    Rgb::INK,
                    Align::Center,
                    Tag::H2,
                    true,
                    true,
                ),
                button(ids(), (cx - 120.0, y0 + 112.0, 240.0, 56.0), "Contact us", NAVY, Rgb::WHITE),
            ];
            (v, h)
        }
        Section::Contact => {
            let h = 410.0;
            let mut details = text(
                ids(),
                (100.0, y0 + 70.0, 460.0, 250.0),
                "{{Address}}\n123 Main Street\n\n{{Phone}}\n(555) 010-0000\n\n{{Email}}\nhello@example.com",
                22.0,
                heading,
                Align::Left,
                Tag::P,
                false,
                false,
            );
            if let Kind::Text(t) = &mut details.kind {
                t.alt = AltStyle { font: FontChoice::Sans, size: 15.0, color: NAVY, bold: true, italic: false };
                if heading == Rgb::WHITE {
                    t.alt.color = AMBER;
                }
            }
            let mut mail = button(ids(), (100.0, y0 + 340.0, 220.0, 52.0), "Email us", NAVY, Rgb::WHITE);
            mail.link = "mailto:hello@example.com".into();
            let v = vec![
                text(
                    ids(),
                    (100.0, y0, width - 200.0, 50.0),
                    "Get in touch",
                    38.0,
                    heading,
                    Align::Left,
                    Tag::H2,
                    true,
                    true,
                ),
                details,
                mail,
            ];
            (v, h)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_a_valid_rectangular_table() {
        for p in table_presets() {
            let t = &p.props;
            assert!(t.rows() >= 1 && t.cols() >= 1, "{}", p.name);
            assert!(t.cells.iter().all(|r| r.len() == t.cols()), "{} is ragged", p.name);
            let (w, h) = table_size(t, 1000.0);
            assert!(w >= 300.0 && h > 0.0);
        }
    }

    #[test]
    fn sections_use_unique_ids_and_fit_the_page() {
        for s in Section::ALL {
            let mut n = 0;
            let (els, h) = build_section(s, 1000.0, 20.0, Rgb::INK, &mut || {
                n += 1;
                n
            });
            let mut ids: Vec<u64> = els.iter().map(|e| e.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), els.len(), "{s:?}");
            for e in &els {
                assert!(e.x >= 0.0 && e.x + e.w <= 1000.0, "{s:?}: element exceeds page width");
                assert!(e.y >= 20.0 && e.y + e.h <= 20.0 + h, "{s:?}: element exceeds section height");
            }
        }
    }
}
