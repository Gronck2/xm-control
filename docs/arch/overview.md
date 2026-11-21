# Architecture overview

Current shape: a scaffold only. `src/main.rs` prints the version; there is no Bluetooth transport, protocol or UI yet.

Intended shape (not built yet): a Windows window app that opens an RFCOMM connection to Sony WH-1000XM6 headphones,
speaks Sony's MDR protocol (framing, ACKs, get/set/notify commands) and shows the headphone settings in one window.
The UI stack is still to be decided (ADR pending).
