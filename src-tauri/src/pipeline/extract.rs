//! Unpack a verified archive into a model directory.
//!
//! Entries that leave the destination (`..`, absolute paths) are rejected.
//! Symbolic links inside the archive are skipped so a tarball cannot point
//! a later read at an unrelated file.

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
        if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
            return Err(format!("The archive contains a link, which is not unpacked: {raw}"));
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
}
