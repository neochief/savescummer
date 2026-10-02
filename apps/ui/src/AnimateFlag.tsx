import { iconPath, useIconAnimation, type AnimatedIconProps, type IconPath } from './animatedIcon';

// The original flag.svg outline, expressed as absolute curves so the cloth can bend while the pole stays put.
const outline: IconPath = [
  ['M', 61.6, 27.9],
  ['C', 57.5, -10.9, -1.8, -8.6, -2.2, 31.1],
  ['C', -4.8, 177, -7.3, 335.2, -3.9, 479.5],
  ['C', -3.9, 497.2, 10.4, 511.5, 28.1, 511.5],
  ['C', 45.8, 511.5, 60.1, 497.2, 60.1, 479.5],
  ['C', 60.1, 436.2, 59.2, 395.5, 58.5, 353.5],
  ['C', 108.3, 346.2, 150.1, 356.1, 199.6, 380.8],
  ['C', 230, 394.6, 266.1, 410.9, 306.2, 414.6],
  ['C', 335.3, 417.3, 381.8, 412.8, 403.4, 410.5],
  ['C', 427.8, 407.8, 445.8, 387.3, 445.9, 363.2],
  ['C', 446.2, 282.2, 449.4, 222.1, 446, 142.8],
  ['C', 442.3, 80, 368.1, 97.9, 324.8, 87.8],
  ['C', 258.5, 75.8, 219.7, 19.7, 139.7, 21.3],
  ['C', 116.9, 21.4, 84.6, 25, 61.6, 27.9],
  ['L', 61.6, 27.9], ['Z'],
  ['M', 61.1, 92.5],
  ['C', 120, 83.2, 173.6, 78.7, 224.3, 115.1],
  ['C', 263.9, 143.3, 328, 156.8, 382.3, 158.3],
  ['C', 385.4, 232.1, 382.2, 272.5, 381.9, 348.3],
  ['C', 320.5, 356.3, 280.3, 350, 223.4, 321.3],
  ['C', 177.8, 297.7, 115.2, 280.5, 57.7, 289.2],
  ['C', 57.1, 229.2, 61.2, 153.6, 61, 92.4], ['Z'],
];
// A horizontal mid-wave pose is also the first frame of each hover loop.
const basePhase = 0.66 * Math.PI * 2 / 1.35;
const resting = wave(0);

function wave(seconds: number, strength = 1) {
  const phase = basePhase + seconds * Math.PI * 2 / 1.35;
  return iconPath(outline, (x, y) => {
    const base = wavePoint(x, y, basePhase), moving = wavePoint(x, y, phase);
    return base.map((value, axis) => value + (moving[axis] - value) * strength);
  });
}

function wavePoint(x: number, y: number, phase: number) {
  const cloth = Math.max(0, (x - 62) / 386);
  const ripple = Math.sin(phase - cloth * Math.PI * 2);
  // Lift the drawing's drooping fly into a horizontal breeze; flutter mostly along its length.
  return [x - cloth * 22 * (1 - Math.cos(phase - cloth * Math.PI)),
    y + cloth * (-72 + 22 * ripple + 5 * Math.sin(phase * 2 - cloth * Math.PI * 3))];
}

export function AnimateFlag({ active = false, className = '' }: AnimatedIconProps) {
  const path = useIconAnimation(active, resting, wave, 0.4);
  return <svg className={`icon animate-flag ${className}`} viewBox="0 0 448 512" aria-hidden="true" focusable="false">
    <path ref={path} fill="#fdf6ee" d={resting} />
  </svg>;
}
