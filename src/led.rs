use crate::hw;
use anyhow::Result;
use std::thread;
use std::time::Duration;

pub struct StatusLed;

impl StatusLed {
    pub fn init() -> Result<Self> {
        hw::led_init()?;
        hw::led_off();
        Ok(Self)
    }

    pub fn idle(&mut self) {
        hw::led_off();
    }

    pub fn recording(&mut self) {
        hw::led_rgb(0, 64, 0);
    }

    pub fn playing(&mut self) {
        hw::led_rgb(64, 0, 0);
    }

    pub fn error_yellow(&mut self) {
        hw::led_rgb(48, 32, 0);
    }

    pub fn blink_sd_error_once(&mut self) {
        for _ in 0..3 {
            hw::led_rgb(0, 0, 48);
            thread::sleep(Duration::from_millis(150));
            hw::led_off();
            thread::sleep(Duration::from_millis(150));
        }
    }
}
