use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use keyring::Entry;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const REDIRECT_URI: &str = "http://127.0.0.1:43821/callback";
const REDIRECT_ADDR: &str = "127.0.0.1:43821";
const TOKEN_SERVICE: &str = "pico-widgets-spotify";
const TOKEN_USER: &str = "oauth-token";
const SCOPES: &str =
    "user-read-currently-playing user-read-playback-state user-modify-playback-state";
const REQUIRED_SCOPES: [&str; 3] = [
    "user-read-currently-playing",
    "user-read-playback-state",
    "user-modify-playback-state",
];
const POLL_INTERVAL: Duration = Duration::from_secs(12);

static SNAPSHOT: OnceLock<Mutex<SpotifySnapshot>> = OnceLock::new();
static TOKENS: OnceLock<Mutex<Option<Tokens>>> = OnceLock::new();
static CLIENT_ID: OnceLock<Mutex<String>> = OnceLock::new();
static CONNECTING: AtomicBool = AtomicBool::new(false);
static MONITOR_RUNNING: AtomicBool = AtomicBool::new(false);
static COMMAND_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Default)]
pub struct SpotifySnapshot {
    pub connected: bool,
    pub connecting: bool,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub is_playing: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Tokens {
    client_id: String,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    #[serde(default)]
    scopes: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

fn snapshot_cell() -> &'static Mutex<SpotifySnapshot> {
    SNAPSHOT.get_or_init(|| Mutex::new(SpotifySnapshot::default()))
}

fn tokens_cell() -> &'static Mutex<Option<Tokens>> {
    TOKENS.get_or_init(|| Mutex::new(None))
}

fn client_id_cell() -> &'static Mutex<String> {
    CLIENT_ID.get_or_init(|| Mutex::new(String::new()))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(12))
        .timeout_write(Duration::from_secs(12))
        .build()
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn set_status(update: impl FnOnce(&mut SpotifySnapshot)) {
    if let Ok(mut state) = snapshot_cell().lock() {
        update(&mut state);
    }
}

pub fn snapshot() -> SpotifySnapshot {
    snapshot_cell()
        .lock()
        .map(|state| state.clone())
        .unwrap_or_default()
}

pub fn configure(client_id: &str) {
    if let Ok(mut current) = client_id_cell().lock() {
        *current = client_id.trim().to_string();
    }
    if client_id.trim().is_empty() {
        set_status(|state| {
            state.connected = false;
            state.connecting = false;
            state.status = "Add a Spotify Client ID in Settings.".into();
        });
        return;
    }

    let stored = Entry::new(TOKEN_SERVICE, TOKEN_USER)
        .ok()
        .and_then(|entry| entry.get_password().ok())
        .and_then(|value| serde_json::from_str::<Tokens>(&value).ok())
        .filter(|tokens| tokens.client_id == client_id.trim());

    match stored {
        Some(tokens) if has_required_scopes(&tokens.scopes) => {
            if let Ok(mut current) = tokens_cell().lock() {
                *current = Some(tokens);
            }
            set_status(|state| {
                state.connected = true;
                state.status = "Connected to Spotify".into();
            });
            start_monitor();
        }
        Some(_) => {
            if let Ok(mut current) = tokens_cell().lock() {
                *current = None;
            }
            set_status(|state| {
                state.connected = false;
                state.status = "Reconnect Spotify to approve playback permissions.".into();
            });
        }
        None => {
            if let Ok(mut current) = tokens_cell().lock() {
                *current = None;
            }
            set_status(|state| {
                state.connected = false;
                state.status = "Ready to connect".into();
            });
        }
    }
}

fn has_required_scopes(scopes: &str) -> bool {
    REQUIRED_SCOPES
        .iter()
        .all(|required| scopes.split_whitespace().any(|scope| scope == *required))
}

pub fn connect(client_id: String) {
    let client_id = client_id.trim().to_string();
    if client_id.is_empty() {
        set_status(|state| state.status = "Enter a Spotify Client ID first.".into());
        return;
    }
    if CONNECTING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Ok(mut current) = client_id_cell().lock() {
        *current = client_id.clone();
    }
    set_status(|state| {
        state.connecting = true;
        state.status = "Waiting for Spotify authorization...".into();
    });
    thread::spawn(move || {
        let result = authorize(&client_id);
        CONNECTING.store(false, Ordering::SeqCst);
        match result {
            Ok(tokens) => {
                let saved = save_tokens(&tokens);
                if let Ok(mut current) = tokens_cell().lock() {
                    *current = Some(tokens);
                }
                set_status(|state| {
                    state.connected = true;
                    state.connecting = false;
                    state.status = if saved.is_ok() {
                        "Connected to Spotify".into()
                    } else {
                        "Connected for this session; token storage failed".into()
                    };
                });
                start_monitor();
            }
            Err(error) => set_status(|state| {
                state.connected = false;
                state.connecting = false;
                state.status = error;
            }),
        }
    });
}

pub fn toggle_playback() {
    let playing = snapshot().is_playing;
    command(if playing {
        PlaybackAction::Pause
    } else {
        PlaybackAction::Play
    });
}

pub fn next_track() {
    command(PlaybackAction::Next);
}

pub fn previous_track() {
    command(PlaybackAction::Previous);
}

#[derive(Clone, Copy)]
enum PlaybackAction {
    Play,
    Pause,
    Next,
    Previous,
}

fn command(action: PlaybackAction) {
    if COMMAND_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let previous_playing = snapshot().is_playing;
    let requested_playing = match action {
        PlaybackAction::Play => Some(true),
        PlaybackAction::Pause => Some(false),
        PlaybackAction::Next | PlaybackAction::Previous => None,
    };
    if let Some(is_playing) = requested_playing {
        set_status(|state| {
            state.is_playing = is_playing;
            state.status = if is_playing { "Playing" } else { "Paused" }.into();
        });
    }
    thread::spawn(move || {
        let result = send_command(action);
        match result {
            Ok(()) => {
                if let Err(error) = poll_current_track() {
                    set_status(|state| state.status = error);
                }
                if let Some(is_playing) = requested_playing {
                    set_status(|state| {
                        if state.is_playing != is_playing {
                            state.is_playing = is_playing;
                            state.status = if is_playing {
                                "Play command sent"
                            } else {
                                "Pause command sent"
                            }
                            .into();
                        }
                    });
                }
            }
            Err(error) => set_status(|state| {
                state.is_playing = previous_playing;
                state.status = error;
            }),
        }
        COMMAND_RUNNING.store(false, Ordering::SeqCst);
    });
}

fn start_monitor() {
    if MONITOR_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(|| loop {
        if let Err(error) = poll_current_track() {
            set_status(|state| {
                state.connected = false;
                state.status = error;
            });
        }
        thread::sleep(POLL_INTERVAL);
    });
}

fn authorize(client_id: &str) -> Result<Tokens, String> {
    let listener = TcpListener::bind(REDIRECT_ADDR)
        .map_err(|error| format!("Could not open Spotify callback port: {}", error))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("Could not prepare Spotify callback: {}", error))?;

    let verifier = random_string(64);
    let state = random_string(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let url = format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&show_dialog=true&code_challenge_method=S256&code_challenge={}",
        percent_encode(client_id),
        percent_encode(REDIRECT_URI),
        percent_encode(SCOPES),
        percent_encode(&state),
        percent_encode(&challenge),
    );
    open_browser(&url)?;

    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let (code, returned_state) = loop {
        if std::time::Instant::now() >= deadline {
            return Err("Spotify authorization timed out.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let request = read_callback(&mut stream)?;
                let (code, returned_state, error) = parse_callback(&request);
                let (status, body) = if error.is_some() || code.is_none() {
                    (
                        "400 Bad Request",
                        "Spotify authorization was not completed.",
                    )
                } else {
                    ("200 OK", "Spotify is connected. You can close this tab.")
                };
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                if let Some(error) = error {
                    return Err(format!("Spotify authorization failed: {}", error));
                }
                break (
                    code.ok_or_else(|| {
                        "Spotify did not return an authorization code.".to_string()
                    })?,
                    returned_state
                        .ok_or_else(|| "Spotify did not return a security state.".to_string())?,
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(format!("Spotify callback failed: {}", error)),
        }
    };

    if returned_state != state {
        return Err("Spotify authorization state did not match.".into());
    }
    exchange_code(client_id, &code, &verifier)
}

fn exchange_code(client_id: &str, code: &str, verifier: &str) -> Result<Tokens, String> {
    let response: TokenResponse = agent()
        .post("https://accounts.spotify.com/api/token")
        .send_form(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", verifier),
        ])
        .map_err(|error| format!("Spotify token exchange failed: {}", error))?
        .into_json()
        .map_err(|error| format!("Spotify token response was invalid: {}", error))?;
    let refresh_token = response
        .refresh_token
        .ok_or_else(|| "Spotify did not return a refresh token.".to_string())?;
    Ok(Tokens {
        client_id: client_id.to_string(),
        access_token: response.access_token,
        refresh_token,
        expires_at: now_seconds() + response.expires_in,
        scopes: response.scope.unwrap_or_else(|| SCOPES.to_string()),
    })
}

fn refresh_token(tokens: &mut Tokens) -> Result<(), String> {
    let response: TokenResponse = agent()
        .post("https://accounts.spotify.com/api/token")
        .send_form(&[
            ("client_id", tokens.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens.refresh_token.as_str()),
        ])
        .map_err(|error| format!("Spotify token refresh failed: {}", error))?
        .into_json()
        .map_err(|error| format!("Spotify token response was invalid: {}", error))?;
    tokens.access_token = response.access_token;
    if let Some(refresh) = response.refresh_token {
        tokens.refresh_token = refresh;
    }
    if let Some(scope) = response.scope {
        tokens.scopes = scope;
    }
    tokens.expires_at = now_seconds() + response.expires_in;
    save_tokens(tokens)?;
    Ok(())
}

fn with_valid_token<T>(f: impl FnOnce(&str) -> Result<T, String>) -> Result<T, String> {
    let mut slot = tokens_cell()
        .lock()
        .map_err(|_| "Spotify token storage is unavailable.".to_string())?;
    let tokens = slot
        .as_mut()
        .ok_or_else(|| "Connect Spotify in Settings first.".to_string())?;
    if tokens.expires_at <= now_seconds() + 45 {
        refresh_token(tokens)?;
    }
    f(&tokens.access_token)
}

fn force_refresh() -> Result<(), String> {
    let mut slot = tokens_cell()
        .lock()
        .map_err(|_| "Spotify token storage is unavailable.".to_string())?;
    let tokens = slot
        .as_mut()
        .ok_or_else(|| "Connect Spotify in Settings first.".to_string())?;
    refresh_token(tokens)
}

fn current_playback(access: &str) -> Result<Option<Value>, String> {
    let response = agent()
        .get("https://api.spotify.com/v1/me/player/currently-playing")
        .set("Authorization", &format!("Bearer {}", access))
        .call();
    let response = match response {
        Ok(response) if response.status() == 204 => return Ok(None),
        Ok(response) => response,
        Err(ureq::Error::Status(204, _)) => return Ok(None),
        Err(ureq::Error::Status(401, _)) => return Err("TOKEN_EXPIRED".into()),
        Err(error) => return Err(format!("Spotify request failed: {}", error)),
    };
    response
        .into_json::<Value>()
        .map(Some)
        .map_err(|error| format!("Spotify response was invalid: {}", error))
}

fn poll_current_track() -> Result<(), String> {
    let result = with_valid_token(current_playback);
    let data = match result {
        Err(error) if error == "TOKEN_EXPIRED" => {
            force_refresh()?;
            with_valid_token(current_playback)?
        }
        other => other?,
    };

    match data {
        None => set_status(|state| {
            state.connected = true;
            state.title.clear();
            state.artist.clear();
            state.album.clear();
            state.is_playing = false;
            state.status = "Nothing is playing".into();
        }),
        Some(value) => {
            let item = value.get("item").and_then(Value::as_object);
            let title = item
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let artist = item
                .and_then(|item| item.get("artists"))
                .and_then(Value::as_array)
                .map(|artists| {
                    artists
                        .iter()
                        .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let album = item
                .and_then(|item| item.get("album"))
                .and_then(|album| album.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let is_playing = value
                .get("is_playing")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            set_status(|state| {
                state.connected = true;
                state.title = title;
                state.artist = artist;
                state.album = album;
                state.is_playing = is_playing;
                state.status = if is_playing { "Playing" } else { "Paused" }.into();
            });
        }
    }
    Ok(())
}

fn send_command(action: PlaybackAction) -> Result<(), String> {
    let (device_id, is_active) = with_refreshed_token(available_device)?
        .ok_or_else(|| "Open Spotify on a device, then try playback again.".to_string())?;
    if !is_active {
        with_refreshed_token(|access| transfer_playback(access, &device_id))?;
        thread::sleep(Duration::from_millis(700));
    }
    with_refreshed_token(|access| request_playback_action(access, action, &device_id))
}

fn with_refreshed_token<T>(
    mut request: impl FnMut(&str) -> Result<T, String>,
) -> Result<T, String> {
    let result = with_valid_token(|access| request(access));
    match result {
        Err(error) if error == "TOKEN_EXPIRED" => {
            force_refresh()?;
            with_valid_token(|access| request(access))
        }
        result => result,
    }
}

fn available_device(access: &str) -> Result<Option<(String, bool)>, String> {
    let response = agent()
        .get("https://api.spotify.com/v1/me/player/devices")
        .set("Authorization", &format!("Bearer {}", access))
        .call();
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => return Err("TOKEN_EXPIRED".into()),
        Err(ureq::Error::Status(403, response)) => {
            return Err(response_error(
                "Reconnect Spotify and approve playback-state access",
                403,
                response,
            ));
        }
        Err(error) => return Err(format!("Could not list Spotify devices: {}", error)),
    };
    let value: Value = response
        .into_json()
        .map_err(|error| format!("Spotify device response was invalid: {}", error))?;
    Ok(choose_device(&value))
}

fn choose_device(value: &Value) -> Option<(String, bool)> {
    let Some(devices) = value.get("devices").and_then(Value::as_array) else {
        return None;
    };
    let device = devices
        .iter()
        .filter_map(|device| {
            if device
                .get("is_restricted")
                .and_then(Value::as_bool)
                .unwrap_or(true)
            {
                return None;
            }
            let id = device.get("id")?.as_str()?.to_string();
            let active = device
                .get("is_active")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some((id, active))
        })
        .max_by_key(|(_, active)| *active);
    device
}

fn transfer_playback(access: &str, device_id: &str) -> Result<(), String> {
    let response = agent()
        .put("https://api.spotify.com/v1/me/player")
        .set("Authorization", &format!("Bearer {}", access))
        .send_json(serde_json::json!({"device_ids": [device_id], "play": false}));
    match response {
        Ok(_) | Err(ureq::Error::Status(204, _)) => Ok(()),
        Err(ureq::Error::Status(401, _)) => Err("TOKEN_EXPIRED".into()),
        Err(ureq::Error::Status(status, response)) => Err(response_error(
            "Spotify could not activate the selected device",
            status,
            response,
        )),
        Err(error) => Err(format!("Spotify device activation failed: {}", error)),
    }
}

fn request_playback_action(
    access: &str,
    action: PlaybackAction,
    device_id: &str,
) -> Result<(), String> {
    let (method, endpoint) = match action {
        PlaybackAction::Play => ("PUT", "play"),
        PlaybackAction::Pause => ("PUT", "pause"),
        PlaybackAction::Next => ("POST", "next"),
        PlaybackAction::Previous => ("POST", "previous"),
    };
    let url = format!(
        "https://api.spotify.com/v1/me/player/{}?device_id={}",
        endpoint,
        percent_encode(device_id)
    );
    let request = match method {
        "PUT" => agent().put(&url),
        _ => agent().post(&url),
    }
    .set("Authorization", &format!("Bearer {}", access));
    match request.send_bytes(&[]) {
        Ok(_) | Err(ureq::Error::Status(204, _)) => Ok(()),
        Err(ureq::Error::Status(401, _)) => Err("TOKEN_EXPIRED".into()),
        Err(ureq::Error::Status(403, response)) => Err(response_error(
            "Spotify rejected the playback command",
            403,
            response,
        )),
        Err(ureq::Error::Status(status, response)) => Err(response_error(
            "Spotify playback command failed",
            status,
            response,
        )),
        Err(error) => Err(format!("Spotify playback command failed: {}", error)),
    }
}

fn response_error(prefix: &str, status: u16, response: ureq::Response) -> String {
    let detail = response.into_json::<Value>().ok().and_then(|value| {
        value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    match (status, detail) {
        (403, Some(detail)) if detail.to_lowercase().contains("premium") => {
            "Spotify playback controls require Premium.".into()
        }
        (403, _) => format!(
            "{} (HTTP 403). Check Premium and approved permissions.",
            prefix
        ),
        (429, _) => "Spotify is rate limiting playback controls. Try again shortly.".into(),
        (_, Some(detail)) => format!("{} (HTTP {}): {}", prefix, status, detail),
        (_, None) => format!("{} (HTTP {}).", prefix, status),
    }
}

fn save_tokens(tokens: &Tokens) -> Result<(), String> {
    let entry = Entry::new(TOKEN_SERVICE, TOKEN_USER).map_err(|error| error.to_string())?;
    let serialized = serde_json::to_string(tokens).map_err(|error| error.to_string())?;
    entry
        .set_password(&serialized)
        .map_err(|error| error.to_string())
}

fn random_string(length: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut bytes = vec![0u8; length];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .into_iter()
        .map(|byte| ALPHABET[byte as usize % ALPHABET.len()] as char)
        .collect()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{:02X}", byte));
        }
    }
    encoded
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    decoded.push(byte);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn parse_callback(request: &str) -> (Option<String>, Option<String>, Option<String>) {
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    let query = target.split_once('?').map(|(_, query)| query).unwrap_or("");
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match percent_decode(key).as_str() {
            "code" => code = Some(percent_decode(value)),
            "state" => state = Some(percent_decode(value)),
            "error" => error = Some(percent_decode(value)),
            _ => {}
        }
    }
    (code, state, error)
}

fn read_callback(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") || request.len() > 8192 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

fn open_browser(url: &str) -> Result<(), String> {
    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let url: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<HWND>() as HWND,
            operation.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        Err("Could not open the browser for Spotify authorization.".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod playback_tests {
    use super::{choose_device, has_required_scopes};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn existing_authorization_without_playback_scope_requires_reconnect() {
        assert!(!has_required_scopes(
            "user-read-currently-playing user-modify-playback-state"
        ));
        assert!(has_required_scopes(
            "user-read-currently-playing user-read-playback-state user-modify-playback-state"
        ));
    }

    #[test]
    fn device_selection_prefers_active_supported_device() {
        let response = json!({
            "devices": [
                {"id":"restricted", "is_active":true, "is_restricted":true},
                {"id":"speaker", "is_active":false, "is_restricted":false},
                {"id":"desktop", "is_active":true, "is_restricted":false}
            ]
        });
        assert_eq!(choose_device(&response), Some(("desktop".into(), true)));
    }

    #[test]
    fn device_selection_falls_back_to_first_supported_device() {
        let response = json!({
            "devices": [
                {"id":"locked", "is_active":false, "is_restricted":true},
                {"id":"speaker", "is_active":false, "is_restricted":false}
            ]
        });
        assert_eq!(choose_device(&response), Some(("speaker".into(), false)));
    }

    #[test]
    fn empty_playback_request_sends_content_length_zero() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            String::from_utf8_lossy(&request).to_ascii_lowercase()
        });

        ureq::post(&format!("http://{}/player", address))
            .send_bytes(&[])
            .unwrap();
        let request = server.join().unwrap();
        assert!(request.contains("content-length: 0\r\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::parse_callback;

    #[test]
    fn callback_decodes_authorization_code_and_state() {
        let request =
            "GET /callback?code=abc%2F123&state=nonce%3D HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            parse_callback(request),
            (Some("abc/123".into()), Some("nonce=".into()), None)
        );
    }

    #[test]
    fn callback_reports_authorization_denial() {
        let request = "GET /callback?error=access_denied&state=nonce HTTP/1.1\r\n\r\n";
        assert_eq!(
            parse_callback(request),
            (None, Some("nonce".into()), Some("access_denied".into()))
        );
    }
}
