//! Global hotkeys that can tell the left modifier from the right one.
//!
//! Modifier-only chords (Right Ctrl on both desktops) are not OS accelerators.
//! A low-level key listener feeds this engine. Dictate, translate, and command
//! all start a recording. The dictate chord also ends the other two modes.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use super::ModuleStatus;

static OS_CAPTURE: AtomicBool = AtomicBool::new(false);
static ALTGR_BOUND: AtomicBool = AtomicBool::new(false);

/// The hook reads this on every key. It is set before any window focus call.
pub fn set_os_capture(on: bool) {
    OS_CAPTURE.store(on, Ordering::SeqCst);
}

pub fn os_capture() -> bool {
    OS_CAPTURE.load(Ordering::SeqCst)
}

pub fn set_altgr_bound(on: bool) {
    ALTGR_BOUND.store(on, Ordering::SeqCst);
}

/// A saved chord uses Right Alt, so the AltGr synthetic Left Ctrl must not leak.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn altgr_bound() -> bool {
    ALTGR_BOUND.load(Ordering::SeqCst)
}

/// Windows virtual-key code for a named key, when it has one stable code.
pub fn virtual_key(key: KeyId) -> Option<u32> {
    Some(match key {
        KeyId::AltLeft => 0xA4,
        KeyId::AltRight => 0xA5,
        KeyId::ControlLeft => 0xA2,
        KeyId::ControlRight => 0xA3,
        KeyId::ShiftLeft => 0xA0,
        KeyId::ShiftRight => 0xA1,
        KeyId::MetaLeft => 0x5B,
        KeyId::MetaRight => 0x5C,
        KeyId::Enter => 0x0D,
        KeyId::Space => 0x20,
        KeyId::Escape => 0x1B,
        KeyId::Tab => 0x09,
        KeyId::Backspace => 0x08,
        KeyId::Delete => 0x2E,
        KeyId::Letter(ch) if ch.is_ascii_uppercase() => ch as u32,
        KeyId::Digit(n) if n <= 9 => 0x30 + u32::from(n),
        KeyId::Function(n) if (1..=12).contains(&n) => 0x6F + u32::from(n),
        KeyId::Vk(code) if code > 0 => u32::from(code),
        _ => return None,
    })
}

/// Decides that Windows has dropped the low-level hook.
///
/// A watched key is physically down, or was pressed, and the hook has not
/// reported anything recently. The first sample only starts a suspicion so a
/// timer that runs a moment before the hook message does not reinstall.
#[derive(Debug, Default, Clone, Copy)]
pub struct HookWatch {
    suspect_since_ms: Option<u64>,
}

impl HookWatch {
    pub fn observe(&mut self, last_callback_ms: u64, now_ms: u64, key_activity: bool) -> bool {
        let recent = last_callback_ms > 0 && now_ms.saturating_sub(last_callback_ms) <= 300;
        if !key_activity || recent {
            self.suspect_since_ms = None;
            return false;
        }
        match self.suspect_since_ms {
            None => {
                self.suspect_since_ms = Some(now_ms);
                false
            }
            Some(since) => now_ms.saturating_sub(since) >= 400,
        }
    }
}

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "hotkey",
        status: "ready",
        note: "Dictate, translate, and command all record. Press the same chord again to stop. The dictate chord also stops translate and command.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsKind {
    Mac,
    Windows,
    Other,
}

pub fn host_os() -> OsKind {
    if cfg!(target_os = "macos") {
        OsKind::Mac
    } else if cfg!(target_os = "windows") {
        OsKind::Windows
    } else {
        OsKind::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyId {
    AltLeft,
    AltRight,
    ControlLeft,
    ControlRight,
    ShiftLeft,
    ShiftRight,
    MetaLeft,
    MetaRight,
    Enter,
    Space,
    Escape,
    Fn,
    Letter(u8),
    Function(u8),
    Slash,
    Backslash,
    Comma,
    Period,
    Minus,
    Equal,
    Semicolon,
    Quote,
    BracketLeft,
    BracketRight,
    Backquote,
    Tab,
    Backspace,
    Delete,
    Digit(u8),
    /// A key without a name. The payload keeps two different keys distinct.
    Vk(u16),
    /// A key we saw but do not name. It still blocks a modifier-only chord.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyMode {
    Dictate,
    Translate,
    Command,
}

impl HotkeyMode {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictate => "dictate",
            Self::Translate => "translate",
            Self::Command => "command",
        }
    }
}

pub type Chord = BTreeSet<KeyId>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEdge {
    Down(KeyId),
    Up(KeyId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    Pressed(HotkeyMode),
    Released(HotkeyMode),
    ExtraKeys,
    Escape,
    Captured(String),
    /// Keys seen so far. The chord is saved only on `Captured`.
    CaptureProgress(String),
    /// Escape while a new chord is being recorded. This is not a dictation cancel.
    CaptureCancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeOutcome {
    pub events: Vec<EngineEvent>,
    /// Swallow this key so it does not reach the foreground app.
    pub suppress: bool,
}

#[derive(Debug, Clone)]
pub struct HotkeyEngine {
    bindings: Vec<(HotkeyMode, Chord)>,
    down: Chord,
    active: Option<(HotkeyMode, Chord)>,
    extra_sent: bool,
    capturing: bool,
    captured: Chord,
    /// Non-modifier keys swallowed on the way down. Their key-up is swallowed too.
    suppressed: Chord,
}

impl Default for HotkeyEngine {
    fn default() -> Self {
        Self {
            bindings: Vec::new(),
            down: Chord::new(),
            active: None,
            extra_sent: false,
            capturing: false,
            captured: Chord::new(),
            suppressed: Chord::new(),
        }
    }
}

impl HotkeyEngine {
    pub fn set_bindings(&mut self, bindings: Vec<(HotkeyMode, Chord)>) {
        self.bindings = bindings;
    }

    pub fn set_capture(&mut self, capturing: bool) {
        if capturing == self.capturing {
            return;
        }
        self.down.clear();
        self.captured.clear();
        self.suppressed.clear();
        self.active = None;
        self.extra_sent = false;
        self.capturing = capturing;
    }

    pub fn uses_alt_right(&self) -> bool {
        self.bindings
            .iter()
            .any(|(_, chord)| chord.contains(&KeyId::AltRight))
    }

    pub fn on_edge(&mut self, edge: KeyEdge) -> Vec<EngineEvent> {
        self.consume(edge).events
    }

    /// Capture mode never matches a saved hotkey. A matched non-modifier is swallowed.
    pub fn consume(&mut self, edge: KeyEdge) -> EdgeOutcome {
        if self.capturing {
            return EdgeOutcome {
                events: self.on_capture(edge),
                suppress: true,
            };
        }
        match edge {
            KeyEdge::Down(KeyId::Escape) => EdgeOutcome {
                events: vec![EngineEvent::Escape],
                suppress: false,
            },
            KeyEdge::Down(key) => {
                if !self.down.insert(key) {
                    return EdgeOutcome {
                        events: Vec::new(),
                        suppress: self.suppressed.contains(&key),
                    };
                }
                let events = self.promote();
                let suppress = self.arm_suppress(key);
                EdgeOutcome { events, suppress }
            }
            KeyEdge::Up(key) => {
                let suppress = self.suppressed.remove(&key);
                self.down.remove(&key);
                let mut events = Vec::new();
                if let Some((mode, chord)) = &self.active {
                    if !chord.is_subset(&self.down) {
                        let mode = *mode;
                        self.active = None;
                        self.extra_sent = false;
                        events.push(EngineEvent::Released(mode));
                    }
                }
                EdgeOutcome { events, suppress }
            }
        }
    }

    fn arm_suppress(&mut self, key: KeyId) -> bool {
        if is_modifier(key) || matches!(key, KeyId::Escape | KeyId::Other) {
            return false;
        }
        let swallow = self
            .active
            .as_ref()
            .is_some_and(|(_, chord)| chord.contains(&key));
        if swallow {
            self.suppressed.insert(key);
        }
        swallow
    }

    fn promote(&mut self) -> Vec<EngineEvent> {
        let exact = self
            .bindings
            .iter()
            .filter(|(_, chord)| !chord.is_empty() && chord == &self.down)
            .max_by_key(|(_, chord)| chord.len())
            .map(|(mode, chord)| (*mode, chord.clone()));
        if let Some((mode, chord)) = exact {
            self.extra_sent = false;
            if self
                .active
                .as_ref()
                .is_some_and(|(_, active)| active == &chord)
            {
                return Vec::new();
            }
            self.active = Some((mode, chord));
            return vec![EngineEvent::Pressed(mode)];
        }
        if self.active.is_some() && !self.extra_sent {
            self.extra_sent = true;
            return vec![EngineEvent::ExtraKeys];
        }
        Vec::new()
    }

    fn on_capture(&mut self, edge: KeyEdge) -> Vec<EngineEvent> {
        match edge {
            KeyEdge::Down(KeyId::Escape) => {
                self.capturing = false;
                self.captured.clear();
                self.down.clear();
                self.suppressed.clear();
                self.active = None;
                vec![EngineEvent::CaptureCancelled]
            }
            KeyEdge::Down(KeyId::Other) | KeyEdge::Up(KeyId::Other) => Vec::new(),
            KeyEdge::Down(key) => {
                let fresh = self.down.insert(key);
                self.captured.insert(key);
                if fresh {
                    vec![EngineEvent::CaptureProgress(canonical(&self.captured))]
                } else {
                    Vec::new()
                }
            }
            KeyEdge::Up(key) => {
                self.down.remove(&key);
                if self.down.is_empty() && !self.captured.is_empty() {
                    let chord = std::mem::take(&mut self.captured);
                    self.capturing = false;
                    self.suppressed.clear();
                    vec![EngineEvent::Captured(canonical(&chord))]
                } else if !self.captured.is_empty() {
                    vec![EngineEvent::CaptureProgress(canonical(&self.captured))]
                } else {
                    Vec::new()
                }
            }
        }
    }
}

pub fn is_modifier(key: KeyId) -> bool {
    matches!(
        key,
        KeyId::AltLeft
            | KeyId::AltRight
            | KeyId::ControlLeft
            | KeyId::ControlRight
            | KeyId::ShiftLeft
            | KeyId::ShiftRight
            | KeyId::MetaLeft
            | KeyId::MetaRight
            | KeyId::Fn
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkMode {
    Toggle,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkEffect {
    Start(HotkeyMode),
    Stop,
    Cancel,
    /// The chord window replaced the mode that just started. Drop that audio.
    CancelThenStart(HotkeyMode),
    Ignore,
}

const CHORD_WINDOW_MS: u64 = 200;

#[derive(Debug, Clone)]
pub struct TalkController {
    pub talk_mode: TalkMode,
    pub active: Option<HotkeyMode>,
    pub started_ms: u64,
}

impl TalkController {
    pub fn new(talk_mode: TalkMode) -> Self {
        Self {
            talk_mode,
            active: None,
            started_ms: 0,
        }
    }

    pub fn on_event(&mut self, event: &EngineEvent, now_ms: u64) -> TalkEffect {
        match event {
            EngineEvent::Pressed(mode) => self.on_pressed(*mode, now_ms),
            EngineEvent::Released(mode) => {
                if self.talk_mode == TalkMode::Hold && self.active == Some(*mode) {
                    self.active = None;
                    TalkEffect::Stop
                } else {
                    TalkEffect::Ignore
                }
            }
            EngineEvent::ExtraKeys => {
                if self.active.is_some()
                    && now_ms.saturating_sub(self.started_ms) <= CHORD_WINDOW_MS
                {
                    self.active = None;
                    TalkEffect::Cancel
                } else {
                    TalkEffect::Ignore
                }
            }
            EngineEvent::Escape => {
                if self.active.take().is_some() {
                    TalkEffect::Cancel
                } else {
                    TalkEffect::Ignore
                }
            }
            EngineEvent::Captured(_)
            | EngineEvent::CaptureProgress(_)
            | EngineEvent::CaptureCancelled => TalkEffect::Ignore,
        }
    }

    fn on_pressed(&mut self, mode: HotkeyMode, now_ms: u64) -> TalkEffect {
        if let Some(active) = self.active {
            if mode == active {
                if self.talk_mode == TalkMode::Toggle {
                    self.active = None;
                    return TalkEffect::Stop;
                }
                return TalkEffect::Ignore;
            }
            let age = now_ms.saturating_sub(self.started_ms);
            if age <= CHORD_WINDOW_MS {
                self.active = Some(mode);
                self.started_ms = now_ms;
                return TalkEffect::CancelThenStart(mode);
            }
            if mode == HotkeyMode::Dictate && self.talk_mode == TalkMode::Toggle {
                self.active = None;
                return TalkEffect::Stop;
            }
            return TalkEffect::Ignore;
        }
        self.active = Some(mode);
        self.started_ms = now_ms;
        TalkEffect::Start(mode)
    }
}

pub fn default_chord(mode: HotkeyMode, _os: OsKind) -> Chord {
    // macOS matches Windows. Right Option is Typeless when a PC keyboard
    // (Right Alt) is used on the Mac, so the factory key is Right Control.
    let mut chord = Chord::new();
    chord.insert(KeyId::ControlRight);
    match mode {
        HotkeyMode::Dictate => {}
        HotkeyMode::Translate => {
            chord.insert(KeyId::ShiftRight);
        }
        HotkeyMode::Command => {
            chord.insert(KeyId::Enter);
        }
    }
    chord
}

pub fn parse_chord(input: &str) -> Result<Chord, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Hotkey is empty.".into());
    }
    let mut chord = Chord::new();
    for token in trimmed.split('+') {
        let key = parse_key(token.trim())?;
        if key == KeyId::Escape {
            return Err("Escape cancels dictation and cannot be a hotkey.".into());
        }
        if !chord.insert(key) {
            return Err(format!(
                "{} is repeated in the hotkey.",
                key_label(key, OsKind::Other)
            ));
        }
    }
    Ok(chord)
}

pub fn effective_chord(stored: &str, mode: HotkeyMode, os: OsKind) -> Result<Chord, String> {
    if stored.trim().is_empty() {
        Ok(default_chord(mode, os))
    } else {
        parse_chord(stored)
    }
}

pub fn canonical(chord: &Chord) -> String {
    let mut keys: Vec<KeyId> = chord.iter().copied().collect();
    keys.sort_by_key(key_sort);
    keys.iter()
        .map(|key| key_token(*key))
        .collect::<Vec<_>>()
        .join("+")
}

pub fn chord_label(chord: &Chord, os: OsKind) -> String {
    let mut keys: Vec<KeyId> = chord.iter().copied().collect();
    keys.sort_by_key(key_sort);
    keys.iter()
        .map(|key| key_label(*key, os))
        .collect::<Vec<_>>()
        .join(" + ")
}

pub fn binding_error(bindings: &[(HotkeyMode, Chord)]) -> Option<String> {
    for (index, (mode, chord)) in bindings.iter().enumerate() {
        if chord.is_empty() {
            return Some(format!("{} hotkey is empty.", mode_name(*mode)));
        }
        for (other_mode, other) in bindings.iter().skip(index + 1) {
            if chord == other {
                return Some(format!(
                    "{} and {} use the same hotkey.",
                    mode_name(*mode),
                    mode_name(*other_mode)
                ));
            }
        }
    }
    None
}

pub fn binding_warnings(bindings: &[(HotkeyMode, Chord)], os: OsKind) -> Vec<String> {
    let mut warnings = Vec::new();
    for (index, (_, chord)) in bindings.iter().enumerate() {
        for (_, other) in bindings.iter().skip(index + 1) {
            if chord != other && (chord.is_subset(other) || other.is_subset(chord)) {
                let (short, long) = if chord.len() <= other.len() {
                    (chord, other)
                } else {
                    (other, chord)
                };
                warnings.push(format!(
                    "{} is also part of {}. A short press uses the shorter one; adding the extra key at the start uses the longer one.",
                    chord_label(short, os),
                    chord_label(long, os)
                ));
            }
        }
        if let Some(message) = known_conflict(chord, os) {
            warnings.push(message.to_string());
        }
    }
    warnings
}

fn known_conflict(chord: &Chord, os: OsKind) -> Option<&'static str> {
    let is = |keys: &[KeyId]| chord == &keys.iter().copied().collect::<Chord>();
    if os == OsKind::Mac && is(&[KeyId::Fn]) {
        return Some(
            "Fn is Typeless's dictate key on macOS. Pick another key if both apps are running.",
        );
    }
    if os == OsKind::Mac && is(&[KeyId::Fn, KeyId::ShiftLeft]) {
        return Some("Fn + Left Shift is a Typeless shortcut on macOS.");
    }
    if os == OsKind::Mac && is(&[KeyId::Fn, KeyId::Space]) {
        return Some("Fn + Space is a Typeless shortcut on macOS.");
    }
    if is(&[KeyId::AltRight]) {
        return Some("Right Option / Right Alt is Typeless's dictate key, including a PC keyboard on a Mac. The newtype default is Right Ctrl.");
    }
    if is(&[KeyId::AltRight, KeyId::ShiftRight]) {
        return Some("Right Option + Right Shift is a Typeless shortcut. The newtype default is Right Ctrl + Right Shift.");
    }
    if is(&[KeyId::ControlLeft, KeyId::Space]) {
        return Some("Left Ctrl + Space switches input methods on many systems.");
    }
    if os == OsKind::Mac && is(&[KeyId::AltLeft, KeyId::Space]) {
        return Some("Left Option + Space is Alfred's usual shortcut.");
    }
    None
}

fn mode_name(mode: HotkeyMode) -> &'static str {
    match mode {
        HotkeyMode::Dictate => "Dictate",
        HotkeyMode::Translate => "Translate",
        HotkeyMode::Command => "Command",
    }
}

fn parse_key(token: &str) -> Result<KeyId, String> {
    let folded: String = token
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    let key = match folded.as_str() {
        "altright" | "rightoption" | "rightalt" | "ralt" | "roption" => KeyId::AltRight,
        "altleft" | "leftoption" | "leftalt" | "lalt" | "loption" => KeyId::AltLeft,
        "controlright" | "rightcontrol" | "rightctrl" | "rctrl" | "rcontrol" => KeyId::ControlRight,
        "controlleft" | "leftcontrol" | "leftctrl" | "lctrl" | "lcontrol" => KeyId::ControlLeft,
        "shiftright" | "rightshift" | "rshift" => KeyId::ShiftRight,
        "shiftleft" | "leftshift" | "lshift" => KeyId::ShiftLeft,
        "metaright" | "rightcommand" | "rightcmd" | "rightmeta" | "rightwin" => KeyId::MetaRight,
        "metaleft" | "leftcommand" | "leftcmd" | "leftmeta" | "leftwin" => KeyId::MetaLeft,
        "enter" | "return" => KeyId::Enter,
        "space" | "spacebar" => KeyId::Space,
        "escape" | "esc" => KeyId::Escape,
        "fn" | "function" => KeyId::Fn,
        "slash" | "/" | "oem2" => KeyId::Slash,
        "backslash" | "\\" => KeyId::Backslash,
        "comma" | "," => KeyId::Comma,
        "period" | "dot" | "." => KeyId::Period,
        "minus" | "-" => KeyId::Minus,
        "equal" | "equals" | "=" => KeyId::Equal,
        "semicolon" | ";" => KeyId::Semicolon,
        "quote" | "'" => KeyId::Quote,
        "bracketleft" | "[" => KeyId::BracketLeft,
        "bracketright" | "]" => KeyId::BracketRight,
        "backquote" | "`" => KeyId::Backquote,
        "tab" => KeyId::Tab,
        "backspace" => KeyId::Backspace,
        "delete" | "del" => KeyId::Delete,
        "capslock" | "caps" => KeyId::Vk(0x14),
        "alt" | "option" | "opt" => {
            return Err("Say which side: Right Option or Left Option.".into());
        }
        "ctrl" | "control" => {
            return Err("Say which side: Right Ctrl or Left Ctrl.".into());
        }
        "shift" => return Err("Say which side: Right Shift or Left Shift.".into()),
        "command" | "cmd" | "meta" | "super" | "win" => {
            return Err("Say which side: Left Command or Right Command.".into());
        }
        other => {
            if let Some(rest) = other.strip_prefix('f') {
                if let Ok(number) = rest.parse::<u8>() {
                    if (1..=12).contains(&number) {
                        return Ok(KeyId::Function(number));
                    }
                }
            }
            if let Some(rest) = other.strip_prefix("digit") {
                if let Ok(number) = rest.parse::<u8>() {
                    if number <= 9 {
                        return Ok(KeyId::Digit(number));
                    }
                }
            }
            if let Some(rest) = other.strip_prefix("vk") {
                if let Ok(code) = rest.parse::<u16>() {
                    if code > 0 {
                        return Ok(KeyId::Vk(code));
                    }
                }
            }
            let mut chars = other.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                if ch.is_ascii_alphabetic() {
                    return Ok(KeyId::Letter(ch.to_ascii_uppercase() as u8));
                }
                if ch.is_ascii_digit() {
                    return Ok(KeyId::Digit(ch.to_digit(10).unwrap_or(0) as u8));
                }
            }
            if let Some(rest) = other.strip_prefix("key") {
                let mut chars = rest.chars();
                if let (Some(ch), None) = (chars.next(), chars.next()) {
                    if ch.is_ascii_alphabetic() {
                        return Ok(KeyId::Letter(ch.to_ascii_uppercase() as u8));
                    }
                }
            }
            return Err(format!("Unknown key \"{token}\"."));
        }
    };
    Ok(key)
}

fn key_sort(key: &KeyId) -> u8 {
    match *key {
        KeyId::ControlLeft | KeyId::ControlRight => 0,
        KeyId::AltLeft | KeyId::AltRight => 1,
        KeyId::ShiftLeft | KeyId::ShiftRight => 2,
        KeyId::MetaLeft | KeyId::MetaRight => 3,
        KeyId::Fn => 4,
        _ => 5,
    }
}

fn key_token(key: KeyId) -> String {
    match key {
        KeyId::AltLeft => "AltLeft".into(),
        KeyId::AltRight => "AltRight".into(),
        KeyId::ControlLeft => "ControlLeft".into(),
        KeyId::ControlRight => "ControlRight".into(),
        KeyId::ShiftLeft => "ShiftLeft".into(),
        KeyId::ShiftRight => "ShiftRight".into(),
        KeyId::MetaLeft => "MetaLeft".into(),
        KeyId::MetaRight => "MetaRight".into(),
        KeyId::Enter => "Enter".into(),
        KeyId::Space => "Space".into(),
        KeyId::Escape => "Escape".into(),
        KeyId::Fn => "Fn".into(),
        KeyId::Letter(ch) => format!("{}", ch as char),
        KeyId::Function(n) => format!("F{n}"),
        KeyId::Slash => "Slash".into(),
        KeyId::Backslash => "Backslash".into(),
        KeyId::Comma => "Comma".into(),
        KeyId::Period => "Period".into(),
        KeyId::Minus => "Minus".into(),
        KeyId::Equal => "Equal".into(),
        KeyId::Semicolon => "Semicolon".into(),
        KeyId::Quote => "Quote".into(),
        KeyId::BracketLeft => "BracketLeft".into(),
        KeyId::BracketRight => "BracketRight".into(),
        KeyId::Backquote => "Backquote".into(),
        KeyId::Tab => "Tab".into(),
        KeyId::Backspace => "Backspace".into(),
        KeyId::Delete => "Delete".into(),
        KeyId::Digit(n) => format!("Digit{n}"),
        KeyId::Vk(code) => format!("Vk{code}"),
        KeyId::Other => "Other".into(),
    }
}

pub fn key_label(key: KeyId, os: OsKind) -> String {
    let mac = os == OsKind::Mac;
    match key {
        KeyId::AltLeft => {
            if mac {
                "Left Option".into()
            } else {
                "Left Alt".into()
            }
        }
        KeyId::AltRight => {
            if mac {
                "Right Option".into()
            } else {
                "Right Alt".into()
            }
        }
        KeyId::ControlLeft => "Left Ctrl".into(),
        KeyId::ControlRight => "Right Ctrl".into(),
        KeyId::ShiftLeft => "Left Shift".into(),
        KeyId::ShiftRight => "Right Shift".into(),
        KeyId::MetaLeft => {
            if mac {
                "Left Command".into()
            } else {
                "Left Win".into()
            }
        }
        KeyId::MetaRight => {
            if mac {
                "Right Command".into()
            } else {
                "Right Win".into()
            }
        }
        KeyId::Enter => "Enter".into(),
        KeyId::Space => "Space".into(),
        KeyId::Escape => "Escape".into(),
        KeyId::Fn => "Fn".into(),
        KeyId::Letter(ch) => format!("{}", ch as char),
        KeyId::Function(n) => format!("F{n}"),
        KeyId::Slash => "Slash".into(),
        KeyId::Backslash => "Backslash".into(),
        KeyId::Comma => "Comma".into(),
        KeyId::Period => "Period".into(),
        KeyId::Minus => "Minus".into(),
        KeyId::Equal => "Equal".into(),
        KeyId::Semicolon => "Semicolon".into(),
        KeyId::Quote => "Quote".into(),
        KeyId::BracketLeft => "BracketLeft".into(),
        KeyId::BracketRight => "BracketRight".into(),
        KeyId::Backquote => "Backquote".into(),
        KeyId::Tab => "Tab".into(),
        KeyId::Backspace => "Backspace".into(),
        KeyId::Delete => "Delete".into(),
        KeyId::Digit(n) => format!("{n}"),
        KeyId::Vk(0x14) => "Caps Lock".into(),
        KeyId::Vk(code) => format!("Vk{code}"),
        KeyId::Other => "Other".into(),
    }
}

/// AltGr on Windows injects a left Ctrl (extended scan `0x1D` / `0x21D`) next to Right Alt.
/// Real Left Ctrl uses scan `0x1D` and is not extended. Right Ctrl is a different virtual key.
pub fn is_altgr_synthetic_ctrl(vk: u32, scan: u32, flags: u32) -> bool {
    const EXTENDED: u32 = 0x01;
    let ctrl = vk == 0xA2 || vk == 0x11;
    if !ctrl {
        return false;
    }
    let extended = flags & EXTENDED != 0;
    let body = scan & 0xFF;
    extended && (body == 0x1D || scan == 0x021D)
}

/// Map one low-level Windows key event. Synthetic AltGr Ctrl is dropped.
pub fn edge_from_windows(vk: u32, scan: u32, flags: u32, key_up: bool) -> Option<KeyEdge> {
    if is_altgr_synthetic_ctrl(vk, scan, flags) {
        return None;
    }
    let key = key_from_windows_detail(vk, scan, flags);
    if key == KeyId::Other && vk == 0 {
        return None;
    }
    let released = key_up || flags & 0x80 != 0;
    Some(if released {
        KeyEdge::Up(key)
    } else {
        KeyEdge::Down(key)
    })
}

fn key_from_windows_detail(vk: u32, scan: u32, flags: u32) -> KeyId {
    const EXTENDED: u32 = 0x01;
    match vk {
        0x10 => {
            if scan & 0xFF == 0x36 {
                KeyId::ShiftRight
            } else {
                KeyId::ShiftLeft
            }
        }
        0x11 => {
            if flags & EXTENDED != 0 {
                KeyId::ControlRight
            } else {
                KeyId::ControlLeft
            }
        }
        0x12 => {
            if flags & EXTENDED != 0 {
                KeyId::AltRight
            } else {
                KeyId::AltLeft
            }
        }
        other => key_from_windows_vk(other),
    }
}

/// DOM `KeyboardEvent.code`. `alt` is true when the event's Alt flag is set,
/// which is how a webview reports the synthetic Ctrl that rides with AltGr.
pub fn edge_from_code(code: &str, down: bool, alt: bool) -> Option<KeyEdge> {
    if code == "ControlLeft" && alt {
        return None;
    }
    let key = match code {
        "AltRight" => KeyId::AltRight,
        "AltLeft" => KeyId::AltLeft,
        "ControlRight" => KeyId::ControlRight,
        "ControlLeft" => KeyId::ControlLeft,
        "ShiftRight" => KeyId::ShiftRight,
        "ShiftLeft" => KeyId::ShiftLeft,
        "MetaRight" | "OSRight" => KeyId::MetaRight,
        "MetaLeft" | "OSLeft" => KeyId::MetaLeft,
        "Enter" | "NumpadEnter" => KeyId::Enter,
        "Space" => KeyId::Space,
        "Escape" => KeyId::Escape,
        "Slash" | "NumpadDivide" => KeyId::Slash,
        "Backslash" => KeyId::Backslash,
        "Comma" => KeyId::Comma,
        "Period" => KeyId::Period,
        "Minus" => KeyId::Minus,
        "Equal" => KeyId::Equal,
        "Semicolon" => KeyId::Semicolon,
        "Quote" => KeyId::Quote,
        "BracketLeft" => KeyId::BracketLeft,
        "BracketRight" => KeyId::BracketRight,
        "Backquote" => KeyId::Backquote,
        "Tab" => KeyId::Tab,
        "Backspace" => KeyId::Backspace,
        "Delete" => KeyId::Delete,
        other => code_to_key(other)?,
    };
    Some(if down {
        KeyEdge::Down(key)
    } else {
        KeyEdge::Up(key)
    })
}

fn code_to_key(code: &str) -> Option<KeyId> {
    if let Some(rest) = code.strip_prefix("Digit") {
        if let Ok(number) = rest.parse::<u8>() {
            if number <= 9 {
                return Some(KeyId::Digit(number));
            }
        }
    }
    if let Some(rest) = code.strip_prefix("Key") {
        let mut chars = rest.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            if ch.is_ascii_alphabetic() {
                return Some(KeyId::Letter(ch.to_ascii_uppercase() as u8));
            }
        }
    }
    if let Some(rest) = code.strip_prefix('F') {
        if let Ok(number) = rest.parse::<u8>() {
            if (1..=12).contains(&number) {
                return Some(KeyId::Function(number));
            }
        }
    }
    None
}

/// Windows virtual-key codes. `VK_CONTROL` (0x11) is not treated as Right Ctrl.
pub fn key_from_windows_vk(code: u32) -> KeyId {
    match code {
        162 => KeyId::ControlLeft,
        163 => KeyId::ControlRight,
        160 => KeyId::ShiftLeft,
        161 => KeyId::ShiftRight,
        164 => KeyId::AltLeft,
        165 => KeyId::AltRight,
        91 => KeyId::MetaLeft,
        92 => KeyId::MetaRight,
        13 => KeyId::Enter,
        27 => KeyId::Escape,
        32 => KeyId::Space,
        0x08 => KeyId::Backspace,
        0x09 => KeyId::Tab,
        0x2E => KeyId::Delete,
        0x10 | 0x11 | 0x12 => KeyId::Other,
        0x30..=0x39 => KeyId::Digit((code - 0x30) as u8),
        0x41..=0x5A => KeyId::Letter(code as u8),
        0x70..=0x7B => KeyId::Function((code - 0x6F) as u8),
        0xBA => KeyId::Semicolon,
        0xBB => KeyId::Equal,
        0xBC => KeyId::Comma,
        0xBD => KeyId::Minus,
        0xBE => KeyId::Period,
        0xBF => KeyId::Slash,
        0xC0 => KeyId::Backquote,
        0xDB => KeyId::BracketLeft,
        0xDC => KeyId::Backslash,
        0xDD => KeyId::BracketRight,
        0xDE => KeyId::Quote,
        1..=65535 => KeyId::Vk(code as u16),
        _ => KeyId::Other,
    }
}

/// Map a hardware key from the listener. Right Option and Right Alt are `AltRight`.
pub fn key_from_rdev(key: &rdev::Key) -> Option<KeyId> {
    use rdev::Key::*;
    Some(match key {
        Alt => KeyId::AltLeft,
        AltGr => KeyId::AltRight,
        ControlLeft => KeyId::ControlLeft,
        ControlRight => KeyId::ControlRight,
        ShiftLeft => KeyId::ShiftLeft,
        ShiftRight => KeyId::ShiftRight,
        MetaLeft => KeyId::MetaLeft,
        MetaRight => KeyId::MetaRight,
        Return | KpReturn => KeyId::Enter,
        Space => KeyId::Space,
        Escape => KeyId::Escape,
        Function => KeyId::Fn,
        KeyA => KeyId::Letter(b'A'),
        KeyB => KeyId::Letter(b'B'),
        KeyC => KeyId::Letter(b'C'),
        KeyD => KeyId::Letter(b'D'),
        KeyE => KeyId::Letter(b'E'),
        KeyF => KeyId::Letter(b'F'),
        KeyG => KeyId::Letter(b'G'),
        KeyH => KeyId::Letter(b'H'),
        KeyI => KeyId::Letter(b'I'),
        KeyJ => KeyId::Letter(b'J'),
        KeyK => KeyId::Letter(b'K'),
        KeyL => KeyId::Letter(b'L'),
        KeyM => KeyId::Letter(b'M'),
        KeyN => KeyId::Letter(b'N'),
        KeyO => KeyId::Letter(b'O'),
        KeyP => KeyId::Letter(b'P'),
        KeyQ => KeyId::Letter(b'Q'),
        KeyR => KeyId::Letter(b'R'),
        KeyS => KeyId::Letter(b'S'),
        KeyT => KeyId::Letter(b'T'),
        KeyU => KeyId::Letter(b'U'),
        KeyV => KeyId::Letter(b'V'),
        KeyW => KeyId::Letter(b'W'),
        KeyX => KeyId::Letter(b'X'),
        KeyY => KeyId::Letter(b'Y'),
        KeyZ => KeyId::Letter(b'Z'),
        F1 => KeyId::Function(1),
        F2 => KeyId::Function(2),
        F3 => KeyId::Function(3),
        F4 => KeyId::Function(4),
        F5 => KeyId::Function(5),
        F6 => KeyId::Function(6),
        F7 => KeyId::Function(7),
        F8 => KeyId::Function(8),
        F9 => KeyId::Function(9),
        F10 => KeyId::Function(10),
        F11 => KeyId::Function(11),
        F12 => KeyId::Function(12),
        Slash | KpDivide => KeyId::Slash,
        BackSlash | IntlBackslash => KeyId::Backslash,
        Comma => KeyId::Comma,
        Dot => KeyId::Period,
        Minus => KeyId::Minus,
        Equal => KeyId::Equal,
        SemiColon => KeyId::Semicolon,
        Quote => KeyId::Quote,
        LeftBracket => KeyId::BracketLeft,
        RightBracket => KeyId::BracketRight,
        BackQuote => KeyId::Backquote,
        Tab => KeyId::Tab,
        Backspace => KeyId::Backspace,
        Delete => KeyId::Delete,
        Num0 | Kp0 => KeyId::Digit(0),
        Num1 | Kp1 => KeyId::Digit(1),
        Num2 | Kp2 => KeyId::Digit(2),
        Num3 | Kp3 => KeyId::Digit(3),
        Num4 | Kp4 => KeyId::Digit(4),
        Num5 | Kp5 => KeyId::Digit(5),
        Num6 | Kp6 => KeyId::Digit(6),
        Num7 | Kp7 => KeyId::Digit(7),
        Num8 | Kp8 => KeyId::Digit(8),
        Num9 | Kp9 => KeyId::Digit(9),
        Unknown(code) if *code > 0 && *code <= u16::MAX as u32 => KeyId::Vk(*code as u16),
        Unknown(_) => KeyId::Other,
        _ => KeyId::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(keys: &[KeyId]) -> Chord {
        keys.iter().copied().collect()
    }

    fn engine(os: OsKind) -> HotkeyEngine {
        let mut engine = HotkeyEngine::default();
        engine.set_bindings(vec![
            (HotkeyMode::Dictate, default_chord(HotkeyMode::Dictate, os)),
            (
                HotkeyMode::Translate,
                default_chord(HotkeyMode::Translate, os),
            ),
            (HotkeyMode::Command, default_chord(HotkeyMode::Command, os)),
        ]);
        engine
    }

    #[test]
    fn windows_right_control_is_not_the_generic_control_key() {
        assert_eq!(key_from_windows_vk(163), KeyId::ControlRight);
        assert_eq!(key_from_windows_vk(162), KeyId::ControlLeft);
        assert_eq!(key_from_windows_vk(0x11), KeyId::Other);
        assert_eq!(key_from_windows_vk(165), KeyId::AltRight);
        assert_ne!(key_from_windows_vk(163), key_from_windows_vk(162));
    }

    #[test]
    fn left_and_right_modifiers_are_different_keys() {
        assert_ne!(
            parse_chord("Right Option").unwrap(),
            parse_chord("Left Option").unwrap()
        );
        assert_ne!(
            parse_chord("Right Ctrl").unwrap(),
            parse_chord("Left Ctrl").unwrap()
        );
        assert!(parse_chord("Option").is_err());
        assert!(parse_chord("Ctrl").is_err());
        assert_eq!(
            canonical(&parse_chord("Right Option + Right Shift").unwrap()),
            "AltRight+ShiftRight"
        );
    }

    #[test]
    fn factory_defaults_avoid_typeless_and_allow_a_longer_chord() {
        for os in [OsKind::Mac, OsKind::Windows] {
            let bindings = [
                (HotkeyMode::Dictate, default_chord(HotkeyMode::Dictate, os)),
                (
                    HotkeyMode::Translate,
                    default_chord(HotkeyMode::Translate, os),
                ),
                (HotkeyMode::Command, default_chord(HotkeyMode::Command, os)),
            ];
            assert!(binding_error(&bindings).is_none());
            let warnings = binding_warnings(&bindings, os);
            assert!(warnings.iter().all(|warning| !warning.contains("Typeless")));
            assert!(warnings
                .iter()
                .any(|warning| warning.contains("short press")));
        }
        let right_ctrl = chord(&[KeyId::ControlRight]);
        assert_eq!(default_chord(HotkeyMode::Dictate, OsKind::Mac), right_ctrl);
        assert_eq!(
            default_chord(HotkeyMode::Dictate, OsKind::Windows),
            right_ctrl
        );
        assert_eq!(
            default_chord(HotkeyMode::Translate, OsKind::Mac),
            chord(&[KeyId::ControlRight, KeyId::ShiftRight])
        );
        assert_eq!(
            default_chord(HotkeyMode::Command, OsKind::Mac),
            chord(&[KeyId::ControlRight, KeyId::Enter])
        );
        assert_ne!(right_ctrl, chord(&[KeyId::ControlLeft]));
        assert_ne!(right_ctrl, chord(&[KeyId::AltRight]));
        let typeless = [(HotkeyMode::Dictate, chord(&[KeyId::AltRight]))];
        assert!(binding_warnings(&typeless, OsKind::Windows)
            .iter()
            .any(|warning| warning.contains("Typeless")));
        assert!(binding_warnings(&typeless, OsKind::Mac)
            .iter()
            .any(|warning| warning.contains("Typeless")));
    }

    #[test]
    fn engine_distinguishes_sides_and_disambiguates_chords() {
        let mut engine = engine(OsKind::Mac);
        assert!(engine.on_edge(KeyEdge::Down(KeyId::ControlLeft)).is_empty());
        assert!(engine.on_edge(KeyEdge::Down(KeyId::AltRight)).is_empty());
        engine.on_edge(KeyEdge::Up(KeyId::ControlLeft));
        engine.on_edge(KeyEdge::Up(KeyId::AltRight));
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::ControlRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
        assert!(engine
            .on_edge(KeyEdge::Down(KeyId::ControlRight))
            .is_empty());
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::ShiftRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Translate)]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Up(KeyId::ShiftRight)),
            vec![EngineEvent::Released(HotkeyMode::Translate)]
        );
    }

    #[test]
    fn extra_key_and_escape_are_visible_to_the_session() {
        let mut engine = engine(OsKind::Windows);
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::ControlRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::Letter(b'A'))),
            vec![EngineEvent::ExtraKeys]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::Escape)),
            vec![EngineEvent::Escape]
        );
    }

    #[test]
    fn capture_records_the_side_that_was_pressed() {
        let mut engine = HotkeyEngine::default();
        engine.set_capture(true);
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::ControlRight)),
            vec![EngineEvent::CaptureProgress("ControlRight".into())]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Up(KeyId::ControlRight)),
            vec![EngineEvent::Captured("ControlRight".into())]
        );
    }

    #[test]
    fn capture_does_not_start_dictation_for_the_saved_chord() {
        let mut engine = HotkeyEngine::default();
        engine.set_bindings(vec![
            (HotkeyMode::Dictate, parse_chord("AltRight").unwrap()),
            (
                HotkeyMode::Translate,
                parse_chord("AltRight+ShiftRight").unwrap(),
            ),
            (HotkeyMode::Command, parse_chord("AltRight+Enter").unwrap()),
        ]);
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::ControlRight)),
            Vec::new()
        );
        engine.on_edge(KeyEdge::Up(KeyId::ControlRight));
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::AltRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
        engine.on_edge(KeyEdge::Up(KeyId::AltRight));

        engine.set_capture(true);
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::AltRight)),
            vec![EngineEvent::CaptureProgress("AltRight".into())]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::ShiftRight)),
            vec![EngineEvent::CaptureProgress("AltRight+ShiftRight".into())]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Up(KeyId::ShiftRight)),
            vec![EngineEvent::CaptureProgress("AltRight+ShiftRight".into())]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Up(KeyId::AltRight)),
            vec![EngineEvent::Captured("AltRight+ShiftRight".into())]
        );
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::AltRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
        engine.on_edge(KeyEdge::Up(KeyId::AltRight));

        engine.set_capture(true);
        engine.on_edge(KeyEdge::Down(KeyId::AltRight));
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::Escape)),
            vec![EngineEvent::CaptureCancelled]
        );
        let mut talk = TalkController::new(TalkMode::Toggle);
        talk.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 0);
        assert_eq!(
            talk.on_event(&EngineEvent::CaptureCancelled, 20),
            TalkEffect::Ignore
        );
        assert_eq!(talk.active, Some(HotkeyMode::Dictate));
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::AltRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
    }

    #[test]
    fn toggle_and_hold_and_the_chord_window() {
        let mut toggle = TalkController::new(TalkMode::Toggle);
        assert_eq!(
            toggle.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 0),
            TalkEffect::Start(HotkeyMode::Dictate)
        );
        assert_eq!(
            toggle.on_event(&EngineEvent::Released(HotkeyMode::Dictate), 40),
            TalkEffect::Ignore
        );
        assert_eq!(
            toggle.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 80),
            TalkEffect::Stop
        );

        let mut hold = TalkController::new(TalkMode::Hold);
        assert_eq!(
            hold.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 0),
            TalkEffect::Start(HotkeyMode::Dictate)
        );
        assert_eq!(
            hold.on_event(&EngineEvent::Released(HotkeyMode::Dictate), 400),
            TalkEffect::Stop
        );

        let mut window = TalkController::new(TalkMode::Toggle);
        window.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 1_000);
        assert_eq!(
            window.on_event(&EngineEvent::Pressed(HotkeyMode::Translate), 1_200),
            TalkEffect::CancelThenStart(HotkeyMode::Translate)
        );
        assert_eq!(window.active, Some(HotkeyMode::Translate));

        let mut late = TalkController::new(TalkMode::Toggle);
        late.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 1_000);
        assert_eq!(
            late.on_event(&EngineEvent::Pressed(HotkeyMode::Command), 1_201),
            TalkEffect::Ignore
        );
        assert_eq!(late.active, Some(HotkeyMode::Dictate));

        let mut extra = TalkController::new(TalkMode::Toggle);
        extra.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 0);
        assert_eq!(
            extra.on_event(&EngineEvent::ExtraKeys, 50),
            TalkEffect::Cancel
        );
        extra.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 0);
        assert_eq!(
            extra.on_event(&EngineEvent::ExtraKeys, 500),
            TalkEffect::Ignore
        );
        assert_eq!(
            extra.on_event(&EngineEvent::Escape, 600),
            TalkEffect::Cancel
        );

        let mut translate = TalkController::new(TalkMode::Toggle);
        assert_eq!(
            translate.on_event(&EngineEvent::Pressed(HotkeyMode::Translate), 0),
            TalkEffect::Start(HotkeyMode::Translate)
        );
        assert_eq!(
            translate.on_event(&EngineEvent::Pressed(HotkeyMode::Translate), 400),
            TalkEffect::Stop
        );
        assert_eq!(
            translate.on_event(&EngineEvent::Pressed(HotkeyMode::Command), 500),
            TalkEffect::Start(HotkeyMode::Command)
        );
        assert_eq!(
            translate.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 900),
            TalkEffect::Stop
        );
        assert!(translate.active.is_none());
    }

    #[test]
    fn altgr_synthetic_ctrl_is_ignored_and_real_left_ctrl_is_kept() {
        assert!(is_altgr_synthetic_ctrl(0xA2, 0x021D, 0x01));
        assert!(is_altgr_synthetic_ctrl(0xA2, 0x1D, 0x01));
        assert!(is_altgr_synthetic_ctrl(0x11, 0x021D, 0x01 | 0x80));
        assert!(!is_altgr_synthetic_ctrl(0xA2, 0x1D, 0));
        assert!(!is_altgr_synthetic_ctrl(0xA3, 0x1D, 0x01));
        assert!(!is_altgr_synthetic_ctrl(0xA5, 0x38, 0x01));
        assert!(edge_from_windows(0xA2, 0x021D, 0x01, false).is_none());
        assert_eq!(
            edge_from_windows(0xA2, 0x1D, 0, false),
            Some(KeyEdge::Down(KeyId::ControlLeft))
        );
        assert_eq!(edge_from_code("ControlLeft", true, true), None);
        assert_eq!(
            edge_from_code("AltRight", true, false),
            Some(KeyEdge::Down(KeyId::AltRight))
        );
    }

    #[test]
    fn capture_alt_right_ignores_altgr_ctrl_then_that_chord_starts_dictation() {
        let steps = [
            (false, 0xA2u32, 0x021Du32, 0x01u32),
            (false, 0xA5, 0x38, 0x01),
            (true, 0xA5, 0x38, 0x01 | 0x80),
            (true, 0xA2, 0x021D, 0x01 | 0x80),
        ];
        let mut engine = HotkeyEngine::default();
        engine.set_capture(true);
        let mut last = Vec::new();
        for (up, vk, scan, flags) in steps {
            if let Some(edge) = edge_from_windows(vk, scan, flags, up) {
                last = engine.on_edge(edge);
            }
        }
        assert_eq!(last, vec![EngineEvent::Captured("AltRight".into())]);

        engine.set_bindings(vec![
            (HotkeyMode::Dictate, parse_chord("AltRight").unwrap()),
            (
                HotkeyMode::Translate,
                parse_chord("AltRight+ShiftRight").unwrap(),
            ),
            (HotkeyMode::Command, parse_chord("AltRight+Enter").unwrap()),
        ]);
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::AltRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
        let mut with_enter = HotkeyEngine::default();
        with_enter.set_capture(true);
        assert!(matches!(
            with_enter.on_edge(KeyEdge::Down(KeyId::AltRight))[0],
            EngineEvent::CaptureProgress(_)
        ));
        with_enter.on_edge(KeyEdge::Down(KeyId::Enter));
        with_enter.on_edge(KeyEdge::Up(KeyId::Enter));
        assert_eq!(
            with_enter.on_edge(KeyEdge::Up(KeyId::AltRight)),
            vec![EngineEvent::Captured("AltRight+Enter".into())]
        );
    }

    #[test]
    fn capture_mode_suppresses_matching_and_right_alt_slash_is_the_chord() {
        let mut engine = HotkeyEngine::default();
        engine.set_bindings(vec![
            (HotkeyMode::Dictate, parse_chord("AltRight").unwrap()),
            (
                HotkeyMode::Translate,
                parse_chord("AltRight+ShiftRight").unwrap(),
            ),
            (HotkeyMode::Command, parse_chord("AltRight+Enter").unwrap()),
        ]);
        assert_eq!(
            engine.on_edge(KeyEdge::Down(KeyId::AltRight)),
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
        engine.on_edge(KeyEdge::Up(KeyId::AltRight));

        engine.set_capture(true);
        let alt = engine.consume(KeyEdge::Down(KeyId::AltRight));
        assert!(
            alt.events
                .iter()
                .all(|event| !matches!(event, EngineEvent::Pressed(_))),
            "capture must not start dictation"
        );
        assert_eq!(
            alt.events,
            vec![EngineEvent::CaptureProgress("AltRight".into())]
        );
        assert!(alt.suppress);

        assert!(edge_from_windows(0xA2, 0x021D, 0x01, false).is_none());
        assert_eq!(
            edge_from_windows(0xA5, 0x38, 0x01, false),
            Some(KeyEdge::Down(KeyId::AltRight))
        );
        let slash_down = edge_from_windows(0xBF, 0x35, 0, false).unwrap();
        assert_eq!(slash_down, KeyEdge::Down(KeyId::Slash));
        let slash = engine.consume(slash_down);
        assert_eq!(
            slash.events,
            vec![EngineEvent::CaptureProgress("AltRight+Slash".into())]
        );
        assert!(slash.suppress);
        engine.consume(KeyEdge::Up(KeyId::Slash));
        let done = engine.consume(KeyEdge::Up(KeyId::AltRight));
        assert_eq!(
            done.events,
            vec![EngineEvent::Captured("AltRight+Slash".into())]
        );
        assert!(done.suppress);

        engine.set_bindings(vec![
            (HotkeyMode::Dictate, parse_chord("AltRight+Slash").unwrap()),
            (
                HotkeyMode::Translate,
                parse_chord("AltRight+ShiftRight").unwrap(),
            ),
            (HotkeyMode::Command, parse_chord("AltRight+Space").unwrap()),
        ]);
        assert!(engine
            .consume(KeyEdge::Down(KeyId::AltRight))
            .events
            .is_empty());
        let matched = engine.consume(KeyEdge::Down(KeyId::Slash));
        assert_eq!(
            matched.events,
            vec![EngineEvent::Pressed(HotkeyMode::Dictate)]
        );
        assert!(matched.suppress);
        let released = engine.consume(KeyEdge::Up(KeyId::Slash));
        assert!(released.suppress);
        assert_eq!(
            released.events,
            vec![EngineEvent::Released(HotkeyMode::Dictate)]
        );
    }

    #[test]
    fn a_quiet_hook_is_dead_only_after_the_key_stays_unreported() {
        let mut watch = HookWatch::default();
        assert!(!watch.observe(0, 1_000, false));
        assert!(!watch.observe(0, 1_000, true));
        assert!(watch.observe(0, 1_500, true));
        let mut fresh = HookWatch::default();
        assert!(!fresh.observe(1_400, 1_500, true));
        assert!(!fresh.observe(1_400, 2_000, true));
        assert_eq!(virtual_key(KeyId::AltRight), Some(0xA5));
        assert_eq!(virtual_key(KeyId::Function(8)), Some(0x77));
        let caps = parse_chord("CapsLock").unwrap();
        assert_eq!(canonical(&caps), "Vk20");
        assert_eq!(chord_label(&caps, OsKind::Windows), "Caps Lock");
    }
}
