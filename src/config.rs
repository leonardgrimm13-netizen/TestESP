//! Zentrale Audio-Konfiguration.
//!
//! Diese Werte sind bewusst leicht aenderbar, weil ein direkt zwischen zwei
//! GPIOs angeschlossener Lautsprecher als Mikrofon elektrisch sehr grenzwertig
//! ist. Software kann keine zusaetzliche elektrische Leistung erzeugen: bei der
//! Wiedergabe wirken Gain, Normalisierung und Kompression nur lauter, indem sie
//! den vorhandenen PWM-Bereich besser ausnutzen und Clipping kontrollieren. Bei
//! der Aufnahme koennen Filter, Gate und AGC das Signal verbessern, aber keinen
//! fehlenden Mikrofon-Vorverstaerker ersetzen.

/// Ziel-Samplerate fuer neu aufgenommene WAV-Dateien.
pub const RECORD_SAMPLE_RATE: u32 = 8_000;

/// ADC-Messungen pro gespeichertem WAV-Sample. 8 reduziert zufaelliges ADC-
/// Rauschen deutlicher als 4, kostet aber mehr CPU-Zeit.
pub const RECORD_OVERSAMPLE: usize = 8;

/// Aufnahme bleibt standardmaessig 8-bit unsigned PCM. Fuer diesen sehr
/// schwachen Lautsprecher-Mikrofon-Aufbau bringt 16-bit meist weniger als die
/// Software-AGC, wuerde aber die SD-Schreibrate verdoppeln.
pub const RECORD_BITS_PER_SAMPLE: u16 = 8;

/// Stillemessung vor jeder Aufnahme zur Bestimmung von DC-Offset und
/// Noise-Floor.
pub const RECORD_CALIBRATION_MS: u32 = 500;

/// Aufnahme zuerst in RAM puffern und erst nach Stop auf SD schreiben. Das
/// vermeidet SPI-/SD-Schreibstoerungen waehrend des ADC-Samplings.
pub const RECORD_TO_RAM_FIRST: bool = true;

/// RAM-Grenze fuer eine Aufnahme. 30 s bei 8 kHz / 8-bit sind ca. 240 KiB.
pub const RECORD_MAX_SECONDS: u32 = 30;

/// ADC-Attenuation:
/// 0 = ADC_ATTEN_DB_0, empfindlichster Bereich fuer sehr kleine Signale
/// 1 = ADC_ATTEN_DB_2_5
/// 2 = ADC_ATTEN_DB_6
/// 3 = ADC_ATTEN_DB_12, groesster Spannungsbereich, aber fuer Mikrofonversuch
///     meist zu unempfindlich
pub const RECORD_ADC_ATTEN_MODE: u8 = 2;

/// Kleinste und groesste erlaubte Aufnahme-AGC.
pub const RECORD_GAIN_MIN: f32 = 1.0;
pub const RECORD_GAIN_MAX: f32 = 64.0;
pub const RECORD_INITIAL_GAIN: f32 = 12.0;

/// Zielpegel nach Gate/Filter in 8-bit signed Einheiten.
pub const RECORD_TARGET_LEVEL: f32 = 92.0;

/// Noise-Gate-Schwelle relativ zur gemessenen mittleren absoluten Abweichung.
pub const NOISE_GATE_MULTIPLIER: f32 = 1.25;

/// Unterhalb der Gate-Schwelle wird nicht hart stummgeschaltet, sondern nur
/// abgesenkt, damit Sprache nicht komplett zerhackt wird.
pub const NOISE_GATE_ATTENUATION: f32 = 0.25;

/// Mic-Modus:
/// 0 = GPIO4 ADC, GPIO5 Pulldown
/// 1 = GPIO4 ADC, GPIO5 High-Z
/// 2 = GPIO4 ADC, GPIO5 Pullup
/// 3 = GPIO5 ADC, GPIO4 Pulldown (ESP32-S3 GPIO5 ist ADC1_CH4)
/// 4 = PSEUDO_DIFF: GPIO4 ADC minus GPIO5 ADC, beide hochohmig
/// 5 = AUTO_SCAN: Modi 0..4 kurz messen und besten Modus waehlen
pub const MIC_MODE: u8 = 4;

/// Optionales Roh-WAV zum Debuggen. Standard aus, damit die SD-Karte nicht
/// unnoetig vollgeschrieben wird.
pub const WRITE_DEBUG_RAW_WAV: bool = false;

/// Playback-Ausgabemodus:
/// 0 = normaler LEDC-PWM-Gegentakt
/// 1 = MAX_LOUD_LEDC: aggressivere Kompression, Clipping und 31.25 kHz PWM
/// 2 = EXPERIMENTAL_SDM_PDM: derzeit bewusst Fallback auf Mode 1, wenn der
///     ESP-IDF-SDM-Treiber in diesem Build nicht stabil verfuegbar ist
pub const PLAYBACK_OUTPUT_MODE: u8 = 1;

/// LEDC-PWM-Frequenzprofil:
/// 0 = 62500 Hz, weniger PWM-Pfeifen
/// 1 = 31250 Hz, Max-Loud-Default
/// 2 = 15625 Hz, sehr experimentell und wahrscheinlich hoerbarer PWM-Ton
pub const PLAYBACK_PWM_FREQ_MODE: u8 = 1;

/// Extreme Lautheit priorisiert Verstaendlichkeit/Lautheit vor HiFi.
pub const PLAYBACK_EXTREME_LOUDNESS: bool = true;
pub const PLAYBACK_HARD_CLIP: bool = true;
pub const PLAYBACK_REMOVE_LOWPASS_FOR_LOUDNESS: bool = true;
pub const PLAYBACK_PREEMPHASIS: bool = true;
pub const PLAYBACK_NOISE_SHAPING: bool = true;

/// Fester Wiedergabe-Gain vor Auto-Normalisierung und Kompressor.
pub const PLAYBACK_GAIN: f32 = 8.0;

/// Pro Block wird der Peak gesucht und bei kleinen Pegeln automatisch
/// angehoben. Das macht leise WAVs deutlich lauter.
pub const PLAYBACK_AUTO_NORMALIZE: bool = true;
pub const PLAYBACK_AUTO_TARGET: f32 = 0.92;
pub const PLAYBACK_AUTO_GAIN_MAX: f32 = 24.0;

/// Einfacher Sprach-Kompressor/Limiter. Erhoeht wahrgenommene Lautheit, kann
/// aber Verzerrungen erzeugen, wenn das Quellmaterial schon stark clipped.
pub const PLAYBACK_COMPRESSOR_ENABLED: bool = true;
pub const PLAYBACK_COMPRESSOR_THRESHOLD: f32 = 0.40;
pub const PLAYBACK_COMPRESSOR_RATIO: f32 = 10.0;
pub const PLAYBACK_LIMITER_ENABLED: bool = true;
pub const PLAYBACK_LIMIT: f32 = 0.98;

/// Kleine Samples werden im Max-Loud-Profil abgesenkt, groessere Samples
/// aggressiver Richtung Vollaussteuerung gedrueckt.
pub const PLAYBACK_CENTER_CLIP_THRESHOLD: f32 = 0.035;
pub const PLAYBACK_EXPAND_POWER: f32 = 0.72;

/// Blockgroesse fuer Playback-DSP. 256 Samples sind klein genug fuer wenig
/// Latenz und gross genug fuer sinnvolle Peak-Normalisierung.
pub const PLAYBACK_BLOCK_SAMPLES: usize = 256;
