//! Resumable HTTPS download with a SHA-256 check before the file is final.
//!
//! A failed check deletes the partial file and does not create the destination.
//! `http://` is only allowed for loopback, which unit tests use. Model URLs are `https`.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::checksum::{self, sha256_file};

const CHUNK: usize = 64 * 1024;

pub struct FetchProgress {
    pub received: u64,
    pub total: u64,
}

pub fn download_verified(
    urls: &[String],
    dest: &Path,
    expected_sha256: &str,
    expected_size: u64,
    cancel: &AtomicBool,
    mut progress: impl FnMut(FetchProgress),
) -> Result<(), String> {
    if urls.is_empty() {
        return Err("No download URL is available for this file.".into());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if dest.is_file() && checksum::matches_file(dest, expected_sha256).unwrap_or(false) {
        let len = dest.metadata().map(|meta| meta.len()).unwrap_or(expected_size);
        progress(FetchProgress {
            received: len,
            total: expected_size.max(len),
        });
        return Ok(());
    }
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    let mut last_error = "Download did not start.".to_string();
    for url in urls {
        if cancel.load(Ordering::Relaxed) {
            return Err("Download cancelled.".into());
        }
        if let Err(reason) = allowed_url(url) {
            last_error = reason;
            continue;
        }
        match download_one(url, dest, expected_sha256, expected_size, cancel, &mut progress) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = err;
            }
        }
    }
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    Err(last_error)
}

pub fn allowed_url(url: &str) -> Result<(), String> {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    if lower.starts_with("http://127.0.0.1:") || lower.starts_with("http://localhost:") {
        return Ok(());
    }
    Err("Refusing a download URL that is not https (loopback is only for tests).".into())
}

fn download_one(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    expected_size: u64,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(FetchProgress),
) -> Result<(), String> {
    let partial = partial_path(dest);
    if partial.is_file() {
        let len = partial.metadata().map(|meta| meta.len()).unwrap_or(0);
        if expected_size > 0 && len > expected_size {
            let _ = fs::remove_file(&partial);
        }
    }
    let mut attempt_without_range = false;
    for _ in 0..2 {
        if cancel.load(Ordering::Relaxed) {
            return Err("Download cancelled.".into());
        }
        let existing = if attempt_without_range {
            let _ = fs::remove_file(&partial);
            0
        } else {
            partial.metadata().map(|meta| meta.len()).unwrap_or(0)
        };
        if expected_size > 0 && existing == expected_size {
            return finish_partial(&partial, dest, expected_sha256, expected_size, progress);
        }
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(2 * 60 * 60))
            .redirect(reqwest::redirect::Policy::limited(8))
            .user_agent("newtype/0.1 (local-model-download)")
            .build()
            .map_err(|err| err.to_string())?;
        let mut request = client.get(url);
        if existing > 0 {
            request = request.header("Range", format!("bytes={existing}-"));
        }
        let response = request.send().map_err(|err| format!("Download from {} failed: {err}", host_of(url)))?;
        let status = response.status();
        if status.as_u16() == 416 {
            attempt_without_range = true;
            continue;
        }
        if !status.is_success() {
            return Err(format!(
                "Download from {} failed with HTTP {}.",
                host_of(url),
                status.as_u16()
            ));
        }
        let start = if status.as_u16() == 206 {
            existing
        } else {
            if existing > 0 {
                let _ = fs::remove_file(&partial);
            }
            0
        };
        let mut file = if start == 0 {
            File::create(&partial).map_err(|err| err.to_string())?
        } else {
            OpenOptions::new()
                .append(true)
                .open(&partial)
                .map_err(|err| err.to_string())?
        };
        let mut received = start;
        let total = expected_size.max(received);
        progress(FetchProgress { received, total });
        let mut reader = response;
        let mut buf = vec![0u8; CHUNK];
        loop {
            if cancel.load(Ordering::Relaxed) {
                file.flush().ok();
                return Err("Download cancelled.".into());
            }
            let read = reader.read(&mut buf).map_err(|err| format!("Download from {} stalled: {err}", host_of(url)))?;
            if read == 0 {
                break;
            }
            file.write_all(&buf[..read]).map_err(|err| err.to_string())?;
            received += read as u64;
            progress(FetchProgress {
                received,
                total: expected_size.max(received),
            });
        }
        file.flush().map_err(|err| err.to_string())?;
        file.sync_all().ok();
        drop(file);
        return finish_partial(&partial, dest, expected_sha256, expected_size, progress);
    }
    Err(format!("Download from {} could not resume.", host_of(url)))
}

fn finish_partial(
    partial: &Path,
    dest: &Path,
    expected_sha256: &str,
    expected_size: u64,
    progress: &mut impl FnMut(FetchProgress),
) -> Result<(), String> {
    let actual = sha256_file(partial)?;
    let size = partial.metadata().map(|meta| meta.len()).unwrap_or(0);
    if !actual.eq_ignore_ascii_case(expected_sha256.trim()) || (expected_size > 0 && size != expected_size) {
        let _ = fs::remove_file(partial);
        let _ = fs::remove_file(dest);
        return Err(format!(
            "SHA-256 did not match. The file was deleted and is not marked installed. Got {actual}, {size} bytes."
        ));
    }
    fs::rename(partial, dest).map_err(|err| format!("Could not move the verified file into place: {err}"))?;
    progress(FetchProgress {
        received: size,
        total: expected_size.max(size),
    });
    Ok(())
}

pub fn partial_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    dest.with_file_name(format!("{name}.partial"))
}

fn host_of(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("the file host")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    fn serve(routes: Arc<Vec<(String, u16, Vec<u8>)>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for _ in 0..8 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let mut header = Vec::new();
                let mut buf = [0u8; 1];
                loop {
                    if stream.read(&mut buf).ok().filter(|n| *n > 0).is_none() {
                        break;
                    }
                    header.push(buf[0]);
                    if header.ends_with(b"\r\n\r\n") {
                        break;
                    }
                    if header.len() > 8192 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&header);
                let path = text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/")
                    .to_string();
                let range = text.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if name.eq_ignore_ascii_case("range") {
                        value.trim().strip_prefix("bytes=")?.split('-').next()?.parse::<u64>().ok()
                    } else {
                        None
                    }
                });
                let route = routes.iter().find(|(candidate, _, _)| candidate == &path);
                let response = match route {
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                    Some((_, status, _)) if *status >= 400 => {
                        format!("HTTP/1.1 {status} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
                    }
                    Some((_, _, body)) => {
                        let start = range.unwrap_or(0).min(body.len() as u64) as usize;
                        if range.is_some() && start >= body.len() {
                            b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                        } else {
                            let slice = &body[start..];
                            let code = if range.is_some() { 206 } else { 200 };
                            let mut head = format!(
                                "HTTP/1.1 {code} OK\r\nContent-Length: {}\r\nConnection: close\r\n",
                                slice.len()
                            );
                            if range.is_some() {
                                head.push_str(&format!(
                                    "Content-Range: bytes {start}-{}/{}\r\n",
                                    body.len().saturating_sub(1),
                                    body.len()
                                ));
                            }
                            head.push_str("\r\n");
                            let mut bytes = head.into_bytes();
                            bytes.extend_from_slice(slice);
                            bytes
                        }
                    }
                };
                let _ = stream.write_all(&response);
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn checksum_failure_deletes_the_partial_and_does_not_create_the_file() {
        let body = b"not-the-model".to_vec();
        let base = serve(Arc::new(vec![("/file".into(), 200, body)]));
        let dir = std::env::temp_dir().join(format!("newtype-dl-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.bin");
        let err = download_verified(
            &[format!("{base}/file")],
            &dest,
            &checksum::sha256_bytes(b"the-model"),
            b"the-model".len() as u64,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert!(!dest.exists());
        assert!(!partial_path(&dest).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resume_continues_from_the_partial_and_the_hash_must_match() {
        let body = b"abcdefghijklmnopqrstuvwxyz".to_vec();
        let base = serve(Arc::new(vec![("/file".into(), 200, body.clone())]));
        let dir = std::env::temp_dir().join(format!("newtype-dl-resume-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.bin");
        fs::write(partial_path(&dest), &body[..10]).unwrap();
        download_verified(
            &[format!("{base}/file")],
            &dest,
            &checksum::sha256_bytes(&body),
            body.len() as u64,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        assert!(!partial_path(&dest).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failing_mirror_falls_through_to_the_next_url() {
        let body = b"weights".to_vec();
        let base = serve(Arc::new(vec![
            ("/bad".into(), 500, Vec::new()),
            ("/good".into(), 200, body.clone()),
        ]));
        let dir = std::env::temp_dir().join(format!("newtype-dl-mirror-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.bin");
        download_verified(
            &[format!("{base}/bad"), format!("{base}/good")],
            &dest,
            &checksum::sha256_bytes(&body),
            body.len() as u64,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn public_http_urls_are_refused_before_a_connection() {
        let err = allowed_url("http://example.com/model.gguf").unwrap_err();
        assert!(err.contains("https"));
        assert!(allowed_url("https://huggingface.co/org/model/resolve/main/file.gguf").is_ok());
        assert!(allowed_url("http://127.0.0.1:9/file").is_ok());
    }
}
