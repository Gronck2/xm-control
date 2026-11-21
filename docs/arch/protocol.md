# Sony MDR protocol (V2) for WH-1000XM6, from mos9527/SonyHeadphonesClient

Source: `https://github.com/mos9527/SonyHeadphonesClient` (MIT), shallow clone at commit
`c0594bd305fe10a69744b94b54e276c26f071bb5` (2026-11-20). Clone it at that commit to follow the file:line references.
Nothing was built or run; the report is based only on reading the source.
All paths below are relative to the repo root. `T1` = `libmdr/include/mdr/Generated/ProtocolV2T1.hpp`,
`T2` = `libmdr/include/mdr/Generated/ProtocolV2T2.hpp`, `V2` = `libmdr/include/mdr/Generated/ProtocolV2.hpp`.

> **Key point:** the library has **no per-model tables**. Every feature is gated at runtime by the
> *support function* list the device returns (`CONNECT_RET_SUPPORT_FUNCTION`, Table 1 and Table 2).
> XM6 knowledge in the repo is limited to: `docs/device-support/WH-1000XM6.md`, and a handful of code
> comments (EQ, voice assistant). There is **no recorded XM6 packet capture** in `tests/` (captures exist only
> for WF-1000XM5, WF-LC900 and WH-1000XM3). So the exact XM6 support-function list is *not* in the repo. Where this
> report says which variant XM6 uses, it is either stated in the source (cited) or inferred (marked **inferred**).
>
> **Update (2026-11-21):** we now have a real XM6 capture (`tests/fixtures/xm6-probe.txt`, firmware 3.1.5). Section 8 records
> what it settled and **overrides every "inferred", "unknown" and "XM6 ✅" remark in sections 4 and 6**.

---

## 1. Transport

### Service UUIDs (`libmdr/include/mdr-c/Base.h:47-53`)
| Name | UUID | Use |
|---|---|---|
| `MDR_SERVICE_UUID_V2` | `956C7B26-D49A-4BA8-B03F-B17D393CB6E2` | RFCOMM/SPP SDP service, **"XM5s and newer"**, so the XM6 uses this one |
| `MDR_SERVICE_UUID_V1` | `96CC203E-5068-46AD-B32D-E316F5E069BA` | RFCOMM, "XM4s and older" |
| `MDR_SERVICE_UUID_BLE` | `5B833E20-6BC7-4802-8E9A-723CECA4BD8F` | BLE GATT service (XM5+), speaks V2 |

The client picks the protocol family from the UUID it connected with (`client/Client.cpp:1180-1188`).
In "auto" mode it tries V2 first, then V1 (`client/Client.cpp:1233-1236`). Each attempt goes through
`TryConnectionAttempt` (`client/Client.cpp:1196-1212`).

### Windows Classic (RFCOMM) path: plain Winsock `AF_BTH`, not WinRT
`libmdr-bt/src/Windows/ConnectionWindows.cpp`:
- `WSAStartup(MAKEWORD(2,2))` (L65-75)
- `socket(AF_BTH, SOCK_STREAM, BTHPROTO_RFCOMM)` (L78)
- set non-blocking with `ioctlsocket(FIONBIO)` (L87)
- `setsockopt(SOL_RFCOMM, SO_BTH_AUTHENTICATE / SO_BTH_ENCRYPT, TRUE)` (L95-96)
- `SOCKADDR_BTH{ addressFamily=AF_BTH, serviceClassId=UuidFromStringA(uuid), btAddr=macAddressToULL(mac) }`, port left 0 so
  Windows resolves the channel through SDP (L97-107)
- `connect()`. `WSAEWOULDBLOCK`/in-progress counts as normal (L111-125). Completion is detected with `select()`
  (read/write/except fds) in `Poll` (≈L180-225)
- `recv`/`send` raw bytes (L140-175). Device enumeration uses `BluetoothFindFirstRadio/BluetoothFindFirstDevice`
  (≈L250+)
- Headers: `BluetoothAPIs.h`, `winsock2.h`, `ws2bth.h`, `rpc.h` (L1-8). Link with `ws2_32`, `Bthprops`, `Rpcrt4`.

Windows BLE path (optional): `libmdr-bt/src/Windows/ConnectionWindowsBLE.cpp` uses **WinRT/WRL COM**
(`IGattDeviceService`, L166-173). It is created in `client/Platform/Windows/ClientPlatform.cpp:255-300`.

The library's I/O loop (`libmdr/src/Headphones.cpp:82-108, 130-165`) runs non-blocking: poll(0), then send the
queued buffer in chunks of up to 2048 bytes, then recv up to 2048 bytes.

---

## 2. Framing (`libmdr/include/mdr/Command.hpp`, `libmdr/src/Command.cpp`)

```
0x3E | ESCAPE( dataType(1) | seq(1) | len(4, uint32 BE, unescaped payload length) | payload(len) | checksum(1) ) | 0x3C
```
- Start marker `0x3E` ('>'), end marker `0x3C` ('<') (`Command.hpp:10-11`).
- Escaping (`Command.hpp:6-9`, `Command.cpp:14-36`) covers **everything between the markers, checksum included**:
  `0x3C → 3D 2C`, `0x3D → 3D 2D`, `0x3E → 3D 2E`. Any other byte after `0x3D` is invalid (`Command.cpp:52-66`).
- Checksum is the **8-bit sum** of `type + seq + len[4] + payload`, computed before escaping (`Command.cpp:6-12, 70-90`).
- Unpack checks the markers, unescapes, needs at least 7 bytes, verifies the checksum and that `len` equals the payload
  size (`Command.cpp:92-138`).
- Receiving: the RX buffer is scanned for the first `0x3E` and the next `0x3C` (`Headphones.cpp:245-249`). Bad frames
  are discarded (`Headphones.cpp:264-279`).
- Maximum packet size is `kMDRMaxPacketSize = 2048` (`Command.hpp:15`).

### Data type byte (`libmdr/include/mdr/Protocol.hpp:44-63`)
| Value | Name | Use |
|---|---|---|
| `0x00` | DATA | |
| `0x01` | **ACK** | empty payload |
| `0x0C` | **DATA_MDR** | MDR **Table 1** commands (most features) |
| `0x0E` | **DATA_MDR_NO2** | MDR **Table 2** commands (peripheral/multipoint, voice guidance, safe listening, wearing) |
| 0x02, 0x09, 0x0A, 0x0D, 0x10-0x1E, 0x2D | MC_NO1/ICD/EV/COMMON/SHOT*/LARGE_DATA_COMMON | not used by the client |

Each generated struct's data type comes from `MDRTraits<T>::kDataType`. Table 1 structs map to `DATA_MDR`
(`libmdr/include/mdr/Generated/ProtocolV2T1Traits.hpp:5-6`) and Table 2 structs to `DATA_MDR_NO2` (`...ProtocolV2T2Traits.hpp`).

### Sequence number and ACK (`libmdr/src/Headphones.cpp:181-222, 311-333`)
- `seq` is **0 or 1** and alternates. The host keeps `mSeqNumber` (initially 0, `Details.hpp:465`) and sends each
  command with it (`Details.hpp:521`).
- **Device ACK to host:** type `0x01`, seq = `1 - mSeqNumber`, empty payload. On a matching ACK the host sets
  `mSeqNumber = 1 - mSeqNumber` and wakes the waiter. A stale ACK seq is logged and ignored (`Headphones.cpp:323-333`).
- **Host ACK to device:** on every received `DATA_MDR`/`DATA_MDR_NO2` frame, the host *immediately* replies with
  `ACK` and seq = `1 - receivedSeq` (`Headphones.cpp:189-190, 206; SendACK L318-321`).
- The payload's first byte is the command ID. If a `DATA_MDR` frame arrives with byte0 == `0x01`, it is handled as
  `CONNECT_RET_PROTOCOL_INFO` (`Headphones.cpp:191-192`).

### Timing and retry (`libmdr/src/Details.hpp:472-474, 576-595`)
- `SendCommandACK`: send, then wait up to **1000 ms** for the ACK (`mACKRetryTimeoutMS`). Up to **10 tries**
  (`mACKRetryCount`). **A retry reuses the same seq.** After 10 failures the task fails with a timeout.
- After the ACK, a response wait (`SendCommandAwait`) uses `mDefaultTimeoutMS = 5000` (`Details.hpp:474`, `Headphones.cpp:11-16`).
- Commands go out strictly one at a time (stop-and-wait). The coroutine waits for the ACK before sending the next one.

### Worked frames (computed from the rules above)
```
GET_PROTOCOL_INFO (T1, seq 0): 3E 0C 00 00 00 00 02 00 00 0E 3C
GET_CAPABILITY_INFO (T1, seq 1): 3E 0C 01 00 00 00 02 02 00 11 3C
Host ACK for received seq 0:     3E 01 01 00 00 00 00 02 3C
Host ACK for received seq 1:     3E 01 00 00 00 00 00 01 3C
```

### Payload primitives (`libmdr/include/mdr/Protocol.hpp`)
- Multi-byte integers are **big-endian** (`Int16BE`, `UInt16BE`, `Int24BE`, `Int32BE`).
- `MDRPrefixedString`: 1-byte length, then bytes (L445-475). `MDRPodArray<T>`: 1-byte count, then `count*sizeof(T)` (L562-590).
  `MDRArray<T>`: 1-byte count, then variable-size elements (L627+).
- **Boolean polarity in V2 is inverted:** `OnOffSettingValue { ON=0x00, OFF=0x01 }`, `EnableDisable { ENABLE=0x00, DISABLE=0x01 }`
  (`V2:14-19, 54-59`). `GsSettingValue` is the same (`T1:957-962`). By contrast, `NcAsmOnOffValue { OFF=0x00, ON=0x01 }` (`T1:1157-1162`).

---

## 3. Init / handshake (XM6 = V2) — `libmdr/src/HeadphonesV2.cpp:8-305`

Payload bytes are listed as `[cmd, args...]`; T1 = type `0x0C`, T2 = type `0x0E`. Every command waits for its ACK.

| # | Table | Payload | Meaning | Reply |
|---|---|---|---|---|
| 1 | T1 | `00 00` | CONNECT_GET_PROTOCOL_INFO (`T1:3641`) | `01 00 <ver:Int32BE> <table1 EnableDisable> <table2 EnableDisable>` (`T1:3745-3759`). The size must be exactly 8 (`HeadphonesV2.cpp:1029-1043`). It fails if Table 1 is not supported (L18-20). |
| 2 | T1 | `02 00` | CONNECT_GET_CAPABILITY_INFO | `03 00 <counter> <pstr uniqueId>` (`T1:3666`) |
| 3 | T1 | `04 02` | GET_DEVICE_INFO FW_VERSION | `05 02 <pstr fw>` (`T1:3692`) |
| 4 | T1 | `04 01` | GET_DEVICE_INFO MODEL_NAME | `05 01 <pstr name>` (`T1:3716`) |
| 5 | T1 | `04 03` | GET_DEVICE_INFO SERIES_AND_COLOR | `05 03 <ModelSeries> <ModelColor>` (`T1:3729`; series PREMIUM=0x30 `T1:1108`) |
| 6 | T1 | `06 00` | CONNECT_GET_SUPPORT_FUNCTION | `07 00 <count> {functionType, priority}*count` (`T1:3762`, `V2:246-250`) |
| 7 | T2 | `06 00` | Table 2 support functions (only if table2 is enabled) | same layout, T2 FunctionType (`V2:255-316`) |

Then, each step gated on the support list:
- General settings 1..4 (`D1..D4`): `D0 <gs 0x01..0x04> <lang ENGLISH>` and `D6 <gs>` (L42-70). These are the
  device-defined toggles/lists. XM6 "Touch sensor", "Capture voice during a call" and similar are **likely here**
  (**inferred**; the device sends the name strings). **Correction (section 8.4):** the `<gs>` byte is `0xD1..0xD4`, not `0x01..0x04`.
- DSEE capability `E0 01` (L72-74), assignable keys capability `F0 <0x03|0x0E>` (L76-82).
- Alerts: `94 <FIXED_MESSAGE> 00` (ALERT_SET_STATUS enable) (L84-86).
- Codec `12 02` (L88-91). Playback `A6 01`, volume `A6 20`, play status `A2 01` (L93-101).
- NC/ASM get param `66 17`, `66 19` or `66 22`, depending on the support function (L103-125, see §4).
- Peripheral (T2): `32 02`/`36 02` or `32 00`/`36 00`, and source switch `36 01` (L126-165).
- Speak-to-Chat: `F6 0C`, `FA 0C` (L167-173). Listening modes: `E0/E6/E2 <03|09,04,06,07>` (L175-198).
- EQ: `50 <eqType> <lang>`, `5A 00`, `52 00`, `56 00` (L200-210). Connection mode `E6 <00|02|05>` (L212-218). DSEE `E2 01`, `E6 01` (L220-225).
- Voice assistant `F0/F2/F6 04` (L235-246). NC/AMB button `66 30` (L248-251).
- Power: head gesture `F6 0F`, auto-off `20 04`/`26 04`, wearing auto-off `20 05`/`26 05`, auto-pause `F6 01` (L253-275).
- Wearing status (T2) `F2 00` (L277-279). Voice guidance (T2) `46 01`, `46 20` (L281-292).
- Operation log on: `C4 <TIME_SERIES_OPERATIONLOG_NOTIFIER> 00` (L294-299).

Periodic sync (`RequestSyncV2`, L307-378): battery `22 00`/`22 08`, safe listening `5A 00/02` (T2), playback, wearing.

---

## 4. Features: IDs, layouts, XM6 status

Notation: `Get → Ret / Ntfy`. NTFY is pushed by the device unsolicited and has the same layout as RET with the NTFY command ID.
The T1 command table is `T1:376-530` and the T2 command table is `T2:23-102`. The dispatcher is `libmdr/src/HeadphonesV2T1.cpp:1236-1300`.
"XM6" = the status given in `docs/device-support/WH-1000XM6.md`.

### 4.1 Battery — XM6 ✅ (T1, POWER 0x20-0x29)
- Get `22 <PowerInquiredType>` (`T1:7017`). Ret `23`, Ntfy `25`.
- `BATTERY` (0x00): `23 00 <level 0-100> <BatteryChargingStatus>` (`T1:7556-7568`).
- `BATTERY_WITH_THRESHOLD` (0x08): `23 08 <level> <charging> <threshold>` (`T1:7590-7603`).
- `BatteryChargingStatus`: NOT_CHARGING 0x00, CHARGING 0x01, UNKNOWN 0x02, CHARGED 0x03 (`T1:313-320`).
- The variant is picked by function `BATTERY_LEVEL_INDICATOR` 0x20 vs `BATTERY_LEVEL_WITH_THRESHOLD` 0x28 (`HeadphonesV2.cpp:311-319`).
  The repo does not say which one the XM6 reports. Support both.
- `PowerInquiredType` full list: `T1:1324-1343`.

### 4.2 NC / Ambient sound — XM6 ✅ "supports new auto ambient level" (T1, NCASM 0x60-0x69)
- Get `66 <NcAsmInquiredType>`, Ret `67`, Set `68`, Ntfy `69`. Inquired types are listed at `T1:1131-1148`.
- Selection (`HeadphonesV2.cpp:103-125` init, `405-457` commit):
  - function `0x6D` MODE_NC_ASM_..._NOISE_ADAPTATION, inquired type **`0x19`** (`..._SEAMLESS_NA`). Layout (`T1:6040-6061`):
    `68 19 <valueChange> <totalEffect> <ncAsmMode> <ambientMode> <ambLevel> <autoAsm NcAsmOnOff> <noiseAdaptiveSensitivity>`
  - function `0x6B`, inquired type `0x17`. Same layout without the last 2 bytes (`T1:6018-6035`)
  - function `0x67`, inquired type `0x22` ASM only: `68 22 <valueChange> <totalEffect> <ambientMode> <ambLevel>` (`T1:5998-6013`)
  - **inferred:** XM6 uses `0x19`, because only that variant carries the auto-ambient field (`ncAsmOnOffValue`, which the client maps to
    `mNcAsmAutoAsmEnabled`, `HeadphonesV2.cpp:424-426`, `HeadphonesV2T1.cpp:151-171`). The docs' "new auto ambient level" matches this.
    Note an init quirk: if a device lists both 0x6B and 0x6D, init queries `0x17` first (L103-110) but commit sends `0x19` (L410).
- Values:
  - `valueChange` UNDER_CHANGING 0x00 (slider dragging) / CHANGED 0x01 (`T1:1728`)
  - `totalEffect` NcAsmOnOffValue OFF 0x00 / ON 0x01; `ncAsmMode` NC 0x00 / ASM 0x01 (`T1:1150-1162`)
  - `ambientMode` NORMAL 0x00 / VOICE 0x01, i.e. **focus on voice** (`T1:222`)
  - `ambLevel` UInt8. The UI range is **1..20** (`kAmbientLevelRange`, `libmdr/src/Headphones.cpp:1901`; V2 uses the constant, L1930)
  - `NoiseAdaptiveSensitivity` STANDARD 0x00, HIGH 0x01, LOW 0x02 (`T1:1204-1210`)
  - "Off" = totalEffect OFF. NC = ON + mode NC. Ambient = ON + mode ASM.
- **NC/AMB button cycle** (XM6 ✅): `66 30` / `68 30 <Function>` (`T1:6114-6124`), gated on function `0x69`. `Function` values:
  NC_ASM_OFF 0x01, NC_ASM 0x02, NC_OFF 0x03, ASM_OFF 0x04 … (`T1:890-939`).
- Capability (ambient level steps per mode): `60 19` → `61 19 <PodArray AmbientSoundModeLevelSet>` (`T1:10785`).
- Function `AUTO_NCASM` 0x70 / `ADAPTIVE_CONTROL…` 0x71 exist in the enum (`V2:180-181`), but the client has **no** code for them.

### 4.3 Equalizer — XM6 ✅ with caveats (T1, EQEBB 0x50-0x5B)
- XM6 answers capability type **`PRESET_EQ_AND_ERRORCODE` (0x04)** with "10 bands, 13 steps, 9 presets" (`libmdr/src/HeadphonesV2T1.cpp:1096`).
  The capability query is `50 04 <DisplayLanguage>` (`T1:3807`), chosen by `eqPresetInquiredType` (`libmdr/src/DetailsV2.hpp:425-438`).
  Reply: `51 04 <band> <step> <MDRArray{presetId, pstr name}>` (`T1:10567-10580`, `T1:4300`).
- Band frequencies: `5A 00` → `5B 00 <PodArray {EqBandInformationType, Int16BE freq}>` (`T1:10600`, `T1:3784-3790`). For XM6:
  10 × HZ {31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000} (`HeadphonesV2T1.cpp:1118`).
- Get param `56 00` → `57 00 <presetId> <PodArray<UInt8> bandSteps>` (`T1:10639`, `EqParam T1:4280-4286`). Ntfy `59`.
- **Set:** `58 00 <presetId> <count> <steps…>` (`T1:10691`, `HeadphonesV2.cpp:746-790`). Set preset only = `58 00 <id> 00`
  (empty steps, L748-755). The client then re-reads with `56 00`.
- **Steps:** raw = value + offset. With 13 steps the offset is 6, so the range is **-6..+6**, raw 0..12 (`DetailsV2.hpp:385-392`, `Headphones.cpp:1903, 1997`).
- **Clear Bass on XM6:** the XM6 param has 10 values = 10 band-info entries, so there is **no Clear Bass slot** (`DetailsV2.hpp:396-409`).
  Clear Bass exists only in the 6-value layout (CB + 5 bands, -10..10), used by XM4/XM5-class devices (`Headphones.cpp:1902-1904`).
- Status `52 00` → `53 00 <EnableDisable> <PodArray errorCodes>` (`T1:4167`). EQ may be disabled, e.g. while a listening mode is active.
- Preset IDs (`T1:754-815`): OFF 00, ROCK 01 … BRIGHT 10 … HEAVY 30, CLEAR 31, HARD 32, SOFT 33, CUSTOM A0, USER_SETTING1-5 A1-A5.
  **XM6 note:** only 0x30-0x33, 0xA0 and the user settings are accepted as of FW 3.0.0 (`docs/device-support/WH-1000XM6.md`, PR #48).

### 4.4 DSEE — gated on function `UPSCALING_AUTO_OFF` 0xE2 (T1, AUDIO 0xE0-0xE9)
- Get `E6 01` → `E7 01 <UpscalingTypeAutoOff>`. Set `E8 01 <OFF 0x00 | AUTO 0x01>` (`T1:3276-3286, 2973`, enum `T1:1721`).
  Status `E2 01` → available/disabled (`HeadphonesV2.cpp:220-225, 819-833`).
- "DSEE Extreme" vs "Auto" has no separate encoding: the only values are OFF and AUTO. A variant with a disable reason exists
  (`0x0B`, `T1:3127`) but is unused. The XM6 doc table does not list DSEE, so it is **untested** for XM6.

### 4.5 Speak-to-Chat — XM6 ✅ (T1 SYSTEM, function `SMART_TALKING_MODE_TYPE2` 0xFC)
- Enable: get `F6 0C`, set `F8 0C <OnOff> <previewOnOff>` with ON=0x00. The client always sends preview OFF (0x01)
  (`T1:9870-9882`, `HeadphonesV2.cpp:599-613`).
- Sensitivity/timeout: get `FA 0C`, set `FC 0C <DetectSensitivity> <ModeOutTime>` (`T1:9742-9754`, `HeadphonesV2.cpp:615-627`).
  `DetectSensitivity` AUTO 0x00, HIGH 0x01, LOW 0x02 (`T1:605-611`). `ModeOutTime` FAST 0x00 (short), MID 0x01, SLOW 0x02, NONE 0x03 (manual off) (`T1:1099-1106`).

### 4.6 Auto power off — XM6 ✅ "Power Off" (T1 POWER)
- Timer: function 0x24. Capability `20 04` → `21 04 <PodArray candidates>` (`T1:7382`). Get `26 04`.
  Set `28 04 <current> <lastSelect>` (`T1:7772-7784`; the client sends lastSelect=0x00, `HeadphonesV2.cpp:874-887`).
  `AutoPowerOffElements`: 5 min 0x00, 30 min 0x01, 60 min 0x02, 180 min 0x03, 15 min 0x04, DISABLE 0x11 (`T1:280-289`).
- With wearing detection: function 0x25, inquired type `0x05`, same layout (`T1:7789-7801`). Its element enum adds
  `POWER_OFF_WHEN_REMOVED_FROM_EARS 0x10` (`T1:291-301`). Use the candidate list from the capability reply.
- Power off now: `24 03 01` (POWER_SET_STATUS, POWER_OFF, USER_POWER_OFF) (`T1:7866-7876`, `HeadphonesV2.cpp:396-403`).
  **Never send `24 03 FF`** (FACTORY_POWER_OFF, `T1:1345-1350`).

### 4.7 Auto pause on removal — function `PLAYBACK_CONTROL_BY_WEARING_REMOVING_HEADPHONE_ON_OFF` 0xF1
- Get `F6 01`, set `F8 01 <OnOff>` (SystemSetParamCommon `T1:9813-9823`, `HeadphonesV2.cpp:902-915`).
- Head gesture (function 0xFF): `F6 0F` / `F8 0F <OnOff>` (`HeadphonesV2.cpp:858-872`).

### 4.8 Wearing status (T2 SYSTEM)
- Get `F2 00` (type 0x0E) → `F3 00 <WearingStatusCode>`. Pushed as `F5 00 <code>` (`T2:2922, 3313, 3800`).
  Codes: NORMAL 0x00, ILLEGAL 0x01, LEFT_NOT_WEAR 0x02, RIGHT_NOT_WEAR 0x03, BOTH_NOT_WEAR 0x04 (`T2:674-682`).
- The client queries it when Table 2 exists and any of T2 `WEARING_STATUS_CHECKER` 0xF0, T1 `WEARING_STATUS_DETECTOR` 0xF6 or T1 0xF1 is listed,
  because 0xF0 alone is not reliable (PR #63) (`libmdr/src/DetailsV2.hpp:371-379`). Not listed in the XM6 docs.

### 4.9 Multipoint and paired devices — XM6 ✅ "Multipoint Control" (T2 PERIPHERAL 0x30-0x3D, type 0x0E)
- Inquired types: CLASSIC_BT 0x00, SOURCE_SWITCH_CONTROL 0x01, WITH_BT_CLASS_OF_DEVICE 0x02 (`T2:332-339`). Type 0x02 is used if
  T2 function 0x32/0x33 is listed, otherwise 0x00 if 0x30 is listed (`HeadphonesV2.cpp:126-158, 529-551`).
- **Device list:** `36 02` → `37 02 <MDRArray{mac[17 ASCII "XX:XX:.."], connected u8, CoD Int24BE, pstr name}> <playbackRightDevice u8>`
  (`T2:4823-4834, 1639-1647`). The type 0x00 variant has no CoD (`T2:4809, 1650`).
- **Pairing mode status:** `32 02` → `33 02 <PeripheralBluetoothMode> <EnableDisable>` (`T2:1902`).
- **Multipoint toggle** (source switch control, T2 function 0x31): get `36 01`, set `38 01 <0|1>` (`T2:1985-1994`; the client sends 1=enabled,
  `HeadphonesV2.cpp:511-527`). The polarity here is a raw u8, **not** OnOffSettingValue.
- **Switch the playing source:** `3C 01 <17-byte ASCII MAC>` (`T2:1945-1954`, `HeadphonesV2.cpp:496-509`).
- **Connect/disconnect/unpair:** `3C <type> <ConnectivityActionType> <17-byte MAC>` (`T2:1931-1942`, `T2:135-141`).
  DISCONNECT 0x00, CONNECT 0x01, **UNPAIR 0x02 = dangerous** (`HeadphonesV2.cpp:575-581`).
- **Pairing mode:** `34 <type> <INQUIRY_SCAN 0x01 | NORMAL 0x00> 00` (`T2:2012-2024`, `HeadphonesV2.cpp:585-596`). This makes the headset discoverable,
  so treat it as sensitive.

### 4.10 Device info (read-only) — §3 steps 3-5
- FW version string, model name string (e.g. "WH-1000XM6") and series/color.
- **The MAC is not read over MDR.** The client already has it from OS enumeration (`ConnectionWindows.cpp` GetDevicesList).
  The capability `uniqueId` string (`T1:3666`) is an opaque ID.
- `COMMON` `DISPLAY_FW_VERSION` 0x09 and function 0x1B exist (`T1:539-552`) but are unused.

### 4.11 Codec — function `CODEC_INDICATOR` 0x12
- Get `12 02` → `13 02 <AudioCodec>`, Ntfy `15 02 …` (`T1:3505`, `HeadphonesV2.cpp:88-91`).
  SBC 0x01, AAC 0x02, LDAC 0x10, aptX 0x20, aptX HD 0x21, LC3 0x30, UNSETTLED 0x00, OTHER 0xFF (`T1:229-240`).
- Connection-quality priority mode: `E6/E8 <00|02|05> …` (`DetailsV2.hpp:443-457`, `HeadphonesV2.cpp:792-817`).

### 4.12 Volume and playback — XM6 ✅ (T1 PLAY 0xA0-0xA9, gated on 0xA1-0xA4)
- Volume: get `A6 20` → `A7 20 <vol>`. Set `A8 20 <vol>` (`T1:8700-8710, 7987`; commit `HeadphonesV2.cpp:472-482`). Range **0..30** (`Headphones.cpp:1900`).
- Play status: `A2 01` → `A3 01 <EnableDisable> <PlaybackStatus> <MusicCallStatus>` (`T1:8087-8101`). PlaybackStatus: UNSETTLED 0, PLAY 1, PAUSE 2, STOP 3 (`T1:1308`).
- Control: `A4 01 00 <PlaybackControl>`: PAUSE 0x01, TRACK_UP 0x02 (next), TRACK_DOWN 0x03 (prev), PLAY 0x07 … (`T1:8728-8740, 1285-1298`; mapping in `Headphones.cpp:777-797`).
- Metadata: `A6 01` → `A7 01 <4 × {PlaybackNameStatus, pstr}>` (title/album/artist/…) (`T1:10895-10904, 6980`).

### 4.13 Touch / custom button — XM6 ✅ "Touch Sensor Enable/Disable", "NC/AMB Button Setting"
- Assignable keys (functions 0xF3/0xFE): get `F6 03|0E`, set `F8 <03|0E> <PodArray<Preset>>` (one per key, `T1:9771-9792`).
  `Preset` values: AMBIENT_SOUND_CONTROL 0x00, VOLUME 0x10, PLAYBACK 0x20, TRACK 0x21, GOOGLE 0x31, ALEXA 0x32, QUICK_ACCESS 0x36, NO_FUNCTION 0xFF … (`T1:1359-1389`).
  Read the allowed combos from `F0 <type>` first.
- The touch sensor on/off is most likely a General Setting boolean (**inferred**): `D8 <gs> 00 <ON 0x00|OFF 0x01>` (`T1:4577`), or a list `D8 <gs> 01 <idx>` (`T1:4594`).
  Labels come from `D1` replies (`T1:4463`). Commit code is at `HeadphonesV2.cpp:945-982`.

### 4.14 Sound pressure / safe listening — XM6 ✅ (T2, functions 0x50-0x53)
- Get `5A <00 HBS_1|02 HBS_2>` → `5B <type> <levelPerPeriod u8 dB> <errorCause>` (`T2:2413, 2604-2616`).
  Headphones use HBS. Which of HBS_1/HBS_2 the XM6 uses is not stated.
- Set `58 <type> <safeListeningMode OnOff> <preview OnOff>` (`T2:2729-2741`).

### 4.15 Voice guidance — XM6 ✅ volume (T2 0x40-0x48)
- On/off: get `46 01`, set `48 01 <OnOff>` (`T2:4635-4645`).
- Volume (T2 function 0x42): get `46 20` → `47 20 <Int8>`. Set `48 20 <vol Int8> <feedbackSound OnOff>`. Range **-2..+2**
  (`T2:4682-4694`, `Headphones.cpp:1905`).

### 4.16 Other (XM6 ✅ in docs)
- **Listening mode** (function 0xE6): BGM `E8 <03|09> <OnOff> <roomSize>`, Upmix cinema `E8 04`, voice contents `E8 06`, sound-leak reduction `E8 07` (`HeadphonesV2.cpp:666-744`).
  **XM6 hazard:** turning on Background Music while Voice Control is active **reboots the headset** (issue #76). The client re-reads status `E2 <type>` right before
  turning a mode on and refuses unavailable modes (L676-693).
- **Voice assistant** (function 0xF4): `F0/F2/F6 04`, set via `SystemSetParamVoiceAssistantSettings` (`HeadphonesV2.cpp:629-664`). XM6 also pushes `VOICE_ASSISTANT_WAKE_WORD`
  (0x05) without advertising it (`HeadphonesV2T1.cpp:1009`).
- Alerts: `98 <FIXED_MESSAGE> <messageType> <action>` answers device prompts (`HeadphonesV2.cpp:384-394`, `T1:2232`).
- Logs: `C4 …`, Ntfy `C9` (`HeadphonesV2T1.cpp:1197`).

---

## 5. Denylist: never send

| Table/type | Bytes | What |
|---|---|---|
| T1 0x0C | cmd **0x30-0x3F** (`UPDT_*`: 30,31,32,33,**34**,35,36,37,**38**,39,**3E** TRANSFER_DATA,3F) | Firmware update / DFU (`T1:404-415`; structs `UpdtSetStatus T1:10428`, `UpdtSetParamTandemExecuteFwUpdate T1:10396`). Block the whole range, including GETs. |
| T1 | `F8 09 xx` | SYSTEM_SET_PARAM RESET_SETTINGS: `00` settings reset, **`01` FACTORY_RESET** (`T1:9855-9865, 1431-1436`) |
| T1 | `24 03 FF` | POWER_SET_STATUS FACTORY_POWER_OFF (`T1:1345-1350`). Only `24 03 01` is a normal power off. |
| T1 | `24 07 …` | POWER_SET_STATUS LINK_CONTROL (`PowerSetStatusLinkControl`); purpose unclear, so avoid |
| T2 0x0E | `3C <00|02> 02 <mac>` | PERI_SET_EXTENDED_PARAM **UNPAIR** (deletes the pairing) (`T2:1931-1942`, `T2:135-141`). Task 0011 widens this to every `3C` form. |
| T2 | `34 <type> 01 …` | Enter pairing (inquiry-scan) mode: disruptive, allow only on explicit user action |
| T1 | `64` NCASM_SET_STATUS NC_TEST_MODE (`T1:1146` type 0x40), `F4` SYSTEM_SET_STATUS training/test modes (FACE_TAP_TEST_MODE 0x0B, HEAD_GESTURE_TRAINING 0x10) | factory/test modes |
| T1 | `0F` GET_TEST | test command (`T1:386`) |
| T1/T2 | LEA (T1 0x40-0x4D, T2 0x60-0x69), PARTY (T2 0x70-0x7C) | not needed; can change connection profiles |
| any | data types other than 0x01/0x0C/0x0E (e.g. 0x2D LARGE_DATA_COMMON) | not used by the client |

Safe approach: **allowlist** only the GET/SET pairs in §4 instead of denylisting.

---

## 6. XM6 vs XM5 / XM4 in the client

- **No model detection or model tables in the library.** The protocol family comes from the RFCOMM UUID (V2 = XM5/XM6, V1 = XM4 and older, `Base.h:47-51`, `Client.cpp:1180-1188`).
  Features come from the support-function lists. The model name string is only displayed.
- **XM4 = V1**: a different command table (`libmdr/include/mdr/Generated/ProtocolV1T1.hpp:212+`) and different code (`libmdr/src/HeadphonesV1*.cpp`).
  The framing in §2 is shared (`Command.cpp`). V1 uses Clear Bass + 5 bands and capability-driven ambient steps (`Headphones.cpp:1921-1928`).
- **XM6-specific facts in the source:**
  - EQ `PRESET_EQ_AND_ERRORCODE`, 10 bands / 13 steps (-6..6), 9 presets, **no Clear Bass slot** in the param (`HeadphonesV2T1.cpp:1096, 1118`; `DetailsV2.hpp:396-409`; `Headphones.cpp:1939, 1997`).
  - Only presets 0x30-0x33, 0xA0 and user settings are accepted (FW 3.0.0) (docs).
  - NC with the auto-ambient field, most likely `0x19` (NA) (docs plus code, **inferred**).
  - Unadvertised `VOICE_ASSISTANT_WAKE_WORD` push (`HeadphonesV2T1.cpp:1009`). BGM + Voice Control reboot (#76) (docs; `HeadphonesV2.cpp:686-693`).
- **XM5 doc** (`docs/device-support/WH-1000XM5.md`): voice guidance volume ❌ and capture voice ❌ on XM5, both ✅ on XM6; playing-track notify is broken on XM5.
- **Unknowns to verify on a real XM6:** the exact T1/T2 support-function list, which battery variant (0x00 vs 0x08), HBS_1 vs HBS_2, whether DSEE and wearing status are advertised,
  and which General Settings slots map to touch sensor / capture voice.
  Tip: dump the `07 00` replies (T1 and T2) once and gate everything on them, the same way the client does.
  **Settled by the capture in section 8** (except the General Settings slot names).

---

## 7. File index

| What | Path |
|---|---|
| UUIDs | `libmdr/include/mdr-c/Base.h:47-53` |
| Framing constants / pack / unpack | `libmdr/include/mdr/Command.hpp`, `libmdr/src/Command.cpp` |
| Data types, BE ints, string/array encodings | `libmdr/include/mdr/Protocol.hpp:44-63, 445-640` |
| Seq/ACK/retry/timeouts | `libmdr/src/Headphones.cpp:11-30, 181-333`; `libmdr/src/Details.hpp:465-474, 500-620` |
| V2 init/sync/commit (what is sent when) | `libmdr/src/HeadphonesV2.cpp` |
| V2 RX handlers T1 / T2 | `libmdr/src/HeadphonesV2T1.cpp` (dispatch L1236-1300), `libmdr/src/HeadphonesV2T2.cpp` |
| Support-function helpers, EQ layout logic | `libmdr/src/DetailsV2.hpp:200-490` |
| Value ranges | `libmdr/src/Headphones.cpp:1900-1905` |
| V2 shared enums + FunctionType T1/T2 | `libmdr/include/mdr/Generated/ProtocolV2.hpp:7-316` |
| V2 Table 1 commands/structs/enums | `libmdr/include/mdr/Generated/ProtocolV2T1.hpp` (Command enum L376-530) |
| V2 Table 2 commands/structs/enums | `libmdr/include/mdr/Generated/ProtocolV2T2.hpp` (Command enum L23-102) |
| Data type per struct | `libmdr/include/mdr/Generated/ProtocolV2T1Traits.hpp`, `...ProtocolV2T2Traits.hpp` |
| (De)serialization | `libmdr/src/Generated/ProtocolV2T1Serialization.cpp`, `...T2Serialization.cpp`, `libmdr/src/ProtocolV2T1CustomSerialization.cpp` |
| V1 (XM4) tables | `libmdr/include/mdr/Generated/ProtocolV1T1.hpp`, `ProtocolV1T2.hpp`; `libmdr/src/HeadphonesV1*.cpp` |
| Windows RFCOMM | `libmdr-bt/src/Windows/ConnectionWindows.cpp` |
| Windows BLE (WinRT) | `libmdr-bt/src/Windows/ConnectionWindowsBLE.cpp` |
| Device support notes | `docs/device-support/WH-1000XM6.md`, `WH-1000XM5.md` |
| Packet captures (no XM6) | `tests/WF-1000XM5-6.1.0`, `tests/WH-1000XM3-4.5.2`, `tests/WF-LC900-*` |

---

## 8. XM6 capture (firmware 3.1.5)

Source: `tests/fixtures/xm6-probe.txt` (our `--probe` dump, anonymised: MAC, unique id, paired-device names and MACs are placeholders),
taken 2026-11-21 on a WH-1000XM6, firmware 3.1.5, protocol version `0x03003032`, series/color `0x30`/`0x05`, Table 2 enabled.
Names below come from the reference enums (`V2:104-244` for T1, `V2:255-310` for T2); struct layouts from `T1`/`T2` as cited in section 4.
"Reference" means read from the source, not seen on the wire. Tests should take their reply bytes from the fixture.

Reading notes:
- Reply payloads below are the payload only (no frame bytes). `T1` frames use data type `0x0C`, `T2` frames `0x0E`.
- The headset ACKs every command, so "no reply" means ACKed and then silent (not a lost frame). The probe waited 5 s.
- **A reply does not have to repeat the request type:** `5A 00` (T2) was answered by `5B 02 ...`. Correlate replies by reply command id, not by echoing the request bytes.

### 8.1 Table 1 support functions (45 entries; priority omitted, 255 = no priority)

| Id | Reference name | MVP use |
|---|---|---|
| 0x10 | CONCIERGE_DATA | no |
| 0x12 | CODEC_INDICATOR | codec read (8.3), no feature task |
| 0x13 | UPSCALING_INDICATOR | no |
| 0x14 | BLE_SETUP | no |
| 0x15 | TUTORIAL_CONTENTS_SELECT_ON_CONCIERGE | no |
| 0x16 | CONNECTION_ESTABLISHED_TIME | no |
| 0x17 | UNNECESSARY_AUTO_RECONNECTION | no |
| 0x19 | PHONE_AND_CONNECTED_DEVICE_INFOMATION_FOR_CLASSIC | no |
| 0x20 | BATTERY_LEVEL_INDICATOR | 0005 (variant `22 00`) |
| 0x23 | POWER_OFF | no (power-off-now stays unused) |
| 0x25 | AUTO_POWER_OFF_WITH_WEARING_DETECTION | 0010 |
| 0x27 | TANDEM_KEEP_ALIVE | no |
| 0x34 | FW_UPDATE_MTK_TRANSFER_WITHOUT_DISCONNECTION_AUTO_UPDATE | never (firmware update, denylisted) |
| 0x41 | HBS_SUPPORTS_A2DP_LEA_UNI_LEA_BROAD_WITH_CTKD | no |
| 0x42 | CLASSIC_ONLY_LE_CLASSIC_SETTING | no |
| 0x44 | CHANGE_TANDEM_CONNECTION_PROFILE_FOR_ANDROID | no |
| 0x56 | TURN_KEY_EQ | no (EQ uses 0x57) |
| 0x57 | PRESET_EQ_AND_ERRORCODE | 0007 (capability type `04`) |
| 0x69 | AMBIENT_SOUND_CONTROL_MODE_SELECT (NC/AMB button) | no (not captured) |
| 0x6D | MODE_NC_ASM_NOISE_CANCELLING_DUAL_AMBIENT_SOUND_MODE_LEVEL_ADJUSTMENT_NOISE_ADAPTATION | 0006 (type `19`) |
| 0x71 | ADAPTIVE_CONTROL_WITH_PARAMETER_NOTIFICATION | no (reference has no code) |
| 0x90 | FIXED_MESSAGE (alerts) | no |
| 0x94 | LE_AUDIO_ALERT_NOTIFICATION | no |
| 0xA1 | PLAYBACK_CONTROLLER_WITH_CALL_VOLUME_ADJUSTMENT | no |
| 0xB0 | SAR | no |
| 0xB2 | GATT_CONNECTABLE | no |
| 0xB6 | SAR_OPTIMIZATION_ACCEL_TYPE | no |
| 0xB7 | HEAD_TRACKER_ACCEL_TYPE | no |
| 0xB8 | INTEGRATED_AUTO_PLAY | no |
| 0xC1 | ACTION_LOG_NOTIFIER | no |
| 0xC2 | TIME_SERIES_OPERATIONLOG_NOTIFIER | no |
| 0xD1 | GENERAL_SETTING_1 | no (see 8.4) |
| 0xD2 | GENERAL_SETTING_2 | no |
| 0xD3 | GENERAL_SETTING_3 | no |
| 0xE2 | UPSCALING_AUTO_OFF (DSEE) | 0008 |
| 0xE5 | UPMIX_CINEMA | no (listening mode, forbidden) |
| 0xE6 | LISTENING_OPTION | no (forbidden: reboot hazard) |
| 0xE7 | CONNECTION_MODE_CLASSIC_AUDIO_LE_AUDIO | no |
| 0xEB | BGM_MODE_SMALL_MIDDLE_LARGE_AND_ERRORCODE | no (forbidden) |
| 0xF1 | PLAYBACK_CONTROL_BY_WEARING_REMOVING_HEADPHONE_ON_OFF | 0010 (auto pause) |
| 0xF4 | VOICE_ASSISTANT_SETTINGS | no |
| 0xF9 | RESET_SETTINGS | never (`F8 09 xx` is denylisted) |
| 0xFC | SMART_TALKING_MODE_TYPE2 | 0009 |
| 0xFD | QUICK_ACCESS | no |
| 0xFF | HEAD_GESTURE_ON_OFF_TRAINING | no |

Not advertised, so absent on XM6 and not to be implemented: 0x24 AUTO_POWER_OFF (timer), 0x28 BATTERY_LEVEL_WITH_THRESHOLD, 0x6B / 0x67 (older NC variants),
0x50 / 0x52 / 0x53 (other EQ variants), 0x21/0x22 L-R and case battery, 0xF6 WEARING_STATUS_DETECTOR, 0xD4 GENERAL_SETTING_4.

### 8.2 Table 2 support functions (16 entries)

| Id | Reference name | MVP use |
|---|---|---|
| 0x21 | CHARGE_IN_USE | no |
| 0x23 | USB_SUBMERSION | no |
| 0x31 | SOURCE_SWITCH_CONTROL (multipoint) | 0011 |
| 0x33 | PAIRING_DEVICE_MANAGEMENT_WITH_BLUETOOTH_CLASS_OF_DEVICE_CLASSIC_LE | 0011 (list type `02`) |
| 0x34 | MUSIC_HAND_OVER_SETTING | no |
| 0x42 | VOICE_GUIDANCE_SETTING_MTK_..._SUPPORT_LANGUAGE_SWITCH_AND_VOLUME_ADJUSTMENT | no |
| 0x52 | SAFE_LISTENING_HBS_2 | no |
| 0x54 | SAFE_VOLUME_CONTROL | no |
| 0x60 | LE_AUDIO_CONNECTION_STATE_NOTIFICATION | no |
| 0x61 | LE_AUDIO_SWITCH_SUPPORTED_COMPATIBILITY | no |
| 0x63 | GET_IDENTITY_RESOLVING_KEY | no |
| 0xF2 | QUICK_ACCESS_EASY_SETTING | no |
| 0xF5 | SONY_VOICE_ASSISTANT | no |
| 0xF8 | LINK_AUTO_SWITCH_FOR_HEADSETS | no |
| 0xF9 | MIC_ON_OFF_BY_HEADPHONE_OPERATION | no |
| 0xFD | VOICE_ASSISTANT_WITH_SPECIFIC_SETUP_LINK_SUPPORT | no |

Not advertised: 0x30 and 0x32 (other pairing-list variants), 0xF0 WEARING_STATUS_CHECKER, 0x50 SAFE_LISTENING_HBS_1.
Wearing status (T2 `F2 00`) is still answered; per the reference rule it is queried because T1 0xF1 is advertised (section 4.8).

### 8.3 Decoded feature reads

Polarity key: **OnOff** = `OnOffSettingValue` (ON `00`, OFF `01`); **EnableDisable** (ENABLE `00`, DISABLE `01`); **NcAsmOnOff** (OFF `00`, ON `01`);
**Upscaling** (OFF `00`, AUTO `01`); **raw** = plain u8 (`01` = on, `00` = off).

| Feature | Command | Reply (payload) | Meaning |
|---|---|---|---|
| Protocol info | T1 `00 00` | `01 00 03 00 30 32 00 00` | version `0x03003032`; table 1 ENABLE, table 2 ENABLE |
| Capability info | T1 `02 00` | `03 00 00 11 <17 ASCII>` | counter 0, unique id (17-char string, private; placeholder in the fixture) |
| Firmware | T1 `04 02` | `05 02 05 33 2E 31 2E 35` | "3.1.5" |
| Model | T1 `04 01` | `05 01 0A "WH-1000XM6"` | "WH-1000XM6" |
| Series / color | T1 `04 03` | `05 03 30 05` | series `0x30`, color `0x05` |
| Support functions | T1 / T2 `06 00` | `07 00 2D ...` / `07 00 10 ...` | 45 / 16 entries, see 8.1 / 8.2 |
| Battery | T1 `22 00` | `23 00 61 00` | level 0x61 = **97 %**, NOT_CHARGING. `22 08` not advertised, never sent. Ntfy `25` (reference) |
| NC / ambient | T1 `66 19` | `67 19 01 01 00 00 14 00 00` | valueChange CHANGED, totalEffect **ON**, mode **NC**, ambientMode NORMAL, ambient level `0x14` = **20**, autoAsm OFF, sensitivity STANDARD. So NC is on; the level is the remembered ambient level. `66 17` / `66 22` not advertised. Ntfy `69` (reference) |
| EQ band info | T1 `5A 00` | `5B 00 0A` then 10 x `01 <freq u16 BE>` | 10 bands, every entry type HZ (`01`): 31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000 Hz (1 kHz and up are still in Hz: `03 E8` = 1000). The `3E 80` pair in the frame is escaped on the wire (`3D 2E 80`) |
| EQ parameters | T1 `56 00` | `57 00 32 0A 0A 0A 05 05 06 06 06 08 08 09` | preset `0x32` (HARD in the reference enum), then count `0x0A` and 10 raw steps `0A 0A 05 05 06 06 06 08 08 09`. With offset 6 (reference): +4 +4 -1 -1 0 0 0 +2 +2 +3 dB for 31 Hz ... 16 kHz. The steps are reported for a factory preset too (it is the preset's own curve). Ntfy `59` (reference) |
| EQ status | T1 `52 00` | **no reply** (ACKed, timed out) | XM6 does not answer EQ status. Do not send it; treat EQ as enabled unless an Ntfy `53`/`55` says otherwise |
| DSEE parameter | T1 `E6 01` | `E7 01 01` | Upscaling = **AUTO** (DSEE on). Set is `E8 01 <00 OFF | 01 AUTO>`; Ntfy `E9 01 <value>` (reference) |
| DSEE status | T1 `E2 01` | `E3 01 00` | EnableDisable ENABLE = available (the reference reads `ENABLE` as available). Ntfy `E5 01 <status>` (reference) |
| Speak-to-Chat enable | T1 `F6 0C` | `F7 0C 01 01` | OnOff `01` = **OFF**, preview OFF. Set `F8 0C <OnOff> <preview>` (preview always `01`); Ntfy `F9 0C <OnOff> <preview>` (reference) |
| Speak-to-Chat sensitivity | T1 `FA 0C` | `FB 0C 00 01` | sensitivity AUTO (`00`), mode-out time MID (`01`). Set `FC 0C <sens> <time>`; Ntfy `FD 0C ...` (reference) |
| Auto power-off (timer) | T1 `20 04`, `26 04` | not sent (0x24 not advertised) | no minute timers on XM6 |
| Auto power-off (wearing), capability | T1 `20 05` | `21 05 02 10 11` | candidate list of 2: `10` POWER_OFF_WHEN_REMOVED_FROM_EARS, `11` DISABLE. No 5/15/30/60/180 min options |
| Auto power-off (wearing), parameter | T1 `26 05` | `27 05 10 00` | current = `10` (when removed from ears), last-select `00` (5 min, unused). Set `28 05 <current> 00`; Ntfy `29 05 ...` (reference) |
| Auto pause on removal | T1 `F6 01` | `F7 01 00` | OnOff `00` = **ON**. Set `F8 01 <OnOff>`; Ntfy `F9 01 <OnOff>` (reference) |
| Codec | T1 `12 02` | `13 02 02` | AAC. Ntfy `15 02 <codec>` (reference) |
| General settings 1..3 | T1 `D6 01` / `D6 02` / `D6 03` | **no reply** | wrong command: the type byte must be `D1` / `D2` / `D3` (see 8.4). The probe also skipped `D6 04` (GENERAL_SETTING_4 not advertised) |
| Wearing status | T2 `F2 00` | `F3 00 00` | code NORMAL (`00`). Ntfy `F5 00 <code>` (reference). Codes: 0 NORMAL, 1 ILLEGAL, 2 LEFT_NOT_WEAR, 3 RIGHT_NOT_WEAR, 4 BOTH_NOT_WEAR |
| Multipoint | T2 `36 01` | `37 01 01` | raw `01` = enabled (the reference reads any non-zero as enabled and sends `1` to enable). Set `38 01 <0|1>`; Ntfy `39 01 <value> <result>` (reference) |
| Paired device list | T2 `36 02` | `37 02 07 <7 entries> 01` | 7 entries, then the trailing byte `01` (playbackRightDevice, see below). Entry = 17 ASCII MAC, connected u8, class-of-device Int24 BE, length-prefixed name. In the fixture: 1st entry connected `01`, class `2A 41 0C`, name "PC-DDD"; the other six are `00` with class `FF FF FF` (unknown) and names such as "Laptop-AAAA...", "PC-EEE", "Phone-BBBB...", "Phone-CCCC...", "PC-DDD" (the repeats are an anonymisation artefact). Ntfy `39 02 ...`, same layout (reference). MACs in the fixture are the literal text `XX:XX:XX:XX:XX:XX`: validate length 17 and printable ASCII only, not hex |
| Pairing mode status | T2 `32 02` | `33 02 00 00` | mode NORMAL (`00`), EnableDisable ENABLE. Not in pairing mode (the reference: pairing = ENABLE and mode INQUIRY_SCAN) |
| Safe listening HBS_1 | T2 `5A 00` | `5B 02 00 03` (arrived as the "unsolicited" frame) | the probe saw a timeout because it waited for `5B 00`. The device answered with type `02` (HBS_2); only 0x52 (HBS_2) is advertised |
| Safe listening HBS_2 | T2 `5A 02` | `5B 02 00 03` | type HBS_2, level 0 dB, error cause `03` (the reference enum knows only 0 NOT_PLAYING, 1 IN_CALL, 2 DETACHED). Safe listening is not an MVP feature |

Unsolicited frames during the capture: one T2 frame `5B 02 00 03`, which is the late answer to `5A 00`, not a push.

### 8.4 General settings: correct command (not captured yet)

The reference's `GsInquiredType` values are `0xD1..0xD4` (`T1:941-948`), the same numbers as the function ids, so the type byte is `D1`, `D2`, `D3` for XM6 (not `01..03`):

| Purpose | Command | Reply (reference layouts) |
|---|---|---|
| Capability | `D0 <D1|D2|D3> 01` (`01` = ENGLISH, `T1:637`) | `D1 <type> <settingType 00 BOOLEAN|01 LIST> <stringFormat> <pstr subject> <pstr summary>` (`T1:4463-4494`) |
| Status | `D2 <type>` | `D3 <type> <EnableDisable>` (`T1:4547-4558`) |
| Parameter | `D6 <type>` | boolean: `D7 <type> 00 <OnOff>`; list: `D7 <type> 01 <index 0..63>` (`T1:4511-4543`) |
| Set | `D8 <type> 00 <OnOff>` / `D8 <type> 01 <index>` (`T1:4575-4606`) | |

These are read-only to learn what the three slots are (touch sensor, capture voice, ...). General settings are not in the MVP; a later task needs
a probe run with these commands before anything is implemented or sent.

### 8.5 Still uncertain

- **Polarity cross-check against the owner's memory of the headset state at capture time.** Decoded states: NC on (level 20, not focus on voice, auto ambient off),
  DSEE auto, Speak-to-Chat **off**, auto pause on removal **on**, auto power-off "when removed from ears", multipoint on, codec AAC, battery 97 %, wearing NORMAL.
  If the owner can say which of these match the Sony app at the time, the OnOff / EnableDisable / raw mappings are proven. Until then they are reference-derived.
- Wearing code 0 NORMAL: presumably both ear cups worn; not known whether the headset was worn during the probe.
- EQ: the capability reply (`50 04 01`) and preset names were not requested, so the 13-step / offset-6 assumption and the preset list are reference-only
  (the observed raw steps 5..10 fit -6..+6). The `52 00` silence means EQ availability cannot be read.
- Multipoint polarity (`01` = on) and the `playbackRightDevice` byte (the reference compares it with each entry's connected byte, which looks like a quirk) are unproven.
  Use only the per-entry connected flag for display.
- Safe listening error cause `03`, and why `5A 00` is answered as HBS_2.
- Ntfy layouts marked "(reference)" were not seen in the capture. Notification ids come from the command tables (`T1:376-530`, `T2:23-102`).
- The NC/AMB button (`66 30`, function 0x69) and the unlisted `ADAPTIVE_CONTROL` (0x71) payloads were not captured.
