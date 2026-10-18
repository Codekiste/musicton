//! Video track support through the system's ffmpeg/ffprobe: probing, frame extraction for the
//! picture window and timeline thumbnails, audio extraction and muxing the final mix with video.

use std::path::Path;
use std::process::Command;

pub const FRAME_FPS: f32 = 8.0;
pub const FRAME_WIDTH: u32 = 320;

pub fn available() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok_and(|o| o.status.success())
}

/// Duration in seconds and whether the file has an audio stream.
pub fn probe(path: &str) -> Result<(f32, bool), String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type", "-of", "default=noprint_wrappers=1", path])
        .output()
        .map_err(|e| format!("ffprobe not found: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let duration = text.lines().find_map(|l| l.strip_prefix("duration=")).and_then(|d| d.trim().parse::<f32>().ok()).ok_or("no duration")?;
    let has_audio = text.lines().any(|l| l.trim() == "codec_type=audio");
    Ok((duration, has_audio))
}

/// Extracts small PNG frames (FRAME_FPS, FRAME_WIDTH wide) into `dir`; returns the frame count.
pub fn extract_frames(path: &str, dir: &Path) -> Result<u32, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let pattern = dir.join("%05d.png");
    let out = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i", path, "-vf", &format!("fps={FRAME_FPS},scale={FRAME_WIDTH}:-2"), "-compression_level", "1"])
        .arg(&pattern)
        .output()
        .map_err(|e| format!("ffmpeg not found: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(std::fs::read_dir(dir).map(|rd| rd.filter(|e| e.as_ref().is_ok_and(|e| e.path().extension().is_some_and(|x| x == "png"))).count() as u32).unwrap_or(0))
}

pub fn extract_audio(path: &str, wav: &Path) -> Result<(), String> {
    let out = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i", path, "-vn", "-ac", "2", "-ar", "48000", "-c:a", "pcm_s16le"])
        .arg(wav)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// Puts the rendered mix under the video. The video starts `offset` seconds into the mix.
pub fn mux(video: &str, audio_wav: &str, offset: f32, out_path: &str) -> Result<(), String> {
    let run = |vcodec: &[&str]| {
        Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i", audio_wav, "-itsoffset", &format!("{offset:.3}"), "-i", video, "-map", "1:v:0", "-map", "0:a:0"])
            .args(vcodec)
            .args(["-c:a", "aac", "-b:a", "256k", "-shortest", out_path])
            .output()
    };
    let out = run(&["-c:v", "copy"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    // The container may not take the original codec: re-encode.
    let out = run(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p"]).map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// Decodes a PNG frame into RGBA.
pub fn load_frame(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(file);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::Grayscale => buf[..w * h].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..w * h * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        _ => return None,
    };
    Some((w, h, rgba))
}
