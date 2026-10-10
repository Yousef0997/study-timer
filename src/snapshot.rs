//! Developer tool (built only with `--features snapshot`): save the window to a PNG and quit.
//! Usage: STUDY_TIMER_SNAPSHOT=out.png STUDY_TIMER_TAB=today|schedule|progress|settings study-timer

use eframe::egui::{self, ColorImage, Event, ViewportCommand};
use std::io::Write;

pub fn requested_tab() -> Option<String> {
    std::env::var("STUDY_TIMER_SNAPSHOT").ok()?;
    Some(std::env::var("STUDY_TIMER_TAB").unwrap_or_else(|_| "today".into()))
}

pub fn step(ctx: &egui::Context, frame: u64) {
    let Ok(path) = std::env::var("STUDY_TIMER_SNAPSHOT") else { return };
    if frame == 20 {
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
    }
    let shot = ctx.input(|i| {
        i.raw.events.iter().find_map(|e| match e {
            Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(image) = shot {
        if let Err(e) = write_png(&path, &image) {
            eprintln!("snapshot failed: {e}");
        }
        std::process::exit(0);
    }
}

fn write_png(path: &str, image: &ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let mut raw = Vec::with_capacity((w * 4 + 1) * h);
    for row in image.pixels.chunks(w) {
        raw.push(0);
        for px in row {
            raw.extend_from_slice(&px.to_srgba_unmultiplied());
        }
    }
    // zlib stream made of uncompressed deflate blocks.
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(if i + 1 == chunks.len() { 1 } else { 0 });
        let len = c.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut out = std::fs::File::create(path)?;
    out.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])?;
    for (kind, data) in [(b"IHDR", &ihdr), (b"IDAT", &z), (b"IEND", &Vec::new())] {
        out.write_all(&(data.len() as u32).to_be_bytes())?;
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(data);
        out.write_all(&crc_input)?;
        out.write_all(&crc32(&crc_input).to_be_bytes())?;
    }
    Ok(())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
