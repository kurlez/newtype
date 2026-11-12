//! API keys live in the macOS Keychain or Windows Credential Manager.
//! A Linux dev file exists only so the settings window can run here. It is not
//! used on the two shipping platforms.

use std::path::Path;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use std::collections::BTreeMap;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "macos", target_os = "windows"))]
const SERVICE: &str = "app.newtype.desktop";

pub fn set_key(provider: &str, key: &str, dev_path: &Path) -> Result<(), String> {
    let account = account(provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err("The API key is empty.".into());
    }
    if key.chars().any(|ch| ch == '\n' || ch == '\r') {
        return Err("The API key cannot contain a line break.".into());
    }
    store(&account, key, dev_path)?;
    match load(&account, dev_path)? {
        Some(found) if found == key => Ok(()),
        _ => {
            let _ = remove(&account, dev_path);
            Err(format!(
                "The key was not kept by {store}. Save again after this build is updated; an in-memory store reports success and then forgets the value.",
                store = store_name()
            ))
        }
    }
}

/// Write, read, and delete a throwaway value. This fails when the crate is
/// linked against keyring's in-memory mock instead of the OS store.
pub fn probe(dev_path: &Path) -> Result<(), String> {
    static PROBES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let account = format!(
        "probe-{}-{}",
        std::process::id(),
        PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let nonce = format!("ok-{account}");
    store(&account, &nonce, dev_path)?;
    let found = load(&account, dev_path);
    let _ = remove(&account, dev_path);
    match found? {
        Some(value) if value == nonce => Ok(()),
        _ => Err(format!(
            "Credential store check failed. {store} did not return a value that was just written. On macOS the build needs the keyring apple-native feature; on Windows it needs windows-native. Without those, keyring keeps the secret in process memory only.",
            store = store_name()
        )),
    }
}

pub fn store_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "macos") {
        "the macOS Keychain"
    } else {
        "the local dev file"
    }
}

pub fn get_key(provider: &str, dev_path: &Path) -> Result<Option<String>, String> {
    let account = account(provider)?;
    load(&account, dev_path)
}

pub fn clear_key(provider: &str, dev_path: &Path) -> Result<(), String> {
    let account = account(provider)?;
    remove(&account, dev_path)
}

pub fn has_key(provider: &str, dev_path: &Path) -> bool {
    get_key(provider, dev_path)
        .ok()
        .flatten()
        .is_some_and(|key| !key.is_empty())
}

fn account(provider: &str) -> Result<String, String> {
    if provider.is_empty()
        || provider.len() > 32
        || !provider
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("Unknown provider.".into());
    }
    Ok(format!("provider:{provider}"))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn store(account: &str, key: &str, _dev_path: &Path) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, account).map_err(|err| err.to_string())?;
    entry.set_password(key).map_err(|err| err.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn load(account: &str, _dev_path: &Path) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, account).map_err(|err| err.to_string())?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn remove(account: &str, _dev_path: &Path) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, account).map_err(|err| err.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn store(account: &str, key: &str, dev_path: &Path) -> Result<(), String> {
    let mut file = read_dev(dev_path);
    file.keys.insert(account.to_string(), key.to_string());
    write_dev(dev_path, &file)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn load(account: &str, dev_path: &Path) -> Result<Option<String>, String> {
    Ok(read_dev(dev_path).keys.get(account).cloned())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn remove(account: &str, dev_path: &Path) -> Result<(), String> {
    let mut file = read_dev(dev_path);
    file.keys.remove(account);
    write_dev(dev_path, &file)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(Default, Serialize, Deserialize)]
struct DevSecrets {
    keys: BTreeMap<String, String>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_dev(path: &Path) -> DevSecrets {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_dev(path: &Path, file: &DevSecrets) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = serde_json::to_vec_pretty(file).map_err(|err| err.to_string())?;
    std::fs::write(path, body).map_err(|err| err.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_store_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "newtype-secrets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        probe(&dir.join("dev-secrets.json")).expect("credential store round trip");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
