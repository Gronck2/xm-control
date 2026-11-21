# MVP plan: order and layout

Each `NNNN-slug` has a task (`docs/tasks/`, what and why) and a plan (`docs/plan/`, how). One task = one PR (`task/NNNN-slug`).
Protocol facts come from `docs/arch/protocol.md` (study of mos9527/SonyHeadphonesClient; section 8 holds the decoded XM6 capture and overrides the "inferred" marks in sections 4 and 6).
"XM6 status" labels used in the tasks: **confirmed** (seen in the XM6 capture), **reference-derived** (read from the reference source, not seen on the wire), **gated** (only if the headset's support-function list advertises it).

## Capture

`tests/fixtures/xm6-probe.txt` is an anonymised `--probe` dump from a WH-1000XM6 (firmware 3.1.5, protocol `0x03003032`): both support-function lists, 22 feature reads and the full frame log.
It settled: the NC variant (`0x19` only), the battery variant (`22 00` only), the EQ layout (10 bands, no Clear Bass), that DSEE (0xE2) is advertised, that auto power-off exists only in the wearing variant (0x25, candidates "when removed from ears" / "never"; no 0x24 timer),
the multipoint list variant (type `0x02`), that wearing status and auto pause are available, that HBS_2 (not HBS_1) is the safe-listening type, and that the General Settings probe used a wrong type byte (`D1..D3`, not `01..03`).
Still open: EQ capability and preset names, General Settings names, NTFY layouts (reference only), polarity proof for the on/off fields (owner cross-check, protocol.md 8.5).
Feature tasks build their unit tests from this file's reply bytes and support lists. Do not commit the raw, un-anonymised dump (`probe.txt` at the repo root).

## Order

```
0001 frame + command guard  ->  0002 RFCOMM transport + session  ->  0003 handshake + --probe   (capture done)
                                                                        |
                                                              0004 device state + notifications + dev CLI
                                                                        |
        0005 battery   0006 NC/ambient   0007 EQ   0008 DSEE   0009 Speak-to-Chat   0010 power/wearing   0011 multipoint   (independent of each other)
                                                                        |
   ADR "UI stack" (owner decides)  ->  0012 UI shell  ->  0013 UI sound panels (battery, NC, EQ, DSEE)
                                                      ->  0014 UI system panels (STC, power/wearing, multipoint)
                                                                        |
                                                              0015 readable errors (Display for the error enums; stacked on 0014)
```

- 0001-0004 and 0005-0011 never touch UI code; the layers are: `mdr` (frame, guard, session), `bt` (transport), `device` (state + features), `ui`.
- 0012 can start after 0004 once the ADR exists; it may run in parallel with 0005-0011, but 0013/0014 need the matching feature tasks.
- The "inferred/unknown" items from before the capture are settled in the feature tasks. Feature tasks must gate on the support list, never on the model name.
- 0015 is a small text-only follow-up (no protocol change, no SETs); it is stacked on the 0014 branch because the UI code lives there.

## Which tasks send SET commands

| Task | Sends SETs | Commands |
|---|---|---|
| 0004 core | no | none (dummy feature on a mock transport) |
| 0005 battery | no | none |
| 0006 NC / ambient | **yes** | `68 19 ...` |
| 0007 equalizer | **yes** | `58 00 ...` |
| 0008 DSEE | **yes** | `E8 01 00|01` |
| 0009 Speak-to-Chat | **yes** | `F8 0C ...`, `FC 0C ...` |
| 0010 power / wearing | **yes** (wearing status is read-only) | `28 05 ...`, `F8 01 ...` |
| 0011 multipoint | **yes** (device list and pairing status are read-only) | `38 01 ...` |
| 0015 readable errors | no | none |

## Cross-cutting rules (apply to every task)

- Every device-bound byte goes through the command guard from 0001 (typed allowlist; denylist wins). Each feature task adds its own allowlist entries and a test for them. The guard matches exact payloads; SET rows with arguments need a small per-row validator, which the first SET PR to land adds (0006 or 0008, whichever merges first) and the others reuse.
- XM6 hazard: enabling Background Music (listening mode `E8 03|09`) while Voice Control is active reboots the headset. Listening modes are NOT in the MVP; the allowlist contains no `E8` row except DSEE's two exact payloads `E8 01 00` and `E8 01 01` (0008), and the guard tests pin that every other `E8` payload is rejected. If listening modes are ever added: read `E2 <type>` status first and refuse unavailable modes.
- Notifications (button presses on the headset, other device changes) arrive unsolicited as NTFY frames; they must update the state and the UI (0004 builds the path, features hook in). NTFY layouts are reference-derived until a feature's hardware check sees one.
- Replies are matched by reply command id; a request can be ACKed and never answered (XM6 does this for EQ status and general settings), and the reply type byte can differ from the request (T2 `5A 00` is answered `5B 02`).
- The agent cannot see or hear the headphones: every "(b) owner verifies" item is a manual checklist the owner runs and reports.
- Update `docs/arch/overview.md` in the PR that changes the shape of the system.

## Owner hardware batches

Feature PRs are stacked in batch order so the owner builds the top branch of a batch once and runs one checklist. Each task's "(b) Owner" list is the detail; a batch run covers its tasks in one session.
Before a batch, the owner can answer the cross-check from protocol.md 8.5 (what the Sony app showed at capture time: NC on level 20, DSEE auto, Speak-to-Chat off, auto pause on, auto power-off "when removed from ears", multipoint on), which proves the on/off polarities cheaply.

| Batch | Tasks | What the owner does | Restore afterwards |
|---|---|---|---|
| 1 | 0004 + 0005 + 0006 | `--get` model/firmware; `--get battery` vs the phone, charger plug (CHARGING/CHARGED), `--watch` shows battery and NC NTFY; NC off / NC / ambient, level 1 vs 20, focus on voice, auto ambient, sensitivity; press the NC/AMB button and see the NTFY | NC on, level 20 |
| 2 | 0007 + 0008 + 0009 | `--get eq` (report the capability output), HEAVY/CLEAR/HARD/SOFT, a custom curve and a reconnect; DSEE OFF/AUTO and read back; Speak-to-Chat on/off (confirm on = really on), sensitivity and timeout | EQ preset 0x32, DSEE auto, Speak-to-Chat as before |
| 3 | 0010 + 0011 | auto power-off never / when removed (last, it powers the headset off), auto pause behaviour, wearing display in `--watch`; device list vs reality, multipoint off/on with `--yes` and what drops | auto power-off "when removed", auto pause on, multipoint on |

- 0004 and 0005 are read-only; per the merge rule below they merge on green CI + clean pipeline, and their hardware items are checked in batch 1. A failed item becomes a follow-up fix, not a blocker for the SET PRs above them in the stack.
- 0006-0011 send SETs: each merges only after the owner confirms its part of the batch. A failed check blocks only its own PR (retarget the PRs above it, see Process notes).

## Process notes

- **Stacked PRs:** when the lower PR is squash-merged with `--delete-branch`, GitHub closes the upper PR. Before merging the lower one, retarget the upper PR to main (`gh pr edit N --base main`), or don't stack; after a squash merge, bring main into the upper branch with a merge commit (no force-push).
- **Merging:** feature PRs that send SET commands are merged only after the owner confirms the behaviour on the headphones; read-only PRs merge on green CI + clean pipeline. Never merge on red or pending CI (see the Pipeline contract in `CLAUDE.md`).

## Owner decisions (2026-11-21)

- UI stack: Win32 with native common controls (`windows-sys`), see ADR 0002.
- EQ: no Clear Bass on XM6 accepted; 10 bands −6..+6 plus presets. EQ sliders send on release.
- DSEE: dropped from the MVP if the probe shows 0xE2 is not advertised. **Outcome: 0xE2 is advertised, DSEE stays (0008).**
- Multipoint toggle only after a confirmation dialog.
- Probe file: MAC redacted by default; a reviewed, anonymised probe dump may be committed as test data (done: `tests/fixtures/xm6-probe.txt`).
- No auto-reconnect in the MVP: a "Connect" button. Battery polled every 60 s only when the headset sends no notifications.
