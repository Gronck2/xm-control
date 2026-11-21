mod bt;
mod device;
mod mdr;
mod probe;

use bt::{MDR_V2_UUID, format_mac, parse_guid, parse_mac};
use mdr::session::{Timeouts, Transport};
use std::process::ExitCode;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long `--probe` listens for unsolicited frames after the reads.
const PROBE_LISTEN: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if !has("--connect-test") && !has("--probe") {
        println!("{}", banner());
        return ExitCode::SUCCESS;
    }
    let result = (|| {
        let device = flag_value(&args, "--device")?;
        if has("--probe") {
            let out = flag_value(&args, "--probe")?.ok_or("--probe needs an output file")?;
            probe(out, device, has("--keep-mac"))
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
