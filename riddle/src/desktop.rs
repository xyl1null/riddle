//! macOS desktop display/input backend for local Apple Silicon development.
//!
//! It keeps the app's native 1620x2160 canvas so PNG capture and reply layout
//! match the tablet, then stretches that buffer into a smaller window.

use std::io;

use minifb::{Key, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};

use crate::fb::{SCREEN_H, SCREEN_W};
use crate::qtfb::{InputEvent, INPUT_PEN_PRESS, INPUT_PEN_RELEASE, INPUT_PEN_UPDATE};
use crate::surface::{PixFmt, Surface};

const DEFAULT_WINDOW_W: usize = 540;
const DEFAULT_WINDOW_H: usize = 720;

pub struct DesktopDisplay {
    window: Window,
    framebuffer: Vec<u32>,
    base_buffer: Vec<u32>,
    present_buffer: Vec<u32>,
    mouse_down: bool,
    last_pos: Option<(i32, i32)>,
    cursor_rect: Option<Rect>,
    frame: u64,
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl DesktopDisplay {
    pub fn open() -> io::Result<(Self, Surface)> {
        let window_w = env_usize("RIDDLE_DESKTOP_W").unwrap_or(DEFAULT_WINDOW_W);
        let window_h = env_usize("RIDDLE_DESKTOP_H").unwrap_or(DEFAULT_WINDOW_H);
        let mut window = Window::new(
            "里德尔日记本",
            window_w,
            window_h,
            WindowOptions {
                resize: true,
                scale_mode: ScaleMode::Stretch,
                ..WindowOptions::default()
            },
        )
        .map_err(io::Error::other)?;
        window.set_target_fps(120);
        window.set_cursor_visibility(false);

        let mut framebuffer = vec![0xFFFF_FFFF; SCREEN_W * SCREEN_H];
        let ptr = framebuffer.as_mut_ptr().cast::<u8>();
        let len = framebuffer.len() * std::mem::size_of::<u32>();
        let surface = Surface::new(ptr, len, SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let mut base_buffer = framebuffer.clone();
        draw_diary_frame(&mut base_buffer);

        let mut display = Self {
            window,
            present_buffer: base_buffer.clone(),
            base_buffer,
            framebuffer,
            mouse_down: false,
            last_pos: None,
            cursor_rect: None,
            frame: 0,
        };
        display.present()?;
        Ok((display, surface))
    }

    pub fn update_region(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {
        self.rebuild_base();
        let _ = self.present();
    }

    pub fn update_all(&mut self) {
        self.rebuild_base();
        let _ = self.present();
    }

    pub fn full_refresh(&mut self) {
        self.rebuild_base();
        let _ = self.present();
    }

    pub fn pump(&mut self) -> io::Result<Vec<InputEvent>> {
        self.present()?;
        if !self.window.is_open() || self.window.is_key_down(Key::Escape) {
            return Err(io::Error::new(io::ErrorKind::ConnectionReset, "desktop window closed"));
        }

        let left = self.window.get_mouse_down(MouseButton::Left);
        let right = self.window.get_mouse_down(MouseButton::Right);
        let down = left || right;
        let pos = self.mouse_pos();
        let mut out = Vec::new();

        match (self.mouse_down, down, pos) {
            (false, true, Some((x, y))) => {
                out.push(pen_event(INPUT_PEN_PRESS, x, y, pressure(right)));
                self.last_pos = Some((x, y));
            }
            (true, true, Some((x, y))) if self.last_pos != Some((x, y)) => {
                out.push(pen_event(INPUT_PEN_UPDATE, x, y, pressure(right)));
                self.last_pos = Some((x, y));
            }
            (true, false, _) => {
                let (x, y) = self.last_pos.unwrap_or((0, 0));
                out.push(pen_event(INPUT_PEN_RELEASE, x, y, 0));
                self.last_pos = None;
            }
            _ => {}
        }

        self.mouse_down = down;
        Ok(out)
    }

    fn present(&mut self) -> io::Result<()> {
        let cursor = self.mouse_pos();
        self.frame = self.frame.wrapping_add(1);
        if let Some(rect) = self.cursor_rect.take() {
            copy_rect_u32(&self.base_buffer, &mut self.present_buffer, rect);
        }
        if let Some((x, y)) = cursor {
            draw_quill_cursor(&mut self.present_buffer, x, y, self.frame, self.mouse_down);
            self.cursor_rect = cursor_bounds(x, y);
        }
        self.window
            .update_with_buffer(&self.present_buffer, SCREEN_W, SCREEN_H)
            .map_err(io::Error::other)
    }

    fn rebuild_base(&mut self) {
        self.base_buffer.copy_from_slice(&self.framebuffer);
        draw_diary_frame(&mut self.base_buffer);
        self.present_buffer.copy_from_slice(&self.base_buffer);
        self.cursor_rect = None;
    }

    fn mouse_pos(&self) -> Option<(i32, i32)> {
        let (mx, my) = self.window.get_mouse_pos(MouseMode::Discard)?;
        let (ww, wh) = self.window.get_size();
        if ww == 0 || wh == 0 {
            return None;
        }
        let x = (mx * SCREEN_W as f32 / ww as f32)
            .floor()
            .clamp(0.0, (SCREEN_W - 1) as f32) as i32;
        let y = (my * SCREEN_H as f32 / wh as f32)
            .floor()
            .clamp(0.0, (SCREEN_H - 1) as f32) as i32;
        Some((x, y))
    }
}

fn pen_event(input_type: i32, x: i32, y: i32, d: i32) -> InputEvent {
    InputEvent {
        input_type,
        dev_id: 0,
        x,
        y,
        d,
    }
}

fn pressure(erasing: bool) -> i32 {
    if erasing {
        -100
    } else {
        80
    }
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok()?.parse().ok()
}

fn draw_diary_frame(buf: &mut [u32]) {
    const LEATHER: u32 = 0xFF13_1110;
    const LEATHER_DARK: u32 = 0xFF08_0707;
    const LEATHER_HI: u32 = 0xFF24_211E;
    const GOLD: u32 = 0xFFB4_8D3A;
    const GOLD_DARK: u32 = 0xFF6F_5522;
    const PAGE_SHADOW: u32 = 0xFF7A_7770;

    const LEFT: i32 = 84;
    const RIGHT: i32 = 62;
    const TOP: i32 = 72;
    const BOTTOM: i32 = 96;

    fill_rect_u32(buf, 0, 0, SCREEN_W as i32, TOP, LEATHER);
    fill_rect_u32(buf, 0, SCREEN_H as i32 - BOTTOM, SCREEN_W as i32, BOTTOM, LEATHER);
    fill_rect_u32(buf, 0, 0, LEFT, SCREEN_H as i32, LEATHER_DARK);
    fill_rect_u32(buf, SCREEN_W as i32 - RIGHT, 0, RIGHT, SCREEN_H as i32, LEATHER);

    draw_line(buf, (LEFT - 8, 0), (LEFT - 8, SCREEN_H as i32), 2, LEATHER_HI);
    draw_line(buf, (LEFT + 2, TOP), (LEFT + 2, SCREEN_H as i32 - BOTTOM), 2, PAGE_SHADOW);
    draw_line(buf, (LEFT, TOP + 2), (SCREEN_W as i32 - RIGHT, TOP + 2), 2, PAGE_SHADOW);
    draw_line(buf, (SCREEN_W as i32 - RIGHT - 2, TOP), (SCREEN_W as i32 - RIGHT - 2, SCREEN_H as i32 - BOTTOM), 2, PAGE_SHADOW);
    draw_line(buf, (LEFT, SCREEN_H as i32 - BOTTOM - 2), (SCREEN_W as i32 - RIGHT, SCREEN_H as i32 - BOTTOM - 2), 2, PAGE_SHADOW);

    draw_corner_plate(buf, 18, 18, 1, 1);
    draw_corner_plate(buf, SCREEN_W as i32 - 18, 18, -1, 1);
    draw_corner_plate(buf, 18, SCREEN_H as i32 - 18, 1, -1);
    draw_corner_plate(buf, SCREEN_W as i32 - 18, SCREEN_H as i32 - 18, -1, -1);

    let label_w = 500;
    let label_h = 48;
    let label_x = (SCREEN_W as i32 - label_w) / 2;
    let label_y = SCREEN_H as i32 - 73;
    fill_rect_u32(buf, label_x, label_y, label_w, label_h, 0xFF10_0F0E);
    draw_rect_u32(buf, label_x, label_y, label_w, label_h, 2, GOLD_DARK);
    draw_rect_u32(buf, label_x + 7, label_y + 7, label_w - 14, label_h - 14, 1, GOLD);
    draw_text(buf, label_x + 56, label_y + 15, "TOM MARVOLO RIDDLE", 4, GOLD);
}

fn draw_corner_plate(buf: &mut [u32], x: i32, y: i32, sx: i32, sy: i32) {
    const GOLD: u32 = 0xFFC1_9540;
    const GOLD_DARK: u32 = 0xFF6B_5121;
    const CUT: u32 = 0xFF13_1110;
    let a = (x, y);
    let b = (x + sx * 96, y);
    let c = (x, y + sy * 96);
    fill_triangle(buf, a, b, c, GOLD_DARK);
    fill_triangle(buf, (x + sx * 10, y + sy * 10), (x + sx * 84, y + sy * 10), (x + sx * 10, y + sy * 84), GOLD);
    fill_triangle(buf, (x + sx * 36, y + sy * 36), (x + sx * 92, y + sy * 18), (x + sx * 18, y + sy * 92), CUT);
    draw_line(buf, (x + sx * 18, y + sy * 74), (x + sx * 74, y + sy * 18), 2, GOLD_DARK);
    draw_line(buf, (x + sx * 13, y + sy * 48), (x + sx * 48, y + sy * 13), 1, GOLD_DARK);
}

fn draw_quill_cursor(buf: &mut [u32], x: i32, y: i32, frame: u64, down: bool) {
    const OUTLINE: u32 = 0xFF12_100E;
    const NIB: u32 = 0xFF2B_2925;
    const NIB_LIGHT: u32 = 0xFFDD_D6C8;
    const SHAFT: u32 = 0xFF6A_4A2F;
    const SHAFT_LIGHT: u32 = 0xFFA6_7B51;
    const FEATHER: u32 = 0xFFE7_E4DB;
    const FEATHER_SHADE: u32 = 0xFFD1_CDC2;
    const FEATHER_EDGE: u32 = 0xFF8C_887F;
    const EYE_GOLD: u32 = 0xFFC4_A24B;
    const EYE_TEAL: u32 = 0xFF2A_8078;
    const EYE_BLUE: u32 = 0xFF1B_314C;

    let sway = triangle_wave(frame, 84, 11);
    let flutter = triangle_wave(frame + 21, 58, 5);
    let nib_y = if down { y + 1 } else { y + triangle_wave(frame + 11, 96, 1) };
    let y = nib_y;

    let shaft = [
        (x + 9, y - 7),
        (x + 17, y - 19),
        (x + 31, y - 41),
        (x + 47 + sway / 8, y - 70),
        (x + 68 + sway / 6, y - 135),
        (x + 94 + sway / 4, y - 222),
        (x + 123 + sway / 2, y - 314),
        (x + 144 + sway + flutter, y - 402),
    ];

    let left_vane = [
        shaft[2],
        (x + 23 + sway / 8, y - 66),
        (x + 21 + sway / 6, y - 132),
        (x + 35 + sway / 5, y - 228),
        (x + 72 + sway / 3, y - 344),
        shaft[7],
        shaft[6],
        shaft[5],
        shaft[4],
        shaft[3],
    ];
    let right_vane = [
        shaft[2],
        (x + 63 + sway / 8, y - 63),
        (x + 88 + sway / 6, y - 136),
        (x + 116 + sway / 5, y - 235),
        (x + 139 + sway / 3 + flutter, y - 346),
        shaft[7],
        shaft[6],
        shaft[5],
        shaft[4],
        shaft[3],
    ];

    fill_polygon(buf, &left_vane, OUTLINE);
    fill_polygon(buf, &right_vane, OUTLINE);
    fill_polygon_inset(buf, &left_vane, 5, FEATHER);
    fill_polygon_inset(buf, &right_vane, 5, FEATHER);
    fill_triangle(buf, shaft[4], (x + 31 + sway / 4, y - 236), shaft[7], FEATHER_SHADE);
    fill_triangle(buf, shaft[4], (x + 125 + sway / 4, y - 238), shaft[7], FEATHER_SHADE);

    let notches = [
        ((x + 25 + sway / 5, y - 120), shaft[4]),
        ((x + 39 + sway / 4, y - 252), shaft[5]),
        ((x + 83 + sway / 5, y - 123), shaft[4]),
        ((x + 116 + sway / 4, y - 255), shaft[5]),
    ];
    for &(a, b) in &notches {
        draw_line(buf, a, b, 1, FEATHER_EDGE);
    }

    draw_polyline(buf, &shaft, 5, OUTLINE);
    draw_polyline(buf, &shaft, 3, SHAFT);
    draw_polyline(buf, &shaft[1..7], 1, SHAFT_LIGHT);

    let eye = (x + 136 + sway + flutter, y - 365);
    fill_ellipse(buf, eye.0, eye.1, 22, 33, OUTLINE);
    fill_ellipse(buf, eye.0, eye.1, 18, 28, EYE_GOLD);
    fill_ellipse(buf, eye.0 - 1, eye.1 + 2, 11, 18, EYE_TEAL);
    fill_ellipse(buf, eye.0 - 1, eye.1 + 3, 5, 9, EYE_BLUE);

    fill_triangle(buf, (x, y), (x + 7, y - 17), (x + 20, y - 9), OUTLINE);
    fill_triangle(buf, (x + 2, y - 1), (x + 8, y - 14), (x + 17, y - 8), NIB);
    draw_line(buf, (x + 4, y - 3), (x + 12, y - 11), 1, NIB_LIGHT);
    put_disk(buf, x, y, 1, OUTLINE);
}

fn triangle_wave(frame: u64, period: u64, amp: i32) -> i32 {
    let half = (period / 2).max(1);
    let p = (frame % period) as i32;
    let half_i = half as i32;
    let v = if p < half_i { p } else { period as i32 - p };
    v * amp * 2 / half_i - amp
}

fn fill_rect_u32(buf: &mut [u32], x: i32, y: i32, w: i32, h: i32, color: u32) {
    let x0 = x.max(0) as usize;
    let y0 = y.max(0) as usize;
    let x1 = (x + w).min(SCREEN_W as i32).max(0) as usize;
    let y1 = (y + h).min(SCREEN_H as i32).max(0) as usize;
    for row in y0..y1 {
        let start = row * SCREEN_W + x0;
        let end = row * SCREEN_W + x1;
        buf[start..end].fill(color);
    }
}

fn draw_rect_u32(buf: &mut [u32], x: i32, y: i32, w: i32, h: i32, t: i32, color: u32) {
    fill_rect_u32(buf, x, y, w, t, color);
    fill_rect_u32(buf, x, y + h - t, w, t, color);
    fill_rect_u32(buf, x, y, t, h, color);
    fill_rect_u32(buf, x + w - t, y, t, h, color);
}

fn draw_text(buf: &mut [u32], x: i32, y: i32, text: &str, scale: i32, color: u32) {
    let mut cx = x;
    for ch in text.chars() {
        if ch == ' ' {
            cx += scale * 4;
            continue;
        }
        let glyph = glyph_5x7(ch);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    fill_rect_u32(buf, cx + col * scale, y + row as i32 * scale, scale, scale, color);
                }
            }
        }
        cx += scale * 6;
    }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'I' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        _ => [0; 7],
    }
}

fn draw_line(buf: &mut [u32], a: (i32, i32), b: (i32, i32), r: i32, color: u32) {
    let (x0, y0) = a;
    let (x1, y1) = b;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let steps = dx.max(dy).max(1);
    for i in 0..=steps {
        let x = x0 + (x1 - x0) * i / steps;
        let y = y0 + (y1 - y0) * i / steps;
        put_disk(buf, x, y, r, color);
    }
}

fn draw_polyline(buf: &mut [u32], points: &[(i32, i32)], r: i32, color: u32) {
    for pair in points.windows(2) {
        draw_line(buf, pair[0], pair[1], r, color);
    }
}

fn put_disk(buf: &mut [u32], cx: i32, cy: i32, r: i32, color: u32) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                put_px(buf, cx + dx, cy + dy, color);
            }
        }
    }
}

fn fill_ellipse(buf: &mut [u32], cx: i32, cy: i32, rx: i32, ry: i32, color: u32) {
    let rx2 = (rx * rx).max(1);
    let ry2 = (ry * ry).max(1);
    for py in cy - ry..=cy + ry {
        for px in cx - rx..=cx + rx {
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx * ry2 + dy * dy * rx2 <= rx2 * ry2 {
                put_px(buf, px, py, color);
            }
        }
    }
}

fn cursor_bounds(x: i32, y: i32) -> Option<Rect> {
    rect_from_i32(x - 28, y - 446, x + 206, y + 24)
}

fn rect_from_i32(x0: i32, y0: i32, x1: i32, y1: i32) -> Option<Rect> {
    let x0 = x0.max(0).min(SCREEN_W as i32);
    let y0 = y0.max(0).min(SCREEN_H as i32);
    let x1 = x1.max(0).min(SCREEN_W as i32);
    let y1 = y1.max(0).min(SCREEN_H as i32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Rect {
        x: x0 as usize,
        y: y0 as usize,
        w: (x1 - x0) as usize,
        h: (y1 - y0) as usize,
    })
}

fn copy_rect_u32(src: &[u32], dst: &mut [u32], rect: Rect) {
    for row in rect.y..rect.y + rect.h {
        let start = row * SCREEN_W + rect.x;
        let end = start + rect.w;
        dst[start..end].copy_from_slice(&src[start..end]);
    }
}

fn fill_triangle(buf: &mut [u32], a: (i32, i32), b: (i32, i32), c: (i32, i32), color: u32) {
    let min_x = a.0.min(b.0).min(c.0).max(0);
    let max_x = a.0.max(b.0).max(c.0).min(SCREEN_W as i32 - 1);
    let min_y = a.1.min(b.1).min(c.1).max(0);
    let max_y = a.1.max(b.1).max(c.1).min(SCREEN_H as i32 - 1);
    let area = edge(a, b, c);
    if area == 0 {
        return;
    }
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let p = (px, py);
            let w0 = edge(b, c, p);
            let w1 = edge(c, a, p);
            let w2 = edge(a, b, p);
            if (area > 0 && w0 >= 0 && w1 >= 0 && w2 >= 0) || (area < 0 && w0 <= 0 && w1 <= 0 && w2 <= 0) {
                put_px(buf, px, py, color);
            }
        }
    }
}

fn fill_polygon(buf: &mut [u32], points: &[(i32, i32)], color: u32) {
    if points.len() < 3 {
        return;
    }
    let min_x = points.iter().map(|p| p.0).min().unwrap().max(0);
    let max_x = points.iter().map(|p| p.0).max().unwrap().min(SCREEN_W as i32 - 1);
    let min_y = points.iter().map(|p| p.1).min().unwrap().max(0);
    let max_y = points.iter().map(|p| p.1).max().unwrap().min(SCREEN_H as i32 - 1);

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            if point_in_polygon(px, py, points) {
                put_px(buf, px, py, color);
            }
        }
    }
}

fn fill_polygon_inset(buf: &mut [u32], points: &[(i32, i32)], inset: i32, color: u32) {
    if points.len() < 3 {
        return;
    }
    let cx = points.iter().map(|p| p.0).sum::<i32>() as f32 / points.len() as f32;
    let cy = points.iter().map(|p| p.1).sum::<i32>() as f32 / points.len() as f32;
    let inset = inset as f32;
    let mut inner = Vec::with_capacity(points.len());
    for &(x, y) in points {
        let dx = cx - x as f32;
        let dy = cy - y as f32;
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        inner.push(((x as f32 + dx * inset / len).round() as i32, (y as f32 + dy * inset / len).round() as i32));
    }
    fill_polygon(buf, &inner, color);
}

fn point_in_polygon(px: i32, py: i32, points: &[(i32, i32)]) -> bool {
    let (px, py) = (px as f32 + 0.5, py as f32 + 0.5);
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = (points[i].0 as f32, points[i].1 as f32);
        let (xj, yj) = (points[j].0 as f32, points[j].1 as f32);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn edge(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> i32 {
    (p.0 - a.0) * (b.1 - a.1) - (p.1 - a.1) * (b.0 - a.0)
}

fn put_px(buf: &mut [u32], x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 && (x as usize) < SCREEN_W && (y as usize) < SCREEN_H {
        buf[y as usize * SCREEN_W + x as usize] = color;
    }
}
