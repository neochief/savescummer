import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

// Original, deterministic 48 kHz mono PCM. Every cue is composed here;
// no existing application sound or third-party sample is used as source.
const uiRoot = join(dirname(fileURLToPath(import.meta.url)), '../public/sounds');
const hostRoot = join(dirname(fileURLToPath(import.meta.url)), '../../../assets/sounds');
const sampleRate = 48000;
const wood = { wave: 'sine', pitch: .77, harmonic: .28, overtone: 0, noise: .42, decay: 2.7, sweep: -.10 };

// Notes: [start seconds, frequency Hz, length seconds, amplitude, pitch travel].
// The two-note pickup is the start cue. The finish is only its final landing;
// played together, they form one rising Save or falling Load phrase.
const savePickup = [[0, 470, .072, .67, .16], [.065, 620, .068, .71, .08]];
const loadPickup = [[0, 790, .075, .70, -.14], [.067, 570, .066, .70, -.10]];
const cues = {
  button: { duration: .078, notes: [[0, 620, .065, .8, -.12]], click: .10, peakDb: -23 },
  checkbox: { duration: .122, notes: [[0, 770, .047, .55, -.18], [.041, 510, .072, .75, -.08]], click: .24, peakDb: -22 },
  success: { duration: .285, notes: [[0, 494, .105, .55, .08], [.083, 622, .107, .58, .06], [.165, 740, .113, .63, -.03]], click: .02, peakDb: -20 },
  detected: { duration: .205, notes: [[0, 370, .092, .7, .27], [.075, 554, .119, .72, -.12]], click: .02, peakDb: -20 },
  'detected-batch': { duration: .410, notes: [[0, 370, .094, .58, .27], [.112, 440, .104, .57, .18], [.244, 554, .150, .67, -.11]], click: .02, peakDb: -22 },
  'game-run': { duration: .255, notes: [[0, 280, .118, .65, .58], [.082, 520, .158, .74, .22]], click: .08, peakDb: -19 },
  'game-stop': { duration: .230, notes: [[0, 590, .105, .56, -.25], [.076, 280, .141, .72, -.22]], click: .05, peakDb: -21 },
  undo: { duration: .188, notes: [[0, 390, .088, .54, .32], [.063, 630, .110, .67, .10]], click: .07, peakDb: -21 },
  'save-start': { duration: .150, notes: savePickup, click: .07, peakDb: -20 },
  'save-complete': { duration: .160, notes: [[0, 785, .145, .86, -.02]], click: .02, peakDb: -17 },
  'load-start': { duration: .150, notes: loadPickup, click: .07, peakDb: -20 },
  'load-complete': { duration: .160, notes: [[0, 390, .145, .88, -.03]], click: .02, peakDb: -17 },
  'operation-failed': { duration: .222, notes: [[0, 346, .117, .80, -.48], [.119, 185, .090, .52, -.24]], click: .38, peakDb: -18 },
  busy: { duration: .044, notes: [[0, 380, .034, .50, -.18]], click: .48, peakDb: -29 },
};

function synth(cue, style, seed) {
  const count = Math.round(cue.duration * sampleRate);
  const samples = new Float64Array(count);
  let random = seed;
  let previousNoise = 0;
  for (const [at, baseFrequency, length, amplitude, travel] of cue.notes) {
    const begin = Math.round(at * sampleRate);
    const end = Math.min(count, begin + Math.round(length * sampleRate));
    const frequency = baseFrequency * style.pitch;
    for (let i = begin; i < end; i++) {
      const t = (i - begin) / sampleRate;
      const phase = 2 * Math.PI * frequency * (t + (travel + style.sweep) * t * t / (2 * length));
      const sin = Math.sin(phase);
      const fundamental = style.wave === 'triangle' ? 2 / Math.PI * Math.asin(sin)
        : style.wave === 'pulse' ? Math.tanh(2.7 * sin) : sin;
      const color = fundamental + style.harmonic * Math.sin(phase * 2) + style.overtone * Math.sin(phase * 2.72);
      const attack = Math.min(1, t / .0035);
      const decay = Math.pow(1 - t / length, style.decay);
      samples[i] += amplitude * attack * decay * color;
    }
  }
  const transient = Math.min(count, Math.round(.024 * sampleRate));
  for (let i = 0; i < transient; i++) {
    random = (1664525 * random + 1013904223) >>> 0;
    const noise = random / 0xffffffff * 2 - 1;
    const snap = noise - previousNoise * .78;
    previousNoise = noise;
    samples[i] += snap * cue.click * style.noise * Math.pow(1 - i / transient, 3);
  }
  // Give each cue a stable level without changing the system volume.
  let peak = 0;
  for (let i = 0; i < count; i++) peak = Math.max(peak, Math.abs(samples[i] * Math.min(1, (count - i) / 650)));
  const level = 10 ** (cue.peakDb / 20) / peak;
  const pcm = Buffer.alloc(count * 2);
  for (let i = 0; i < count; i++) {
    const fade = Math.min(1, (count - i) / 650);
    pcm.writeInt16LE(Math.round(Math.max(-1, Math.min(1, samples[i] * fade * level)) * 32767), i * 2);
  }
  return pcm;
}

// A quiet, band-limited brush of friction for the Info sheet sliding in its pocket.
// The two envelopes follow the different opening and closing animation lengths.
function drawer(open, seed) {
  const count = Math.round((open ? .240 : .175) * sampleRate);
  const samples = new Float64Array(count);
  let random = seed, upper = 0, lower = 0, peak = 0;
  for (let i = 0; i < count; i++) {
    random = (1664525 * random + 1013904223) >>> 0;
    const noise = random / 0xffffffff * 2 - 1;
    upper += .58 * (noise - upper);
    lower += .11 * (noise - lower);
    const t = i / sampleRate, progress = i / (count - 1);
    const attack = Math.min(1, t / .006);
    const release = Math.min(1, (count - 1 - i) / (sampleRate * .030));
    const envelope = attack * release * (open ? .75 + .25 * progress : 1 - .35 * progress);
    samples[i] = envelope * ((upper - lower) + .035 * Math.sin(2 * Math.PI * (155 + 18 * progress) * t));
    peak = Math.max(peak, Math.abs(samples[i]));
  }
  const level = 10 ** ((open ? -27 : -28) / 20) / peak;
  const pcm = Buffer.alloc(count * 2);
  for (let i = 0; i < count; i++) pcm.writeInt16LE(Math.round(samples[i] * level * 32767), i * 2);
  return pcm;
}

function wav(pcm) {
  const out = Buffer.alloc(44 + pcm.length);
  out.write('RIFF', 0);
  out.writeUInt32LE(out.length - 8, 4);
  out.write('WAVEfmt ', 8);
  out.writeUInt32LE(16, 16);
  out.writeUInt16LE(1, 20);
  out.writeUInt16LE(1, 22);
  out.writeUInt32LE(sampleRate, 24);
  out.writeUInt32LE(sampleRate * 2, 28);
  out.writeUInt16LE(2, 32);
  out.writeUInt16LE(16, 34);
  out.write('data', 36);
  out.writeUInt32LE(pcm.length, 40);
  pcm.copy(out, 44);
  return out;
}

const operationNames = ['save-start', 'save-complete', 'load-start', 'load-complete', 'operation-failed', 'busy'];
const hostCues = new Set(operationNames);
mkdirSync(uiRoot, { recursive: true });
mkdirSync(hostRoot, { recursive: true });
Object.entries(cues).forEach(([name, cue], index) => {
  const bytes = wav(synth(cue, wood, 3017 + index));
  writeFileSync(join(uiRoot, `${name}.wav`), bytes);
  if (hostCues.has(name)) writeFileSync(join(hostRoot, `${name}.wav`), bytes);
});
writeFileSync(join(uiRoot, 'drawer-open.wav'), wav(drawer(true, 9283)));
writeFileSync(join(uiRoot, 'drawer-close.wav'), wav(drawer(false, 9284)));

const descriptions = {
  'save-start': 'Rising two-note pickup.',
  'save-complete': 'Higher landing that completes Save.',
  'load-start': 'Falling two-note pickup.',
  'load-complete': 'Lower landing that completes Load.',
  'operation-failed': 'Textured descending response.',
  busy: 'Quiet dry tick.',
};

function stats(file) {
  const bytes = readFileSync(file);
  const count = (bytes.length - 44) / 2;
  let peak = 0, power = 0;
  for (let i = 0; i < count; i++) {
    const value = bytes.readInt16LE(44 + i * 2) / 32768;
    peak = Math.max(peak, Math.abs(value));
    power += value * value;
  }
  return {
    duration_ms: count / sampleRate * 1000,
    peak_dbfs: Number((20 * Math.log10(peak)).toFixed(1)),
    rms_dbfs: Number((20 * Math.log10(Math.sqrt(power / count))).toFixed(1)),
    bytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  };
}

writeFileSync(join(hostRoot, 'manifest.json'), JSON.stringify({
  format: { container: 'wav', encoding: 'PCM', sample_rate_hz: sampleRate, bits_per_sample: 16, channels: 1 },
  source: 'Original procedural synthesis; no recordings or third-party samples.',
  generator: 'apps/ui/scripts/generate-interface-sounds.mjs',
  style: 'wood',
  cues: operationNames.map((name) => ({ id: name, file: `${name}.wav`, description: descriptions[name], ...stats(join(hostRoot, `${name}.wav`)) })),
}, null, 2) + '\n');
