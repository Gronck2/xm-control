//! Paired Bluetooth device list via `BluetoothFindFirstRadio` / `BluetoothFindFirstDevice`.

use std::io;
use windows_sys::Win32::Devices::Bluetooth::*;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};

pub struct Device {
    pub name: String,
    pub mac: u64,
    pub connected: bool,
}

/// Lists remembered/paired devices of every radio. Errors if there is no Bluetooth radio.
pub fn paired_devices() -> io::Result<Vec<Device>> {
    let mut out = Vec::new();
    let params = BLUETOOTH_FIND_RADIO_PARAMS { dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32 };
    let mut radio: HANDLE = std::ptr::null_mut();
    // SAFETY: plain Win32 calls with valid out-pointers; every handle opened here is closed before returning.
    unsafe {
        let find = BluetoothFindFirstRadio(&params, &mut radio);
        if find.is_null() {
            return Err(io::Error::other(format!("no Bluetooth radio found (Win32 error {})", GetLastError())));
        }
        loop {
            list_radio_devices(radio, &mut out);
            CloseHandle(radio);
            if BluetoothFindNextRadio(find, &mut radio) == 0 {
                break;
            }
        }
        BluetoothFindRadioClose(find);
    }
    Ok(out)
}

/// SAFETY: `radio` must be a valid open radio handle.
unsafe fn list_radio_devices(radio: HANDLE, out: &mut Vec<Device>) {
    let search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: 1,
        fReturnRemembered: 1,
        fReturnUnknown: 0,
        fReturnConnected: 1,
        fIssueInquiry: 0, // no inquiry: it would take seconds and we only want already-paired devices
        cTimeoutMultiplier: 0,
        hRadio: radio,
    };
    let mut info = BLUETOOTH_DEVICE_INFO { dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32, ..Default::default() };
    // SAFETY: `search` and `info` are valid for the calls; the find handle is closed below.
    unsafe {
        let find = BluetoothFindFirstDevice(&search, &mut info);
        if find.is_null() {
            return;
        }
        loop {
            let len = info.szName.iter().position(|&c| c == 0).unwrap_or(info.szName.len());
            out.push(Device { name: String::from_utf16_lossy(&info.szName[..len]), mac: info.Address.Anonymous.ullLong, connected: info.fConnected != 0 });
            if BluetoothFindNextDevice(find, &mut info) == 0 {
                break;
            }
        }
        BluetoothFindDeviceClose(find);
    }
}
