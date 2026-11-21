# 0002. UI stack: Win32 with native common controls

Status: accepted (2026-11-21, owner decision)

## Context

XM Control is an ordinary settings window (no tray) with many standard controls: toggles, dropdowns, sliders (ambient level,
10 EQ bands), a read-only device list and a confirmation dialog. Candidates were Win32 + Direct2D with custom drawing
(the maintainer's other projects, C:\test\Accs and C:\test\wtf_sync), Win32 with native common controls, and egui/eframe.

## Decision

Win32 with native common controls (comctl32 v6: trackbar, combobox, button/checkbox, listview, task dialog) through `windows-sys`,
no UI framework. Direct2D only if a custom-drawn element is wanted later (e.g. a battery icon).

## Consequences

- Little UI code, small exe, native look and accessibility, same `windows-sys` style as the other projects.
- Common controls do not fully support dark mode; the window follows the light system look unless a later ADR changes that.
- The protocol/transport/device layers stay UI-independent; the UI only reads `DeviceState` and sends `Command`s.
