import { useLayoutEffect, useMemo, useRef, type HTMLAttributes } from 'react';
import source from '../public/character/no-game-selected.svg?raw';

/*
 * The "no game selected" skeleton: it points toward the game list, gamepad in the other hand, and eyes the person using
 * the page, its red eyes following the mouse.
 *
 * - Its usual look is fairly neutral: thin dark lids over the eyes and the brows slightly raised.
 * - The eyes are round red eyeballs placed where the drawing's red sits (a big, low left eye that peeks over its lower
 *   lid; a big right eye), and they follow the mouse anywhere in the window, smoothly and within restrained bounds.
 * - When the mouse leaves the window, and before it first moves, the eyes glance left, toward the game list. Under
 *   reduced motion they stay there.
 * - When the eyes finish moving, it jabs toward the game list: the pointing arm pulls back a little, then thrusts
 *   forward, the upper arm, forearm and hand turning together at the shoulder, elbow and wrist. On the thrust the eyes
 *   dart left to the list and it squints like the original drawing (deep lids, lowered brows); it holds that glare for
 *   a beat after the arm settles, then relaxes and looks back at the mouse. Not more often than every couple of
 *   seconds, so a restless mouse doesn't turn it into constant jabbing.
 * - With each jab the other hand offers the gamepad, inviting to play: on the thrust it lifts it a little at the elbow
 *   and pushes it toward the viewer with a few quick shakes that fade out by the end of the glare, then settles back.
 * - Nothing else moves: head and body stay as drawn.
 *
 * The SVG (public/character/no-game-selected.svg) is the single source of the artwork and its rig: round eyes at 0%
 * opacity inside each eye's clipped group, and a 0%-opacity `rig` layer with the arm's pivot markers and an oval per
 * eye for how far it may look. This component shows an ID-prefixed copy of it (so several can share a page) and
 * animates that copy; the file itself stays a plain static drawing with the crescent eyes. The rig is only named,
 * transparent shapes, so it survives an Affinity Designer round trip; hidden layers, data attributes and hand-written
 * <defs> would not. Layer names are the IDs this component looks up.
 *
 * It scales to its container, only animates while the eyes are catching up with the mouse or a jab is playing, pauses
 * while the page is hidden, and cleans up on unmount. It shares no code with the sleepy skeleton.
 */

export const pointingSkeletonMotion = {
  // The usual look and the squint: where the eyeballs rest (head coordinates), how far the dark lids reach in from the
  // eye-opening edge over them, and how far the brows shift down (negative: up) from where they're drawn. The squint
  // matches the red of the original drawing; in both looks the big left eye sits low in its socket, so only its top
  // shows and the two eyes read as one gaze. The lids keep a dark band between the red and the bone.
  expression: {
    neutral: { eyes: { left: { center: [530, 645], radius: 60 }, right: { center: [806, 506], radius: 62 } }, lidDepth: 20, brow: -8 },
    squint: { eyes: { left: { center: [530, 645], radius: 60 }, right: { center: [806, 506], radius: 62 } }, lidDepth: 36, brow: 6 },
  },
  // How far the eyes travel, as multiples of the SVG's gaze bounds (24 sideways, 12 up, ~85-105 down). The eyes can't
  // leave the skull (the eye openings clip them); these keep a clear part of each red eye showing at every extreme.
  // Downward is the tightest: the low left eye soon slips under its lower lid.
  reach: { horizontal: 2, up: 3, down: 0.3 },
  gazeReach: 420, // head units from between the eyes at which the gaze saturates
  gazeRate: 7, // 1/s: how quickly the eyes ease toward their target
  restingGaze: [-1, 0] as [number, number], // before the mouse moves, and while it's outside the window
  // The jab, as [shoulder, elbow, wrist] turns in degrees: pull back, thrust past rest, settle; then the glare holds and
  // relaxes. `timing` is seconds per step (pull, thrust, settle, hold, relax); the eyes dart left at `dartRate` from the
  // thrust until the hold ends. `cooldown` is the least time between jabs.
  jab: {
    retract: [-8, 20, 10], thrust: [6, -12, -8], timing: [0.28, 0.12, 0.25, 0.35, 0.45], dartRate: 30, cooldown: 2.5,
  },
  // The gamepad offer, played with each jab: from the thrust the forearm lifts `lift` degrees at the elbow, the hand
  // pushes the gamepad toward the viewer (it grows by `grow` around the wrist) and shakes it `shakes` times a second
  // by up to `shake` degrees, fading out by the end of the hold; everything settles back as the glare relaxes.
  offer: { lift: -5, grow: 0.07, shake: 5, shakes: 7 },
};

// The gamepad hand's groups the offer moves; their drawn placement is kept aside because setup may run twice under
// StrictMode.
const offering = ['controller-forearm', 'controller-palm', 'gamepad', 'controller-fingers'];

type Side = 'left' | 'right';
const sides: Side[] = ['left', 'right'];
const inOut = (t: number) => 0.5 - Math.cos(Math.PI * t) / 2;
const out = (t: number) => 1 - (1 - t) ** 3;

/** Clamps a gaze direction to the unit disk, then maps it into the eye's asymmetric oval (up is shallower than down). */
function gazeOffset(u: number, v: number, limits: { horizontal: number; up: number; down: number }) {
  const length = Math.hypot(u, v);
  if (length > 1) { u /= length; v /= length; }
  return [u * limits.horizontal, v * (v < 0 ? limits.up : limits.down)];
}

// Every instance gets its own ID prefix so clip paths and <use> references never resolve into another copy.
let instances = 0;
function inlineSvg(prefix: string, rigged = true): { __html: string; rigged: boolean } {
  const doc = new DOMParser().parseFromString(source, 'image/svg+xml');
  const svg = doc.documentElement;
  svg.querySelectorAll('title, desc').forEach((node) => node.remove());
  svg.removeAttribute('aria-labelledby');
  svg.removeAttribute('role');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('focusable', 'false');
  // Fill the container, whatever size the file itself declares.
  svg.setAttribute('width', '100%');
  svg.setAttribute('height', '100%');
  if (rigged) {
    try {
      const { expression } = pointingSkeletonMotion;
      for (const side of sides) {
        const part = (id: string) => svg.querySelector<SVGElement>(`[id="${id}-${side}"]`)!;
        // Round eyeballs instead of the drawn crescents, resting where the crescents were. Lids (the socket's outline,
        // drawn inside the eye's clip so it only reaches inward) set how open the eyes look.
        part('iris-static').remove();
        const dynamic = part('iris-dynamic');
        dynamic.removeAttribute('opacity');
        const circle = part('iris-circle'), { center: [cx, cy], radius } = expression.neutral.eyes[side];
        circle.setAttribute('cx', String(cx));
        circle.setAttribute('cy', String(cy));
        circle.setAttribute('r', String(radius));
        const socket = part('eye-socket');
        const lids = doc.createElementNS('http://www.w3.org/2000/svg', 'path');
        lids.setAttribute('id', `eye-lids-${side}`);
        lids.setAttribute('d', socket.getAttribute('d')!);
        lids.setAttribute('fill', 'none');
        lids.setAttribute('stroke', socket.style.fill);
        lids.setAttribute('stroke-width', String(2 * expression.neutral.lidDepth));
        lids.setAttribute('stroke-linejoin', 'round');
        dynamic.after(lids);
      }
      for (const id of offering) {
        const node = svg.querySelector(`[id="${id}"]`)!;
        node.setAttribute('data-drawn-transform', node.getAttribute('transform') ?? '');
      }
    } catch (error) {
      // A re-export that lost a rig part shows the static drawing instead of taking down the page.
      console.error('Pointing skeleton: the drawing is missing rig parts; showing it static.', error);
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

function rig(root: SVGSVGElement, prefix: string) {
  const el = <T extends Element = SVGElement>(id: string) => {
    const node = root.getElementById(prefix + id);
    if (!node) throw new Error(`The drawing has no "${id}" layer`);
    return node as T;
  };
  const num = (id: string, attr: string) => Number(el(id).getAttribute(attr));
  // A point on a guide shape, in the coordinates of `space` (the guides may sit in any group, under any transform).
  const at = (id: string, space: SVGGraphicsElement, dx = 0, dy = 0) => {
    const matrix = space.getScreenCTM()!.inverse().multiply(el<SVGGraphicsElement>(id).getScreenCTM()!);
    const p = new DOMPoint(num(id, 'cx') + dx, num(id, 'cy') + dy).matrixTransform(matrix);
    return [p.x, p.y];
  };
  const { reach } = pointingSkeletonMotion;
  // The eyes' parent groups sit untransformed in the head, so their space is head coordinates. Each gaze oval spans
  // how far the eye may travel: its width sideways, its top and bottom up and down from the resting point.
  const headSpace = el<SVGGraphicsElement>('iris-dynamic-left');
  const eyes = sides.map((side) => {
    const bounds = `gaze-bounds-${side}`, rx = num(bounds, 'rx'), ry = num(bounds, 'ry');
    const neutral = at(`gaze-neutral-${side}`, headSpace);
    const [left, right, top, bottom] = [at(bounds, headSpace, -rx), at(bounds, headSpace, rx), at(bounds, headSpace, 0, -ry), at(bounds, headSpace, 0, ry)];
    return {
      side, gaze: el(`iris-gaze-${side}`), circle: el(`iris-circle-${side}`), lids: el(`eye-lids-${side}`), brow: el(`brow-${side}`),
      limits: {
        horizontal: (right[0] - left[0]) / 2 * reach.horizontal, up: (neutral[1] - top[1]) * reach.up,
        down: (bottom[1] - neutral[1]) * reach.down,
      },
      neutral,
    };
  });
  const between = [(eyes[0].neutral[0] + eyes[1].neutral[0]) / 2, (eyes[0].neutral[1] + eyes[1].neutral[1]) / 2];
  // The pointing arm's chain, each bone turning at its pivot marker: shoulder, elbow, wrist.
  const joint = (id: string, pivot: string) => {
    const bone = el<SVGGraphicsElement>(id);
    return { bone, pivot: at(pivot, bone.parentNode as SVGGraphicsElement).map((v) => v.toFixed(2)).join(' ') };
  };
  const arm = [
    joint('upper-arm-left', 'pivot-shoulder'), joint('forearm-left', 'pivot-elbow'),
    joint('hand-left', 'pivot-wrist'),
  ];
  // The gamepad hand: the forearm turns at the elbow, and the palm, gamepad and fingers turn with it and around the
  // wrist. They all share one parent, so the pivots are measured there.
  const placed = (id: string) => ({ node: el(id), drawn: el(id).getAttribute('data-drawn-transform') ?? '' });
  const [forearm, ...hand] = offering.map(placed), space = forearm.node.parentNode as SVGGraphicsElement;
  const pivot = (id: string) => at(id, space).map((v) => +v.toFixed(2));
  const offer = { forearm, hand, elbow: pivot('pivot-elbow-controller'), wrist: pivot('pivot-wrist-controller') };
  return { eyes, headSpace, between, arm, offer };
}

/**
 * The pointing skeleton from the empty game selection: its eyes follow the mouse, and it jabs toward the game list with
 * a squinting glare. Holds its gaze to the left, toward the game list, until the pointer first moves, and permanently
 * under reduced motion.
 */
export function PointingSkeleton(props: HTMLAttributes<HTMLDivElement>) {
  const container = useRef<HTMLDivElement>(null);
  const prefix = useMemo(() => `pointing${++instances}-`, []);
  const markup = useMemo(() => inlineSvg(prefix), [prefix]);

  // Layout effect: the resting gaze is drawn before the browser paints, so the eyes never jump into place.
  useLayoutEffect(() => {
    if (typeof window.matchMedia !== 'function') return; // no media queries (e.g. jsdom): hold still
    const root = container.current!.querySelector('svg')!;
    if (!markup.rigged) return;
    let parts: ReturnType<typeof rig>;
    try {
      parts = rig(root, prefix);
    } catch (error) {
      console.error('Pointing skeleton: the drawing is missing rig parts; holding still.', error);
      return;
    }
    const { eyes, headSpace, between, arm, offer } = parts;
    const motion = pointingSkeletonMotion;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    const gaze = [...motion.restingGaze];
    let target = [...motion.restingGaze];
    let frame = 0, last = 0;
    let looked = false; // the eyes had somewhere new to look since the last jab
    let jab = -1, jabbed = -Infinity; // seconds into the current jab (-1: none), and when the last one started
    let squint = 0; // 0 = usual look, 1 = the drawing's squint

    // Each bone carries the turns of the joints above it, so the chain stays connected.
    const drawArm = (turns: number[]) => {
      let chain = '';
      arm.forEach(({ bone, pivot }, i) => {
        chain += ` rotate(${turns[i].toFixed(2)} ${pivot})`;
        bone.setAttribute('transform', chain.trim());
      });
    };
    // How far the gamepad is offered (0..1) and its shake in degrees, at a point in the jab.
    const offerPose = (at: number) => {
      const [pull, push, settle, hold, relax] = motion.jab.timing, since = at - pull;
      if (since < 0) return { reach: 0, shake: 0 };
      const reach = since < push ? out(since / push)
        : since < push + settle + hold ? 1 : 1 - inOut(Math.min((since - push - settle - hold) / relax, 1));
      const fade = Math.max(0, 1 - since / (push + settle + hold));
      return { reach, shake: motion.offer.shake * fade * Math.sin(2 * Math.PI * motion.offer.shakes * since) };
    };
    const drawOffer = ({ reach, shake }: { reach: number; shake: number }) => {
      const { lift, grow } = motion.offer, [ex, ey] = offer.elbow, [wx, wy] = offer.wrist;
      const turn = `rotate(${(lift * reach).toFixed(2)} ${ex} ${ey})`;
      offer.forearm.node.setAttribute('transform', `${turn} ${offer.forearm.drawn}`);
      const toward = `rotate(${shake.toFixed(2)} ${wx} ${wy}) translate(${wx} ${wy}) scale(${(1 + grow * reach).toFixed(4)}) translate(${-wx} ${-wy})`;
      for (const { node, drawn } of offer.hand) node.setAttribute('transform', `${turn} ${toward} ${drawn}`);
    };
    const jabPose = (at: number) => {
      const { retract, thrust, timing: [pull, push, settle] } = motion.jab;
      if (at >= pull + push + settle) return [0, 0, 0];
      const blend = (a: number[], b: number[], t: number) => a.map((v, i) => v + (b[i] - v) * t);
      if (at < pull) return blend([0, 0, 0], retract, inOut(at / pull));
      if (at < pull + push) return blend(retract, thrust, out((at - pull) / push));
      return blend(thrust, [0, 0, 0], inOut(Math.min((at - pull - push) / settle, 1)));
    };
    // The squint builds on the thrust, holds through the settle and a beat after, then relaxes; the eyes dart left over
    // the same stretch until the hold ends.
    const jabSquint = (at: number) => {
      const [pull, push, settle, hold, relax] = motion.jab.timing;
      if (at < pull) return 0;
      if (at < pull + push) return out((at - pull) / push);
      if (at < pull + push + settle + hold) return 1;
      return 1 - inOut(Math.min((at - pull - push - settle - hold) / relax, 1));
    };
    const darting = (at: number) => {
      const [pull, push, settle, hold] = motion.jab.timing;
      return at >= pull && at < pull + push + settle + hold;
    };
    const draw = () => {
      const { neutral, squint: glare } = motion.expression;
      const lidDepth = neutral.lidDepth + (glare.lidDepth - neutral.lidDepth) * squint;
      const brow = neutral.brow + (glare.brow - neutral.brow) * squint;
      const mix = (a: number, b: number) => (a + (b - a) * squint).toFixed(2);
      for (const eye of eyes) {
        const [dx, dy] = gazeOffset(gaze[0], gaze[1], eye.limits);
        const usual = neutral.eyes[eye.side], glaring = glare.eyes[eye.side];
        eye.circle.setAttribute('cx', mix(usual.center[0], glaring.center[0]));
        eye.circle.setAttribute('cy', mix(usual.center[1], glaring.center[1]));
        eye.circle.setAttribute('r', mix(usual.radius, glaring.radius));
        eye.gaze.setAttribute('transform', `translate(${dx.toFixed(2)} ${dy.toFixed(2)})`);
        eye.lids.setAttribute('stroke-width', (2 * lidDepth).toFixed(2));
        eye.brow.setAttribute('transform', `translate(0 ${brow.toFixed(2)})`);
      }
    };
    const aim = (x: number, y: number) => {
      const ctm = headSpace.getScreenCTM();
      if (!ctm) return;
      const p = new DOMPoint(x, y).matrixTransform(ctm.inverse());
      const u = (p.x - between[0]) / motion.gazeReach, v = (p.y - between[1]) / motion.gazeReach;
      const length = Math.hypot(u, v);
      target = length > 1 ? [u / length, v / length] : [u, v];
    };
    // Eases toward the target and stops once the eyes have arrived, so an idle pointer costs nothing.
    const tick = (now: number) => {
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0;
      last = now;
      const dart = jab >= 0 && darting(jab);
      const aimAt = dart ? motion.restingGaze : target;
      const k = 1 - Math.exp(-(dart ? motion.jab.dartRate : motion.gazeRate) * dt);
      gaze[0] += (aimAt[0] - gaze[0]) * k;
      gaze[1] += (aimAt[1] - gaze[1]) * k;
      squint = jab >= 0 ? jabSquint(jab) : 0;
      draw();
      // Once the eyes are nearly where they're headed, jab toward the game list (unless the last jab was too recent).
      const distance = Math.hypot(aimAt[0] - gaze[0], aimAt[1] - gaze[1]), arrived = distance < 0.001;
      if (distance < 0.05 && looked && jab < 0 && now / 1000 - jabbed >= motion.jab.cooldown) {
        looked = false;
        jab = 0;
        jabbed = now / 1000;
      }
      if (jab >= 0) {
        jab += dt;
        const done = jab >= motion.jab.timing.reduce((a, b) => a + b);
        drawArm(jabPose(jab));
        drawOffer(offerPose(jab));
        if (done) { jab = -1; squint = 0; draw(); }
      }
      frame = arrived && jab < 0 ? 0 : requestAnimationFrame(tick);
    };
    const run = () => {
      if (frame || document.hidden || reduced.matches) return;
      last = 0;
      frame = requestAnimationFrame(tick);
    };
    const stop = () => { cancelAnimationFrame(frame); frame = 0; };

    const onMove = (event: PointerEvent) => { aim(event.clientX, event.clientY); looked = true; run(); };
    // relatedTarget is null only when the pointer leaves the window itself, not the skeleton or any other element.
    const onOut = (event: MouseEvent) => {
      if (event.relatedTarget) return;
      target = [...motion.restingGaze];
      looked = true;
      run();
    };
    const onVisibility = () => document.hidden ? stop() : run();
    const onReducedMotion = () => {
      if (!reduced.matches) return;
      stop();
      [gaze[0], gaze[1]] = target = [...motion.restingGaze];
      jab = -1;
      squint = 0;
      draw();
      drawArm([0, 0, 0]);
      drawOffer({ reach: 0, shake: 0 });
    };

    draw();
    addEventListener('pointermove', onMove);
    document.addEventListener('mouseout', onOut);
    document.addEventListener('visibilitychange', onVisibility);
    reduced.addEventListener('change', onReducedMotion);
    return () => {
      stop();
      removeEventListener('pointermove', onMove);
      document.removeEventListener('mouseout', onOut);
      document.removeEventListener('visibilitychange', onVisibility);
      reduced.removeEventListener('change', onReducedMotion);
    };
  }, [prefix, markup]);

  return <div {...props} ref={container} dangerouslySetInnerHTML={markup} />;
}
