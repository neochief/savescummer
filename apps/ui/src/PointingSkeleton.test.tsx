import { afterEach, expect, test } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import { PointingSkeleton } from './PointingSkeleton';
import source from '../public/character/no-game-selected.svg?raw';
import { eyeLayers, eyeProblems } from './skeletonEyes';

afterEach(cleanup);

test('the drawing has every layer the rig reads, in a form Affinity Designer keeps on export', () => {
  const svg = new DOMParser().parseFromString(source, 'image/svg+xml');
  const parts = [
    'upper-arm-left', 'forearm-left', 'hand-left', 'pivot-shoulder', 'pivot-elbow', 'pivot-wrist',
    'controller-upper-arm', 'controller-forearm', 'controller-palm', 'gamepad', 'controller-fingers',
    'pivot-shoulder-controller', 'pivot-elbow-controller', 'pivot-wrist-controller',
    ...eyeLayers,
  ];
  expect(parts.filter((id) => !svg.querySelector(`[id="${id}"]`))).toEqual([]);
  expect(eyeProblems(svg.documentElement)).toEqual([]);
  // Each ID must be the layer's own name. Affinity mangles a name that isn't a valid ID (spaces, a leading number) and
  // keeps the original in serif:id, so the ID would change whenever someone renames or renumbers that layer.
  expect(parts.filter((id) => svg.querySelector(`[id="${id}"]`)?.hasAttribute('serif:id'))).toEqual([]);
  // Affinity drops hidden layers, data attributes and <use> references on export.
  const lost = [...svg.querySelectorAll('*')].filter((node) => node.localName === 'use'
    || /display:\s*none/.test(node.getAttribute('style') ?? '') || [...node.attributes].some((a) => a.name.startsWith('data-')));
  expect(lost.map((node) => node.id || node.localName)).toEqual([]);
});

test('each instance references only its own clip paths and shapes, with round eyes in place of the crescents', () => {
  const { container } = render(<><PointingSkeleton /><PointingSkeleton /></>);
  const ids = [...container.querySelectorAll('[id]')].map((node) => node.id);
  expect(new Set(ids).size).toBe(ids.length);
  for (const svg of container.querySelectorAll('svg')) {
    const refs = [...svg.querySelectorAll('*')].flatMap((node) => [
      node.getAttribute('href'), node.getAttribute('xlink:href'), node.getAttribute('clip-path')?.match(/url\(#(.+)\)/)?.[1],
    ]).filter(Boolean).map((ref) => ref!.replace(/^#/, ''));
    expect(refs.length).toBeGreaterThan(0);
    for (const ref of refs) expect(svg.querySelector(`[id="${ref}"]`)).toBeTruthy();
    expect(svg.querySelector('[id$="iris-static-left"]')).toBeNull();
    expect(svg.querySelectorAll('circle[id*="iris-circle"]')).toHaveLength(2);
  }
});
