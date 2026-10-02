import { between, easeGaze, findEyes, gazeOffset, pointerGaze, prepareEyes, sides } from './skeleton-eyes.js';

// Source anchors: panel left x=379, lower grip y=734, upper grip y=390.
// The left character follows the panel's bottom; the independent right hand follows its top.
export function createPopupHug(tooltip, panel, onReady) {
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
  let loading, layers, eyes, head, origin, find, pivots;
  let frame = 0, last = 0, pointer = null;
  const gaze = [0, 0];
  const visible = () => !tooltip.hidden && tooltip.classList.contains('hugged');
  const scale = () => Math.min(0.6, panel.offsetWidth / 700);

  function bounds() {
    const s = scale();
    return { left: 250 * s, right: 28 * s, top: Math.max(50 * s, 490 * s - panel.offsetHeight), bottom: 58 * s };
  }

  function update() {
    cancelAnimationFrame(frame);
    frame = 0;
    if (!visible()) return;
    if (!layers) {
      loading ??= load().then(onReady).catch(error => console.warn('Popup character could not load', error));
      return;
    }
    const s = scale(), width = panel.offsetWidth, height = panel.offsetHeight;
    for (const layer of layers) {
      layer.setAttribute('viewBox', `0 0 ${width} ${height}`);
      layer.firstElementChild.setAttribute('transform', `translate(${-379 * s} ${height - 734 * s}) scale(${s})`);
    }
    find('right-hand').setAttribute('transform', `translate(${width / s - 700} ${734 - height / s - 390})`);
    if (!eyes) {
      eyes = findEyes(find, head);
      origin = between(eyes);
      for (const joint of pivots.values()) {
        const matrix = joint.part.parentElement.getScreenCTM().inverse().multiply(joint.guide.getScreenCTM());
        const center = new DOMPoint(Number(joint.guide.getAttribute('cx')), Number(joint.guide.getAttribute('cy'))).matrixTransform(matrix);
        joint.center = `${center.x} ${center.y}`;
      }
    }
    pose(0);
    if (reducedMotion.matches || document.hidden) {
      for (const side of sides) eyes[side].look([0, 0]);
      return;
    }
    last = performance.now();
    frame = requestAnimationFrame(tick);
  }

  async function load() {
    const response = await fetch(new URL('./popup-hug.svg', import.meta.url));
    if (!response.ok) throw new Error(`Artwork: ${response.status}`);
    const svg = new DOMParser().parseFromString(await response.text(), 'image/svg+xml').documentElement;
    prepareEyes(svg, { rimWidth: 0, lidDepth: 4 });
    // One source, split only for painting the ribs behind the panel and the face/hands above it.
    const makeLayer = (name, groups) => {
      const layer = document.createElementNS(svg.namespaceURI, 'svg');
      layer.setAttribute('class', `game-tooltip-mascot game-tooltip-mascot-${name}`);
      layer.setAttribute('aria-hidden', 'true');
      layer.setAttribute('focusable', 'false');
      layer.setAttribute('style', svg.getAttribute('style'));
      const holder = document.createElementNS(svg.namespaceURI, 'g');
      holder.append(...groups);
      layer.append(holder);
      panel.prepend(layer);
      return layer;
    };
    const torso = svg.querySelector('[id="torso"]');
    const back = makeLayer('back', [torso]);
    const front = makeLayer('front', [...svg.children].filter(node => node.localName === 'g'));
    find = id => front.querySelector(`[id="${id}"]`);
    head = find('head-pose');
    pivots = new Map([...front.querySelectorAll('circle[id^="pivot-"]')].map(guide => {
      const part = guide.parentElement.parentElement;
      return [guide.id, { guide, part, rest: part.getAttribute('transform') ?? '' }];
    }));
    // Prefix all fragment references so the page can also contain other skeleton artwork.
    for (const layer of [back, front]) {
      for (const node of layer.querySelectorAll('*')) {
        for (const attribute of [...node.attributes]) {
          if (attribute.name !== 'id') node.setAttribute(attribute.name, attribute.value.replace(/url\(#([^)]+)\)/g, 'url(#popup-hug-$1)'));
        }
        if (node.id) node.id = `popup-hug-${node.id}`;
      }
    }
    find = id => front.querySelector(`[id="popup-hug-${id}"]`);
    layers = [back, front];
  }

  function rotate(part, pivot, degrees) {
    const joint = pivots.get(pivot);
    find(part).setAttribute('transform', `rotate(${degrees.toFixed(3)} ${joint.center}) ${joint.rest}`);
  }

  function pose(sway) {
    rotate('head-pose', 'pivot-head', sway * 1.6);
    rotate('left-arm', 'pivot-left-shoulder', sway * 0.3);
    rotate('left-forearm', 'pivot-left-elbow', sway * -0.6);
    rotate('left-hand', 'pivot-left-wrist', sway * 0.3);
    for (const side of sides) {
      for (const finger of ['inner', 'middle', 'outer']) rotate(`${side}-finger-${finger}`, `pivot-${side}-finger-${finger}`, sway * 0.5);
    }
  }

  function tick(now) {
    if (!visible() || document.hidden || reducedMotion.matches) { frame = 0; return; }
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;
    pose(Math.sin(now / 1700));
    const target = pointer ? pointerGaze(head, origin, 320, pointer) ?? [0, 0] : [0, 0];
    easeGaze(gaze, target, 9, dt);
    for (const side of sides) eyes[side].look(gazeOffset(...gaze, eyes[side].limits));
    frame = requestAnimationFrame(tick);
  }

  document.addEventListener('pointermove', event => { pointer = [event.clientX, event.clientY]; }, { passive: true });
  document.documentElement.addEventListener('pointerleave', () => { pointer = null; });
  document.addEventListener('visibilitychange', update);
  reducedMotion.addEventListener('change', update);
  new ResizeObserver(() => { if (visible()) onReady(); }).observe(panel);
  return { update, bounds };
}
