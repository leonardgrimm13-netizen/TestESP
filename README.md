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

Der 2-Pin-Lautsprecher ist elektrisch kein gutes Mikrofon. Ohne Vorverstärker und Bias-Schaltung liefert die Schwingspule nur ein sehr kleines, schlecht definiertes Signal. Der ESP32-S3 hat keinen echten differenziellen ADC für diese Anwendung. Dieses Projekt nutzt deshalb als besten Software-Versuch eine pseudo-differenzielle Messung: GPIO4 und GPIO5 werden nacheinander per ADC gelesen und in Rust als Differenz ausgewertet. Das ersetzt kein Mikrofonmodul und keinen Vorverstärker.

Bei der Wiedergabe treiben GPIO4 und GPIO5 den Lautsprecher per LEDC-PWM im Gegentakt. Software kann keine zusätzliche elektrische Leistung erzeugen. Gain, Normalisierung, Kompression und Clipping können nur den vorhandenen PWM-/GPIO-Bereich aggressiver ausnutzen, wodurch das Signal lauter und rauer wirkt. Ein direkt am GPIO betriebener Lautsprecher kann den ESP belasten; dieses Projekt setzt trotzdem keine zusätzlichen Bauteile voraus.

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
- Wiedergabe: 8-bit unsigned und 16-bit signed PCM, Mono oder Stereo-zu-Mono

## Audio-Profile testen

Alle zentralen Werte stehen in `src/config.rs`.

- `PLAYBACK_OUTPUT_MODE = 0`: normaler LEDC-Gegentakt mit 62.5 kHz PWM.
- `PLAYBACK_OUTPUT_MODE = 1`: `MAX_LOUD_LEDC`, aggressiv komprimiert/geclippt mit 31.25 kHz PWM. Das ist der aktuelle Default.
- `PLAYBACK_OUTPUT_MODE = 2`: experimenteller SDM/PDM-Wunschmodus. Wenn der ESP-IDF-5.2.3-SDM-Treiber im aktuellen Build nicht sauber verfügbar ist, fällt die native Schicht automatisch auf Mode 1 zurück.
- `PLAYBACK_PWM_FREQ_MODE = 0/1/2`: 62.5 kHz, 31.25 kHz oder sehr experimentell 15.625 kHz. Niedrigere Frequenzen koennen lauter wirken, aber PWM-Pfeifen hörbarer machen.
- `PLAYBACK_EXTREME_LOUDNESS`, `PLAYBACK_HARD_CLIP`, `PLAYBACK_PREEMPHASIS`, `PLAYBACK_NOISE_SHAPING`: machen Playback lauter und verständlicher, aber auch rauer.
- `MIC_MODE = 4`: pseudo-differenziell, GPIO4 ADC minus GPIO5 ADC. Das ist der aktuelle Default.
- `MIC_MODE = 5`: Auto-Scan der Modi 0..4 vor jeder Aufnahme, mit Log-Tabelle und Score.
- `RECORD_ADC_ATTEN_MODE = 0`: `ADC_ATTEN_DB_0`, empfindlichster ADC-Bereich für sehr kleine Lautsprecher-Mikrofon-Signale.
- `RECORD_TO_RAM_FIRST = true`: während der Aufnahme wird nur in RAM geschrieben; erst nach Stop wird die WAV-Datei auf SD geschrieben. Das reduziert SD-/SPI-Störungen im ADC-Sampling.

Empfohlene Tests:

1. Erst mit `MIC_MODE = 4`, `RECORD_ADC_ATTEN_MODE = 0`, `RECORD_TO_RAM_FIRST = true` testen.
2. Danach `MIC_MODE = 5` testen und im Monitor die `MIC_SCAN`-Scores vergleichen.
3. Auf die Lautsprechermembran klopfen und eine kurze Aufnahme speichern.
4. Sehr laut und direkt vor dem Lautsprecher sprechen.
5. Die WAV-Dateien von der SD-Karte am PC anhören.

Erwartete Monitor-Ausgaben:

- `AUDIO BUILD: playback_output_mode=...`
- `AUDIO BUILD: mic_mode=... record_to_ram_first=... record_oversample=... adc_atten=...`
- `RECORD START ...`
- `RECORD MIC ...`
- `RECORD CAL ...`
- `RECORD WRITE ...`
- `RECORD STATS ...`
- `PLAYBACK START ...`
- `PLAYBACK FORMAT ...`
- `PLAYBACK DSP ...`
- `PLAYBACK OUTPUT ...`
- `PLAYBACK END ...`

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
