import { useLayoutEffect, useRef } from 'react';

export type IconPath = readonly (readonly [string, ...number[]])[];
export type AnimatedIconProps = { active?: boolean; className?: string };

/** Deform the drawing's absolute control points, keeping its continuous outline intact. */
export function iconPath(outline: IconPath, move = (x: number, y: number) => [x, y]): string {
  return outline.map(([command, ...points]) => {
    const moved = [];
    for (let i = 0; i < points.length; i += 2) moved.push(...move(points[i], points[i + 1]));
    return command + moved.map((value) => +value.toFixed(3)).join(' ');
  }).join(' ');
}

/** Each icon owns its clock; an optional settling time fades motion back into its resting pose. */
export function useIconAnimation(active: boolean, resting: string, frame: (seconds: number, strength: number) => string, settleDuration = 0) {
  const path = useRef<SVGPathElement>(null);
  const playing = useRef(active);
  const refresh = useRef<(() => void) | undefined>(undefined);
  useLayoutEffect(() => {
    const node = path.current;
    if (!node || typeof window.matchMedia !== 'function') return;
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
    let request = 0;
    let previous: number | undefined;
    let seconds = 0;
    let strength = 0;
    const stop = () => {
      cancelAnimationFrame(request);
      request = 0;
      previous = undefined;
      seconds = strength = 0;
      node.setAttribute('d', resting);
    };
    const tick = (now: number) => {
      const delta = previous === undefined ? 0 : (now - previous) / 1000;
      previous = now;
      seconds += delta;
      strength = settleDuration
        ? Math.max(0, Math.min(1, strength + (playing.current ? 1 : -1) * delta / settleDuration))
        : Number(playing.current);
      if (!playing.current && strength === 0) { stop(); return; }
      // Ease the wind down while the wave keeps travelling. Re-hover continues from the current pose.
      node.setAttribute('d', frame(seconds, strength * strength * (3 - 2 * strength)));
      request = requestAnimationFrame(tick);
    };
    const sync = () => {
      if (reduced.matches || document.hidden || (!playing.current && (!settleDuration || !strength))) {
        stop();
        return;
      }
      if (!request) request = requestAnimationFrame(tick);
    };
    refresh.current = sync;
    sync();
    reduced.addEventListener('change', sync);
    document.addEventListener('visibilitychange', sync);
    return () => {
      stop();
      refresh.current = undefined;
      reduced.removeEventListener('change', sync);
      document.removeEventListener('visibilitychange', sync);
    };
  }, [resting, frame, settleDuration]);
  useLayoutEffect(() => {
    playing.current = active;
    refresh.current?.();
  }, [active]);
  return path;
}
