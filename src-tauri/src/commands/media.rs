use crate::models::VideoMetadata;
use base64::Engine;
use std::process::Command;
use std::fs;
use std::path::PathBuf;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tauri::Manager;
use tokio::process::Command as AsyncCommand;

/// Stable cache key for a source file path. Used to name proxy files so
/// re-importing the same source reuses the existing proxy.
fn hash_path(path: &str) -> String {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn proxy_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("app_cache_dir failed: {}", e))?;
    let dir = cache.join("proxies");
    fs::create_dir_all(&dir).map_err(|e| format!("create proxies dir failed: {}", e))?;
    Ok(dir)
}

#[tauri::command]
pub fn get_video_metadata(path: String) -> Result<VideoMetadata, String> {
    // Check if ffprobe exists
    let check = Command::new("which")
        .arg("ffprobe")
        .output();
    
    if check.is_err() || !check.unwrap().status.success() {
        return Err("ffprobe not found. Please install FFmpeg: brew install ffmpeg".to_string());
    }

    // First, check if this is an audio-only file
    let stream_check = Command::new("ffprobe")
        .args(&[
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=codec_type",
            "-of", "default=noprint_wrappers=1:nokey=1",
            &path,
        ])
        .output()
        .map_err(|e| format!("ffprobe stream check failed: {}", e))?;

    let has_video = !String::from_utf8_lossy(&stream_check.stdout).trim().is_empty();

    let output = if has_video {
        // Video file - get video stream info
        Command::new("ffprobe")
            .args(&[
                "-v", "error",
                "-select_streams", "v:0",
                "-show_entries", "stream=width,height,r_frame_rate,duration",
                "-of", "default=noprint_wrappers=1:nokey=1",
                &path,
            ])
            .output()
            .map_err(|e| format!("ffprobe execution failed: {}", e))?
    } else {
        // Audio file - get format duration instead
        Command::new("ffprobe")
            .args(&[
                "-v", "error",
                "-show_entries", "format=duration",
                "-of", "default=noprint_wrappers=1:nokey=1",
                &path,
            ])
            .output()
            .map_err(|e| format!("ffprobe execution failed: {}", e))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffprobe failed: {}", stderr));
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = output_str.trim().lines().collect();

    let (width, height, fps, duration) = if has_video {
        if lines.len() < 4 {
            return Err(format!("Invalid ffprobe output (got {} lines, expected 4): {}", lines.len(), output_str));
        }
        let width = lines[0].parse::<u32>().unwrap_or(1920);
        let height = lines[1].parse::<u32>().unwrap_or(1080);
        let fps_str = lines[2];
        let fps = if let Some(idx) = fps_str.find('/') {
            let num = fps_str[..idx].parse::<f64>().unwrap_or(30.0);
            let den = fps_str[idx + 1..].parse::<f64>().unwrap_or(1.0);
            num / den
        } else {
            fps_str.parse::<f64>().unwrap_or(30.0)
        };
        let duration = lines[3].parse::<f64>().unwrap_or(0.0);
        (width, height, fps, duration)
    } else {
        // Audio file - use default dimensions and get duration
        if lines.is_empty() {
            return Err(format!("Invalid ffprobe output for audio: {}", output_str));
        }
        let duration = lines[0].parse::<f64>().unwrap_or(0.0);
        (0, 0, 0.0, duration)
    };

    let metadata = fs::metadata(&path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(VideoMetadata {
        duration,
        width,
        height,
        fps,
        size: metadata,
    })
}

#[tauri::command]
pub fn extract_poster_frame(path: String, time: f64) -> Result<String, String> {
    // Check if ffmpeg exists
    let check = Command::new("which")
        .arg("ffmpeg")
        .output();
    
    if check.is_err() || !check.unwrap().status.success() {
        return Err("ffmpeg not found. Please install FFmpeg: brew install ffmpeg".to_string());
    }

    // Scale to 320px wide and encode as JPEG so the data URL stays small.
    // Without scaling, a 4K source produces a >10 MB base64 string that breaks
    // the project JSON, the React state, and the renderer.
    let output = Command::new("ffmpeg")
        .args(&[
            "-ss", &time.to_string(),
            "-i", &path,
            "-vframes", "1",
            "-vf", "scale=320:-2",
            "-q:v", "5",
            "-f", "image2",
            "-vcodec", "mjpeg",
            "pipe:1",
        ])
        .output()
        .map_err(|e| format!("ffmpeg execution failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg failed: {}", stderr));
    }

    let encoded = base64::engine::general_purpose::STANDARD.encode(&output.stdout);
    Ok(format!("data:image/jpeg;base64,{}", encoded))
}

/// Ensure an audio file is in a format WebKit's <audio> element actually
/// renders sound for. WebKit only reliably plays 16-bit PCM WAV at sane sample
/// rates — 24-bit/32-bit/float WAVs load (readyState=4) but produce no output.
/// If the source is already 16-bit ≤48kHz mono/stereo, returns the source path
/// unchanged. Otherwise transcodes to a cached 16-bit 48kHz stereo proxy and
/// returns the proxy path.
#[tauri::command]
pub async fn prepare_audio_proxy(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let probe = AsyncCommand::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "a:0",
            "-show_entries", "stream=sample_fmt,sample_rate,channels,codec_name",
            "-of", "default=noprint_wrappers=1:nokey=1",
            &path,
        ])
        .output()
        .await
        .map_err(|e| format!("ffprobe failed: {}", e))?;
    if !probe.status.success() {
        return Err(format!("ffprobe error: {}", String::from_utf8_lossy(&probe.stderr)));
    }
    let info = String::from_utf8_lossy(&probe.stdout);
    let mut lines = info.trim().lines();
    let codec = lines.next().unwrap_or("").to_string();
    let sample_fmt = lines.next().unwrap_or("").to_string();
    let sample_rate: u32 = lines.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let channels: u32 = lines.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    // WebKit-safe combo: 16-bit PCM, ≤48kHz, mono/stereo
    let already_safe =
        codec == "pcm_s16le" && sample_fmt == "s16" && sample_rate <= 48000 && (1..=2).contains(&channels);
    if already_safe {
        return Ok(path);
    }

    let dir = proxy_dir(&app)?;
    let proxy = dir.join(format!("{}_audio.wav", hash_path(&path)));
    if proxy.exists() {
        return Ok(proxy.to_string_lossy().into_owned());
    }
    let proxy_str = proxy
        .to_str()
        .ok_or_else(|| "proxy path not utf-8".to_string())?;

    let output = AsyncCommand::new("ffmpeg")
        .args([
            "-y", "-hide_banner", "-loglevel", "error",
            "-i", &path,
            "-acodec", "pcm_s16le",
            "-ar", "48000",
            "-ac", "2",
            proxy_str,
        ])
        .output()
        .await
        .map_err(|e| format!("ffmpeg failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "audio proxy transcode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(proxy.to_string_lossy().into_owned())
}

/// Generate a 1080p H.264 proxy for video preview when the source is heavier
/// than WebKit's <video> can decode in real-time (4K, HEVC, high bitrate, etc.).
/// Uses VideoToolbox hardware acceleration on macOS so encoding is fast.
/// If the source is already H.264 ≤1080p with AAC audio, returns the source
/// path unchanged. Otherwise returns a cached proxy path.
#[tauri::command]
pub async fn prepare_video_proxy(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let probe = AsyncCommand::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=codec_name,width,height",
            "-of", "default=noprint_wrappers=1:nokey=1",
            &path,
        ])
        .output()
        .await
        .map_err(|e| format!("ffprobe failed: {}", e))?;
    if !probe.status.success() {
        return Err(format!("ffprobe error: {}", String::from_utf8_lossy(&probe.stderr)));
    }
    let info = String::from_utf8_lossy(&probe.stdout);
    let mut lines = info.trim().lines();
    let codec = lines.next().unwrap_or("").to_string();
    let width: u32 = lines.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let height: u32 = lines.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    let already_safe = codec == "h264" && width <= 1920 && height <= 1080;
    if already_safe {
        return Ok(path);
    }

    let dir = proxy_dir(&app)?;
    let proxy = dir.join(format!("{}_video.mp4", hash_path(&path)));
    if proxy.exists() {
        return Ok(proxy.to_string_lossy().into_owned());
    }
    let proxy_str = proxy
        .to_str()
        .ok_or_else(|| "proxy path not utf-8".to_string())?;

    let scale_filter = "scale='if(gt(iw/ih,16/9),min(1920,iw),-2)':'if(gt(iw/ih,16/9),-2,min(1080,ih))'";
    let output = AsyncCommand::new("ffmpeg")
        .args([
            "-y", "-hide_banner", "-loglevel", "error",
            "-i", &path,
            "-vf", scale_filter,
            "-c:v", "h264_videotoolbox",
            "-b:v", "5M",
            "-c:a", "aac",
            "-b:a", "192k",
            "-movflags", "+faststart",
            proxy_str,
        ])
        .output()
        .await
        .map_err(|e| format!("ffmpeg failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "video proxy transcode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(proxy.to_string_lossy().into_owned())
}
