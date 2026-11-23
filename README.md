# XM Control

A small Windows app, written in Rust, that controls Sony WH-1000XM6 headphones from a PC without the phone app.
It talks to the headphones directly over Bluetooth Classic (RFCOMM) using Sony's "MDR" protocol.

Not affiliated with or endorsed by Sony. "Sony" and "WH-1000XM6" are trademarks of their owners.

## Status

Early development. The window (pick the headset, Connect/Disconnect, model, firmware, battery and the panels below) and a
read-only developer CLI. Each control has been checked on real headphones.

| Feature | Notes |
|---|---|
| Noise cancelling / ambient sound | Off / NC / Ambient, ambient level 1-20, focus on voice, auto ambient, sensitivity |
| Equalizer | Presets (Heavy, Clear, Hard, Soft, Custom, user slots) and 10 bands from -6 to +6 dB. The XM6 has no Clear Bass slot, so none is offered |
| DSEE | Off / Auto |
| Speak-to-Chat | On/off, sensitivity, timeout |
| Power | Auto power-off when taken off, auto pause, wearing status |
| Multipoint | On/off, behind a confirmation (it can drop a connection) |
| Paired devices | Read-only list with a "connected" marker |

Only the WH-1000XM6 (firmware 3.1.5) has been tested. Other Sony headsets that speak the same protocol may partly work:
the app only shows controls the headset says it supports.

## Safety

The app can never send firmware update (DFU), factory reset, unpair, pairing-mode or connect/disconnect commands.
Every outgoing command passes a command guard: a denylist checked first, then an allowlist of exact reads and of
settings with validated arguments ([docs/adr/0003-command-guard.md](docs/adr/0003-command-guard.md)).

## Install

Build the installer yourself (there are no published releases yet). It installs per user, with no admin rights,
into `%LOCALAPPDATA%\Programs\XM Control` and adds a Start Menu shortcut.

1. Build the exe (see below).
2. With [Inno Setup 6](https://jrsoftware.org/isinfo.php):
   `ISCC.exe /DAppVersion=0.1.0 /DSourceExe=..\target\release\xm-control.exe /O..\dist packaging\installer.iss`
3. Run `dist\XM-Control-Setup-0.1.0.exe`.

The exe is not signed, so Windows SmartScreen may warn on first start.

## Use

Pair the headphones with Windows first (Settings > Bluetooth). Start XM Control, pick the headset and press Connect.
Changes made on the headset (buttons) or in another app show up in the window.
There is no tray icon and no auto-reconnect: press Connect when you need it.

Developer CLI (run from a terminal; it uses the same connection, so close the window first):

```
xm-control --connect-test             connect and disconnect
xm-control --probe [--keep-mac]       read-only dump of what the headset supports (MACs are hidden unless --keep-mac)
xm-control --get [feature]            read the state
xm-control --watch [secs]             print changes coming from the headset
xm-control --device AA:BB:CC:DD:EE:FF pick a headset when several are paired
```

## Build

Stable Rust on Windows, `x86_64-pc-windows-gnu` (with mingw) or `x86_64-pc-windows-msvc`:

```
cargo build --release
cargo test --release
```

Architecture notes: [docs/arch/overview.md](docs/arch/overview.md); protocol notes: [docs/arch/protocol.md](docs/arch/protocol.md).

## Credits

The protocol knowledge comes from [mos9527/SonyHeadphonesClient](https://github.com/mos9527/SonyHeadphonesClient) (MIT).
This is an independent Rust rewrite, not a copy of its code; see [NOTICE](NOTICE).

## License

MIT, see [LICENSE](LICENSE).

---

## По-русски

Небольшое Windows-приложение на Rust для управления наушниками Sony WH-1000XM6 с компьютера, без телефона.
Работает напрямую по Bluetooth (RFCOMM, протокол Sony «MDR»). Не связано с Sony.

**Статус:** ранняя разработка. В `main` сейчас есть окно (выбор наушников, подключение, модель, прошивка, заряд) и
CLI для разработки, который только читает. Панели настроек (шумоподавление и окружающий звук, эквалайзер на 10 полос
и пресеты, DSEE, Speak-to-Chat, автовыключение, автопауза, мультипоинт, список устройств) готовы в pull request'ах и
вливаются по мере проверки на реальных наушниках. Clear Bass у XM6 нет, поэтому его нет и в программе.

**Безопасность:** программа не может отправить обновление прошивки, сброс к заводским настройкам, удаление сопряжения
или перевод в режим сопряжения: каждая команда проходит через защитный фильтр.

**Установка:** собрать exe (`cargo build --release`), затем установщик через Inno Setup 6 (команда выше) и запустить
`dist\XM-Control-Setup-0.1.0.exe`. Ставится для текущего пользователя без прав администратора.

**Использование:** сначала сопрягите наушники с Windows, затем запустите XM Control, выберите наушники и нажмите Connect.

Протокол изучен по [mos9527/SonyHeadphonesClient](https://github.com/mos9527/SonyHeadphonesClient) (MIT), код написан заново. Лицензия MIT.
