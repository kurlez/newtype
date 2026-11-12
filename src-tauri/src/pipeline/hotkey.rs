//! Global hotkeys that can tell the left modifier from the right one.
//!
//! Modifier-only chords (Right Ctrl on both desktops) are not OS accelerators.
//! A low-level key listener feeds this engine. Translate and command are
//! registered here; the session treats them as stubs.

use std::collections::BTreeSet;

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "hotkey",
        status: "ready",
        note: "Dictate, translate, and command chords are registered. Translate and command do not record yet.",
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
    /// Escape while a new chord is being recorded. This is not a dictation cancel.
    CaptureCancelled,
}

#[derive(Debug, Clone)]
pub struct HotkeyEngine {
    bindings: Vec<(HotkeyMode, Chord)>,
    down: Chord,
    active: Option<(HotkeyMode, Chord)>,
    extra_sent: bool,
    capturing: bool,
    captured: Chord,
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
        }
    }
}

impl HotkeyEngine {
    pub fn set_bindings(&mut self, bindings: Vec<(HotkeyMode, Chord)>) {
        self.bindings = bindings;
    }

    pub fn set_capture(&mut self, capturing: bool) {
        if capturing && !self.capturing {
            self.down.clear();
            self.captured.clear();
            self.active = None;
            self.extra_sent = false;
        }
        self.capturing = capturing;
    }

    pub fn on_edge(&mut self, edge: KeyEdge) -> Vec<EngineEvent> {
        if self.capturing {
            return self.on_capture(edge);
        }
        match edge {
            KeyEdge::Down(KeyId::Escape) => vec![EngineEvent::Escape],
            KeyEdge::Down(key) => {
                if !self.down.insert(key) {
                    return Vec::new();
                }
                self.promote()
            }
            KeyEdge::Up(key) => {
                self.down.remove(&key);
                if let Some((mode, chord)) = &self.active {
                    if !chord.is_subset(&self.down) {
                        let mode = *mode;
                        self.active = None;
                        self.extra_sent = false;
                        return vec![EngineEvent::Released(mode)];
                    }
                }
                Vec::new()
            }
        }
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
                self.active = None;
                vec![EngineEvent::CaptureCancelled]
            }
            KeyEdge::Down(key) => {
                self.captured.insert(key);
                self.down.insert(key);
                Vec::new()
            }
            KeyEdge::Up(key) => {
                self.down.remove(&key);
                if self.down.is_empty() && !self.captured.is_empty() {
                    let chord = std::mem::take(&mut self.captured);
                    self.capturing = false;
                    vec![EngineEvent::Captured(canonical(&chord))]
                } else {
                    Vec::new()
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkMode {
    Toggle,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkEffect {
    StartDictate,
    Stop,
    Cancel,
    Stub(HotkeyMode),
    CancelThenStub(HotkeyMode),
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
            EngineEvent::Captured(_) | EngineEvent::CaptureCancelled => TalkEffect::Ignore,
        }
    }

    fn on_pressed(&mut self, mode: HotkeyMode, now_ms: u64) -> TalkEffect {
        if let Some(active) = self.active {
            if mode == HotkeyMode::Dictate
                && active == HotkeyMode::Dictate
                && self.talk_mode == TalkMode::Toggle
            {
                self.active = None;
                return TalkEffect::Stop;
            }
            let age = now_ms.saturating_sub(self.started_ms);
            if mode != active && age <= CHORD_WINDOW_MS {
                self.active = None;
                return match mode {
                    HotkeyMode::Dictate => {
                        self.active = Some(HotkeyMode::Dictate);
                        self.started_ms = now_ms;
                        TalkEffect::StartDictate
                    }
                    other => TalkEffect::CancelThenStub(other),
                };
            }
            return TalkEffect::Ignore;
        }
        match mode {
            HotkeyMode::Dictate => {
                self.active = Some(HotkeyMode::Dictate);
                self.started_ms = now_ms;
                TalkEffect::StartDictate
            }
            other => TalkEffect::Stub(other),
        }
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
            let mut chars = other.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                if ch.is_ascii_alphabetic() {
                    return Ok(KeyId::Letter(ch.to_ascii_uppercase() as u8));
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
        KeyId::Other => "Other".into(),
    }
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
        0x41..=0x5A => KeyId::Letter(code as u8),
        0x70..=0x7B => KeyId::Function((code - 0x6F) as u8),
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
        assert!(engine
            .on_edge(KeyEdge::Down(KeyId::ControlRight))
            .is_empty());
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
        assert_eq!(engine.on_edge(KeyEdge::Down(KeyId::AltRight)), Vec::new());
        assert_eq!(engine.on_edge(KeyEdge::Down(KeyId::ShiftRight)), Vec::new());
        assert_eq!(engine.on_edge(KeyEdge::Up(KeyId::ShiftRight)), Vec::new());
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
            TalkEffect::StartDictate
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
            TalkEffect::StartDictate
        );
        assert_eq!(
            hold.on_event(&EngineEvent::Released(HotkeyMode::Dictate), 400),
            TalkEffect::Stop
        );

        let mut window = TalkController::new(TalkMode::Toggle);
        window.on_event(&EngineEvent::Pressed(HotkeyMode::Dictate), 1_000);
        assert_eq!(
            window.on_event(&EngineEvent::Pressed(HotkeyMode::Translate), 1_200),
            TalkEffect::CancelThenStub(HotkeyMode::Translate)
        );
        assert!(window.active.is_none());

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
    }
}
