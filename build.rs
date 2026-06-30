use std::env;
use std::path::{Path, PathBuf};

fn add(build: &mut cc::Build, idf: &Path, rel: &str) {
    let p = idf.join(rel);
    if p.exists() {
        build.include(p);
    } else {
        println!("cargo:warning=missing include: {}", p.display());
    }
}

fn find_esp_idf_path() -> PathBuf {
    if let Ok(idf_path) = env::var("IDF_PATH") {
        return PathBuf::from(idf_path);
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    manifest_dir.join(".embuild/espressif/esp-idf/v5.2.3")
}

fn find_esp_idf_build_config() -> Option<PathBuf> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").ok()?);
    let build_root = out_dir.parent()?.parent()?;

    for entry in std::fs::read_dir(build_root).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if name.starts_with("esp-idf-sys-") {
            let config = entry.path().join("out/build/config");
            if config.exists() {
                return Some(config);
            }
        }
    }

    None
}

fn main() {
    embuild::espidf::sysenv::output();

    println!("cargo:rerun-if-changed=native/yd_hw.c");
    println!("cargo:rerun-if-changed=native/yd_hw.h");

    let idf = find_esp_idf_path();
    println!("cargo:warning=Using ESP-IDF path: {}", idf.display());

    let mut build = cc::Build::new();

    // Wichtig: Nicht den generischen xtensa-esp-elf-gcc verwenden.
    // Der erzeugt hier ein Big-Endian-Objekt. ESP32-S3 braucht xtensa-esp32s3-elf-gcc.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let gcc = manifest_dir.join(".embuild/espressif/tools/xtensa-esp-elf/esp-13.2.0_20230928/xtensa-esp-elf/bin/xtensa-esp32s3-elf-gcc");

    if gcc.exists() {
        println!("cargo:warning=Using C compiler: {}", gcc.display());
        build.compiler(gcc);
    } else {
        println!(
            "cargo:warning=xtensa-esp32s3-elf-gcc not found at expected path, falling back to PATH"
        );
        build.compiler("xtensa-esp32s3-elf-gcc");
    }

    build
        .file("native/yd_hw.c")
        .include("native")
        .warnings(false)
        .flag("-mlongcalls")
        .define("ESP_PLATFORM", Some("1"))
        .define("IDF_TARGET_ESP32S3", Some("1"));

    if let Some(config) = find_esp_idf_build_config() {
        build.include(config);
        build.flag("-include");
        build.flag("sdkconfig.h");
    }

    // Nur gezielte ESP-IDF-Include-Pfade, nicht mehr alles rekursiv.
    add(&mut build, &idf, "components/newlib/platform_include");

    add(&mut build, &idf, "components/freertos/config/include");
    add(
        &mut build,
        &idf,
        "components/freertos/config/include/freertos",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/config/xtensa/include",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/config/xtensa/include/freertos",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel/include",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel/include/freertos",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel/portable/xtensa/include",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel/portable/xtensa/include/freertos",
    );

    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel-SMP/include",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel-SMP/include/freertos",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel-SMP/portable/xtensa/include",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/FreeRTOS-Kernel-SMP/portable/xtensa/include/freertos",
    );

    add(
        &mut build,
        &idf,
        "components/freertos/esp_additions/include",
    );
    add(
        &mut build,
        &idf,
        "components/freertos/esp_additions/include/freertos",
    );

    add(&mut build, &idf, "components/esp_common/include");
    add(&mut build, &idf, "components/esp_system/include");
    add(&mut build, &idf, "components/esp_system/port/include");
    add(&mut build, &idf, "components/esp_timer/include");
    add(&mut build, &idf, "components/esp_rom/include");
    add(&mut build, &idf, "components/esp_hw_support/include");
    add(&mut build, &idf, "components/heap/include");
    add(&mut build, &idf, "components/log/include");

    add(&mut build, &idf, "components/soc/include");
    add(&mut build, &idf, "components/soc/esp32s3/include");

    add(&mut build, &idf, "components/xtensa/include");
    add(&mut build, &idf, "components/xtensa/esp32s3/include");

    add(&mut build, &idf, "components/hal/include");
    add(&mut build, &idf, "components/hal/esp32s3/include");

    add(&mut build, &idf, "components/driver/include");
    add(&mut build, &idf, "components/driver/gpio/include");
    add(&mut build, &idf, "components/driver/ledc/include");
    add(&mut build, &idf, "components/driver/spi/include");
    add(&mut build, &idf, "components/driver/sdspi/include");
    add(&mut build, &idf, "components/driver/sdmmc/include");

    add(&mut build, &idf, "components/esp_adc/include");
    add(&mut build, &idf, "components/esp_adc/esp32s3/include");

    add(&mut build, &idf, "components/sdmmc/include");
    add(&mut build, &idf, "components/fatfs/src");
    add(&mut build, &idf, "components/fatfs/vfs");
    add(&mut build, &idf, "components/vfs/include");
    add(&mut build, &idf, "components/wear_levelling/include");
    add(&mut build, &idf, "components/esp_partition/include");
    add(&mut build, &idf, "components/spi_flash/include");

    build.compile("yd_hw");
}
