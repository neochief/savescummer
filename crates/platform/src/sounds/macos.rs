//! macOS: `NSSound` from the WAV in memory, played to its end on the sound
//! worker. No temporary files, no `afplay`. No audio device, or any
//! failure, is silence.

use std::time::{Duration, Instant};

use objc2::AnyThread;
use objc2_app_kit::NSSound;
use objc2_foundation::NSData;

pub fn play(wav: &'static [u8]) {
    let data = NSData::with_bytes(wav);
    let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) else { return };
    if !sound.play() {
        return;
    }
    // Waits for the end, so the next cue never cuts this one off. Off the
    // main thread `isPlaying` may stay set past the end: the sound's length
    // plus the output's latency bounds the wait.
    let deadline = Instant::now() + Duration::from_secs_f64(sound.duration().max(0.0)) + Duration::from_millis(100);
    while sound.isPlaying() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    sound.stop();
}
