//! Tom Riddle's hand: rasterize reply text in Dancing Script, thin it to
//! single-pixel pen paths (Zhang-Suen), trace them into ordered strokes, and
//! yield them for stroke-by-stroke animation.

use ab_glyph::{Font, FontRef, Glyph, GlyphId, PxScale, ScaleFont};

pub struct Line {
    pub width: usize,
    pub height: usize,
    /// Bit mask of inked pixels, row-major.
    pub mask: Vec<bool>,
}

pub struct FontStack<'font> {
    fonts: Vec<FontRef<'font>>,
}

impl<'font> FontStack<'font> {
    pub fn single(font: FontRef<'font>) -> Self {
        Self { fonts: vec![font] }
    }

    pub fn with_fallbacks(primary: FontRef<'font>, fallbacks: impl IntoIterator<Item = FontRef<'font>>) -> Self {
        let mut fonts = vec![primary];
        fonts.extend(fallbacks);
        Self { fonts }
    }

    fn glyph_for(&self, c: char) -> Option<(usize, GlyphId)> {
        for (i, font) in self.fonts.iter().enumerate() {
            let id = font.glyph_id(c);
            if id.0 != 0 {
                return Some((i, id));
            }
        }
        None
    }

    fn vertical_metrics(&self, px: f32) -> (f32, f32) {
        let mut ascent = 0.0f32;
        let mut descent = 0.0f32;
        for font in &self.fonts {
            let scaled = font.as_scaled(PxScale::from(px));
            ascent = ascent.max(scaled.ascent());
            descent = descent.min(scaled.descent());
        }
        (ascent, ascent - descent)
    }
}

/// Rasterize one line of text at `px` height into a boolean mask.
pub fn rasterize_line(font: &FontRef, text: &str, px: f32) -> Line {
    rasterize_line_stack(&FontStack::single(font.clone()), text, px)
}

/// Rasterize one line with per-character font fallback.
pub fn rasterize_line_stack(fonts: &FontStack, text: &str, px: f32) -> Line {
    struct PositionedGlyph {
        font_i: usize,
        glyph: Glyph,
    }

    let scale = PxScale::from(px);
    let (baseline, height_px) = fonts.vertical_metrics(px);
    let mut glyphs: Vec<PositionedGlyph> = Vec::new();
    let mut caret = 0.0f32;
    let mut prev: Option<(usize, GlyphId)> = None;
    for c in text.chars() {
        let Some((font_i, id)) = fonts.glyph_for(c) else {
            prev = None;
            continue;
        };
        let font = &fonts.fonts[font_i];
        let scaled = font.as_scaled(scale);
        if let Some((prev_i, prev_id)) = prev {
            if prev_i == font_i {
                caret += scaled.kern(prev_id, id);
            }
        }
        let mut glyph = id.with_scale(scale);
        glyph.position = ab_glyph::point(caret, baseline);
        caret += scaled.h_advance(id);
        glyphs.push(PositionedGlyph { font_i, glyph });
        prev = Some((font_i, id));
    }
    let width = (caret.ceil() as usize + 4).max(1);
    let height = (height_px.ceil() as usize + 4).max(1);
    let mut mask = vec![false; width * height];
    for g in glyphs {
        if let Some(outline) = fonts.fonts[g.font_i].outline_glyph(g.glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|x, y, cov| {
                if cov > 0.5 {
                    let px_x = bounds.min.x as i32 + x as i32;
                    let px_y = bounds.min.y as i32 + y as i32;
                    if px_x >= 0 && px_y >= 0 && (px_x as usize) < width && (px_y as usize) < height {
                        mask[px_y as usize * width + px_x as usize] = true;
                    }
                }
            });
        }
    }
    Line { width, height, mask }
}

/// Measure the advance width of text at `px` without rasterizing.
pub fn measure(font: &FontRef, text: &str, px: f32) -> f32 {
    measure_stack(&FontStack::single(font.clone()), text, px)
}

/// Measure text with per-character font fallback.
pub fn measure_stack(fonts: &FontStack, text: &str, px: f32) -> f32 {
    let scale = PxScale::from(px);
    let mut caret = 0.0f32;
    let mut prev: Option<(usize, GlyphId)> = None;
    for c in text.chars() {
        let Some((font_i, id)) = fonts.glyph_for(c) else {
            prev = None;
            continue;
        };
        let scaled = fonts.fonts[font_i].as_scaled(scale);
        if let Some((prev_i, prev_id)) = prev {
            if prev_i == font_i {
                caret += scaled.kern(prev_id, id);
            }
        }
        caret += scaled.h_advance(id);
        prev = Some((font_i, id));
    }
    caret
}

/// Zhang-Suen thinning: reduce the mask to 1px-wide skeleton lines.
pub fn thin(line: &mut Line) {
    let (w, h) = (line.width, line.height);
    let idx = |x: usize, y: usize| y * w + x;
    loop {
        let mut changed = false;
        for phase in 0..2 {
            let mut to_clear = Vec::new();
            for y in 1..h.saturating_sub(1) {
                for x in 1..w.saturating_sub(1) {
                    if !line.mask[idx(x, y)] {
                        continue;
                    }
                    let p = [
                        line.mask[idx(x, y - 1)],     // p2 N
                        line.mask[idx(x + 1, y - 1)], // p3 NE
                        line.mask[idx(x + 1, y)],     // p4 E
                        line.mask[idx(x + 1, y + 1)], // p5 SE
                        line.mask[idx(x, y + 1)],     // p6 S
                        line.mask[idx(x - 1, y + 1)], // p7 SW
                        line.mask[idx(x - 1, y)],     // p8 W
                        line.mask[idx(x - 1, y - 1)], // p9 NW
                    ];
                    let b: u32 = p.iter().map(|&v| v as u32).sum();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let mut a = 0;
                    for i in 0..8 {
                        if !p[i] && p[(i + 1) % 8] {
                            a += 1;
                        }
                    }
                    if a != 1 {
                        continue;
                    }
                    let (c1, c2) = if phase == 0 {
                        (!(p[0] && p[2] && p[4]), !(p[2] && p[4] && p[6]))
                    } else {
                        (!(p[0] && p[2] && p[6]), !(p[0] && p[4] && p[6]))
                    };
                    if c1 && c2 {
                        to_clear.push(idx(x, y));
                    }
                }
            }
            if !to_clear.is_empty() {
                changed = true;
                for i in to_clear {
                    line.mask[i] = false;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Trace the skeleton into polyline strokes, ordered left-to-right so the
/// animation writes like a hand.
pub fn trace(line: &Line) -> Vec<Vec<(i32, i32)>> {
    let (w, h) = (line.width, line.height);
    let at = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && line.mask[y as usize * w + x as usize]
    };
    let neighbors = |x: i32, y: i32| -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                if (dx != 0 || dy != 0) && at(x + dx, y + dy) {
                    out.push((x + dx, y + dy));
                }
            }
        }
        out
    };

    let mut visited = vec![false; w * h];
    let vis = |v: &mut Vec<bool>, x: i32, y: i32| {
        v[y as usize * w + x as usize] = true;
    };
    let seen = |v: &Vec<bool>, x: i32, y: i32| -> bool { v[y as usize * w + x as usize] };

    // Endpoints first (degree 1), then any remaining pixels (loops).
    let mut starts: Vec<(i32, i32)> = Vec::new();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if at(x, y) && neighbors(x, y).len() == 1 {
                starts.push((x, y));
            }
        }
    }
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if at(x, y) {
                starts.push((x, y));
            }
        }
    }

    let mut strokes: Vec<Vec<(i32, i32)>> = Vec::new();
    for (sx, sy) in starts {
        if seen(&visited, sx, sy) {
            continue;
        }
        let mut path = vec![(sx, sy)];
        vis(&mut visited, sx, sy);
        let (mut cx, mut cy) = (sx, sy);
        loop {
            let next = neighbors(cx, cy)
                .into_iter()
                .find(|&(nx, ny)| !seen(&visited, nx, ny));
            match next {
                Some((nx, ny)) => {
                    vis(&mut visited, nx, ny);
                    path.push((nx, ny));
                    cx = nx;
                    cy = ny;
                }
                None => break,
            }
        }
        if path.len() >= 3 {
            strokes.push(path);
        }
    }
    strokes.sort_by_key(|s| s.iter().map(|&(x, _)| x).min().unwrap_or(0));
    strokes
}

/// Word-wrap `text` to lines that fit `max_px` at scale `px`.
pub fn wrap(font: &FontRef, text: &str, px: f32, max_px: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.lines() {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if measure(font, &cand, px) <= max_px || cur.is_empty() {
                cur = cand;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    lines
}

/// Wrap text with fallback-font measurement. Non-ASCII runs can break between
/// characters, which keeps Chinese/Japanese/Korean replies inside the page.
pub fn wrap_stack(fonts: &FontStack, text: &str, px: f32, max_px: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.lines() {
        let mut cur = String::new();
        for token in wrap_tokens(para) {
            if token == " " {
                if !cur.is_empty() && !cur.ends_with(' ') {
                    cur.push(' ');
                }
                continue;
            }

            let cand = if cur.is_empty() { token.clone() } else { format!("{cur}{token}") };
            if measure_stack(fonts, &cand, px) <= max_px || cur.is_empty() {
                cur = cand;
            } else {
                lines.push(cur.trim_end().to_string());
                cur = token.trim_start().to_string();
            }
        }
        if !cur.trim().is_empty() {
            lines.push(cur.trim_end().to_string());
        }
    }
    lines
}

fn wrap_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii_word = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !ascii_word.is_empty() {
                tokens.push(std::mem::take(&mut ascii_word));
            }
            tokens.push(" ".to_string());
        } else if ch.is_ascii() {
            ascii_word.push(ch);
        } else {
            if !ascii_word.is_empty() {
                tokens.push(std::mem::take(&mut ascii_word));
            }
            tokens.push(ch.to_string());
        }
    }
    if !ascii_word.is_empty() {
        tokens.push(ascii_word);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_produces_strokes() {
        let font = FontRef::try_from_slice(include_bytes!("../fonts/DancingScript.ttf")).unwrap();
        let mut line = rasterize_line(&font, "Yes, Harry?", 96.0);
        assert!(line.width > 100 && line.height > 50);
        let inked_before: usize = line.mask.iter().filter(|&&v| v).count();
        thin(&mut line);
        let inked_after: usize = line.mask.iter().filter(|&&v| v).count();
        assert!(inked_after * 3 < inked_before, "thinning should slim the glyphs: {inked_before} -> {inked_after}");
        let strokes = trace(&line);
        assert!(!strokes.is_empty());
        let total: usize = strokes.iter().map(|s| s.len()).sum();
        println!("strokes={} total_points={} ({}x{})", strokes.len(), total, line.width, line.height);
        assert!(total > 200, "expected a decent path length, got {total}");
        // Wrap sanity.
        let lines = wrap(&font, "Do you know anything about the Chamber of Secrets?", 96.0, 1380.0);
        assert!(lines.len() >= 2);
    }

    #[test]
    fn fallback_font_renders_chinese_and_wraps_without_spaces() {
        let primary = FontRef::try_from_slice(include_bytes!("../fonts/DancingScript.ttf")).unwrap();
        let cjk = FontRef::try_from_slice(include_bytes!("../fonts/LXGWWenKai-Regular.ttf")).unwrap();
        assert_eq!(primary.glyph_id('你').0, 0);
        assert_ne!(cjk.glyph_id('你').0, 0);

        let fonts = FontStack::with_fallbacks(primary, [cjk]);
        let mut line = rasterize_line_stack(&fonts, "Tom, 你好。", 96.0);
        assert!(line.width > 250 && line.height > 50);
        let inked_before: usize = line.mask.iter().filter(|&&v| v).count();
        assert!(inked_before > 500, "expected Chinese glyphs to produce ink, got {inked_before}");
        thin(&mut line);
        assert!(!trace(&line).is_empty());

        let wrapped = wrap_stack(&fonts, "这是一个没有空格的中文句子，用来确认换行不会超出页面。", 96.0, 520.0);
        assert!(wrapped.len() > 1, "Chinese text without spaces should wrap");
        assert!(wrapped.iter().all(|line| measure_stack(&fonts, line, 96.0) <= 520.0));
    }
}
