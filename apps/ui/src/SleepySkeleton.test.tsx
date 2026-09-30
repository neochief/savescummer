import { afterEach, expect, test } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import { SleepySkeleton, gazeOffset } from './SleepySkeleton';
import source from '../public/character/no-games-found.svg?raw';

afterEach(cleanup);

test('the drawing has every layer the rig reads, in a form Affinity Designer keeps on export', () => {
  const svg = new DOMParser().parseFromString(source, 'image/svg+xml');
  const parts = [
    'head-pose', 'pivot-head', 'hanging-finger-outer', 'hanging-finger-middle', 'hanging-finger-left',
    'support-outer-distal', 'support-middle-distal', 'support-inner-distal',
    ...['left', 'right'].flatMap((side) => [
      'eye-socket', 'eye-contents', 'iris-static', 'iris-dynamic', 'iris-gaze', 'iris-circle', 'brow', 'gaze-bounds',
      'gaze-neutral', 'guide-open-opening', 'guide-closed-opening', 'guide-open-brow', 'guide-closed-brow',
    ].map((part) => `${part}-${side}`)),
  ];
  expect(parts.filter((id) => !svg.querySelector(`[id="${id}"]`))).toEqual([]);
  // Each ID must be the layer's own name. Affinity mangles a name that isn't a valid ID (spaces, a leading number) and
  // keeps the original in serif:id, so the ID would change whenever someone renames or renumbers that layer.
  expect(parts.filter((id) => svg.querySelector(`[id="${id}"]`)?.hasAttribute('serif:id'))).toEqual([]);
  // Each eyelid guide morphs into its pair number by number, so a redrawn guide must keep the same path commands.
  const numbers = (id: string) => svg.querySelector(`[id="${id}"]`)!.getAttribute('d')!.match(/-?\d*\.?\d+/g)!.length;
  const pairs = ['opening', 'brow'].flatMap((part) => ['left', 'right'].map((side) => `${part}-${side}`));
  expect(pairs.filter((pair) => numbers(`guide-open-${pair}`) !== numbers(`guide-closed-${pair}`))).toEqual([]);
  // Affinity drops hidden layers, data attributes and <use> references on export.
  const lost = [...svg.querySelectorAll('*')].filter((node) => node.localName === 'use'
    || /display:\s*none/.test(node.getAttribute('style') ?? '') || [...node.attributes].some((a) => a.name.startsWith('data-')));
  expect(lost.map((node) => node.id || node.localName)).toEqual([]);
});

test('gaze is clamped to the unit disk before mapping into the asymmetric oval', () => {
  const limits = { horizontal: 24, up: 12, down: 82.32 };
  const [dx, dy] = gazeOffset(5, 5, limits);
  expect((dx / 24) ** 2 + (dy / 82.32) ** 2).toBeCloseTo(1);
  expect(gazeOffset(0, -3, limits)[1]).toBeCloseTo(-12);
  expect(gazeOffset(0.5, 0, limits)).toEqual([12, 0]);
});

test('each instance references only its own clip paths and shapes', () => {
  const { container } = render(<><SleepySkeleton /><SleepySkeleton /></>);
  const ids = [...container.querySelectorAll('[id]')].map((node) => node.id);
  expect(new Set(ids).size).toBe(ids.length);
  for (const svg of container.querySelectorAll('svg')) {
    const refs = [...svg.querySelectorAll('*')].flatMap((node) => [
      node.getAttribute('href'), node.getAttribute('xlink:href'), node.getAttribute('clip-path')?.match(/url\(#(.+)\)/)?.[1],
    ]).filter(Boolean).map((ref) => ref!.replace(/^#/, ''));
    expect(refs.length).toBeGreaterThan(0);
    for (const ref of refs) expect(svg.querySelector(`[id="${ref}"]`)).toBeTruthy();
  }
});
