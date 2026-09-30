import { afterEach, expect, test } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import { SleepySkeleton, gazeOffset } from './SleepySkeleton';

afterEach(cleanup);

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
