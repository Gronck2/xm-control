# 0013 UI sound panels: battery, NC/ambient, equalizer, DSEE

## Goal
Add the sound-related panels to the window: battery indicator, NC/ambient controls, equalizer, DSEE.

## Why
The main daily controls.

## Blocked by
0012 (shell + UI ADR), 0005, 0006, 0007, 0008 (DSEE panel only if 0xE2 is advertised on XM6).

## Scope
- Battery: percent + charging state, updated live.
- NC/ambient: segmented/radio Off | Noise cancelling | Ambient; ambient level slider 1..20; focus-on-voice toggle; auto ambient toggle and adaptive sensitivity dropdown only if the headset advertises them. The slider sends UNDER_CHANGING while dragging and CHANGED on release (core handles rate limiting, 0006).
- Equalizer: preset dropdown with only the XM6-accepted presets (HEAVY, CLEAR, HARD, SOFT, CUSTOM, user slots as advertised); 10 sliders -6..+6 labelled 31 Hz..16 kHz, enabled when the preset is CUSTOM/user; whole panel disabled when EQ status says unavailable. **No Clear Bass control** (XM6 has none).
- DSEE: toggle OFF/AUTO, hidden if not advertised.
- All panels reflect headset-side changes (notifications), and do not feed back echo loops (a change received from the headset must not trigger a SET).
- Controls whose feature is None are hidden.

## Out of scope
Listening modes (BGM etc.; XM6 reboot hazard when BGM is enabled while Voice Control is active), saving EQ presets, tray/hotkeys.

## XM6 status
Battery, NC/ambient, EQ: **supported**. DSEE: **unknown** until the capture. Auto ambient/adaptive sensitivity: **gated**.

## Acceptance
(a) Unit tests on the view model: event updates each panel's displayed values; hidden when None; EQ sliders disabled for non-custom presets and when EQ unavailable; no SET emitted on events received from the device; slider bounds; preset list excludes forbidden ids.
(b) Owner: each control changes what the headphones do, same as the CLI results in 0005-0008; press the NC/AMB button on the headset and see the segmented control follow; change EQ on another device and see sliders follow; drag the ambient slider smoothly with no stutter or runaway changes; report visual/layout problems.

## Open owner decisions
- Whether EQ sliders should send continuously while dragging or on release: recommend on release (one SET per change, fewer headset glitches).
