import { useLayoutEffect, useMemo, useRef, type HTMLAttributes } from 'react';
import source from '../public/character/no-checkpoints.svg?raw';
import { between as midpoint, easeGaze, findEyes, gazeOffset, morph, pointerGaze, prepareEyes, type Point } from './skeletonEyes';
import { playSkeletonRattle, skeletonMotion, skeletonTiming } from './SleepySkeleton';

/*
 * The "no checkpoints" skeleton: crouched like a sprinter at the start line, gamepad in the back hand, the other hand
 * on the ground, getting ready to run.
 *
 * - It bounces on its legs while it dozes, then coils as it falls asleep. The gamepad hand falls to the ground,
 *   startling the face and body into a forward twitch. A click starts the same wake and twitch immediately, with the
 *   sleepy skeleton's bony rattle when sound is enabled.
 *   Feet and the grounded hand stay planted; the legs and supporting arm bend to keep reaching them.
 * - The red eyes follow the mouse anywhere in the window, looking ahead while the mouse is outside the window or
 *   before it first moves.
 * - Under reduced motion it holds still in the drawn crouch, looking ahead.
 *
 * The SVG (public/character/no-checkpoints.svg) is the single source of the artwork and its rig. The head holds the
 * eyes laid out as skeletonEyes.ts requires, and a 0%-opacity `head-rig` layer with the eyes' gaze, sleepy
 * open/closed and startled wake guides (with matching path commands). A 0%-opacity `body-rig` layer holds the joint
 * pivots. This component shows an ID-prefixed copy of it (so several can share a page) and animates that copy; the
 * file itself stays a plain static drawing. The rig uses only named, transparent shapes, so it survives an Affinity
 * Designer round trip; hidden layers, data attributes and
 * hand-written <defs> would not. Layer names are the IDs this component looks up. Timing and amplitudes live here.
 *
 * It scales to its container, pauses while the page is hidden, and cleans up on unmount.
 */

export const crouchingSkeletonMotion = {
  rhythm: { bouncePeriods: [2, 2.3, 2.5] as [number, number, number], overshoot: 0.35 },
  sleepy: {
    headDrop: 32, headTilt: 18,
    armDroop: -28, handDrop: -44, handDropLead: 0.28,
  }, // the gamepad arm pivots down at the shoulder, then falls sharply just before waking
  // The body at full depth for each move, in drawing units: how far it moves back (negative x) and down, and how many
  // degrees it tips forward around the pelvis. The head turns back `headKeep` of that tip, so the eyes stay on target.
  depth: { bounce: { back: 10, down: 26, tip: 2 }, spring: { back: 28, down: 26, tip: 2.5 }, headKeep: 0.6 },
  lidDepth: 18, // how far the dark lids reach in from the opening edge over the eyeballs
  rimWidth: 16, // the dark rim around the round eyes
  gazeReach: 420, // head units from between the eyes at which the gaze saturates
  gazeRate: 7, // 1/s: how quickly the eyes ease toward their target
  dartRate: 30, // the same, while snapping ahead on waking
  ahead: [1, 0.15] as [number, number], // where "straight ahead" is: toward the run, slightly down
  // The gamepad arm swings at the elbow against the body's vertical speed (degrees per drawing unit per second),
  // easing at `swingRate`, never beyond `maxSwing`.
  swing: 0.06, swingRate: 10, maxSwing: 10,
};

const inOut = (t: number) => 0.5 - Math.cos(Math.PI * Math.min(Math.max(t, 0), 1)) / 2;
const out = (t: number) => 1 - (1 - Math.min(Math.max(t, 0), 1)) ** 3;
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const degrees = (radians: number) => radians * 180 / Math.PI;
const angle = ([x, y]: Point) => degrees(Math.atan2(y, x));

type HeadPose = { closure: number; sag: number; startle: number };
const headPhases = [
  { duration: skeletonTiming.classic, pose: { closure: 0, sag: 0, startle: 0 } },
  { duration: skeletonTiming.drowse, pose: { closure: skeletonMotion.poses.bored.closure, sag: 0.15, startle: 0 } },
  { duration: skeletonTiming.heavy, pose: { closure: skeletonMotion.poses.heavy.closure, sag: 0.45, startle: 0 } },
  { duration: skeletonTiming.almostClosed, pose: { closure: skeletonMotion.poses['almost-closed'].closure, sag: 0.8, startle: 0 } },
  { duration: skeletonTiming.closing, pose: { closure: 1, sag: 1, startle: 0 } },
  { duration: skeletonTiming.asleep, pose: { closure: 1, sag: 1, startle: 0 } },
  { duration: skeletonTiming.wake, pose: { closure: 0, sag: -skeletonMotion.wakeLift, startle: 1 }, ease: out },
  { duration: skeletonTiming.surprised, pose: { closure: 0, sag: -skeletonMotion.wakeLift, startle: 1 } },
  { duration: skeletonTiming.recover, pose: { closure: 0, sag: 0, startle: 0 } },
];
const headCycleDuration = headPhases.reduce((sum, phase) => sum + phase.duration, 0);
const wakeDuration = skeletonTiming.wake + skeletonTiming.surprised + skeletonTiming.recover;
const bounceDuration = crouchingSkeletonMotion.rhythm.bouncePeriods.reduce((sum, period) => sum + period, 0);
const wakeStart = headCycleDuration - wakeDuration;

/** Extra shoulder rotation from the late hand drop, released as the skeleton wakes. */
export function crouchingHandDropAt(seconds: number) {
  const elapsed = seconds % headCycleDuration;
  const { armDroop, handDrop, handDropLead } = crouchingSkeletonMotion.sleepy;
  const dropStart = wakeStart - handDropLead;
  if (elapsed < dropStart) return 0;
  if (elapsed < wakeStart) {
    const t = (elapsed - dropStart) / handDropLead;
    return (handDrop - armDroop) * t * t;
  }
  return (handDrop - armDroop) * (1 - out((elapsed - wakeStart) / skeletonTiming.wake));
}

/** The sibling's drowse, sleep and wake timing, applied to this character's head and eye shapes. */
export function crouchingHeadAt(seconds: number): HeadPose {
  const cycleElapsed = seconds % headCycleDuration;
  let elapsed = cycleElapsed;
  // Keep the head and gamepad arm descending steadily from the start, rather than easing anew at each eye pose.
  const sleepySag = Math.min(cycleElapsed / (wakeStart - skeletonTiming.asleep), 1);
  let from: HeadPose = { closure: 0, sag: 0, startle: 0 };
  for (const { duration, pose, ease = inOut } of headPhases) {
    if (elapsed < duration) {
      const t = ease(elapsed / duration);
      return {
        closure: lerp(from.closure, pose.closure, t),
        sag: cycleElapsed < wakeStart ? sleepySag : lerp(from.sag, pose.sag, t),
        startle: lerp(from.startle, pose.startle, t),
      };
    }
    elapsed -= duration;
    from = pose;
  }
  return { closure: 0, sag: 0, startle: 0 };
}

function startledHeadAt(from: HeadPose, seconds: number): HeadPose {
  if (seconds < skeletonTiming.wake) {
    const t = out(seconds / skeletonTiming.wake);
    return { closure: lerp(from.closure, 0, t), sag: lerp(from.sag, -skeletonMotion.wakeLift, t),
      startle: lerp(from.startle, 1, t) };
  }
  if (seconds < skeletonTiming.wake + skeletonTiming.surprised)
    return { closure: 0, sag: -skeletonMotion.wakeLift, startle: 1 };
  const t = inOut((seconds - skeletonTiming.wake - skeletonTiming.surprised) / skeletonTiming.recover);
  return { closure: 0, sag: lerp(-skeletonMotion.wakeLift, 0, t), startle: 1 - t };
}

type Move = 'bounce' | 'spring';

/** A single wake stroke shared by the automatic cycle and clicks. */
function wakeTwitchAt(fromDepth: number, seconds: number) {
  const { wake, surprised, recover } = skeletonTiming;
  const overshoot = -crouchingSkeletonMotion.rhythm.overshoot;
  if (seconds < wake) return lerp(fromDepth, overshoot, out(seconds / wake));
  return lerp(overshoot, 0, inOut((seconds - wake) / (surprised + recover)));
}

/** Body motion on the same clock as the sleepy head. It coils only as sleep arrives, then startles forward. */
export function crouchingBodyAt(seconds: number): { depth: number; move: Move } {
  let elapsed = seconds % headCycleDuration;
  if (elapsed < bounceDuration) {
    for (const period of crouchingSkeletonMotion.rhythm.bouncePeriods) {
      if (elapsed < period) return { depth: (1 - Math.cos(2 * Math.PI * elapsed / period)) / 2, move: 'bounce' };
      elapsed -= period;
    }
  }
  if (seconds % headCycleDuration < wakeStart) {
    const coilAt = (seconds % headCycleDuration - bounceDuration) / skeletonTiming.closing;
    return { depth: inOut(coilAt), move: 'spring' };
  }
  return { depth: wakeTwitchAt(1, seconds % headCycleDuration - wakeStart), move: 'spring' };
}

// Groups the flex moves; their drawn placement is kept aside because setup may run twice under StrictMode.
const moving = [
  'torso', 'head', 'arm-controller', 'arm-controller-forearm',
  'leg-back-thigh', 'leg-back-shin', 'leg-back-foot', 'leg-front-thigh', 'leg-front-shin', 'leg-front-foot',
  'arm-support-upper', 'arm-support-forearm', 'hand-support',
];

// Every instance gets its own ID prefix so clip paths and <use> references never resolve into another copy.
let instances = 0;
function inlineSvg(prefix: string, rigged = true): { __html: string; rigged: boolean } {
  const doc = new DOMParser().parseFromString(source, 'image/svg+xml');
  const svg = doc.documentElement;
  svg.querySelectorAll('title, desc').forEach((node) => node.remove());
  svg.removeAttribute('aria-labelledby');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('focusable', 'false');
  if (rigged) {
    try {
      prepareEyes(svg, crouchingSkeletonMotion);
      for (const id of moving) {
        const node = svg.querySelector(`[id="${id}"]`)!;
        node.setAttribute('data-drawn-transform', node.getAttribute('transform') ?? '');
      }
    } catch (error) {
      // A re-export that lost a rig part shows the static drawing instead of taking down the page.
      console.error('Crouching skeleton: the drawing is missing rig parts; showing it static.', error);
      return inlineSvg(prefix, false);
    }
  }
  for (const node of [svg, ...svg.querySelectorAll('*')]) {
    for (const attr of [...node.attributes]) {
      if (attr.name === 'id') attr.value = prefix + attr.value;
      else if ((attr.name === 'href' || attr.name === 'xlink:href') && attr.value.startsWith('#')) attr.value = `#${prefix}${attr.value.slice(1)}`;
      else if (attr.value.includes('url(#')) attr.value = attr.value.replace(/url\(#/g, `url(#${prefix}`);
    }
  }
  return { __html: new XMLSerializer().serializeToString(svg), rigged };
}

const toString = (m: DOMMatrix) => `matrix(${[m.a, m.b, m.c, m.d, m.e, m.f].map((v) => +v.toFixed(4)).join(' ')})`;

function rig(root: SVGSVGElement, prefix: string) {
  const el = <T extends Element = SVGGraphicsElement>(id: string) => {
    const node = root.getElementById(prefix + id);
    if (!node) throw new Error(`The drawing has no "${id}" layer`);
    return node as T;
  };
  const rootInverse = root.getScreenCTM()!.inverse();
  // The matrix from an element's coordinates to the drawing's, and a guide's centre in any element's coordinates (the
  // guides may sit in any group, under any transform).
  const toRoot = (node: SVGGraphicsElement) => DOMMatrix.fromMatrix(rootInverse.multiply(node.getScreenCTM()!));
  const at = (id: string, space: SVGGraphicsElement): Point => {
    const guide = el(id);
    const p = new DOMPoint(Number(guide.getAttribute('cx')), Number(guide.getAttribute('cy')))
      .matrixTransform(space.getScreenCTM()!.inverse().multiply(guide.getScreenCTM()!));
    return [p.x, p.y];
  };
  const parent = (node: Element) => node.parentNode as SVGGraphicsElement;
  const placed = (id: string) => ({ node: el(id), drawn: el(id).getAttribute('data-drawn-transform') ?? '' });

  const head = el('head');
  // The eyes have separate guides for slowly closing in sleep and opening wide when startled.
  const found = findEyes(el, head), between = midpoint(found);
  const eyes = Object.values(found).map((eye) => ({
    ...eye,
    sleepyOpeningAt: morph(el(`guide-open-opening-${eye.side}`).getAttribute('d')!,
      el(`guide-closed-opening-${eye.side}`).getAttribute('d')!),
    sleepyBrowAt: morph(el(`guide-open-brow-${eye.side}`).getAttribute('d')!,
      el(`guide-closed-brow-${eye.side}`).getAttribute('d')!),
    wakeOpening: el(`guide-wake-opening-${eye.side}`).getAttribute('d')!,
    wakeBrow: el(`guide-wake-brow-${eye.side}`).getAttribute('d')!,
    drawnRadius: Number(eye.circle.getAttribute('r')),
  }));

  // Parts that ride on the body: each is given the body's movement, expressed in its parent's coordinates.
  const riders = ['torso', 'head', 'arm-controller'].map((id) => ({ ...placed(id), space: toRoot(parent(el(id))) }));
  // A limb from the body to a planted end: upper bone, lower bone and end, each nested in the one before, turning at
  // the base, middle and end pivots. The base follows the body; the end stays where it's drawn.
  const limb = (bones: [string, string, string], pivots: [string, string, string]) => {
    const [upper, lower, end] = bones.map(placed);
    const space = parent(upper.node), toDrawing = toRoot(space);
    const [base, middle, tip] = pivots.map((id) => at(id, space));
    return {
      upper, lower, end, toDrawing, base, middle, tip,
      middlePivot: at(pivots[1], parent(lower.node)), tipPivot: at(pivots[2], parent(end.node)),
      lengths: [Math.hypot(middle[0] - base[0], middle[1] - base[1]), Math.hypot(tip[0] - middle[0], tip[1] - middle[1])],
      // Which way the middle joint bends, so the solved limb never flips.
      bend: Math.sign((middle[0] - base[0]) * (tip[1] - base[1]) - (middle[1] - base[1]) * (tip[0] - base[0])),
    };
  };
  const limbs = [
    limb(['leg-back-thigh', 'leg-back-shin', 'leg-back-foot'], ['pivot-hip-back', 'pivot-knee-back', 'pivot-ankle-back']),
    limb(['leg-front-thigh', 'leg-front-shin', 'leg-front-foot'], ['pivot-hip-front', 'pivot-knee-front', 'pivot-ankle-front']),
    limb(['arm-support-upper', 'arm-support-forearm', 'hand-support'], ['pivot-shoulder-support', 'pivot-elbow-support', 'pivot-wrist-support']),
  ];
  const controllerArm = el('arm-controller');
  const forearm = placed('arm-controller-forearm');
  return {
    head, eyes, between, riders, limbs, controllerArm, forearm,
    pelvis: at('pivot-body', root), neck: at('pivot-neck', root),
    controllerShoulder: at('pivot-shoulder-controller', root),
    elbow: at('pivot-elbow-controller', parent(forearm.node)),
    reset: () => { for (const id of moving) { const { node, drawn } = placed(id); if (drawn) node.setAttribute('transform', drawn); else node.removeAttribute('transform'); } },
  };
}

type Limb = ReturnType<typeof rig>['limbs'][number];

/** Bends a limb so its base sits at `base` (limb coordinates) while its end stays planted, keeping its bend direction. */
function solve(limb: Limb, base: Point) {
  const [l1, l2] = limb.lengths, [tx, ty] = limb.tip;
  const reach = Math.hypot(tx - base[0], ty - base[1]);
  // Out of reach (or folded flat), the end slides a little rather than the limb breaking.
  const d = Math.min(Math.max(reach, Math.abs(l1 - l2) + 0.01), l1 + l2 - 0.01);
  const toTip = Math.atan2(ty - base[1], tx - base[0]);
  const spread = Math.acos((l1 * l1 + d * d - l2 * l2) / (2 * l1 * d));
  const direction = toTip - limb.bend * spread;
  const middle: Point = [base[0] + l1 * Math.cos(direction), base[1] + l1 * Math.sin(direction)];
  const upper = angle([middle[0] - base[0], middle[1] - base[1]]) - angle([limb.middle[0] - limb.base[0], limb.middle[1] - limb.base[1]]);
  const whole = angle([tx - middle[0], ty - middle[1]]) - angle([tx - limb.middle[0], ty - limb.middle[1]]);
  const [bx, by] = limb.base, [mx, my] = limb.middlePivot, [ex, ey] = limb.tipPivot;
  const fixed = (v: number) => v.toFixed(3);
  limb.upper.node.setAttribute('transform',
    `translate(${fixed(base[0] - bx)} ${fixed(base[1] - by)}) rotate(${fixed(upper)} ${bx} ${by}) ${limb.upper.drawn}`);
  limb.lower.node.setAttribute('transform', `rotate(${fixed(whole - upper)} ${mx} ${my}) ${limb.lower.drawn}`);
  // The end keeps its drawn angle, flat on the ground.
  limb.end.node.setAttribute('transform', `rotate(${fixed(-whole)} ${ex} ${ey}) ${limb.end.drawn}`);
}

/**
 * The crouching skeleton from the empty checkpoint history: it keeps its running bounce while its head dozes and
 * wakes. Clicking it startles the head awake and triggers a forward twitch. Holds still under reduced motion.
 */
export function CrouchingSkeleton({ sound = true, ...props }: HTMLAttributes<HTMLDivElement> & { sound?: boolean }) {
  const container = useRef<HTMLDivElement>(null);
  const soundOn = useRef(sound);
  soundOn.current = sound;
  const prefix = useMemo(() => `crouching${++instances}-`, []);
  const markup = useMemo(() => inlineSvg(prefix), [prefix]);

  // Layout effect: the first frame is drawn before the browser paints, so the raw SVG pose never flashes.
  useLayoutEffect(() => {
    if (typeof window.matchMedia !== 'function') return; // no media queries (e.g. jsdom): hold still
    const root = container.current!.querySelector('svg')!;
    if (!markup.rigged) return;
    let parts: ReturnType<typeof rig>;
    try {
      parts = rig(root, prefix);
    } catch (error) {
      console.error('Crouching skeleton: the drawing is missing rig parts; holding still.', error);
      return;
    }
    const { head, eyes, between, riders, limbs, controllerArm, forearm, pelvis, neck, controllerShoulder, elbow, reset } = parts;
    const motion = crouchingSkeletonMotion;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    let cycleClock = 0, wakeClock = -1;
    let headPose = crouchingHeadAt(0), wakeFrom = headPose;
    let bodyPose = crouchingBodyAt(0), wakeDepth = bodyPose.depth;
    let handDrop = 0, wakeHandDrop = 0;
    let frame = 0, last = 0, swing = 0, lastDown = 0;
    let audio: AudioContext | undefined;
    let pointer: Point | undefined;
    const gaze: Point = [...motion.ahead];

    const pointerDirection = (): Point => {
      if (!pointer) return motion.ahead;
      return pointerGaze(head, between, motion.gazeReach, pointer) ?? motion.ahead;
    };

    const draw = () => {
      const { depth, move } = bodyPose;
      const { back, down, tip } = motion.depth[move], { headKeep } = motion.depth;
      const turn = (m: DOMMatrix, degrees: number, [x, y]: Point) => m.translate(x, y).rotate(degrees).translate(-x, -y);
      const body = turn(new DOMMatrix().translate(-back * depth, down * depth), tip * depth, pelvis);
      const { degrees: shakeDegrees, frequency, duration } = skeletonMotion.clickShake;
      const shake = wakeClock >= 0 && wakeClock < duration
        ? shakeDegrees * (1 - wakeClock / duration) ** 2 * Math.sin(2 * Math.PI * frequency * wakeClock) : 0;
      for (const { node, drawn, space } of riders) {
        const headAngle = -tip * depth * headKeep + motion.sleepy.headTilt * headPose.sag + shake;
        const moved = node === head
          ? turn(body.translate(0, motion.sleepy.headDrop * headPose.sag), headAngle, neck)
          : node === controllerArm
            ? turn(body, motion.sleepy.armDroop * headPose.sag + handDrop, controllerShoulder)
            : body;
        node.setAttribute('transform', `${toString(space.inverse().multiply(moved).multiply(space))} ${drawn}`);
      }
      for (const limb of limbs) {
        const m = limb.toDrawing.inverse().multiply(body).multiply(limb.toDrawing);
        const p = new DOMPoint(...limb.base).matrixTransform(m);
        solve(limb, [p.x, p.y]);
      }
      forearm.node.setAttribute('transform', `rotate(${swing.toFixed(2)} ${elbow[0]} ${elbow[1]}) ${forearm.drawn}`);
      for (const eye of eyes) {
        const sleepyOpening = eye.sleepyOpeningAt(headPose.closure);
        const sleepyBrow = eye.sleepyBrowAt(headPose.closure);
        eye.setOpening(headPose.startle ? morph(sleepyOpening, eye.wakeOpening)(headPose.startle) : sleepyOpening);
        eye.brow.setAttribute('d', headPose.startle ? morph(sleepyBrow, eye.wakeBrow)(headPose.startle) : sleepyBrow);
        eye.look(gazeOffset(gaze[0], gaze[1], eye.limits));
        eye.circle.setAttribute('r', lerp(eye.drawnRadius, skeletonMotion.poses.wake.radius[eye.side], headPose.startle).toFixed(2));
        eye.circle.setAttribute('opacity', (1 - inOut((headPose.closure - 0.85) / 0.15)).toFixed(2));
        const lidDepth = lerp(motion.lidDepth, 50, headPose.closure);
        eye.lids.setAttribute('stroke-width', (2 * lidDepth).toFixed(2));
      }
    };

    const tick = (now: number) => {
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0;
      last = now;
      if (wakeClock >= 0) {
        wakeClock += dt;
        if (wakeClock >= wakeDuration) { wakeClock = -1; cycleClock = 0; }
        headPose = wakeClock >= 0 ? startledHeadAt(wakeFrom, wakeClock) : crouchingHeadAt(0);
        bodyPose = wakeClock >= 0 ? { depth: wakeTwitchAt(wakeDepth, wakeClock), move: 'spring' } : crouchingBodyAt(0);
        handDrop = wakeClock >= 0 ? wakeHandDrop * (1 - out(wakeClock / skeletonTiming.wake)) : 0;
      } else {
        cycleClock = (cycleClock + dt) % headCycleDuration;
        headPose = crouchingHeadAt(cycleClock);
        bodyPose = crouchingBodyAt(cycleClock);
        handDrop = crouchingHandDropAt(cycleClock);
      }
      const startled = headPose.startle > 0;
      const target = startled ? motion.ahead : pointerDirection();
      easeGaze(gaze, target, startled ? motion.dartRate : motion.gazeRate, dt);
      // The gamepad arm lags the body's bounce: it swings up while the body drops, and down as it springs back up.
      const downward = motion.depth[bodyPose.move].down * bodyPose.depth, speed = dt ? (downward - lastDown) / dt : 0;
      lastDown = downward;
      const swingTarget = Math.max(-motion.maxSwing, Math.min(motion.maxSwing, speed * motion.swing));
      swing += (swingTarget - swing) * (1 - Math.exp(-motion.swingRate * dt));
      draw();
      frame = requestAnimationFrame(tick);
    };
    const run = () => {
      if (frame || document.hidden || reduced.matches) return;
      last = 0;
      frame = requestAnimationFrame(tick);
    };
    const stop = () => { cancelAnimationFrame(frame); frame = 0; };

    const onMove = (event: PointerEvent) => { pointer = [event.clientX, event.clientY]; };
    const onClick = () => {
      if (soundOn.current && typeof AudioContext === 'function') {
        audio ??= new AudioContext();
        if (audio.state === 'suspended') audio.resume().catch(() => undefined);
        playSkeletonRattle(audio);
      }
      if (reduced.matches) return;
      wakeFrom = headPose;
      wakeDepth = bodyPose.depth;
      wakeHandDrop = handDrop;
      wakeClock = 0;
      bodyPose = { depth: wakeDepth, move: 'spring' };
      lastDown = motion.depth.spring.down * wakeDepth;
      draw();
    };
    // relatedTarget is null only when the pointer leaves the window itself, not the character or any other element.
    const onOut = (event: MouseEvent) => { if (!event.relatedTarget) pointer = undefined; };
    const onVisibility = () => document.hidden ? stop() : run();
    // Holding still is the drawn crouch, looking ahead.
    const showStatic = () => {
      stop(); cycleClock = 0; wakeClock = -1;
      headPose = crouchingHeadAt(0); bodyPose = crouchingBodyAt(0);
      handDrop = 0; swing = 0; lastDown = 0; [gaze[0], gaze[1]] = motion.ahead;
      draw();
    };
    const onReducedMotion = () => reduced.matches ? showStatic() : run();

    showStatic();
    run();
    addEventListener('pointermove', onMove);
    root.addEventListener('click', onClick);
    document.addEventListener('mouseout', onOut);
    document.addEventListener('visibilitychange', onVisibility);
    reduced.addEventListener('change', onReducedMotion);
    return () => {
      stop();
      reset();
      removeEventListener('pointermove', onMove);
      root.removeEventListener('click', onClick);
      document.removeEventListener('mouseout', onOut);
      document.removeEventListener('visibilitychange', onVisibility);
      reduced.removeEventListener('change', onReducedMotion);
      audio?.close().catch(() => undefined);
    };
  }, [prefix, markup]);

  return <div {...props} ref={container} dangerouslySetInnerHTML={markup} />;
}
