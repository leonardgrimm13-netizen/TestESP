use crate::hw;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const MOUNT_POINT: &str = "/sdcard";

pub struct SdCard;

impl SdCard {
    pub fn mount() -> Result<Self> {
        hw::sd_mount()?;
        Ok(Self)
    }

    pub fn first_wav(&self) -> Result<Option<PathBuf>> {
        let mut files = self.wav_files()?;
        files.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_uppercase())
                .unwrap_or_default()
        });
        Ok(files.into_iter().next())
    }

    pub fn next_recording_path(&self) -> Result<PathBuf> {
        for index in 1..=9999 {
            let path = Path::new(MOUNT_POINT).join(format!("REC_{index:04}.WAV"));
            if !path.exists() {
                return Ok(path);
            }
        }

        bail!("REC_0001.WAV bis REC_9999.WAV sind bereits vorhanden")
    }

    fn wav_files(&self) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();

        for entry in
            fs::read_dir(MOUNT_POINT).context("SD-Wurzelverzeichnis konnte nicht gelesen werden")?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_wav_path(&path) {
                out.push(path);
            }
        }

        Ok(out)
    }
}

impl Drop for SdCard {
    fn drop(&mut self) {
        hw::sd_unmount();
    }
}

fn is_wav_path(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
}
