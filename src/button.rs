#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ButtonEvent {
    ShortPress,
    LongPress,
}

pub struct DebouncedButton {
    last_raw_pressed: bool,
    stable_pressed: bool,
    last_raw_change_ms: u32,
    pressed_since_ms: Option<u32>,
    long_already_reported: bool,
    debounce_ms: u32,
    long_press_ms: u32,
}

impl DebouncedButton {
    pub fn new(now_ms: u32) -> Self {
        Self {
            last_raw_pressed: false,
            stable_pressed: false,
            last_raw_change_ms: now_ms,
            pressed_since_ms: None,
            long_already_reported: false,
            debounce_ms: 30,
            long_press_ms: 500,
        }
    }

    pub fn poll(&mut self, now_ms: u32, raw_pressed: bool) -> Option<ButtonEvent> {
        if raw_pressed != self.last_raw_pressed {
            self.last_raw_pressed = raw_pressed;
            self.last_raw_change_ms = now_ms;
        }

        if elapsed_ms(now_ms, self.last_raw_change_ms) >= self.debounce_ms
            && self.stable_pressed != self.last_raw_pressed
        {
            self.stable_pressed = self.last_raw_pressed;

            if self.stable_pressed {
                self.pressed_since_ms = Some(now_ms);
                self.long_already_reported = false;
            } else if let Some(start) = self.pressed_since_ms.take() {
                let held_ms = elapsed_ms(now_ms, start);
                if held_ms < self.long_press_ms && !self.long_already_reported {
                    return Some(ButtonEvent::ShortPress);
                }
            }
        }

        if self.stable_pressed && !self.long_already_reported {
            if let Some(start) = self.pressed_since_ms {
                if elapsed_ms(now_ms, start) >= self.long_press_ms {
                    self.long_already_reported = true;
                    return Some(ButtonEvent::LongPress);
                }
            }
        }

        None
    }
}

fn elapsed_ms(now: u32, then: u32) -> u32 {
    now.wrapping_sub(then)
}
