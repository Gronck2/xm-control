//! RFCOMM over Winsock `AF_BTH` (docs/arch/protocol.md section 1): authenticate + encrypt, SDP-resolved channel (port 0),
//! connect with a timeout, then reads with a timeout so a worker thread never hangs.

use crate::mdr::session::Transport;
use std::io;
use std::sync::Once;
use std::time::Duration;
use windows_sys::Win32::Devices::Bluetooth::*;
use windows_sys::Win32::Networking::WinSock::*;
use windows_sys::core::GUID;

pub struct WinsockRfcomm {
    sock: SOCKET,
}

fn last_error() -> io::Error {
    // Winsock error codes are Win32 codes, so the OS supplies the readable text.
    // SAFETY: no preconditions.
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

fn timeval(d: Duration) -> TIMEVAL {
    TIMEVAL { tv_sec: d.as_secs().min(i32::MAX as u64) as i32, tv_usec: d.subsec_micros() as i32 }
}

/// Waits until `sock` is readable (`write == false`) or writable/failed (`write == true`). Returns false on timeout.
fn wait(sock: SOCKET, write: bool, timeout: Duration) -> io::Result<bool> {
    let mut set = FD_SET { fd_count: 1, ..Default::default() };
    set.fd_array[0] = sock;
    let mut except = set;
    let tv = timeval(timeout);
    // SAFETY: the sets and timeval outlive the call; the first argument is ignored by Winsock.
    let n = unsafe {
        if write { select(0, std::ptr::null_mut(), &mut set, &mut except, &tv) } else { select(0, &mut set, std::ptr::null_mut(), &mut except, &tv) }
    };
    if n == SOCKET_ERROR { Err(last_error()) } else { Ok(n > 0) }
}

fn set_opt<T>(sock: SOCKET, level: i32, name: i32, value: &T) -> io::Result<()> {
    // SAFETY: `value` is valid for `size_of::<T>()` bytes.
    let r = unsafe { setsockopt(sock, level, name, value as *const T as *const u8, size_of::<T>() as i32) };
    if r == SOCKET_ERROR { Err(last_error()) } else { Ok(()) }
}

fn set_blocking(sock: SOCKET, blocking: bool) {
    let mut nb = u32::from(!blocking);
    // SAFETY: valid socket and pointer.
    unsafe { ioctlsocket(sock, FIONBIO, &mut nb) };
}

impl WinsockRfcomm {
    /// Opens an RFCOMM connection to `mac` for service `uuid`. Sends nothing.
    pub fn connect(mac: u64, uuid: GUID, timeout: Duration) -> io::Result<Self> {
        static INIT: Once = Once::new();
        let mut init_err = None;
        INIT.call_once(|| {
            let mut data = WSADATA::default();
            // SAFETY: valid out-pointer.
            let r = unsafe { WSAStartup(0x0202, &mut data) };
            if r != 0 {
                init_err = Some(io::Error::from_raw_os_error(r));
            }
        });
        if let Some(e) = init_err {
            return Err(e);
        }
        // SAFETY: plain socket creation.
        let sock = unsafe { socket(AF_BTH as i32, SOCK_STREAM, BTHPROTO_RFCOMM as i32) };
        if sock == INVALID_SOCKET {
            return Err(last_error());
        }
        let s = WinsockRfcomm { sock }; // Drop closes it on every error path below
        let on: u32 = 1;
        set_opt(sock, SOL_RFCOMM as i32, SO_BTH_AUTHENTICATE as i32, &on)?;
        set_opt(sock, SOL_RFCOMM as i32, SO_BTH_ENCRYPT as i32, &on)?;
        set_blocking(sock, false);
        let addr = SOCKADDR_BTH { addressFamily: AF_BTH, btAddr: mac, serviceClassId: uuid, port: 0 };
        // SAFETY: `addr` is a valid SOCKADDR_BTH of the given length.
        let r = unsafe { connect(sock, &addr as *const SOCKADDR_BTH as *const SOCKADDR, size_of::<SOCKADDR_BTH>() as i32) };
        if r == SOCKET_ERROR {
            let e = last_error();
            if e.raw_os_error() != Some(WSAEWOULDBLOCK) {
                return Err(e);
            }
            if !wait(sock, true, timeout)? {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "connect timed out (headset off, out of range, or not answering)"));
            }
            let mut code: i32 = 0;
            let mut len = size_of::<i32>() as i32;
            // SAFETY: valid out-pointers of the stated size.
            unsafe { getsockopt(sock, SOL_SOCKET, SO_ERROR, &mut code as *mut i32 as *mut u8, &mut len) };
            if code != 0 {
                return Err(io::Error::from_raw_os_error(code));
            }
        }
        set_blocking(sock, true); // reads are gated by select, writes by SO_SNDTIMEO
        set_opt(sock, SOL_SOCKET, SO_SNDTIMEO, &5000u32)?;
        Ok(s)
    }
}

impl Transport for WinsockRfcomm {
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if !wait(self.sock, false, timeout)? {
            return Ok(0);
        }
        // SAFETY: `buf` is valid for `len` bytes.
        let n = unsafe { recv(self.sock, buf.as_mut_ptr(), buf.len().min(i32::MAX as usize) as i32, 0) };
        match n {
            0 => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed by the headset")),
            n if n < 0 => Err(last_error()),
            n => Ok(n as usize),
        }
    }

    fn write(&mut self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            // SAFETY: `data` is valid for `len` bytes.
            let n = unsafe { send(self.sock, data.as_ptr(), data.len().min(i32::MAX as usize) as i32, 0) };
            if n < 0 {
                return Err(last_error());
            }
            data = &data[n as usize..];
        }
        Ok(())
    }

    fn close(&mut self) {
        if self.sock != INVALID_SOCKET {
            // SAFETY: the socket is ours and closed once.
            unsafe { closesocket(self.sock) };
            self.sock = INVALID_SOCKET;
        }
    }
}

impl Drop for WinsockRfcomm {
    fn drop(&mut self) {
        self.close();
    }
}
