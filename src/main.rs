mod audio;
mod button;
mod config;
mod hw;
mod led;
mod sdcard;
mod wav;

use anyhow::{Context, Result};
use button::{ButtonEvent, DebouncedButton};
use esp_idf_svc::log::EspLogger;
use led::StatusLed;
use log::{error, info, warn};
use sdcard::SdCard;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("YD-ESP32-23 ESP32-S3 Audio/SD/WAV startet");
    info!("Audio: GPIO4/GPIO5, SD: CS10 MOSI11 CLK12 MISO13, Button: GPIO21, RGB: GPIO48");
    audio::log_build_config();

    let mut led = StatusLed::init().context("RGB-LED konnte nicht initialisiert werden")?;
    hw::init_button().context("Button GPIO21 konnte nicht initialisiert werden")?;
    hw::audio_idle().ok();

    let sd = loop {
        match SdCard::mount() {
            Ok(sd) => break sd,
            Err(err) => {
                error!("SD-Karte nicht bereit: {err:#}");
                led.blink_sd_error_once();
                thread::sleep(Duration::from_secs(2));
            }
        }
    };

    info!("SD-Karte gemountet unter {}", sdcard::MOUNT_POINT);
    led.idle();

    let mut button = DebouncedButton::new(hw::millis());

    loop {
        if let Some(event) = button.poll(hw::millis(), hw::button_pressed()) {
            match event {
                ButtonEvent::ShortPress => {
                    let path = sd
                        .next_recording_path()
                        .context("kein freier REC_XXXX.WAV-Dateiname gefunden")?;

                    info!("Kurzer Druck: Aufnahme startet: {}", path.display());
                    led.recording();

                    match audio::record_until_short_press(&path, &mut button) {
                        Ok(bytes) => info!("Aufnahme gespeichert: {} Bytes Audio", bytes),
                        Err(err) => {
                            error!("Aufnahmefehler: {err:#}");
                            led.error_yellow();
                            thread::sleep(Duration::from_millis(700));
                        }
                    }

                    hw::audio_idle().ok();
                    led.idle();
                }
                ButtonEvent::LongPress => {
                    let Some(path) = sd
                        .first_wav()
                        .context("WAV-Liste konnte nicht gelesen werden")?
                    else {
                        warn!("Keine WAV-Datei im SD-Wurzelverzeichnis gefunden");
                        led.error_yellow();
                        thread::sleep(Duration::from_millis(500));
                        led.idle();
                        continue;
                    };

                    info!("Langer Druck: Wiedergabe startet: {}", path.display());
                    led.playing();

                    if let Err(err) = audio::play_wav_blocking(&path) {
                        error!("Wiedergabefehler: {err:#}");
                        led.error_yellow();
                        thread::sleep(Duration::from_millis(700));
                    }

                    hw::audio_idle().ok();
                    led.idle();
                }
            }
        }

        thread::sleep(Duration::from_millis(5));
    }
}
