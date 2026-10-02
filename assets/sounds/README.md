# SaveScummer sounds

Six original Wood-style host cues. All production files are mono, 48 kHz,
16-bit PCM WAV. No recordings, external samples, paid services, or additional
runtime packages are required.

| Event | Asset | Duration | Peak | Character |
| --- | --- | --- | --- | --- |
| Save started | [save-start.wav](save-start.wav) | 150 ms | -20 dBFS | Rising two-note pickup |
| Save completed | [save-complete.wav](save-complete.wav) | 160 ms | -17 dBFS | Higher landing |
| Load started | [load-start.wav](load-start.wav) | 150 ms | -20 dBFS | Falling two-note pickup |
| Load completed | [load-complete.wav](load-complete.wav) | 160 ms | -17 dBFS | Lower landing |
| Operation failed / could not start | [operation-failed.wav](operation-failed.wav) | 222 ms | -18 dBFS | Textured descent |
| Request rejected while busy | [busy.wav](busy.wav) | 44 ms | -29 dBFS | Quiet, dry tick |

## Regenerate

With Node.js installed, run from `apps/ui`:

```sh
node scripts/generate-interface-sounds.mjs
```

The generator also works from another current directory. It uses only Node.js
built-ins. It writes all 16 Wood cues to `apps/ui/public/sounds`
and the six Wood operation cues here, then rebuilds `manifest.json`. Seeded
percussion is repeatable; the manifest records durations, levels, and SHA-256
hashes. Synthesis is an authoring step, not a build or runtime requirement.

## Playback integration

The host embeds these exact WAVs through `crates/platform/src/sounds/mod.rs`.
Its dedicated worker plays Save/Load start and committed completion cues for
actions from the UI, CLI, or hotkeys, plus failure knocks and rate-limited busy
ticks. It inserts a short gap between sounds without delaying file operations
or extending operation locks.
Retries of already accepted requests do not replay cues. Recovery resolution never
plays a Save/Load completion sound. Playback errors do not fail file operations.

The app-wide Play sounds checkbox is enabled by default and persists through the
host's settings. The CLI can also change it with `SaveScummer.CLI sounds on` or
`SaveScummer.CLI sounds off`. Disabling it discards queued cues; an already playing
short cue can finish. `SaveScummer --no-audio` silences an isolated host run
without changing the saved preference.

No separate audio files are needed beside the host executable.

## Provenance

All waveforms are produced from oscillators and seeded noise in
`apps/ui/scripts/generate-interface-sounds.mjs`. No third-party audio is incorporated and no separate
sample attribution is needed. No additional license is imposed by this asset set;
distribution follows the project's chosen license.
