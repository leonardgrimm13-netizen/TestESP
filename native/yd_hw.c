#include "yd_hw.h"

#include <string.h>

#include "driver/gpio.h"
#include "driver/ledc.h"
#include "driver/sdspi_host.h"
#include "driver/spi_common.h"
#include "esp_adc/adc_oneshot.h"
#include "esp_err.h"
#include "esp_log.h"
#include "esp_rom_sys.h"
#include "esp_timer.h"
#include "esp_vfs_fat.h"
#include "freertos/FreeRTOS.h"
#include "sdmmc_cmd.h"

#define PIN_AUDIO_A GPIO_NUM_4
#define PIN_AUDIO_B GPIO_NUM_5
#define PIN_SD_CS GPIO_NUM_10
#define PIN_SD_MOSI GPIO_NUM_11
#define PIN_SD_CLK GPIO_NUM_12
#define PIN_SD_MISO GPIO_NUM_13
#define PIN_BUTTON GPIO_NUM_21
#define PIN_RGB GPIO_NUM_48

#define SD_MOUNT_POINT "/sdcard"
#define PWM_FREQ_HZ 62500
#define PWM_TIMER LEDC_TIMER_0
#define PWM_MODE LEDC_LOW_SPEED_MODE
#define PWM_CH_A LEDC_CHANNEL_0
#define PWM_CH_B LEDC_CHANNEL_1

static const char *TAG = "yd_hw";

static sdmmc_card_t *s_card = NULL;
static bool s_spi_bus_initialized = false;
static adc_oneshot_unit_handle_t s_adc1 = NULL;
static bool s_adc_ch3_configured = false;
static bool s_adc_ch4_configured = false;
static adc_channel_t s_adc_channel = ADC_CHANNEL_3;
static bool s_pwm_ready = false;

int yd_hw_init_button(void) {
    gpio_config_t cfg = {
        .pin_bit_mask = 1ULL << PIN_BUTTON,
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    return gpio_config(&cfg);
}

bool yd_button_is_pressed(void) {
    return gpio_get_level(PIN_BUTTON) == 0;
}

int yd_sd_mount(void) {
    if (s_card != NULL) {
        return ESP_OK;
    }

    sdmmc_host_t host = SDSPI_HOST_DEFAULT();

    spi_bus_config_t bus_cfg = {
        .mosi_io_num = PIN_SD_MOSI,
        .miso_io_num = PIN_SD_MISO,
        .sclk_io_num = PIN_SD_CLK,
        .quadwp_io_num = -1,
        .quadhd_io_num = -1,
        .data4_io_num = -1,
        .data5_io_num = -1,
        .data6_io_num = -1,
        .data7_io_num = -1,
        .max_transfer_sz = 4096,
        .flags = SPICOMMON_BUSFLAG_MASTER,
        .intr_flags = 0,
    };

    esp_err_t err = spi_bus_initialize(host.slot, &bus_cfg, SDSPI_DEFAULT_DMA);
    if (err == ESP_ERR_INVALID_STATE) {
        ESP_LOGW(TAG, "SPI bus was already initialized; continuing");
    } else if (err != ESP_OK) {
        ESP_LOGE(TAG, "spi_bus_initialize failed: %s", esp_err_to_name(err));
        return err;
    } else {
        s_spi_bus_initialized = true;
    }

    sdspi_device_config_t slot_config = SDSPI_DEVICE_CONFIG_DEFAULT();
    slot_config.gpio_cs = PIN_SD_CS;
    slot_config.host_id = host.slot;

    esp_vfs_fat_sdmmc_mount_config_t mount_config = {
        .format_if_mount_failed = false,
        .max_files = 5,
        .allocation_unit_size = 16 * 1024,
    };

    err = esp_vfs_fat_sdspi_mount(SD_MOUNT_POINT, &host, &slot_config, &mount_config, &s_card);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SD mount failed: %s", esp_err_to_name(err));
        if (s_spi_bus_initialized) {
            spi_bus_free(host.slot);
            s_spi_bus_initialized = false;
        }
        return err;
    }

    return ESP_OK;
}

void yd_sd_unmount(void) {
    if (s_card != NULL) {
        esp_vfs_fat_sdcard_unmount(SD_MOUNT_POINT, s_card);
        s_card = NULL;
    }

    if (s_spi_bus_initialized) {
        sdmmc_host_t host = SDSPI_HOST_DEFAULT();
        spi_bus_free(host.slot);
        s_spi_bus_initialized = false;
    }
}

int yd_led_init(void) {
    // RGB/WS2812 ist vorübergehend deaktiviert.
    // Grund: alter RMT-Code mit driver/rmt.h passt nicht sauber zu ESP-IDF 5.2.x.
    // Audio, SD und Button sollen zuerst kompilieren. LED bauen wir danach sauber neu.
    return ESP_OK;
}

void yd_led_set_rgb(uint8_t r, uint8_t g, uint8_t b) {
    (void)r;
    (void)g;
    (void)b;
}

void yd_led_off(void) {
}

static int yd_adc_init_if_needed(void) {
    if (s_adc1 != NULL) {
        return ESP_OK;
    }

    adc_oneshot_unit_init_cfg_t init_cfg = {
        .unit_id = ADC_UNIT_1,
        .ulp_mode = ADC_ULP_MODE_DISABLE,
    };

    esp_err_t err = adc_oneshot_new_unit(&init_cfg, &s_adc1);
    if (err != ESP_OK) {
        return err;
    }

    return ESP_OK;
}

static int yd_adc_config_channel_if_needed(adc_channel_t channel) {
    esp_err_t err = yd_adc_init_if_needed();
    if (err != ESP_OK) {
        return err;
    }

    bool *configured = NULL;
    if (channel == ADC_CHANNEL_3) {
        configured = &s_adc_ch3_configured;
    } else if (channel == ADC_CHANNEL_4) {
        configured = &s_adc_ch4_configured;
    } else {
        return ESP_ERR_INVALID_ARG;
    }

    if (*configured) {
        return ESP_OK;
    }

    adc_oneshot_chan_cfg_t chan_cfg = {
        .atten = ADC_ATTEN_DB_12,
        .bitwidth = ADC_BITWIDTH_DEFAULT,
    };

    err = adc_oneshot_config_channel(s_adc1, channel, &chan_cfg);
    if (err == ESP_OK) {
        *configured = true;
    }
    return err;
}

int yd_audio_record_prepare(void) {
    return yd_audio_record_prepare_mode(0);
}

int yd_audio_record_prepare_mode(uint8_t mic_mode) {
    yd_pwm_stop();

    // Der S3 hat hier keinen nutzbaren differenziellen ADC. Der Lautsprecher
    // liefert als Mikrofon ohne Bias/Vorverstaerker nur ein sehr kleines Signal.
    // Die Modi testen unterschiedliche schwache Referenz-Zustaende am zweiten
    // Lautsprecherpin. Es wird bewusst kein Pin als starker Ausgang betrieben.
    gpio_num_t ref_pin = PIN_AUDIO_B;
    gpio_pullup_t pull_up = GPIO_PULLUP_DISABLE;
    gpio_pulldown_t pull_down = GPIO_PULLDOWN_ENABLE;
    adc_channel_t channel = ADC_CHANNEL_3; // GPIO4 auf ESP32-S3.

    switch (mic_mode) {
        case 0:
            ref_pin = PIN_AUDIO_B;
            pull_up = GPIO_PULLUP_DISABLE;
            pull_down = GPIO_PULLDOWN_ENABLE;
            channel = ADC_CHANNEL_3;
            break;
        case 1:
            ref_pin = PIN_AUDIO_B;
            pull_up = GPIO_PULLUP_DISABLE;
            pull_down = GPIO_PULLDOWN_DISABLE;
            channel = ADC_CHANNEL_3;
            break;
        case 2:
            ref_pin = PIN_AUDIO_B;
            pull_up = GPIO_PULLUP_ENABLE;
            pull_down = GPIO_PULLDOWN_DISABLE;
            channel = ADC_CHANNEL_3;
            break;
        case 3:
            // ESP32-S3: GPIO5 entspricht ADC1_CH4. Hier wird GPIO5 gemessen
            // und GPIO4 bekommt nur einen schwachen Pulldown als Referenz.
            ref_pin = PIN_AUDIO_A;
            pull_up = GPIO_PULLUP_DISABLE;
            pull_down = GPIO_PULLDOWN_ENABLE;
            channel = ADC_CHANNEL_4;
            break;
        default:
            return ESP_ERR_INVALID_ARG;
    }

    gpio_config_t ref_cfg = {
        .pin_bit_mask = 1ULL << ref_pin,
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = pull_up,
        .pull_down_en = pull_down,
        .intr_type = GPIO_INTR_DISABLE,
    };
    esp_err_t err = gpio_config(&ref_cfg);
    if (err != ESP_OK) {
        return err;
    }

    s_adc_channel = channel;
    return yd_adc_config_channel_if_needed(channel);
}

int yd_adc_read_gpio4(int *raw) {
    esp_err_t err = yd_adc_config_channel_if_needed(ADC_CHANNEL_3);
    if (err != ESP_OK) {
        return err;
    }
    return adc_oneshot_read(s_adc1, ADC_CHANNEL_3, raw);
}

int yd_adc_read_active(int *raw) {
    esp_err_t err = yd_adc_config_channel_if_needed(s_adc_channel);
    if (err != ESP_OK) {
        return err;
    }
    return adc_oneshot_read(s_adc1, s_adc_channel, raw);
}

int yd_audio_playback_prepare(void) {
    return yd_pwm_set_sample(128);
}

int yd_pwm_set_sample(uint8_t sample) {
    if (!s_pwm_ready) {
        ledc_timer_config_t timer = {
            .speed_mode = PWM_MODE,
            .duty_resolution = LEDC_TIMER_8_BIT,
            .timer_num = PWM_TIMER,
            .freq_hz = PWM_FREQ_HZ,
            .clk_cfg = LEDC_AUTO_CLK,
        };
        esp_err_t err = ledc_timer_config(&timer);
        if (err != ESP_OK) {
            return err;
        }

        ledc_channel_config_t ch_a = {
            .gpio_num = PIN_AUDIO_A,
            .speed_mode = PWM_MODE,
            .channel = PWM_CH_A,
            .intr_type = LEDC_INTR_DISABLE,
            .timer_sel = PWM_TIMER,
            .duty = 128,
            .hpoint = 0,
        };
        err = ledc_channel_config(&ch_a);
        if (err != ESP_OK) {
            return err;
        }

        ledc_channel_config_t ch_b = {
            .gpio_num = PIN_AUDIO_B,
            .speed_mode = PWM_MODE,
            .channel = PWM_CH_B,
            .intr_type = LEDC_INTR_DISABLE,
            .timer_sel = PWM_TIMER,
            .duty = 128,
            .hpoint = 0,
        };
        err = ledc_channel_config(&ch_b);
        if (err != ESP_OK) {
            return err;
        }

        s_pwm_ready = true;
    }

    // Volle 8-bit-PWM-Ausnutzung im Gegentakt: GPIO4 bekommt den Sample-Duty,
    // GPIO5 den invertierten Duty. Das erzeugt keine zusaetzliche elektrische
    // Leistung; es nutzt nur die vorhandene Differenzspannung besser aus.
    // Ein direkt am GPIO betriebener Lautsprecher kann den ESP belasten, daher
    // begrenzt der Rust-Audiopfad den Signalpegel per Kompressor/Limiter.
    uint32_t duty_a = (uint32_t)sample;
    uint32_t duty_b = (uint32_t)(255U - sample);

    esp_err_t err = ledc_set_duty(PWM_MODE, PWM_CH_A, duty_a);
    if (err != ESP_OK) {
        return err;
    }
    err = ledc_update_duty(PWM_MODE, PWM_CH_A);
    if (err != ESP_OK) {
        return err;
    }
    err = ledc_set_duty(PWM_MODE, PWM_CH_B, duty_b);
    if (err != ESP_OK) {
        return err;
    }
    return ledc_update_duty(PWM_MODE, PWM_CH_B);
}

int yd_pwm_stop(void) {
    if (s_pwm_ready) {
        ledc_stop(PWM_MODE, PWM_CH_A, 0);
        ledc_stop(PWM_MODE, PWM_CH_B, 0);
        s_pwm_ready = false;
    }

    gpio_config_t input_cfg = {
        .pin_bit_mask = (1ULL << PIN_AUDIO_A) | (1ULL << PIN_AUDIO_B),
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    return gpio_config(&input_cfg);
}

int yd_audio_idle(void) {
    return yd_pwm_stop();
}

uint32_t yd_millis(void) {
    return (uint32_t)(esp_timer_get_time() / 1000ULL);
}

uint64_t yd_micros(void) {
    return (uint64_t)esp_timer_get_time();
}

void yd_delay_us(uint32_t us) {
    esp_rom_delay_us(us);
}
