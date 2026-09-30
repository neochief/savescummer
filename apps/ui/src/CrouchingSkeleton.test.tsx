import { afterEach, expect, test } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import { CrouchingSkeleton } from './CrouchingSkeleton';
import source from '../public/character/no-checkpoints.svg?raw';

afterEach(cleanup);

test('the drawing has every layer the rig reads, in a form Affinity Designer keeps on export', () => {
  const svg = new DOMParser().parseFromString(source, 'image/svg+xml');
  const parts = [
    'head', 'torso', 'arm-controller', 'arm-controller-forearm',
    ...['back', 'front'].flatMap((leg) => [`leg-${leg}-thigh`, `leg-${leg}-shin`, `leg-${leg}-foot`]),
    'arm-support-upper', 'arm-support-forearm', 'hand-support',
    ...['body', 'neck', 'hip-back', 'knee-back', 'ankle-back', 'hip-front', 'knee-front', 'ankle-front', 'shoulder-support',
      'elbow-support', 'wrist-support', 'elbow-controller'].map((joint) => `pivot-${joint}`),
    ...['left', 'right'].flatMap((side) => [
      'eye-socket', 'eye-contents', 'iris-static', 'iris-dynamic', 'iris-gaze', 'iris-circle', 'brow', 'gaze-bounds',
      'gaze-neutral', 'guide-squint-opening', 'guide-squint-brow',
    ].map((part) => `${part}-${side}`)),
  ];
  expect(parts.filter((id) => !svg.querySelector(`[id="${id}"]`))).toEqual([]);
  // Each ID must be the layer's own name. Affinity mangles a name that isn't a valid ID (spaces, a leading number) and
  // keeps the original in serif:id, so the ID would change whenever someone renames or renumbers that layer.
  expect(parts.filter((id) => svg.querySelector(`[id="${id}"]`)?.hasAttribute('serif:id'))).toEqual([]);
  // Each squint guide morphs from the drawn shape number by number, so a redrawn one must keep the same path commands.
  const numbers = (id: string) => svg.querySelector(`[id="${id}"]`)!.getAttribute('d')!.match(/-?\d*\.?\d+/g)!.length;
  const pairs = ['left', 'right'].flatMap((side) => [[`eye-socket-${side}`, `guide-squint-opening-${side}`], [`brow-${side}`, `guide-squint-brow-${side}`]]);
  expect(pairs.filter(([drawn, guide]) => numbers(drawn) !== numbers(guide))).toEqual([]);
  // Affinity drops hidden layers, data attributes and <use> references on export.
  const lost = [...svg.querySelectorAll('*')].filter((node) => node.localName === 'use'
    || /display:\s*none/.test(node.getAttribute('style') ?? '') || [...node.attributes].some((a) => a.name.startsWith('data-')));
  expect(lost.map((node) => node.id || node.localName)).toEqual([]);
});

test('each instance references only its own clip paths and shapes, with round eyes in place of the crescents', () => {
  const { container } = render(<><CrouchingSkeleton /><CrouchingSkeleton /></>);
  const ids = [...container.querySelectorAll('[id]')].map((node) => node.id);
  expect(new Set(ids).size).toBe(ids.length);
  for (const svg of container.querySelectorAll('svg')) {
    const refs = [...svg.querySelectorAll('*')].flatMap((node) => [
      node.getAttribute('href'), node.getAttribute('xlink:href'), node.getAttribute('clip-path')?.match(/url\(#(.+)\)/)?.[1],
    ]).filter(Boolean).map((ref) => ref!.replace(/^#/, ''));
    expect(refs.length).toBeGreaterThan(0);
    for (const ref of refs) expect(svg.querySelector(`[id="${ref}"]`)).toBeTruthy();
    expect(svg.querySelector('[id$="iris-static-left"]')).toBeNull();
  }
});
