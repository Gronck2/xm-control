# Plan 0002 RFCOMM transport and MDR session

## Approach
`src/bt/` holds the OS-specific part (`enum_devices`, `WinsockRfcomm`), `src/mdr/session.rs` holds the protocol logic over `trait Transport`.
Follow the study's Winsock sequence (protocol.md §1) but write it ourselves: `WSAStartup 2.2`, `socket(AF_BTH, SOCK_STREAM, BTHPROTO_RFCOMM)`, `SO_BTH_AUTHENTICATE`/`SO_BTH_ENCRYPT`,
`SOCKADDR_BTH` with the service class id and port 0, non-blocking connect + `select` with timeout, then blocking-with-timeout reads (`SO_RCVTIMEO` or select).
Session core is a pure state machine fed by `Transport` reads, with an injectable clock/timeouts for tests.

## Steps
1. Add `windows-sys` (features: Win32_Networking_WinSock, Win32_Devices_Bluetooth, Win32_System_Rpc/Rpcrt4 for UUID parse or parse the GUID by hand to avoid Rpcrt4).
2. `Transport` trait + `MockTransport` (scripted reads, recorded writes).
3. `session.rs`: `send(cmd) -> Result<()>`, `request(cmd, expected_reply_id) -> Result<Frame>`, `poll_notifications()`; ACK/seq/retry rules from protocol.md §2.
4. Winsock transport + device enumeration; error mapping (`WSAGetLastError` to readable text).
5. `--connect-test [--device <mac>]` in `main.rs` (connect, print, close); tests as in the task.

## Files likely touched
`Cargo.toml`, `src/main.rs`, `src/bt/{mod,winsock,enumerate}.rs`, `src/mdr/session.rs`, `docs/arch/overview.md`.

## Tradeoffs
- Winsock vs WinRT RFCOMM: Winsock is what the reference uses on Windows Classic and needs no COM; WinRT would add a heavy dependency.
- Blocking worker thread vs async: one serial stop-and-wait link needs no concurrency; simpler to test with a mock.
- Guid parse by hand avoids linking Rpcrt4; small and unit-testable.
