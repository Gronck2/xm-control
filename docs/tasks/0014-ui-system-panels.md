# 0014 UI system panels: Speak-to-Chat, power and wearing, multipoint and devices

## Goal
Add the remaining panels: Speak-to-Chat, auto power-off / auto pause / wearing status, multipoint toggle and the read-only device list.

## Why
Completes the MVP feature set in the window.

## Blocked by
0012, 0009, 0010, 0011.

## Scope
- Speak-to-Chat: toggle, sensitivity dropdown (Auto/High/Low), timeout dropdown (Fast/Mid/Slow/Manual-off).
- Power: auto power-off dropdown with only the advertised candidates (incl. "when removed from ears" if listed), auto-pause toggle, wearing status text (left/right/both not worn, normal), live via notifications.
- Multipoint toggle with a confirmation dialog (it may drop a connection), and a list of paired devices with a connected marker, read-only. **No unpair, no pairing mode, no connect/disconnect buttons.**
- Hidden when not advertised; changes from the headset update the panels.

## Out of scope
Power-off-now button, listening modes, voice guidance, general settings (touch sensor etc.: needs per-device capture, later task).

## XM6 status
Speak-to-Chat, power off, multipoint: **supported**. Auto pause, wearing-based power off, wearing status: **gated/unknown** until capture.

## Acceptance
(a) Unit tests on the view model: dropdown options come from the advertised candidates; hidden when None; wearing codes mapped to text (unknown code shown as "unknown"); multipoint toggle emits a command only after confirmation; no widget can produce a `3C`/`34` command (the guard rejects them anyway; test that the intent set has none).
(b) Owner: each control behaves as in the CLI checks of 0009-0011; wearing text follows putting the headset on/off; the multipoint confirmation appears and cancel does nothing; the device list matches the paired devices and updates when a device connects/disconnects; report layout issues.

## Open owner decisions
- Whether to show the wearing status as text only or as a small left/right graphic: recommend text for the MVP.
