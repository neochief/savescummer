import { afterEach, expect, test } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import { CrouchingSkeleton, crouchingBodyAt, crouchingHandDropAt, crouchingHeadAt,
  crouchingSkeletonMotion } from './CrouchingSkeleton';
import source from '../public/character/no-checkpoints.svg?raw';
import { corresponds, eyeLayers, eyeProblems } from './skeletonEyes';
import { skeletonTiming } from './SleepySkeleton';

afterEach(cleanup);

test('the head drowses, briefly sleeps, then wakes on the sibling animation timing', () => {
  const asleepAt = skeletonTiming.classic + skeletonTiming.drowse + skeletonTiming.heavy
    + skeletonTiming.almostClosed + skeletonTiming.closing;
  expect(crouchingHeadAt(0)).toEqual({ closure: 0, sag: 0, startle: 0 });
  expect(crouchingHeadAt(skeletonTiming.classic).sag).toBeGreaterThan(0);
  expect(crouchingHeadAt(skeletonTiming.classic + skeletonTiming.drowse).sag)
    .toBeCloseTo((skeletonTiming.classic + skeletonTiming.drowse) / asleepAt);
  expect(crouchingHeadAt(asleepAt)).toEqual({ closure: 1, sag: 1, startle: 0 });
  expect(crouchingHeadAt(asleepAt + skeletonTiming.asleep + skeletonTiming.wake))
    .toEqual({ closure: 0, sag: -0.35, startle: 1 });
});

test('the bounce, sleep coil, and startled twitch share the head cycle', () => {
  const closingAt = skeletonTiming.classic + skeletonTiming.drowse + skeletonTiming.heavy
    + skeletonTiming.almostClosed;
  const wakeAt = closingAt + skeletonTiming.closing + skeletonTiming.asleep;
  expect(crouchingBodyAt(0)).toEqual({ depth: 0, move: 'bounce' });
  expect(crouchingBodyAt(closingAt)).toEqual({ depth: 0, move: 'spring' });
  expect(crouchingBodyAt(wakeAt).depth).toBe(1);
  expect(crouchingHeadAt(wakeAt).closure).toBeCloseTo(1);
  expect(crouchingHeadAt(wakeAt).sag).toBeCloseTo(1);
  expect(crouchingHeadAt(wakeAt).startle).toBeCloseTo(0);
  expect(crouchingBodyAt(wakeAt + skeletonTiming.wake).depth).toBeCloseTo(-0.35);
  expect(crouchingHeadAt(wakeAt + skeletonTiming.wake).startle).toBeCloseTo(1);
  expect(crouchingHandDropAt(wakeAt - crouchingSkeletonMotion.sleepy.handDropLead)).toBeCloseTo(0);
  expect(crouchingHandDropAt(wakeAt - crouchingSkeletonMotion.sleepy.handDropLead / 2)).toBeLessThan(0);
  expect(crouchingHandDropAt(wakeAt)).toBe(
    crouchingSkeletonMotion.sleepy.handDrop - crouchingSkeletonMotion.sleepy.armDroop);
  expect(crouchingHandDropAt(wakeAt + skeletonTiming.wake)).toBeCloseTo(0);
});

test('the drawing has every layer the rig reads, in a form Affinity Designer keeps on export', () => {
  const svg = new DOMParser().parseFromString(source, 'image/svg+xml');
  const parts = [
    'head', 'torso', 'arm-controller', 'arm-controller-forearm',
    ...['back', 'front'].flatMap((leg) => [`leg-${leg}-thigh`, `leg-${leg}-shin`, `leg-${leg}-foot`]),
    'arm-support-upper', 'arm-support-forearm', 'hand-support',
    ...['body', 'neck', 'shoulder-controller', 'hip-back', 'knee-back', 'ankle-back', 'hip-front', 'knee-front', 'ankle-front',
      'shoulder-support', 'elbow-support', 'wrist-support', 'elbow-controller'].map((joint) => `pivot-${joint}`),
    ...eyeLayers, ...['left', 'right'].flatMap((side) => [
      `guide-squint-opening-${side}`, `guide-squint-brow-${side}`,
      `guide-open-opening-${side}`, `guide-open-brow-${side}`,
      `guide-closed-opening-${side}`, `guide-closed-brow-${side}`,
      `guide-wake-opening-${side}`, `guide-wake-brow-${side}`,
    ]),
  ];
  expect(parts.filter((id) => !svg.querySelector(`[id="${id}"]`))).toEqual([]);
  expect(eyeProblems(svg.documentElement)).toEqual([]);
  // Each ID must be the layer's own name. Affinity mangles a name that isn't a valid ID (spaces, a leading number) and
  // keeps the original in serif:id, so the ID would change whenever someone renames or renumbers that layer.
  expect(parts.filter((id) => svg.querySelector(`[id="${id}"]`)?.hasAttribute('serif:id'))).toEqual([]);
  // Every guide morphs from the drawn shape number by number, so a redrawn one must keep the same path commands.
  const d = (id: string) => svg.querySelector(`[id="${id}"]`)!.getAttribute('d')!;
  const pairs = ['left', 'right'].flatMap((side) => ['squint', 'open', 'closed', 'wake'].flatMap((pose) => [
    [`eye-socket-${side}`, `guide-${pose}-opening-${side}`],
    [`brow-${side}`, `guide-${pose}-brow-${side}`],
  ]));
  expect(pairs.filter(([drawn, guide]) => !corresponds(d(drawn), d(guide)))).toEqual([]);
  const headRig = svg.querySelector('#head-rig')!;
  expect(headRig.getAttribute('opacity')).toBe('0');
  expect(parts.filter((id) => id.startsWith('guide-') && !headRig.contains(svg.querySelector(`[id="${id}"]`)))).toEqual([]);
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
