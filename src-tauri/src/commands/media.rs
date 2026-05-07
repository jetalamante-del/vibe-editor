use crate::models::VideoMetadata;
use base64::Engine;
use std::process::Command;
use std::fs;

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
