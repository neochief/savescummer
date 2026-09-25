use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};

/// Plays a complete in-memory WAV to the end.
pub fn play(wav: &'static [u8]) {
    // SAFETY: with SND_MEMORY the "name" is a pointer to a complete in-memory
    // WAV image; it is 'static, so it outlives the synchronous call. A failure
    // (no audio device) is ignored: sound never fails an operation.
    unsafe {
        PlaySoundW(wav.as_ptr().cast(), std::ptr::null_mut(), SND_MEMORY | SND_SYNC | SND_NODEFAULT);
    }
}
