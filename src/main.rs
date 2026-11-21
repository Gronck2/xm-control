#![windows_subsystem = "windows"]

mod bt;
mod device;
mod mdr;
mod probe;
mod ui;

use bt::{MDR_V2_UUID, format_mac, parse_guid, parse_mac};
use device::{Command, DeviceHandle, Event, FEATURES};
use mdr::session::{Timeouts, Transport};
use std::process::ExitCode;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long `--probe` listens for unsolicited frames after the reads.
const PROBE_LISTEN: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return match ui::view::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                console::attach();
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
    }
    console::attach();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if !["--connect-test", "--probe", "--get", "--set", "--watch"].iter().any(|f| has(f)) {
        println!("{}", banner());
        return ExitCode::SUCCESS;
    }
    let result = (|| {
        let device = flag_value(&args, "--device")?;
        if has("--probe") {
            let out = flag_value(&args, "--probe")?.ok_or("--probe needs an output file")?;
            probe(out, device, has("--keep-mac"))
        } else if has("--get") || has("--set") || has("--watch") {
            device_cli(&args, device)
        } else {
            connect_test(device)
        }
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The value after `flag`, `None` if the flag is absent, an error if it is last.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Result<Option<&'a str>, String> {
    match args.iter().position(|a| a == flag) {
        Some(i) => args.get(i + 1).filter(|v| !v.starts_with("--")).map(|v| Some(v.as_str())).ok_or_else(|| format!("{flag} needs a value")),
        None => Ok(None),
    }
}

/// `--device` if given, else the single paired `WH-1000XM*` (the paired list is printed).
fn resolve_mac(device: Option<&str>) -> Result<u64, String> {
    match device {
        Some(s) => parse_mac(s).ok_or_else(|| format!("bad MAC address {s:?}, expected AA:BB:CC:DD:EE:FF")),
        None => {
            let devices = bt::enumerate::paired_devices().map_err(|e| e.to_string())?;
            for d in &devices {
                println!("paired: {} {} {}", format_mac(d.mac), if d.connected { "connected" } else { "-" }, d.name);
            }
            let mut xm = devices.iter().filter(|d| d.name.contains("WH-1000XM"));
            match (xm.next(), xm.next()) {
                (Some(d), None) => Ok(d.mac),
                (None, _) => Err("no WH-1000XM device is paired; pass --device <mac>".into()),
                _ => Err("several WH-1000XM devices are paired; pass --device <mac>".into()),
            }
        }
    }
}

fn connect(mac: u64) -> Result<bt::winsock::WinsockRfcomm, String> {
    let uuid = parse_guid(MDR_V2_UUID).expect("constant UUID is valid");
    println!("connecting to {} ...", format_mac(mac));
    let t = bt::winsock::WinsockRfcomm::connect(mac, uuid, CONNECT_TIMEOUT).map_err(|e| format!("connect failed: {e}"))?;
    println!("connected");
    Ok(t)
}

/// Connects, prints and closes. Sends no MDR command at all.
fn connect_test(device: Option<&str>) -> Result<(), String> {
    let mut t = connect(resolve_mac(device)?)?;
    t.close();
    println!("disconnected");
    Ok(())
}

/// Read-only diagnostic: handshake plus feature GETs, written to `out` (overwritten if it exists). The file is written even if the run fails part-way.
fn probe(out: &str, device: Option<&str>, keep_mac: bool) -> Result<(), String> {
    let mac = resolve_mac(device)?;
    let t = connect(mac)?;
    let (report, result) = probe::run(t, mac, keep_mac, PROBE_LISTEN, Timeouts::default());
    std::fs::write(out, report).map_err(|e| format!("cannot write {out}: {e}"))?;
    println!("wrote {out}");
    result
}

/// Dev CLI over the device worker (ADR 0004): `--get [feature]` prints the device and its state after the initial sync, `--set <feature> <value>`
/// sends one SET and logs events for a few seconds, `--watch [secs]` logs events (default 60 s). `--get` and `--watch` send no command beyond the
/// initial read-only sync. The feature name is checked before connecting, so an unknown one never reaches the headset.
fn device_cli(args: &[String], device: Option<&str>) -> Result<(), String> {
    let after = |flag: &str, n: usize| args.iter().position(|a| a == flag).and_then(|i| args.get(i + n)).filter(|v| !v.starts_with("--"));
    let (command, linger) = if let Some(feature) = after("--set", 1) {
        let value = after("--set", 2).ok_or("--set needs <feature> <value>")?;
        (Some(Command::Set { feature: feature.clone(), value: value.clone() }), Duration::from_secs(3))
    } else if let Some(i) = args.iter().position(|a| a == "--watch") {
        (None, Duration::from_secs(args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(60)))
    } else {
        (None, Duration::ZERO)
    };
    if args.iter().any(|a| a == "--set") && command.is_none() {
        return Err("--set needs <feature> <value>".into());
    }
    if let Some(name) = after("--set", 1).or_else(|| after("--get", 1))
        && !FEATURES.iter().any(|f| f.name() == name)
    {
        let known: Vec<_> = FEATURES.iter().map(|f| f.name()).collect();
        return Err(format!("unknown feature {name:?} (known: {})", if known.is_empty() { "none yet".into() } else { known.join(", ") }));
    }
    let (handle, events) = DeviceHandle::connect(resolve_mac(device)?, None);
    println!("connecting ...");
    let (mut command, mut until) = (command, None::<Instant>);
    loop {
        let wait = until.map_or(CONNECT_TIMEOUT * 3, |u| u.saturating_duration_since(Instant::now()));
        match events.recv_timeout(wait) {
            Ok(Event::Connected(i)) => {
                println!("connected: {} firmware {} ({} + {} support functions)", i.model_name, i.firmware, i.t1_functions.len(), i.t2_functions.len())
            }
            Ok(Event::Synced(state)) => {
                println!("synced: {state:?}");
                if let Some(c) = command.take() {
                    handle.send(c).map_err(|_| "the device worker has ended")?;
                }
                until = Some(Instant::now() + linger);
            }
            Ok(Event::Disconnected(reason)) => return Err(format!("disconnected: {reason}")),
            Ok(e) => println!("{e:?}"),
            Err(RecvTimeoutError::Timeout) if until.is_some() => return Ok(()),
            Err(RecvTimeoutError::Timeout) => return Err("no answer from the headset".into()),
            Err(RecvTimeoutError::Disconnected) => return Err("device worker ended unexpectedly".into()),
        }
    }
}

/// The exe is a GUI-subsystem program (no console window when double-clicked), so the CLI flags attach to the parent console.
mod console {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};

    const GENERIC_WRITE: u32 = 0x4000_0000;

    /// Attaches to the parent's console and points stdout/stderr at it unless they are already redirected (file or pipe).
    pub fn attach() {
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        // SAFETY: plain Win32 calls; the CONOUT$ handle is intentionally kept open for the life of the process.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
            for std in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let h = GetStdHandle(std);
                if h.is_null() || h == INVALID_HANDLE_VALUE {
                    let con =
                        CreateFileW(name.as_ptr(), GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
                    SetStdHandle(std, con);
                }
            }
        }
    }
}

fn banner() -> String {
    format!("xm-control {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn banner_has_version() {
        assert_eq!(super::banner(), concat!("xm-control ", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn flag_value_cases() {
        let a: Vec<String> = ["--probe", "out.txt", "--device"].map(String::from).to_vec();
        assert_eq!(super::flag_value(&a, "--probe").unwrap(), Some("out.txt"));
        assert_eq!(super::flag_value(&a, "--keep-mac").unwrap(), None);
        assert!(super::flag_value(&a, "--device").is_err());
        let b: Vec<String> = ["--probe", "--keep-mac"].map(String::from).to_vec();
        assert!(super::flag_value(&b, "--probe").is_err());
    }
}
