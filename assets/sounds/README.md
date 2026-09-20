# Save Scummer sounds

Six original, procedurally synthesized cues matching the Play sounds section of
`PLAN.md`. All production files are mono, 48 kHz, 16-bit PCM WAV. No recordings,
external samples, paid services or additional runtime packages are required.

| Event | Asset | Duration | Peak | Character |
| --- | --- | --- | --- | --- |
| Save started | [save-start.wav](save-start.wav) | 100 ms | -18 dBFS | Soft E5 to A5 |
| Save completed | [save-complete.wav](save-complete.wav) | 200 ms | -14 dBFS | Brighter B5 to E6 |
| Load started | [load-start.wav](load-start.wav) | 100 ms | -18 dBFS | Soft A5 to E5 |
| Load completed | [load-complete.wav](load-complete.wav) | 200 ms | -14 dBFS | Rounded D5 to A4 |
| Operation failed / could not start | [operation-failed.wav](operation-failed.wav) | 170 ms | -16 dBFS | Low double knock |
| Request rejected while busy | [busy.wav](busy.wav) | 28 ms | -26 dBFS | Quiet, dry tick |

## Listen

Open [preview.html](preview.html) in a browser to play each cue or the two sequences.
Nothing autoplays. The preview uses the production files at their original levels.

- [All six cues](preview/all-cues.wav): Save start, Save completion, Load start,
  Load completion, failure, busy. Cues start at 0.4, 1.6, 2.8, 4.0, 5.2 and 6.4 s.
- [Fast operations](preview/fast-operations.wav): Save success, Load success,
  Save failure, Load failure. Each pair leaves 50 ms after the start cue finishes.
  This gap demonstrates audible separation; file operations must remain independent
  of playback timing.

Listen with headphones and speakers, including over game audio, before finalizing
the mix. Start cues are softer than completion cues, and busy is intentionally
quieter. Do not independently normalize files during packaging or preview playback.

## Regenerate

With Node.js installed, run from the repository root:

```sh
node scripts/generate-sounds.mjs
```

The generator also works from another current directory. It uses only Node.js
built-ins and writes the six WAVs, both preview WAVs and `manifest.json` beside this
file. Seeded percussion is repeatable; the manifest records durations, levels and
SHA-256 hashes. Keep the generated production WAVs in version control. Synthesis
is an authoring step, not a build or runtime requirement.

## Playback integration

The Windows host embeds these exact WAVs through `crates/platform/src/sounds.rs`.
Its dedicated worker plays Save/Load start and committed completion cues, failure
knocks and rate-limited busy ticks independently of the UI. It inserts a 50 ms gap
between sounds without delaying file operations or extending operation locks.
Retries of already accepted requests do not replay cues. Recovery resolution never
plays a Save/Load completion sound. Playback errors do not fail file operations.

The app-wide Play sounds checkbox is enabled by default and persists through the
host's settings. The CLI can also change it with `savescummer sounds on` or
`savescummer sounds off`. Disabling it discards queued cues; an already playing
short cue can finish. `savescummer-host --no-audio` silences an isolated host run
without changing the saved preference. Native audio on other platforms is not
implemented yet.

No separate audio files are needed beside the host executable. The preview
directory and HTML are audition aids and are not embedded in the application.

## Provenance

All waveforms are produced from oscillators and seeded noise in
`scripts/generate-sounds.mjs`. No third-party audio is incorporated and no separate
sample attribution is needed. No additional license is imposed by this asset set;
distribution follows the project's chosen license.
