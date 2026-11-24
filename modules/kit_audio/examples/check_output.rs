//! `cargo run -p kit_audio --example check_output [sound-file]`: open the audio
//! device and play a sound file (or a moment of silence) to check sound works.

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let mixer = std::sync::Arc::new(std::sync::Mutex::new(kit_audio::Mixer::new(48_000)));
    let out = kit_audio::device::open(mixer.clone())?;
    println!("audio output open: {} Hz", out.rate);
    let seconds = match std::env::args().nth(1) {
        Some(path) => {
            let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
            let clip = std::sync::Arc::new(kit_audio::decode(std::fs::read(&path)?, &ext)?);
            println!("playing {path}: {:.2} s at {} Hz", clip.duration(), clip.rate);
            let d = clip.duration();
            mixer.lock().unwrap().play(1, clip, 1.0, 0.0, 1.0, false);
            d
        }
        None => 0.3,
    };
    std::thread::sleep(std::time::Duration::from_secs_f32(seconds + 0.2));
    println!("done");
    Ok(())
}
