# XM Control

A Windows app (Rust) that controls Sony WH-1000XM6 headphones from a PC, without the phone app:
noise cancelling and ambient sound, equalizer (10 bands and presets; the XM6 has no Clear Bass slot), DSEE, Speak-to-Chat, auto power off, multipoint and more.
It talks to the headphones directly over Bluetooth Classic (RFCOMM, Sony's "MDR" protocol).

Status: early development. The window (Connect, battery, noise control and ambient sound, equalizer, DSEE, Speak-to-Chat, auto power-off, auto pause, wearing status, multipoint with a confirmation, read-only list of paired devices) is implemented but each control still has to be verified on real headphones. There is no Clear Bass on the XM6, so it is not offered.

## Build

Stable Rust on Windows (`x86_64-pc-windows-gnu` or `-msvc`):

```
cargo build --release
```

## Credits

The protocol knowledge comes from [mos9527/SonyHeadphonesClient](https://github.com/mos9527/SonyHeadphonesClient) (MIT).
This is an independent Rust rewrite, not a copy of its code; see [NOTICE](NOTICE).
Not affiliated with Sony.

## License

MIT, see [LICENSE](LICENSE).

---

## По-русски

Windows-приложение на Rust для управления наушниками Sony WH-1000XM6 с ПК без телефона: шумоподавление и режим окружения,
эквалайзер, DSEE, Speak-to-Chat, автовыключение, multipoint и прочее. Работает напрямую по Bluetooth (RFCOMM, протокол Sony «MDR»).
Статус: ранняя разработка. Сборка: `cargo build --release`. Протокол изучен по mos9527/SonyHeadphonesClient (MIT), код написан заново.
