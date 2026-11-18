//! Unpack a verified archive into a model directory.
//!
//! Entries that leave the destination (`..`, absolute paths) are rejected.
//! A symbolic link is created only when its target also stays inside the
//! destination. The llama.cpp macOS archive uses relative links for versioned
//! libraries; rejecting those links aborted the unpack.

use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Component, Path, PathBuf};

use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use tar::Archive;

pub fn safe_join(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains('\0') {
        return Err("The archive contains an empty path.".into());
    }
    let relative = Path::new(name);
    if relative.is_absolute() {
        return Err(format!("The archive path is absolute: {name}"));
    }
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("The archive path leaves the model directory: {name}"));
    }
    Ok(root.join(relative))
}

/// Unpack `archive` into a staging directory, then move the result into `dest`.
/// A failure deletes the staging directory and leaves `dest` without those files.
pub fn extract_archive_atomic(archive: &Path, dest: &Path) -> Result<(), String> {
    let staging = dest.join(".unpack-staging");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|err| format!("Could not remove an old unpack directory: {err}"))?;
    }
    fs::create_dir_all(&staging).map_err(|err| err.to_string())?;
    if let Err(err) = extract_archive(archive, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }
    if let Err(err) = promote_staging(&staging, dest) {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

fn promote_staging(staging: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|err| err.to_string())?;
    let entries = fs::read_dir(staging).map_err(|err| err.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if to.exists() {
            if to.is_dir() {
                fs::remove_dir_all(&to).map_err(|err| err.to_string())?;
            } else {
                fs::remove_file(&to).map_err(|err| err.to_string())?;
            }
        }
        fs::rename(&from, &to).map_err(|err| {
            format!(
                "Could not move unpacked {} into place: {err}",
                to.display()
            )
        })?;
    }
    Ok(())
}

pub fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    let name = archive
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    fs::create_dir_all(dest).map_err(|err| err.to_string())?;
    if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        extract_tar(BzDecoder::new(BufReader::new(File::open(archive).map_err(|err| err.to_string())?)), dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar(GzDecoder::new(BufReader::new(File::open(archive).map_err(|err| err.to_string())?)), dest)
    } else if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else {
        Err(format!("Unsupported archive {name}."))
    }
}

fn extract_tar(decoder: impl std::io::Read, dest: &Path) -> Result<(), String> {
    let mut tar = Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|err| format!("Could not read the archive: {err}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|err| format!("Could not read an archive entry: {err}"))?;
        let raw = entry
            .path()
            .map_err(|err| format!("Archive entry path: {err}"))?
            .to_string_lossy()
            .into_owned();
        if raw.is_empty() || raw == "." || raw == "./" {
            continue;
        }
        let target = safe_join(dest, raw.trim_start_matches("./"))?;
        if entry.header().entry_type().is_hard_link() {
            return Err(format!("The archive contains a hard link, which is not unpacked: {raw}"));
        }
        if entry.header().entry_type().is_symlink() {
            let link_name = entry
                .link_name()
                .map_err(|err| format!("Archive link target: {err}"))?
                .ok_or_else(|| format!("The archive link has no target: {raw}"))?;
            let link_name = link_name.to_string_lossy().into_owned();
            place_symlink(dest, &target, &link_name, &raw)?;
            continue;
        }
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target).map_err(|err| err.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut outfile = File::create(&target).map_err(|err| format!("Could not create {}: {err}", target.display()))?;
        io::copy(&mut entry, &mut outfile).map_err(|err| format!("Could not unpack {raw}: {err}"))?;
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|err| err.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| format!("Could not read the zip archive: {err}"))?;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|err| format!("Could not read a zip entry: {err}"))?;
        let Some(enclosed) = entry.enclosed_name() else {
            return Err("The zip archive contains a path that leaves the model directory.".into());
        };
        let raw = enclosed.to_string_lossy().into_owned();
        let target = safe_join(dest, &raw)?;
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|err| err.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut outfile = File::create(&target).map_err(|err| err.to_string())?;
        io::copy(&mut entry, &mut outfile).map_err(|err| format!("Could not unpack {raw}: {err}"))?;
    }
    Ok(())
}

fn place_symlink(root: &Path, link_path: &Path, link_name: &str, entry_name: &str) -> Result<(), String> {
    if link_name.is_empty() || link_name.contains('\0') || Path::new(link_name).is_absolute() {
        return Err(format!("The archive link target is not relative: {entry_name}"));
    }
    let parent = link_path.parent().unwrap_or(root);
    let resolved = parent.join(link_name);
    if !stays_inside(root, &resolved) {
        return Err(format!(
            "The archive link leaves the model directory: {entry_name} -> {link_name}"
        ));
    }
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if link_path.exists() || link_path.symlink_metadata().is_ok() {
        let _ = fs::remove_file(link_path);
    }
    create_symlink(link_name, link_path)
}

fn stays_inside(root: &Path, candidate: &Path) -> bool {
    let mut acc = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match acc.file_name() {
                Some(_) => {
                    acc.pop();
                }
                None => return false,
            },
            Component::Normal(part) => acc.push(part),
            Component::RootDir | Component::Prefix(_) => acc.push(component.as_os_str()),
        }
    }
    acc.starts_with(root)
}

fn create_symlink(link_name: &str, link_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(link_name, link_path).map_err(|err| {
            format!("Could not create link {}: {err}", link_path.display())
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (link_name, link_path);
        Err("This archive contains a symbolic link, which is not unpacked on Windows.".into())
    }
}

pub fn find_named(root: &Path, name: &str) -> Option<PathBuf> {
    let mut found = None;
    visit(root, &mut |path, is_dir| {
        if found.is_some() {
            return;
        }
        if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            if is_dir || path.is_file() {
                found = Some(path.to_path_buf());
            }
        }
    });
    found
}

fn visit(root: &Path, visit_one: &mut dyn FnMut(&Path, bool)) {
    let Ok(entries) = fs::read_dir(root) else {
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
            visit_one(&path, true);
            visit(&path, visit_one);
        } else if kind.is_file() {
            visit_one(&path, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("newtype-extract-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn safe_join_rejects_parent_and_absolute_paths() {
        let root = Path::new("/models/qwen");
        assert!(safe_join(root, "encoder.int8.onnx").unwrap().ends_with("encoder.int8.onnx"));
        assert!(safe_join(root, "tokenizer/vocab.json").is_ok());
        assert!(safe_join(root, "../secret").is_err());
        assert!(safe_join(root, "/etc/passwd").is_err());
        assert!(safe_join(root, "dir/../../outside").is_err());
    }

    #[test]
    fn zip_with_a_parent_path_is_rejected_and_a_normal_zip_unpacks() {
        let dir = scratch("zip");
        let good = dir.join("good.zip");
        {
            let mut writer = zip::ZipWriter::new(File::create(&good).unwrap());
            writer
                .start_file("hello.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"hi").unwrap();
            writer.finish().unwrap();
        }
        let dest = dir.join("out");
        extract_archive(&good, &dest).unwrap();
        assert_eq!(fs::read(dest.join("hello.txt")).unwrap(), b"hi");

        let bad = dir.join("bad.zip");
        {
            let mut writer = zip::ZipWriter::new(File::create(&bad).unwrap());
            writer
                .start_file("../evil.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"nope").unwrap();
            writer.finish().unwrap();
        }
        let err = extract_archive(&bad, &dir.join("bad-out")).unwrap_err();
        assert!(err.contains("leaves") || err.contains("path"), "{err}");
        assert!(!dir.join("evil.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_relative_library_symlink_is_unpacked_and_an_escaping_link_is_rejected() {
        let dir = scratch("symlink");
        let archive = dir.join("llama.tar.gz");
        {
            let file = File::create(&archive).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_path("llama/libllama-server-impl.dylib").unwrap();
            header.set_size(4);
            header.set_cksum();
            builder.append(&header, &b"dylib"[..]).unwrap();
            let mut link = tar::Header::new_gnu();
            link.set_entry_type(tar::EntryType::Symlink);
            link.set_path("llama/libllama.0.dylib").unwrap();
            link.set_link_name("libllama.0.9.0.dylib").unwrap();
            link.set_size(0);
            link.set_cksum();
            builder.append(&link, std::io::empty()).unwrap();
            let mut outside = tar::Header::new_gnu();
            outside.set_entry_type(tar::EntryType::Symlink);
            outside.set_path("llama/escape").unwrap();
            outside.set_link_name("../../outside").unwrap();
            outside.set_size(0);
            outside.set_cksum();
            builder.append(&outside, std::io::empty()).unwrap();
            builder.finish().unwrap();
        }
        let only_safe = dir.join("safe.tar.gz");
        {
            let file = File::create(&only_safe).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_path("llama/llama-server").unwrap();
            header.set_size(3);
            header.set_cksum();
            builder.append(&header, &b"bin"[..]).unwrap();
            let mut link = tar::Header::new_gnu();
            link.set_entry_type(tar::EntryType::Symlink);
            link.set_path("llama/libllama.dylib").unwrap();
            link.set_link_name("libllama.0.dylib").unwrap();
            link.set_size(0);
            link.set_cksum();
            builder.append(&link, std::io::empty()).unwrap();
            builder.finish().unwrap();
        }
        let dest = dir.join("out");
        extract_archive_atomic(&only_safe, &dest).unwrap();
        let link = dest.join("llama/libllama.dylib");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("libllama.0.dylib"));
        assert!(find_named(&dest, "llama-server").is_some());
        assert!(!dest.join(".unpack-staging").exists());
        let err = extract_archive_atomic(&archive, &dir.join("bad")).unwrap_err();
        assert!(err.contains("leaves"), "{err}");
        assert!(!dir.join("bad").join(".unpack-staging").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
