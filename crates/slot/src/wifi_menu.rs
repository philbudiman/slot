//! Controller-friendly Wi-Fi screen. Passwords are masked and never part of a render cache key.
use crate::wifi::{self, Network, Request, Security, Service};
use slot_input::Btn;
use slot_ui::{UndoFace, OUT_H, OUT_W};
use std::path::Path;
use std::time::{Duration, Instant};

const KEYS: [&str; 3] = [
    "abcdefghijklmnopqrstuvwxyz0123456789-_. ",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_. ",
    "!@#$%^&*()[]{}<>?/\\|;:'\",+=`~_- .0123456",
];

pub struct WifiMenu {
    service: Option<Service>,
    pub networks: Vec<Network>,
    pub saved: Vec<String>,
    connected: Option<String>,
    pub status: String,
    pub row: usize,
    pub busy: bool,
    enabled: bool,
    saved_view: bool,
    editing: Option<Network>,
    password: String,
    key: usize,
    page: usize,
    revision: u64,
    checked: Instant,
}

impl Default for WifiMenu {
    fn default() -> Self {
        Self {
            service: None,
            networks: Vec::new(),
            saved: Vec::new(),
            connected: None,
            status: "Wi-Fi requires BaseOS on the handheld".into(),
            row: 0,
            busy: false,
            enabled: true,
            saved_view: false,
            editing: None,
            password: String::new(),
            key: 0,
            page: 0,
            revision: 1,
            checked: Instant::now(),
        }
    }
}

impl WifiMenu {
    pub fn boot(&mut self, root: &Path) {
        self.enabled = wifi::enabled(root);
        self.service = Service::start(root.to_path_buf());
        if self.service.is_some() {
            self.status = "Press X to scan networks".into();
            if !self.enabled {
                self.request(Request::Disable, "Turning Wi-Fi off...");
            } else if wifi::auto_connect(root) {
                self.request(Request::Reconnect, "Connecting to a saved network...");
            }
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn open(&mut self) {
        self.row = 0;
        self.networks.clear();
        self.saved_view = false;
        self.editing = None;
        self.password.clear();
        if !self.busy && self.enabled {
            self.request(Request::Status, "Checking Wi-Fi status...");
        }
        self.revision += 1;
    }
    fn request(&mut self, request: Request, status: &str) {
        if let Some(service) = &self.service {
            if service.send(request) {
                self.busy = true;
                self.status = status.into();
            } else {
                self.status = "Wi-Fi worker stopped; restart Slot+".into();
            }
        }
        self.revision += 1;
    }
    fn visible_networks(&self) -> impl Iterator<Item = &Network> {
        self.networks
            .iter()
            .filter(|network| Some(network.ssid.as_str()) != self.connected.as_deref())
    }
    fn last_row(&self) -> usize {
        1 + usize::from(self.connected.is_some()) + self.visible_networks().count()
    }
    pub fn poll(&mut self, visible: bool) {
        if let Some(snapshot) = self.service.as_ref().and_then(Service::poll) {
            self.busy = false;
            self.status = snapshot.status;
            self.enabled = snapshot.enabled;
            self.connected = snapshot.connected;
            if !self.enabled {
                self.networks.clear();
            }
            if let Some(networks) = snapshot.networks {
                self.networks = networks;
            }
            self.saved = snapshot.saved;
            self.row = self.row.min(if self.saved_view {
                self.saved.len().saturating_sub(1)
            } else {
                self.last_row()
            });
            self.revision += 1;
            self.checked = Instant::now();
        }
        if visible
            && !self.busy
            && self.editing.is_none()
            && self.status.starts_with("Connected")
            && self.checked.elapsed() > Duration::from_secs(10)
        {
            let status = self.status.clone();
            self.request(Request::Status, &status);
            self.checked = Instant::now();
        }
    }
    /// True means return to settings. A worker may finish while this screen is closed.
    pub fn input(&mut self, button: Btn) -> bool {
        self.revision += 1;
        if button == Btn::B {
            if self.editing.take().is_some() {
                self.password.clear();
                return false;
            }
            if self.saved_view {
                self.saved_view = false;
                self.row = 1;
                return false;
            }
            return true;
        }
        if self.busy {
            return false;
        }
        if let Some(network) = self.editing.clone() {
            match button {
                Btn::Up => self.key = self.key.saturating_sub(10),
                Btn::Down => self.key = (self.key + 10).min(39),
                Btn::Left => self.key = self.key.saturating_sub(1),
                Btn::Right => self.key = (self.key + 1).min(39),
                Btn::Y => self.page = (self.page + 1) % KEYS.len(),
                Btn::X => {
                    self.password.pop();
                }
                Btn::A if self.password.len() < 64 => self
                    .password
                    .push(KEYS[self.page].as_bytes()[self.key] as char),
                Btn::Start => {
                    if let Err(error) = wifi::validate(&network, &self.password) {
                        self.status = error;
                    } else {
                        let password = std::mem::take(&mut self.password);
                        self.editing = None;
                        self.request(
                            Request::Connect(network, password),
                            "Connecting... (up to 45 seconds)",
                        );
                    }
                }
                _ => {}
            }
            return false;
        }
        if self.saved_view {
            match button {
                Btn::Up => self.row = self.row.saturating_sub(1),
                Btn::Down => self.row = (self.row + 1).min(self.saved.len().saturating_sub(1)),
                Btn::A if !self.enabled => self.status = "Turn Wi-Fi on first".into(),
                Btn::A => {
                    if let Some(ssid) = self.saved.get(self.row).cloned() {
                        self.request(Request::ConnectSaved(ssid), "Connecting to saved network...");
                    }
                }
                Btn::X => {
                    if let Some(ssid) = self.saved.get(self.row).cloned() {
                        self.request(Request::ForgetSaved(ssid), "Forgetting saved network...");
                    }
                }
                _ => {}
            }
            return false;
        }
        match button {
            Btn::Up => self.row = self.row.saturating_sub(1),
            Btn::Down => self.row = (self.row + 1).min(self.last_row()),
            Btn::X if !self.enabled => self.status = "Turn Wi-Fi on first".into(),
            Btn::X => self.request(Request::Scan, "Scanning for networks..."),
            Btn::Y if self.row == 2 && self.connected.is_some() => {
                self.request(Request::Disconnect, "Disconnecting...");
            }
            Btn::A if !self.enabled && self.row > 1 => {
                self.status = "Turn Wi-Fi on first".into();
            }
            Btn::A => match self.row {
                0 if self.enabled => self.request(Request::Disable, "Turning Wi-Fi off..."),
                0 => self.request(Request::Enable, "Turning Wi-Fi on..."),
                1 => {
                    self.saved_view = true;
                    self.row = 0;
                }
                2 if self.connected.is_some() => {}
                index => {
                    let network = self
                        .visible_networks()
                        .nth(index - 2 - usize::from(self.connected.is_some()))
                        .cloned();
                    if let Some(network) = network {
                        match network.security {
                            Security::Open => self
                                .request(Request::Connect(network, String::new()), "Connecting..."),
                            Security::Unsupported => {
                                self.status = "This network's security is not supported".into()
                            }
                            Security::Personal => {
                                self.editing = Some(network);
                                self.password.clear();
                                self.key = 0;
                                self.page = 0;
                                self.status = "Enter the Wi-Fi password".into();
                            }
                        }
                    }
                }
            },
            _ => {}
        }
        false
    }

    pub fn face(&self) -> UndoFace {
        let (w, h) = (OUT_W as usize, OUT_H as usize);
        let content_width = OUT_W as i32 - 56;
        let key_pitch = content_width / 10;
        let mut face = UndoFace {
            rgba: vec![0; w * h * 4],
            w: w as u32,
            h: h as u32,
        };
        fill(
            &mut face,
            0,
            0,
            OUT_W as i32,
            OUT_H as i32,
            [20, 21, 25, 255],
        );
        text(
            &mut face,
            if self.saved_view { "Saved networks" } else { "Wi-Fi" },
            28,
            20,
            30.0,
            content_width,
            [240, 240, 244],
        );
        text(
            &mut face,
            &self.status,
            28,
            63,
            18.0,
            content_width,
            [185, 190, 200],
        );
        if let Some(network) = &self.editing {
            text(
                &mut face,
                &network.ssid,
                28,
                99,
                22.0,
                content_width,
                [240, 240, 244],
            );
            let masked = format!(
                "{}  ({} characters)",
                "*".repeat(self.password.len().min(24)),
                self.password.len()
            );
            text(
                &mut face,
                &masked,
                28,
                139,
                19.0,
                content_width,
                [185, 190, 200],
            );
            for (i, key) in KEYS[self.page].chars().enumerate() {
                let x = 28 + (i % 10) as i32 * key_pitch;
                let y = 190 + (i / 10) as i32 * 45;
                if i == self.key {
                    fill(
                        &mut face,
                        x - 3,
                        y - 4,
                        key_pitch - 10,
                        40,
                        [65, 70, 85, 255],
                    );
                }
                let label = if key == ' ' {
                    "SP".to_string()
                } else {
                    key.to_string()
                };
                text(&mut face, &label, x + 8, y, 23.0, 40, [240, 240, 244]);
            }
            text(
                &mut face,
                "A Type   X Erase   Y abc/ABC/123",
                28,
                391,
                19.0,
                content_width,
                [185, 190, 200],
            );
            text(
                &mut face,
                "START Connect   B Cancel",
                28,
                433,
                19.0,
                content_width,
                [240, 240, 244],
            );
        } else {
            let rows = if self.saved_view {
                if self.saved.is_empty() {
                    vec!["No saved networks".into()]
                } else {
                    self.saved.clone()
                }
            } else {
                let mut rows = vec![
                    format!("Wi-Fi: {}", if self.enabled { "On" } else { "Off" }),
                    "Saved networks".into(),
                ];
                if let Some(ssid) = &self.connected {
                    rows.push(format!("{ssid}  [Connected]"));
                }
                rows.extend(self.visible_networks().map(|n| {
                    format!(
                        "{}  [{}]",
                        n.ssid,
                        match n.security {
                            Security::Open => "Open",
                            Security::Personal => "Password",
                            Security::Unsupported => "Unsupported",
                        }
                    )
                }));
                rows
            };
            let first = self.row.saturating_sub(5);
            for (i, label) in rows.iter().enumerate().skip(first).take(6) {
                let y = 105 + (i - first) as i32 * 46;
                if i == self.row && (!self.saved_view || !self.saved.is_empty()) {
                    fill(
                        &mut face,
                        16,
                        y - 3,
                        OUT_W as i32 - 32,
                        41,
                        [65, 70, 85, 255],
                    );
                }
                text(
                    &mut face,
                    label,
                    28,
                    y,
                    22.0,
                    content_width,
                    [240, 240, 244],
                );
            }
            if !self.saved_view {
                text(
                    &mut face,
                    "SFTP: port 22 | user root | your BaseOS password",
                    28,
                    391,
                    18.0,
                    content_width,
                    [185, 190, 200],
                );
            }
            text(
                &mut face,
                if self.busy {
                    "Working...   B Back"
                } else if self.saved_view && self.saved.is_empty() {
                    "B Back"
                } else if self.saved_view {
                    "Up/Down Choose   A Connect   B Back   X Forget"
                } else if self.row == 2 && self.connected.is_some() {
                    "Up/Down Choose   A Select   B Back   X Scan   Y Disconnect"
                } else {
                    "Up/Down Choose   A Select   B Back   X Scan"
                },
                28,
                433,
                19.0,
                content_width,
                [240, 240, 244],
            );
        }
        face
    }
}

pub(crate) fn fill(face: &mut UndoFace, x: i32, y: i32, w: i32, h: i32, colour: [u8; 4]) {
    for yy in y.max(0)..(y + h).min(face.h as i32) {
        for xx in x.max(0)..(x + w).min(face.w as i32) {
            let at = (yy as usize * face.w as usize + xx as usize) * 4;
            face.rgba[at..at + 4].copy_from_slice(&colour);
        }
    }
}

/// Preserve case: SSIDs and keyboard keys are case-sensitive, unlike Slot's menu typography.
pub(crate) fn text(
    face: &mut UndoFace,
    s: &str,
    x: i32,
    y: i32,
    size: f32,
    width: i32,
    ink: [u8; 3],
) {
    let Some(font) = slot_ui::text::label_font() else {
        return;
    };
    let mut pen = x as f32;
    let baseline = y + size as i32;
    for c in s.chars() {
        let (m, bitmap) = font.rasterize(c, size);
        if pen + m.advance_width > (x + width) as f32 {
            break;
        }
        for yy in 0..m.height {
            for xx in 0..m.width {
                let dx = pen as i32 + m.xmin + xx as i32;
                let dy = baseline - m.ymin - m.height as i32 + yy as i32;
                if dx < 0 || dy < 0 || dx >= face.w as i32 || dy >= face.h as i32 {
                    continue;
                }
                let a = bitmap[yy * m.width + xx] as u32;
                let at = (dy as usize * face.w as usize + dx as usize) * 4;
                for (k, channel) in ink.iter().enumerate() {
                    face.rgba[at + k] =
                        ((*channel as u32 * a + face.rgba[at + k] as u32 * (255 - a)) / 255) as u8;
                }
            }
        }
        pen += m.advance_width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_off_choice_does_not_scan_when_the_menu_opens() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("System")).unwrap();
        std::fs::write(root.path().join("System/wifi.disabled"), b"disabled\n").unwrap();
        let mut menu = WifiMenu::default();
        menu.boot(root.path());
        assert!(!menu.enabled);
        menu.open();
        menu.networks.push(Network {
            ssid: "Home".into(),
            signal: -30,
            security: Security::Open,
        });
        menu.row = 2;
        menu.input(Btn::A);
        assert_eq!(menu.status, "Turn Wi-Fi on first");
        menu.input(Btn::X);
        assert_eq!(menu.status, "Turn Wi-Fi on first");
        menu.row = 1;
        menu.input(Btn::A);
        assert!(menu.saved_view);
        assert!(!menu.input(Btn::B));
        assert!(!menu.saved_view);
    }
    #[test]
    fn wifi_face_matches_the_compositor_without_rescaling() {
        let face = WifiMenu::default().face();
        assert_eq!((face.w, face.h), (OUT_W, OUT_H));
        assert_eq!(face.rgba.len(), (OUT_W * OUT_H * 4) as usize);
    }
    #[test]
    fn saved_networks_have_their_own_bounded_list() {
        let mut menu = WifiMenu::default();
        menu.saved = vec!["Cafe".into(), "home".into()];
        menu.row = 1;
        menu.input(Btn::A);
        assert!(menu.saved_view);
        assert_eq!(menu.row, 0);
        menu.input(Btn::Down);
        menu.input(Btn::Down);
        assert_eq!(menu.row, 1);
        assert!(!menu.input(Btn::B));
        assert_eq!(menu.row, 1);
        assert!(!menu.saved_view);
    }
    #[test]
    fn connected_network_stays_above_scan_results_without_a_duplicate() {
        let mut menu = WifiMenu::default();
        menu.connected = Some("Home".into());
        for ssid in ["Cafe", "Home"] {
            menu.networks.push(Network {
                ssid: ssid.into(),
                signal: -30,
                security: Security::Open,
            });
        }
        assert_eq!(
            menu.visible_networks()
                .map(|n| n.ssid.as_str())
                .collect::<Vec<_>>(),
            vec!["Cafe"]
        );
        assert_eq!(menu.last_row(), 3);
        menu.row = 2;
        menu.input(Btn::A);
        assert_eq!(menu.status, "Wi-Fi requires BaseOS on the handheld");
        menu.input(Btn::Down);
        assert_eq!(menu.row, 3);
    }
    #[test]
    fn opening_wifi_keeps_the_connection_but_clears_old_scan_results() {
        let mut menu = WifiMenu::default();
        menu.connected = Some("Home".into());
        menu.networks.push(Network {
            ssid: "Cafe".into(),
            signal: -30,
            security: Security::Open,
        });
        menu.open();
        assert_eq!(menu.connected.as_deref(), Some("Home"));
        assert!(menu.networks.is_empty());
        assert_eq!(menu.last_row(), 2);
    }
    #[test]
    fn keyboard_contains_every_printable_ascii_character() {
        for keys in KEYS {
            assert_eq!(keys.len(), 40);
        }
        for b in 32..=126 {
            assert!(
                KEYS.iter().any(|s| s.as_bytes().contains(&b)),
                "missing {b}"
            );
        }
    }
    #[test]
    fn password_keyboard_masks_input_and_b_cancels_before_leaving() {
        let mut menu = WifiMenu::default();
        menu.networks.push(Network {
            ssid: "Home".into(),
            signal: -30,
            security: Security::Personal,
        });
        menu.row = 2;
        menu.input(Btn::A);
        menu.input(Btn::A);
        assert_eq!(menu.password, "a");
        menu.input(Btn::Y);
        menu.input(Btn::A);
        assert_eq!(menu.password, "aA");
        menu.input(Btn::X);
        assert_eq!(menu.password, "a");
        menu.input(Btn::Start);
        assert!(menu.status.contains("8-63"));
        assert!(!menu.input(Btn::B));
        assert!(menu.password.is_empty());
        assert!(menu.input(Btn::B));
    }
}
