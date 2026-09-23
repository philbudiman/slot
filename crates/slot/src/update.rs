//! Update the Slot executable from the latest published main release.
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use serde_json::Value;
use slot_input::Millis;
use slot_ui::{UndoFace, OUT_H, OUT_W};

use crate::wifi_menu::{fill, text};

const API: &str = "https://api.github.com/repos/philbudiman/slot-plus/releases/latest";
const ASSET: &str = "slot-h700";
pub(crate) const MAX_SIZE: u64 = 128 * 1024 * 1024;

#[derive(Clone)]
struct Release {
    tag: String,
    notes: Vec<String>,
    asset: Option<Asset>,
}

#[derive(Clone)]
struct Asset {
    digest: String,
    size: u64,
}

enum ResultMessage {
    Checked(Release, bool),
    Installed,
}

enum State {
    Checking,
    Available(Release),
    Downloading,
    UpToDate(Release),
    Installed,
    Error(String),
}

pub struct UpdateMenu {
    state: State,
    worker: Option<Receiver<Result<ResultMessage, String>>>,
    revision: u64,
    scroll: usize,
    hold: Option<Hold>,
}

#[derive(Clone, Copy)]
struct Hold {
    down: bool,
    since: Millis,
    next: Millis,
}

impl Default for UpdateMenu {
    fn default() -> Self {
        Self {
            state: State::Checking,
            worker: None,
            revision: 1,
            scroll: 0,
            hold: None,
        }
    }
}

impl UpdateMenu {
    pub fn open(&mut self, root: Option<&Path>) {
        self.revision += 1;
        self.state = State::Checking;
        self.scroll = 0;
        self.hold = None;
        if root.is_none() || !cfg!(feature = "device") {
            self.state = State::Error("Updates require Slot+ on BaseOS or AGS-102".into());
            return;
        }
        self.worker = Some(spawn(|| check(crate::build_info::Build::current().hash)));
    }

    pub fn confirm(&mut self, root: Option<&Path>) {
        let State::Available(release) = &self.state else {
            return;
        };
        if release.asset.is_none() {
            return;
        }
        let Some(root) = root else { return };
        let release = release.clone();
        let root = root.to_path_buf();
        self.state = State::Downloading;
        self.hold = None;
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
            Ok(ResultMessage::Checked(release, false)) => State::Available(release),
            Ok(ResultMessage::Checked(release, true)) => State::UpToDate(release),
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

    pub fn scroll(&mut self, down: bool) {
        let notes = match &self.state {
            State::Available(release) | State::UpToDate(release) => &release.notes,
            _ => return,
        };
        let next = if down {
            (self.scroll + 1).min(notes.len().saturating_sub(10))
        } else {
            self.scroll.saturating_sub(1)
        };
        if next != self.scroll {
            self.scroll = next;
            self.revision += 1;
        }
    }

    pub fn press(&mut self, down: bool, now: Millis) {
        if self.hold.is_some_and(|hold| hold.down == down) {
            return;
        }
        self.scroll(down);
        self.hold = Some(Hold {
            down,
            since: now,
            next: now + 350,
        });
    }

    pub fn release(&mut self, down: bool) {
        if self.hold.is_some_and(|hold| hold.down == down) {
            self.hold = None;
        }
    }

    pub fn tick(&mut self, now: Millis) {
        let Some(mut hold) = self.hold else { return };
        if now >= hold.next {
            self.scroll(hold.down);
            let elapsed = now.saturating_sub(hold.since);
            let interval = 200u64.saturating_sub(elapsed / 20).max(50);
            hold.next = now + interval;
            self.hold = Some(hold);
        }
    }

    pub fn jump(&mut self, bottom: bool) {
        let notes = match &self.state {
            State::Available(release) | State::UpToDate(release) => &release.notes,
            _ => return,
        };
        let next = if bottom {
            notes.len().saturating_sub(10)
        } else {
            0
        };
        if self.scroll != next {
            self.scroll = next;
            self.revision += 1;
        }
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
            "Update Slot+",
            28,
            22,
            30.0,
            width,
            [240, 240, 244],
        );
        let (message, hint, release) = match &self.state {
            State::Checking => ("Checking the latest release...".into(), "B Back", None),
            State::Available(release) => (
                if release.asset.is_some() {
                    format!("{} is available", release.tag)
                } else {
                    "Release has no Slot+ binary".into()
                },
                if release.asset.is_some() {
                    "Up/Down Notes   A Install   B Back"
                } else {
                    "Up/Down Notes   B Back"
                },
                Some(release),
            ),
            State::Downloading => ("Downloading and verifying...".into(), "Please wait", None),
            State::UpToDate(release) => (
                "Slot+ is up to date".into(),
                "Up/Down Notes   B Back",
                Some(release),
            ),
            State::Installed => (
                "Update installed. Restarting Slot+...".into(),
                "Please wait",
                None,
            ),
            State::Error(message) => (message.clone(), "A Retry   B Back", None),
        };
        text(&mut face, &message, 28, 78, 25.0, width, [240, 240, 244]);
        if let Some(release) = release {
            text(
                &mut face,
                "RELEASE NOTES",
                28,
                124,
                20.0,
                width,
                [185, 190, 200],
            );
            for (row, line) in release.notes.iter().skip(self.scroll).take(10).enumerate() {
                text(
                    &mut face,
                    line,
                    28,
                    156 + row as i32 * 25,
                    19.0,
                    width,
                    [240, 240, 244],
                );
            }
            if release.notes.len() > 10 {
                let track = 245;
                let thumb = (track * 10 / release.notes.len() as i32).max(20);
                let travel = track - thumb;
                let position = travel * self.scroll as i32 / (release.notes.len() as i32 - 10);
                fill(
                    &mut face,
                    OUT_W as i32 - 20,
                    156,
                    8,
                    track,
                    [65, 70, 85, 255],
                );
                fill(
                    &mut face,
                    OUT_W as i32 - 20,
                    156 + position,
                    8,
                    thumb,
                    [229, 219, 191, 255],
                );
            }
        }
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
    let current = release.tag[5..].starts_with(current_hash);
    Ok(ResultMessage::Checked(release, current))
}

fn parse_release(json: &[u8]) -> Result<Release, String> {
    let value: Value =
        serde_json::from_slice(json).map_err(|_| "Invalid release data".to_string())?;
    let tag = value["tag_name"].as_str().ok_or("Release tag missing")?;
    let hash = tag.strip_prefix("main-").ok_or("Unexpected release tag")?;
    if hash.len() != 12 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Unexpected release tag".into());
    }
    let notes = wrap_notes(value["body"].as_str().unwrap_or(""));
    let asset = value["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"] == ASSET))
        .map(|asset| -> Result<Asset, String> {
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
            Ok(Asset {
                digest: digest.to_ascii_lowercase(),
                size,
            })
        })
        .transpose()?;
    Ok(Release {
        tag: tag.into(),
        notes,
        asset,
    })
}

fn wrap_notes(body: &str) -> Vec<String> {
    let Some(font) = slot_ui::text::label_font() else {
        return vec!["No release notes".into()];
    };
    let mut lines = Vec::new();
    for raw in body.lines() {
        let line = raw.trim().trim_start_matches('#').trim().replace('`', "");
        if line.is_empty() {
            if !lines.is_empty() && lines.last().is_some_and(|s: &String| !s.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.extend(
                slot_ui::text::fit(font, &line, (OUT_W - 56) as f32, usize::MAX, 19.0, 19.0).lines,
            );
        }
    }
    if lines.is_empty() {
        lines.push("No release notes".into());
    }
    lines
}

fn install(root: &Path, release: &Release) -> Result<ResultMessage, String> {
    let asset = release.asset.as_ref().ok_or("Release has no Slot+ binary")?;
    let system = root.join("System");
    let target = system.join("slot");
    let backup = system.join("slot.previous");
    let pending = system.join("slot.download");
    let url = format!(
        "https://github.com/philbudiman/slot-plus/releases/download/{}/{}",
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
        verify(&pending, asset)?;
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
    if fs::symlink_metadata(backup).is_ok_and(|m| !m.is_file()) {
        return Err("Slot+ backup is not a regular file".into());
    }
    let permissions = fs::metadata(target)
        .map_err(|_| "Slot+ binary missing".to_string())?
        .permissions();
    fs::set_permissions(pending, permissions).map_err(|_| "Could not make Slot+ executable")?;
    fs::copy(target, backup).map_err(|_| "Could not back up Slot+".to_string())?;
    File::open(backup)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Could not save backup".to_string())?;
    fs::rename(pending, target).map_err(|_| "Could not install update".to_string())
}

fn verify(path: &Path, asset: &Asset) -> Result<(), String> {
    let mut file = File::open(path).map_err(|_| "Downloaded binary missing")?;
    if file
        .metadata()
        .map_err(|_| "Downloaded binary missing")?
        .len()
        != asset.size
    {
        return Err("Downloaded binary size differs".into());
    }
    verify_header(&mut file)?;
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|_| "Checksum tool unavailable".to_string())?;
    if !output.status.success() || !output.stdout.starts_with(asset.digest.as_bytes()) {
        return Err("Downloaded binary checksum differs".into());
    }
    file.sync_all()
        .map_err(|_| "Could not save download".to_string())?;
    Ok(())
}

fn verify_header(file: &mut File) -> Result<(), String> {
    let mut header = [0u8; 20];
    file.read_exact(&mut header)
        .map_err(|_| "Downloaded binary is incomplete")?;
    if &header[..4] != b"\x7fELF" || header[4] != 2 || header[5] != 1 || header[18..20] != [183, 0]
    {
        return Err("Downloaded binary is not an AArch64 Slot+ build".into());
    }
    Ok(())
}

pub(crate) fn install_local(root: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|_| "Slot+ card is unavailable")?;
    let system = root.join("System");
    if system.canonicalize().ok().as_deref() != Some(system.as_path()) {
        return Err("Slot+ System folder is unavailable".into());
    }
    let target = system.join("slot");
    let pending = system.join("slot.upload");
    if !fs::symlink_metadata(&target).is_ok_and(|m| m.is_file())
        || !fs::symlink_metadata(&pending).is_ok_and(|m| m.is_file())
    {
        return Err("Test build or Slot+ binary missing".into());
    }
    verify_local(&pending)?;
    activate(&target, &system.join("slot.previous"), &pending)
}

pub(crate) fn verify_local(pending: &Path) -> Result<(), String> {
    let mut file = File::open(pending).map_err(|_| "Test build missing")?;
    let len = file.metadata().map_err(|_| "Test build missing")?.len();
    if len == 0 || len > MAX_SIZE {
        return Err("Test build size is invalid".into());
    }
    verify_header(&mut file)?;
    file.sync_all()
        .map_err(|_| "Could not save test build".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_requires_the_expected_asset_and_digest() {
        let data = br#"{"tag_name":"main-012345abcdef","assets":[{"name":"slot-h700","size":20,"digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#;
        assert_eq!(parse_release(data).unwrap().tag, "main-012345abcdef");
        let without_asset = parse_release(br###"{"tag_name":"main-012345abcdef","body":"## Changes\nFixed save bug","assets":[]}"###).unwrap();
        assert!(without_asset.asset.is_none());
        assert!(without_asset
            .notes
            .iter()
            .any(|line| line.contains("FIXED SAVE BUG")));
        assert!(parse_release(br#"{"tag_name":"other-012345abcdef","assets":[]}"#).is_err());
    }

    #[test]
    fn activation_keeps_a_bootable_binary_and_backup() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("slot");
        let previous = dir.path().join("slot.previous");
        let pending = dir.path().join("slot.download");
        fs::write(&old, b"old").unwrap();
        fs::set_permissions(&old, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&pending, b"new").unwrap();
        activate(&old, &previous, &pending).unwrap();
        assert_eq!(fs::read(&old).unwrap(), b"new");
        assert_eq!(fs::read(&previous).unwrap(), b"old");
        assert_eq!(
            fs::metadata(&old).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!pending.exists());
    }

    #[test]
    fn local_install_rejects_invalid_build_and_preserves_current_slot() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("System");
        fs::create_dir(&system).unwrap();
        fs::write(system.join("slot"), b"current").unwrap();
        fs::write(system.join("slot.upload"), b"bad").unwrap();
        assert!(install_local(dir.path()).is_err());
        assert_eq!(fs::read(system.join("slot")).unwrap(), b"current");
        assert!(!system.join("slot.previous").exists());
    }

    #[test]
    fn local_install_replaces_slot_after_validation() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("System");
        fs::create_dir(&system).unwrap();
        fs::write(system.join("slot"), b"current").unwrap();
        let mut binary = vec![0u8; 20];
        binary[..4].copy_from_slice(b"\x7fELF");
        binary[4] = 2;
        binary[5] = 1;
        binary[18..20].copy_from_slice(&[183, 0]);
        fs::write(system.join("slot.upload"), &binary).unwrap();
        install_local(dir.path()).unwrap();
        assert_eq!(fs::read(system.join("slot")).unwrap(), binary);
        assert_eq!(fs::read(system.join("slot.previous")).unwrap(), b"current");
    }

    #[test]
    fn notes_scroll_to_the_last_visible_line() {
        let mut menu = UpdateMenu::default();
        menu.state = State::Available(Release {
            tag: "main-012345abcdef".into(),
            notes: (0..15).map(|n| format!("line {n}")).collect(),
            asset: None,
        });
        for _ in 0..20 {
            menu.scroll(true);
        }
        assert_eq!(menu.scroll, 5);
        menu.scroll(false);
        assert_eq!(menu.scroll, 4);
    }

    #[test]
    fn notes_hold_accelerates_and_left_right_jump() {
        let mut menu = UpdateMenu::default();
        menu.state = State::Available(Release {
            tag: "main-012345abcdef".into(),
            notes: (0..30).map(|n| format!("line {n}")).collect(),
            asset: None,
        });
        menu.press(true, 0);
        assert_eq!(menu.scroll, 1);
        menu.tick(349);
        assert_eq!(menu.scroll, 1);
        menu.tick(350);
        assert_eq!(menu.scroll, 2);
        let first_interval = menu.hold.unwrap().next - 350;
        menu.tick(2_000);
        assert!(menu.hold.unwrap().next - 2_000 < first_interval);
        menu.release(true);
        let stopped = menu.scroll;
        menu.tick(3_000);
        assert_eq!(menu.scroll, stopped);
        menu.jump(true);
        assert_eq!(menu.scroll, 20);
        menu.jump(false);
        assert_eq!(menu.scroll, 0);
    }
}
