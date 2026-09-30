// Mirror of apps/host/src/policy.rs: derives a game's availability and guidance from observed facts,
// so mock scenarios state *what is wrong* and the UI sees what the real host would send.
import type { Availability, Failure, Game, Guidance, HostState } from '../types';

export interface Facts {
  busy?: boolean;
  blocked?: Failure;
  recovery?: Failure;
  store?: Failure;
  location?: Failure;
  target?: Failure;
  hasData: boolean;
  hasSaves: boolean;
}

type Action = 'save' | 'load' | 'restore' | 'retry' | 'delete' | 'flush' | 'configure';

const details: Record<string, string> = {
  starting: 'the host is still starting',
  shutting_down: 'the host is shutting down',
  busy: 'another operation owns this game or the checkpoint store',
  blocked: 'the game waits on an interrupted operation',
  game_running: 'exit the game before saving, loading or retrying recovery',
  no_game_data: 'no save location matches anything yet',
  no_saves: 'no usable saved checkpoint to load',
  invalid_request: "the game isn't blocked; send the original command again instead",
};
const failure = (kind: string): Failure => ({ kind, detail: details[kind] });

function check(facts: Facts, phase: HostState['phase'], exitLocked: boolean, action: Action): Failure | undefined {
  if (phase !== 'ready') return failure(phase);
  if (facts.busy && action !== 'delete') return failure('busy');
  if (action === 'configure') return;
  if (facts.blocked && action !== 'retry') return failure('blocked');
  if (facts.store) return facts.store;
  if (action === 'retry') {
    if (!facts.blocked) return failure('invalid_request');
    return facts.recovery ?? (exitLocked ? failure('game_running') : undefined);
  }
  if (action === 'delete' || action === 'flush') return;
  if (facts.location) return facts.location;
  if (facts.target) return facts.target;
  if (exitLocked) return failure('game_running');
  if (action === 'save' && !facts.hasData) return failure('no_game_data');
  if (action === 'load' && !facts.hasSaves) return failure('no_saves');
}

function guidance(facts: Facts, exitLocked: boolean): Guidance | undefined {
  if (facts.blocked) {
    const access = [facts.store, facts.recovery].find((f) => f?.kind === 'access_needed');
    return access ? { kind: 'blocked', save: true, load: true, failure: access, remedy: 'request_access' }
      : { kind: 'blocked', save: true, load: true, failure: facts.blocked, remedy: 'retry' };
  }
  const problem = facts.store ?? facts.location ?? facts.target;
  if (problem) {
    const kinds: Record<string, [Guidance['kind'], Guidance['remedy']]> = {
      access_needed: ['access_needed', 'request_access'],
      no_save_location: ['no_save_location', 'configure'],
      invalid_target: ['invalid_target', 'configure'],
      invalid_config: ['invalid_target', 'configure'],
      target_unavailable: ['target_unavailable', undefined],
    };
    const match = kinds[problem.kind];
    if (!match) return; // The checkpoint store has its own app-wide notice.
    return { kind: match[0], save: true, load: true, failure: problem, remedy: match[1] };
  }
  if (exitLocked) return { kind: 'game_running', save: true, load: true };
  if (facts.hasData && facts.hasSaves) return;
  return { kind: !facts.hasData ? (facts.hasSaves ? 'no_game_data' : 'play_first') : 'no_saves', save: !facts.hasData, load: !facts.hasSaves };
}

/** Rewrites the game's capabilities and guidance, as the host does on every state publish. */
export function applyPolicy(game: Game, facts: Facts, phase: HostState['phase']) {
  const exitLocked = game.running && !game.expert_mode;
  const availability = (action: Action): Availability => {
    const f = check(facts, phase, exitLocked, action);
    return f ? { available: false, reason: f.kind, failure: f } : { available: true };
  };
  for (const action of ['save', 'load', 'restore', 'retry', 'delete', 'flush', 'configure'] as const) game[action] = availability(action);
  game.guidance = guidance(facts, exitLocked);
}
