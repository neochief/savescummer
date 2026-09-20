#!/usr/bin/env node
// Original procedural sound design; Node.js built-ins only.
// Run from any directory: node /path/to/savescummer/scripts/generate-sounds.mjs
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const output = resolve(dirname(fileURLToPath(import.meta.url)), '../assets/sounds');
const sampleRate = 48000;
const tau = 2 * Math.PI;
const samples = ms => Math.round(ms * sampleRate / 1000);
const silence = ms => new Float64Array(samples(ms));
const smooth = x => 0.5 - 0.5 * Math.cos(Math.PI * Math.max(0, Math.min(1, x)));

// Every voice starts and ends at zero. Partial-specific decay makes a small,
// rounded mallet sound without a long reverberant tail or a hard waveform cut.
function tone(frequency, durationMs, brightness = 0.15) {
  const data = silence(durationMs);
  for (let i = 0; i < data.length; i++) {
    const t = i / sampleRate;
    const attack = smooth(i / samples(3));
    const release = smooth((data.length - 1 - i) / samples(12));
    const decay = Math.exp(-t / (durationMs / 1000 * 0.42));
    data[i] = attack * release * decay * (
      Math.sin(tau * frequency * t)
      + brightness * Math.exp(-t / 0.025) * Math.sin(tau * frequency * 2 * t)
      + brightness * 0.32 * Math.exp(-t / 0.014) * Math.sin(tau * frequency * 3 * t)
    );
  }
  return data;
}

function mix(target, source, atMs, gain = 1) {
  const start = samples(atMs);
  if (start < 0 || start + source.length > target.length) {
    throw new Error('Voice exceeds its cue; adjust timing instead of truncating it.');
  }
  for (let i = 0; i < source.length; i++) target[start + i] += source[i] * gain;
}

function melody(durationMs, notes, brightness) {
  const data = silence(durationMs);
  for (const [atMs, frequency, lengthMs, gain = 1] of notes) {
    mix(data, tone(frequency, lengthMs, brightness), atMs, gain);
  }
  return data;
}

// Seeded noise keeps the percussion repeatable without using recorded samples.
function noiseGenerator(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(1664525, state) + 1013904223) >>> 0;
    return state / 2147483648 - 1;
  };
}

function knock(frequency, durationMs, seed) {
  const data = silence(durationMs);
  const random = noiseGenerator(seed);
  let filteredNoise = 0;
  for (let i = 0; i < data.length; i++) {
    const t = i / sampleRate;
    filteredNoise += 0.14 * (random() - filteredNoise);
    const envelope = smooth(i / samples(1.5))
      * smooth((data.length - 1 - i) / samples(10));
    data[i] = envelope * (
      Math.sin(tau * frequency * t) * Math.exp(-t / 0.014)
      + 0.38 * Math.sin(tau * frequency * 2.31 * t) * Math.exp(-t / 0.007)
      + 0.18 * filteredNoise * Math.exp(-t / 0.004)
    );
  }
  return data;
}

function busyTick() {
  const data = silence(28);
  const random = noiseGenerator(0x53415645);
  let filteredNoise = 0;
  for (let i = 0; i < data.length; i++) {
    const t = i / sampleRate;
    filteredNoise += 0.23 * (random() - filteredNoise);
    const envelope = smooth(i / samples(0.8))
      * smooth((data.length - 1 - i) / samples(6));
    data[i] = envelope * (
      0.65 * filteredNoise * Math.exp(-t / 0.003)
      + 0.35 * Math.sin(tau * 1450 * t) * Math.exp(-t / 0.0025)
    );
  }
  return data;
}

// Remove DC with a smooth correction, leaving the zero-valued edges intact,
// then set deliberately different peaks for start, result and busy cues.
function master(data, peakDbfs) {
  const window = Array.from(data, (_, i) => Math.sin(Math.PI * i / (data.length - 1)) ** 2);
  const offset = data.reduce((sum, value) => sum + value, 0)
    / window.reduce((sum, value) => sum + value, 0);
  const centered = data.map((value, i) => value - offset * window[i]);
  const peak = centered.reduce((max, value) => Math.max(max, Math.abs(value)), 0);
  const gain = 10 ** (peakDbfs / 20) / peak;
  return centered.map(value => value * gain);
}

function encodeWave(data) {
  const wav = Buffer.alloc(44 + data.length * 2);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write('WAVEfmt ', 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20); // PCM
  wav.writeUInt16LE(1, 22); // mono
  wav.writeUInt32LE(sampleRate, 24);
  wav.writeUInt32LE(sampleRate * 2, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(data.length * 2, 40);
  for (let i = 0; i < data.length; i++) {
    if (!Number.isFinite(data[i]) || Math.abs(data[i]) >= 1) {
      throw new Error(`Invalid or clipped sample at ${i}`);
    }
    wav.writeInt16LE(Math.round(data[i] * 32767), 44 + i * 2);
  }
  return wav;
}

const failure = silence(170);
mix(failure, knock(190, 60, 0x4641494c), 0);
mix(failure, knock(165, 70, 0x4b4e4f43), 100, 0.9);

const cues = [
  {
    id: 'save-start', label: 'Save started', peak: -18,
    description: 'Two soft ascending notes, E5 to A5.',
    data: melody(100, [[0, 659.255, 43], [47, 880, 53]], 0.1),
  },
  {
    id: 'save-complete', label: 'Save completed', peak: -14,
    description: 'A brighter ascending resolution, B5 to E6.',
    data: melody(200, [[0, 987.767, 80, 0.82], [60, 1318.51, 140]], 0.23),
  },
  {
    id: 'load-start', label: 'Load started', peak: -18,
    description: 'Two soft descending notes, A5 to E5.',
    data: melody(100, [[0, 880, 43], [47, 659.255, 53]], 0.1),
  },
  {
    id: 'load-complete', label: 'Load completed', peak: -14,
    description: 'A rounded descending resolution, D5 to A4.',
    data: melody(200, [[0, 587.33, 80, 0.82], [60, 440, 140]], 0.08),
  },
  {
    id: 'operation-failed', label: 'Operation failed', peak: -16,
    description: 'Two low, muted knocks; the second is slightly lower.',
    data: failure,
  },
  {
    id: 'busy', label: 'Busy', peak: -26,
    description: 'One quiet, dry tick.',
    data: busyTick(),
  },
];

mkdirSync(join(output, 'preview'), { recursive: true });
const manifest = {
  format: { container: 'wav', encoding: 'PCM', sample_rate_hz: sampleRate, bits_per_sample: 16, channels: 1 },
  source: 'Original procedural synthesis; no recordings, third-party samples or generative-audio services.',
  generator: 'scripts/generate-sounds.mjs',
  cues: [],
};

for (const cue of cues) {
  cue.data = master(cue.data, cue.peak);
  const wav = encodeWave(cue.data);
  const file = `${cue.id}.wav`;
  writeFileSync(join(output, file), wav);
  const rms = Math.sqrt(cue.data.reduce((sum, value) => sum + value * value, 0) / cue.data.length);
  manifest.cues.push({
    id: cue.id, file, event: cue.label, description: cue.description,
    duration_ms: cue.data.length / sampleRate * 1000,
    peak_dbfs: cue.peak, rms_dbfs: Number((20 * Math.log10(rms)).toFixed(2)),
    bytes: wav.length, sha256: createHash('sha256').update(wav).digest('hex'),
  });
  console.log(`${file.padEnd(23)} ${(cue.data.length / sampleRate * 1000).toFixed(0).padStart(3)} ms  ${cue.peak} dBFS peak`);
}

// Auditions use the actual production samples with unchanged levels. The gaps
// are for listening only and never prescribe operation/lock timing.
function preview(file, entries, durationMs) {
  const data = silence(durationMs);
  const timeline = entries.map(([id, atMs]) => {
    const cue = cues.find(item => item.id === id);
    if (!cue) throw new Error(`Unknown preview cue: ${id}`);
    mix(data, cue.data, atMs);
    return { cue: id, at_ms: atMs };
  });
  writeFileSync(join(output, 'preview', file), encodeWave(data));
  return { file: `preview/${file}`, duration_ms: durationMs, timeline };
}

manifest.previews = [
  preview('all-cues.wav', cues.map((cue, index) => [cue.id, 400 + index * 1200]), 7100),
  preview('fast-operations.wav', [
    ['save-start', 400], ['save-complete', 550],
    ['load-start', 1700], ['load-complete', 1850],
    ['save-start', 3000], ['operation-failed', 3150],
    ['load-start', 4300], ['operation-failed', 4450],
  ], 5200),
];
writeFileSync(join(output, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(`Wrote six cues, two previews and manifest.json to ${output}`);
