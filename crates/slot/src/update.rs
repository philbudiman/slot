//! Update the Slot executable from the latest published main release.
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use serde_json::Value;
use slot_ui::{UndoFace, OUT_H, OUT_W};

use crate::wifi_menu::{fill, text};

const API: &str = "https://api.github.com/repos/philbudiman/slot/releases/latest";
const ASSET: &str = "slot-h700";
const MAX_SIZE: u64 = 128 * 1024 * 1024;

#[derive(Clone)]
struct Release {
    tag: String,
    digest: String,
    size: u64,
}

enum ResultMessage {
    Checked(Option<Release>),
    Installed,
}

enum State {
    Checking,
    Available(Release),
    Downloading,
    UpToDate,
    Installed,
    Error(String),
}

pub struct UpdateMenu {
    state: State,
    worker: Option<Receiver<Result<ResultMessage, String>>>,
    revision: u64,
}

impl Default for UpdateMenu {
    fn default() -> Self {
        Self {
            state: State::Checking,
            worker: None,
            revision: 1,
        }
    }
}

impl UpdateMenu {
    pub fn open(&mut self, root: Option<&Path>) {
        self.revision += 1;
        self.state = State::Checking;
        if root.is_none() || !cfg!(feature = "device") {
            self.state = State::Error("Updates require Slot on BaseOS".into());
            return;
        }
        self.worker = Some(spawn(|| check(crate::build_info::Build::current().hash)));
    }

    pub fn confirm(&mut self, root: Option<&Path>) {
        let State::Available(release) = &self.state else {
            return;
        };
        let Some(root) = root else { return };
        let release = release.clone();
        let root = root.to_path_buf();
        self.state = State::Downloading;
        self.revision += 1;
        self.worker = Some(spawn(move || install(&root, &release)));
    }

    pub fn poll(&mut self) {
        let Some(worker) = &self.worker else { return };
        let result = match worker.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Update worker stopped".into()),
        };
        self.worker = None;
        self.state = match result {
            Ok(ResultMessage::Checked(Some(release))) => State::Available(release),
            Ok(ResultMessage::Checked(None)) => State::UpToDate,
            Ok(ResultMessage::Installed) => State::Installed,
            Err(error) => State::Error(error),
        };
        self.revision += 1;
    }

    pub fn busy(&self) -> bool {
        matches!(self.state, State::Checking | State::Downloading)
    }

    pub fn downloading(&self) -> bool {
        matches!(self.state, State::Downloading)
    }

    pub fn failed(&self) -> bool {
        matches!(self.state, State::Error(_))
    }

    pub fn installed(&self) -> bool {
        matches!(self.state, State::Installed)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn face(&self) -> UndoFace {
        let mut face = UndoFace {
            rgba: vec![0; (OUT_W * OUT_H * 4) as usize],
            w: OUT_W,
            h: OUT_H,
        };
        fill(
            &mut face,
            0,
            0,
            OUT_W as i32,
            OUT_H as i32,
            [20, 21, 25, 255],
        );
        let width = OUT_W as i32 - 56;
        text(
            &mut face,
            "Update Slot",
            28,
            22,
            30.0,
            width,
            [240, 240, 244],
        );
        let (message, hint) = match &self.state {
            State::Checking => ("Checking the latest release...".into(), "B Back"),
            State::Available(release) => (
                format!("{} is available", release.tag),
                "A Install   B Back",
            ),
            State::Downloading => ("Downloading and verifying...".into(), "Please wait"),
            State::UpToDate => ("Slot is up to date".into(), "B Back"),
            State::Installed => ("Update installed. Restarting Slot...".into(), "Please wait"),
            State::Error(message) => (message.clone(), "A Retry   B Back"),
        };
        text(&mut face, &message, 28, 140, 25.0, width, [240, 240, 244]);
        text(&mut face, hint, 28, 433, 20.0, width, [240, 240, 244]);
        face
    }
}

fn spawn(
    f: impl FnOnce() -> Result<ResultMessage, String> + Send + 'static,
) -> Receiver<Result<ResultMessage, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx
}

fn check(current_hash: &str) -> Result<ResultMessage, String> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "20",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            API,
        ])
        .output()
        .map_err(|_| "BaseOS curl is unavailable".to_string())?;
    if !output.status.success() {
        return Err("Could not reach GitHub. Check Wi-Fi.".into());
    }
    let release = parse_release(&output.stdout)?;
    if release.tag[5..].starts_with(current_hash) {
        Ok(ResultMessage::Checked(None))
    } else {
        Ok(ResultMessage::Checked(Some(release)))
    }
}

fn parse_release(json: &[u8]) -> Result<Release, String> {
    let value: Value =
        serde_json::from_slice(json).map_err(|_| "Invalid release data".to_string())?;
    let tag = value["tag_name"].as_str().ok_or("Release tag missing")?;
    let hash = tag.strip_prefix("main-").ok_or("Unexpected release tag")?;
    if hash.len() != 12 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Unexpected release tag".into());
    }
    let asset = value["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"] == ASSET))
        .ok_or("Release has no Slot binary")?;
    let size = asset["size"].as_u64().ok_or("Binary size missing")?;
    if size == 0 || size > MAX_SIZE {
        return Err("Binary size is invalid".into());
    }
    let digest = asset["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("Binary checksum missing")?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Binary checksum is invalid".into());
    }
    Ok(Release {
        tag: tag.into(),
        digest: digest.to_ascii_lowercase(),
        size,
    })
}

fn install(root: &Path, release: &Release) -> Result<ResultMessage, String> {
    let system = root.join("System");
    let target = system.join("slot");
    let backup = system.join("slot.previous");
    let pending = system.join("slot.download");
    let url = format!(
        "https://github.com/philbudiman/slot/releases/download/{}/{}",
        release.tag, ASSET
    );
    let result = (|| {
        let status = Command::new("curl")
            .args([
                "-fsSL",
                "--max-time",
                "180",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "-o",
            ])
            .arg(&pending)
            .arg(&url)
            .status()
            .map_err(|_| "BaseOS curl is unavailable".to_string())?;
        if !status.success() {
            return Err("Download failed. Check Wi-Fi.".into());
        }
        verify(&pending, release)?;
        // A copy keeps System/slot in place if the final rename fails.
        activate(&target, &backup, &pending)?;
        Ok(ResultMessage::Installed)
    })();
    if result.is_err() {
        let _ = fs::remove_file(pending);
    }
    result
}

fn activate(target: &Path, backup: &Path, pending: &Path) -> Result<(), String> {
    fs::copy(target, backup).map_err(|_| "Could not back up Slot".to_string())?;
    File::open(backup)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Could not save backup".to_string())?;
    fs::rename(pending, target).map_err(|_| "Could not install update".to_string())
}

fn verify(path: &Path, release: &Release) -> Result<(), String> {
    let mut file = File::open(path).map_err(|_| "Downloaded binary missing")?;
    if file
        .metadata()
        .map_err(|_| "Downloaded binary missing")?
        .len()
        != release.size
    {
        return Err("Downloaded binary size differs".into());
    }
    let mut header = [0u8; 20];
    file.read_exact(&mut header)
        .map_err(|_| "Downloaded binary is incomplete")?;
    if &header[..4] != b"\x7fELF" || header[4] != 2 || header[5] != 1 || header[18..20] != [183, 0]
    {
        return Err("Downloaded binary is not AArch64 Slot".into());
    }
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|_| "Checksum tool unavailable".to_string())?;
    if !output.status.success() || !output.stdout.starts_with(release.digest.as_bytes()) {
        return Err("Downloaded binary checksum differs".into());
    }
    file.sync_all()
        .map_err(|_| "Could not save download".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_requires_the_expected_asset_and_digest() {
        let data = br#"{"tag_name":"main-012345abcdef","assets":[{"name":"slot-h700","size":20,"digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#;
        assert_eq!(parse_release(data).unwrap().tag, "main-012345abcdef");
        assert!(parse_release(br#"{"tag_name":"main-012345abcdef","assets":[]}"#).is_err());
        assert!(parse_release(br#"{"tag_name":"other-012345abcdef","assets":[]}"#).is_err());
    }

    #[test]
    fn activation_keeps_a_bootable_binary_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("slot");
        let previous = dir.path().join("slot.previous");
        let pending = dir.path().join("slot.download");
        fs::write(&old, b"old").unwrap();
        fs::write(&pending, b"new").unwrap();
        activate(&old, &previous, &pending).unwrap();
        assert_eq!(fs::read(&old).unwrap(), b"new");
        assert_eq!(fs::read(&previous).unwrap(), b"old");
        assert!(!pending.exists());
    }
}
