use crate::transfer::Server;
use crate::wifi_menu::{fill, text};
use slot_ui::{UndoFace, OUT_H, OUT_W};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

pub struct TransferMenu {
    server: Option<Server>,
    message: String,
    uploaded: u64,
    revision: u64,
    root: Option<PathBuf>,
    build_ready: bool,
    confirm: bool,
    installed: bool,
}
impl Default for TransferMenu {
    fn default() -> Self {
        Self {
            server: None,
            message: String::new(),
            uploaded: 0,
            revision: 1,
            root: None,
            build_ready: false,
            confirm: false,
            installed: false,
        }
    }
}
impl TransferMenu {
    pub fn open(&mut self, root: Option<&Path>, wifi_status: &str) {
        self.stop();
        self.root = root.map(Path::to_path_buf);
        self.installed = false;
        let ip = wifi_status
            .split(" | IP ")
            .nth(1)
            .and_then(|s| s.parse::<Ipv4Addr>().ok());
        self.server = match (root, ip) {
            (Some(root), Some(ip)) => match Server::start(root, ip, 8080) {
                Ok(server) => Some(server),
                Err(_) => {
                    self.message = "Cannot start. Check Wi-Fi, then press A to retry.".into();
                    None
                }
            },
            _ => {
                self.message = "Connect in Settings > Wi-Fi first, then try again.".into();
                None
            }
        };
        self.revision += 1;
        self.poll();
    }
    pub fn running(&self) -> bool {
        self.server.is_some()
    }
    pub fn installed(&self) -> bool {
        self.installed
    }
    pub fn confirming(&self) -> bool {
        self.confirm
    }
    pub fn cancel_confirmation(&mut self) {
        self.confirm = false;
        self.revision += 1;
    }
    pub fn confirm_build(&mut self) {
        if !self.build_ready {
            return;
        }
        if !self.confirm {
            self.confirm = true;
            self.revision += 1;
            return;
        }
        if let Some(server) = self.server.take() {
            server.stop();
        }
        self.confirm = false;
        self.build_ready = false;
        self.message = match self.root.as_deref().map(crate::update::install_local) {
            Some(Ok(())) => {
                self.installed = true;
                "Test build installed. Restarting Slot+...".into()
            }
            Some(Err(message)) => message,
            None => "Slot+ card is unavailable".into(),
        };
        self.revision += 1;
    }
    pub fn stop(&mut self) {
        self.server = None;
        if let Some(root) = &self.root {
            let _ = std::fs::remove_file(root.join("System/slot.upload"));
        }
        self.build_ready = false;
        self.confirm = false;
        self.revision += 1;
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn poll(&mut self) {
        if let Some(server) = &self.server {
            let status = server.status();
            if status.message != self.message
                || status.uploaded != self.uploaded
                || status.build_ready != self.build_ready
            {
                self.message = status.message;
                self.uploaded = status.uploaded;
                self.build_ready = status.build_ready;
                self.revision += 1;
            }
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
            "File Transfer",
            28,
            22,
            30.0,
            width,
            [240, 240, 244],
        );
        if let Some(server) = &self.server {
            text(
                &mut face,
                "Open this address on a device on the same Wi-Fi:",
                28,
                84,
                20.0,
                width,
                [185, 190, 200],
            );
            text(
                &mut face,
                &format!("http://{}/", server.address),
                28,
                125,
                32.0,
                width,
                [240, 240, 244],
            );
            text(
                &mut face,
                "Enter this transfer code:",
                28,
                193,
                20.0,
                width,
                [185, 190, 200],
            );
            text(
                &mut face,
                &server.pin,
                28,
                228,
                44.0,
                width,
                [229, 219, 191],
            );
            text(
                &mut face,
                &format!("{} files uploaded", self.uploaded),
                28,
                312,
                20.0,
                width,
                [185, 190, 200],
            );
            text(
                &mut face,
                if self.confirm {
                    "Replace Slot+ with this test build?"
                } else if self.build_ready {
                    "Test build uploaded"
                } else {
                    "Restart Slot+ after adding games or labels."
                },
                28,
                389,
                19.0,
                width,
                [185, 190, 200],
            );
            text(
                &mut face,
                if self.confirm {
                    "A Confirm   B Cancel"
                } else if self.build_ready {
                    "A Review install   B Back"
                } else {
                    "B Stop & back"
                },
                28,
                433,
                20.0,
                width,
                [240, 240, 244],
            );
        } else {
            text(
                &mut face,
                "A Retry   B Back",
                28,
                433,
                20.0,
                width,
                [240, 240, 244],
            );
        }
        text(
            &mut face,
            &self.message,
            28,
            352,
            19.0,
            width,
            [185, 190, 200],
        );
        face
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_build_needs_a_second_press_on_handheld() {
        let root = tempfile::tempdir().unwrap();
        let system = root.path().join("System");
        fs::create_dir(&system).unwrap();
        fs::write(system.join("slot"), b"current").unwrap();
        let mut binary = vec![0u8; 20];
        binary[..4].copy_from_slice(b"\x7fELF");
        binary[4] = 2;
        binary[5] = 1;
        binary[18..20].copy_from_slice(&[183, 0]);
        fs::write(system.join("slot.upload"), &binary).unwrap();
        let mut menu = TransferMenu::default();
        menu.root = Some(root.path().to_path_buf());
        menu.build_ready = true;
        menu.confirm_build();
        assert!(menu.confirming());
        assert_eq!(fs::read(system.join("slot")).unwrap(), b"current");
        menu.cancel_confirmation();
        assert!(!menu.confirming());
        menu.confirm_build();
        menu.confirm_build();
        assert!(menu.installed());
        assert_eq!(fs::read(system.join("slot")).unwrap(), binary);
    }
}
