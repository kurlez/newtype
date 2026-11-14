use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Dev,
    #[default]
    Stable,
}

impl Channel {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dev" => Some(Self::Dev),
            "stable" => Some(Self::Stable),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Stable => "stable",
        }
    }

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Dev => feeds().dev.as_str(),
            Self::Stable => feeds().stable.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub channel: Channel,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // New installs follow stable. A saved "dev" or "stable" value is
            // loaded as written and is not rewritten.
            channel: Channel::Stable,
        }
    }
}

pub fn load(path: &Path) -> Settings {
    let Ok(bytes) = std::fs::read(path) else {
        return Settings::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = serde_json::to_vec_pretty(settings).map_err(|err| err.to_string())?;
    std::fs::write(path, body).map_err(|err| err.to_string())
}

struct Feeds {
    dev: String,
    stable: String,
}

fn feeds() -> &'static Feeds {
    static FEEDS: std::sync::OnceLock<Feeds> = std::sync::OnceLock::new();
    FEEDS.get_or_init(|| {
        #[derive(Deserialize)]
        struct File {
            channels: Channels,
        }
        #[derive(Deserialize)]
        struct Channels {
            dev: String,
            stable: String,
        }
        let file: File = serde_json::from_str(include_str!("../update-endpoints.json"))
            .expect("update-endpoints.json");
        Feeds {
            dev: file.channels.dev,
            stable: file.channels.stable,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_install_uses_stable_and_a_saved_dev_channel_stays() {
        assert_eq!(Channel::default(), Channel::Stable);
        assert_eq!(Settings::default().channel, Channel::Stable);

        let dir = std::env::temp_dir().join(format!(
            "newtype-settings-channel-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(load(&dir.join("missing.json")).channel, Channel::Stable);

        let saved = dir.join("settings.json");
        std::fs::write(&saved, "{\"channel\":\"dev\"}\n").unwrap();
        assert_eq!(load(&saved).channel, Channel::Dev);

        std::fs::write(&saved, "{\"channel\":\"stable\"}\n").unwrap();
        assert_eq!(load(&saved).channel, Channel::Stable);

        std::fs::write(&saved, "{").unwrap();
        assert_eq!(load(&saved).channel, Channel::Stable);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
