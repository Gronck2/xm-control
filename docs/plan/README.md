# MVP plan: order and layout

Each `NNNN-slug` has a task (`docs/tasks/`, what and why) and a plan (`docs/plan/`, how). One task = one PR (`task/NNNN-slug`).
Protocol facts come from `docs/arch/protocol.md` (study of mos9527/SonyHeadphonesClient, no XM6 capture yet).
"XM6 status" labels used in the tasks: **supported** (stated for XM6 in the study), **gated** (only if the headset's support-function list advertises it),
**unknown** (not confirmed until a real capture from `--probe`, task 0003).

## Order

```
0001 frame + command guard  ->  0002 RFCOMM transport + session  ->  0003 handshake + --probe   (owner sends a capture here)
                                                                        |
                                                              0004 device state + notifications + dev CLI
                                                                        |
        0005 battery   0006 NC/ambient   0007 EQ   0008 DSEE   0009 Speak-to-Chat   0010 power/wearing   0011 multipoint   (independent of each other)
                                                                        |
   ADR "UI stack" (owner decides)  ->  0012 UI shell  ->  0013 UI sound panels (battery, NC, EQ, DSEE)
                                                      ->  0014 UI system panels (STC, power/wearing, multipoint)
```

- 0001-0004 and 0005-0011 never touch UI code; the layers are: `mdr` (frame, guard, session), `bt` (transport), `device` (state + features), `ui`.
- 0012 can start after 0004 once the ADR exists; it may run in parallel with 0005-0011, but 0013/0014 need the matching feature tasks.
- After 0003 the owner sends the capture; the "inferred/unknown" items (NC type 0x19, battery 0x00 vs 0x08, DSEE, wearing status, HBS type, general-settings slots)
  are then fixed in the feature tasks before they are implemented. Feature tasks must gate on the support list, never on the model name.

## Cross-cutting rules (apply to every task)

- Every device-bound byte goes through the command guard from 0001 (typed allowlist; denylist wins). Each feature task adds its own allowlist entries and a test for them.
- XM6 hazard: enabling Background Music (listening mode `E8 03|09`) while Voice Control is active reboots the headset. Listening modes are NOT in the MVP;
  the allowlist must not contain `E8` and the guard has a test for that. If it is ever added: read `E2 <type>` status first and refuse unavailable modes.
- Notifications (button presses on the headset, other device changes) arrive unsolicited as NTFY frames; they must update the state and the UI (0004 builds the path, features hook in).
- The agent cannot see or hear the headphones: every "(b) owner verifies" item is a manual checklist the owner runs and reports.
- Update `docs/arch/overview.md` in the PR that changes the shape of the system.

## Owner decisions (2026-11-21)

- UI stack: Win32 with native common controls (`windows-sys`), see ADR 0002.
- EQ: no Clear Bass on XM6 accepted; 10 bands −6..+6 plus presets. EQ sliders send on release.
- DSEE: dropped from the MVP if the probe shows 0xE2 is not advertised.
- Multipoint toggle only after a confirmation dialog.
- Probe file: MAC redacted by default; a reviewed, anonymised probe dump may be committed as test data.
- No auto-reconnect in the MVP: a "Connect" button. Battery polled every 60 s only when the headset sends no notifications.
