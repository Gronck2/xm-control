mod bt;
mod mdr;

use bt::{MDR_V2_UUID, format_mac, parse_guid, parse_mac};
use mdr::session::Transport;
use std::process::ExitCode;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|a| a == "--connect-test") {
        println!("{}", banner());
        return ExitCode::SUCCESS;
    }
    let device = args.iter().position(|a| a == "--device").and_then(|i| args.get(i + 1));
    match connect_test(device.map(String::as_str)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Connects, prints and closes. Sends no MDR command at all.
fn connect_test(device: Option<&str>) -> Result<(), String> {
    let mac = match device {
        Some(s) => parse_mac(s).ok_or_else(|| format!("bad MAC address {s:?}, expected AA:BB:CC:DD:EE:FF"))?,
        None => {
            let devices = bt::enumerate::paired_devices().map_err(|e| e.to_string())?;
            for d in &devices {
                println!("paired: {} {} {}", format_mac(d.mac), if d.connected { "connected" } else { "-" }, d.name);
            }
            let mut xm = devices.iter().filter(|d| d.name.contains("WH-1000XM"));
            match (xm.next(), xm.next()) {
                (Some(d), None) => d.mac,
                (None, _) => return Err("no WH-1000XM device is paired; pass --device <mac>".into()),
                _ => return Err("several WH-1000XM devices are paired; pass --device <mac>".into()),
            }
        }
    };
    let uuid = parse_guid(MDR_V2_UUID).expect("constant UUID is valid");
    println!("connecting to {} ...", format_mac(mac));
    let mut t = bt::winsock::WinsockRfcomm::connect(mac, uuid, CONNECT_TIMEOUT).map_err(|e| format!("connect failed: {e}"))?;
    println!("connected");
    t.close();
    println!("disconnected");
    Ok(())
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
}
