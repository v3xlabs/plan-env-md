//! A drawn stand-in for a revision's thumbnail, built from the layout of the
//! rendered page rather than from its pixels: the page's own surfaces, rules,
//! media colours and text lines, in the page's own colours, with the headings
//! kept as real text.
//!
//! The walk runs inside the document, so its result is as untrusted as the
//! document. Every value is checked here and the markup is written here; a
//! box that fails a check is dropped and the rest still draw.

use std::fmt::{self, Display, Write};

use serde::Deserialize;

/// The function the worker calls in the settled page. It returns the layout as
/// data, never as markup.
pub const WALKER: &str = include_str!("placeholder.js");

const MAX_BOXES: usize = 1500;
const MAX_GLYPHS: u32 = 400;
const MAX_STOPS: usize = 16;
const MAX_HEADING_CHARS: usize = 200;
/// Beyond this a coordinate is not on a 1280 wide viewport; it is a forged one.
const MAX_COORD: f64 = 10_000.0;
/// An icon whose pixels could not be read still reads as a mark in its ink.
const ICON_MAX: f64 = 40.0;
const FAMILY: &str = "system-ui, -apple-system, Segoe UI, sans-serif";

#[derive(Deserialize)]
struct Layout {
    width: f64,
    height: f64,
    canvas: Color,
    boxes: Vec<serde_json::Value>,
}

/// `#rrggbb` and nothing else, so a colour can go into an attribute as is.
#[derive(Deserialize)]
#[serde(try_from = "String")]
struct Color(String);

impl TryFrom<String> for Color {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let is_hex = value.len() == 7
            && value.starts_with('#')
            && value[1..].bytes().all(|b| b.is_ascii_hexdigit());
        if is_hex {
            Ok(Self(value))
        } else {
            Err("not a #rrggbb colour")
        }
    }
}

impl Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Deserialize)]
struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Frame {
    fn is_plausible(&self) -> bool {
        [self.x, self.y].iter().all(|v| v.abs() <= MAX_COORD)
            && [self.w, self.h].iter().all(|v| *v > 0.0 && *v <= MAX_COORD)
    }
}

#[derive(Deserialize)]
struct Paint {
    color: Color,
    alpha: f64,
}

#[derive(Deserialize)]
struct Gradient {
    angle: f64,
    stops: Vec<Paint>,
}

/// Average colours of an image in reading order, `None` where it is clear.
#[derive(Deserialize)]
struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Option<Paint>>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum GlyphKind {
    Shade1,
    Shade2,
    Shade3,
    Solid,
    Hline,
    Vline,
    Cross,
}

#[derive(Deserialize)]
struct Line {
    #[serde(flatten)]
    frame: Frame,
    size: f64,
    color: Color,
    weight: f64,
    gradient: Option<Gradient>,
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Shape {
    Surface {
        #[serde(flatten)]
        frame: Frame,
        radius: f64,
        fill: Option<Color>,
        #[serde(rename = "fillAlpha")]
        fill_alpha: f64,
        gradient: Option<Gradient>,
        stroke: Option<Color>,
        #[serde(rename = "strokeWidth")]
        stroke_width: f64,
    },
    Rule {
        #[serde(flatten)]
        frame: Frame,
        color: Color,
        alpha: f64,
    },
    Media {
        #[serde(flatten)]
        frame: Frame,
        radius: f64,
        tint: Color,
        grid: Option<Grid>,
    },
    Glyph {
        #[serde(flatten)]
        frame: Frame,
        glyph: GlyphKind,
        size: f64,
        color: Color,
        count: u32,
    },
    Heading(Line),
    Text(Line),
    Link(Line),
    Code(Line),
}

impl Shape {
    fn frame(&self) -> &Frame {
        match self {
            Self::Surface { frame, .. }
            | Self::Rule { frame, .. }
            | Self::Media { frame, .. }
            | Self::Glyph { frame, .. } => frame,
            Self::Heading(line) | Self::Text(line) | Self::Link(line) | Self::Code(line) => {
                &line.frame
            }
        }
    }
}

/// A coordinate at the precision a thumbnail can show.
struct Num(f64);

impl Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if (self.0 - self.0.round()).abs() < 0.05 {
            write!(f, "{:.0}", self.0)
        } else {
            write!(f, "{:.1}", self.0)
        }
    }
}

/// Builds the placeholder SVG from the walker's result. Fails only when the
/// frame of the picture itself is unusable; a bad box is skipped.
pub fn render(layout: serde_json::Value) -> Result<String, String> {
    let layout: Layout = serde_json::from_value(layout).map_err(|e| e.to_string())?;
    let size_ok = |v: f64| v > 0.0 && v <= MAX_COORD;
    if !size_ok(layout.width) || !size_ok(layout.height) {
        return Err("the viewport size is not plausible".to_string());
    }

    let mut svg = Svg(String::with_capacity(16 * 1024));
    let (width, height) = (Num(layout.width), Num(layout.height));
    // slice rather than meet: a thumbnail box of another shape crops the foot
    // of the page, never the top where the title sits
    svg.raw(format_args!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" preserveAspectRatio="xMidYMin slice">"#
    ));
    svg.rect(
        (0.0, 0.0, layout.width, layout.height),
        Fill::Color(&layout.canvas),
        1.0,
        0.0,
    );

    for (index, value) in layout.boxes.into_iter().take(MAX_BOXES).enumerate() {
        let Ok(shape) = serde_json::from_value::<Shape>(value) else {
            continue;
        };
        if shape.frame().is_plausible() {
            svg.shape(&shape, index);
        }
    }

    svg.raw(format_args!("</svg>"));
    Ok(svg.0)
}

enum Fill<'a> {
    Color(&'a Color),
    Url(&'a str),
}

impl Display for Fill<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Color(color) => color.fmt(f),
            Self::Url(id) => write!(f, "url(#{id})"),
        }
    }
}

struct Svg(String);

impl Svg {
    fn raw(&mut self, args: fmt::Arguments<'_>) {
        // writing into a String cannot fail
        let _ = self.0.write_fmt(args);
    }

    fn rect_with_stroke(
        &mut self,
        (x, y, w, h): (f64, f64, f64, f64),
        fill: Option<Fill<'_>>,
        opacity: f64,
        radius: f64,
        stroke: Option<(&Color, f64)>,
    ) {
        self.raw(format_args!(
            r#"<rect x="{}" y="{}" width="{}" height="{}""#,
            Num(x),
            Num(y),
            Num(w),
            Num(h)
        ));
        if radius > 0.0 {
            self.raw(format_args!(
                r#" rx="{}""#,
                Num(radius.min(h / 2.0).min(w / 2.0))
            ));
        }
        match fill {
            Some(fill) => {
                self.raw(format_args!(r#" fill="{fill}""#));
                let opacity = opacity.clamp(0.0, 1.0);
                if opacity < 1.0 {
                    self.raw(format_args!(r#" fill-opacity="{opacity:.2}""#));
                }
            }
            None => self.raw(format_args!(r#" fill="none""#)),
        }
        if let Some((color, width)) = stroke {
            self.raw(format_args!(
                r#" stroke="{color}" stroke-width="{}""#,
                Num(width.clamp(0.0, 64.0))
            ));
        }
        self.raw(format_args!("/>"));
    }

    fn rect(&mut self, area: (f64, f64, f64, f64), fill: Fill<'_>, opacity: f64, radius: f64) {
        self.rect_with_stroke(area, Some(fill), opacity, radius, None);
    }

    fn shape(&mut self, shape: &Shape, index: usize) {
        match shape {
            Shape::Surface {
                frame,
                radius,
                fill,
                fill_alpha,
                gradient,
                stroke,
                stroke_width,
            } => {
                if fill.is_some() || stroke.is_some() {
                    let opacity = if *fill_alpha > 0.0 { *fill_alpha } else { 1.0 };
                    self.rect_with_stroke(
                        (frame.x, frame.y, frame.w, frame.h),
                        fill.as_ref().map(Fill::Color),
                        opacity,
                        *radius,
                        stroke.as_ref().map(|color| (color, *stroke_width)),
                    );
                }
                if let Some(gradient) = gradient {
                    let id = format!("g{index}");
                    self.gradient(gradient, &id);
                    self.rect(
                        (frame.x, frame.y, frame.w, frame.h),
                        Fill::Url(&id),
                        1.0,
                        *radius,
                    );
                }
            }
            Shape::Rule {
                frame,
                color,
                alpha,
            } => {
                self.rect(
                    (frame.x, frame.y, frame.w, frame.h),
                    Fill::Color(color),
                    *alpha,
                    0.0,
                );
            }
            Shape::Media {
                frame,
                radius,
                tint,
                grid,
            } => self.media(frame, *radius, tint, grid.as_ref(), index),
            Shape::Glyph {
                frame,
                glyph,
                size,
                color,
                count,
            } => self.glyph(frame, *glyph, *size, color, *count),
            Shape::Heading(line) => self.line(line, 0.85, true, index),
            Shape::Text(line) => self.line(line, 0.42, false, index),
            Shape::Link(line) => self.line(line, 0.7, false, index),
            Shape::Code(line) => self.line(line, 0.55, false, index),
        }
    }

    fn line(&mut self, line: &Line, bar_opacity: f64, is_heading: bool, index: usize) {
        let Line {
            frame,
            size,
            color,
            weight,
            gradient,
            text,
        } = line;
        let size = size.clamp(1.0, 400.0);
        let id = format!("g{index}");
        // gradient text is the loudest ink on its page, so it goes at full strength
        let (fill, opacity) = match gradient {
            Some(gradient) => {
                self.gradient(gradient, &id);
                (Fill::Url(&id), 1.0)
            }
            None => (Fill::Color(color), bar_opacity),
        };

        let heading = text
            .as_deref()
            .map(str::trim)
            .filter(|text| is_heading && !text.is_empty());
        if let Some(text) = heading {
            let text: String = text.chars().take(MAX_HEADING_CHARS).collect();
            let weight = (weight.clamp(100.0, 900.0) / 100.0).round() * 100.0;
            // stretched to the width the browser measured, so a fallback font
            // with other metrics still ends where the real heading ends
            self.raw(format_args!(
                r#"<text x="{}" y="{}" font-family="{FAMILY}" font-size="{}" font-weight="{weight}" fill="{fill}" textLength="{}" lengthAdjust="spacingAndGlyphs">{}</text>"#,
                Num(frame.x),
                Num(frame.y + frame.h * 0.78),
                Num(size),
                Num(frame.w),
                Escaped(&text)
            ));
            return;
        }

        // a bar the height of the x-height, where the lowercase letters sit
        let height = (size * if is_heading { 0.55 } else { 0.42 }).max(2.0);
        let top = frame.y + (frame.h - height) / 2.0 + size * 0.06;
        self.rect((frame.x, top, frame.w, height), fill, opacity, height / 2.0);
    }

    fn media(
        &mut self,
        frame: &Frame,
        radius: f64,
        tint: &Color,
        grid: Option<&Grid>,
        index: usize,
    ) {
        let Frame { x, y, w, h } = *frame;
        let grid = grid.filter(|grid| {
            (1..=8).contains(&grid.cols)
                && (1..=6).contains(&grid.rows)
                && grid.cells.len() == grid.cols * grid.rows
        });
        let Some(grid) = grid else {
            if w <= ICON_MAX && h <= ICON_MAX {
                self.rect((x, y, w, h), Fill::Color(tint), 0.55, (w / 4.0).min(4.0));
            } else {
                self.rect((x, y, w, h), Fill::Color(tint), 0.14, radius);
            }
            return;
        };

        let clip = radius > 0.0;
        if clip {
            self.raw(format_args!(r#"<clipPath id="m{index}">"#));
            self.rect((x, y, w, h), Fill::Color(tint), 1.0, radius);
            self.raw(format_args!(r#"</clipPath><g clip-path="url(#m{index})">"#));
        }
        let (cell_w, cell_h) = (w / grid.cols as f64, h / grid.rows as f64);
        for (i, cell) in grid.cells.iter().enumerate() {
            let Some(cell) = cell else { continue };
            let (col, row) = ((i % grid.cols) as f64, (i / grid.cols) as f64);
            // a hair of overlap, so antialiased seams do not show between cells
            self.rect(
                (
                    x + col * cell_w,
                    y + row * cell_h,
                    cell_w + 0.5,
                    cell_h + 0.5,
                ),
                Fill::Color(&cell.color),
                cell.alpha,
                0.0,
            );
        }
        if clip {
            self.raw(format_args!("</g>"));
        }
    }

    fn glyph(&mut self, frame: &Frame, glyph: GlyphKind, size: f64, color: &Color, count: u32) {
        let Frame { x, y, w, h } = *frame;
        let shade = match glyph {
            GlyphKind::Shade1 => Some(0.25),
            GlyphKind::Shade2 => Some(0.5),
            GlyphKind::Shade3 => Some(0.75),
            GlyphKind::Solid => Some(1.0),
            GlyphKind::Hline | GlyphKind::Vline | GlyphKind::Cross => None,
        };
        if let Some(opacity) = shade {
            self.rect((x, y, w, h), Fill::Color(color), opacity, 0.0);
            return;
        }

        let stroke = (size.clamp(1.0, 400.0) * 0.08).max(1.0);
        if matches!(glyph, GlyphKind::Hline | GlyphKind::Cross) {
            self.rect(
                (x, y + h / 2.0 - stroke / 2.0, w, stroke),
                Fill::Color(color),
                0.9,
                0.0,
            );
        }
        if matches!(glyph, GlyphKind::Vline | GlyphKind::Cross) {
            let count = count.min(MAX_GLYPHS);
            let step = w / f64::from(count.max(1));
            for i in 0..count {
                let left = x + step * f64::from(i) + step / 2.0 - stroke / 2.0;
                self.rect((left, y, stroke, h), Fill::Color(color), 0.9, 0.0);
            }
        }
    }

    fn gradient(&mut self, gradient: &Gradient, id: &str) {
        // CSS 0deg points up and turns clockwise; SVG wants the line in
        // bounding-box units
        let turn = gradient.angle.rem_euclid(360.0).to_radians();
        let (dx, dy) = (turn.sin() / 2.0, -turn.cos() / 2.0);
        self.raw(format_args!(
            r#"<linearGradient id="{id}" x1="{:.3}" y1="{:.3}" x2="{:.3}" y2="{:.3}">"#,
            0.5 - dx,
            0.5 - dy,
            0.5 + dx,
            0.5 + dy
        ));
        let stops = &gradient.stops[..gradient.stops.len().min(MAX_STOPS)];
        let last = stops.len().saturating_sub(1).max(1) as f64;
        for (i, stop) in stops.iter().enumerate() {
            self.raw(format_args!(
                r#"<stop offset="{:.2}" stop-color="{}" stop-opacity="{:.2}"/>"#,
                i as f64 / last,
                stop.color,
                stop.alpha.clamp(0.0, 1.0)
            ));
        }
        self.raw(format_args!("</linearGradient>"));
    }
}

/// Text content escaped for an SVG element body.
struct Escaped<'a>(&'a str);

impl Display for Escaped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in self.0.chars() {
            match ch {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                '\'' => f.write_str("&#39;")?,
                // XML 1.0 has no representation for most control characters
                c if c.is_control() => {}
                c => f.write_char(c)?,
            }
        }
        Ok(())
    }
}
