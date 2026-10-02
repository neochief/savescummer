import { useEffect, useState } from 'react';
import type { AnimatedIconProps } from './animatedIcon';
import './AnimateCircleReturn.css';

/** Rock the original return icon back and forth as one solid piece. */
export function AnimateCircleReturn({ active = false, className = '' }: AnimatedIconProps) {
  const [running, setRunning] = useState(active);

  useEffect(() => {
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    if (!reduced) {
      if (active) setRunning(true);
      return;
    }
    const syncMotion = () => {
      if (reduced.matches) setRunning(false);
      else if (active) setRunning(true);
    };
    syncMotion();
    reduced.addEventListener('change', syncMotion);
    return () => reduced.removeEventListener('change', syncMotion);
  }, [active]);

  return <img className={`icon animate-circle-return ${running ? 'is-animated' : ''} ${className}`}
    src="/icons/rotate-left.svg" alt="" aria-hidden="true"
    onAnimationIteration={(event) => {
      // Each loop ends at rest; a pointer exit lets the current push and spring return finish.
      if (event.animationName === 'circle-return-rock' && !active) setRunning(false);
    }} />;
}
