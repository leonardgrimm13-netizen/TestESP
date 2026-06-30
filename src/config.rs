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

/// ADC-Messungen pro gespeichertem WAV-Sample. 4 ist ein guter Kompromiss aus
/// Rauschminderung und CPU-Zeit bei 8 kHz Zielrate.
pub const RECORD_OVERSAMPLE: usize = 4;

/// Aufnahme bleibt standardmaessig 8-bit unsigned PCM. Fuer diesen sehr
/// schwachen Lautsprecher-Mikrofon-Aufbau bringt 16-bit meist weniger als die
/// Software-AGC, wuerde aber die SD-Schreibrate verdoppeln.
pub const RECORD_BITS_PER_SAMPLE: u16 = 8;

/// Stillemessung vor jeder Aufnahme zur Bestimmung von DC-Offset und
/// Noise-Floor.
pub const RECORD_CALIBRATION_MS: u32 = 400;

/// Kleinste und groesste erlaubte Aufnahme-AGC.
pub const RECORD_GAIN_MIN: f32 = 1.0;
pub const RECORD_GAIN_MAX: f32 = 32.0;
pub const RECORD_INITIAL_GAIN: f32 = 8.0;

/// Zielpegel nach Gate/Filter in 8-bit signed Einheiten.
pub const RECORD_TARGET_LEVEL: f32 = 92.0;

/// Noise-Gate-Schwelle relativ zur gemessenen mittleren absoluten Abweichung.
pub const NOISE_GATE_MULTIPLIER: f32 = 2.0;

/// Unterhalb der Gate-Schwelle wird nicht hart stummgeschaltet, sondern nur
/// abgesenkt, damit Sprache nicht komplett zerhackt wird.
pub const NOISE_GATE_ATTENUATION: f32 = 0.20;

/// Mic-Modus:
/// 0 = GPIO4 ADC, GPIO5 Pulldown
/// 1 = GPIO4 ADC, GPIO5 High-Z
/// 2 = GPIO4 ADC, GPIO5 Pullup
/// 3 = GPIO5 ADC, GPIO4 Pulldown (ESP32-S3 GPIO5 ist ADC1_CH4)
pub const MIC_MODE: u8 = 0;

/// Optionales Roh-WAV zum Debuggen. Standard aus, damit die SD-Karte nicht
/// unnoetig vollgeschrieben wird.
pub const WRITE_DEBUG_RAW_WAV: bool = false;

/// Fester Wiedergabe-Gain vor Auto-Normalisierung und Kompressor.
pub const PLAYBACK_GAIN: f32 = 4.0;

/// Pro Block wird der Peak gesucht und bei kleinen Pegeln automatisch
/// angehoben. Das macht leise WAVs deutlich lauter.
pub const PLAYBACK_AUTO_NORMALIZE: bool = true;
pub const PLAYBACK_AUTO_TARGET: f32 = 0.92;
pub const PLAYBACK_AUTO_GAIN_MAX: f32 = 8.0;

/// Einfacher Sprach-Kompressor/Limiter. Erhoeht wahrgenommene Lautheit, kann
/// aber Verzerrungen erzeugen, wenn das Quellmaterial schon stark clipped.
pub const PLAYBACK_COMPRESSOR_ENABLED: bool = true;
pub const PLAYBACK_COMPRESSOR_THRESHOLD: f32 = 0.65;
pub const PLAYBACK_COMPRESSOR_RATIO: f32 = 4.0;
pub const PLAYBACK_LIMITER_ENABLED: bool = true;
pub const PLAYBACK_LIMIT: f32 = 0.98;

/// Blockgroesse fuer Playback-DSP. 256 Samples sind klein genug fuer wenig
/// Latenz und gross genug fuer sinnvolle Peak-Normalisierung.
pub const PLAYBACK_BLOCK_SAMPLES: usize = 256;
