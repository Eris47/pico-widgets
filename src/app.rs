//! The Windows side of the widgets. Creates one see-through window per widget,
//! keeps them up to date, handles dragging, the right-click menu, and reloads
//! the settings file when it changes.

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::SystemTime;

use fontdue::Font;
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, SetProcessDpiAwareness};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{Config, WidgetConfig};
use crate::draw::{self, Canvas, ClockData};

const CLASS_NAME: &str = "PicoWidgetsWindow";
const CLOCK_TIMER: usize = 1;
const WATCH_TIMER: usize = 2;
const SPOTIFY_TIMER: usize = 3;
const WATCH_MS: u32 = 1000;
const ID_OPEN_SETTINGS: usize = 1001;
const ID_QUIT: usize = 1002;
const ID_OPEN_FILE: usize = 1003;
const ID_SPOTIFY_PREVIOUS: usize = 1004;
const ID_SPOTIFY_TOGGLE: usize = 1005;
const ID_SPOTIFY_NEXT: usize = 1006;

/// One widget on screen.
struct Win {
    hwnd: HWND,
    /// Which entry in config.widgets this window belongs to.
    index: usize,
    cfg: WidgetConfig,
    display_font: Option<Font>,
    scale: f32,
    width: i32,
    height: i32,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    /// Pixel memory of `bitmap`, owned by Windows. Valid until destroy_win.
    bits: *mut u8,
    canvas: Canvas,
    photo: Option<Pixmap>,
    last_minute: u16,
}

/// What Windows needs to show a widget. Copyable, so it can leave the state borrow.
#[derive(Clone, Copy)]
struct Frame {
    hwnd: HWND,
    mem_dc: HDC,
    width: i32,
    height: i32,
}

impl Win {
    fn is_clock(&self) -> bool {
        self.cfg.kind == "clock"
    }

    fn is_spotify(&self) -> bool {
        self.cfg.kind == "spotify"
    }

    fn frame(&self) -> Frame {
        Frame {
            hwnd: self.hwnd,
            mem_dc: self.mem_dc,
            width: self.width,
            height: self.height,
        }
    }

    /// Draws the widget into its bitmap. Makes no window calls.
    fn render(&mut self, font: Option<&Font>, now: &ClockData) {
        let font = self.display_font.as_ref().or(font);
        self.canvas
            .draw(font, &self.cfg, self.scale, self.photo.as_ref(), now);
        let len = (self.width * self.height * 4) as usize;
        // SAFETY: `bits` is a bitmap of exactly width*height*4 bytes that lives
        // until destroy_win is called.
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, len) };
        self.canvas.copy_to_bgra(dst);
    }
}

/// Everything the app remembers between messages.
struct State {
    wins: Vec<Win>,
    font: Option<Font>,
    scale: f32,
    config: Config,
    /// File timestamp we last loaded or saved. A different one means you edited it.
    mtime: Option<SystemTime>,
    /// A hidden window that only checks the settings file once a second.
    controller: HWND,
}

impl State {
    fn new() -> State {
        State {
            wins: Vec::new(),
            font: None,
            scale: 1.0,
            config: Config::default(),
            mtime: None,
            controller: null_mut(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::new());
}

/// Runs `f` with exclusive access to the app state.
/// Rule: never call a window function that sends messages inside `f`.
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|cell| f(&mut cell.borrow_mut()))
}

/// Windows wants text as UTF-16 with a 0 at the end.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn xy(lparam: LPARAM) -> (i32, i32) {
    let value = lparam as usize;
    (
        (value & 0xFFFF) as i16 as i32,
        ((value >> 16) & 0xFFFF) as i16 as i32,
    )
}

pub fn run() {
    unsafe {
        // System-aware DPI: the app looks sharp on high-DPI screens.
        SetProcessDpiAwareness(1);

        let hinst = GetModuleHandleW(null());
        let class = wide(CLASS_NAME);

        let mut wc: WNDCLASSEXW = zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinst;
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.lpszClassName = class.as_ptr();
        if RegisterClassExW(&wc) == 0 {
            return;
        }

        let scale = GetDpiForSystem() as f32 / 96.0;
        let controller = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("pico-controller").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE, // invisible, takes no space
            null_mut(),
            hinst,
            null(),
        );
        with_state(|s| {
            s.scale = scale;
            s.font = draw::load_font();
            s.controller = controller;
        });
        SetTimer(controller, WATCH_TIMER, WATCH_MS, None);

        reload();

        // The message loop. The program sleeps in GetMessageW between events.
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Reads the settings file and rebuilds all widgets from it.
unsafe fn reload() {
    let result = Config::load();
    // Remember the timestamp first, so our own saves don't trigger another reload.
    let mtime = Config::modified();
    with_state(|s| s.mtime = mtime);
    match result {
        Ok(cfg) => build(cfg),
        // Keep the current widgets and show the mistake.
        Err(msg) => show_error(&msg),
    }
}

pub(crate) fn apply_saved_config(cfg: Config) {
    let mtime = Config::modified();
    with_state(|s| s.mtime = mtime);
    unsafe { build(cfg) };
}

/// Replaces every widget on screen with the ones in `cfg`.
unsafe fn build(cfg: Config) {
    // With no widgets there would be nothing to right-click, so fall back to the default clock.
    let cfg = if cfg.widgets.is_empty() {
        Config::default_config()
    } else {
        cfg
    };
    let widgets = cfg.widgets.clone();
    let spotify_client_id = cfg.spotify_client_id.clone();

    let old = with_state(|s| {
        s.config = cfg;
        std::mem::take(&mut s.wins)
    });
    for win in old {
        destroy_win(win);
    }

    let scale = with_state(|s| s.scale);
    let class = wide(CLASS_NAME);
    for (index, wc) in widgets.iter().enumerate() {
        if let Some(win) = create_win(index, wc, scale, &class) {
            with_state(|s| s.wins.push(win));
        }
    }
    crate::spotify::configure(&spotify_client_id);
}

unsafe fn create_win(index: usize, wc: &WidgetConfig, scale: f32, class: &[u16]) -> Option<Win> {
    // The card is the size you chose. The window is larger, so the shadow fits around it.
    let m = draw::margin(wc, scale);
    let card_w = ((wc.width as f32) * scale).round().max(40.0);
    let card_h = ((wc.height as f32) * scale).round().max(40.0);
    let width = (card_w + 2.0 * m).round() as i32;
    let height = (card_h + 2.0 * m).round() as i32;

    let screen_width = GetSystemMetrics(SM_CXSCREEN).max(width);
    let screen_height = GetSystemMetrics(SM_CYSCREEN).max(height);
    let (x, y) = match (wc.x, wc.y) {
        (Some(x), Some(y)) => (x, y),
        _ => default_widget_position(index, width, height, screen_width, screen_height),
    };
    let x = x.clamp(0, screen_width - width);
    let y = y.clamp(0, screen_height - height);

    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW, // per-pixel transparency, no taskbar button
        class.as_ptr(),
        wide("pico-widgets").as_ptr(),
        WS_POPUP,
        x,
        y,
        width,
        height,
        null_mut(),
        null_mut(),
        GetModuleHandleW(null()),
        null(),
    );
    if hwnd.is_null() {
        return None;
    }

    // A bitmap we can write pixels into directly.
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height; // negative = top row first
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let screen_dc = GetDC(null_mut());
    let mem_dc = CreateCompatibleDC(screen_dc);
    let mut bits: *mut c_void = null_mut();
    let bitmap = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    ReleaseDC(null_mut(), screen_dc);

    let canvas = match Canvas::new(width as u32, height as u32) {
        Some(c) if !bitmap.is_null() && !bits.is_null() => c,
        _ => {
            if !bitmap.is_null() {
                DeleteObject(bitmap as HGDIOBJ);
            }
            DeleteDC(mem_dc);
            DestroyWindow(hwnd);
            return None;
        }
    };
    let old_bitmap = SelectObject(mem_dc, bitmap as HGDIOBJ);

    let photo = if wc.kind == "photo" || wc.kind == "spotify" {
        let border = (wc.border_width.max(0.0) * scale).min(card_w.min(card_h) / 2.0);
        let photo_w = (card_w - border * 2.0).max(1.0).round() as u32;
        let photo_h = (card_h - border * 2.0).max(1.0).round() as u32;
        draw::load_photo(&wc.path, photo_w, photo_h)
    } else {
        None
    };

    let mut win = Win {
        hwnd,
        index,
        cfg: wc.clone(),
        display_font: if wc.kind == "spotify" || wc.kind == "clock" {
            draw::load_spotify_font()
        } else {
            None
        },
        scale,
        width,
        height,
        mem_dc,
        bitmap,
        old_bitmap,
        bits: bits as *mut u8,
        canvas,
        photo,
        last_minute: u16::MAX,
    };

    let now = local_time();
    win.last_minute = now.minute;
    with_state(|s| win.render(s.font.as_ref(), &now));
    present(win.frame());

    if win.is_clock() {
        schedule_clock(hwnd);
    } else if win.is_spotify() {
        SetTimer(hwnd, SPOTIFY_TIMER, 500, None);
    }
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    send_to_back(hwnd);
    Some(win)
}

fn default_widget_position(
    index: usize,
    width: i32,
    height: i32,
    screen_width: i32,
    screen_height: i32,
) -> (i32, i32) {
    const EDGE: i32 = 40;
    const GAP: i32 = 20;

    let columns = ((screen_width - EDGE * 2 + GAP) / (width + GAP)).max(1) as usize;
    let rows = ((screen_height - EDGE * 2 + GAP) / (height + GAP)).max(1) as usize;
    let slot = index % (columns * rows);
    let column = slot % columns;
    let row = slot / columns;
    let max_x = (screen_width - width).max(0);
    let max_y = (screen_height - height).max(0);
    let x = (screen_width - EDGE - width - column as i32 * (width + GAP)).clamp(0, max_x);
    let y = (EDGE + row as i32 * (height + GAP)).clamp(0, max_y);
    (x, y)
}

unsafe fn destroy_win(win: Win) {
    KillTimer(win.hwnd, CLOCK_TIMER);
    KillTimer(win.hwnd, SPOTIFY_TIMER);
    DestroyWindow(win.hwnd);
    SelectObject(win.mem_dc, win.old_bitmap);
    DeleteObject(win.bitmap as HGDIOBJ);
    DeleteDC(win.mem_dc);
}

/// Pushes a widget's bitmap to the screen, with its per-pixel transparency.
unsafe fn present(f: Frame) {
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

unsafe fn local_time() -> ClockData {
    let mut st: SYSTEMTIME = zeroed();
    GetLocalTime(&mut st);
    ClockData {
        hour: st.wHour,
        minute: st.wMinute,
        weekday: st.wDayOfWeek,
        day: st.wDay,
        month: st.wMonth,
    }
}

/// Redraws one widget. Clocks skip the work unless the minute changed (or `force`).
unsafe fn redraw(hwnd: HWND, force: bool) {
    let now = local_time();
    let frame = with_state(|s| {
        let font = s.font.as_ref();
        let win = s.wins.iter_mut().find(|w| w.hwnd == hwnd)?;
        if !force && (!win.is_clock() || win.last_minute == now.minute) {
            return None;
        }
        win.last_minute = now.minute;
        win.render(font, &now);
        Some(win.frame())
    });
    if let Some(f) = frame {
        present(f);
    }
}

/// Wakes the clock up right after the next minute starts.
unsafe fn schedule_clock(hwnd: HWND) {
    let mut st: SYSTEMTIME = zeroed();
    GetLocalTime(&mut st);
    let ms = (60 - st.wSecond as u32) * 1000 - st.wMilliseconds as u32 + 50;
    SetTimer(hwnd, CLOCK_TIMER, ms, None);
}

fn is_clock_win(hwnd: HWND) -> bool {
    with_state(|s| {
        s.wins
            .iter()
            .find(|w| w.hwnd == hwnd)
            .map(|w| w.is_clock())
            .unwrap_or(false)
    })
}

fn is_locked_win(hwnd: HWND) -> bool {
    with_state(|s| {
        s.wins
            .iter()
            .find(|w| w.hwnd == hwnd)
            .map(|w| w.cfg.locked)
            .unwrap_or(false)
    })
}

fn is_spotify_win(hwnd: HWND) -> bool {
    with_state(|s| {
        s.wins
            .iter()
            .find(|w| w.hwnd == hwnd)
            .map(|w| w.is_spotify())
            .unwrap_or(false)
    })
}

fn spotify_control_for_local(hwnd: HWND, x: i32, y: i32) -> Option<u8> {
    with_state(|s| {
        let win = s.wins.iter().find(|win| win.hwnd == hwnd)?;
        if !win.is_spotify() {
            return None;
        }
        let scale = win.scale;
        let margin = draw::margin(&win.cfg, scale);
        let border = (win.cfg.border_width.max(0.0) * scale)
            .min(win.cfg.width.min(win.cfg.height) as f32 * scale / 2.0);
        let left = margin + border;
        let top = margin + border;
        let width = (win.cfg.width as f32 * scale - border * 2.0).max(1.0);
        let height = (win.cfg.height as f32 * scale - border * 2.0).max(1.0);
        spotify_control_at(x as f32, y as f32, left, top, width, height, scale)
    })
}

fn spotify_control_at(
    x: f32,
    y: f32,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    scale: f32,
) -> Option<u8> {
    let icon_size = (height * 0.15).clamp(20.0, 26.0);
    let center_y = top + height - icon_size - 8.0;
    let hit_height = (icon_size * 2.0 + 12.0).max(32.0 * scale);
    if x < left
        || x >= left + width
        || y < center_y - hit_height / 2.0
        || y > center_y + hit_height / 2.0
    {
        return None;
    }
    Some((((x - left) / width * 3.0).floor() as u8).min(2))
}

fn widget_hit_test(hwnd: HWND, screen_x: i32, screen_y: i32) -> LRESULT {
    if is_locked_win(hwnd) {
        return HTCLIENT as LRESULT;
    }
    if is_spotify_win(hwnd) {
        let mut rect: RECT = unsafe { zeroed() };
        if unsafe { GetWindowRect(hwnd, &mut rect) } != 0
            && spotify_control_for_local(hwnd, screen_x - rect.left, screen_y - rect.top).is_some()
        {
            return HTCLIENT as LRESULT;
        }
    }
    HTCAPTION as LRESULT
}

pub(crate) fn preserve_live_positions(cfg: &mut Config, source_indices: &[Option<usize>]) {
    let windows = with_state(|s| {
        source_indices
            .iter()
            .enumerate()
            .filter_map(|(new_index, source_index)| {
                let source_index = (*source_index)?;
                let win = s.wins.iter().find(|win| win.index == source_index)?;
                Some((new_index, win.hwnd))
            })
            .collect::<Vec<_>>()
    });

    for (index, hwnd) in windows {
        let mut rect: RECT = unsafe { zeroed() };
        if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
            if let Some(widget) = cfg.widgets.get_mut(index) {
                widget.x = Some(rect.left);
                widget.y = Some(rect.top);
            }
        }
    }
}

/// Reloads if the settings file changed on disk.
unsafe fn watch_settings_file() {
    let changed = with_state(|s| Config::modified() != s.mtime);
    if changed {
        reload();
    }
}

/// Keeps widgets behind other windows, like a desktop widget should be.
unsafe fn send_to_back(hwnd: HWND) {
    SetWindowPos(
        hwnd,
        HWND_BOTTOM,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

/// After a drag: store the new position in the settings file.
unsafe fn remember_position(hwnd: HWND) {
    let mut rect: RECT = zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return;
    }
    with_state(|s| {
        let Some(index) = s.wins.iter().find(|w| w.hwnd == hwnd).map(|w| w.index) else {
            return;
        };
        if let Some(w) = s.config.widgets.get_mut(index) {
            w.x = Some(rect.left);
            w.y = Some(rect.top);
        }
        let _ = s.config.save();
        s.mtime = Config::modified();
    });
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    AppendMenuW(
        menu,
        MF_STRING,
        ID_OPEN_SETTINGS,
        wide("Settings...").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING,
        ID_OPEN_FILE,
        wide("Open settings file").as_ptr(),
    );
    if is_spotify_win(hwnd) {
        AppendMenuW(
            menu,
            MF_STRING,
            ID_SPOTIFY_PREVIOUS,
            wide("Previous track").as_ptr(),
        );
        AppendMenuW(
            menu,
            MF_STRING,
            ID_SPOTIFY_TOGGLE,
            wide(if crate::spotify::snapshot().is_playing {
                "Pause"
            } else {
                "Play"
            })
            .as_ptr(),
        );
        AppendMenuW(
            menu,
            MF_STRING,
            ID_SPOTIFY_NEXT,
            wide("Next track").as_ptr(),
        );
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, ID_QUIT, wide("Quit").as_ptr());

    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd); // so the menu closes when you click elsewhere
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);

    match cmd as usize {
        ID_OPEN_SETTINGS => crate::settings::open(),
        ID_OPEN_FILE => {
            let _ = std::process::Command::new("notepad.exe")
                .arg(Config::path())
                .spawn();
        }
        ID_SPOTIFY_PREVIOUS => crate::spotify::previous_track(),
        ID_SPOTIFY_TOGGLE => crate::spotify::toggle_playback(),
        ID_SPOTIFY_NEXT => crate::spotify::next_track(),
        ID_QUIT => PostQuitMessage(0),
        _ => {}
    }
}

unsafe fn show_error(msg: &str) {
    let text = wide(msg);
    let title = wide("pico-widgets");
    MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), MB_ICONWARNING);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // The hidden controller only watches the settings file.
    if with_state(|s| s.controller == hwnd) {
        if msg == WM_TIMER && wparam == WATCH_TIMER {
            watch_settings_file();
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    match msg {
        // Unlocked widgets act like title bars, so dragging anywhere works.
        WM_NCHITTEST => {
            let (x, y) = xy(lparam);
            widget_hit_test(hwnd, x, y)
        }

        WM_TIMER => {
            if wparam == CLOCK_TIMER {
                redraw(hwnd, false);
                schedule_clock(hwnd);
            } else if wparam == SPOTIFY_TIMER {
                redraw(hwnd, true);
            }
            0
        }

        WM_LBUTTONDOWN => {
            let (x, y) = xy(lparam);
            match spotify_control_for_local(hwnd, x, y) {
                Some(0) => crate::spotify::previous_track(),
                Some(1) => crate::spotify::toggle_playback(),
                Some(2) => crate::spotify::next_track(),
                _ => {}
            }
            0
        }

        // The system clock was changed: redraw right away.
        WM_TIMECHANGE => {
            redraw(hwnd, true);
            if is_clock_win(hwnd) {
                schedule_clock(hwnd);
            }
            0
        }

        // Right-click on the widget.
        WM_NCRBUTTONUP | WM_RBUTTONUP => {
            show_menu(hwnd);
            0
        }

        // Drag finished: save the position and go back behind other windows.
        WM_EXITSIZEMOVE => {
            remember_position(hwnd);
            send_to_back(hwnd);
            0
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::{default_widget_position, spotify_control_at};

    #[test]
    fn default_positions_stay_on_screen_for_later_widgets() {
        let screen = (1280, 720);
        let widget = (420, 300);

        for index in 0..100 {
            let (x, y) = default_widget_position(index, widget.0, widget.1, screen.0, screen.1);
            assert!(x >= 0 && x + widget.0 <= screen.0);
            assert!(y >= 0 && y + widget.1 <= screen.1);
        }
    }

    #[test]
    fn spotify_icon_centers_map_to_previous_toggle_and_next() {
        let left = 12.0;
        let top = 12.0;
        let width = 320.0;
        let height: f32 = 160.0;
        let icon_size = (height * 0.15).clamp(20.0, 26.0);
        let y = top + height - icon_size - 8.0;

        for expected in 0..3 {
            let x = left + width * (expected as f32 + 0.5) / 3.0;
            assert_eq!(
                spotify_control_at(x, y, left, top, width, height, 1.0),
                Some(expected as u8)
            );
        }
    }
}
