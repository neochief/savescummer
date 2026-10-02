import { afterEach, expect, test } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import { CrouchingSkeleton } from './CrouchingSkeleton';
import { PointingSkeleton } from './PointingSkeleton';
import { SleepySkeleton } from './SleepySkeleton';
import { easeGaze, eyeProblems, gazeOffset, morph, prepareEyes, sides } from './skeletonEyes';
import crouching from '../public/character/no-checkpoints.svg?raw';
import pointing from '../public/character/no-game-selected.svg?raw';
import sleepy from '../public/character/no-games-found.svg?raw';
import hugging from '../public/character/popup-hug.svg?raw';

afterEach(cleanup);

const parse = (source: string) => new DOMParser().parseFromString(source, 'image/svg+xml').documentElement;

test('every character drawing lays its eyes out as the eye rig requires', () => {
  for (const source of [crouching, pointing, sleepy, hugging]) expect(eyeProblems(parse(source))).toEqual([]);
});

test('a drawing that breaks the eye requirements is named, part by part, and not rigged', () => {
  const svg = parse(crouching);
  svg.querySelector('[id="iris-circle-left"]')!.remove();
  svg.querySelector('[id="brow-right"]')!.setAttribute('id', 'Brow right');
  svg.querySelector('[id="eye-contents-right"]')!.setAttribute('transform', 'matrix(1,0,0,1,4,0)');
  expect(eyeProblems(svg)).toEqual(['iris-circle-left: missing', 'brow-right: missing', 'eye-contents-right: transformed']);
  expect(() => prepareEyes(svg, { rimWidth: 16, lidDepth: 20 })).toThrow(/iris-circle-left: missing/);
});

test('the rigged eyes are plain shapes, with each clip right before what it clips', () => {
  // WebKitGTK drops a clip built from a <use>, which hides the red eyeballs.
  for (const source of [crouching, pointing, sleepy, hugging]) {
    const svg = parse(source);
    prepareEyes(svg, { rimWidth: 16, lidDepth: 20 });
    expect(svg.querySelectorAll('use')).toHaveLength(0);
    for (const side of sides) {
      const contents = svg.querySelector(`[id="eye-contents-${side}"]`)!;
      expect(contents.getAttribute('clip-path')).toBe(`url(#eye-clip-${side})`);
      expect(contents.previousElementSibling!.id).toBe(`eye-clip-${side}`);
      expect(contents.previousElementSibling!.firstElementChild!.localName).toBe('path');
      expect(svg.querySelector(`[id="iris-static-${side}"]`)).toBeNull();
    }
  }
});

test('the hugging artwork keeps its articulated chain and attachment guides inside their moving parts', () => {
  const svg = parse(hugging);
  const chain = ['left-arm', 'left-forearm', 'left-hand'];
  for (let i = 1; i < chain.length; i++) {
    expect(svg.querySelector(`[id="${chain[i]}"]`)!.parentElement!.id).toBe(chain[i - 1]);
  }
  const joints = [
    ['head-pose', 'pivot-head'], ['left-arm', 'pivot-left-shoulder'],
    ['left-forearm', 'pivot-left-elbow'], ['left-hand', 'pivot-left-wrist'],
    ['left-hand', 'anchor-grip-left'], ['right-hand-pose', 'pivot-right-wrist'],
    ['right-hand-pose', 'anchor-grip-right'],
    ...['left', 'right'].flatMap((side) => ['inner', 'middle', 'outer'].map((finger) =>
      [`${side}-finger-${finger}`, `pivot-${side}-finger-${finger}`])),
  ];
  for (const [part, joint] of joints) {
    const guide = svg.querySelector(`[id="${part}"] [id="${joint}"]`)!;
    expect(guide.localName).toBe('circle');
    expect(guide.parentElement!.getAttribute('opacity')).toBe('0');
  }
  const ids = [...svg.querySelectorAll('[id]')].map((node) => node.id);
  expect(new Set(ids).size).toBe(ids.length);
  expect(ids.every((id) => /^[a-z][a-z0-9-]*$/.test(id))).toBe(true);
  expect(svg.querySelectorAll('image, use, foreignObject')).toHaveLength(0);
});

test('every character shows round red eyes in place of the drawn crescents', () => {
  const { container } = render(<><CrouchingSkeleton /><PointingSkeleton /><SleepySkeleton /></>);
  for (const svg of container.querySelectorAll('svg')) {
    expect(svg.querySelectorAll('[id$="iris-static-left"], [id$="iris-static-right"], use')).toHaveLength(0);
    expect(svg.querySelectorAll('circle[id*="iris-circle"]')).toHaveLength(2);
  }
});

test('gaze is clamped to the unit disk before mapping into the asymmetric oval', () => {
  const limits = { horizontal: 24, up: 12, down: 82.32 };
  const [dx, dy] = gazeOffset(5, 5, limits);
  expect((dx / 24) ** 2 + (dy / 82.32) ** 2).toBeCloseTo(1);
  expect(gazeOffset(0, -3, limits)[1]).toBeCloseTo(-12);
  expect(gazeOffset(0.5, 0, limits)).toEqual([12, 0]);
});

test('all three animations share the same clamped gaze easing', () => {
  const gaze: [number, number] = [0, 0];
  expect(easeGaze(gaze, [3, 4], Math.log(2), 1)).toEqual([0.6, 0.8]);
  expect(gaze[0]).toBeCloseTo(0.3);
  expect(gaze[1]).toBeCloseTo(0.4);
});

test('a guide morphs from its shape number by number', () => {
  const squint = morph('M0,0C10,0 10,10 0,10Z', 'M0,4C10,4 10,6 0,6Z');
  expect(squint(0)).toBe('M0,0C10,0 10,10 0,10Z');
  expect(squint(0.5)).toBe('M0,2C10,2 10,8 0,8Z');
  expect(() => morph('M0,0L1,1Z', 'M0,0C1,1 2,2 3,3Z')).toThrow();
});
