use anyhow::{anyhow, Result};

extern "C" {
    fn yd_hw_init_button() -> i32;
    fn yd_button_is_pressed() -> bool;

    fn yd_sd_mount() -> i32;
    fn yd_sd_unmount();

    fn yd_led_init() -> i32;
    fn yd_led_set_rgb(r: u8, g: u8, b: u8);
    fn yd_led_off();

    fn yd_audio_record_prepare_mode_atten(mic_mode: u8, atten_mode: u8) -> i32;
    fn yd_audio_playback_prepare_mode_freq(mode: u8, pwm_freq_mode: u8) -> i32;
    fn yd_audio_idle() -> i32;

    fn yd_adc_read_pair(raw_a: *mut i32, raw_b: *mut i32) -> i32;

    fn yd_pwm_set_sample(sample: u8) -> i32;
    fn yd_pwm_stop() -> i32;

    fn yd_millis() -> u32;
    fn yd_micros() -> u64;
    fn yd_delay_us(us: u32);
}

fn check(code: i32, what: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "{what} fehlgeschlagen: ESP-IDF error {code} / 0x{code:08x}"
        ))
    }
}

pub fn init_button() -> Result<()> {
    check(unsafe { yd_hw_init_button() }, "Button-Init")
}

pub fn button_pressed() -> bool {
    unsafe { yd_button_is_pressed() }
}

pub fn sd_mount() -> Result<()> {
    check(unsafe { yd_sd_mount() }, "SD-Mount")
}

pub fn sd_unmount() {
    unsafe { yd_sd_unmount() }
}

pub fn led_init() -> Result<()> {
    check(unsafe { yd_led_init() }, "LED-Init")
}

pub fn led_rgb(r: u8, g: u8, b: u8) {
    unsafe { yd_led_set_rgb(r, g, b) }
}

pub fn led_off() {
    unsafe { yd_led_off() }
}

pub fn prepare_recording(mic_mode: u8, atten_mode: u8) -> Result<()> {
    check(
        unsafe { yd_audio_record_prepare_mode_atten(mic_mode, atten_mode) },
        "Audio-Aufnahme-Prepare",
    )
}

pub fn prepare_playback(mode: u8, pwm_freq_mode: u8) -> Result<()> {
    check(
        unsafe { yd_audio_playback_prepare_mode_freq(mode, pwm_freq_mode) },
        "Audio-Wiedergabe-Prepare",
    )
}

pub fn audio_idle() -> Result<()> {
    check(unsafe { yd_audio_idle() }, "Audio-Idle")
}

pub fn adc_read_pair() -> Result<(i32, i32)> {
    let mut raw_a = 0_i32;
    let mut raw_b = 0_i32;
    check(
        unsafe { yd_adc_read_pair(&mut raw_a as *mut i32, &mut raw_b as *mut i32) },
        "ADC-Read GPIO4/GPIO5 pair",
    )?;
    Ok((raw_a, raw_b))
}

pub fn pwm_set_sample(sample: u8) -> Result<()> {
    check(unsafe { yd_pwm_set_sample(sample) }, "PWM-Sample")
}

pub fn pwm_stop() -> Result<()> {
    check(unsafe { yd_pwm_stop() }, "PWM-Stop")
}

pub fn millis() -> u32 {
    unsafe { yd_millis() }
}

pub fn micros() -> u64 {
    unsafe { yd_micros() }
}

pub fn delay_us(us: u32) {
    unsafe { yd_delay_us(us) }
}
