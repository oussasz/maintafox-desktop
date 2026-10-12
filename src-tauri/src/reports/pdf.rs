//! PDF engine for reports and exports (Phase 6.6): multi-page A4/A3 documents with Unicode text
//! (accents, dashes, symbols), wrapped paragraphs, tables that continue across pages, bar and
//! Pareto charts drawn as vectors, and "page n / N" footers.
//!
//! Text uses a TrueType font of the operating system, embedded in the PDF as a CID font
//! (Identity-H with a ToUnicode map, so text stays searchable and copyable). No font file is
//! shipped with the application: [`load_system_font`] picks the first installed font of a fixed
//! list whose licence allows embedding, and fails with a clear message when there is none.

use std::collections::BTreeMap;

use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};

use crate::errors::{AppError, AppResult};

/// An embeddable TrueType font (file bytes).
pub struct PdfFont {
    data: Vec<u8>,
    source: String,
}

impl PdfFont {
    /// Wraps font bytes after checking they parse and may be embedded.
    pub fn from_bytes(data: Vec<u8>, source: &str) -> AppResult<Self> {
        let face = ttf_parser::Face::parse(&data, 0)
            .map_err(|e| AppError::ValidationFailed(vec![format!("Font {source} cannot be read: {e}")]))?;
        if matches!(face.permissions(), Some(ttf_parser::Permissions::Restricted)) {
            return Err(AppError::ValidationFailed(vec![format!(
                "Font {source} does not allow embedding."
            )]));
        }
        if face.tables().glyf.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Font {source} is not a TrueType (glyf) font."
            )]));
        }
        Ok(Self {
            data,
            source: source.to_string(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// System fonts tried in order (Windows, Linux, macOS). All are Unicode Latin fonts that allow
/// document embedding.
fn system_font_candidates() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) {
        let fonts = std::path::Path::new(&windir).join("Fonts");
        for f in ["segoeui.ttf", "arial.ttf", "calibri.ttf", "tahoma.ttf"] {
            out.push(fonts.join(f));
        }
    }
    for f in [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
    ] {
        out.push(f.into());
    }
    out
}

/// First usable system font, or an error naming what was looked for.
pub fn load_system_font() -> AppResult<PdfFont> {
    let candidates = system_font_candidates();
    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            if let Ok(font) = PdfFont::from_bytes(data, &path.display().to_string()) {
                return Ok(font);
            }
        }
    }
    Err(AppError::ValidationFailed(vec![format!(
        "No embeddable TrueType font was found for PDF export (looked for: {}).",
        candidates
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect::<Vec<_>>()
            .join(", ")
    )]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperSize {
    A4Portrait,
    A4Landscape,
    A3Landscape,
}

impl PaperSize {
    #[must_use]
    pub fn from_code(code: &str, landscape: bool) -> Self {
        match (code.to_ascii_uppercase().as_str(), landscape) {
            ("A3", _) => Self::A3Landscape,
            (_, true) => Self::A4Landscape,
            _ => Self::A4Portrait,
        }
    }

    fn dims(self) -> (f32, f32) {
        match self {
            Self::A4Portrait => (595.0, 842.0),
            Self::A4Landscape => (842.0, 595.0),
            Self::A3Landscape => (1191.0, 842.0),
        }
    }
}

const MARGIN: f32 = 40.0;
const FOOTER: f32 = 24.0;
const BODY_SIZE: f32 = 10.0;
const LINE: f32 = 1.35;

/// One drawing operation, positioned in page coordinates (origin bottom-left).
#[derive(Debug, Clone)]
enum Op {
    Text {
        x: f32,
        y: f32,
        size: f32,
        text: String,
        rgb: (f32, f32, f32),
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rgb: (f32, f32, f32),
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        rgb: (f32, f32, f32),
    },
}

/// Lays out blocks into pages, then renders the PDF.
pub struct PdfDocument<'f> {
    face: ttf_parser::Face<'f>,
    font: &'f PdfFont,
    paper: PaperSize,
    pages: Vec<Vec<Op>>,
    y: f32,
    footer_label: String,
}

impl<'f> PdfDocument<'f> {
    pub fn new(font: &'f PdfFont, paper: PaperSize, footer_label: &str) -> AppResult<Self> {
        let face = ttf_parser::Face::parse(&font.data, 0)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("font parse: {e}")))?;
        let (_, h) = paper.dims();
        Ok(Self {
            face,
            font,
            paper,
            pages: vec![Vec::new()],
            y: h - MARGIN,
            footer_label: footer_label.to_string(),
        })
    }

    fn width(&self) -> f32 {
        self.paper.dims().0 - 2.0 * MARGIN
    }

    /// Advance width of `text` at `size` points.
    #[must_use]
    pub fn text_width(&self, text: &str, size: f32) -> f32 {
        let upem = f32::from(self.face.units_per_em());
        text.chars()
            .map(|c| {
                let gid = self.face.glyph_index(c).unwrap_or(ttf_parser::GlyphId(0));
                f32::from(self.face.glyph_hor_advance(gid).unwrap_or(0))
            })
            .sum::<f32>()
            * size
            / upem
    }

    fn ops(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().expect("at least one page")
    }

    fn ensure_space(&mut self, needed: f32) {
        if self.y - needed < MARGIN + FOOTER {
            self.pages.push(Vec::new());
            self.y = self.paper.dims().1 - MARGIN;
        }
    }

    /// Splits `text` into lines no wider than `max` (words longer than a line are cut).
    #[must_use]
    pub fn wrap(&self, text: &str, size: f32, max: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split_whitespace() {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if self.text_width(&candidate, size) <= max {
                    line = candidate;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                // A single word wider than the line is cut by characters.
                let mut chunk = String::new();
                for c in word.chars() {
                    let next = format!("{chunk}{c}");
                    if self.text_width(&next, size) > max && !chunk.is_empty() {
                        lines.push(std::mem::take(&mut chunk));
                        chunk.push(c);
                    } else {
                        chunk = next;
                    }
                }
                line = chunk;
            }
            lines.push(line);
        }
        lines
    }

    pub fn heading(&mut self, text: &str) {
        let size = 15.0;
        self.ensure_space(size * 2.0);
        self.y -= size * LINE;
        let y = self.y;
        self.ops().push(Op::Text {
            x: MARGIN,
            y,
            size,
            text: text.to_string(),
            rgb: (0.1, 0.15, 0.3),
        });
        self.y -= 4.0;
    }

    pub fn subheading(&mut self, text: &str) {
        let size = 12.0;
        self.ensure_space(size * 2.5);
        self.y -= size * LINE + 4.0;
        let y = self.y;
        self.ops().push(Op::Text {
            x: MARGIN,
            y,
            size,
            text: text.to_string(),
            rgb: (0.15, 0.15, 0.15),
        });
        let w = self.width();
        self.ops().push(Op::Line {
            x1: MARGIN,
            y1: y - 3.0,
            x2: MARGIN + w,
            y2: y - 3.0,
            width: 0.5,
            rgb: (0.7, 0.7, 0.7),
        });
        self.y -= 6.0;
    }

    pub fn paragraph(&mut self, text: &str) {
        let width = self.width();
        for line in self.wrap(text, BODY_SIZE, width) {
            self.ensure_space(BODY_SIZE * LINE);
            self.y -= BODY_SIZE * LINE;
            let y = self.y;
            self.ops().push(Op::Text {
                x: MARGIN,
                y,
                size: BODY_SIZE,
                text: line,
                rgb: (0.0, 0.0, 0.0),
            });
        }
        self.y -= 4.0;
    }

    /// Table with relative column widths; the header repeats on every page it spans.
    pub fn table(&mut self, headers: &[&str], rows: &[Vec<String>], weights: &[f32]) {
        let total: f32 = weights.iter().sum::<f32>().max(f32::EPSILON);
        let widths: Vec<f32> = weights.iter().map(|w| self.width() * w / total).collect();
        let size = 8.5;
        let pad = 3.0;
        let header_cells: Vec<String> = headers.iter().map(|h| (*h).to_string()).collect();
        let draw_row = |doc: &mut Self, cells: &[String], header: bool| {
            let wrapped: Vec<Vec<String>> = cells
                .iter()
                .zip(&widths)
                .map(|(c, w)| doc.wrap(c, size, w - 2.0 * pad))
                .collect();
            let lines = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let h = lines as f32 * size * LINE + 2.0 * pad;
            let top = doc.y;
            if header {
                let w = doc.width();
                doc.ops().push(Op::Rect {
                    x: MARGIN,
                    y: top - h,
                    w,
                    h,
                    rgb: (0.9, 0.92, 0.96),
                });
            }
            let mut x = MARGIN;
            for (col, cell_lines) in wrapped.iter().enumerate() {
                for (i, l) in cell_lines.iter().enumerate() {
                    doc.ops().push(Op::Text {
                        x: x + pad,
                        y: top - pad - (i as f32 + 1.0) * size * LINE + 2.0,
                        size,
                        text: l.clone(),
                        rgb: (0.0, 0.0, 0.0),
                    });
                }
                x += widths[col];
            }
            let w = doc.width();
            doc.ops().push(Op::Line {
                x1: MARGIN,
                y1: top - h,
                x2: MARGIN + w,
                y2: top - h,
                width: 0.3,
                rgb: (0.75, 0.75, 0.75),
            });
            doc.y -= h;
            h
        };
        let row_height = |doc: &Self, cells: &[String]| {
            let lines = cells
                .iter()
                .zip(&widths)
                .map(|(c, w)| doc.wrap(c, size, w - 2.0 * pad).len())
                .max()
                .unwrap_or(1)
                .max(1);
            lines as f32 * size * LINE + 2.0 * pad
        };
        let header_h = row_height(self, &header_cells);
        self.ensure_space(header_h * 2.0);
        draw_row(self, &header_cells, true);
        for row in rows {
            let h = row_height(self, row);
            if self.y - h < MARGIN + FOOTER {
                self.pages.push(Vec::new());
                self.y = self.paper.dims().1 - MARGIN;
                draw_row(self, &header_cells, true);
            }
            draw_row(self, row, false);
        }
        self.y -= 8.0;
    }

    /// Horizontal bar chart; with `cumulative`, a Pareto line of cumulative share (0–100 %) is
    /// drawn over the bars, with its own scale on the right.
    pub fn bar_chart(&mut self, title: &str, items: &[(String, f64)], unit: &str, cumulative: bool) {
        let bar_h = 12.0;
        let gap = 4.0;
        let label_w = self.width() * 0.32;
        let value_w = 70.0;
        let plot_w = self.width() - label_w - value_w;
        let height = items.len() as f32 * (bar_h + gap) + 30.0;
        self.subheading(title);
        self.ensure_space(height.min(self.paper.dims().1 - 2.0 * MARGIN - FOOTER));
        let max = items.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max).max(f64::EPSILON);
        let total: f64 = items.iter().map(|(_, v)| *v).sum::<f64>().max(f64::EPSILON);
        let mut running = 0.0;
        let mut points: Vec<(f32, f32)> = Vec::new();
        for (label, value) in items {
            if self.y - (bar_h + gap) < MARGIN + FOOTER {
                self.pages.push(Vec::new());
                self.y = self.paper.dims().1 - MARGIN;
                points.clear();
            }
            self.y -= bar_h + gap;
            let y = self.y;
            let shown = self
                .wrap(label, 8.0, label_w - 6.0)
                .into_iter()
                .next()
                .unwrap_or_default();
            self.ops().push(Op::Text {
                x: MARGIN,
                y: y + 2.5,
                size: 8.0,
                text: shown,
                rgb: (0.1, 0.1, 0.1),
            });
            let w = (value / max) as f32 * plot_w;
            self.ops().push(Op::Rect {
                x: MARGIN + label_w,
                y,
                w: w.max(0.5),
                h: bar_h,
                rgb: (0.24, 0.42, 0.75),
            });
            self.ops().push(Op::Text {
                x: MARGIN + label_w + plot_w + 4.0,
                y: y + 2.5,
                size: 8.0,
                text: format!("{} {unit}", format_number(*value)),
                rgb: (0.1, 0.1, 0.1),
            });
            running += value;
            points.push((MARGIN + label_w + (running / total) as f32 * plot_w, y + bar_h / 2.0));
        }
        if cumulative {
            for pair in points.windows(2) {
                self.ops().push(Op::Line {
                    x1: pair[0].0,
                    y1: pair[0].1,
                    x2: pair[1].0,
                    y2: pair[1].1,
                    width: 1.2,
                    rgb: (0.85, 0.35, 0.1),
                });
            }
        }
        self.y -= 10.0;
    }

    /// Renders every page, with "label — page n / N" footers.
    pub fn finish(self) -> AppResult<Vec<u8>> {
        let (w, h) = self.paper.dims();
        let total = self.pages.len();
        let mut used: BTreeMap<u16, char> = BTreeMap::new();
        let encode = |face: &ttf_parser::Face, used: &mut BTreeMap<u16, char>, text: &str| -> Vec<u8> {
            let mut out = Vec::with_capacity(text.len() * 2);
            for c in text.chars() {
                let gid = face.glyph_index(c).map_or(0, |g| g.0);
                if gid != 0 {
                    used.entry(gid).or_insert(c);
                }
                out.extend_from_slice(&gid.to_be_bytes());
            }
            out
        };

        let mut pdf = Pdf::new();
        let catalog = Ref::new(1);
        let pages_id = Ref::new(2);
        let type0 = Ref::new(3);
        let cid = Ref::new(4);
        let descriptor = Ref::new(5);
        let font_file = Ref::new(6);
        let to_unicode = Ref::new(7);
        let mut next = 8;
        let mut page_ids = Vec::with_capacity(total);
        let mut contents = Vec::with_capacity(total);
        for (index, ops) in self.pages.iter().enumerate() {
            let mut c = Content::new();
            let mut all = ops.clone();
            all.push(Op::Text {
                x: MARGIN,
                y: MARGIN - 10.0,
                size: 7.5,
                text: format!("{} — {} / {}", self.footer_label, index + 1, total),
                rgb: (0.45, 0.45, 0.45),
            });
            for op in &all {
                match op {
                    Op::Text { x, y, size, text, rgb } => {
                        let bytes = encode(&self.face, &mut used, text);
                        c.set_fill_rgb(rgb.0, rgb.1, rgb.2);
                        c.begin_text();
                        c.set_font(Name(b"F1"), *size);
                        c.next_line(*x, *y);
                        c.show(Str(&bytes));
                        c.end_text();
                    }
                    Op::Rect { x, y, w, h, rgb } => {
                        c.set_fill_rgb(rgb.0, rgb.1, rgb.2);
                        c.rect(*x, *y, *w, *h);
                        c.fill_nonzero();
                    }
                    Op::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        width,
                        rgb,
                    } => {
                        c.set_stroke_rgb(rgb.0, rgb.1, rgb.2);
                        c.set_line_width(*width);
                        c.move_to(*x1, *y1);
                        c.line_to(*x2, *y2);
                        c.stroke();
                    }
                }
            }
            let page_id = Ref::new(next);
            let content_id = Ref::new(next + 1);
            next += 2;
            page_ids.push(page_id);
            contents.push((page_id, content_id, c.finish()));
        }

        pdf.catalog(catalog).pages(pages_id);
        pdf.pages(pages_id).kids(page_ids.iter().copied()).count(total as i32);
        for (page_id, content_id, data) in &contents {
            let mut page = pdf.page(*page_id);
            page.media_box(Rect::new(0.0, 0.0, w, h))
                .parent(pages_id)
                .contents(*content_id);
            page.resources().fonts().pair(Name(b"F1"), type0);
            page.finish();
            pdf.stream(*content_id, data);
        }

        let upem = f32::from(self.face.units_per_em());
        let scale = |v: f32| v * 1000.0 / upem;
        let base = Name(b"MaintafoxEmbedded");
        pdf.type0_font(type0)
            .base_font(base)
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(cid)
            .to_unicode(to_unicode);
        let mut cid_font = pdf.cid_font(cid);
        cid_font
            .subtype(CidFontType::Type2)
            .base_font(base)
            .system_info(SystemInfo {
                registry: Str(b"Adobe"),
                ordering: Str(b"Identity"),
                supplement: 0,
            })
            .font_descriptor(descriptor)
            .default_width(scale(f32::from(
                self.face.glyph_hor_advance(ttf_parser::GlyphId(0)).unwrap_or(0),
            )))
            .cid_to_gid_map_predefined(Name(b"Identity"));
        {
            let mut widths = cid_font.widths();
            for gid in used.keys() {
                let adv = self.face.glyph_hor_advance(ttf_parser::GlyphId(*gid)).unwrap_or(0);
                widths.consecutive(*gid, [scale(f32::from(adv))]);
            }
        }
        cid_font.finish();
        let bbox = self.face.global_bounding_box();
        pdf.font_descriptor(descriptor)
            .name(base)
            .flags(FontFlags::NON_SYMBOLIC)
            .bbox(Rect::new(
                scale(f32::from(bbox.x_min)),
                scale(f32::from(bbox.y_min)),
                scale(f32::from(bbox.x_max)),
                scale(f32::from(bbox.y_max)),
            ))
            .italic_angle(0.0)
            .ascent(scale(f32::from(self.face.ascender())))
            .descent(scale(f32::from(self.face.descender())))
            .cap_height(scale(f32::from(
                self.face.capital_height().unwrap_or(self.face.ascender()),
            )))
            .stem_v(80.0)
            .font_file2(font_file);
        pdf.stream(font_file, &self.font.data)
            .pair(Name(b"Length1"), self.font.data.len() as i32);
        let mut cmap = UnicodeCmap::new(
            Name(b"Maintafox-UCS"),
            SystemInfo {
                registry: Str(b"Adobe"),
                ordering: Str(b"UCS"),
                supplement: 0,
            },
        );
        for (gid, c) in &used {
            cmap.pair(*gid, *c);
        }
        let cmap_bytes = cmap.finish();
        pdf.cmap(to_unicode, &cmap_bytes);
        Ok(pdf.finish())
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Number with at most 2 decimals and no trailing zeros (locale-neutral, for PDFs).
#[must_use]
pub fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a document only when an embeddable system font exists on this machine; the
    /// environment without fonts is covered by `missing_font_is_a_clear_error`.
    fn with_font(f: impl FnOnce(&PdfFont)) {
        match load_system_font() {
            Ok(font) => f(&font),
            Err(AppError::ValidationFailed(m)) => {
                assert!(m[0].contains("No embeddable TrueType font"), "{m:?}");
            }
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn missing_font_is_a_clear_error() {
        let err = PdfFont::from_bytes(b"not a font".to_vec(), "fake.ttf")
            .err()
            .expect("error");
        assert!(matches!(err, AppError::ValidationFailed(m) if m[0].contains("cannot be read")));
    }

    #[test]
    fn number_format() {
        assert_eq!(format_number(12.0), "12");
        assert_eq!(format_number(12.345), "12.35");
        assert_eq!(format_number(f64::NAN), "—");
    }

    #[test]
    fn multi_page_unicode_document_with_tables_and_charts() {
        with_font(|font| {
            let mut doc = PdfDocument::new(font, PaperSize::A4Portrait, "Maintafox — Rapport d'essai").expect("doc");
            doc.heading("Fiabilité — Pareto des modes de défaillance");
            doc.paragraph("Équipement P-101 : « roulement côté accouplement » — 3 défaillances, coût 12 500 €.");
            let rows: Vec<Vec<String>> = (0..120)
                .map(|i| vec![format!("Mode n°{i}"), format!("{i}"), format!("{} €", i * 10)])
                .collect();
            doc.table(&["Mode", "Nombre", "Coût"], &rows, &[3.0, 1.0, 1.0]);
            let items: Vec<(String, f64)> = (0..8).map(|i| (format!("Défaut {i}"), f64::from(8 - i))).collect();
            doc.bar_chart("Pareto", &items, "h", true);
            assert!(doc.page_count() >= 3, "120 rows span several pages");
            let bytes = doc.finish().expect("pdf");
            assert!(bytes.starts_with(b"%PDF-"));
            let text = String::from_utf8_lossy(&bytes);
            assert!(text.contains("/Identity-H"));
            assert!(text.contains("/ToUnicode"));
            assert!(text.contains("/FontFile2"));
            assert!(text.contains("/Count 3") || text.contains("/Count 4"));
        });
    }

    #[test]
    fn wrapping_respects_width_and_cuts_long_words() {
        with_font(|font| {
            let doc = PdfDocument::new(font, PaperSize::A4Portrait, "t").expect("doc");
            let lines = doc.wrap("aaaa bbbb cccc dddd", 10.0, doc.text_width("aaaa bbbb", 10.0) + 0.1);
            assert_eq!(lines, vec!["aaaa bbbb".to_string(), "cccc dddd".to_string()]);
            let long = "x".repeat(200);
            let cut = doc.wrap(&long, 10.0, 50.0);
            assert!(cut.len() > 1);
            assert!(cut.iter().all(|l| doc.text_width(l, 10.0) <= 50.0 + 0.01));
        });
    }
}
