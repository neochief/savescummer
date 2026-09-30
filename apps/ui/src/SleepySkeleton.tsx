import { useLayoutEffect, useMemo, useRef, type HTMLAttributes } from 'react';
import source from '../public/character/no-games-found.svg?raw';

/*
 * The empty-library skeleton: bored, sleepy, and mildly responsive to the person using the page.
 *
 * One cycle:
 * - Scowl: it starts in the grumpy face of the original drawing, with deep dark lids over big red eyes.
 * - Drowse: from there it slowly slides into a bored, half-closed look, then heavier and heavier lids. The brows follow
 *   the lids, the head tips forward and sinks, and the lower hand's fingers drum on the sill, slower and softer as it
 *   dozes off.
 * - Asleep: briefly. The red is gone completely, leaving only the curved eyelid lines; the fingers are still.
 * - Wake: suddenly and wide-eyed, with small centred red eyes and a jolt of the head. The fingers of both hands spring
 *   out, and mouse tracking pauses.
 * - Back to the scowl, quickly, as if it just realised what happened and is a bit annoyed, and the cycle repeats.
 *
 * Throughout, both red eyes follow the mouse anywhere in the window, smoothly and within restrained bounds. They're
 * always whole circles, partly hidden by the eye openings; looking all the way down leaves about a fifth of their
 * height peeking over a dark lower rim, which gives the eyes depth.
 *
 * Poking it (clicking the drawn skeleton) startles it awake at once, with a quick shake of the head and a soft bony
 * rattle, which follows the app's sound setting and the `sound` prop.
 *
 * When the mouse leaves the window, it stops the cycle and settles into the awake, bored look glancing left, head at
 * rest, and waits there; when the mouse comes back, a fresh cycle starts from the scowl. Before the mouse first moves,
 * and always under reduced motion, it holds still in the scowl, glancing left.
 *
 * Head movement pivots where the cheek rests on the supporting hand, so the cheek stays on the fingers. The body and
 * arms never move; only the fingers do (the drumming and the startle).
 *
 * The SVG (public/character/no-games-found.svg) is the single source of the artwork and of the rig: separate body
 * parts, a movable head, the original crescent eyes (its standalone static look, never shown here), round eyes, the
 * shared eye openings that both clip and outline them, and invisible guides for the eyelid motion and gaze limits.
 * This component shows an ID-prefixed copy of it (so several can share a page) and animates that copy; the file
 * itself stays a plain static drawing. Editors that drop hidden elements or <defs> on save strip that rig.
 *
 * It scales to its container, pauses while the page is hidden, and cleans up on unmount.
 */

/** Seconds per phase. Drowsiness is slow, waking is quick, the sleep and surprise holds are brief. */
export const skeletonTiming = {
  classic: 1, drowse: 2, heavy: 2, almostClosed: 1.8, closing: 1.1, asleep: 0.9,
  wake: 0.18, surprised: 0.45, recover: 0.3, pointerExit: 0.6,
};

export const skeletonMotion = {
  gazeReach: 420, // head units from between the eyes at which the gaze saturates
  gazeRate: 7, // 1/s: how quickly the eyes ease toward their target
  wakeGazeRate: 40, // eyes snap to centre while surprised
  headGazeTilt: [0.8, 1.6], // degrees of head roll per unit of horizontal / upward gaze
  headSag: 10, // degrees the head droops when fully asleep
  headDrop: 90, // head units the head sinks when fully asleep (~10 px at the app's 140 px size)
  wakeLift: 0.35, // share of headSag the head jerks up on waking
  clickShake: { degrees: 4, frequency: 7, duration: 0.6 }, // damped head wobble around the hand when clicked
  awayGaze: [-1, 0] as [number, number], // gaze while the pointer is outside the window
  // Where the eyeballs rest in the illustrated scowl, matching the red of the original artwork (head coordinates):
  // both eyeballs are big, and the left one sits much lower so only its top shows, like the squint.
  scowlEyes: { left: { center: [529, 611], radius: 60 }, right: { center: [818, 467], radius: 64 } },
  scowlGlance: 0.3, // share of the pointer tracking the eyes keep while scowling
  lidDepth: 20, // how far the dark lids reach in from the socket edge over the eyeballs
  scowlLidDepth: 36, // the same, while scowling
  // The hanging fingers of the lower hand drum on the sill one after another, rightmost first: each lifts `lift` head
  // units, then drops back down fast, followed by a pause. `tempo` is rolls per second when wide awake, slowing toward
  // `sleepyTempo` as the drumming fades; `spread` (start-to-start) and `tap` are shares of one roll, `rise` is the share
  // of a tap spent rising. The top bone lifts only `topShare` of the way, so the lower bones slide up past it.
  drumming: { lift: 44, topShare: 0.4, tempo: 0.9, sleepyTempo: 0.35, spread: 0.2, tap: 0.18, rise: 0.7 },
  // Startled fingers: the lower hand's fingers fan apart by `fan` degrees and their lower bones stretch `reach` head
  // units down; the supporting hand's upper finger bones flick `flick` degrees outward, away from the cheek.
  startle: { fan: 8, reach: 18, flick: 16 },
};

/** The rattle played when the skeleton is poked: `clacks` quick bone clacks `spacing` seconds apart, fading as they go. */
export const skeletonSound = { volume: 0.1, clacks: 3, spacing: 0.05 };

type Side = 'left' | 'right';
type Pose = 'bored' | 'heavy' | 'almost-closed' | 'closed' | 'wake';
type Phase = {
  name: string; pose: Pose; duration: number; sag: number; track: boolean; ease: (t: number) => number; gazeRate?: number;
  follow?: number; // how much the head tilts after the gaze (default 1)
  classic?: number; // blend toward the illustrated scowl: 0 = rigged eyes, 1 = original artwork (default 0)
  drum?: number; // how actively the fingers drum: 0 = still, 1 = wide awake (default 0)
  startle?: number; // how far the fingers spring out in surprise (default 0)
};
type Values = {
  closure: number; sag: number; follow: number; classic: number; drum: number; startle: number; radius: Record<Side, number>;
};

const sides: Side[] = ['left', 'right'];
const drummingFingers = ['Short-outer-digit', 'Middle-hanging-finger', 'Left-hanging-finger']; // roll order, rightmost first
const inOut = (t: number) => 0.5 - Math.cos(Math.PI * t) / 2;
const out = (t: number) => 1 - (1 - t) ** 3;
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

function cycle(): Phase[] {
  const t = skeletonTiming, lift = -skeletonMotion.wakeLift, snap = skeletonMotion.wakeGazeRate;
  return [
    // Falling asleep is one slide: scowl → bored → heavy → almost closed → closed.
    { name: 'classic', pose: 'bored', duration: t.classic, sag: 0, classic: 1, drum: 1, track: true, ease: inOut },
    { name: 'drowse', pose: 'bored', duration: t.drowse, sag: 0, drum: 0.8, track: true, ease: inOut },
    { name: 'heavy', pose: 'heavy', duration: t.heavy, sag: 0.45, drum: 0.35, track: true, ease: inOut },
    { name: 'almost-closed', pose: 'almost-closed', duration: t.almostClosed, sag: 0.8, drum: 0.08, track: true, ease: inOut },
    { name: 'closing', pose: 'closed', duration: t.closing, sag: 1, track: true, ease: inOut },
    { name: 'asleep', pose: 'closed', duration: t.asleep, sag: 1, track: true, ease: inOut },
    { name: 'wake', pose: 'wake', duration: t.wake, sag: lift, startle: 1, track: false, ease: out, gazeRate: snap },
    { name: 'surprised', pose: 'wake', duration: t.surprised, sag: lift, startle: 1, track: false, ease: inOut, gazeRate: snap },
    { name: 'recover', pose: 'bored', duration: t.recover, sag: 0, classic: 1, drum: 1, track: false, ease: inOut },
  ];
}

/** Clamps a gaze direction to the unit disk, then maps it into the eye's asymmetric oval (up is shallower than down). */
export function gazeOffset(u: number, v: number, limits: { horizontal: number; up: number; down: number }) {
  const length = Math.hypot(u, v);
  if (length > 1) { u /= length; v /= length; }
  return [u * limits.horizontal, v * (v < 0 ? limits.up : limits.down)];
}

const numbers = (d: string) => d.match(/-?\d*\.?\d+/g)!.map(Number);

/** Interpolates two paths with identical command structure, number by number. */
function morph(from: string, to: string) {
  const a = numbers(from), b = numbers(to);
  if (a.length !== b.length) throw new Error('Eyelid guides do not correspond');
  return (t: number) => { let i = 0; return from.replace(/-?\d*\.?\d+/g, () => (+lerp(a[i], b[i++], t).toFixed(2)).toString()); };
}

type Point = [number, number];

/** Points evenly spaced along a closed path made of one M and absolute C commands (all shapes in this rig). */
function outline(d: string, count = 96): Point[] {
  const n = numbers(d), dense: Point[] = [];
  for (let i = 2; i + 5 < n.length; i += 6) {
    const x0 = n[i - 2], y0 = n[i - 1];
    for (let s = 0; s < 16; s++) {
      const t = s / 16, u = 1 - t, a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, e = t * t * t;
      dense.push([a * x0 + b * n[i] + c * n[i + 2] + e * n[i + 4], a * y0 + b * n[i + 1] + c * n[i + 3] + e * n[i + 5]]);
    }
  }
  dense.push(dense[0]);
  const lengths = [0];
  for (let i = 1; i < dense.length; i++) lengths.push(lengths[i - 1] + Math.hypot(dense[i][0] - dense[i - 1][0], dense[i][1] - dense[i - 1][1]));
  const points: Point[] = [];
  for (let k = 0, j = 0; k < count; k++) {
    const at = lengths[lengths.length - 1] * k / count;
    while (lengths[j + 1] < at) j++;
    const t = (at - lengths[j]) / (lengths[j + 1] - lengths[j] || 1);
    points.push([lerp(dense[j][0], dense[j + 1][0], t), lerp(dense[j][1], dense[j + 1][1], t)]);
  }
  return points;
}

const area = (points: Point[]) => points.reduce((sum, [x, y], i) => {
  const [nx, ny] = points[(i + 1) % points.length];
  return sum + x * ny - nx * y;
}, 0);

/**
 * Blends two unrelated closed shapes: both are sampled evenly, wound the same way, and rotated so corresponding points
 * sit closest together, then interpolated point by point.
 */
function blendOutlines(from: string, to: Point[], t: number) {
  const a = outline(from, to.length);
  const b = area(a) * area(to) < 0 ? [...to].reverse() : to;
  let offset = 0, best = Infinity;
  for (let o = 0; o < b.length; o++) {
    let cost = 0;
    for (let i = 0; i < a.length && cost < best; i++) {
      const p = b[(i + o) % b.length];
      cost += (p[0] - a[i][0]) ** 2 + (p[1] - a[i][1]) ** 2;
    }
    if (cost < best) { best = cost; offset = o; }
  }
  return `M${a.map((p, i) => {
    const q = b[(i + offset) % b.length];
    return `${lerp(p[0], q[0], t).toFixed(1)},${lerp(p[1], q[1], t).toFixed(1)}`;
  }).join('L')}Z`;
}

/**
 * A short bony rattle, synthesised so no audio asset is needed. Each clack is a burst of band-passed noise (the tick)
 * over a quickly falling triangle tone (the hollow knock); pitch and timing vary a little so no two pokes sound alike.
 */
function rattle(audio: BaseAudioContext) {
  const { volume, clacks, spacing } = skeletonSound;
  const noise = audio.createBuffer(1, Math.round(audio.sampleRate * 0.05), audio.sampleRate);
  const samples = noise.getChannelData(0);
  for (let i = 0; i < samples.length; i++) samples[i] = Math.random() * 2 - 1;
  for (let n = 0; n < clacks; n++) {
    const at = audio.currentTime + n * spacing * (0.8 + Math.random() * 0.5);
    const level = audio.createGain();
    level.gain.setValueAtTime(volume * (1 - n / (clacks + 1)), at);
    level.gain.exponentialRampToValueAtTime(0.001, at + 0.08);
    level.connect(audio.destination);
    const tick = audio.createBufferSource(), band = audio.createBiquadFilter();
    tick.buffer = noise;
    band.type = 'bandpass';
    band.frequency.value = 1800 + Math.random() * 1200;
    band.Q.value = 3;
    tick.connect(band).connect(level);
    tick.start(at);
    const knock = audio.createOscillator(), knockLevel = audio.createGain();
    knock.type = 'triangle';
    knock.frequency.setValueAtTime(650 + Math.random() * 300, at);
    knock.frequency.exponentialRampToValueAtTime(320, at + 0.07);
    knockLevel.gain.value = 0.6;
    knock.connect(knockLevel).connect(level);
    knock.start(at);
    knock.stop(at + 0.09);
  }
}

// Every instance gets its own ID prefix so clip paths and <use> references never resolve into another copy.
let instances = 0;
function inlineSvg(prefix: string) {
  const doc = new DOMParser().parseFromString(source, 'image/svg+xml');
  const svg = doc.documentElement;
  svg.querySelectorAll('title, desc').forEach((node) => node.remove());
  svg.removeAttribute('aria-labelledby');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('focusable', 'false');
  // Round eyeballs only: the illustrated crescents never show. The rim's original width is kept aside because the
  // scowl thins it (the illustrated sockets draw their own outline) and setup may run twice under StrictMode.
  for (const side of sides) {
    const part = (id: string) => svg.querySelector(`[id="${id}-${side}"]`)!;
    part('iris-static').remove();
    part('iris-dynamic').removeAttribute('style');
    part('eye-upper-lid').removeAttribute('style');
    part('upper-lid-line').setAttribute('data-rim-width', part('upper-lid-line').getAttribute('stroke-width')!);
    // Deep lids: the socket outline again, but inside the eyeball's clip, so a thick stroke only reaches
    // inward over the red and never widens the socket.
    const lids = doc.createElementNS('http://www.w3.org/2000/svg', 'use');
    lids.setAttribute('id', `eye-scowl-lids-${side}`);
    lids.setAttribute('href', `#eye-opening-${side}`);
    lids.setAttribute('fill', 'none');
    lids.setAttribute('stroke', part('upper-lid-line').getAttribute('stroke')!);
    lids.setAttribute('stroke-width', '0');
    lids.setAttribute('stroke-linejoin', 'round');
    part('eye-contents').append(lids);
  }
  // The fingers' and their bones' drawn placement, kept aside because taps add to it and setup may run twice under
  // StrictMode.
  for (const id of drummingFingers) {
    const finger = svg.querySelector(`[id="${id}"]`)!;
    for (const node of [finger, ...finger.children]) node.setAttribute('data-drawn-transform', node.getAttribute('transform') ?? '');
  }
  for (const node of [svg, ...svg.querySelectorAll('*')]) {
    for (const attr of [...node.attributes]) {
      if (attr.name === 'id') attr.value = prefix + attr.value;
      else if ((attr.name === 'href' || attr.name === 'xlink:href') && attr.value.startsWith('#')) attr.value = `#${prefix}${attr.value.slice(1)}`;
      else if (attr.value.includes('url(#')) attr.value = attr.value.replace(/url\(#/g, `url(#${prefix}`);
    }
  }
  return new XMLSerializer().serializeToString(svg);
}

function rig(root: SVGSVGElement, prefix: string) {
  const el = <T extends Element = SVGElement>(id: string) => root.getElementById(prefix + id) as T;
  const num = (id: string, attr: string) => Number(el(id).getAttribute(attr));
  const eyes = Object.fromEntries(sides.map((side) => {
    const bounds = `gaze-bounds-${side}`;
    const opening = el(`eye-opening-${side}`), brow = el(`brow-${side}`);
    const illustrated = { opening: el(`pose-illustrated-opening-${side}`).getAttribute('d')!, brow: el(`pose-illustrated-brow-${side}`).getAttribute('d')! };
    return [side, {
      opening, brow, illustrated,
      classicOutline: { opening: outline(illustrated.opening), brow: outline(illustrated.brow) },
      openingAt: morph(el(`guide-open-opening-${side}`).getAttribute('d')!, el(`guide-closed-opening-${side}`).getAttribute('d')!),
      browAt: morph(el(`guide-open-brow-${side}`).getAttribute('d')!, el(`guide-closed-brow-${side}`).getAttribute('d')!),
      neutral: [num(`gaze-neutral-${side}`, 'cx'), num(`gaze-neutral-${side}`, 'cy')],
      limits: { horizontal: num(bounds, 'data-max-horizontal'), up: num(bounds, 'data-max-up'), down: num(bounds, 'data-max-down') },
      gaze: el(`iris-gaze-${side}`), circle: el(`iris-circle-${side}`),
      rim: el(`upper-lid-line-${side}`), rimWidth: num(`upper-lid-line-${side}`, 'data-rim-width'), lids: el(`eye-scowl-lids-${side}`),
    }];
  })) as Record<Side, {
    opening: SVGElement; brow: SVGElement;
    illustrated: { opening: string; brow: string }; classicOutline: { opening: Point[]; brow: Point[] };
    openingAt: (t: number) => string; browAt: (t: number) => string; neutral: number[];
    limits: { horizontal: number; up: number; down: number };
    gaze: SVGElement; circle: SVGElement; rim: SVGElement; rimWidth: number; lids: SVGElement;
  }>;
  const presets = Object.fromEntries((['bored', 'heavy', 'almost-closed', 'closed', 'wake'] as Pose[]).map((name) => {
    const preset = el(`pose-${name}`);
    const value = (attr: string) => Number(preset.getAttribute(attr));
    return [name, { closure: value('data-closure-progress'), sag: 0, follow: 1, classic: 0, drum: 1, startle: 0, radius: { left: value('data-iris-radius-left'), right: value('data-iris-radius-right') } }];
  })) as Record<Pose, Values>;
  const pose = (name: Pose) => presets[name];
  const head = el<SVGGraphicsElement>('head-pose');
  const pivot = head.getAttribute('data-pivot')!.split(/\s+/).map(Number);
  const between = [(eyes.left.neutral[0] + eyes.right.neutral[0]) / 2, (eyes.left.neutral[1] + eyes.right.neutral[1]) / 2];
  const fingers = drummingFingers.map((id) => {
    const finger = el<SVGGraphicsElement>(id), drawn = finger.getAttribute('data-drawn-transform')!;
    // A straight-up lift in the hand's space, expressed in the finger's own (drawn, rotated) space for its bones.
    const inverse = new DOMMatrix(drawn || 'none').inverse();
    const up = (dy: number) => [inverse.c * -dy, inverse.d * -dy];
    const bones = [...finger.children].map((bone) => ({ bone, drawn: bone.getAttribute('data-drawn-transform')! }));
    // Startled fingers fan around their knuckle: the top middle of the finger, placed as drawn.
    const box = finger.getBBox(), knuckle = new DOMMatrix(drawn || 'none').transformPoint(new DOMPoint(box.x + box.width / 2, box.y));
    return { finger, drawn, up, bones, knuckle: [knuckle.x, knuckle.y] };
  });
  // The supporting hand's upper finger bones, each flicking around its joint with the bone below (its bottom middle).
  const flickers = ['support-outer-distal', 'support-middle-distal', 'support-inner-distal'].map((id) => {
    const bone = el<SVGGraphicsElement>(id), box = bone.getBBox();
    return { bone, joint: [box.x + box.width / 2, box.y + box.height] };
  });
  return { eyes, pose, head, pivot, between, fingers, flickers };
}

/**
 * The bored skeleton from the empty library: eyes follow the pointer while it slowly nods off, then startles awake.
 * Clicking it startles it awake immediately, with a bony rattle unless `sound` is false.
 * Holds still in the illustrated scowl, glancing left, until the pointer first moves, and permanently under
 * reduced motion.
 */
export function SleepySkeleton({ sound = true, ...props }: HTMLAttributes<HTMLDivElement> & { sound?: boolean }) {
  const container = useRef<HTMLDivElement>(null);
  const soundOn = useRef(sound);
  soundOn.current = sound;
  const prefix = useMemo(() => `skeleton${++instances}-`, []);
  const markup = useMemo(() => ({ __html: inlineSvg(prefix) }), [prefix]);

  // Layout effect: the first frame is drawn before the browser paints, so the raw SVG pose never flashes.
  useLayoutEffect(() => {
    if (typeof window.matchMedia !== 'function') return; // no media queries (e.g. jsdom): hold still
    const root = container.current!.querySelector('svg')!;
    const { eyes, pose, head, pivot, between, fingers, flickers } = rig(root, prefix);
    const phases = cycle();
    const motion = skeletonMotion;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');

    let mode: 'static' | 'cycle' | 'away' = 'static';
    let index = 0, elapsed = 0, frame = 0, last = 0, shaken = Infinity; // seconds since the last click
    let drumClock = 0; // progress through drum rolls
    let audio: AudioContext | undefined; // created on the first poke, which is a user gesture browsers allow sound from
    const classicPose: Values = { ...pose('bored'), classic: 1 };
    let from = classicPose, current = classicPose;
    let pointer: [number, number] | undefined;
    const gaze = [0, 0];

    const enter = (next: typeof mode, phase?: string) => {
      mode = next; index = Math.max(0, phases.findIndex((step) => step.name === phase)); elapsed = 0; from = current;
    };
    const phase = (): Phase => mode === 'away'
      ? { name: 'away', pose: 'bored', duration: skeletonTiming.pointerExit, sag: 0, follow: 0, drum: 0, track: false, ease: inOut }
      : phases[index];

    const pointerDirection = () => {
      if (!pointer) return [0, 0];
      const ctm = head.getScreenCTM();
      if (!ctm) return [0, 0];
      const p = new DOMPoint(pointer[0], pointer[1]).matrixTransform(ctm.inverse());
      return [(p.x - between[0]) / motion.gazeReach, (p.y - between[1]) / motion.gazeReach];
    };

    const draw = () => {
      const drumming = motion.drumming;
      const { fan, reach, flick } = motion.startle;
      fingers.forEach(({ finger, drawn, up, bones, knuckle: [kx, ky] }, i) => {
        // Rise smoothly, then strike down with growing speed.
        const at = ((((drumClock - i * drumming.spread) % 1) + 1) % 1) / drumming.tap;
        const lift = at >= 1 ? 0 : at < drumming.rise ? inOut(at / drumming.rise) : 1 - ((at - drumming.rise) / (1 - drumming.rise)) ** 2;
        const rise = drumming.lift * current.drum * lift;
        // The whole finger lifts by the top bone's share; the lower bones lift the rest of the way on top of that.
        // Startled, the outer fingers fan away from the middle one and the lower bones stretch down.
        const spread = (i - (fingers.length - 1) / 2) * fan * current.startle;
        finger.setAttribute('transform',
          `rotate(${spread.toFixed(2)} ${kx.toFixed(1)} ${ky.toFixed(1)}) translate(0 ${(-rise * drumming.topShare).toFixed(2)}) ${drawn}`);
        const [x, y] = up(rise * (1 - drumming.topShare) - reach * current.startle);
        bones.slice(1).forEach(({ bone, drawn: placed }) => bone.setAttribute('transform', `translate(${x.toFixed(2)} ${y.toFixed(2)}) ${placed}`));
      });
      for (const { bone, joint: [jx, jy] } of flickers) {
        bone.setAttribute('transform', `rotate(${(flick * current.startle).toFixed(2)} ${jx.toFixed(1)} ${jy.toFixed(1)})`);
      }
      const tilt = gaze[0] * motion.headGazeTilt[0] - Math.min(gaze[1], 0) * motion.headGazeTilt[1];
      const { degrees, frequency, duration } = motion.clickShake;
      const shake = shaken < duration ? degrees * (1 - shaken / duration) ** 2 * Math.sin(2 * Math.PI * frequency * shaken) : 0;
      const angle = -current.sag * motion.headSag + current.follow * tilt + shake;
      const drop = current.sag * motion.headDrop;
      head.setAttribute('transform', `translate(0 ${drop.toFixed(2)}) rotate(${angle.toFixed(3)} ${pivot[0]} ${pivot[1]})`);
      const classic = current.classic;
      for (const side of sides) {
        const eye = eyes[side];
        // The scowl pulls each eyeball to its illustrated resting place and size, keeping a little of the tracking.
        const [dx, dy] = gazeOffset(gaze[0], gaze[1], eye.limits);
        const rest = motion.scowlEyes[side], glance = lerp(1, motion.scowlGlance, classic);
        const x = (rest.center[0] - eye.neutral[0]) * classic + dx * glance;
        const y = (rest.center[1] - eye.neutral[1]) * classic + dy * glance;
        const shape = (part: 'opening' | 'brow', rigged: string) =>
          classic <= 0 ? rigged : classic >= 1 ? eye.illustrated[part] : blendOutlines(rigged, eye.classicOutline[part], classic);
        eye.opening.setAttribute('d', shape('opening', eye.openingAt(current.closure)));
        eye.brow.setAttribute('d', shape('brow', eye.browAt(current.closure)));
        eye.circle.setAttribute('r', lerp(current.radius[side], rest.radius, classic).toFixed(2));
        eye.gaze.setAttribute('transform', `translate(${x.toFixed(2)} ${y.toFixed(2)})`);
        eye.rim.setAttribute('stroke-width', (eye.rimWidth * (1 - classic)).toFixed(2));
        eye.lids.setAttribute('stroke-width', (2 * lerp(motion.lidDepth, motion.scowlLidDepth, classic)).toFixed(2));
      }
    };

    const tick = (now: number) => {
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0;
      last = now;
      let step = phase();
      elapsed += dt;
      shaken += dt;
      while (mode === 'cycle' && elapsed >= step.duration) {
        elapsed -= step.duration;
        from = {
          ...pose(step.pose), sag: step.sag, follow: step.follow ?? 1, classic: step.classic ?? 0, drum: step.drum ?? 0,
          startle: step.startle ?? 0,
        };
        index = (index + 1) % phases.length; step = phase();
      }
      const t = step.ease(Math.min(elapsed / step.duration, 1));
      const to = pose(step.pose);
      current = {
        closure: lerp(from.closure, to.closure, t), sag: lerp(from.sag, step.sag, t), follow: lerp(from.follow, step.follow ?? 1, t),
        drum: lerp(from.drum, step.drum ?? 0, t), startle: lerp(from.startle, step.startle ?? 0, t),
        classic: lerp(from.classic, step.classic ?? 0, t),
        radius: { left: lerp(from.radius.left, to.radius.left, t), right: lerp(from.radius.right, to.radius.right, t) },
      };
      const { tempo, sleepyTempo } = motion.drumming;
      drumClock += dt * lerp(sleepyTempo, tempo, current.drum);
      const target = mode === 'away' ? motion.awayGaze : step.track ? pointerDirection() : [0, 0];
      const length = Math.hypot(target[0], target[1]);
      const [u, v] = length > 1 ? [target[0] / length, target[1] / length] : target;
      const k = 1 - Math.exp(-(step.gazeRate ?? motion.gazeRate) * dt);
      gaze[0] += (u - gaze[0]) * k;
      gaze[1] += (v - gaze[1]) * k;
      draw();
      // Once the pointer-exit pose has settled nothing moves, so stop until the pointer comes back.
      const settled = mode === 'away' && elapsed >= step.duration && Math.hypot(u - gaze[0], v - gaze[1]) < 0.001;
      frame = settled ? 0 : requestAnimationFrame(tick);
    };
    const run = () => {
      if (frame || mode === 'static' || document.hidden) return;
      last = 0;
      frame = requestAnimationFrame(tick);
    };
    const stop = () => { cancelAnimationFrame(frame); frame = 0; };

    const onMove = (event: PointerEvent) => {
      pointer = [event.clientX, event.clientY];
      if (reduced.matches) return;
      if (mode === 'static' || mode === 'away') enter('cycle', 'classic');
      run();
    };
    // relatedTarget is null only when the pointer leaves the window itself, not the character or any other element.
    const onOut = (event: MouseEvent) => {
      if (event.relatedTarget || mode === 'static') return;
      pointer = undefined;
      enter('away');
      run();
    };
    // A click startles the skeleton awake from wherever it is in the cycle, with a quick shake of the head.
    const onClick = (event: MouseEvent) => {
      if (soundOn.current && typeof AudioContext === 'function') {
        audio ??= new AudioContext();
        if (audio.state === 'suspended') audio.resume().catch(() => undefined);
        rattle(audio);
      }
      if (reduced.matches) return;
      onMove(event as PointerEvent);
      enter('cycle', 'wake');
      shaken = 0;
    };
    const onVisibility = () => document.hidden ? stop() : run();
    // Holding still is simply the scowl glancing left, with the head at rest.
    const showStatic = () => {
      stop(); mode = 'static'; current = classicPose; [gaze[0], gaze[1]] = motion.awayGaze;
      draw(); head.removeAttribute('transform');
    };
    const onReducedMotion = () => { if (reduced.matches) showStatic(); };

    showStatic();
    addEventListener('pointermove', onMove);
    root.addEventListener('click', onClick);
    document.addEventListener('mouseout', onOut);
    document.addEventListener('visibilitychange', onVisibility);
    reduced.addEventListener('change', onReducedMotion);
    return () => {
      stop();
      removeEventListener('pointermove', onMove);
      root.removeEventListener('click', onClick);
      document.removeEventListener('mouseout', onOut);
      document.removeEventListener('visibilitychange', onVisibility);
      reduced.removeEventListener('change', onReducedMotion);
      audio?.close().catch(() => undefined);
    };
  }, [prefix]);

  return <div {...props} ref={container} dangerouslySetInnerHTML={markup} />;
}
