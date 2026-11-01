//! Drawing code. Knows nothing about Windows: it paints pixels.

use fontdue::{Font, FontSettings};
use tiny_skia::{
    Color, FillRule, FilterQuality, IntSize, Paint, Path, PathBuilder, Pattern, Pixmap, SpreadMode,
    Stroke, Transform,
};

use crate::config::{parse_hex, WidgetConfig};

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The current local time, as the clock needs it.
pub struct ClockData {
    pub hour: u16,
    pub minute: u16,
    /// 0 = Sunday ... 6 = Saturday
    pub weekday: u16,
    pub day: u16,
    /// 1 = January ... 12 = December
    pub month: u16,
}

/// Reads a system font from Windows' font folder, so we don't ship a font file.
pub fn load_font() -> Option<Font> {
    load_windows_font(&["segoeuil.ttf", "segoeui.ttf", "arial.ttf"])
}

pub fn load_spotify_font() -> Option<Font> {
    load_windows_font(&["oldlondon.ttf", "OLDENGL.TTF"]).or_else(|| load_font())
}

fn load_windows_font(names: &[&str]) -> Option<Font> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    for name in names {
        let path = std::path::Path::new(&windir).join("Fonts").join(name);
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                return Some(font);
            }
        }
    }
    None
}

/// Loads a picture and shrinks/crops it to exactly width x height.
pub fn load_photo(path: &str, width: u32, height: u32) -> Option<Pixmap> {
    if path.is_empty() {
        return None;
    }
    let img = image::open(path).ok()?;
    let rgba = img
        .resize_to_fill(width, height, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    // The drawing library wants premultiplied colors (already multiplied by opacity).
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        px[0] = (px[0] as u32 * a / 255) as u8;
        px[1] = (px[1] as u32 * a / 255) as u8;
        px[2] = (px[2] as u32 * a / 255) as u8;
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h)?)
}

/// Empty space around the card, in pixels, for the shadow and outline.
pub fn margin(cfg: &WidgetConfig, scale: f32) -> f32 {
    let shadow = if cfg.shadow {
        cfg.shadow_blur.max(0.0) + cfg.shadow_offset.abs()
    } else {
        0.0
    };
    ((shadow + cfg.border_width.max(0.0) + 2.0) * scale).ceil()
}

/// A rounded rectangle path. The radius is clamped so it always fits.
pub fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.max(0.0).min(w / 2.0).min(h / 2.0);
    let k = 0.5523 * r; // makes a Bezier curve look like a quarter circle
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Fills a rounded rectangle with a color (rgba, 0-255).
pub fn fill_rrect(pix: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, rgba: [u8; 4]) {
    let Some(path) = rrect(x, y, w, h, r) else {
        return;
    };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    pix.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// Draws the outline of a rounded rectangle.
pub fn stroke_rrect(
    pix: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    width: f32,
    rgba: [u8; 4],
) {
    let Some(path) = rrect(x, y, w, h, r) else {
        return;
    };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    let mut stroke = Stroke::default();
    stroke.width = width;
    pix.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// A soft shadow, built from many slightly larger, fainter shapes.
pub fn soft_shadow(
    pix: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    blur: f32,
    offset: f32,
    opacity: f32,
    rgb: [u8; 3],
) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    // Each layer needs at least 2/255 of alpha to show, so limit the number of layers.
    let max_steps = ((opacity * 255.0) / 2.0).max(1.0) as usize;
    let by_blur = if blur <= 0.5 {
        1
    } else {
        ((blur / 1.5).ceil() as usize).clamp(2, 40)
    };
    let steps = by_blur.min(max_steps).max(1);
    let a = ((opacity / steps as f32) * 255.0).round() as u8;
    for i in 0..steps {
        let grow = if blur <= 0.5 {
            0.0
        } else {
            blur * i as f32 / steps as f32
        };
        fill_rrect(
            pix,
            x - grow,
            y + offset - grow,
            w + 2.0 * grow,
            h + 2.0 * grow,
            r + grow,
            [rgb[0], rgb[1], rgb[2], a],
        );
    }
}

/// Width of a text line in pixels.
pub fn text_width(font: &Font, text: &str, px: f32) -> f32 {
    text.chars()
        .map(|c| font.metrics(c, px).advance_width)
        .sum::<f32>()
}

pub fn wrap_text(font: &Font, text: &str, px: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for source in text.split('\n') {
        let mut line = String::new();
        for word in source.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", line, word)
            };
            if !line.is_empty() && text_width(font, &candidate, px) > max_width {
                lines.push(std::mem::take(&mut line));
                line.push_str(word);
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// Draws text with its left edge at x and its baseline at `baseline`.
pub fn draw_text(
    pix: &mut Pixmap,
    font: &Font,
    text: &str,
    px: f32,
    x: f32,
    baseline: f32,
    rgba: [u8; 4],
) {
    let width = pix.width() as i32;
    let height = pix.height() as i32;
    let data = pix.data_mut();
    let mut pen = x;

    for ch in text.chars() {
        let (m, coverage) = font.rasterize(ch, px);
        let gx = (pen + m.xmin as f32).round() as i32;
        let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;

        for row in 0..m.height {
            for col in 0..m.width {
                let cov = coverage[row * m.width + col] as u32;
                if cov == 0 {
                    continue;
                }
                let dx = gx + col as i32;
                let dy = gy + row as i32;
                if dx < 0 || dy < 0 || dx >= width || dy >= height {
                    continue;
                }
                // Source-over blending with premultiplied alpha.
                let a = cov * rgba[3] as u32 / 255;
                let inv = 255 - a;
                let i = ((dy * width + dx) * 4) as usize;
                data[i] = (rgba[0] as u32 * a / 255 + data[i] as u32 * inv / 255) as u8;
                data[i + 1] = (rgba[1] as u32 * a / 255 + data[i + 1] as u32 * inv / 255) as u8;
                data[i + 2] = (rgba[2] as u32 * a / 255 + data[i + 2] as u32 * inv / 255) as u8;
                data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
            }
        }
        pen += m.advance_width;
    }
}

/// Font size that fits the text inside max_w, but never bigger than max_px.
fn fit_px(font: &Font, text: &str, max_w: f32, max_px: f32) -> f32 {
    let unit = text_width(font, text, 1.0);
    let fit = if unit > 0.0 {
        max_w * 0.9 / unit
    } else {
        max_px
    };
    max_px.min(fit).max(4.0)
}

/// One widget's picture in memory.
pub struct Canvas {
    pub pixmap: Pixmap,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Option<Canvas> {
        Some(Canvas {
            pixmap: Pixmap::new(width, height)?,
        })
    }

    /// Draws the whole widget: card (or picture), outline and clock text.
    pub fn draw(
        &mut self,
        font: Option<&Font>,
        cfg: &WidgetConfig,
        scale: f32,
        photo: Option<&Pixmap>,
        t: &ClockData,
    ) {
        self.pixmap.fill(Color::TRANSPARENT);
        let m = margin(cfg, scale);
        let w = self.pixmap.width() as f32 - 2.0 * m;
        let h = self.pixmap.height() as f32 - 2.0 * m;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = (cfg.radius * scale).clamp(0.0, w.min(h) / 2.0);
        let alpha = cfg.opacity.clamp(0.0, 1.0);
        let pix = &mut self.pixmap;

        if cfg.shadow {
            soft_shadow(
                pix,
                m,
                m,
                w,
                h,
                r,
                cfg.shadow_blur * scale,
                cfg.shadow_offset * scale,
                cfg.shadow_opacity,
                parse_hex(&cfg.shadow_color),
            );
        }

        let border_width = (cfg.border_width * scale).clamp(0.0, w.min(h) / 2.0);
        if border_width > 0.0 && cfg.border_opacity > 0.0 {
            let [br, bg, bb] = parse_hex(&cfg.border_color);
            fill_rrect(
                pix,
                m,
                m,
                w,
                h,
                r,
                [
                    br,
                    bg,
                    bb,
                    (cfg.border_opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                ],
            );
        }

        let content_inset = border_width;
        let content_w = w - content_inset * 2.0;
        let content_h = h - content_inset * 2.0;
        if content_w <= 0.0 || content_h <= 0.0 {
            return;
        }
        let content_x = m + content_inset;
        let content_y = m + content_inset;
        let content_r = (r - content_inset).clamp(0.0, content_w.min(content_h) / 2.0);

        match photo {
            Some(img) => {
                let mut paint = Paint::default();
                paint.anti_alias = true;
                paint.shader = Pattern::new(
                    img.as_ref(),
                    SpreadMode::Pad,
                    FilterQuality::Bilinear,
                    alpha,
                    Transform::from_translate(content_x, content_y),
                );
                if let Some(path) = rrect(content_x, content_y, content_w, content_h, content_r) {
                    pix.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            None => {
                let [cr, cg, cb] = parse_hex(&cfg.background);
                fill_rrect(
                    pix,
                    content_x,
                    content_y,
                    content_w,
                    content_h,
                    content_r,
                    [cr, cg, cb, (alpha * 255.0).round() as u8],
                );
            }
        }

        if cfg.kind == "clock" {
            if let Some(f) = font {
                draw_clock_text(pix, f, cfg, scale, t, content_x, content_w, content_h);
            }
        } else if cfg.kind == "note" {
            if let Some(f) = font {
                draw_note_text(pix, f, cfg, content_x, content_y, content_w, content_h);
            }
        } else if cfg.kind == "spotify" {
            if let Some(f) = font {
                draw_spotify_text(
                    pix,
                    f,
                    cfg,
                    &crate::spotify::snapshot(),
                    content_x,
                    content_y,
                    content_w,
                    content_h,
                    content_r,
                );
            }
        }
    }

    /// Copies the picture into a Windows bitmap buffer (Windows wants B,G,R,A).
    pub fn copy_to_bgra(&self, dst: &mut [u8]) {
        let src = self.pixmap.data();
        let n = src.len().min(dst.len());
        for (d, s) in dst[..n].chunks_exact_mut(4).zip(src[..n].chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
}

fn draw_note_text(
    pix: &mut Pixmap,
    font: &Font,
    cfg: &WidgetConfig,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let [r, g, b] = parse_hex(&cfg.text_color);
    let px = (h * 0.12 * cfg.text_scale.clamp(0.5, 2.0)).clamp(12.0, 28.0);
    let line_height = px * 1.4;
    let padding = (px * 0.7).min(18.0);
    let lines = wrap_text(font, &cfg.text, px, (w - padding * 2.0).max(1.0));
    let max_lines = ((h - padding * 2.0) / line_height).floor().max(0.0) as usize;
    for (index, line) in lines.iter().take(max_lines).enumerate() {
        draw_text(
            pix,
            font,
            line,
            px,
            x + padding,
            y + padding + px + index as f32 * line_height,
            [r, g, b, 255],
        );
    }
}

fn draw_spotify_text(
    pix: &mut Pixmap,
    font: &Font,
    cfg: &WidgetConfig,
    state: &crate::spotify::SpotifySnapshot,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
) {
    let [r, g, b] = parse_hex(&cfg.text_color);
    let padding = (w * 0.06).clamp(12.0, 24.0);
    let available = (w - padding * 2.0).max(1.0);
    let muted = [165, 176, 166, 255];
    if !cfg.path.is_empty() {
        fill_rrect(pix, x, y, w, h, radius, [0, 0, 0, 112]);
    }

    let title = if state.title.is_empty() {
        if state.connected {
            "Nothing is playing"
        } else {
            "Connect Spotify in Settings"
        }
    } else {
        &state.title
    };
    let title_px = fit_px(font, title, available, (h * 0.26).clamp(22.0, 40.0));
    draw_text(
        pix,
        font,
        title,
        title_px,
        x + (w - text_width(font, title, title_px)) / 2.0,
        y + h * 0.34,
        [r, g, b, 255],
    );

    let subtitle = if state.artist.is_empty() {
        &state.status
    } else {
        &state.artist
    };
    let artist_px = (h * 0.13).clamp(14.0, 19.0);
    let subtitle = fit_text(font, subtitle, available, artist_px);
    draw_text(
        pix,
        font,
        &subtitle,
        artist_px,
        x + (w - text_width(font, &subtitle, artist_px)) / 2.0,
        y + h * 0.54,
        muted,
    );

    if !state.album.is_empty() {
        let album_px = 14.0_f32.min((h * 0.1).max(12.0));
        let album = fit_text(font, &state.album, available, album_px);
        draw_text(
            pix,
            font,
            &album,
            album_px,
            x + (w - text_width(font, &album, album_px)) / 2.0,
            y + h * 0.66,
            muted,
        );
    }

    let icon_size = (h * 0.15).clamp(20.0, 26.0);
    let control_y = y + h - icon_size - 8.0;
    for (index, control) in [
        PlaybackIcon::Previous,
        PlaybackIcon::Toggle,
        PlaybackIcon::Next,
    ]
    .iter()
    .enumerate()
    {
        let center = x + w * (index as f32 + 0.5) / 3.0;
        draw_playback_icon(
            pix,
            center,
            control_y,
            icon_size,
            *control,
            state.is_playing,
        );
    }
}

#[derive(Clone, Copy)]
enum PlaybackIcon {
    Previous,
    Toggle,
    Next,
}

fn draw_playback_icon(
    pix: &mut Pixmap,
    cx: f32,
    cy: f32,
    size: f32,
    icon: PlaybackIcon,
    is_playing: bool,
) {
    fill_rrect(
        pix,
        cx - size,
        cy - size,
        size * 2.0,
        size * 2.0,
        size,
        [34, 105, 57, 230],
    );
    let white = [255, 255, 255, 255];
    match icon {
        PlaybackIcon::Previous => {
            fill_rrect(
                pix,
                cx - size * 0.52,
                cy - size * 0.46,
                size * 0.16,
                size * 0.92,
                size * 0.08,
                white,
            );
            fill_triangle(
                pix,
                [
                    (cx + size * 0.25, cy - size * 0.48),
                    (cx - size * 0.3, cy),
                    (cx + size * 0.25, cy + size * 0.48),
                ],
                white,
            );
        }
        PlaybackIcon::Next => {
            fill_triangle(
                pix,
                [
                    (cx - size * 0.25, cy - size * 0.48),
                    (cx + size * 0.3, cy),
                    (cx - size * 0.25, cy + size * 0.48),
                ],
                white,
            );
            fill_rrect(
                pix,
                cx + size * 0.36,
                cy - size * 0.46,
                size * 0.16,
                size * 0.92,
                size * 0.08,
                white,
            );
        }
        PlaybackIcon::Toggle if is_playing => {
            fill_rrect(
                pix,
                cx - size * 0.43,
                cy - size * 0.45,
                size * 0.28,
                size * 0.9,
                size * 0.08,
                white,
            );
            fill_rrect(
                pix,
                cx + size * 0.15,
                cy - size * 0.45,
                size * 0.28,
                size * 0.9,
                size * 0.08,
                white,
            );
        }
        PlaybackIcon::Toggle => fill_triangle(
            pix,
            [
                (cx - size * 0.28, cy - size * 0.5),
                (cx + size * 0.42, cy),
                (cx - size * 0.28, cy + size * 0.5),
            ],
            white,
        ),
    }
}

fn fill_triangle(pix: &mut Pixmap, points: [(f32, f32); 3], rgba: [u8; 4]) {
    let mut path = PathBuilder::new();
    path.move_to(points[0].0, points[0].1);
    path.line_to(points[1].0, points[1].1);
    path.line_to(points[2].0, points[2].1);
    path.close();
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    pix.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn fit_text(font: &Font, text: &str, max_width: f32, px: f32) -> String {
    if text_width(font, text, px) <= max_width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "...";
        if text_width(font, &candidate, px) <= max_width {
            return candidate;
        }
    }
    "...".into()
}

#[cfg(test)]
mod tests {
    use super::{margin, Canvas, ClockData};
    use crate::config::WidgetConfig;

    #[test]
    fn border_ring_uses_configured_color_and_opacity() {
        let cfg: WidgetConfig = serde_json::from_str(
            r##"{"type":"note","width":40,"height":40,"radius":8,"background":"#112233","border_width":4,"border_opacity":0.18,"border_color":"#FF0000","shadow":false}"##,
        )
        .unwrap();
        let margin = margin(&cfg, 1.0);
        let size = (40.0 + margin * 2.0) as u32;
        let mut canvas = Canvas::new(size, size).unwrap();
        let now = ClockData {
            hour: 0,
            minute: 0,
            weekday: 0,
            day: 1,
            month: 1,
        };

        canvas.draw(None, &cfg, 1.0, None, &now);

        let width = canvas.pixmap.width() as usize;
        let data = canvas.pixmap.data();
        let edge = ((margin as usize + 1) * width + margin as usize + 20) * 4;
        let inside = ((margin as usize + 20) * width + margin as usize + 6) * 4;
        assert_eq!(&data[edge..edge + 4], &[46, 0, 0, 46]);
        assert_eq!(&data[inside..inside + 4], &[17, 34, 51, 255]);
    }

    #[test]
    fn widget_opacity_does_not_change_border_opacity() {
        let cfg: WidgetConfig = serde_json::from_str(
            r##"{"type":"clock","width":40,"height":40,"radius":8,"opacity":0.25,"background":"#112233","border_width":4,"border_opacity":0.8,"border_color":"#FF0000","shadow":false}"##,
        )
        .unwrap();
        let margin = margin(&cfg, 1.0);
        let size = (40.0 + margin * 2.0) as u32;
        let mut canvas = Canvas::new(size, size).unwrap();
        let now = ClockData {
            hour: 0,
            minute: 0,
            weekday: 0,
            day: 1,
            month: 1,
        };

        canvas.draw(None, &cfg, 1.0, None, &now);

        let width = canvas.pixmap.width() as usize;
        let edge = ((margin as usize + 1) * width + margin as usize + 20) * 4;
        assert_eq!(&canvas.pixmap.data()[edge..edge + 4], &[204, 0, 0, 204]);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_clock_text(
    pix: &mut Pixmap,
    font: &Font,
    cfg: &WidgetConfig,
    _scale: f32,
    t: &ClockData,
    m: f32,
    w: f32,
    h: f32,
) {
    let ts = cfg.text_scale.clamp(0.5, 2.0);
    let [r, g, b] = parse_hex(&cfg.text_color);

    let time = if cfg.hour12 {
        let hh = match t.hour % 12 {
            0 => 12,
            x => x,
        };
        format!("{}:{:02}", hh, t.minute)
    } else {
        format!("{:02}:{:02}", t.hour, t.minute)
    };
    let time_px = fit_px(font, &time, w, h * 0.5 * ts);
    let time_w = text_width(font, &time, time_px);
    let time_base = if cfg.show_date {
        m + h * 0.6
    } else {
        m + h * 0.5 + time_px * 0.35
    };
    draw_text(
        pix,
        font,
        &time,
        time_px,
        m + (w - time_w) / 2.0,
        time_base,
        [r, g, b, 255],
    );

    if cfg.show_date {
        let date = format!(
            "{}, {} {}",
            WEEKDAYS[(t.weekday as usize) % 7],
            t.day,
            MONTHS[((t.month as usize).max(1) - 1) % 12]
        );
        let date_px = fit_px(font, &date, w, h * 0.13 * ts);
        let date_w = text_width(font, &date, date_px);
        draw_text(
            pix,
            font,
            &date,
            date_px,
            m + (w - date_w) / 2.0,
            m + h * 0.86,
            [r, g, b, 200],
        );
    }
}
