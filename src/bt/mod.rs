//! Windows Bluetooth Classic: paired-device enumeration and the Winsock RFCOMM transport.

pub mod enumerate;
pub mod winsock;

use windows_sys::core::GUID;

/// MDR V2 RFCOMM service (XM5 and newer, so the XM6; docs/arch/protocol.md section 1).
pub const MDR_V2_UUID: &str = "956C7B26-D49A-4BA8-B03F-B17D393CB6E2";

/// Parses `8-4-4-4-12` hex digits into a GUID (parsed by hand to avoid linking Rpcrt4).
pub fn parse_guid(s: &str) -> Option<GUID> {
    let parts: Vec<&str> = s.split('-').collect();
    let [a, b, c, d, e] = parts[..] else { return None };
    if [a.len(), b.len(), c.len(), d.len(), e.len()] != [8, 4, 4, 4, 12] || !s.bytes().all(|x| x == b'-' || x.is_ascii_hexdigit()) {
        return None;
    }
    let tail = u64::from_str_radix(&format!("{d}{e}"), 16).ok()?.to_be_bytes();
    Some(GUID { data1: u32::from_str_radix(a, 16).ok()?, data2: u16::from_str_radix(b, 16).ok()?, data3: u16::from_str_radix(c, 16).ok()?, data4: tail })
}

/// Parses `AA:BB:CC:DD:EE:FF` (or with `-`) into the 48-bit address as a u64.
pub fn parse_mac(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split([':', '-']).collect();
    if parts.len() != 6 || parts.iter().any(|p| p.len() != 2) {
        return None;
    }
    parts.iter().try_fold(0u64, |acc, p| Some(acc << 8 | u64::from_str_radix(p, 16).ok()?))
}

pub fn format_mac(mac: u64) -> String {
    let b = mac.to_be_bytes();
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[2], b[3], b[4], b[5], b[6], b[7])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_parses_to_guid_bytes() {
        let g = parse_guid(MDR_V2_UUID).unwrap();
        assert_eq!((g.data1, g.data2, g.data3), (0x956C7B26, 0xD49A, 0x4BA8));
        assert_eq!(g.data4, [0xB0, 0x3F, 0xB1, 0x7D, 0x39, 0x3C, 0xB6, 0xE2]);
    }

    #[test]
    fn bad_uuid_rejected() {
        for s in [
            "",
            "956C7B26-D49A-4BA8-B03F",
            "956C7B26-D49A-4BA8-B03F-B17D393CB6E2-00",
            "956C7B2G-D49A-4BA8-B03F-B17D393CB6E2",
            "956C7B26D49A4BA8B03FB17D393CB6E2",
        ] {
            assert!(parse_guid(s).is_none(), "{s}");
        }
    }

    #[test]
    fn mac_round_trip() {
        assert_eq!(parse_mac("AA:BB:CC:DD:EE:FF"), Some(0xAABBCCDDEEFF));
        assert_eq!(parse_mac("aa-bb-cc-dd-ee-01"), Some(0xAABBCCDDEE01));
        assert_eq!(format_mac(0xAABBCCDDEE01), "AA:BB:CC:DD:EE:01");
        for s in ["", "AA:BB:CC:DD:EE", "AA:BB:CC:DD:EE:FF:00", "AA:BB:CC:DD:EE:GG", "A:BB:CC:DD:EE:FF"] {
            assert!(parse_mac(s).is_none(), "{s}");
        }
    }
}
