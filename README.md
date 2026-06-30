# YD-ESP32-23 ESP32-S3 Audio/SD/WAV

## Pinbelegung

| Funktion | ESP32-S3 Pin | Hinweis |
| --- | ---: | --- |
| Lautsprecher Audio A / ADC | GPIO4 | Aufnahme: ADC1 CH3, Wiedergabe: PWM |
| Lautsprecher Audio B / Referenz | GPIO5 | Aufnahme: hochohmig mit internem Pulldown, Wiedergabe: invertiertes PWM |
| SD CS | GPIO10 | SPI Chip Select |
| SD MOSI | GPIO11 | SPI MOSI |
| SD CLK | GPIO12 | SPI Clock |
| SD MISO | GPIO13 | SPI MISO |
| Button | GPIO21 | Taster gegen GND, interner Pull-Up |
| Onboard RGB-LED | GPIO48 | WS2812/NeoPixel, GRB-Datenformat |

## Grenzen des Direktanschlusses

Der 2-Pin-Lautsprecher ist elektrisch kein gutes Mikrofon. Ohne Vorverstärker und Bias-Schaltung liefert die Schwingspule nur ein sehr kleines, schlecht definiertes Signal. Der ESP32-S3 hat keinen echten differenziellen ADC für diese Anwendung. Deshalb nimmt dieses Projekt GPIO4 als ADC-Eingang und lässt GPIO5 hochohmig mit schwachem internem Pulldown als Referenz. In Software werden DC-Anteil, Rauschen und Pegel mit Hochpass und einfacher AGC nachgeführt. Das ist die bestmögliche direkte Lösung ohne externe Bauteile, ersetzt aber kein Mikrofonmodul.

Bei der Wiedergabe treiben GPIO4 und GPIO5 den Lautsprecher per LEDC-PWM im Gegentakt. Die PWM-Amplitude wird begrenzt, damit die Pins nicht dauerhaft mit extremen Tastgraden gegeneinander arbeiten. Ein kleiner Audioverstärker ist für Lautstärke und Pin-Schutz trotzdem dringend empfohlen.

## Verhalten

- Kurz drücken: Aufnahme starten, grüne LED.
- Kurz drücken während Aufnahme: Aufnahme stoppen, WAV-Header finalisieren.
- Lang drücken ab 500 ms im Idle: erste WAV-Datei alphabetisch von SD abspielen, rote LED.
- Lang drücken während Aufnahme oder Wiedergabe: wird ignoriert.
- SD-Fehler: Meldung über Serial, LED blinkt blau.

## WAV-Format

- 8000 Hz
- Mono
- PCM
- Aufnahme: 8-bit unsigned
- Wiedergabe: 8-bit unsigned und 16-bit signed PCM mono

## Build und Flash unter Linux

Installiere zuerst die Linux-Pakete aus der ESP-IDF-Anleitung, z.B. auf Debian/Ubuntu:

```bash
sudo apt-get install git wget flex bison gperf python3 python3-pip python3-venv cmake ninja-build ccache libffi-dev libssl-dev libxml2 dfu-util libusb-1.0-0 libudev-dev
```

Installiere die Rust-/ESP-Werkzeuge:

```bash
cargo install espup
cargo install espflash
cargo install cargo-espflash
cargo install ldproxy
espup install
. "$HOME/export-esp.sh"
rustup +esp target add xtensa-esp32s3-espidf
```

Bauen:

```bash
. "$HOME/export-esp.sh"
cargo build --release
```

Flashen mit Monitor:

```bash
cargo run --release
```

Alternative ohne Cargo-Runner:

```bash
espflash flash --monitor target/xtensa-esp32s3-espidf/release/yd_esp32_s3_audio_sd
```

Hinweis: Dieses Projekt pinnt `esp-idf-svc 0.52.1`, `esp-idf-sys 0.37.2` und verwendet ESP-IDF-C-APIs für SD, ADC, LEDC und RMT. Falls eine lokale ESP-IDF-Version ältere Felder in C-Structs nicht kennt, ist `native/yd_hw.c` die einzige Stelle, die angepasst werden muss.
