import { useEffect, useMemo, useRef, type HTMLAttributes } from 'react';
import source from '../public/character/no-games-found.svg?raw';

// Behaviour for the rigged skeleton in public/character/no-games-found.svg. The SVG stays a plain static drawing
// (its illustrated crescent gaze); this component inlines a copy and drives the rig: eyes follow the pointer, the lids
// slowly close, the skull wakes up surprised, and the cycle repeats. All geometry is read from the SVG's own guides.

/** Seconds per phase. Drowsiness is slow, waking is quick, the sleep and surprise holds are brief. */
export const skeletonTiming = {
  bored: 3.5, heavy: 3, almostClosed: 3, closing: 1.8, asleep: 1.4,
  wake: 0.18, surprised: 0.9, recover: 0.7, pointerExit: 0.6,
};

export const skeletonMotion = {
  gazeReach: 420, // head units from between the eyes at which the gaze saturates
  gazeRate: 7, // 1/s: how quickly the eyes ease toward their target
  wakeGazeRate: 40, // eyes snap to centre while surprised
  headGazeTilt: [0.8, 1.6], // degrees of head roll per unit of horizontal / upward gaze
  headSag: 3, // degrees the head droops when fully asleep
  wakeLift: 0.35, // share of headSag the head jerks up on waking
  awayGaze: [-1, 0] as [number, number], // gaze while the pointer is outside the window
};

type Side = 'left' | 'right';
type Pose = 'bored' | 'heavy' | 'almost-closed' | 'closed' | 'wake';
type Phase = {
  pose: Pose; duration: number; sag: number; track: boolean; ease: (t: number) => number; gazeRate?: number;
  follow?: number; // how much the head tilts after the gaze (default 1)
};
type Values = { closure: number; sag: number; follow: number; radius: Record<Side, number> };

const sides: Side[] = ['left', 'right'];
const inOut = (t: number) => 0.5 - Math.cos(Math.PI * t) / 2;
const out = (t: number) => 1 - (1 - t) ** 3;
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

function cycle(): Phase[] {
  const t = skeletonTiming, lift = -skeletonMotion.wakeLift;
  return [
    { pose: 'bored', duration: t.bored, sag: 0, track: true, ease: inOut },
    { pose: 'heavy', duration: t.heavy, sag: 0.45, track: true, ease: inOut },
    { pose: 'almost-closed', duration: t.almostClosed, sag: 0.8, track: true, ease: inOut },
    { pose: 'closed', duration: t.closing, sag: 1, track: true, ease: inOut },
    { pose: 'closed', duration: t.asleep, sag: 1, track: true, ease: inOut },
    { pose: 'wake', duration: t.wake, sag: lift, track: false, ease: out, gazeRate: skeletonMotion.wakeGazeRate },
    { pose: 'wake', duration: t.surprised, sag: lift, track: false, ease: inOut, gazeRate: skeletonMotion.wakeGazeRate },
    { pose: 'bored', duration: t.recover, sag: 0, track: true, ease: inOut },
  ];
}

/** Clamps a gaze direction to the unit disk, then maps it into the eye's asymmetric oval (up is shallower than down). */
export function gazeOffset(u: number, v: number, limits: { horizontal: number; up: number; down: number }) {
  const length = Math.hypot(u, v);
  if (length > 1) { u /= length; v /= length; }
  return [u * limits.horizontal, v * (v < 0 ? limits.up : limits.down)];
}

/** Interpolates two paths with identical command structure, number by number. */
function morph(from: string, to: string) {
  const a = from.match(/-?\d*\.?\d+/g)!.map(Number), b = to.match(/-?\d*\.?\d+/g)!.map(Number);
  if (a.length !== b.length) throw new Error('Eyelid guides do not correspond');
  return (t: number) => { let i = 0; return from.replace(/-?\d*\.?\d+/g, () => (+lerp(a[i], b[i++], t).toFixed(2)).toString()); };
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
    return [side, {
      opening, brow, illustrated: { opening: opening.getAttribute('d')!, brow: brow.getAttribute('d')! },
      openingAt: morph(el(`guide-open-opening-${side}`).getAttribute('d')!, el(`guide-closed-opening-${side}`).getAttribute('d')!),
      browAt: morph(el(`guide-open-brow-${side}`).getAttribute('d')!, el(`guide-closed-brow-${side}`).getAttribute('d')!),
      neutral: [num(`gaze-neutral-${side}`, 'cx'), num(`gaze-neutral-${side}`, 'cy')],
      limits: { horizontal: num(bounds, 'data-max-horizontal'), up: num(bounds, 'data-max-up'), down: num(bounds, 'data-max-down') },
      gaze: el(`iris-gaze-${side}`), circle: el(`iris-circle-${side}`),
      staticIris: el(`iris-static-${side}`), dynamicIris: el(`iris-dynamic-${side}`), lid: el(`eye-upper-lid-${side}`),
    }];
  })) as Record<Side, {
    opening: SVGElement; brow: SVGElement; illustrated: { opening: string; brow: string };
    openingAt: (t: number) => string; browAt: (t: number) => string; neutral: number[];
    limits: { horizontal: number; up: number; down: number };
    gaze: SVGElement; circle: SVGElement; staticIris: SVGElement; dynamicIris: SVGElement; lid: SVGElement;
  }>;
  const presets = Object.fromEntries((['bored', 'heavy', 'almost-closed', 'closed', 'wake'] as Pose[]).map((name) => {
    const preset = el(`pose-${name}`);
    const value = (attr: string) => Number(preset.getAttribute(attr));
    return [name, { closure: value('data-closure-progress'), sag: 0, follow: 1, radius: { left: value('data-iris-radius-left'), right: value('data-iris-radius-right') } }];
  })) as Record<Pose, Values>;
  const pose = (name: Pose) => presets[name];
  const head = el<SVGGraphicsElement>('head-pose');
  const pivot = head.getAttribute('data-pivot')!.split(/\s+/).map(Number);
  const between = [(eyes.left.neutral[0] + eyes.right.neutral[0]) / 2, (eyes.left.neutral[1] + eyes.right.neutral[1]) / 2];
  return { eyes, pose, head, pivot, between };
}

/**
 * The bored skeleton from the empty library: eyes follow the pointer while it slowly nods off, then startles awake.
 * Clicking it startles it awake immediately.
 * Shows the original illustrated artwork until the pointer first moves, and permanently under reduced motion.
 */
export function SleepySkeleton(props: HTMLAttributes<HTMLDivElement>) {
  const container = useRef<HTMLDivElement>(null);
  const prefix = useMemo(() => `skeleton${++instances}-`, []);
  const markup = useMemo(() => ({ __html: inlineSvg(prefix) }), [prefix]);

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return; // no media queries (e.g. jsdom): keep the static artwork
    const root = container.current!.querySelector('svg')!;
    const { eyes, pose, head, pivot, between } = rig(root, prefix);
    const phases = cycle();
    const motion = skeletonMotion;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');

    let mode: 'static' | 'cycle' | 'away' = 'static';
    let index = 0, elapsed = 0, frame = 0, last = 0;
    let from: Values = pose('bored'), current: Values = pose('bored');
    let pointer: [number, number] | undefined;
    const gaze = [0, 0];

    const showDynamic = (dynamic: boolean) => {
      for (const side of sides) {
        const eye = eyes[side];
        eye.staticIris.style.display = dynamic ? 'none' : '';
        eye.dynamicIris.style.display = eye.lid.style.display = dynamic ? '' : 'none';
        if (!dynamic) { eye.opening.setAttribute('d', eye.illustrated.opening); eye.brow.setAttribute('d', eye.illustrated.brow); }
      }
      if (!dynamic) head.removeAttribute('transform');
    };
    const enter = (next: typeof mode, phase = 0) => {
      mode = next; index = phase; elapsed = 0; from = current;
    };
    const phase = (): Phase => mode === 'away'
      ? { pose: 'bored', duration: skeletonTiming.pointerExit, sag: 0, follow: 0, track: false, ease: inOut }
      : phases[index];

    const pointerDirection = () => {
      if (!pointer) return [0, 0];
      const ctm = head.getScreenCTM();
      if (!ctm) return [0, 0];
      const p = new DOMPoint(pointer[0], pointer[1]).matrixTransform(ctm.inverse());
      return [(p.x - between[0]) / motion.gazeReach, (p.y - between[1]) / motion.gazeReach];
    };

    const draw = () => {
      const tilt = gaze[0] * motion.headGazeTilt[0] - Math.min(gaze[1], 0) * motion.headGazeTilt[1];
      const angle = -current.sag * motion.headSag + current.follow * tilt;
      head.setAttribute('transform', `rotate(${angle.toFixed(3)} ${pivot[0]} ${pivot[1]})`);
      for (const side of sides) {
        const eye = eyes[side];
        const [dx, dy] = gazeOffset(gaze[0], gaze[1], eye.limits);
        eye.opening.setAttribute('d', eye.openingAt(current.closure));
        eye.brow.setAttribute('d', eye.browAt(current.closure));
        eye.circle.setAttribute('r', current.radius[side].toFixed(2));
        eye.gaze.setAttribute('transform', `translate(${dx.toFixed(2)} ${dy.toFixed(2)})`);
      }
    };

    const tick = (now: number) => {
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0;
      last = now;
      let step = phase();
      elapsed += dt;
      while (mode === 'cycle' && elapsed >= step.duration) {
        elapsed -= step.duration; from = { ...pose(step.pose), sag: step.sag, follow: step.follow ?? 1 };
        index = (index + 1) % phases.length; step = phase();
      }
      const t = step.ease(Math.min(elapsed / step.duration, 1));
      const to = pose(step.pose);
      current = {
        closure: lerp(from.closure, to.closure, t), sag: lerp(from.sag, step.sag, t), follow: lerp(from.follow, step.follow ?? 1, t),
        radius: { left: lerp(from.radius.left, to.radius.left, t), right: lerp(from.radius.right, to.radius.right, t) },
      };
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
      if (mode === 'static') { showDynamic(true); current = pose('bored'); enter('cycle'); draw(); }
      else if (mode === 'away') enter('cycle');
      run();
    };
    // relatedTarget is null only when the pointer leaves the window itself, not the character or any other element.
    const onOut = (event: MouseEvent) => {
      if (event.relatedTarget || mode === 'static') return;
      pointer = undefined;
      enter('away');
      run();
    };
    // A click startles the skeleton awake from wherever it is in the cycle.
    const onClick = (event: MouseEvent) => {
      if (reduced.matches) return;
      onMove(event as PointerEvent);
      enter('cycle', phases.findIndex((step) => step.pose === 'wake'));
    };
    const onVisibility = () => document.hidden ? stop() : run();
    const onReducedMotion = () => {
      if (!reduced.matches) return;
      stop(); mode = 'static'; showDynamic(false);
    };

    showDynamic(false);
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
    };
  }, [prefix]);

  return <div {...props} ref={container} dangerouslySetInnerHTML={markup} />;
}
