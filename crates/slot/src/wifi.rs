//! BaseOS home Wi-Fi. All system work runs on one worker, never on the frame loop.
//! We own a separate supplicant/control socket and never stop another frontend's daemon.
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

const CONTROL: &str = "/run/slot-wifi";
const RUNTIME: &str = "/run/slot-wifi.conf";
const PROFILE: &str = "System/wifi.conf";
const DISABLED: &str = "System/wifi.disabled";
const DHCP_PID: &str = "/run/slot-wifi-dhcp.pid";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    Personal,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    pub signal: i32,
    pub security: Security,
}

pub struct Snapshot {
    pub networks: Option<Vec<Network>>,
    pub saved: Vec<String>,
    pub connected: Option<String>,
    pub status: String,
    pub enabled: bool,
}

// Intentionally not Debug: Connect carries a password.
pub enum Request {
    Status,
    Enable,
    Scan,
    Connect(Network, String),
    Reconnect,
    ConnectSaved(String),
    Disconnect,
    Disable,
    ForgetSaved(String),
}

struct SavedProfile {
    ssid: String,
    block: String,
}

pub struct Service {
    tx: Sender<Request>,
    rx: Receiver<Snapshot>,
}

impl Service {
    pub fn start(root: PathBuf) -> Option<Self> {
        if !cfg!(feature = "device") || !Path::new("/usr/sbin/baseos-config").exists() {
            return None;
        }
        let (tx, requests) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        thread::spawn(move || {
            let boot_guard = Instant::now() + Duration::from_secs(20);
            loop {
                let request = if !enabled(&root) && Instant::now() < boot_guard {
                    match requests.recv_timeout(Duration::from_secs(1)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // BaseOS can finish its delayed driver retry after Slot starts.
                            if !other_wifi_active() {
                                let _ = block_radio();
                            }
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    let Ok(request) = requests.recv() else { break };
                    request
                };
                let mut result = handle(&root, request).unwrap_or_else(|status| Snapshot {
                    networks: None,
                    saved: Vec::new(),
                    connected: None,
                    status,
                    enabled: enabled(&root),
                });
                result.connected = if result.enabled { current_ssid() } else { None };
                match load_profiles(&root) {
                    Ok(profiles) => result.saved = sorted_names(&profiles),
                    Err(error) => result.status = error,
                }
                if results.send(result).is_err() {
                    break;
                }
            }
        });
        Some(Self { tx, rx })
    }
    pub fn send(&self, request: Request) -> bool {
        self.tx.send(request).is_ok()
    }
    pub fn poll(&self) -> Option<Snapshot> {
        self.rx.try_recv().ok()
    }
}

pub fn saved(root: &Path) -> bool {
    root.join(PROFILE).is_file()
}
pub fn enabled(root: &Path) -> bool {
    !root.join(DISABLED).exists()
}
pub fn auto_connect(root: &Path) -> bool {
    saved(root) && enabled(root)
}

fn load_profiles(root: &Path) -> Result<Vec<SavedProfile>, String> {
    let config = match fs::read_to_string(root.join(PROFILE)) {
        Ok(config) => config,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Cannot read saved networks".into()),
    };
    parse_profiles(&config)
}

fn parse_profiles(config: &str) -> Result<Vec<SavedProfile>, String> {
    let header = format!("ctrl_interface={CONTROL}\nupdate_config=0\n");
    let mut rest = config
        .strip_prefix(&header)
        .ok_or("Cannot read saved networks")?;
    let mut profiles: Vec<SavedProfile> = Vec::new();
    while !rest.is_empty() {
        let body = rest
            .strip_prefix("network={\n")
            .ok_or("Cannot read saved networks")?;
        let (fields, next) = body.split_once("}\n").ok_or("Cannot read saved networks")?;
        let hex = fields
            .lines()
            .find_map(|line| line.trim().strip_prefix("ssid="))
            .ok_or("Cannot read saved networks")?;
        if hex.is_empty() || hex.len() > 64 || hex.len() % 2 != 0 {
            return Err("Cannot read saved networks".into());
        }
        let bytes: Result<Vec<u8>, _> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or(""), 16))
            .collect();
        let ssid = String::from_utf8(bytes.map_err(|_| "Cannot read saved networks")?)
            .map_err(|_| "Cannot read saved networks")?;
        if ssid.chars().any(char::is_control) || profiles.iter().any(|p| p.ssid == ssid) {
            return Err("Cannot read saved networks".into());
        }
        profiles.push(SavedProfile {
            ssid,
            block: format!("network={{\n{fields}}}\n"),
        });
        rest = next;
    }
    Ok(profiles)
}

fn render_profiles(profiles: &[SavedProfile]) -> String {
    let mut config = format!("ctrl_interface={CONTROL}\nupdate_config=0\n");
    for profile in profiles {
        config.push_str(&profile.block);
    }
    config
}

fn sorted_names(profiles: &[SavedProfile]) -> Vec<String> {
    let mut names: Vec<_> = profiles.iter().map(|p| p.ssid.clone()).collect();
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    names
}

fn add_profile(root: &Path, network: &Network, profile: &str) -> Result<String, String> {
    let mut profiles = load_profiles(root)?;
    profiles.retain(|p| p.ssid != network.ssid);
    profiles.push(SavedProfile {
        ssid: network.ssid.clone(),
        block: profile
            .strip_prefix(&format!("ctrl_interface={CONTROL}\nupdate_config=0\n"))
            .unwrap()
            .to_string(),
    });
    Ok(render_profiles(&profiles))
}

fn forget_profile(root: &Path, ssid: &str) -> Result<Vec<SavedProfile>, String> {
    let mut profiles = load_profiles(root)?;
    let old_len = profiles.len();
    profiles.retain(|p| p.ssid != ssid);
    if profiles.len() == old_len {
        return Err("Saved network not found".into());
    }
    if profiles.is_empty() {
        remove(&root.join(PROFILE))?;
    } else {
        private_write(&root.join(PROFILE), render_profiles(&profiles).as_bytes())?;
    }
    Ok(profiles)
}

fn command(program: &str, args: &[&str], seconds: u64) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| format!("BaseOS is missing {program}"))?;
    // Drain stdout while waiting, so a long scan cannot fill a pipe and deadlock.
    let stdout = child.stdout.take().unwrap();
    let (output, read) = mpsc::channel();
    thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stdout.take(262_144).read_to_end(&mut data);
        let _ = output.send(data);
    });
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Helpers that daemonise must close stdout; all commands here do so.
                let data = read
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap_or_default();
                return if status.success() {
                    Ok(String::from_utf8_lossy(&data).into_owned())
                } else {
                    Err(format!("{program} failed; try restarting the device"))
                };
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} timed out"));
            }
        }
    }
}

fn cli(args: &[&str]) -> Result<String, String> {
    let mut all = vec!["-p", CONTROL, "-i", "wlan0"];
    all.extend_from_slice(args);
    command("wpa_cli", &all, 3)
}

fn ok(args: &[&str]) -> Result<(), String> {
    if cli(args)?.lines().any(|s| s == "OK") {
        Ok(())
    } else {
        Err("Wi-Fi did not accept the request; try again".into())
    }
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let temp = path.with_extension("new");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|_| "Cannot write Wi-Fi settings".to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temp, path))
        .map_err(|_| "Cannot save Wi-Fi settings".to_string())
}

fn ensure() -> Result<(), String> {
    if other_wifi_active() {
        return Err("Another Wi-Fi service is active; end multiplayer or restart".into());
    }
    // BaseOS brings up its driver asynchronously; leave Slot responsive during the wait.
    let deadline = Instant::now() + Duration::from_secs(25);
    while !Path::new("/sys/class/net/wlan0").exists() {
        if Instant::now() >= deadline {
            return Err("Wi-Fi radio unavailable; restart and try again".into());
        }
        thread::sleep(Duration::from_millis(250));
    }
    command("rfkill", &["unblock", "wifi"], 3)?;
    command("ip", &["link", "set", "wlan0", "up"], 3)?;
    if cli(&["ping"]).is_ok_and(|s| s.trim() == "PONG") {
        return Ok(());
    }
    // Refuse to compete with an existing home or multiplayer connection.
    if other_wifi_active() {
        return Err("Another Wi-Fi service is active; end multiplayer or restart".into());
    }
    fs::create_dir_all(CONTROL).map_err(|_| "Cannot start Wi-Fi".to_string())?;
    private_write(
        Path::new(RUNTIME),
        format!("ctrl_interface={CONTROL}\nupdate_config=0\n").as_bytes(),
    )?;
    command(
        "wpa_supplicant",
        &["-B", "-i", "wlan0", "-D", "nl80211,wext", "-c", RUNTIME],
        5,
    )?;
    for _ in 0..20 {
        if cli(&["ping"]).is_ok_and(|s| s.trim() == "PONG") {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("Could not start Wi-Fi".into())
}

fn other_wifi_active() -> bool {
    command("pidof", &["hostapd"], 2).is_ok()
        || (command("pidof", &["wpa_supplicant"], 2).is_ok()
            && !cli(&["ping"]).is_ok_and(|s| s.trim() == "PONG"))
}

fn block_radio() -> Result<(), String> {
    if Path::new("/sys/class/net/wlan0").exists() {
        let _ = command("ip", &["link", "set", "wlan0", "down"], 3);
    }
    command("rfkill", &["block", "wifi"], 3)?;
    Ok(())
}

fn handle(root: &Path, request: Request) -> Result<Snapshot, String> {
    if !enabled(root)
        && matches!(
            &request,
            Request::Scan
                | Request::Connect(_, _)
                | Request::Reconnect
                | Request::ConnectSaved(_)
                | Request::Disconnect
        )
    {
        return Err("Turn Wi-Fi on first".into());
    }
    match request {
        Request::Status => Ok(Snapshot {
            networks: None,
            saved: Vec::new(),
            connected: None,
            status: if !enabled(root) {
                "Wi-Fi off".into()
            } else if cli(&["ping"]).is_ok_and(|s| s.trim() == "PONG") {
                status()?
            } else {
                "Press X to scan networks".into()
            },
            enabled: enabled(root),
        }),
        Request::Enable => {
            if let Err(error) = ensure() {
                if !other_wifi_active() {
                    let _ = block_radio();
                }
                return Err(error);
            }
            remove(&root.join(DISABLED))?;
            Ok(Snapshot {
                networks: None,
                saved: Vec::new(),
                connected: None,
                status: status()?,
                enabled: true,
            })
        }
        Request::Scan => {
            ensure()?;
            ok(&["scan"])?;
            thread::sleep(Duration::from_secs(3));
            Ok(Snapshot {
                networks: Some(parse_scan(&cli(&["scan_results"])?)),
                saved: Vec::new(),
                connected: None,
                status: status()?,
                enabled: true,
            })
        }
        Request::Connect(network, password) => {
            let profile = profile(&network, &password)?;
            let saved_config = add_profile(root, &network, &profile)?;
            ensure()?;
            let status = connect_preserving_current(&profile)?;
            private_write(&root.join(PROFILE), saved_config.as_bytes())?;
            Ok(Snapshot {
                networks: None,
                saved: Vec::new(),
                connected: None,
                status,
                enabled: true,
            })
        }
        Request::Reconnect => {
            let profiles = load_profiles(root)?;
            if profiles.is_empty() {
                return Err("No saved networks; select one below".into());
            }
            ensure()?;
            let status = connect(&render_profiles(&profiles))?;
            Ok(Snapshot {
                networks: None,
                saved: Vec::new(),
                connected: None,
                status,
                enabled: true,
            })
        }
        Request::ConnectSaved(ssid) => {
            let profiles = load_profiles(root)?;
            let profile = profiles
                .iter()
                .find(|p| p.ssid == ssid)
                .ok_or("Saved network not found")?;
            ensure()?;
            let status = connect_preserving_current(&format!(
                "ctrl_interface={CONTROL}\nupdate_config=0\n{}",
                profile.block
            ))?;
            Ok(Snapshot {
                networks: None,
                saved: Vec::new(),
                connected: None,
                status,
                enabled: true,
            })
        }
        Request::ForgetSaved(ssid) => {
            let profiles = forget_profile(root, &ssid)?;
            let mut message = format!("Forgot {ssid}");
            if enabled(root) && cli(&["ping"]).is_ok_and(|s| s.trim() == "PONG") {
                let active = current_ssid();
                let config = render_profiles(&profiles);
                if profiles.is_empty() || active.is_none() {
                    clear_runtime_network()?;
                    message.push_str("; not connected");
                } else if active.as_deref() == Some(ssid.as_str()) {
                    message = connect(&config).unwrap_or_else(|_| {
                        let _ = clear_runtime_network();
                        format!("Forgot {ssid}; select another network to connect")
                    });
                } else {
                    private_write(Path::new(RUNTIME), config.as_bytes())?;
                    ok(&["reconfigure"])?;
                    if current_ssid() != active {
                        message = connect(&config).unwrap_or_else(|_| {
                            let _ = clear_runtime_network();
                            format!("Forgot {ssid}; select another network to connect")
                        });
                    }
                }
            }
            Ok(Snapshot {
                networks: None,
                saved: Vec::new(),
                connected: None,
                status: message,
                enabled: enabled(root),
            })
        }
        Request::Disconnect => {
            if cli(&["ping"]).is_ok_and(|s| s.trim() == "PONG") {
                clear_runtime_network()?;
            }
            Ok(Snapshot {
                networks: None,
                saved: Vec::new(),
                connected: None,
                status: "Not connected".into(),
                enabled: true,
            })
        }
        Request::Disable => {
            // Only change wlan0 if it belongs to this service.
            if other_wifi_active() {
                return Err("Another Wi-Fi service is active; end multiplayer first".into());
            }
            private_write(&root.join(DISABLED), b"disabled\n")?;
            if cli(&["ping"]).is_ok_and(|s| s.trim() == "PONG") {
                let _ = ok(&["disconnect"]);
            }
            // Cleanup is best effort; blocking the radio is the required off action.
            let _ = stop_dhcp();
            if Path::new("/sys/class/net/wlan0").exists() {
                let _ = command("ip", &["addr", "flush", "dev", "wlan0"], 3);
            }
            block_radio()?;
            Ok(Snapshot {
                networks: None,
                saved: Vec::new(),
                connected: None,
                status: "Wi-Fi off".into(),
                enabled: false,
            })
        }
    }
}

fn remove(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Cannot update Wi-Fi settings".into()),
    }
}

fn connect_preserving_current(profile: &str) -> Result<String, String> {
    let previous = fs::read(RUNTIME).unwrap_or_default();
    let was_connected = status()?.starts_with("Connected");
    match connect(profile) {
        Ok(status) => Ok(status),
        Err(error) => {
            // Failed joins must not replace a working connection or its saved credentials.
            if was_connected && previous.windows(8).any(|w| w == b"network=") {
                let _ = connect(&String::from_utf8_lossy(&previous));
            } else {
                let _ = private_write(Path::new(RUNTIME), &previous);
                let _ = ok(&["reconfigure"]);
                let _ = ok(&["disconnect"]);
            }
            Err(error)
        }
    }
}

fn current_ssid() -> Option<String> {
    let response = cli(&["status"]).ok()?;
    if !response.lines().any(|line| line == "wpa_state=COMPLETED") {
        return None;
    }
    response
        .lines()
        .find_map(|line| line.strip_prefix("ssid="))
        .map(str::to_string)
}

fn clear_runtime_network() -> Result<(), String> {
    stop_dhcp()?;
    let _ = ok(&["disconnect"]);
    let _ = command("ip", &["addr", "flush", "dev", "wlan0"], 3);
    private_write(
        Path::new(RUNTIME),
        format!("ctrl_interface={CONTROL}\nupdate_config=0\n").as_bytes(),
    )?;
    ok(&["reconfigure"])?;
    ok(&["disconnect"])
}

fn connect(profile: &str) -> Result<String, String> {
    stop_dhcp()?;
    command("ip", &["addr", "flush", "dev", "wlan0"], 3)?;
    private_write(Path::new(RUNTIME), profile.as_bytes())?;
    ok(&["reconfigure"])?;
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if cli(&["status"])?
            .lines()
            .any(|s| s == "wpa_state=COMPLETED")
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err("Could not join; check password and signal".into());
        }
        thread::sleep(Duration::from_millis(500));
    }
    // Keep the DHCP client alive to renew leases; -f lets us reap it after it exits.
    // Its private PID file lets a restarted Slot identify only its own client.
    let mut client = Command::new("udhcpc")
        .args([
            "-f",
            "-i",
            "wlan0",
            "-p",
            DHCP_PID,
            "-s",
            "/usr/share/udhcpc/default.script",
            "-t",
            "5",
            "-T",
            "2",
            "-A",
            "5",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Could not start DHCP".to_string())?;
    thread::spawn(move || {
        let _ = client.wait();
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let current = status()?;
        if current.contains(" | IP ") {
            return Ok(current);
        }
        if Instant::now() >= deadline {
            stop_dhcp()?;
            return Err("Joined Wi-Fi, but no IP address; try reconnecting".into());
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn stop_dhcp() -> Result<(), String> {
    let Ok(pid) = fs::read_to_string(DHCP_PID) else {
        return Ok(());
    };
    let Ok(pid) = pid.trim().parse::<u32>() else {
        return Err("Invalid Wi-Fi DHCP state; restart device".into());
    };
    let process = format!("/proc/{pid}/cmdline");
    let Ok(cmdline) = fs::read(&process) else {
        let _ = fs::remove_file(DHCP_PID);
        return Ok(());
    };
    let parts: Vec<_> = cmdline.split(|b| *b == 0).collect();
    if pid <= 1
        || !parts.first().is_some_and(|p| p.ends_with(b"udhcpc"))
        || !parts
            .windows(2)
            .any(|p| p == [b"-p".as_slice(), DHCP_PID.as_bytes()])
    {
        return Err("Wi-Fi DHCP state changed; restart device".into());
    }
    command("kill", &["-TERM", &pid.to_string()], 2)?;
    for _ in 0..30 {
        if !Path::new(&process).exists() {
            let _ = fs::remove_file(DHCP_PID);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("DHCP is still stopping; try again".into())
}

fn status() -> Result<String, String> {
    let response = cli(&["status"])?;
    let value = |key: &str| response.lines().find_map(|l| l.strip_prefix(key));
    if value("wpa_state=") != Some("COMPLETED") {
        return Ok("Not connected".into());
    }
    Ok(match value("ip_address=") {
        Some(ip) => format!("Connected | IP {ip}"),
        None => "Connected; no IP address yet".into(),
    })
}

pub fn validate(network: &Network, password: &str) -> Result<(), String> {
    if network.ssid.is_empty() || network.ssid.len() > 32 {
        return Err("Invalid network name".into());
    }
    match network.security {
        Security::Unsupported => {
            Err("Only open and WPA/WPA2 personal networks are supported".into())
        }
        Security::Personal
            if !(password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()))
                && !((8..=63).contains(&password.len())
                    && password.bytes().all(|b| (32..=126).contains(&b))) =>
        {
            Err("Password needs 8-63 characters (or a 64-digit hex key)".into())
        }
        _ => Ok(()),
    }
}

pub fn profile(network: &Network, password: &str) -> Result<String, String> {
    validate(network, password)?;
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let key = match network.security {
        Security::Open => "    key_mgmt=NONE\n".to_string(),
        Security::Unsupported => {
            return Err("Only open and WPA/WPA2 personal networks are supported".into())
        }
        Security::Personal => {
            let psk = if password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()) {
                password.to_string()
            } else if (8..=63).contains(&password.len())
                && password.bytes().all(|b| (32..=126).contains(&b))
            {
                let mut psk = [0u8; 32];
                pbkdf2::pbkdf2_hmac::<sha1::Sha1>(
                    password.as_bytes(),
                    network.ssid.as_bytes(),
                    4096,
                    &mut psk,
                );
                hex(&psk)
            } else {
                return Err("Password needs 8-63 characters (or a 64-digit hex key)".into());
            };
            format!("    key_mgmt=WPA-PSK\n    psk={psk}\n")
        }
    };
    // SSIDs are hex, and PSKs are derived before writing: no config/shell injection, no
    // plaintext password in the saved file or process arguments.
    Ok(format!(
        "ctrl_interface={CONTROL}\nupdate_config=0\nnetwork={{\n    ssid={}\n{key}}}\n",
        hex(network.ssid.as_bytes())
    ))
}

pub fn parse_scan(text: &str) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line.splitn(5, '\t').collect();
        if fields.len() != 5 {
            continue;
        }
        let Ok(signal) = fields[2].parse::<i32>() else {
            continue;
        };
        let Some(ssid) = decode_ssid(fields[4]) else {
            continue;
        };
        if ssid.is_empty() || ssid.len() > 32 {
            continue;
        }
        let flags = fields[3];
        let security = if flags.contains("PSK") {
            Security::Personal
        } else if flags.contains("WEP")
            || flags.contains("WPA")
            || flags.contains("RSN")
            || flags.contains("OWE")
            || flags.contains("EAP")
            || flags.contains("SAE")
        {
            Security::Unsupported
        } else {
            Security::Open
        };
        if let Some(existing) = networks
            .iter_mut()
            .find(|n| n.ssid == ssid && n.security == security)
        {
            existing.signal = existing.signal.max(signal);
        } else {
            networks.push(Network {
                ssid,
                signal,
                security,
            });
        }
    }
    networks.sort_by(|a, b| b.signal.cmp(&a.signal).then(a.ssid.cmp(&b.ssid)));
    networks
}

fn decode_ssid(text: &str) -> Option<String> {
    let mut result = Vec::new();
    let mut bytes = text.bytes();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            result.push(b);
            continue;
        }
        match bytes.next()? {
            b'x' => {
                let high = (bytes.next()? as char).to_digit(16)?;
                let low = (bytes.next()? as char).to_digit(16)?;
                result.push((high * 16 + low) as u8);
            }
            b'\\' => result.push(b'\\'),
            b'"' => result.push(b'"'),
            _ => return None,
        }
    }
    let s = String::from_utf8(result).ok()?;
    if s.chars().any(char::is_control) {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stuck_helpers_are_stopped_with_a_bounded_timeout() {
        let start = Instant::now();
        assert!(command("/bin/sleep", &["10"], 0)
            .unwrap_err()
            .contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn saving_and_disabling_survive_a_new_read_of_the_card() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("System")).unwrap();
        assert!(enabled(root.path()));
        assert!(!auto_connect(root.path()));
        private_write(&root.path().join(PROFILE), b"profile").unwrap();
        assert!(auto_connect(root.path()));
        private_write(&root.path().join(DISABLED), b"disabled").unwrap();
        assert!(!enabled(root.path()));
        assert!(!auto_connect(root.path()));
        assert!(saved(root.path()));
        remove(&root.path().join(DISABLED)).unwrap();
        assert!(enabled(root.path()));
        assert!(auto_connect(root.path()));
        remove(&root.path().join(PROFILE)).unwrap();
        assert!(!saved(root.path()));
    }

    #[test]
    fn legacy_profile_grows_without_losing_other_saved_networks() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("System")).unwrap();
        let network = |ssid: &str| Network {
            ssid: ssid.into(),
            signal: -40,
            security: Security::Open,
        };
        let home = network("home");
        let cafe = network("Cafe");
        private_write(&root.path().join(PROFILE), profile(&home, "").unwrap().as_bytes())
            .unwrap();
        let config = add_profile(root.path(), &cafe, &profile(&cafe, "").unwrap()).unwrap();
        private_write(&root.path().join(PROFILE), config.as_bytes()).unwrap();
        assert_eq!(
            sorted_names(&load_profiles(root.path()).unwrap()),
            vec!["Cafe".to_string(), "home".to_string()]
        );

        let secured_home = Network {
            security: Security::Personal,
            ..home
        };
        let updated = add_profile(
            root.path(),
            &secured_home,
            &profile(&secured_home, "password123").unwrap(),
        )
        .unwrap();
        assert_eq!(parse_profiles(&updated).unwrap().len(), 2);
        assert!(updated.contains("key_mgmt=WPA-PSK"));
        private_write(&root.path().join(PROFILE), updated.as_bytes()).unwrap();
        private_write(&root.path().join(DISABLED), b"disabled\n").unwrap();
        handle(root.path(), Request::ForgetSaved("Cafe".into())).unwrap();
        assert_eq!(
            sorted_names(&load_profiles(root.path()).unwrap()),
            vec!["home".to_string()]
        );
        handle(root.path(), Request::ForgetSaved("home".into())).unwrap();
        assert!(!saved(root.path()));

        private_write(&root.path().join(PROFILE), b"invalid profile").unwrap();
        assert!(add_profile(root.path(), &cafe, &profile(&cafe, "").unwrap()).is_err());
        assert!(forget_profile(root.path(), "Cafe").is_err());
        assert_eq!(
            fs::read(root.path().join(PROFILE)).unwrap(),
            b"invalid profile".to_vec()
        );
    }

    #[test]
    fn saved_off_choice_rejects_network_work() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("System")).unwrap();
        private_write(&root.path().join(DISABLED), b"disabled\n").unwrap();
        assert_eq!(
            handle(root.path(), Request::Scan).err().as_deref(),
            Some("Turn Wi-Fi on first")
        );
        assert_eq!(
            handle(root.path(), Request::Reconnect).err().as_deref(),
            Some("Turn Wi-Fi on first")
        );
    }
    #[test]
    fn password_derivation_matches_the_wpa_test_vector() {
        let network = Network {
            ssid: "IEEE".into(),
            signal: -40,
            security: Security::Personal,
        };
        let text = profile(&network, "password").unwrap();
        assert!(
            text.contains("psk=f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e")
        );
        assert!(!text.contains("password"));
        assert!(profile(&network, "short").is_err());
    }
    #[test]
    fn scans_decode_names_and_deduplicate_without_merging_security() {
        let rows = "header\n00\t2412\t-70\t[WPA2-PSK-CCMP][ESS]\tMy\\x20WiFi\n01\t2412\t-30\t[WPA2-PSK-CCMP][ESS]\tMy WiFi\n02\t2412\t-20\t[ESS]\t\n03\t2412\t-50\t[WPA2-EAP-CCMP][ESS]\tWork\n04\t2412\t-60\t[ESS]\tGuest";
        let n = parse_scan(rows);
        assert_eq!(n.len(), 3);
        assert_eq!(n[0].ssid, "My WiFi");
        assert_eq!(n[0].signal, -30);
        assert_eq!(n[1].security, Security::Unsupported);
        assert_eq!(n[2].security, Security::Open);
    }
    #[test]
    fn network_names_cannot_inject_configuration() {
        let network = Network {
            ssid: "\"\\\nnetwork={".into(),
            signal: -40,
            security: Security::Open,
        };
        let config = profile(&network, "").unwrap();
        assert_eq!(config.matches("network={").count(), 1);
    }
}
