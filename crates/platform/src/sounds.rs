//! Host-owned, asynchronous feedback. File operations never wait for playback.
use savescummer_core::{Action, ErrorCode, OperationStatus};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    SaveStart,
    SaveComplete,
    LoadStart,
    LoadComplete,
    Failed,
    Busy,
}
impl Cue {
    pub fn wav(self) -> &'static [u8] {
        match self {
            Self::SaveStart => include_bytes!("../../../assets/sounds/save-start.wav"),
            Self::SaveComplete => include_bytes!("../../../assets/sounds/save-complete.wav"),
            Self::LoadStart => include_bytes!("../../../assets/sounds/load-start.wav"),
            Self::LoadComplete => include_bytes!("../../../assets/sounds/load-complete.wav"),
            Self::Failed => include_bytes!("../../../assets/sounds/operation-failed.wav"),
            Self::Busy => include_bytes!("../../../assets/sounds/busy.wav"),
        }
    }
}

/// Synchronous playback is confined to the audio worker. Implementations return
/// after playback finishes and may fail without affecting the file operation.
pub trait SoundPlayer: Send + Sync + 'static {
    fn play(&self, cue: Cue) -> io::Result<()>;
}
pub struct NativeSoundPlayer;
impl SoundPlayer for NativeSoundPlayer {
    fn play(&self, cue: Cue) -> io::Result<()> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Media::Audio::{
                PlaySoundA, SND_MEMORY, SND_NODEFAULT, SND_SYNC,
            };
            // SND_MEMORY treats this pointer as a complete WAV image, not text.
            // Embedded bytes live for the process lifetime. No default OS beep,
            // loop, file lookup, or call from the core/operation thread is used.
            let played = unsafe {
                PlaySoundA(
                    cue.wav().as_ptr(),
                    std::ptr::null_mut(),
                    SND_MEMORY | SND_NODEFAULT | SND_SYNC,
                )
            };
            if played == 0 {
                return Err(io::Error::other("Windows could not play the sound cue"));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = cue;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "native sound playback is currently implemented for Windows",
            ))
        }
    }
}

pub struct Sounds {
    sender: mpsc::Sender<(u64, Cue)>,
    // Odd = enabled; each change invalidates previously queued cues, including
    // those queued before a mute/unmute cycle.
    epoch: Arc<AtomicU64>,
    last_busy: Mutex<Option<Instant>>,
}
impl Sounds {
    pub fn new(enabled: bool, player: Arc<dyn SoundPlayer>) -> Self {
        let (sender, receiver) = mpsc::channel::<(u64, Cue)>();
        let epoch = Arc::new(AtomicU64::new(u64::from(enabled)));
        let worker_epoch = epoch.clone();
        let worker = std::thread::Builder::new()
            .name("sound-feedback".into())
            .spawn(move || {
                let mut finished: Option<Instant> = None;
                let mut reported_error = false;
                while let Ok((generation, cue)) = receiver.recv() {
                    // Keep a small gap even when the operation finishes before
                    // the start cue. This wait belongs only to the audio thread.
                    if let Some(at) = finished {
                        std::thread::sleep(Duration::from_millis(50).saturating_sub(at.elapsed()));
                    }
                    if worker_epoch.load(Ordering::SeqCst) != generation {
                        continue;
                    }
                    if let Err(error) = player.play(cue)
                        && !reported_error
                    {
                        eprintln!("sound feedback: {error}");
                        reported_error = true;
                    }
                    finished = Some(Instant::now());
                }
            });
        if let Err(error) = worker {
            // Audio is best effort; failure to create its worker must not make
            // the host or save/restore functionality unavailable.
            eprintln!("sound feedback worker: {error}");
        }
        Self {
            sender,
            epoch,
            last_busy: Mutex::new(None),
        }
    }
    pub fn set_enabled(&self, enabled: bool) {
        let _ = self
            .epoch
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                ((old % 2 == 1) != enabled).then_some(old.wrapping_add(1))
            });
    }
    fn enqueue(&self, cue: Cue) {
        let generation = self.epoch.load(Ordering::SeqCst);
        if generation % 2 == 1 {
            let _ = self.sender.send((generation, cue));
        }
    }
    pub fn started(&self, action: &Action) {
        match action {
            Action::Save => self.enqueue(Cue::SaveStart),
            Action::Load { .. } => self.enqueue(Cue::LoadStart),
            _ => (),
        }
    }
    pub fn finished(&self, action: &Action, status: OperationStatus) {
        match (action, status) {
            (Action::Save, OperationStatus::Completed) => self.enqueue(Cue::SaveComplete),
            (Action::Load { .. }, OperationStatus::Completed) => self.enqueue(Cue::LoadComplete),
            (_, OperationStatus::Failed | OperationStatus::RecoveryNeeded) => {
                self.enqueue(Cue::Failed)
            }
            _ => (),
        }
    }
    pub fn rejected(&self, code: ErrorCode) {
        if code == ErrorCode::Busy {
            if self.epoch.load(Ordering::SeqCst).is_multiple_of(2) {
                return;
            }
            let Ok(mut last) = self.last_busy.lock() else {
                return;
            };
            let now = Instant::now();
            if last.is_none_or(|at| now.duration_since(at) >= Duration::from_secs(1)) {
                *last = Some(now);
                self.enqueue(Cue::Busy);
            }
        } else {
            self.enqueue(Cue::Failed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingPlayer(mpsc::Sender<(Cue, Instant)>);
    impl SoundPlayer for RecordingPlayer {
        fn play(&self, cue: Cue) -> io::Result<()> {
            self.0.send((cue, Instant::now())).unwrap();
            Ok(())
        }
    }
    fn recorder() -> (Sounds, mpsc::Receiver<(Cue, Instant)>) {
        let (sender, receiver) = mpsc::channel();
        (
            Sounds::new(true, Arc::new(RecordingPlayer(sender))),
            receiver,
        )
    }
    fn next(receiver: &mpsc::Receiver<(Cue, Instant)>) -> (Cue, Instant) {
        receiver.recv_timeout(Duration::from_secs(5)).unwrap()
    }

    #[test]
    fn fast_operations_preserve_order_gap_and_failure_semantics() {
        let (sounds, receiver) = recorder();
        sounds.started(&Action::Save);
        sounds.finished(&Action::Save, OperationStatus::Completed);
        let load = Action::Load { target: None };
        sounds.started(&load);
        sounds.finished(&load, OperationStatus::Failed);
        // Recovery resolution must never replay the original success sound.
        sounds.finished(&load, OperationStatus::Resolved);
        drop(sounds);
        let events = receiver.iter().collect::<Vec<_>>();
        assert_eq!(
            events.iter().map(|e| e.0).collect::<Vec<_>>(),
            [
                Cue::SaveStart,
                Cue::SaveComplete,
                Cue::LoadStart,
                Cue::Failed,
            ]
        );
        for pair in events.windows(2) {
            assert!(pair[1].1.duration_since(pair[0].1) >= Duration::from_millis(50));
        }
    }

    #[test]
    fn busy_is_rate_limited_and_rejections_have_no_start() {
        let (sounds, receiver) = recorder();
        for _ in 0..50 {
            sounds.rejected(ErrorCode::Busy);
        }
        sounds.rejected(ErrorCode::Unavailable);
        drop(sounds);
        assert_eq!(
            receiver.iter().map(|e| e.0).collect::<Vec<_>>(),
            [Cue::Busy, Cue::Failed]
        );
    }

    struct BlockingPlayer {
        started: mpsc::Sender<Cue>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl SoundPlayer for BlockingPlayer {
        fn play(&self, cue: Cue) -> io::Result<()> {
            self.started.send(cue).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(())
        }
    }
    #[test]
    fn mute_discards_queued_audio_even_after_reenabling_and_never_waits_for_player() {
        let (started, events) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let sounds = Sounds::new(
            true,
            Arc::new(BlockingPlayer {
                started,
                release: Mutex::new(gate),
            }),
        );
        sounds.started(&Action::Save);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Cue::SaveStart
        );
        // All these calls complete while the player is blocked.
        sounds.finished(&Action::Save, OperationStatus::Completed);
        sounds.set_enabled(false);
        sounds.started(&Action::Save);
        sounds.rejected(ErrorCode::Busy);
        sounds.set_enabled(true);
        sounds.started(&Action::Load { target: None });
        release.send(()).unwrap();
        assert_eq!(
            events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Cue::LoadStart
        );
        release.send(()).unwrap();
        drop(sounds);
        assert!(events.recv_timeout(Duration::from_secs(5)).is_err());
    }

    #[test]
    fn failed_playback_does_not_stop_subsequent_cues() {
        struct FailingPlayer(mpsc::Sender<Cue>);
        impl SoundPlayer for FailingPlayer {
            fn play(&self, cue: Cue) -> io::Result<()> {
                self.0.send(cue).unwrap();
                Err(io::Error::other("test device unavailable"))
            }
        }
        let (sender, receiver) = mpsc::channel();
        let sounds = Sounds::new(true, Arc::new(FailingPlayer(sender)));
        sounds.started(&Action::Save);
        sounds.finished(&Action::Save, OperationStatus::Completed);
        drop(sounds);
        assert_eq!(
            receiver.iter().collect::<Vec<_>>(),
            [Cue::SaveStart, Cue::SaveComplete]
        );
    }

    #[test]
    fn load_success_uses_the_approved_embedded_assets() {
        let (sounds, receiver) = recorder();
        let load = Action::Load { target: None };
        sounds.started(&load);
        sounds.finished(&load, OperationStatus::Completed);
        assert_eq!(next(&receiver).0, Cue::LoadStart);
        assert_eq!(next(&receiver).0, Cue::LoadComplete);
        for cue in [
            Cue::SaveStart,
            Cue::SaveComplete,
            Cue::LoadStart,
            Cue::LoadComplete,
            Cue::Failed,
            Cue::Busy,
        ] {
            assert_eq!(&cue.wav()[..4], b"RIFF");
            assert_eq!(&cue.wav()[8..12], b"WAVE");
        }
    }
}
