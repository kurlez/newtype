//! Single gate for outbound network.
//!
//! Cloud speech, rewrite, connectivity probes, model downloads, and update
//! checks all ask here before a socket is opened. Loopback traffic to
//! `llama-server` does not come through this module: that client is hard-wired
//! to `127.0.0.1` and must keep working while strict offline is on.
//!
//! The process mode is what the app sets from the saved preference. Tests pin
//! a thread-local override so they do not race the process flag.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetMode {
    Open = 0,
    LocalOnly = 1,
    StrictOffline = 2,
}

impl NetMode {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::LocalOnly,
            2 => Self::StrictOffline,
            _ => Self::Open,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Cloud,
    Probe,
    ModelDownload,
    UpdateCheck,
}

impl Purpose {
    pub fn all() -> [Self; 4] {
        [Self::Cloud, Self::Probe, Self::ModelDownload, Self::UpdateCheck]
    }
}

static MODE: AtomicU8 = AtomicU8::new(NetMode::Open as u8);

thread_local! {
    static OVERRIDE: Cell<Option<NetMode>> = const { Cell::new(None) };
}

pub struct ModeGuard {
    previous: Option<NetMode>,
}

impl ModeGuard {
    pub fn set(mode: NetMode) -> Self {
        let previous = OVERRIDE.with(|slot| slot.replace(Some(mode)));
        Self { previous }
    }

    pub fn strict() -> Self {
        Self::set(NetMode::StrictOffline)
    }

    pub fn local_only() -> Self {
        Self::set(NetMode::LocalOnly)
    }
}

impl Drop for ModeGuard {
    fn drop(&mut self) {
        OVERRIDE.with(|slot| slot.set(self.previous));
    }
}

pub fn set_mode(mode: NetMode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}

pub fn process_mode() -> NetMode {
    NetMode::from_u8(MODE.load(Ordering::Relaxed))
}

pub fn mode() -> NetMode {
    OVERRIDE.with(|slot| slot.get().unwrap_or_else(process_mode))
}

pub fn allowed(mode: NetMode, purpose: Purpose) -> bool {
    match mode {
        NetMode::Open => true,
        NetMode::LocalOnly => matches!(purpose, Purpose::ModelDownload | Purpose::UpdateCheck),
        NetMode::StrictOffline => false,
    }
}

pub fn authorize(purpose: Purpose) -> Result<(), String> {
    let mode = mode();
    if allowed(mode, purpose) {
        Ok(())
    } else {
        Err(denied_message(mode, purpose))
    }
}

pub fn denied_message(mode: NetMode, purpose: Purpose) -> String {
    match mode {
        NetMode::StrictOffline => match purpose {
            Purpose::Cloud => "Strict offline is on. The cloud request was not sent.".into(),
            Purpose::Probe => "Strict offline is on. The connectivity probe was not sent.".into(),
            Purpose::ModelDownload => {
                "Strict offline is on. Model downloads are off until you leave strict offline.".into()
            }
            Purpose::UpdateCheck => "Strict offline is on. Update checks are off.".into(),
        },
        NetMode::LocalOnly => {
            "Local only is on. Cloud calls and connectivity probes stay off.".into()
        }
        NetMode::Open => "The network gate allowed this request.".into(),
    }
}

pub fn blocking_client(
    purpose: Purpose,
    timeout: Duration,
) -> Result<reqwest::blocking::Client, String> {
    blocking_client_with(purpose, |builder| builder.timeout(timeout))
}

pub fn blocking_client_with(
    purpose: Purpose,
    configure: impl FnOnce(reqwest::blocking::ClientBuilder) -> reqwest::blocking::ClientBuilder,
) -> Result<reqwest::blocking::Client, String> {
    authorize(purpose)?;
    configure(reqwest::blocking::Client::builder())
        .build()
        .map_err(|err| format!("Could not build the HTTP client: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_offline_denies_every_outbound_purpose() {
        let _guard = ModeGuard::strict();
        for purpose in Purpose::all() {
            let err = authorize(purpose).unwrap_err();
            assert!(err.contains("Strict offline"), "{purpose:?}: {err}");
            assert!(!allowed(NetMode::StrictOffline, purpose));
        }
    }

    #[test]
    fn local_only_blocks_cloud_and_probes_and_allows_downloads_and_updates() {
        let _guard = ModeGuard::local_only();
        assert!(authorize(Purpose::Cloud).unwrap_err().contains("Local only"));
        assert!(authorize(Purpose::Probe).is_err());
        assert!(authorize(Purpose::ModelDownload).is_ok());
        assert!(authorize(Purpose::UpdateCheck).is_ok());
    }

    #[test]
    fn open_mode_allows_outbound() {
        let _guard = ModeGuard::set(NetMode::Open);
        for purpose in Purpose::all() {
            authorize(purpose).unwrap();
        }
    }

    #[test]
    fn a_thread_local_guard_does_not_stick_after_drop() {
        {
            let _guard = ModeGuard::strict();
            assert!(authorize(Purpose::Cloud).is_err());
        }
        assert_eq!(mode(), process_mode());
        assert!(allowed(NetMode::Open, Purpose::Cloud));
    }
}
