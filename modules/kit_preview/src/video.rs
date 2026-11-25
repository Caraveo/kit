//! Stream frame previews through a local media decoder.

use anyhow::{Context, Result};
use image::RgbaImage;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub struct Video {
    child: Child,
    frames: Receiver<RgbaImage>,
    audio_child: Option<Child>,
    audio: Option<Receiver<Vec<f32>>>,
}

impl Video {
    pub fn start(path: &Path, audio_rate: Option<u32>) -> Result<Self> {
        let (mut child, ffmpeg) = if let Some(executable) = bundled_ffmpeg() {
            (start_ffmpeg(&executable, path).context("Starting bundled FFmpeg")?, Some(executable))
        } else {
            match start_gstreamer(path) {
                Ok(child) => (child, None),
                Err(_) => {
                    let executable = PathBuf::from("ffmpeg");
                    let child = start_ffmpeg(&executable, path)
                        .context("Video previews require GStreamer or FFmpeg for unpackaged builds")?;
                    (child, Some(executable))
                }
            }
        };
        let mut audio_child = ffmpeg
            .as_deref()
            .zip(audio_rate)
            .and_then(|(executable, rate)| start_ffmpeg_audio(executable, path, rate).ok());
        let audio = audio_child.as_mut().and_then(|child| child.stdout.take()).map(read_audio);
        let stream = child.stdout.take().expect("configured stdout pipe");
        let (tx, frames) = mpsc::sync_channel(2);
        thread::spawn(move || {
            let mut image = Vec::new();
            let mut previous = 0;
            let mut reader = BufReader::new(stream);
            let mut chunk = [0; 8192];
            while let Ok(count) = reader.read(&mut chunk) {
                if count == 0 {
                    break;
                }
                for &byte in &chunk[..count] {
                    if image.is_empty() {
                        if previous == 0xff && byte == 0xd8 {
                            image.extend_from_slice(&[0xff, 0xd8]);
                        }
                    } else {
                        image.push(byte);
                        if previous == 0xff && byte == 0xd9 {
                            if let Ok(frame) = image::load_from_memory_with_format(&image, image::ImageFormat::Jpeg) {
                                if let Err(mpsc::TrySendError::Disconnected(_)) = tx.try_send(frame.into_rgba8()) {
                                    return;
                                }
                            }
                            image.clear();
                        } else if image.len() > 4 * 1024 * 1024 {
                            image.clear();
                        }
                    }
                    previous = byte;
                }
            }
        });
        Ok(Self { child, frames, audio_child, audio })
    }

    pub fn take_audio(&mut self) -> Option<Receiver<Vec<f32>>> {
        self.audio.take()
    }

    pub fn frame(&self) -> Option<RgbaImage> {
        self.frames.try_iter().last()
    }

    pub fn finished(&mut self) -> Result<Option<bool>> {
        let Some(status) = self.child.try_wait()? else { return Ok(None) };
        if status.success() {
            if let Some(audio) = &mut self.audio_child {
                if audio.try_wait()?.is_none() {
                    return Ok(None);
                }
            }
        }
        Ok(Some(status.success()))
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = self.child.kill();
        if let Some(audio) = &mut self.audio_child {
            let _ = audio.kill();
            let _ = audio.wait();
        }
        let _ = self.child.wait();
    }
}

fn bundled_ffmpeg() -> Option<PathBuf> {
    let editor = std::env::current_exe().ok()?;
    let directory = editor.parent()?;
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    #[cfg(target_os = "macos")]
    let adjacent = directory.join(name);
    #[cfg(not(target_os = "macos"))]
    let adjacent = directory.join("ffmpeg").join(name);
    if adjacent.is_file() {
        return Some(adjacent);
    }
    #[cfg(target_os = "linux")]
    if let Some(prefix) = directory.parent() {
        let installed = prefix.join("lib/kit/ffmpeg").join(name);
        if installed.is_file() {
            return Some(installed);
        }
    }
    None
}

fn start_ffmpeg(executable: &Path, path: &Path) -> std::io::Result<Child> {
    let mut ffmpeg = Command::new(executable);
    ffmpeg.args(["-nostdin", "-loglevel", "error", "-re", "-protocol_whitelist", "file,pipe", "-i"]).arg(path).args([
        "-map",
        "0:v:0",
        "-an",
        "-vf",
        "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2,fps=24",
        "-f",
        "image2pipe",
        "-c:v",
        "mjpeg",
        "pipe:1",
    ]);
    spawn(ffmpeg)
}

fn start_ffmpeg_audio(executable: &Path, path: &Path, rate: u32) -> std::io::Result<Child> {
    let mut ffmpeg = Command::new(executable);
    ffmpeg
        .args(["-nostdin", "-loglevel", "error", "-re", "-protocol_whitelist", "file,pipe", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-vn", "-ac", "2", "-ar"])
        .arg(rate.to_string())
        .args(["-c:a", "pcm_s16le", "-f", "s16le", "pipe:1"]);
    spawn(ffmpeg)
}

fn read_audio(mut input: impl Read + Send + 'static) -> Receiver<Vec<f32>> {
    let (sender, receiver) = mpsc::sync_channel(12);
    thread::spawn(move || {
        let mut buffer = [0; 4096];
        let mut pending = 0;
        while let Ok(read) = input.read(&mut buffer[pending..]) {
            if read == 0 {
                break;
            }
            let end = pending + read;
            let aligned = end / 4 * 4;
            if aligned > 0 {
                let samples = buffer[..aligned]
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
                    .collect();
                if matches!(sender.try_send(samples), Err(mpsc::TrySendError::Disconnected(_))) {
                    break;
                }
            }
            buffer.copy_within(aligned..end, 0);
            pending = end - aligned;
        }
    });
    receiver
}

fn start_gstreamer(path: &Path) -> Result<Child> {
    let file = path.to_str().context("Video path is not valid UTF-8")?;
    let pipeline = "videoconvert ! videoscale add-borders=true ! video/x-raw,width=640,height=360,pixel-aspect-ratio=1/1 ! jpegenc ! fdsink fd=1 sync=true";
    let mut gst = Command::new("gst-launch-1.0");
    gst.args(["-q", "playbin", &format!("uri={}", file_uri(file)), &format!("video-sink={pipeline}")]);
    Ok(spawn(gst)?)
}

fn spawn(mut command: Command) -> std::io::Result<Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
}

fn file_uri(path: &str) -> String {
    let mut uri = if path.starts_with('/') { "file://".to_owned() } else { "file:///".to_owned() };
    for byte in path.bytes() {
        match byte {
            b'\\' => uri.push('/'),
            b'/' | b':' | b'-' | b'_' | b'.' | b'~' => uri.push(byte as char),
            c if c.is_ascii_alphanumeric() => uri.push(c as char),
            byte => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_reader_preserves_interleaved_stereo_frames() {
        let pcm = [i16::MIN, i16::MAX, 0, 16384].into_iter().flat_map(i16::to_le_bytes).collect::<Vec<_>>();
        let audio = read_audio(std::io::Cursor::new(pcm));
        let samples = audio.recv().unwrap();
        assert_eq!(samples, vec![-1.0, 32767.0 / 32768.0, 0.0, 0.5]);
    }
}
