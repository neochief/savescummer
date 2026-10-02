export type InterfaceCue = 'button' | 'checkbox' | 'success' | 'detected' | 'detected-batch' | 'game-run' | 'game-stop' | 'undo'
  | 'save-start' | 'save-complete' | 'load-start' | 'load-complete' | 'operation-failed' | 'busy'
  | 'drawer-open' | 'drawer-close';
let lastButton = -Infinity;
let lastDiscovery = -Infinity;
const players = new Map<string, HTMLAudioElement>();

function playerFor(cue: InterfaceCue): HTMLAudioElement {
  const path = `/sounds/${cue}.wav`;
  let player = players.get(path);
  if (!player) {
    player = new Audio(path);
    player.preload = 'auto';
    players.set(path, player);
  }
  return player;
}

export function prepareDrawerSounds() {
  if (typeof Audio !== 'function') return;
  playerFor('drawer-open');
  playerFor('drawer-close');
}

export function playInterfaceSound(cue: InterfaceCue, enabled = true) {
  if (!enabled || typeof Audio !== 'function') return;
  const now = performance.now();
  if (cue === 'button' && now - lastButton < 55) return;
  if (cue.startsWith('detected') && now - lastDiscovery < 1600) return;
  if (cue === 'button') lastButton = now;
  if (cue.startsWith('detected')) lastDiscovery = now;
  const player = playerFor(cue);
  // Repeated quick clicks should sound once, without building an audio queue.
  if (player.currentTime > 0) player.currentTime = 0;
  try {
    const playback = player.play();
    if (playback) void playback.catch(() => undefined); // Autoplay can block background discovery.
  } catch { /* No audio device should never block a UI action. */ }
}

export function interfaceClickSound(event: MouseEvent, enabled: boolean) {
  if (!enabled || !(event.target instanceof Element)) return;
  const input = event.target.closest('input[type="checkbox"]');
  if (input instanceof HTMLInputElement && !input.disabled) {
    playInterfaceSound('checkbox');
  }
}
