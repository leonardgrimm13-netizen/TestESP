use crate::config;
use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Seek, SeekFrom, Write};

pub const SAMPLE_RATE_HZ: u32 = config::RECORD_SAMPLE_RATE;
pub const CHANNELS: u16 = 1;
pub const RECORD_BITS_PER_SAMPLE: u16 = config::RECORD_BITS_PER_SAMPLE;

#[derive(Debug, Clone, Copy)]
pub struct WavInfo {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub block_align: u16,
    pub data_start: u64,
    pub data_len: u32,
}

impl WavInfo {
    pub fn bytes_per_sample(self) -> Result<usize> {
        match self.bits_per_sample {
            8 => Ok(1),
            16 => Ok(2),
            other => bail!("nicht unterstuetzte Bit-Tiefe: {other}"),
        }
    }

    pub fn bytes_per_frame(self) -> Result<usize> {
        Ok(self.bytes_per_sample()? * self.channels as usize)
    }

    pub fn frame_count(self) -> Result<u32> {
        let bytes_per_frame = self.bytes_per_frame()? as u32;
        Ok(if bytes_per_frame == 0 {
            0
        } else {
            self.data_len / bytes_per_frame
        })
    }
}

pub fn write_placeholder_header<W: Write>(mut writer: W) -> Result<()> {
    write_header(&mut writer, 0)
}

pub fn finalize_header<W: Write + Seek>(writer: &mut W, data_len: u32) -> Result<()> {
    writer.seek(SeekFrom::Start(0))?;
    write_header(writer, data_len)?;
    writer.seek(SeekFrom::End(0))?;
    Ok(())
}

fn write_header<W: Write>(writer: &mut W, data_len: u32) -> Result<()> {
    let byte_rate = SAMPLE_RATE_HZ * CHANNELS as u32 * RECORD_BITS_PER_SAMPLE as u32 / 8;
    let block_align = CHANNELS * RECORD_BITS_PER_SAMPLE / 8;
    let riff_len = 36_u32
        .checked_add(data_len)
        .ok_or_else(|| anyhow!("WAV-Datei zu gross"))?;

    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_len.to_le_bytes())?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    writer.write_all(&16_u32.to_le_bytes())?;
    writer.write_all(&1_u16.to_le_bytes())?;
    writer.write_all(&CHANNELS.to_le_bytes())?;
    writer.write_all(&SAMPLE_RATE_HZ.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&RECORD_BITS_PER_SAMPLE.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_len.to_le_bytes())?;
    Ok(())
}

pub fn read_info<R: Read + Seek>(reader: &mut R) -> Result<WavInfo> {
    let mut riff = [0_u8; 12];
    reader
        .read_exact(&mut riff)
        .context("WAV-RIFF-Header fehlt")?;

    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        bail!("keine RIFF/WAVE-Datei");
    }

    let mut fmt_seen = false;
    let mut sample_rate_hz = 0_u32;
    let mut channels = 0_u16;
    let mut bits_per_sample = 0_u16;
    let mut block_align = 0_u16;

    loop {
        let mut chunk_header = [0_u8; 8];
        reader
            .read_exact(&mut chunk_header)
            .context("WAV-Datei endet vor dem data-Chunk")?;

        let chunk_id = &chunk_header[0..4];
        let chunk_len = u32::from_le_bytes(chunk_header[4..8].try_into().unwrap());
        let chunk_start = reader.stream_position()?;

        match chunk_id {
            b"fmt " => {
                let mut fmt = vec![0_u8; chunk_len as usize];
                reader.read_exact(&mut fmt)?;
                if fmt.len() < 16 {
                    bail!("WAV fmt-Chunk ist zu kurz");
                }

                let audio_format = u16::from_le_bytes(fmt[0..2].try_into().unwrap());
                channels = u16::from_le_bytes(fmt[2..4].try_into().unwrap());
                sample_rate_hz = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
                block_align = u16::from_le_bytes(fmt[12..14].try_into().unwrap());
                bits_per_sample = u16::from_le_bytes(fmt[14..16].try_into().unwrap());

                if audio_format != 1 {
                    bail!("nur PCM-WAV wird unterstuetzt, Format={audio_format}");
                }

                fmt_seen = true;
            }
            b"data" => {
                if !fmt_seen {
                    bail!("data-Chunk kam vor fmt-Chunk");
                }

                if channels != 1 && channels != 2 {
                    bail!("nur Mono- oder Stereo-WAV wird unterstuetzt, channels={channels}");
                }

                if bits_per_sample != 8 && bits_per_sample != 16 {
                    bail!("nur 8-bit unsigned oder 16-bit signed PCM wird unterstuetzt");
                }

                return Ok(WavInfo {
                    sample_rate_hz,
                    channels,
                    bits_per_sample,
                    block_align,
                    data_start: chunk_start,
                    data_len: chunk_len,
                });
            }
            _ => {
                reader.seek(SeekFrom::Start(chunk_start + chunk_len as u64))?;
            }
        }

        if chunk_len % 2 != 0 {
            reader.seek(SeekFrom::Current(1))?;
        }
    }
}
