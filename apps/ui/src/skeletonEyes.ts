/*
 * The red eyes every empty-state skeleton shares. Each character stays its own component with its own motion; this
 * module turns a drawing's eyes into a rig, finds them again for animation, and provides their shared pointer and
 * gaze motion. `eyeProblems` checks a drawing against these requirements; each character's tests run it.
 *
 * What the drawing must have, for each side (`left` and `right`), as layers named exactly so in Affinity Designer
 * (the layer name becomes the ID; a name that isn't a valid ID gets mangled, so no spaces):
 *
 *   eye-socket-<side>        <path>, filled with the dark ink (inline `fill` style): the eye opening.
 *   eye-contents-<side>      <g>, untransformed, a sibling of the socket under the same parent, so the clip and lids
 *                            drawn inside it share the socket's coordinates. Whatever clip Affinity exports inside
 *                            it is replaced by the rig's own.
 *     iris-static-<side>       <g> inside eye-contents: the drawn crescent. Removed when rigged; the static file shows it.
 *     iris-dynamic-<side>      <g> inside eye-contents, untransformed, at 0% opacity in the file. Shown when rigged.
 *       iris-gaze-<side>         <g> inside iris-dynamic, untransformed: the rig moves it to look around.
 *         iris-circle-<side>       <circle> inside iris-gaze: the round red eyeball.
 *   brow-<side>              <path>: the brow, which a character may reshape or move.
 *   gaze-bounds-<side>       <ellipse> (any group, any transform, typically a 0%-opacity rig layer): how far the
 *                            eyeball may travel, its width sideways and its top and bottom up and down.
 *   gaze-neutral-<side>      <circle> or <ellipse> next to it: where the eyeball rests, looking straight ahead.
 *
 * A character may add its own guides (other shapes of the socket or brow, such as a squint or closed lids). They must
 * use the same path commands as the shape they morph from, so `morph` can blend them number by number.
 *
 * Hidden layers, data attributes, <use> references and hand-written <defs> don't survive an Affinity export, so the
 * drawing holds none of them. The rig built here also avoids <use>: WebKitGTK (the Linux webview) drops a clip
 * built from a <use>, and the red eyeballs vanish. Every copy of the opening is a plain path instead.
 *
 * What the rig adds, per side, inside the drawing's ID prefix:
 *   eye-clip-<side>          <clipPath> right before eye-contents, holding eye-clip-shape-<side>: a copy of the
 *                            opening that clips the eyeball.
 *   eye-rim-<side>           <path> right after eye-contents: the opening's outline as a dark rim over the eyeball's
 *                            edge, `rimWidth` wide (0 hides it).
 *   eye-lids-<side>          <path> last inside eye-contents, so it's clipped too: the opening's outline as dark
 *                            lids reaching `lidDepth` in from the edge over the eyeball (the stroke is twice that,
 *                            half of it falls outside the clip).
 * The socket and brow keep their drawn shapes in `data-drawn`, because setup may run twice under StrictMode.
 */

export type Side = 'left' | 'right';
export type Point = [number, number];
export const sides: Side[] = ['left', 'right'];

type Requirement = { tag: string; inside?: string; sibling?: string; attributes?: string[] };
const requirements: Record<string, Requirement> = {
  'eye-socket': { tag: 'path', attributes: ['d'] },
  'eye-contents': { tag: 'g', sibling: 'eye-socket' },
  'iris-static': { tag: 'g', inside: 'eye-contents' },
  'iris-dynamic': { tag: 'g', inside: 'eye-contents' },
  'iris-gaze': { tag: 'g', inside: 'iris-dynamic' },
  'iris-circle': { tag: 'circle', inside: 'iris-gaze' },
  'brow': { tag: 'path', attributes: ['d'] },
  'gaze-bounds': { tag: 'ellipse', attributes: ['cx', 'cy', 'rx', 'ry'] },
  'gaze-neutral': { tag: 'circle|ellipse', attributes: ['cx', 'cy'] },
};

/** Every eye layer a drawing must have, for both sides. */
export const eyeLayers = sides.flatMap((side) => Object.keys(requirements).map((part) => `${part}-${side}`));

/** What keeps a drawing's eyes from meeting the requirements above, one line each; empty when they meet them. */
export function eyeProblems(svg: Element): string[] {
  const problems: string[] = [];
  for (const side of sides) {
    const find = (part: string) => svg.querySelector(`[id="${part}-${side}"]`);
    for (const [part, { tag, inside, sibling, attributes = [] }] of Object.entries(requirements)) {
      const id = `${part}-${side}`, node = find(part);
      if (!node) { problems.push(`${id}: missing`); continue; }
      if (!tag.split('|').includes(node.localName)) problems.push(`${id}: a <${node.localName}>, not a <${tag.replace('|', '> or <')}>`);
      if (inside && find(inside) && !find(inside)!.contains(node.parentNode)) problems.push(`${id}: not inside ${inside}-${side}`);
      if (sibling && find(sibling) && find(sibling)!.parentNode !== node.parentNode) problems.push(`${id}: not beside ${sibling}-${side}`);
      for (const attribute of attributes) if (!node.hasAttribute(attribute)) problems.push(`${id}: no ${attribute}`);
    }
    // Moving a group in Affinity exports a transform on it, which would shift the clip and lids off the socket.
    for (const part of ['eye-contents', 'iris-dynamic', 'iris-gaze']) {
      if (find(part)?.hasAttribute('transform')) problems.push(`${part}-${side}: transformed`);
    }
    if (find('eye-socket') && !(find('eye-socket') as SVGElement).style?.fill) problems.push(`eye-socket-${side}: no fill`);
  }
  return problems;
}

/**
 * Rigs the eyes of a parsed drawing, before it's ID-prefixed and shown: round eyeballs in place of the crescents, and
 * the eye opening copied into a clip, a rim and lids that follow it. Throws if the drawing doesn't meet the
 * requirements, so the caller can show it static instead.
 */
export function prepareEyes(svg: Element, { rimWidth, lidDepth }: { rimWidth: number; lidDepth: number }) {
  const problems = eyeProblems(svg);
  if (problems.length) throw new Error(`The drawing's eyes don't fit the rig: ${problems.join('; ')}`);
  const doc = svg.ownerDocument;
  const make = (tag: string, attributes: Record<string, string>) => {
    const node = doc.createElementNS('http://www.w3.org/2000/svg', tag);
    for (const [name, value] of Object.entries(attributes)) node.setAttribute(name, value);
    return node;
  };
  for (const side of sides) {
    const part = (id: string) => svg.querySelector<SVGElement>(`[id="${id}-${side}"]`)!;
    part('iris-static').remove();
    part('iris-dynamic').removeAttribute('opacity');
    const socket = part('eye-socket'), contents = part('eye-contents'), brow = part('brow');
    const ink = socket.style.fill, drawn = socket.getAttribute('d')!;
    socket.setAttribute('data-drawn', drawn);
    brow.setAttribute('data-drawn', brow.getAttribute('d')!);
    contents.querySelectorAll('clipPath').forEach((node) => node.remove());
    contents.querySelectorAll('[clip-path]').forEach((node) => node.removeAttribute('clip-path'));
    const clip = make('clipPath', { id: `eye-clip-${side}`, clipPathUnits: 'userSpaceOnUse' });
    clip.append(make('path', { id: `eye-clip-shape-${side}`, d: drawn }));
    contents.before(clip);
    contents.setAttribute('clip-path', `url(#eye-clip-${side})`);
    const outline = (id: string, width: number) => make('path', {
      id, d: drawn, fill: 'none', stroke: ink, 'stroke-width': String(width), 'stroke-linejoin': 'round',
    });
    contents.after(outline(`eye-rim-${side}`, rimWidth));
    contents.append(outline(`eye-lids-${side}`, 2 * lidDepth));
  }
}

export type Eye = {
  side: Side;
  /** The drawn opening and brow, as they were before any animation. */
  drawn: { opening: string; brow: string };
  brow: SVGElement; gaze: SVGElement; circle: SVGElement; rim: SVGElement; lids: SVGElement;
  /** Where the eyeball rests, in the coordinates of the `space` given to `findEyes`. */
  neutral: Point;
  /** How far the eyeball may travel from rest: sideways, up and down. */
  limits: { horizontal: number; up: number; down: number };
  /** Reshapes the eye opening: the socket and every copy of it (clip, rim, lids). */
  setOpening: (d: string) => void;
  /** Moves the eyeball by a gaze offset (see `gazeOffset`). */
  look: (offset: number[]) => void;
};

/**
 * Finds the rigged eyes in a shown drawing. `find` looks a layer up by its unprefixed ID (and throws when it's
 * missing); `space` is the element whose coordinates `neutral` and `limits` are given in, normally the head.
 */
export function findEyes(find: (id: string) => Element, space: SVGGraphicsElement): Record<Side, Eye> {
  const eye = (side: Side): Eye => {
    const el = <T extends Element = SVGElement>(part: string) => find(`${part}-${side}`) as T;
    const bounds = el<SVGGraphicsElement>('gaze-bounds'), neutralGuide = el<SVGGraphicsElement>('gaze-neutral');
    const toSpace = (guide: SVGGraphicsElement) => space.getScreenCTM()!.inverse().multiply(guide.getScreenCTM()!);
    const point = (guide: SVGGraphicsElement, dx = 0, dy = 0): Point => {
      const p = new DOMPoint(Number(guide.getAttribute('cx')) + dx, Number(guide.getAttribute('cy')) + dy).matrixTransform(toSpace(guide));
      return [p.x, p.y];
    };
    const rx = Number(bounds.getAttribute('rx')), ry = Number(bounds.getAttribute('ry'));
    const neutral = point(neutralGuide);
    const [left, right, top, bottom] = [point(bounds, -rx), point(bounds, rx), point(bounds, 0, -ry), point(bounds, 0, ry)];
    const socket = el('eye-socket'), brow = el('brow'), gaze = el('iris-gaze');
    const copies = [socket, el('eye-clip-shape'), el('eye-rim'), el('eye-lids')];
    return {
      side, brow, gaze, circle: el('iris-circle'), rim: el('eye-rim'), lids: el('eye-lids'), neutral,
      drawn: { opening: socket.getAttribute('data-drawn')!, brow: brow.getAttribute('data-drawn')! },
      limits: { horizontal: (right[0] - left[0]) / 2, up: neutral[1] - top[1], down: bottom[1] - neutral[1] },
      setOpening: (d) => { for (const copy of copies) copy.setAttribute('d', d); },
      look: ([dx, dy]) => gaze.setAttribute('transform', `translate(${dx.toFixed(2)} ${dy.toFixed(2)})`),
    };
  };
  return { left: eye('left'), right: eye('right') };
}

/** The point between the two resting eyeballs, where gaze directions are measured from. */
export const between = (eyes: Record<Side, Eye>): Point =>
  [(eyes.left.neutral[0] + eyes.right.neutral[0]) / 2, (eyes.left.neutral[1] + eyes.right.neutral[1]) / 2];

/** Pointer position in the head's coordinates, expressed as a gaze direction within the unit disk. */
export function pointerGaze(space: SVGGraphicsElement, origin: Point, reach: number, [x, y]: Point): Point | undefined {
  const ctm = space.getScreenCTM();
  if (!ctm) return;
  const p = new DOMPoint(x, y).matrixTransform(ctm.inverse());
  const u = (p.x - origin[0]) / reach, v = (p.y - origin[1]) / reach;
  const length = Math.hypot(u, v);
  return length > 1 ? [u / length, v / length] : [u, v];
}

/** Move a gaze toward a target at a frame-rate-independent speed; return the clamped target. */
export function easeGaze(gaze: Point, target: Point, rate: number, dt: number): Point {
  const length = Math.hypot(target[0], target[1]);
  const [u, v]: Point = length > 1 ? [target[0] / length, target[1] / length] : target;
  const k = 1 - Math.exp(-rate * dt);
  gaze[0] += (u - gaze[0]) * k;
  gaze[1] += (v - gaze[1]) * k;
  return [u, v];
}

/** Clamps a gaze direction to the unit disk, then maps it into the eye's asymmetric oval (up is shallower than down). */
export function gazeOffset(u: number, v: number, limits: { horizontal: number; up: number; down: number }) {
  const length = Math.hypot(u, v);
  if (length > 1) { u /= length; v /= length; }
  return [u * limits.horizontal, v * (v < 0 ? limits.up : limits.down)];
}

const numbers = (d: string) => d.match(/-?\d*\.?\d+/g)!.map(Number);

/** Whether two paths can morph into each other: the same path commands, so the same count of numbers. */
export const corresponds = (from: string, to: string) => numbers(from).length === numbers(to).length;

/** Interpolates two paths with identical command structure, number by number. */
export function morph(from: string, to: string) {
  const a = numbers(from), b = numbers(to);
  if (a.length !== b.length) throw new Error('A guide does not correspond to the shape it morphs from');
  return (t: number) => {
    let i = 0;
    return from.replace(/-?\d*\.?\d+/g, () => { const v = a[i] + (b[i] - a[i]) * t; i++; return (+v.toFixed(2)).toString(); });
  };
}
