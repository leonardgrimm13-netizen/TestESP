use crate::button::{ButtonEvent, DebouncedButton};
use crate::{config, hw, wav};
use anyhow::{bail, Context, Result};
use log::{info, warn};
use std::fs::OpenOptions;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const SAMPLE_PERIOD_US: u64 = 1_000_000 / config::RECORD_SAMPLE_RATE as u64;
const RECORD_HIGHPASS_ALPHA: f32 = 0.94;
const RECORD_LOWPASS_ALPHA: f32 = 0.45;
const PLAYBACK_HIGHPASS_ALPHA: f32 = 0.997;
const PLAYBACK_LOWPASS_ALPHA: f32 = 0.78;
const PLAYBACK_RMS_TARGET: f32 = 0.58;
const YIELD_EVERY_SAMPLES: usize = 128;
const ADC_SATURATION_LOW: i32 = 8;
const ADC_SATURATION_HIGH: i32 = 4087;

pub fn log_build_config() {
    info!(
        "AUDIO BUILD: playback_output_mode={} extreme_loudness={} hard_clip={} pre_emphasis={} noise_shaping={}",
        config::PLAYBACK_OUTPUT_MODE,
        config::PLAYBACK_EXTREME_LOUDNESS,
        config::PLAYBACK_HARD_CLIP,
        config::PLAYBACK_PREEMPHASIS,
        config::PLAYBACK_NOISE_SHAPING
    );
    info!(
        "AUDIO BUILD: playback_pwm_freq_mode={} ({})",
        config::PLAYBACK_PWM_FREQ_MODE,
        playback_pwm_freq_description(config::PLAYBACK_PWM_FREQ_MODE)
    );
    info!(
        "AUDIO BUILD: mic_mode={} record_to_ram_first={} record_oversample={} adc_atten={}",
        config::MIC_MODE,
        config::RECORD_TO_RAM_FIRST,
        config::RECORD_OVERSAMPLE,
        adc_atten_description(config::RECORD_ADC_ATTEN_MODE)
    );
}

pub fn record_until_short_press(path: &Path, button: &mut DebouncedButton) -> Result<u32> {
    let selected_mode = if config::MIC_MODE == 5 {
        mic_mode_scan()?
    } else {
        config::MIC_MODE
    };

    info!("RECORD START path={}", path.display());
    info!(
        "RECORD MIC mode={} desc=\"{}\" adc_atten={} pseudo_diff={} record_to_ram_first={}",
        selected_mode,
        mic_mode_description(selected_mode),
        adc_atten_description(config::RECORD_ADC_ATTEN_MODE),
        selected_mode == 4,
        config::RECORD_TO_RAM_FIRST
    );

    hw::prepare_recording(selected_mode, config::RECORD_ADC_ATTEN_MODE)?;
    let calibration = calibrate_mic(selected_mode)?;
    info!(
        "RECORD CAL mode={} samples={} dc_offset={:.2} noise_floor={:.2} raw_a_min={} raw_a_max={} raw_b_min={} raw_b_max={} diff_min={} diff_max={} diff_p2p={}",
        selected_mode,
        calibration.samples,
        calibration.dc_offset,
        calibration.noise_floor,
        calibration.raw_a_min,
        calibration.raw_a_max,
        calibration.raw_b_min,
        calibration.raw_b_max,
        calibration.diff_min,
        calibration.diff_max,
        calibration.diff_max - calibration.diff_min
    );

    let max_samples = (config::RECORD_SAMPLE_RATE * config::RECORD_MAX_SECONDS) as usize;
    let mut processed = Vec::with_capacity(max_samples.min(64 * 1024));
    let mut debug = DebugCapture::new();
    let mut processor = RecordingProcessor::new(calibration);
    let mut stats = RecordStats::new(selected_mode, calibration);
    let mut next_sample = hw::micros();

    loop {
        wait_until(next_sample);
        next_sample = next_sample.wrapping_add(SAMPLE_PERIOD_US);

        let raw = read_oversampled_raw(selected_mode)?;
        stats.observe_raw(raw);
        let sample = processor.process(raw.signal, &mut stats);
        processed.push(sample);
        debug.push(raw, sample);

        if processed.len() >= max_samples {
            warn!(
                "RECORD STOP reason=max_seconds max_seconds={} samples={}",
                config::RECORD_MAX_SECONDS,
                processed.len()
            );
            break;
        }

        if processed.len() % 32 == 0 {
            if let Some(event) = button.poll(hw::millis(), hw::button_pressed()) {
                match event {
                    ButtonEvent::ShortPress => {
                        info!("RECORD STOP reason=short_press samples={}", processed.len());
                        break;
                    }
                    ButtonEvent::LongPress => {
                        warn!("RECORD BUTTON long_press_ignored=true");
                    }
                }
            }
        }

        if processed.len() % YIELD_EVERY_SAMPLES == 0 {
            scheduler_yield();
            next_sample = hw::micros().wrapping_add(SAMPLE_PERIOD_US);
        }
    }

    hw::audio_idle().ok();

    stats.final_gain = processor.gain;
    stats.written_bytes = processed.len() as u32;
    let gate_percent = if stats.samples == 0 {
        0.0
    } else {
        stats.gate_open_samples as f32 * 100.0 / stats.samples as f32
    };
    let saturation_percent = if stats.samples == 0 {
        0.0
    } else {
        stats.saturation_sample_count as f32 * 100.0 / stats.samples as f32
    };
    let gate_threshold = processor.noise_threshold;

    let write_start_ms = hw::millis();
    write_wav_file(path, &processed)?;
    let write_time_ms = hw::millis().wrapping_sub(write_start_ms);
    debug.write_files(path)?;

    info!(
        "RECORD WRITE path={} bytes={} sd_write_time_ms={} ram_first={}",
        path.display(),
        processed.len(),
        write_time_ms,
        config::RECORD_TO_RAM_FIRST
    );
    info!(
        "RECORD STATS mode={} atten={} samples={} raw_a_min={} raw_a_max={} raw_b_min={} raw_b_max={} diff_min={} diff_max={} diff_p2p={} dc_offset={:.2} noise_floor={:.2} gate_threshold={:.2} gate_open_samples={} gate_open_percent={:.1} final_gain={:.2} clipped_samples={} saturation_a_count={} saturation_b_count={} saturation_percent={:.2} sd_write_time_ms={} ram_first={}",
        stats.mode,
        adc_atten_description(config::RECORD_ADC_ATTEN_MODE),
        stats.samples,
        stats.raw_a_min,
        stats.raw_a_max,
        stats.raw_b_min,
        stats.raw_b_max,
        stats.diff_min,
        stats.diff_max,
        stats.diff_max - stats.diff_min,
        stats.dc_offset,
        stats.noise_floor,
        gate_threshold,
        stats.gate_open_samples,
        gate_percent,
        stats.final_gain,
        stats.clipped_samples,
        stats.saturation_a_count,
        stats.saturation_b_count,
        saturation_percent,
        write_time_ms,
        config::RECORD_TO_RAM_FIRST
    );
    if saturation_percent > 1.0 {
        warn!("RECORD RECOMMENDATION ADC saturation high: try higher RECORD_ADC_ATTEN_MODE");
    }
    if gate_percent < 1.0 {
        warn!("RECORD RECOMMENDATION Gate too strict or signal too weak: lower NOISE_GATE_MULTIPLIER or speak louder");
    }

    Ok(processed.len() as u32)
}

pub fn mic_mode_scan() -> Result<u8> {
    info!(
        "MIC_SCAN start atten={} test_ms=250 oversample={}",
        adc_atten_description(config::RECORD_ADC_ATTEN_MODE),
        config::RECORD_OVERSAMPLE
    );

    let mut best_mode = 4_u8;
    let mut best_score = f32::MIN;

    for mode in 0_u8..=4 {
        hw::prepare_recording(mode, config::RECORD_ADC_ATTEN_MODE)?;
        let metrics = scan_mode_metrics(mode, 250)?;
        let saturation_penalty = metrics.saturation_count as f32 * 0.25;
        let signal_score = metrics.p2p as f32 / (metrics.noise_floor + 1.0);
        let score = signal_score - saturation_penalty;

        if score > best_score {
            best_score = score;
            best_mode = mode;
        }

        info!(
            "MIC_SCAN mode={} desc=\"{}\" noise={:.2} p2p={} saturation_count={} score={:.2}",
            mode,
            mic_mode_description(mode),
            metrics.noise_floor,
            metrics.p2p,
            metrics.saturation_count,
            score
        );
    }

    info!(
        "MIC_SCAN selected=true mode={} score={:.2}",
        best_mode, best_score
    );
    Ok(best_mode)
}

pub fn play_wav_blocking(path: &Path) -> Result<()> {
    info!("PLAYBACK START file={}", path.display());

    let file = OpenOptions::new().read(true).open(path).with_context(|| {
        format!(
            "WAV-Datei konnte nicht geoeffnet werden: {}",
            path.display()
        )
    })?;

    let mut reader = BufReader::new(file);
    let info = wav::read_info(&mut reader)?;
    let metrics = scan_playback_metrics(&mut reader, info)?;
    let input_peak_norm = metrics.peak as f32 / 32768.0;
    let estimated_gain = playback_gain_for_levels(input_peak_norm, metrics.rms);

    info!(
        "PLAYBACK FORMAT file={} format=PCM channels={} sample_rate={} bits_per_sample={} block_align={} frames={} data_bytes={}",
        path.display(),
        info.channels,
        info.sample_rate_hz,
        info.bits_per_sample,
        info.block_align,
        metrics.frames,
        info.data_len
    );
    info!(
        "PLAYBACK DSP input_peak={} input_peak_norm={:.3} input_rms={:.3} base_gain={:.2} estimated_gain={:.2} hard_clip={} pre_emphasis={} remove_lowpass={} noise_shaping={}",
        metrics.peak,
        input_peak_norm,
        metrics.rms,
        config::PLAYBACK_GAIN,
        estimated_gain,
        config::PLAYBACK_HARD_CLIP,
        config::PLAYBACK_PREEMPHASIS,
        config::PLAYBACK_REMOVE_LOWPASS_FOR_LOUDNESS,
        config::PLAYBACK_NOISE_SHAPING
    );
    info!(
        "PLAYBACK OUTPUT mode={} desc=\"{}\" button_policy=ignored",
        config::PLAYBACK_OUTPUT_MODE,
        playback_mode_description(config::PLAYBACK_OUTPUT_MODE)
    );

    if info.sample_rate_hz != config::RECORD_SAMPLE_RATE {
        warn!(
            "PLAYBACK RATE input_hz={} record_hz={} resampler=none output_timing=input_header",
            info.sample_rate_hz,
            config::RECORD_SAMPLE_RATE
        );
    }

    reader.seek(SeekFrom::Start(info.data_start))?;
    hw::prepare_playback(config::PLAYBACK_OUTPUT_MODE, config::PLAYBACK_PWM_FREQ_MODE)?;

    let period_us = (1_000_000_u64 / info.sample_rate_hz.max(1) as u64).max(1);
    let mut remaining_frames = metrics.frames as usize;
    let mut scratch = Vec::new();
    let mut samples = Vec::with_capacity(config::PLAYBACK_BLOCK_SAMPLES);
    let mut processor = PlaybackProcessor::default();
    let mut stats = PlaybackStats::new(metrics.peak, metrics.rms);
    let mut next_sample = hw::micros();

    while remaining_frames > 0 {
        let want_frames = remaining_frames.min(config::PLAYBACK_BLOCK_SAMPLES);
        read_pcm_block(&mut reader, info, want_frames, &mut scratch, &mut samples)?;
        if samples.is_empty() {
            break;
        }

        let (block_peak, block_rms) = block_levels(&samples);
        let gain = playback_gain_for_levels(block_peak, block_rms);
        stats.observe_gain(gain);

        for &sample in &samples {
            let normalized = sample as f32 / 32768.0;
            let processed = processor.process(normalized, gain, &mut stats);
            let out = processor.quantize(processed);

            wait_until(next_sample);
            next_sample = next_sample.wrapping_add(period_us);
            hw::pwm_set_sample(out)?;
            stats.observe_output(processed, out);
        }

        remaining_frames -= samples.len();
    }

    hw::pwm_stop().ok();
    info!(
        "PLAYBACK END samples={} input_peak={} input_rms={:.3} output_peak={:.3} output_rms={:.3} pwm_min={} pwm_max={} min_gain={:.2} max_gain={:.2} avg_gain={:.2} limiter_active={} clipped_samples={}",
        stats.output_samples,
        stats.input_peak,
        stats.input_rms,
        stats.output_peak,
        stats.output_rms(),
        stats.pwm_min,
        stats.pwm_max,
        stats.min_gain,
        stats.max_gain,
        stats.average_gain(),
        stats.clipped_samples > 0,
        stats.clipped_samples
    );
    Ok(())
}

fn calibrate_mic(mode: u8) -> Result<Calibration> {
    let count = ((config::RECORD_SAMPLE_RATE as u64 * config::RECORD_CALIBRATION_MS as u64) / 1000)
        .max(16) as usize;
    let mut samples = Vec::with_capacity(count);
    let mut next_sample = hw::micros();

    for index in 0..count {
        wait_until(next_sample);
        next_sample = next_sample.wrapping_add(SAMPLE_PERIOD_US);
        samples.push(read_oversampled_raw(mode)?);
        if (index + 1) % YIELD_EVERY_SAMPLES == 0 {
            scheduler_yield();
            next_sample = hw::micros().wrapping_add(SAMPLE_PERIOD_US);
        }
    }

    Ok(Calibration::from_samples(&samples))
}

fn scan_mode_metrics(mode: u8, ms: u32) -> Result<ScanMetrics> {
    let count = ((config::RECORD_SAMPLE_RATE as u64 * ms as u64) / 1000).max(16) as usize;
    let mut samples = Vec::with_capacity(count);
    let mut next_sample = hw::micros();

    for index in 0..count {
        wait_until(next_sample);
        next_sample = next_sample.wrapping_add(SAMPLE_PERIOD_US);
        samples.push(read_oversampled_raw(mode)?);
        if (index + 1) % YIELD_EVERY_SAMPLES == 0 {
            scheduler_yield();
            next_sample = hw::micros().wrapping_add(SAMPLE_PERIOD_US);
        }
    }

    let cal = Calibration::from_samples(&samples);
    let saturation_count = samples
        .iter()
        .filter(|sample| is_saturated(sample.raw_a) || is_saturated(sample.raw_b))
        .count() as u32;

    Ok(ScanMetrics {
        noise_floor: cal.noise_floor,
        p2p: cal.diff_max - cal.diff_min,
        saturation_count,
    })
}

fn read_oversampled_raw(mode: u8) -> Result<RawSample> {
    let mut raw_a = [0_i32; config::RECORD_OVERSAMPLE];
    let mut raw_b = [0_i32; config::RECORD_OVERSAMPLE];
    let mut signal = [0_i32; config::RECORD_OVERSAMPLE];

    for i in 0..config::RECORD_OVERSAMPLE {
        let (a, b) = hw::adc_read_pair()?;
        raw_a[i] = a;
        raw_b[i] = b;
        signal[i] = signal_from_pair(mode, a, b);
    }

    raw_a.sort_unstable();
    raw_b.sort_unstable();
    signal.sort_unstable();

    Ok(RawSample {
        raw_a: trimmed_mean(&raw_a),
        raw_b: trimmed_mean(&raw_b),
        signal: trimmed_mean(&signal),
    })
}

fn signal_from_pair(mode: u8, raw_a: i32, raw_b: i32) -> i32 {
    match mode {
        3 => raw_b,
        4 => raw_a - raw_b,
        _ => raw_a,
    }
}

fn trimmed_mean(values: &[i32]) -> i32 {
    let slice = if values.len() >= 6 {
        &values[1..values.len() - 1]
    } else {
        values
    };
    slice.iter().sum::<i32>() / slice.len() as i32
}

fn write_wav_file(path: &Path, data: &[u8]) -> Result<()> {
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
    writer.write_all(data)?;
    writer.flush()?;
    let mut file = writer.into_inner()?;
    wav::finalize_header(&mut file, data.len() as u32)?;
    file.flush()?;
    Ok(())
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

fn scan_playback_metrics<R: Read + Seek>(
    reader: &mut R,
    info: wav::WavInfo,
) -> Result<PlaybackMetrics> {
    reader.seek(SeekFrom::Start(info.data_start))?;

    let mut remaining_frames = info.frame_count()? as usize;
    let mut scratch = Vec::new();
    let mut samples = Vec::with_capacity(config::PLAYBACK_BLOCK_SAMPLES);
    let mut peak = 0_i32;
    let mut sum_sq = 0.0_f64;
    let mut frames = 0_u32;

    while remaining_frames > 0 {
        let want_frames = remaining_frames.min(config::PLAYBACK_BLOCK_SAMPLES);
        read_pcm_block(reader, info, want_frames, &mut scratch, &mut samples)?;
        for &sample in &samples {
            let abs = sample.unsigned_abs() as i32;
            peak = peak.max(abs);
            let unit = sample as f64 / 32768.0;
            sum_sq += unit * unit;
            frames += 1;
        }
        remaining_frames -= samples.len();
    }

    let rms = if frames == 0 {
        0.0
    } else {
        (sum_sq / frames as f64).sqrt() as f32
    };

    Ok(PlaybackMetrics { peak, rms, frames })
}

fn block_levels(samples: &[i16]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let mut peak = 0_i32;
    let mut sum_sq = 0.0_f64;
    for &sample in samples {
        peak = peak.max(sample.unsigned_abs() as i32);
        let unit = sample as f64 / 32768.0;
        sum_sq += unit * unit;
    }

    (
        peak as f32 / 32768.0,
        (sum_sq / samples.len() as f64).sqrt() as f32,
    )
}

fn playback_gain_for_levels(peak: f32, rms: f32) -> f32 {
    let peak_gain = if config::PLAYBACK_AUTO_NORMALIZE && peak > 0.001 {
        (config::PLAYBACK_AUTO_TARGET / peak).clamp(1.0, config::PLAYBACK_AUTO_GAIN_MAX)
    } else {
        1.0
    };
    let rms_gain = if config::PLAYBACK_EXTREME_LOUDNESS && rms > 0.001 {
        (PLAYBACK_RMS_TARGET / rms).clamp(1.0, config::PLAYBACK_AUTO_GAIN_MAX)
    } else {
        1.0
    };

    config::PLAYBACK_GAIN * peak_gain.max(rms_gain)
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

fn scheduler_yield() {
    thread::sleep(Duration::from_millis(1));
}

fn is_saturated(raw: i32) -> bool {
    raw <= ADC_SATURATION_LOW || raw >= ADC_SATURATION_HIGH
}

fn raw_to_debug_u8(raw: i32) -> u8 {
    (raw.clamp(0, 4095) >> 4) as u8
}

fn diff_to_debug_u8(diff: i32) -> u8 {
    ((diff / 32) + 128).clamp(0, 255) as u8
}

fn mic_mode_description(mode: u8) -> &'static str {
    match mode {
        0 => "GPIO4 ADC, GPIO5 schwacher Pulldown",
        1 => "GPIO4 ADC, GPIO5 High-Z",
        2 => "GPIO4 ADC, GPIO5 schwacher Pullup",
        3 => "GPIO5 ADC, GPIO4 schwacher Pulldown",
        4 => "PSEUDO_DIFF GPIO4 ADC minus GPIO5 ADC, beide High-Z",
        5 => "AUTO_SCAN Modi 0..4",
        _ => "ungueltig",
    }
}

fn adc_atten_description(mode: u8) -> &'static str {
    match mode {
        0 => "ADC_ATTEN_DB_0",
        1 => "ADC_ATTEN_DB_2_5",
        2 => "ADC_ATTEN_DB_6",
        3 => "ADC_ATTEN_DB_12",
        _ => "ADC_ATTEN_DB_0(default)",
    }
}

fn playback_mode_description(mode: u8) -> &'static str {
    match mode {
        0 => "LEDC normal 62.5 kHz",
        1 => "MAX_LOUD_LEDC 31.25 kHz",
        2 => "EXPERIMENTAL_SDM_PDM requested, native fallback to MAX_LOUD_LEDC",
        _ => "unknown, native fallback",
    }
}

fn playback_pwm_freq_description(mode: u8) -> &'static str {
    match mode {
        0 => "62500 Hz",
        1 => "31250 Hz",
        2 => "15625 Hz very experimental",
        _ => "native default",
    }
}

#[derive(Clone, Copy)]
struct RawSample {
    raw_a: i32,
    raw_b: i32,
    signal: i32,
}

#[derive(Clone, Copy)]
struct Calibration {
    dc_offset: f32,
    noise_floor: f32,
    raw_a_min: i32,
    raw_a_max: i32,
    raw_b_min: i32,
    raw_b_max: i32,
    diff_min: i32,
    diff_max: i32,
    samples: u32,
}

impl Calibration {
    fn from_samples(samples: &[RawSample]) -> Self {
        let mut raw_a_min = i32::MAX;
        let mut raw_a_max = i32::MIN;
        let mut raw_b_min = i32::MAX;
        let mut raw_b_max = i32::MIN;
        let mut diff_min = i32::MAX;
        let mut diff_max = i32::MIN;
        let mut sum = 0_i64;

        for &sample in samples {
            raw_a_min = raw_a_min.min(sample.raw_a);
            raw_a_max = raw_a_max.max(sample.raw_a);
            raw_b_min = raw_b_min.min(sample.raw_b);
            raw_b_max = raw_b_max.max(sample.raw_b);
            diff_min = diff_min.min(sample.signal);
            diff_max = diff_max.max(sample.signal);
            sum += sample.signal as i64;
        }

        let count = samples.len().max(1);
        let dc_offset = sum as f32 / count as f32;
        let noise_floor = samples
            .iter()
            .map(|sample| (sample.signal as f32 - dc_offset).abs())
            .sum::<f32>()
            / count as f32;

        Self {
            dc_offset,
            noise_floor: noise_floor.max(0.5),
            raw_a_min,
            raw_a_max,
            raw_b_min,
            raw_b_max,
            diff_min,
            diff_max,
            samples: samples.len() as u32,
        }
    }
}

struct ScanMetrics {
    noise_floor: f32,
    p2p: i32,
    saturation_count: u32,
}

struct RecordStats {
    mode: u8,
    samples: u32,
    raw_a_min: i32,
    raw_a_max: i32,
    raw_b_min: i32,
    raw_b_max: i32,
    diff_min: i32,
    diff_max: i32,
    dc_offset: f32,
    noise_floor: f32,
    final_gain: f32,
    gate_open_samples: u32,
    clipped_samples: u32,
    saturation_a_count: u32,
    saturation_b_count: u32,
    saturation_sample_count: u32,
    written_bytes: u32,
}

impl RecordStats {
    fn new(mode: u8, calibration: Calibration) -> Self {
        Self {
            mode,
            samples: 0,
            raw_a_min: i32::MAX,
            raw_a_max: i32::MIN,
            raw_b_min: i32::MAX,
            raw_b_max: i32::MIN,
            diff_min: i32::MAX,
            diff_max: i32::MIN,
            dc_offset: calibration.dc_offset,
            noise_floor: calibration.noise_floor,
            final_gain: config::RECORD_INITIAL_GAIN,
            gate_open_samples: 0,
            clipped_samples: 0,
            saturation_a_count: 0,
            saturation_b_count: 0,
            saturation_sample_count: 0,
            written_bytes: 0,
        }
    }

    fn observe_raw(&mut self, raw: RawSample) {
        self.samples += 1;
        self.raw_a_min = self.raw_a_min.min(raw.raw_a);
        self.raw_a_max = self.raw_a_max.max(raw.raw_a);
        self.raw_b_min = self.raw_b_min.min(raw.raw_b);
        self.raw_b_max = self.raw_b_max.max(raw.raw_b);
        self.diff_min = self.diff_min.min(raw.signal);
        self.diff_max = self.diff_max.max(raw.signal);
        let saturated_a = is_saturated(raw.raw_a);
        let saturated_b = is_saturated(raw.raw_b);
        if saturated_a {
            self.saturation_a_count += 1;
        }
        if saturated_b {
            self.saturation_b_count += 1;
        }
        if saturated_a || saturated_b {
            self.saturation_sample_count += 1;
        }
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

    fn process(&mut self, signal: i32, stats: &mut RecordStats) -> u8 {
        let centered = signal as f32 - self.dc_offset;
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
            self.gain += 0.002 * (config::RECORD_GAIN_MIN - self.gain);
            self.lp_y * config::NOISE_GATE_ATTENUATION
        };

        if gate_open && abs > 0.5 {
            let desired = (config::RECORD_TARGET_LEVEL / abs)
                .clamp(config::RECORD_GAIN_MIN, config::RECORD_GAIN_MAX);
            let rate = if desired < self.gain { 0.10 } else { 0.0015 };
            self.gain += rate * (desired - self.gain);
        }

        let amplified = gated * self.gain;
        let limited = amplified.clamp(-127.0, 127.0);
        if (limited - amplified).abs() > f32::EPSILON {
            stats.clipped_samples += 1;
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
    pre_prev_x: f32,
    quant_error: f32,
}

impl PlaybackProcessor {
    fn process(&mut self, sample: f32, gain: f32, stats: &mut PlaybackStats) -> f32 {
        let hp = sample - self.hp_prev_x + PLAYBACK_HIGHPASS_ALPHA * self.hp_prev_y;
        self.hp_prev_x = sample;
        self.hp_prev_y = hp;

        let mut y = if config::PLAYBACK_PREEMPHASIS {
            let emphasized = hp - 0.75 * self.pre_prev_x;
            self.pre_prev_x = hp;
            emphasized
        } else {
            hp
        };

        if !config::PLAYBACK_REMOVE_LOWPASS_FOR_LOUDNESS {
            self.lp_y += PLAYBACK_LOWPASS_ALPHA * (y - self.lp_y);
            y = self.lp_y;
        }

        y *= gain;

        if config::PLAYBACK_COMPRESSOR_ENABLED {
            y = compress(y);
        }

        if config::PLAYBACK_EXTREME_LOUDNESS {
            y = center_clip_and_expand(y);
        }

        if config::PLAYBACK_HARD_CLIP {
            if y.abs() > config::PLAYBACK_LIMIT {
                stats.clipped_samples += 1;
            }
            y = y.clamp(-config::PLAYBACK_LIMIT, config::PLAYBACK_LIMIT);
        } else if config::PLAYBACK_LIMITER_ENABLED && y.abs() > config::PLAYBACK_LIMIT {
            stats.clipped_samples += 1;
            y = y.clamp(-config::PLAYBACK_LIMIT, config::PLAYBACK_LIMIT);
        }

        y.clamp(-1.0, 1.0)
    }

    fn quantize(&mut self, sample: f32) -> u8 {
        let shaped = if config::PLAYBACK_NOISE_SHAPING {
            (sample + self.quant_error * 0.75).clamp(-1.0, 1.0)
        } else {
            sample
        };
        let signed = (shaped.clamp(-1.0, 1.0) * 127.0).round() as i32;
        let out = (signed + 128).clamp(0, 255) as u8;

        if config::PLAYBACK_NOISE_SHAPING {
            let quantized = (out as f32 - 128.0) / 127.0;
            self.quant_error = shaped - quantized;
        }

        out
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

fn center_clip_and_expand(sample: f32) -> f32 {
    let sign = sample.signum();
    let abs = sample.abs();
    if abs < config::PLAYBACK_CENTER_CLIP_THRESHOLD {
        0.0
    } else {
        let normalized = ((abs - config::PLAYBACK_CENTER_CLIP_THRESHOLD)
            / (1.0 - config::PLAYBACK_CENTER_CLIP_THRESHOLD))
            .clamp(0.0, 1.0);
        sign * normalized.powf(config::PLAYBACK_EXPAND_POWER)
    }
}

struct PlaybackMetrics {
    peak: i32,
    rms: f32,
    frames: u32,
}

struct PlaybackStats {
    input_peak: i32,
    input_rms: f32,
    output_samples: u32,
    output_peak: f32,
    output_sum_sq: f64,
    pwm_min: u8,
    pwm_max: u8,
    clipped_samples: u32,
    min_gain: f32,
    max_gain: f32,
    gain_sum: f32,
    gain_blocks: u32,
}

impl PlaybackStats {
    fn new(input_peak: i32, input_rms: f32) -> Self {
        Self {
            input_peak,
            input_rms,
            output_samples: 0,
            output_peak: 0.0,
            output_sum_sq: 0.0,
            pwm_min: u8::MAX,
            pwm_max: u8::MIN,
            clipped_samples: 0,
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

    fn observe_output(&mut self, sample: f32, pwm: u8) {
        self.output_samples += 1;
        self.output_peak = self.output_peak.max(sample.abs());
        self.output_sum_sq += (sample as f64) * (sample as f64);
        self.pwm_min = self.pwm_min.min(pwm);
        self.pwm_max = self.pwm_max.max(pwm);
    }

    fn average_gain(&self) -> f32 {
        if self.gain_blocks == 0 {
            0.0
        } else {
            self.gain_sum / self.gain_blocks as f32
        }
    }

    fn output_rms(&self) -> f32 {
        if self.output_samples == 0 {
            0.0
        } else {
            (self.output_sum_sq / self.output_samples as f64).sqrt() as f32
        }
    }
}

struct DebugCapture {
    raw_a: Option<Vec<u8>>,
    raw_b: Option<Vec<u8>>,
    diff: Option<Vec<u8>>,
    proc: Option<Vec<u8>>,
}

impl DebugCapture {
    fn new() -> Self {
        if config::WRITE_DEBUG_RAW_WAV {
            Self {
                raw_a: Some(Vec::new()),
                raw_b: Some(Vec::new()),
                diff: Some(Vec::new()),
                proc: Some(Vec::new()),
            }
        } else {
            Self {
                raw_a: None,
                raw_b: None,
                diff: None,
                proc: None,
            }
        }
    }

    fn push(&mut self, raw: RawSample, processed: u8) {
        if let Some(buf) = self.raw_a.as_mut() {
            buf.push(raw_to_debug_u8(raw.raw_a));
        }
        if let Some(buf) = self.raw_b.as_mut() {
            buf.push(raw_to_debug_u8(raw.raw_b));
        }
        if let Some(buf) = self.diff.as_mut() {
            buf.push(diff_to_debug_u8(raw.signal));
        }
        if let Some(buf) = self.proc.as_mut() {
            buf.push(processed);
        }
    }

    fn write_files(self, record_path: &Path) -> Result<()> {
        if let Some(data) = self.raw_a {
            write_debug_wav(record_path, "RAW_A", &data)?;
        }
        if let Some(data) = self.raw_b {
            write_debug_wav(record_path, "RAW_B", &data)?;
        }
        if let Some(data) = self.diff {
            write_debug_wav(record_path, "DIFF", &data)?;
        }
        if let Some(data) = self.proc {
            write_debug_wav(record_path, "PROC", &data)?;
        }
        Ok(())
    }
}

fn write_debug_wav(record_path: &Path, prefix: &str, data: &[u8]) -> Result<()> {
    let Some(file_name) = record_path.file_name() else {
        return Ok(());
    };
    let stem = file_name.to_string_lossy().replace(".WAV", "");
    let debug_path: PathBuf = record_path.with_file_name(format!("{prefix}_{stem}.WAV"));
    write_wav_file(&debug_path, data)?;
    info!(
        "RECORD DEBUG path={} bytes={}",
        debug_path.display(),
        data.len()
    );
    Ok(())
}
