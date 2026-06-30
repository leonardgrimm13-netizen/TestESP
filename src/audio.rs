use crate::button::{ButtonEvent, DebouncedButton};
use crate::{config, hw, wav};
use anyhow::{bail, Context, Result};
use log::{info, warn};
use std::fs::OpenOptions;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SAMPLE_PERIOD_US: u64 = 1_000_000 / config::RECORD_SAMPLE_RATE as u64;
const RECORD_FLUSH_BYTES: u32 = config::RECORD_SAMPLE_RATE;
const RECORD_HIGHPASS_ALPHA: f32 = 0.94;
const RECORD_LOWPASS_ALPHA: f32 = 0.45;
const PLAYBACK_HIGHPASS_ALPHA: f32 = 0.995;
const PLAYBACK_LOWPASS_ALPHA: f32 = 0.65;

pub fn record_until_short_press(path: &Path, button: &mut DebouncedButton) -> Result<u32> {
    info!(
        "Aufnahme-Mic-Modus {}: {}",
        config::MIC_MODE,
        mic_mode_description(config::MIC_MODE)
    );
    hw::prepare_recording(config::MIC_MODE)?;

    let calibration = calibrate_mic()?;
    info!(
        "Aufnahme-Kalibrierung: samples={} dc_offset={:.1} noise_floor_mad={:.2} raw_min={} raw_max={} raw_p2p={}",
        calibration.samples,
        calibration.dc_offset,
        calibration.noise_floor,
        calibration.raw_min,
        calibration.raw_max,
        calibration.raw_max - calibration.raw_min
    );

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "Aufnahmedatei konnte nicht erstellt werden: {}",
                path.display()
            )
        })?;

    let mut writer = BufWriter::new(file);
    wav::write_placeholder_header(&mut writer)?;

    let mut debug_raw = DebugRawWriter::maybe_create(path)?;
    let mut processor = RecordingProcessor::new(calibration);
    let mut stats = RecordStats::new(calibration);
    let mut data_len = 0_u32;
    let mut next_sample = hw::micros();

    loop {
        wait_until(next_sample);
        next_sample = next_sample.wrapping_add(SAMPLE_PERIOD_US);

        let raw = read_oversampled_raw()?;
        stats.observe_raw(raw);

        let sample = processor.process(raw, &mut stats);
        writer.write_all(&[sample])?;
        debug_raw.write_sample(raw)?;

        data_len = data_len
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("WAV-Datenbereich ist voll"))?;

        if data_len % 64 == 0 {
            if let Some(event) = button.poll(hw::millis(), hw::button_pressed()) {
                match event {
                    ButtonEvent::ShortPress => {
                        info!("Kurzer Druck waehrend Aufnahme: stoppe Aufnahme");
                        break;
                    }
                    ButtonEvent::LongPress => {
                        warn!("Langer Druck waehrend Aufnahme wird ignoriert");
                    }
                }
            }
        }

        if data_len % RECORD_FLUSH_BYTES == 0 {
            writer.flush()?;
            debug_raw.flush()?;
        }
    }

    writer.flush()?;
    let mut file = writer.into_inner()?;
    wav::finalize_header(&mut file, data_len)?;
    file.flush()?;
    debug_raw.finish()?;

    stats.final_gain = processor.gain;
    stats.written_bytes = data_len;
    info!(
        "Aufnahme-Statistik: samples={} raw_min={} raw_max={} raw_p2p={} dc_offset={:.1} noise_floor={:.2} final_agc_gain={:.2} gate_open_samples={} limited_samples={} written_bytes={}",
        stats.samples,
        stats.raw_min,
        stats.raw_max,
        stats.raw_max - stats.raw_min,
        stats.dc_offset,
        stats.noise_floor,
        stats.final_gain,
        stats.gate_open_samples,
        stats.limited_samples,
        stats.written_bytes
    );

    hw::audio_idle().ok();
    Ok(data_len)
}

pub fn play_wav_blocking(path: &Path) -> Result<()> {
    let file = OpenOptions::new().read(true).open(path).with_context(|| {
        format!(
            "WAV-Datei konnte nicht geoeffnet werden: {}",
            path.display()
        )
    })?;

    let mut reader = BufReader::new(file);
    let info = wav::read_info(&mut reader)?;
    let frames = info.frame_count()?;
    let input_peak = scan_peak(&mut reader, info)?;
    let input_peak_norm = input_peak as f32 / 32768.0;
    let estimated_gain = playback_gain_for_peak(input_peak_norm);

    info!(
        "Playback WAV: file={} format=PCM channels={} sample_rate={} bits_per_sample={} block_align={} frames={} data_bytes={}",
        path.display(),
        info.channels,
        info.sample_rate_hz,
        info.bits_per_sample,
        info.block_align,
        frames,
        info.data_len
    );
    info!(
        "Playback DSP: input_peak={} peak_norm={:.3} base_gain={:.2} estimated_first_gain={:.2} auto_normalize={} compressor={} limiter={}",
        input_peak,
        input_peak_norm,
        config::PLAYBACK_GAIN,
        estimated_gain,
        config::PLAYBACK_AUTO_NORMALIZE,
        config::PLAYBACK_COMPRESSOR_ENABLED,
        config::PLAYBACK_LIMITER_ENABLED
    );
    info!("Playback-Button: Eingaben waehrend Wiedergabe werden ignoriert");

    if info.sample_rate_hz != config::RECORD_SAMPLE_RATE {
        warn!(
            "WAV hat {} Hz statt {} Hz; keine hochwertige Resampling-Stufe aktiv, Ausgabe taktet mit der Header-Samplerate",
            info.sample_rate_hz,
            config::RECORD_SAMPLE_RATE
        );
    }

    reader.seek(SeekFrom::Start(info.data_start))?;
    hw::prepare_playback()?;

    let period_us = (1_000_000_u64 / info.sample_rate_hz.max(1) as u64).max(1);
    let mut remaining_frames = frames as usize;
    let mut scratch = Vec::new();
    let mut samples = Vec::with_capacity(config::PLAYBACK_BLOCK_SAMPLES);
    let mut processor = PlaybackProcessor::default();
    let mut stats = PlaybackStats::new(input_peak);
    let mut next_sample = hw::micros();

    while remaining_frames > 0 {
        let want_frames = remaining_frames.min(config::PLAYBACK_BLOCK_SAMPLES);
        read_pcm_block(&mut reader, info, want_frames, &mut scratch, &mut samples)?;
        if samples.is_empty() {
            break;
        }

        let block_peak = samples
            .iter()
            .map(|sample| sample.unsigned_abs() as i32)
            .max()
            .unwrap_or(0);
        let gain = playback_gain_for_peak(block_peak as f32 / 32768.0);
        stats.observe_gain(gain);

        for &sample in &samples {
            let normalized = sample as f32 / 32768.0;
            let processed = processor.process(normalized, gain, &mut stats);
            let out = signed_unit_to_u8(processed);

            wait_until(next_sample);
            next_sample = next_sample.wrapping_add(period_us);
            hw::pwm_set_sample(out)?;
            stats.output_samples += 1;
        }

        remaining_frames -= samples.len();
    }

    hw::pwm_stop().ok();
    info!(
        "Playback fertig: samples={} input_peak={} min_gain={:.2} max_gain={:.2} avg_gain={:.2} limiter_active={} limited_samples={}",
        stats.output_samples,
        stats.input_peak,
        stats.min_gain,
        stats.max_gain,
        stats.average_gain(),
        stats.limited_samples > 0,
        stats.limited_samples
    );
    Ok(())
}

fn calibrate_mic() -> Result<Calibration> {
    let count = ((config::RECORD_SAMPLE_RATE as u64 * config::RECORD_CALIBRATION_MS as u64) / 1000)
        .max(16) as usize;
    let mut samples = Vec::with_capacity(count);
    let mut next_sample = hw::micros();

    for _ in 0..count {
        wait_until(next_sample);
        next_sample = next_sample.wrapping_add(SAMPLE_PERIOD_US);
        samples.push(read_oversampled_raw()?);
    }

    let raw_min = *samples.iter().min().unwrap_or(&0);
    let raw_max = *samples.iter().max().unwrap_or(&0);
    let sum: i64 = samples.iter().map(|&sample| sample as i64).sum();
    let dc_offset = sum as f32 / samples.len() as f32;
    let mad = samples
        .iter()
        .map(|&sample| (sample as f32 - dc_offset).abs())
        .sum::<f32>()
        / samples.len() as f32;

    Ok(Calibration {
        dc_offset,
        noise_floor: mad.max(0.5),
        raw_min,
        raw_max,
        samples: samples.len() as u32,
    })
}

fn read_oversampled_raw() -> Result<i32> {
    let mut values = [0_i32; config::RECORD_OVERSAMPLE];
    for value in values.iter_mut() {
        *value = hw::adc_read()?;
    }

    values.sort_unstable();
    let slice = if values.len() >= 4 {
        &values[1..values.len() - 1]
    } else {
        &values[..]
    };

    let sum: i32 = slice.iter().copied().sum();
    Ok(sum / slice.len() as i32)
}

fn read_pcm_block<R: Read>(
    reader: &mut R,
    info: wav::WavInfo,
    frames: usize,
    scratch: &mut Vec<u8>,
    samples: &mut Vec<i16>,
) -> Result<()> {
    let bytes_per_frame = info.bytes_per_frame()?;
    scratch.resize(frames * bytes_per_frame, 0);
    reader.read_exact(scratch)?;

    samples.clear();
    for frame in scratch.chunks_exact(bytes_per_frame) {
        samples.push(decode_frame_to_mono_i16(frame, info)?);
    }

    Ok(())
}

fn decode_frame_to_mono_i16(frame: &[u8], info: wav::WavInfo) -> Result<i16> {
    match info.bits_per_sample {
        8 => {
            let mut sum = 0_i32;
            for ch in 0..info.channels as usize {
                sum += (frame[ch] as i32 - 128) << 8;
            }
            Ok((sum / info.channels as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
        }
        16 => {
            let mut sum = 0_i32;
            for ch in 0..info.channels as usize {
                let i = ch * 2;
                sum += i16::from_le_bytes([frame[i], frame[i + 1]]) as i32;
            }
            Ok((sum / info.channels as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
        }
        other => bail!("nicht unterstuetzte Bit-Tiefe: {other}"),
    }
}

fn scan_peak<R: Read + Seek>(reader: &mut R, info: wav::WavInfo) -> Result<i32> {
    reader.seek(SeekFrom::Start(info.data_start))?;

    let mut remaining_frames = info.frame_count()? as usize;
    let mut scratch = Vec::new();
    let mut samples = Vec::with_capacity(config::PLAYBACK_BLOCK_SAMPLES);
    let mut peak = 0_i32;

    while remaining_frames > 0 {
        let want_frames = remaining_frames.min(config::PLAYBACK_BLOCK_SAMPLES);
        read_pcm_block(reader, info, want_frames, &mut scratch, &mut samples)?;
        peak = peak.max(
            samples
                .iter()
                .map(|sample| sample.unsigned_abs() as i32)
                .max()
                .unwrap_or(0),
        );
        remaining_frames -= samples.len();
    }

    Ok(peak)
}

fn playback_gain_for_peak(peak: f32) -> f32 {
    let auto_gain = if config::PLAYBACK_AUTO_NORMALIZE && peak > 0.001 {
        (config::PLAYBACK_AUTO_TARGET / peak).clamp(1.0, config::PLAYBACK_AUTO_GAIN_MAX)
    } else {
        1.0
    };

    config::PLAYBACK_GAIN * auto_gain
}

fn wait_until(target_us: u64) {
    loop {
        let now = hw::micros();
        if now >= target_us {
            break;
        }

        let remain = target_us - now;
        if remain > 80 {
            hw::delay_us((remain - 40).min(u32::MAX as u64) as u32);
        }
    }
}

fn signed_unit_to_u8(sample: f32) -> u8 {
    let signed = (sample.clamp(-1.0, 1.0) * 127.0).round() as i32;
    (signed + 128).clamp(0, 255) as u8
}

fn raw_to_debug_u8(raw: i32) -> u8 {
    (raw.clamp(0, 4095) >> 4) as u8
}

fn mic_mode_description(mode: u8) -> &'static str {
    match mode {
        0 => "GPIO4 ADC, GPIO5 schwacher Pulldown",
        1 => "GPIO4 ADC, GPIO5 High-Z",
        2 => "GPIO4 ADC, GPIO5 schwacher Pullup",
        3 => "GPIO5 ADC, GPIO4 schwacher Pulldown",
        _ => "ungueltig",
    }
}

#[derive(Clone, Copy)]
struct Calibration {
    dc_offset: f32,
    noise_floor: f32,
    raw_min: i32,
    raw_max: i32,
    samples: u32,
}

struct RecordStats {
    samples: u32,
    raw_min: i32,
    raw_max: i32,
    dc_offset: f32,
    noise_floor: f32,
    final_gain: f32,
    gate_open_samples: u32,
    limited_samples: u32,
    written_bytes: u32,
}

impl RecordStats {
    fn new(calibration: Calibration) -> Self {
        Self {
            samples: 0,
            raw_min: i32::MAX,
            raw_max: i32::MIN,
            dc_offset: calibration.dc_offset,
            noise_floor: calibration.noise_floor,
            final_gain: config::RECORD_INITIAL_GAIN,
            gate_open_samples: 0,
            limited_samples: 0,
            written_bytes: 0,
        }
    }

    fn observe_raw(&mut self, raw: i32) {
        self.samples += 1;
        self.raw_min = self.raw_min.min(raw);
        self.raw_max = self.raw_max.max(raw);
    }
}

struct RecordingProcessor {
    dc_offset: f32,
    noise_threshold: f32,
    hp_prev_x: f32,
    hp_prev_y: f32,
    lp_y: f32,
    gain: f32,
}

impl RecordingProcessor {
    fn new(calibration: Calibration) -> Self {
        Self {
            dc_offset: calibration.dc_offset,
            noise_threshold: (calibration.noise_floor * config::NOISE_GATE_MULTIPLIER).max(1.0),
            hp_prev_x: 0.0,
            hp_prev_y: 0.0,
            lp_y: 0.0,
            gain: config::RECORD_INITIAL_GAIN
                .clamp(config::RECORD_GAIN_MIN, config::RECORD_GAIN_MAX),
        }
    }

    fn process(&mut self, raw: i32, stats: &mut RecordStats) -> u8 {
        let centered = raw as f32 - self.dc_offset;
        let hp = centered - self.hp_prev_x + RECORD_HIGHPASS_ALPHA * self.hp_prev_y;
        self.hp_prev_x = centered;
        self.hp_prev_y = hp;

        self.lp_y += RECORD_LOWPASS_ALPHA * (hp - self.lp_y);
        let abs = self.lp_y.abs();
        let gate_open = abs >= self.noise_threshold;
        let gated = if gate_open {
            stats.gate_open_samples += 1;
            self.lp_y
        } else {
            self.lp_y * config::NOISE_GATE_ATTENUATION
        };

        if gate_open && abs > 0.5 {
            let desired = (config::RECORD_TARGET_LEVEL / abs)
                .clamp(config::RECORD_GAIN_MIN, config::RECORD_GAIN_MAX);
            let rate = if desired < self.gain { 0.08 } else { 0.002 };
            self.gain += rate * (desired - self.gain);
        } else {
            self.gain += 0.0005 * (config::RECORD_GAIN_MIN - self.gain);
        }

        let amplified = gated * self.gain;
        let limited = amplified.clamp(-127.0, 127.0);
        if (limited - amplified).abs() > f32::EPSILON {
            stats.limited_samples += 1;
        }

        stats.final_gain = self.gain;
        (limited.round() as i32 + 128).clamp(0, 255) as u8
    }
}

#[derive(Default)]
struct PlaybackProcessor {
    hp_prev_x: f32,
    hp_prev_y: f32,
    lp_y: f32,
}

impl PlaybackProcessor {
    fn process(&mut self, sample: f32, gain: f32, stats: &mut PlaybackStats) -> f32 {
        let hp = sample - self.hp_prev_x + PLAYBACK_HIGHPASS_ALPHA * self.hp_prev_y;
        self.hp_prev_x = sample;
        self.hp_prev_y = hp;

        self.lp_y += PLAYBACK_LOWPASS_ALPHA * (hp - self.lp_y);
        let mut y = self.lp_y * gain;

        if config::PLAYBACK_COMPRESSOR_ENABLED {
            y = compress(y);
        }

        if config::PLAYBACK_LIMITER_ENABLED && y.abs() > config::PLAYBACK_LIMIT {
            stats.limited_samples += 1;
            y = y.clamp(-config::PLAYBACK_LIMIT, config::PLAYBACK_LIMIT);
        }

        y.clamp(-1.0, 1.0)
    }
}

fn compress(sample: f32) -> f32 {
    let sign = sample.signum();
    let abs = sample.abs();

    if abs <= config::PLAYBACK_COMPRESSOR_THRESHOLD {
        sample
    } else {
        let compressed = config::PLAYBACK_COMPRESSOR_THRESHOLD
            + (abs - config::PLAYBACK_COMPRESSOR_THRESHOLD)
                / config::PLAYBACK_COMPRESSOR_RATIO.max(1.0);
        sign * compressed
    }
}

struct PlaybackStats {
    input_peak: i32,
    output_samples: u32,
    limited_samples: u32,
    min_gain: f32,
    max_gain: f32,
    gain_sum: f32,
    gain_blocks: u32,
}

impl PlaybackStats {
    fn new(input_peak: i32) -> Self {
        Self {
            input_peak,
            output_samples: 0,
            limited_samples: 0,
            min_gain: f32::MAX,
            max_gain: 0.0,
            gain_sum: 0.0,
            gain_blocks: 0,
        }
    }

    fn observe_gain(&mut self, gain: f32) {
        self.min_gain = self.min_gain.min(gain);
        self.max_gain = self.max_gain.max(gain);
        self.gain_sum += gain;
        self.gain_blocks += 1;
    }

    fn average_gain(&self) -> f32 {
        if self.gain_blocks == 0 {
            0.0
        } else {
            self.gain_sum / self.gain_blocks as f32
        }
    }
}

struct DebugRawWriter {
    path: Option<PathBuf>,
    writer: Option<BufWriter<std::fs::File>>,
    data_len: u32,
}

impl DebugRawWriter {
    fn maybe_create(record_path: &Path) -> Result<Self> {
        if !config::WRITE_DEBUG_RAW_WAV {
            return Ok(Self {
                path: None,
                writer: None,
                data_len: 0,
            });
        }

        let Some(file_name) = record_path.file_name() else {
            return Ok(Self {
                path: None,
                writer: None,
                data_len: 0,
            });
        };

        let raw_name = file_name
            .to_string_lossy()
            .replacen("REC_", "RAW_", 1)
            .replace(".WAV", "_RAW.WAV");
        let raw_path = record_path.with_file_name(raw_name);
        let file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&raw_path)
        {
            Ok(file) => file,
            Err(err) => {
                warn!(
                    "Debug-Roh-WAV wird nicht geschrieben: {}: {err}",
                    raw_path.display()
                );
                return Ok(Self {
                    path: None,
                    writer: None,
                    data_len: 0,
                });
            }
        };

        let mut writer = BufWriter::new(file);
        wav::write_placeholder_header(&mut writer)?;
        info!("Debug-Roh-WAV aktiv: {}", raw_path.display());
        Ok(Self {
            path: Some(raw_path),
            writer: Some(writer),
            data_len: 0,
        })
    }

    fn write_sample(&mut self, raw: i32) -> Result<()> {
        if let Some(writer) = self.writer.as_mut() {
            writer.write_all(&[raw_to_debug_u8(raw)])?;
            self.data_len += 1;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if let Some(writer) = self.writer.as_mut() {
            writer.flush()?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
            let mut file = writer.into_inner()?;
            wav::finalize_header(&mut file, self.data_len)?;
            file.flush()?;
            if let Some(path) = self.path {
                info!(
                    "Debug-Roh-WAV gespeichert: {} ({} Bytes)",
                    path.display(),
                    self.data_len
                );
            }
        }
        Ok(())
    }
}
