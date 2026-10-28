// download.rs — fetch speech models from Hugging Face.
//
// Each file is written to `<file>.part` (resumed with an HTTP Range request
// after an interruption), checked against its pinned size + SHA-256, and only
// then renamed into place — a truncated download or a captive-portal error
// page can never be loaded as a model. One download per model at a time; a
// second caller waits for the first instead of fetching twice.
//
// Progress is streamed as the "model-download" event:
//   { id, phase: "downloading"|"verifying"|"done"|"error"|"cancelled",
//     received, total, pct, message }

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::models::{self, ModelSpec};

const RETRIES: usize = 5;

static ACTIVE: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
static CANCELLED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

#[derive(Serialize, Clone)]
pub struct DownloadProgress {
    pub id: &'static str,
    pub phase: &'static str,
    pub received: u64,
    pub total: u64,
    pub pct: f64,
    pub message: String,
}

fn emit(app: &AppHandle, spec: &ModelSpec, phase: &'static str, received: u64, message: &str) {
    let pct = if phase == "done" {
        100.0
    } else {
        received as f64 / spec.size as f64 * 100.0
    };
    let _ = app.emit(
        "model-download",
        DownloadProgress {
            id: spec.id,
            phase,
            received,
            total: spec.size,
            pct,
            message: message.into(),
        },
    );
}

fn set<'a>(
    m: &'a Mutex<BTreeSet<&'static str>>,
) -> std::sync::MutexGuard<'a, BTreeSet<&'static str>> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn models_dir(app_data: &Path) -> PathBuf {
    app_data.join("models")
}

pub fn path_for(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(spec.file)
}

/// Cheap check: the file exists with exactly the pinned size. (The full
/// SHA-256 runs once, right after the download.)
pub fn is_downloaded(dir: &Path, spec: &ModelSpec) -> bool {
    fs::metadata(path_for(dir, spec))
        .map(|m| m.len() == spec.size)
        .unwrap_or(false)
}

pub fn is_downloading(id: &str) -> bool {
    set(&ACTIVE).contains(id)
}

pub fn cancel(id: &str) {
    if let Some(spec) = models::find(id) {
        if is_downloading(spec.id) {
            set(&CANCELLED).insert(spec.id);
        }
    }
}

pub fn delete(dir: &Path, spec: &ModelSpec) -> Result<(), String> {
    for p in [
        path_for(dir, spec),
        path_for(dir, spec).with_extension("bin.part"),
    ] {
        if p.exists() {
            fs::remove_file(&p).map_err(|e| format!("delete {}: {e}", p.display()))?;
        }
    }
    Ok(())
}

/// Make sure `spec` is on disk: return at once if it is, wait for a download
/// already in flight, or download it now. Blocking.
pub fn ensure(app: &AppHandle, dir: &Path, spec: &'static ModelSpec) -> Result<(), String> {
    loop {
        if is_downloaded(dir, spec) {
            return Ok(());
        }
        if set(&ACTIVE).insert(spec.id) {
            break; // we own the download now
        }
        std::thread::sleep(Duration::from_millis(500)); // someone else is on it
    }
    set(&CANCELLED).remove(spec.id);
    let result = fs::create_dir_all(dir)
        .map_err(|e| format!("create {}: {e}", dir.display()))
        .and_then(|()| download_verified(app, dir, spec));
    set(&ACTIVE).remove(spec.id);
    let cancelled = set(&CANCELLED).remove(spec.id);
    match &result {
        Err(_) if cancelled => emit(app, spec, "cancelled", 0, "Download cancelled"),
        Err(e) => emit(app, spec, "error", 0, e),
        Ok(()) => {}
    }
    result
}

fn download_verified(app: &AppHandle, dir: &Path, spec: &ModelSpec) -> Result<(), String> {
    let dest = path_for(dir, spec);
    let part = dest.with_extension("bin.part");
    let mut last_err = String::new();
    for attempt in 1..=RETRIES {
        match download_to(app, spec, &part) {
            Ok(()) => {
                last_err.clear();
                break;
            }
            Err(e) if e == CANCELLED_MSG => return Err(e),
            Err(e) => {
                log::warn!("{} download attempt {attempt}/{RETRIES}: {e}", spec.id);
                last_err = e;
                std::thread::sleep(Duration::from_secs(2 * attempt as u64));
            }
        }
    }
    if !last_err.is_empty() {
        return Err(format!("couldn't download {}: {last_err}", spec.name));
    }

    emit(app, spec, "verifying", spec.size, "Checking the download…");
    let digest = sha256_file(&part)?;
    if digest != spec.sha256 {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "{} failed its checksum — deleted, try again",
            spec.name
        ));
    }
    fs::rename(&part, &dest).map_err(|e| format!("finalize {}: {e}", spec.file))?;
    log::info!("downloaded {}", spec.file);
    emit(app, spec, "done", spec.size, "Ready");
    Ok(())
}

const CANCELLED_MSG: &str = "cancelled";

/// Download (or resume) into `part` until it holds the full file.
fn download_to(app: &AppHandle, spec: &ModelSpec, part: &Path) -> Result<(), String> {
    let mut have = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if have > spec.size {
        fs::remove_file(part).map_err(|e| format!("reset {}: {e}", part.display()))?;
        have = 0;
    }
    if have == spec.size {
        return Ok(());
    }

    // no overall cap (slow lines are fine), but a 60 s stall aborts the
    // attempt so a dead connection never hangs forever
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .build();
    let mut req = agent.get(&models::url(spec));
    if have > 0 {
        req = req.set("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| format!("GET {}: {e}", spec.file))?;
    let mut out = match resp.status() {
        206 => OpenOptions::new()
            .append(true)
            .open(part)
            .map_err(|e| format!("open {}: {e}", part.display()))?,
        200 => {
            have = 0; // server ignored the Range header: start over
            File::create(part).map_err(|e| format!("create {}: {e}", part.display()))?
        }
        s => return Err(format!("unexpected HTTP status {s}")),
    };

    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    loop {
        if set(&CANCELLED).contains(spec.id) {
            return Err(CANCELLED_MSG.into()); // keep the .part for a later resume
        }
        let n = reader.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| format!("write: {e}"))?;
        have += n as u64;
        if have > spec.size {
            drop(out);
            let _ = fs::remove_file(part);
            return Err("server sent more data than expected".into());
        }
        if last_emit.elapsed() >= Duration::from_millis(200) {
            last_emit = Instant::now();
            emit(app, spec, "downloading", have, "Downloading");
        }
    }
    out.flush().map_err(|e| format!("flush: {e}"))?;
    if have != spec.size {
        return Err(format!(
            "connection closed early ({have}/{} bytes)",
            spec.size
        ));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
