import { afterEach, expect, test } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import { PointingSkeleton } from './PointingSkeleton';

afterEach(cleanup);

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
