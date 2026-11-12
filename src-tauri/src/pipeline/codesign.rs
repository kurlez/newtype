//! One signature for every Mach-O in a downloaded runtime tree.
//!
//! The app bundle is signed at build time. `llama-server` and its libraries
//! are downloaded later, outside that bundle, so they need their own
//! signature or macOS quarantines them. Every file in one tree uses the same
//! identity: the stable self-signed name when that private key is in the
//! keychain, otherwise ad-hoc (`-`). The private key is not shipped, so an
//! installed copy ad-hoc signs the download. `--timestamp=none` keeps a
//! self-signed certificate from calling Apple's timestamp server.

use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

pub const AD_HOC: &str = "-";

pub fn is_macho(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    let le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let be = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    matches!(
        le,
        0xFEED_FACE | 0xFEED_FACF | 0xCAFE_BABE | 0xCAFE_BABF
    ) || matches!(be, 0xFEED_FACE | 0xFEED_FACF | 0xCAFE_BABE | 0xCAFE_BABF)
}

/// Libraries first, then executables, so a binary is sealed after its dependencies.
pub fn sign_order(names: &[String]) -> Vec<String> {
    let mut libraries = Vec::new();
    let mut programs = Vec::new();
    for name in names {
        if name.ends_with(".dylib") || name.ends_with(".so") {
            libraries.push(name.clone());
        } else {
            programs.push(name.clone());
        }
    }
    libraries.sort();
    programs.sort();
    libraries.extend(programs);
    libraries
}

pub fn codesign_args(identity: &str, path: &Path) -> Vec<String> {
    vec![
        "--force".into(),
        "--sign".into(),
        identity.to_string(),
        "--timestamp=none".into(),
        path.display().to_string(),
    ]
}

/// Use `requested` only when `security find-identity` listed it. Otherwise ad-hoc.
pub fn choose_identity(requested: &str, listed: &[String]) -> String {
    let requested = requested.trim();
    if requested.is_empty() || requested == AD_HOC {
        return AD_HOC.into();
    }
    if listed.iter().any(|name| name == requested || name.contains(requested)) {
        requested.to_string()
    } else {
        AD_HOC.into()
    }
}

pub fn parse_identity_list(output: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in output.lines() {
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start + 1..].find('"') {
                let name = &line[start + 1..start + 1 + end];
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

pub fn sign_runtime_tree(dir: &Path) {
    #[cfg(target_os = "macos")]
    {
        sign_runtime_tree_macos(dir);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dir;
    }
}

#[cfg(target_os = "macos")]
fn sign_runtime_tree_macos(dir: &Path) {
    let mut files = Vec::new();
    collect_machos(dir, &mut files);
    if files.is_empty() {
        return;
    }
    files.sort_by(|left, right| {
        let left_name = left.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let right_name = right.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let left_lib = left_name.ends_with(".dylib") || left_name.ends_with(".so");
        let right_lib = right_name.ends_with(".dylib") || right_name.ends_with(".so");
        right_lib.cmp(&left_lib).then_with(|| left.cmp(right))
    });
    let identity = choose_identity(compiled_identity(), &keychain_identities());
    crate::diag::record(&format!("signing runtime tree as {identity}"));
    for path in files {
        let _ = Command::new("/usr/bin/xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&path)
            .status();
        let args = codesign_args(&identity, &path);
        let status = Command::new("/usr/bin/codesign").args(&args).status();
        if !status.as_ref().is_ok_and(|status| status.success()) {
            crate::diag::record(&format!("codesign failed for {}", path.display()));
        }
    }
}

#[cfg(target_os = "macos")]
fn collect_machos(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            collect_machos(&path, out);
        } else if kind.is_file() && std::fs::File::open(&path).ok().and_then(|mut file| {
            use std::io::Read;
            let mut magic = [0u8; 4];
            file.read_exact(&mut magic).ok().map(|_| is_macho(&magic))
        }).unwrap_or(false) {
            out.push(path);
        }
    }
}

#[cfg(target_os = "macos")]
fn compiled_identity() -> &'static str {
    option_env!("NEWTYPE_MAC_SIGN_IDENTITY").unwrap_or("")
}

#[cfg(target_os = "macos")]
fn keychain_identities() -> Vec<String> {
    let output = Command::new("/usr/bin/security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    parse_identity_list(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macho_magic_is_recognized_and_other_bytes_are_not() {
        assert!(is_macho(&[0xCF, 0xFA, 0xED, 0xFE, 0, 0]));
        assert!(is_macho(&[0xFE, 0xED, 0xFA, 0xCF]));
        assert!(is_macho(&[0xCA, 0xFE, 0xBA, 0xBE]));
        assert!(!is_macho(b"PK\x03\x04"));
        assert!(!is_macho(&[0xCF, 0xFA]));
    }

    #[test]
    fn libraries_are_signed_before_executables_and_share_one_identity() {
        let names = vec![
            "llama-server".into(),
            "libllama.dylib".into(),
            "libggml-metal.dylib".into(),
        ];
        let order = sign_order(&names);
        assert_eq!(
            order,
            vec![
                "libggml-metal.dylib".to_string(),
                "libllama.dylib".to_string(),
                "llama-server".to_string(),
            ]
        );
        let identity = choose_identity("newtype-self-signed", &["newtype-self-signed".into()]);
        assert_eq!(identity, "newtype-self-signed");
        let args: Vec<Vec<String>> = order
            .iter()
            .map(|name| codesign_args(&identity, Path::new(name)))
            .collect();
        assert!(args.iter().all(|arg| arg[2] == identity && arg.contains(&"--timestamp=none".into())));
        assert_eq!(choose_identity("newtype-self-signed", &[]), AD_HOC);
        assert_eq!(choose_identity("", &["newtype-self-signed".into()]), AD_HOC);
        assert_eq!(choose_identity("-", &["newtype-self-signed".into()]), AD_HOC);
    }

    #[test]
    fn identity_list_parser_reads_quoted_names() {
        let output = "  1) AAAA1111 \"newtype-self-signed\"\n  2) BBBB2222 \"Developer ID Application: Example (TEAM)\"\n     0 valid identities found\n";
        assert_eq!(
            parse_identity_list(output),
            vec![
                "newtype-self-signed".to_string(),
                "Developer ID Application: Example (TEAM)".to_string(),
            ]
        );
    }
}
