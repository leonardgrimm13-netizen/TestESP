#pragma once

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int yd_hw_init_button(void);
bool yd_button_is_pressed(void);

int yd_sd_mount(void);
void yd_sd_unmount(void);

int yd_led_init(void);
void yd_led_set_rgb(uint8_t r, uint8_t g, uint8_t b);
void yd_led_off(void);

int yd_audio_record_prepare(void);
int yd_audio_record_prepare_mode(uint8_t mic_mode);
int yd_audio_record_prepare_mode_atten(uint8_t mic_mode, uint8_t atten_mode);
int yd_audio_playback_prepare(void);
int yd_audio_playback_prepare_mode(uint8_t mode);
int yd_audio_playback_prepare_mode_freq(uint8_t mode, uint8_t pwm_freq_mode);
int yd_audio_idle(void);

int yd_adc_read_gpio4(int *raw);
int yd_adc_read_active(int *raw);
int yd_adc_read_pair(int *raw_a, int *raw_b);

int yd_pwm_set_sample(uint8_t sample);
int yd_pwm_stop(void);

uint32_t yd_millis(void);
uint64_t yd_micros(void);
void yd_delay_us(uint32_t us);

#ifdef __cplusplus
}
#endif
