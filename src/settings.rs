//! The settings window is drawn as a compact, flat preferences panel. It only
//! exists while it is open, so it uses no memory the rest of the time.

use std::cell::RefCell;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use fontdue::Font;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::Dialogs::{
    ChooseColorW, GetOpenFileNameW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, OFN_FILEMUSTEXIST,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{parse_hex, Config, WidgetConfig};
use crate::draw::{self, Canvas};

const CLASS_NAME: &str = "PicoSettings";
const TITLE: &str = "pico-widgets settings";

// Layout, in logical pixels (scaled for your screen automatically).
const CARD_W: f32 = 760.0;
const CARD_H: f32 = 680.0;
const MARGIN: f32 = 0.0;
const HEADER_H: f32 = 56.0;
const SIDE_X: f32 = 16.0;
const SIDE_W: f32 = 200.0;
const RIGHT_X: f32 = 240.0;
const RIGHT_W: f32 = 496.0;
const TABS: [&str; 6] = ["Appearance", "Effects", "Clock", "Photo", "Note", "Spotify"];

// Colors, RGBA 0-255.
const COL_ACCENT: [u8; 4] = [74, 166, 100, 255];
const COL_WHITE: [u8; 4] = [255, 255, 255, 255];
const COL_TEXT: [u8; 4] = [235, 239, 235, 255];
const COL_SOFT: [u8; 4] = [163, 172, 164, 255];
const COL_FILL: [u8; 4] = [48, 55, 49, 198];
const COL_HOT: [u8; 4] = [59, 82, 63, 224];
const COL_BORDER: [u8; 4] = [110, 132, 112, 190];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dim {
    Width,
    Height,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Number {
    Slider(Slide),
    Dimension(Dim),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Slide {
    Radius,
    Opacity,
    Border,
    BorderOpacity,
    Blur,
    ShadowStrength,
    ShadowOffset,
    TextScale,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tog {
    Shadow,
    Locked,
    ShowDate,
    Hour12,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Swat {
    Border,
    ShadowColor,
    Text,
    Background,
}

/// Everything you can click or drag in the window.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Tab(usize),
    List(usize),
    AddClock,
    AddPhoto,
    AddNote,
    AddSpotify,
    Delete,
    Browse,
    NoteText,
    SpotifyClientId,
    SpotifyConnect,
    Save,
    Close,
    Step(Dim, i32),
    Slider(Slide),
    Number(Number),
    Toggle(Tog),
    Swatch(Swat),
}

fn slide_range(s: Slide) -> (f32, f32) {
    match s {
        Slide::Radius => (0.0, 150.0),
        Slide::Opacity | Slide::BorderOpacity | Slide::ShadowStrength => (0.0, 1.0),
        Slide::Border => (0.0, 6.0),
        Slide::Blur => (0.0, 60.0),
        Slide::ShadowOffset => (0.0, 30.0),
        Slide::TextScale => (0.5, 2.0),
    }
}

fn slide_get(w: &WidgetConfig, s: Slide) -> f32 {
    match s {
        Slide::Radius => w.radius,
        Slide::Opacity => w.opacity,
        Slide::Border => w.border_width,
        Slide::BorderOpacity => w.border_opacity,
        Slide::Blur => w.shadow_blur,
        Slide::ShadowStrength => w.shadow_opacity,
        Slide::ShadowOffset => w.shadow_offset,
        Slide::TextScale => w.text_scale,
    }
}

fn slide_set(w: &mut WidgetConfig, s: Slide, v: f32) {
    match s {
        Slide::Radius => w.radius = v,
        Slide::Opacity => w.opacity = v,
        Slide::Border => w.border_width = v,
        Slide::BorderOpacity => w.border_opacity = v,
        Slide::Blur => w.shadow_blur = v,
        Slide::ShadowStrength => w.shadow_opacity = v,
        Slide::ShadowOffset => w.shadow_offset = v,
        Slide::TextScale => w.text_scale = v,
    }
}

fn slide_name(s: Slide) -> &'static str {
    match s {
        Slide::Radius => "Corner roundness",
        Slide::Opacity => "Background opacity",
        Slide::Border => "Border width",
        Slide::BorderOpacity => "Border opacity",
        Slide::Blur => "Shadow blur",
        Slide::ShadowStrength => "Shadow strength",
        Slide::ShadowOffset => "Shadow offset",
        Slide::TextScale => "Text size",
    }
}

fn slide_value(s: Slide, v: f32) -> String {
    match s {
        Slide::Opacity | Slide::BorderOpacity | Slide::ShadowStrength | Slide::TextScale => {
            format!("{:.0}%", v * 100.0)
        }
        Slide::Border => format!("{:.1} px", v),
        _ => format!("{:.0} px", v),
    }
}

fn slide_input(s: Slide, v: f32) -> String {
    match s {
        Slide::Opacity | Slide::BorderOpacity | Slide::ShadowStrength | Slide::TextScale => {
            format!("{:.0}", v * 100.0)
        }
        Slide::Border => format!("{:.1}", v),
        _ => format!("{:.0}", v),
    }
}

fn swat_get(w: &WidgetConfig, sw: Swat) -> &str {
    match sw {
        Swat::Border => &w.border_color,
        Swat::ShadowColor => &w.shadow_color,
        Swat::Text => &w.text_color,
        Swat::Background => &w.background,
    }
}

fn swat_set(w: &mut WidgetConfig, sw: Swat, hex: String) {
    match sw {
        Swat::Border => w.border_color = hex,
        Swat::ShadowColor => w.shadow_color = hex,
        Swat::Text => w.text_color = hex,
        Swat::Background => w.background = hex,
    }
}

fn list_label(w: &WidgetConfig) -> String {
    if w.kind == "photo" {
        let name = std::path::Path::new(&w.path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("no file chosen");
        format!("Photo: {}", name)
    } else if w.kind == "note" {
        "Notepad".to_string()
    } else if w.kind == "spotify" {
        "Spotify music".to_string()
    } else {
        "Clock".to_string()
    }
}

fn clock_default() -> WidgetConfig {
    serde_json::from_str(r#"{"type":"clock"}"#).expect("default clock")
}

fn photo_default() -> WidgetConfig {
    serde_json::from_str(r#"{"type":"photo","width":240,"height":240,"radius":40,"opacity":1.0}"#)
        .expect("default photo")
}

fn note_default() -> WidgetConfig {
    serde_json::from_str(r#"{"type":"note","width":300,"height":240,"radius":8,"opacity":1.0}"#)
        .expect("default notepad")
}

fn spotify_default() -> WidgetConfig {
    serde_json::from_str(r#"{"type":"spotify","width":340,"height":180,"radius":12,"opacity":1.0}"#)
        .expect("default Spotify widget")
}

/// Draws the layout into `canvas` (when given) and returns where every control is.
/// Painting and hit-testing use the same code, so they always agree.
fn build<'a>(
    widgets: &[WidgetConfig],
    selected: Option<usize>,
    tab: usize,
    hover: Option<Ctl>,
    note_focused: bool,
    number_edit: Option<Number>,
    number_text: &'a str,
    spotify_client_id: &'a str,
    spotify_id_focused: bool,
    font: Option<&'a Font>,
    s: f32,
    canvas: Option<&'a mut Canvas>,
) -> Vec<(Ctl, [f32; 4])> {
    let mut p = Painter {
        canvas,
        font,
        s,
        hover,
        number_edit,
        number_text,
        hits: Vec::new(),
        y: 0.0,
    };
    let w = selected.and_then(|i| widgets.get(i));

    p.card();
    p.text(24.0, 38.0, 20.0, "pico-widgets", COL_TEXT);
    p.text(RIGHT_X, 38.0, 15.0, TABS[tab.min(TABS.len() - 1)], COL_SOFT);
    p.button(Ctl::Close, CARD_W - 48.0, 14.0, 28.0, 28.0, "×", false);

    p.rect(SIDE_X, 72.0, SIDE_W, CARD_H - 88.0, 8.0, [38, 44, 39, 184]);
    p.rect(
        SIDE_X + SIDE_W + 7.0,
        72.0,
        1.0,
        CARD_H - 88.0,
        0.0,
        [255, 255, 255, 24],
    );
    p.text(28.0, 94.0, 11.0, "WIDGETS", COL_SOFT);
    for (i, item) in widgets.iter().enumerate() {
        let y = 102.0 + i as f32 * 25.0;
        if y > 252.0 {
            break;
        }
        if selected == Some(i) {
            p.rect(24.0, y, 184.0, 23.0, 5.0, COL_ACCENT);
        } else if hover == Some(Ctl::List(i)) {
            p.rect(24.0, y, 184.0, 23.0, 5.0, COL_HOT);
        }
        let shown = p.fit_text(&list_label(item), 13.0, 164.0);
        p.text(
            36.0,
            y + 16.0,
            13.0,
            &shown,
            if selected == Some(i) {
                COL_WHITE
            } else {
                COL_TEXT
            },
        );
        p.hit(Ctl::List(i), 24.0, y, 184.0, 23.0);
    }
    p.text(28.0, 292.0, 11.0, "SETTINGS", COL_SOFT);
    for (i, name) in TABS.iter().enumerate() {
        let y = 302.0 + i as f32 * 31.0;
        if i == tab {
            p.rect(24.0, y, 184.0, 27.0, 5.0, COL_ACCENT);
        } else if hover == Some(Ctl::Tab(i)) {
            p.rect(24.0, y, 184.0, 27.0, 5.0, COL_HOT);
        }
        p.text(
            36.0,
            y + 18.0,
            13.0,
            name,
            if i == tab { COL_WHITE } else { COL_TEXT },
        );
        p.hit(Ctl::Tab(i), 24.0, y, 184.0, 27.0);
    }
    p.button(Ctl::AddClock, 24.0, 518.0, 88.0, 30.0, "Clock +", false);
    p.button(Ctl::AddPhoto, 120.0, 518.0, 88.0, 30.0, "Photo +", false);
    p.button(Ctl::AddNote, 24.0, 554.0, 88.0, 30.0, "Note +", false);
    p.button(Ctl::AddSpotify, 120.0, 554.0, 88.0, 30.0, "Music +", false);
    p.button(
        Ctl::Delete,
        24.0,
        590.0,
        184.0,
        30.0,
        "Remove selected",
        false,
    );

    // Tab contents.
    p.y = 112.0;
    match (w, tab) {
        (None, _) => {
            p.text(
                RIGHT_X,
                150.0,
                14.0,
                "Select a widget on the left, or add one.",
                COL_SOFT,
            );
        }
        (Some(w), 0) => {
            p.stepper_row(Dim::Width, "Width", w.width);
            p.stepper_row(Dim::Height, "Height", w.height);
            p.slider_row(Slide::Radius, w);
            p.slider_row(Slide::Border, w);
            p.slider_row(Slide::Opacity, w);
            p.slider_row(Slide::BorderOpacity, w);
            p.swatch_row(Swat::Border, "Border color", &w.border_color);
        }
        (Some(w), 1) => {
            p.toggle_row(Tog::Shadow, "Drop shadow", w.shadow);
            p.toggle_row(Tog::Locked, "Lock position", w.locked);
            p.slider_row(Slide::Blur, w);
            p.slider_row(Slide::ShadowStrength, w);
            p.slider_row(Slide::ShadowOffset, w);
            p.swatch_row(Swat::ShadowColor, "Shadow color", &w.shadow_color);
        }
        (Some(w), 2) => {
            p.toggle_row(Tog::ShowDate, "Show date", w.show_date);
            p.toggle_row(Tog::Hour12, "12-hour clock", w.hour12);
            p.slider_row(Slide::TextScale, w);
            p.swatch_row(Swat::Text, "Text color", &w.text_color);
            p.swatch_row(Swat::Background, "Background color", &w.background);
        }
        (Some(w), 3) => {
            if w.kind == "photo" || w.kind == "spotify" {
                let label = if w.kind == "spotify" {
                    "Widget background photo"
                } else {
                    "Photo file"
                };
                p.text(RIGHT_X, 150.0, 13.0, label, COL_SOFT);
                let name = if w.path.is_empty() {
                    "No picture chosen".to_string()
                } else {
                    std::path::Path::new(&w.path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("picture")
                        .to_string()
                };
                let shown = p.fit_text(&name, 14.0, RIGHT_W - 140.0);
                p.text(RIGHT_X, 180.0, 14.0, &shown, COL_TEXT);
                p.button(
                    Ctl::Browse,
                    RIGHT_X + RIGHT_W - 124.0,
                    160.0,
                    124.0,
                    30.0,
                    "Choose photo...",
                    false,
                );
                p.text(
                    RIGHT_X,
                    214.0,
                    12.0,
                    if w.kind == "spotify" {
                        "Photo is cropped behind track details and controls."
                    } else {
                        "The picture is cut to the widget's rounded shape."
                    },
                    COL_SOFT,
                );
            } else {
                p.text(
                    RIGHT_X,
                    150.0,
                    14.0,
                    "Select a photo widget on the left, or add one.",
                    COL_SOFT,
                );
            }
        }
        (Some(w), 4) if w.kind == "note" => {
            p.text(RIGHT_X, 142.0, 14.0, "Note text", COL_TEXT);
            p.note_area(&w.text, note_focused);
            p.text(
                RIGHT_X,
                510.0,
                12.0,
                "Click the field to edit. Save to apply.",
                COL_SOFT,
            );
        }
        (_, 5) => {
            let status = crate::spotify::snapshot();
            p.text(RIGHT_X, 142.0, 15.0, "Connect Spotify", COL_TEXT);
            p.text(
                RIGHT_X,
                168.0,
                12.0,
                "Spotify Developer Client ID",
                COL_SOFT,
            );
            p.rect(RIGHT_X, 178.0, RIGHT_W, 34.0, 5.0, COL_FILL);
            p.stroke(
                RIGHT_X,
                178.0,
                RIGHT_W,
                34.0,
                5.0,
                1.0,
                if spotify_id_focused {
                    COL_ACCENT
                } else {
                    COL_BORDER
                },
            );
            let shown = if spotify_client_id.is_empty() {
                "Paste your Client ID here"
            } else {
                spotify_client_id
            };
            p.text(RIGHT_X + 10.0, 200.0, 13.0, shown, COL_TEXT);
            p.hit(Ctl::SpotifyClientId, RIGHT_X, 178.0, RIGHT_W, 34.0);
            p.button(
                Ctl::SpotifyConnect,
                RIGHT_X,
                226.0,
                150.0,
                32.0,
                if status.connecting {
                    "Connecting..."
                } else if status.connected {
                    "Reconnect"
                } else {
                    "Connect"
                },
                true,
            );
            let status_text = p.fit_text(&status.status, 13.0, RIGHT_W);
            p.text(RIGHT_X, 286.0, 13.0, &status_text, COL_TEXT);
            p.text(
                RIGHT_X,
                330.0,
                12.0,
                "Register redirect URI: http://127.0.0.1:43821/callback",
                COL_SOFT,
            );
            p.text(
                RIGHT_X,
                352.0,
                12.0,
                "Playback controls require Spotify Premium.",
                COL_SOFT,
            );
        }
        (Some(_), _) => {
            p.text(
                RIGHT_X,
                150.0,
                14.0,
                "Select a notepad widget to edit its text.",
                COL_SOFT,
            );
        }
    }

    // Footer.
    p.button(
        Ctl::Save,
        CARD_W - 24.0 - 110.0 - 10.0 - 96.0,
        CARD_H - 62.0,
        110.0,
        34.0,
        "Save",
        true,
    );
    p.button(
        Ctl::Close,
        CARD_W - 24.0 - 96.0,
        CARD_H - 62.0,
        96.0,
        34.0,
        "Close",
        false,
    );

    p.hits
}

/// Paints controls and records where they are. All positions are logical;
/// the card's top-left corner is at (MARGIN, MARGIN) inside the window.
struct Painter<'a> {
    canvas: Option<&'a mut Canvas>,
    font: Option<&'a Font>,
    s: f32,
    hover: Option<Ctl>,
    number_edit: Option<Number>,
    number_text: &'a str,
    hits: Vec<(Ctl, [f32; 4])>,
    /// Running height for stacked rows.
    y: f32,
}

impl<'a> Painter<'a> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, rgba: [u8; 4]) {
        let s = self.s;
        if let Some(c) = self.canvas.as_mut() {
            draw::fill_rrect(
                &mut c.pixmap,
                (x + MARGIN) * s,
                (y + MARGIN) * s,
                w * s,
                h * s,
                r * s,
                rgba,
            );
        }
    }

    fn stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, rgba: [u8; 4]) {
        let s = self.s;
        if let Some(c) = self.canvas.as_mut() {
            draw::stroke_rrect(
                &mut c.pixmap,
                (x + MARGIN) * s,
                (y + MARGIN) * s,
                w * s,
                h * s,
                r * s,
                width * s,
                rgba,
            );
        }
    }

    fn hit(&mut self, ctl: Ctl, x: f32, y: f32, w: f32, h: f32) {
        self.hits.push((ctl, [x, y, w, h]));
    }

    /// Width of a text line in logical pixels.
    fn measure(&self, t: &str, px: f32) -> f32 {
        match self.font {
            Some(f) => draw::text_width(f, t, px * self.s) / self.s,
            None => 0.0,
        }
    }

    /// Draws text with its left edge at x and its baseline at `base`.
    fn text(&mut self, x: f32, base: f32, px: f32, t: &str, rgba: [u8; 4]) {
        let s = self.s;
        if let (Some(c), Some(f)) = (self.canvas.as_mut(), self.font) {
            draw::draw_text(
                &mut c.pixmap,
                f,
                t,
                px * s,
                (x + MARGIN) * s,
                (base + MARGIN) * s,
                rgba,
            );
        }
    }

    fn text_right(&mut self, right: f32, base: f32, px: f32, t: &str, rgba: [u8; 4]) {
        let w = self.measure(t, px);
        self.text(right - w, base, px, t, rgba);
    }

    fn text_center(&mut self, cx: f32, base: f32, px: f32, t: &str, rgba: [u8; 4]) {
        let w = self.measure(t, px);
        self.text(cx - w / 2.0, base, px, t, rgba);
    }

    /// Shortens text with "..." so it fits in max_w.
    fn fit_text(&self, t: &str, px: f32, max_w: f32) -> String {
        if self.measure(t, px) <= max_w {
            return t.to_string();
        }
        let mut chars: Vec<char> = t.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let candidate: String = chars.iter().collect::<String>() + "...";
            if self.measure(&candidate, px) <= max_w {
                return candidate;
            }
        }
        "...".to_string()
    }

    /// The opaque settings surface.
    fn card(&mut self) {
        self.rect(0.0, 0.0, CARD_W, CARD_H, 10.0, [27, 31, 28, 208]);
        self.rect(0.0, 0.0, CARD_W, 3.0, 1.5, [255, 255, 255, 26]);
        self.stroke(0.0, 0.0, CARD_W, CARD_H, 10.0, 1.0, COL_BORDER);
    }

    /// A rounded button with a centered label. Primary buttons use the accent color.
    fn button(&mut self, ctl: Ctl, x: f32, y: f32, w: f32, h: f32, label: &str, primary: bool) {
        let hot = self.hover == Some(ctl);
        let fill = if primary {
            COL_ACCENT
        } else if hot {
            COL_HOT
        } else {
            COL_FILL
        };
        self.rect(x, y, w, h, 6.0, fill);
        self.text_center(x + w / 2.0, y + h / 2.0 + 4.5, 13.0, label, COL_WHITE);
        self.hit(ctl, x, y, w, h);
    }

    /// "Width  [-]  300  [+]"
    fn stepper_row(&mut self, d: Dim, label: &str, value: u32) {
        let y = self.y;
        self.text(RIGHT_X, y + 22.0, 14.0, label, COL_TEXT);
        let mx = RIGHT_X + RIGHT_W - 120.0;
        self.button(Ctl::Step(d, -10), mx, y + 4.0, 32.0, 28.0, "-", false);
        let target = Number::Dimension(d);
        let value_text = if self.number_edit == Some(target) {
            self.number_text.to_string()
        } else {
            value.to_string()
        };
        self.rect(mx + 36.0, y + 4.0, 48.0, 28.0, 5.0, COL_FILL);
        self.stroke(
            mx + 36.0,
            y + 4.0,
            48.0,
            28.0,
            5.0,
            1.0,
            if self.number_edit == Some(target) {
                COL_ACCENT
            } else {
                COL_BORDER
            },
        );
        self.text_center(mx + 60.0, y + 23.0, 13.0, &value_text, COL_TEXT);
        self.hit(Ctl::Number(target), mx + 36.0, y + 4.0, 48.0, 28.0);
        self.button(Ctl::Step(d, 10), mx + 88.0, y + 4.0, 32.0, 28.0, "+", false);
        self.y += 36.0;
    }

    /// A slider with its label and value. Dragging anywhere on the row moves it.
    fn slider_row(&mut self, s: Slide, w: &WidgetConfig) {
        let v = slide_get(w, s);
        let (lo, hi) = slide_range(s);
        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        let y = self.y;
        self.text(RIGHT_X, y + 16.0, 14.0, slide_name(s), COL_TEXT);
        let shown = slide_value(s, v);
        let target = Number::Slider(s);
        let shown = if self.number_edit == Some(target) {
            self.number_text.to_string()
        } else {
            shown
        };
        let field_x = RIGHT_X + RIGHT_W - 72.0;
        self.rect(field_x, y - 2.0, 72.0, 23.0, 5.0, COL_FILL);
        self.stroke(
            field_x,
            y - 2.0,
            72.0,
            23.0,
            5.0,
            1.0,
            if self.number_edit == Some(target) {
                COL_ACCENT
            } else {
                COL_BORDER
            },
        );
        self.text_center(field_x + 36.0, y + 14.0, 12.0, &shown, COL_TEXT);
        self.hit(Ctl::Number(target), field_x, y - 2.0, 72.0, 23.0);
        let ty = y + 30.0;
        self.rect(RIGHT_X, ty, RIGHT_W, 6.0, 3.0, COL_FILL);
        self.rect(RIGHT_X, ty, RIGHT_W * t, 6.0, 3.0, COL_ACCENT);
        self.rect(
            RIGHT_X + RIGHT_W * t - 9.0,
            ty - 6.0,
            18.0,
            18.0,
            9.0,
            COL_WHITE,
        );
        self.hit(Ctl::Slider(s), RIGHT_X, y + 22.0, RIGHT_W, 22.0);
        self.y += 44.0;
    }

    /// A switch that turns a setting on or off.
    fn toggle_row(&mut self, t: Tog, label: &str, on: bool) {
        let y = self.y;
        self.text(RIGHT_X, y + 24.0, 14.0, label, COL_TEXT);
        let sx = RIGHT_X + RIGHT_W - 52.0;
        self.rect(
            sx,
            y + 8.0,
            52.0,
            28.0,
            14.0,
            if on { COL_ACCENT } else { COL_FILL },
        );
        let kx = if on { sx + 28.0 } else { sx + 4.0 };
        self.rect(kx, y + 12.0, 20.0, 20.0, 10.0, COL_WHITE);
        self.hit(Ctl::Toggle(t), RIGHT_X, y, RIGHT_W, 40.0);
        self.y += 40.0;
    }

    /// A color: its name, the hex code and a swatch. Click to pick a new color.
    fn swatch_row(&mut self, sw: Swat, label: &str, hex: &str) {
        let y = self.y;
        self.text(RIGHT_X, y + 24.0, 14.0, label, COL_TEXT);
        let rgb = parse_hex(hex);
        let sx = RIGHT_X + RIGHT_W - 64.0;
        self.rect(sx, y + 8.0, 52.0, 24.0, 8.0, [rgb[0], rgb[1], rgb[2], 255]);
        self.stroke(sx, y + 8.0, 52.0, 24.0, 8.0, 1.0, COL_BORDER);
        self.text_right(sx - 10.0, y + 24.0, 12.0, hex, COL_SOFT);
        self.hit(Ctl::Swatch(sw), RIGHT_X, y, RIGHT_W, 40.0);
        self.y += 40.0;
    }

    fn note_area(&mut self, text: &str, focused: bool) {
        let x = RIGHT_X;
        let y = 156.0;
        let w = RIGHT_W;
        let h = 338.0;
        self.rect(x, y, w, h, 5.0, [34, 39, 35, 205]);
        self.stroke(
            x,
            y,
            w,
            h,
            5.0,
            if focused { 2.0 } else { 1.0 },
            if focused { COL_ACCENT } else { COL_BORDER },
        );
        if let Some(font) = self.font {
            let lines = draw::wrap_text(font, text, 14.0 * self.s, (w - 24.0) * self.s);
            let first = lines.len().saturating_sub(17);
            for (index, line) in lines.iter().skip(first).enumerate() {
                self.text(
                    x + 12.0,
                    y + 24.0 + index as f32 * 19.0,
                    14.0,
                    line,
                    COL_TEXT,
                );
            }
        }
        self.hit(Ctl::NoteText, x, y, w, h);
    }
}

// ---------------------------------------------------------------------------
// The window's memory and behaviour
// ---------------------------------------------------------------------------

/// Everything the open settings window remembers.
struct Ui {
    hwnd: HWND,
    canvas: Canvas,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    /// Pixel memory of `bitmap`, owned by Windows.
    bits: *mut u8,
    s: f32,
    font: Option<Font>,
    widgets: Vec<WidgetConfig>,
    source_indices: Vec<Option<usize>>,
    selected: Option<usize>,
    tab: usize,
    hover: Option<Ctl>,
    note_focused: bool,
    spotify_client_id: String,
    spotify_id_focused: bool,
    number_edit: Option<Number>,
    number_text: String,
    number_replace: bool,
    drag: Option<Slide>,
    /// Control positions from the last paint.
    hits: Vec<(Ctl, [f32; 4])>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = RefCell::new(None);
}

/// Runs `f` on the window's data, if it is open.
/// Rule: don't call this inside another with_ui call.
fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|cell| cell.borrow_mut().as_mut().map(f))
}

struct Frame {
    hwnd: HWND,
    mem_dc: HDC,
    width: i32,
    height: i32,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn show_message(text: &str) {
    let t = wide(text);
    let title = wide("pico-widgets");
    unsafe {
        MessageBoxW(null_mut(), t.as_ptr(), title.as_ptr(), MB_ICONWARNING);
    }
}

fn sel_widget(u: &Ui) -> Option<&WidgetConfig> {
    u.widgets.get(u.selected?)
}

fn sel_mut(u: &mut Ui) -> Option<&mut WidgetConfig> {
    let i = u.selected?;
    u.widgets.get_mut(i)
}

fn hit_test(hits: &[(Ctl, [f32; 4])], x: f32, y: f32) -> Option<Ctl> {
    hits.iter()
        .rev()
        .find(|(_, r)| x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])
        .map(|(c, _)| *c)
}

/// Sets a slider from a horizontal position inside its row.
fn set_slider_from_x(u: &mut Ui, s: Slide, lx: f32) {
    let Some(rect) = u
        .hits
        .iter()
        .find(|(c, _)| *c == Ctl::Slider(s))
        .map(|(_, r)| *r)
    else {
        return;
    };
    let t = ((lx - rect[0]) / rect[2]).clamp(0.0, 1.0);
    let (lo, hi) = slide_range(s);
    let v = lo + t * (hi - lo);
    if let Some(w) = sel_mut(u) {
        slide_set(w, s, v);
    }
}

fn add_widget(u: &mut Ui, widget: WidgetConfig) {
    u.widgets.push(widget);
    u.source_indices.push(None);
    u.selected = Some(u.widgets.len() - 1);
}

fn delete_selected(u: &mut Ui) {
    if let Some(i) = u.selected {
        u.widgets.remove(i);
        u.source_indices.remove(i);
        if u.widgets.is_empty() {
            u.widgets.push(clock_default());
            u.source_indices.push(None);
        }
        u.selected = Some(i.min(u.widgets.len() - 1));
    }
}

fn flip(w: &mut WidgetConfig, t: Tog) {
    match t {
        Tog::Shadow => w.shadow = !w.shadow,
        Tog::Locked => w.locked = !w.locked,
        Tog::ShowDate => w.show_date = !w.show_date,
        Tog::Hour12 => w.hour12 = !w.hour12,
    }
}

/// Applies a click that doesn't open a dialog.
fn apply(u: &mut Ui, ctl: Ctl, lx: f32) {
    u.note_focused = ctl == Ctl::NoteText;
    u.spotify_id_focused = ctl == Ctl::SpotifyClientId;
    match ctl {
        Ctl::Tab(i) => u.tab = i,
        Ctl::List(i) => {
            if i < u.widgets.len() {
                u.selected = Some(i);
                u.tab = match u.widgets[i].kind.as_str() {
                    "note" => 4,
                    "spotify" => 5,
                    "photo" => 3,
                    _ => 0,
                };
            }
        }
        Ctl::AddClock => add_widget(u, clock_default()),
        Ctl::AddPhoto => add_widget(u, photo_default()),
        Ctl::AddNote => {
            add_widget(u, note_default());
            u.tab = 4;
        }
        Ctl::AddSpotify => {
            add_widget(u, spotify_default());
            u.tab = 5;
        }
        Ctl::Delete => delete_selected(u),
        Ctl::Step(d, delta) => {
            if let Some(w) = sel_mut(u) {
                let v = match d {
                    Dim::Width => &mut w.width,
                    Dim::Height => &mut w.height,
                };
                *v = (*v as i32 + delta).clamp(40, 2000) as u32;
            }
        }
        Ctl::Slider(s) => {
            u.drag = Some(s);
            set_slider_from_x(u, s, lx);
        }
        Ctl::Number(target) => begin_number_edit(u, target),
        Ctl::Toggle(t) => {
            if let Some(w) = sel_mut(u) {
                flip(w, t);
            }
        }
        Ctl::NoteText => {}
        Ctl::SpotifyClientId | Ctl::SpotifyConnect => {}
        _ => {}
    }
}

/// Paints the window and copies it to the screen.
fn repaint() {
    let frame = with_ui(|u| {
        u.canvas.pixmap.fill(tiny_skia::Color::TRANSPARENT);
        let hits = build(
            &u.widgets,
            u.selected,
            u.tab,
            u.hover,
            u.note_focused,
            u.number_edit,
            &u.number_text,
            &u.spotify_client_id,
            u.spotify_id_focused,
            u.font.as_ref(),
            u.s,
            Some(&mut u.canvas),
        );
        u.hits = hits;
        let width = u.canvas.pixmap.width() as i32;
        let height = u.canvas.pixmap.height() as i32;
        let len = (width * height * 4) as usize;
        // SAFETY: `bits` is a bitmap of exactly width*height*4 bytes owned by this Ui.
        let dst = unsafe { std::slice::from_raw_parts_mut(u.bits, len) };
        u.canvas.copy_to_bgra(dst);
        Frame {
            hwnd: u.hwnd,
            mem_dc: u.mem_dc,
            width,
            height,
        }
    });
    if let Some(f) = frame {
        present(f);
    }
}

fn present(f: Frame) {
    unsafe {
        let size = SIZE {
            cx: f.width,
            cy: f.height,
        };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            f.hwnd,
            null_mut(),
            null(),
            &size,
            f.mem_dc,
            &origin,
            0,
            &blend,
            ULW_ALPHA,
        );
    }
}

fn on_mouse_down(lx: f32, ly: f32) {
    let Some(ctl) = with_ui(|u| hit_test(&u.hits, lx, ly)).flatten() else {
        return;
    };
    with_ui(commit_number_edit);
    with_ui(commit_spotify_client_id);
    match ctl {
        // These open dialogs, so they must run with no state borrow held.
        Ctl::Swatch(sw) => pick_color(sw),
        Ctl::Browse => browse(),
        Ctl::AddSpotify => {
            with_ui(|u| apply(u, ctl, lx));
            save();
            repaint();
        }
        Ctl::SpotifyConnect => {
            save();
            let client_id = with_ui(|u| u.spotify_client_id.clone()).unwrap_or_default();
            crate::spotify::connect(client_id);
            repaint();
        }
        Ctl::Save => save(),
        Ctl::Close => close(),
        _ => {
            with_ui(|u| apply(u, ctl, lx));
            repaint();
        }
    }
}

fn commit_spotify_client_id(u: &mut Ui) {
    u.spotify_client_id = u.spotify_client_id.trim().to_string();
    u.spotify_id_focused = false;
}

fn on_spotify_client_id_char(code: u32) -> bool {
    let Some(ch) = char::from_u32(code) else {
        return false;
    };
    let changed = with_ui(|u| {
        if !u.spotify_id_focused {
            return false;
        }
        match ch {
            '\u{8}' => {
                u.spotify_client_id.pop();
            }
            '\r' | '\n' => commit_spotify_client_id(u),
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') => {
                if u.spotify_client_id.len() < 128 {
                    u.spotify_client_id.push(c);
                }
            }
            _ => return false,
        }
        true
    })
    .unwrap_or(false);
    if changed {
        repaint();
    }
    changed
}

fn begin_number_edit(u: &mut Ui, target: Number) {
    let Some(w) = sel_widget(u) else {
        return;
    };
    u.number_text = match target {
        Number::Slider(slide) => slide_input(slide, slide_get(w, slide)),
        Number::Dimension(Dim::Width) => w.width.to_string(),
        Number::Dimension(Dim::Height) => w.height.to_string(),
    };
    u.number_edit = Some(target);
    u.number_replace = true;
}

fn commit_number_edit(u: &mut Ui) {
    let Some(target) = u.number_edit.take() else {
        return;
    };
    let text = u.number_text.trim().to_string();
    if let Some(w) = sel_mut(u) {
        match target {
            Number::Slider(slide) => {
                if let Ok(mut value) = text.parse::<f32>() {
                    if matches!(
                        slide,
                        Slide::Opacity | Slide::ShadowStrength | Slide::TextScale
                    ) {
                        value /= 100.0;
                    }
                    let (lo, hi) = slide_range(slide);
                    slide_set(w, slide, value.clamp(lo, hi));
                }
            }
            Number::Dimension(dim) => {
                if let Ok(value) = text.parse::<u32>() {
                    let value = value.clamp(40, 2000);
                    match dim {
                        Dim::Width => w.width = value,
                        Dim::Height => w.height = value,
                    }
                }
            }
        }
    }
    u.number_text.clear();
    u.number_replace = false;
}

fn on_numeric_char(code: u32) -> bool {
    let Some(ch) = char::from_u32(code) else {
        return false;
    };
    let handled = with_ui(|u| {
        let Some(target) = u.number_edit else {
            return false;
        };
        match ch {
            '\r' | '\n' => {
                commit_number_edit(u);
            }
            '\u{8}' => {
                if u.number_replace {
                    u.number_text.clear();
                    u.number_replace = false;
                } else {
                    u.number_text.pop();
                }
            }
            c if c.is_ascii_digit() || (c == '.' && matches!(target, Number::Slider(_))) => {
                if u.number_replace {
                    u.number_text.clear();
                    u.number_replace = false;
                }
                if u.number_text.len() < 12 && (c != '.' || !u.number_text.contains('.')) {
                    u.number_text.push(c);
                }
            }
            _ => return false,
        }
        true
    })
    .unwrap_or(false);
    if handled {
        repaint();
    }
    handled
}

fn cancel_number_edit() -> bool {
    let cancelled = with_ui(|u| {
        if u.number_edit.take().is_none() {
            return false;
        }
        u.number_text.clear();
        u.number_replace = false;
        true
    })
    .unwrap_or(false);
    if cancelled {
        repaint();
    }
    cancelled
}

fn on_input_char(code: u32) {
    if !on_numeric_char(code) {
        if !on_spotify_client_id_char(code) {
            on_note_char(code);
        }
    }
}

fn on_note_char(code: u32) -> bool {
    let Some(ch) = char::from_u32(code) else {
        return false;
    };
    let changed = with_ui(|u| {
        if !u.note_focused {
            return false;
        }
        let Some(w) = sel_mut(u) else {
            return false;
        };
        if w.kind != "note" {
            return false;
        }
        match ch {
            '\u{8}' => {
                w.text.pop();
            }
            '\r' | '\n' => w.text.push('\n'),
            c if !c.is_control() && w.text.len() < 8192 => w.text.push(c),
            _ => return false,
        }
        true
    })
    .unwrap_or(false);
    if changed {
        repaint();
    }
    changed
}

fn on_mouse_move(lx: f32, ly: f32, button_down: bool) {
    let mut changed = false;
    with_ui(|u| {
        let h = hit_test(&u.hits, lx, ly);
        if h != u.hover {
            u.hover = h;
            changed = true;
        }
        if let Some(s) = u.drag {
            if button_down {
                set_slider_from_x(u, s, lx);
                changed = true;
            } else {
                u.drag = None;
            }
        }
    });
    if changed {
        repaint();
    }
}

/// Opens the Windows color picker for the selected widget's color.
fn pick_color(sw: Swat) {
    let Some(owner) = with_ui(|u| u.hwnd) else {
        return;
    };
    let current = with_ui(|u| sel_widget(u).map(|w| parse_hex(swat_get(w, sw)))).flatten();
    let Some([r, g, b]) = current else {
        return;
    };

    let mut custom = [0u32; 16];
    let mut cc: CHOOSECOLORW = unsafe { zeroed() };
    cc.lStructSize = size_of::<CHOOSECOLORW>() as u32;
    cc.hwndOwner = owner;
    cc.rgbResult = r as u32 | ((g as u32) << 8) | ((b as u32) << 16);
    cc.lpCustColors = custom.as_mut_ptr();
    cc.Flags = CC_RGBINIT | CC_FULLOPEN;
    if unsafe { ChooseColorW(&mut cc) } == 0 {
        return;
    }

    let c = cc.rgbResult;
    let hex = format!(
        "#{:02X}{:02X}{:02X}",
        c & 0xFF,
        (c >> 8) & 0xFF,
        (c >> 16) & 0xFF
    );
    with_ui(|u| {
        if let Some(w) = sel_mut(u) {
            swat_set(w, sw, hex);
        }
    });
    repaint();
}

/// Opens the file picker and sets the photo of the selected photo widget.
fn browse() {
    let Some(owner) = with_ui(|u| u.hwnd) else {
        return;
    };
    let filter: Vec<u16> = "Pictures (*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp)\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp\0All files\0*.*\0\0"
        .encode_utf16()
        .collect();
    let mut file = vec![0u16; 1024];
    let mut ofn: OPENFILENAMEW = unsafe { zeroed() };
    ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return;
    }

    let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    let path = String::from_utf16_lossy(&file[..end]);
    with_ui(|u| {
        if let Some(w) = sel_mut(u) {
            if w.kind == "photo" || w.kind == "spotify" {
                w.path = path;
            }
        }
    });
    repaint();
}

/// Writes the settings file. The widgets update within a second.
fn save() {
    let (widgets, source_indices, spotify_client_id) = with_ui(|u| {
        (
            u.widgets.clone(),
            u.source_indices.clone(),
            u.spotify_client_id.clone(),
        )
    })
    .unwrap_or_default();
    let mut config = Config {
        widgets,
        spotify_client_id,
    };
    crate::app::preserve_live_positions(&mut config, &source_indices);
    match config.save() {
        Ok(()) => {
            crate::app::apply_saved_config(config);
            with_ui(|u| {
                u.source_indices = (0..u.widgets.len()).map(Some).collect();
            });
        }
        Err(e) => show_message(&format!("Could not save settings:\n{}", e)),
    }
}

fn close() {
    if let Some(h) = with_ui(|u| u.hwnd) {
        unsafe {
            DestroyWindow(h);
        }
    }
}

/// Mouse position in logical card coordinates.
fn to_logical(x: i32, y: i32) -> (f32, f32) {
    let s = with_ui(|u| u.s).unwrap_or(1.0);
    (x as f32 / s - MARGIN, y as f32 / s - MARGIN)
}

/// Reads the x and y that Windows packs into a message parameter.
fn xy(lparam: LPARAM) -> (i32, i32) {
    let l = lparam as usize;
    (
        (l & 0xFFFF) as i16 as i32,
        ((l >> 16) & 0xFFFF) as i16 as i32,
    )
}

/// Creates the memory bitmap we draw into. Returns (dc, bitmap, old bitmap, pixels).
unsafe fn make_bitmap(width: i32, height: i32) -> Option<(HDC, HBITMAP, HGDIOBJ, *mut u8)> {
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let screen_dc = GetDC(null_mut());
    let mem_dc = CreateCompatibleDC(screen_dc);
    let mut bits: *mut std::ffi::c_void = null_mut();
    let bitmap = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    ReleaseDC(null_mut(), screen_dc);
    if bitmap.is_null() || bits.is_null() {
        if !bitmap.is_null() {
            DeleteObject(bitmap as HGDIOBJ);
        }
        DeleteDC(mem_dc);
        return None;
    }
    let old = SelectObject(mem_dc, bitmap as HGDIOBJ);
    Some((mem_dc, bitmap, old, bits as *mut u8))
}

/// Frees the window's bitmap and device context.
unsafe fn cleanup(u: Ui) {
    SelectObject(u.mem_dc, u.old_bitmap);
    DeleteObject(u.bitmap as HGDIOBJ);
    DeleteDC(u.mem_dc);
}

/// Opens the settings window, or brings it to the front if it is already open.
pub fn open() {
    if let Some(h) = with_ui(|u| u.hwnd) {
        unsafe {
            ShowWindow(h, SW_SHOW);
            SetForegroundWindow(h);
        }
        return;
    }

    let cfg = match Config::load() {
        Ok(c) => c,
        Err(msg) => {
            show_message(&msg);
            return;
        }
    };
    let selected = if cfg.widgets.is_empty() {
        None
    } else {
        Some(0)
    };
    let spotify_client_id = cfg.spotify_client_id.clone();
    let source_indices = (0..cfg.widgets.len()).map(Some).collect();
    let s = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let font = draw::load_font();

    unsafe {
        let hinst = GetModuleHandleW(null());
        let class = wide(CLASS_NAME);
        let mut wc: WNDCLASSEXW = zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(settings_proc);
        wc.hInstance = hinst;
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.lpszClassName = class.as_ptr();
        RegisterClassExW(&wc); // harmless if already registered

        let width = ((CARD_W + 2.0 * MARGIN) * s).round() as i32;
        let height = ((CARD_H + 2.0 * MARGIN) * s).round() as i32;
        let x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - height) / 2;
        let title = wide(TITLE);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_APPWINDOW, // transparent, with a taskbar button
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            width,
            height,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            return;
        }

        let Some((mem_dc, bitmap, old_bitmap, bits)) = make_bitmap(width, height) else {
            DestroyWindow(hwnd);
            return;
        };
        let Some(canvas) = Canvas::new(width as u32, height as u32) else {
            DestroyWindow(hwnd);
            return;
        };

        let ui = Ui {
            hwnd,
            canvas,
            mem_dc,
            bitmap,
            old_bitmap,
            bits,
            s,
            font,
            widgets: cfg.widgets,
            source_indices,
            spotify_client_id,
            selected,
            tab: 0,
            hover: None,
            note_focused: false,
            spotify_id_focused: false,
            number_edit: None,
            number_text: String::new(),
            number_replace: false,
            drag: None,
            hits: Vec::new(),
        };
        UI.with(|c| *c.borrow_mut() = Some(ui));
        repaint();
        SetTimer(hwnd, 4, 1000, None);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

unsafe extern "system" fn settings_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // The header acts as a title bar, so the window can be dragged by it.
        WM_NCHITTEST => {
            let (sx, sy) = xy(lparam);
            let mut rect: RECT = zeroed();
            GetWindowRect(hwnd, &mut rect);
            let (lx, ly) = to_logical(sx - rect.left, sy - rect.top);
            if ly > 0.0 && ly < HEADER_H && lx > 0.0 && lx < CARD_W - 60.0 {
                HTCAPTION as LRESULT
            } else {
                HTCLIENT as LRESULT
            }
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lparam);
            let (lx, ly) = to_logical(x, y);
            on_mouse_down(lx, ly);
            0
        }
        WM_LBUTTONUP => {
            with_ui(|u| u.drag = None);
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = xy(lparam);
            let (lx, ly) = to_logical(x, y);
            on_mouse_move(lx, ly, (wparam & 1) != 0);
            0
        }
        WM_TIMER if wparam == 4 => {
            repaint();
            0
        }
        WM_CHAR => {
            on_input_char(wparam as u32);
            0
        }
        WM_KEYDOWN if wparam as u32 == 0x1B => {
            if cancel_number_edit() {
                0
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_DESTROY => {
            if let Some(u) = UI.with(|c| c.borrow_mut().take()) {
                cleanup(u);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
