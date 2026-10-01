import { useLayoutEffect, useMemo, useRef, type HTMLAttributes } from 'react';
import source from '../public/character/no-checkpoints.svg?raw';
import { between as midpoint, findEyes, gazeOffset, morph, prepareEyes, type Point } from './skeletonEyes';

/*
 * The "no checkpoints" skeleton: crouched like a sprinter at the start line, gamepad in the back hand, the other hand
 * on the ground, getting ready to run.
 *
 * - It bounces on its legs, one smooth dip and rise every couple of seconds; every fourth move is a spring instead:
 *   the body sinks down and back, holds a beat, then springs forward past rest and settles. Feet and the grounded
 *   hand stay planted; the legs and the supporting arm bend to keep reaching them. The gamepad arm swings a little
 *   behind the body.
 * - At the bottom of the spring's dip it squints (lids drop, brows lower) and its eyes snap straight ahead, to the
 *   right, as if about to launch; it relaxes as it springs back.
 * - Otherwise the red eyes follow the mouse anywhere in the window, smoothly and within restrained bounds, and look
 *   ahead while the mouse is outside the window or before it first moves.
 * - Under reduced motion it holds still in the drawn crouch, looking ahead.
 *
 * The SVG (public/character/no-checkpoints.svg) is the single source of the artwork and its rig. The head holds the
 * eyes laid out as skeletonEyes.ts requires, and a 0%-opacity `head-rig` layer with the eyes' gaze guides and the squint
 * guides (each eye opening and brow as squinted, with the same path commands as the
 * drawn one). A 0%-opacity `body-rig` layer holds the joint pivots. This component shows an ID-prefixed copy of it (so
 * several can share a page) and animates that copy; the file itself stays a plain static drawing. The rig is only
 * named, transparent shapes, so it survives an Affinity Designer round trip; hidden layers, data attributes and
 * hand-written <defs> would not. Layer names are the IDs this component looks up. Timing and amplitudes live here.
 *
 * It scales to its container, pauses while the page is hidden, and cleans up on unmount. It shares only the eye rig
 * (skeletonEyes.ts) with the other skeletons.
 */

export const crouchingSkeletonMotion = {
  // The rhythm: `bounces` smooth bounces (a dip to full depth at mid-period and back up), then one spring. Each move
  // takes a period drawn from `period` (seconds), so it doesn't feel mechanical.
  rhythm: { bounces: 3, period: [2, 3] as [number, number] },
  // The spring, as shares of its period: sink (coil) to full depth, hold there, spring forward past rest, settle back.
  spring: { sink: 0.5, hold: 0.2, spring: 0.1, overshoot: 0.35 },
  // The body at full depth for each move, in drawing units: how far it moves back (negative x) and down, and how many
  // degrees it tips forward around the pelvis. The head turns back `headKeep` of that tip, so the eyes stay on target.
  depth: { bounce: { back: 10, down: 26, tip: 2 }, spring: { back: 28, down: 26, tip: 2.5 }, headKeep: 0.6 },
  // The squint comes with the spring only, as shares of its period: it builds before the bottom of the dip, holds
  // through the hold, and relaxes over the settle. `nod` is how many degrees the head dips toward the target.
  squint: { from: 0.38, full: 0.5, until: 0.8, gone: 0.95, nod: 3 },
  lidDepth: 18, // how far the dark lids reach in from the opening edge over the eyeballs
  squintLidDepth: 30, // the same, while squinting
  rimWidth: 16, // the dark rim around the round eyes
  gazeReach: 420, // head units from between the eyes at which the gaze saturates
  gazeRate: 7, // 1/s: how quickly the eyes ease toward their target
  dartRate: 30, // the same, while snapping ahead into the squint
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

type Move = 'bounce' | 'spring';

/** Where a move is, as the body's depth (0 = as drawn, 1 = full depth, negative = sprung forward) and its squint. */
function flexAt(move: Move, at: number) {
  if (move === 'bounce') return { depth: (1 - Math.cos(2 * Math.PI * at)) / 2, squint: 0 };
  const { sink, hold, spring, overshoot } = crouchingSkeletonMotion.spring;
  const s = crouchingSkeletonMotion.squint;
  const depth = at < sink ? inOut(at / sink)
    : at < sink + hold ? 1
      : at < sink + hold + spring ? lerp(1, -overshoot, out((at - sink - hold) / spring))
        : lerp(-overshoot, 0, inOut((at - sink - hold - spring) / (1 - sink - hold - spring)));
  const squint = at < s.from ? 0 : at < s.full ? out((at - s.from) / (s.full - s.from))
    : at < s.until ? 1 : 1 - inOut((at - s.until) / (s.gone - s.until));
  return { depth, squint };
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
  // The eyes, each with its squint: the drawn opening and brow morph into their squint guides.
  const found = findEyes(el, head), between = midpoint(found);
  const eyes = Object.values(found).map((eye) => ({
    ...eye,
    openingAt: morph(eye.drawn.opening, el(`guide-squint-opening-${eye.side}`).getAttribute('d')!),
    browAt: morph(eye.drawn.brow, el(`guide-squint-brow-${eye.side}`).getAttribute('d')!),
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
  const forearm = placed('arm-controller-forearm');
  return {
    head, eyes, between, riders, limbs, forearm,
    pelvis: at('pivot-body', root), neck: at('pivot-neck', root), elbow: at('pivot-elbow-controller', parent(forearm.node)),
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
 * The crouching skeleton from the empty checkpoint history: it flexes like a runner before the start and squints
 * ahead at the bottom of each dip; its eyes follow the mouse in between. Holds still under reduced motion.
 */
export function CrouchingSkeleton(props: HTMLAttributes<HTMLDivElement>) {
  const container = useRef<HTMLDivElement>(null);
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
    const { head, eyes, between, riders, limbs, forearm, pelvis, neck, elbow, reset } = parts;
    const motion = crouchingSkeletonMotion;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    const newPeriod = () => lerp(motion.rhythm.period[0], motion.rhythm.period[1], Math.random());
    // Seconds per move, how far into the current one (0..1), and how many moves so far.
    let period = newPeriod(), at = 0, moves = 0;
    const move = (): Move => moves % (motion.rhythm.bounces + 1) === motion.rhythm.bounces ? 'spring' : 'bounce';
    let frame = 0, last = 0, swing = 0, lastDown = 0;
    let pointer: Point | undefined;
    const gaze = [...motion.ahead];

    const pointerDirection = (): Point => {
      if (!pointer) return motion.ahead;
      const ctm = head.getScreenCTM();
      if (!ctm) return motion.ahead;
      const p = new DOMPoint(pointer[0], pointer[1]).matrixTransform(ctm.inverse());
      return [(p.x - between[0]) / motion.gazeReach, (p.y - between[1]) / motion.gazeReach];
    };

    const draw = (depth: number, squint: number) => {
      const { back, down, tip } = motion.depth[move()], { headKeep } = motion.depth;
      const turn = (m: DOMMatrix, degrees: number, [x, y]: Point) => m.translate(x, y).rotate(degrees).translate(-x, -y);
      const body = turn(new DOMMatrix().translate(-back * depth, down * depth), tip * depth, pelvis);
      for (const { node, drawn, space } of riders) {
        const moved = node === head ? turn(body, -tip * depth * headKeep + motion.squint.nod * squint, neck) : body;
        node.setAttribute('transform', `${toString(space.inverse().multiply(moved).multiply(space))} ${drawn}`);
      }
      for (const limb of limbs) {
        const m = limb.toDrawing.inverse().multiply(body).multiply(limb.toDrawing);
        const p = new DOMPoint(...limb.base).matrixTransform(m);
        solve(limb, [p.x, p.y]);
      }
      forearm.node.setAttribute('transform', `rotate(${swing.toFixed(2)} ${elbow[0]} ${elbow[1]}) ${forearm.drawn}`);
      for (const eye of eyes) {
        eye.setOpening(eye.openingAt(squint));
        eye.brow.setAttribute('d', eye.browAt(squint));
        eye.look(gazeOffset(gaze[0], gaze[1], eye.limits));
        eye.lids.setAttribute('stroke-width', (2 * lerp(motion.lidDepth, motion.squintLidDepth, squint)).toFixed(2));
      }
    };

    const tick = (now: number) => {
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0;
      last = now;
      at += dt / period;
      if (at >= 1) { at %= 1; period = newPeriod(); moves++; }
      const { depth, squint } = flexAt(move(), at);
      // While squinting the eyes snap ahead; otherwise they follow the pointer.
      const target = squint > 0 ? motion.ahead : pointerDirection();
      const length = Math.hypot(target[0], target[1]);
      const [u, v] = length > 1 ? [target[0] / length, target[1] / length] : target;
      const k = 1 - Math.exp(-(squint > 0 ? motion.dartRate : motion.gazeRate) * dt);
      gaze[0] += (u - gaze[0]) * k;
      gaze[1] += (v - gaze[1]) * k;
      // The gamepad arm lags the body's bounce: it swings up while the body drops, and down as it springs back up.
      const downward = motion.depth[move()].down * depth, speed = dt ? (downward - lastDown) / dt : 0;
      lastDown = downward;
      const swingTarget = Math.max(-motion.maxSwing, Math.min(motion.maxSwing, speed * motion.swing));
      swing += (swingTarget - swing) * (1 - Math.exp(-motion.swingRate * dt));
      draw(depth, squint);
      frame = requestAnimationFrame(tick);
    };
    const run = () => {
      if (frame || document.hidden || reduced.matches) return;
      last = 0;
      frame = requestAnimationFrame(tick);
    };
    const stop = () => { cancelAnimationFrame(frame); frame = 0; };

    const onMove = (event: PointerEvent) => { pointer = [event.clientX, event.clientY]; };
    // relatedTarget is null only when the pointer leaves the window itself, not the character or any other element.
    const onOut = (event: MouseEvent) => { if (!event.relatedTarget) pointer = undefined; };
    const onVisibility = () => document.hidden ? stop() : run();
    // Holding still is the drawn crouch, looking ahead.
    const showStatic = () => {
      stop(); at = 0; moves = 0; swing = 0; lastDown = 0; [gaze[0], gaze[1]] = motion.ahead;
      draw(0, 0);
    };
    const onReducedMotion = () => reduced.matches ? showStatic() : run();

    showStatic();
    run();
    addEventListener('pointermove', onMove);
    document.addEventListener('mouseout', onOut);
    document.addEventListener('visibilitychange', onVisibility);
    reduced.addEventListener('change', onReducedMotion);
    return () => {
      stop();
      reset();
      removeEventListener('pointermove', onMove);
      document.removeEventListener('mouseout', onOut);
      document.removeEventListener('visibilitychange', onVisibility);
      reduced.removeEventListener('change', onReducedMotion);
    };
  }, [prefix, markup]);

  return <div {...props} ref={container} dangerouslySetInnerHTML={markup} />;
}
