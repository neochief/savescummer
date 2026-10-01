// Named UI states for the dev mock bridge: open `pnpm dev` with `?scenario=<name>`, or browse them at /scenarios.html.
// Scenarios describe host *facts*; policy.ts turns them into the availability and guidance the real host would publish.
import type { Failure, Game, HostState, UiRequest } from '../types';
import type { Facts } from './policy';

export interface Preview {
  state: HostState;
  facts: Record<string, Facts>;
  game(id: string): Game;
}

export interface Scenario {
  title: string;
  /** The game selected when the preview opens. */
  focus?: string;
  setup?(preview: Preview): void;
  /** Requests the host refuses outright. */
  refuse?: Partial<Record<UiRequest['type'], Failure>>;
  /** Operations the host accepts but that finish failed, by operation kind. */
  fail?: Partial<Record<'save' | 'load' | 'revert' | 'retry' | 'delete' | 'flush', Failure>>;
  /** The host never answers. */
  offline?: boolean;
}

const FTL = 'steam-212680';
const RISK_OF_RAIN = 'steam-1337520';
const CRUSADER_KINGS = 'steam-203770';
const ISAAC = 'steam-250900';
const GUNGEON = 'steam-311690';
const SLAY_THE_SPIRE = 'steam-646570';

const accessNeeded = (denied: boolean): Failure => ({ kind: 'access_needed',
  access: { category: 'documents', denied, settings_url: 'x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders' } });
const interrupted: Failure = { kind: 'swap_failed', detail: 'the load stopped while replacing save files' };

const run = (game: string, expert = false) => ({ state, game: get }: Preview) => {
  const g = get(game);
  g.running = true;
  g.expert_mode = expert;
  g.can_close = expert;
  state.active_stack = [game];
};
const block = (game: string, recovery?: Failure) => ({ facts, game: get }: Preview) => {
  facts[game].blocked = get(game).blocked = interrupted;
  facts[game].recovery = recovery;
};

export const scenarios: Record<string, Scenario> = {
  library: { title: 'Library, nothing selected' },
  'no-games': { title: 'Empty library', setup: ({ state }) => { state.games = []; } },
  offline: { title: 'Waiting for the host', offline: true },
  starting: { title: 'Host still starting', focus: FTL, setup: ({ state }) => { state.phase = 'starting'; } },

  playing: { title: 'Game running', focus: FTL, setup: run(FTL) },
  'playing-first': { title: 'Running, no progress or checkpoints', focus: SLAY_THE_SPIRE, setup: (preview) => {
    run(SLAY_THE_SPIRE)(preview);
    preview.facts[SLAY_THE_SPIRE].hasData = false;
  } },
  'playing-load': { title: 'Running, checkpoints but no progress', focus: GUNGEON, setup: (preview) => {
    run(GUNGEON)(preview);
    preview.facts[GUNGEON].hasData = false;
  } },
  'playing-save-first': { title: 'Running, progress but no checkpoints', focus: SLAY_THE_SPIRE, setup: run(SLAY_THE_SPIRE) },
  'playing-expert': { title: 'Game running, expert mode', focus: FTL, setup: run(FTL, true) },
  busy: { title: 'Save in progress', focus: FTL, setup: ({ facts, game }) => {
    facts[FTL].busy = true;
    game(FTL).busy = { id: 'op-busy', game: FTL, kind: 'save', status: 'running' };
  } },

  'no-checkpoints': { title: 'No checkpoints yet', focus: SLAY_THE_SPIRE },
  'play-first': { title: 'Nothing played yet', focus: SLAY_THE_SPIRE, setup: ({ facts }) => { facts[SLAY_THE_SPIRE].hasData = false; } },
  'no-game-data': { title: 'Save files gone, checkpoints kept', focus: GUNGEON, setup: ({ facts }) => { facts[GUNGEON].hasData = false; } },

  recovery: { title: 'Interrupted operation, recovery available', focus: FTL, setup: block(FTL) },
  'recovery-stuck': { title: 'Interrupted operation, recovery fails', focus: FTL,
    setup: block(FTL, { kind: 'in_use', detail: 'Steam is holding continue.sav open. Quit Steam and try again.' }) },
  'recovery-access': { title: 'Interrupted operation, needs folder access', focus: FTL, setup: block(FTL, accessNeeded(false)) },

  'access-needed': { title: 'Needs Documents access', focus: CRUSADER_KINGS, setup: ({ facts }) => { facts[CRUSADER_KINGS].location = accessNeeded(false); } },
  'access-denied': { title: 'Documents access denied', focus: CRUSADER_KINGS, setup: ({ facts }) => { facts[CRUSADER_KINGS].location = accessNeeded(true); } },
  'no-save-location': { title: 'Save folder unknown', focus: ISAAC, setup: ({ facts }) => { facts[ISAAC].location = { kind: 'no_save_location' }; } },
  'invalid-overlap': { title: 'Save folder overlaps another game', focus: ISAAC, setup: ({ facts }) => {
    facts[ISAAC].target = { kind: 'invalid_target', paths: ['~/Documents/Paradox Interactive'], target_cause: { kind: 'overlap', name: 'Crusader Kings II' } };
  } },
  'invalid-broad': { title: 'Save folder too broad', focus: ISAAC, setup: ({ facts }) => {
    facts[ISAAC].target = { kind: 'invalid_target', paths: ['~/Documents'], target_cause: { kind: 'too_broad' } };
  } },
  'target-unavailable': { title: 'Save folder on a disconnected drive', focus: RISK_OF_RAIN, setup: ({ facts }) => {
    facts[RISK_OF_RAIN].target = { kind: 'target_unavailable', paths: ['/Volumes/Games/Risk of Rain Returns/Saves'] };
  } },
  'store-unavailable': { title: 'Checkpoint store unavailable', focus: FTL, setup: ({ state, facts }) => {
    state.store = { path: state.store?.path ?? '', available: false };
    for (const f of Object.values(facts)) f.store = { kind: 'store_unavailable' };
  } },

  'last-failed': { title: 'Previous operation failed', focus: FTL, setup: ({ game }) => {
    game(FTL).last_result = { id: 'op-old', game: FTL, kind: 'load', status: 'failed',
      error: { kind: 'disk_full', detail: 'Not enough disk space to load this checkpoint (needs 1.2 GB).' } };
  } },
  'save-fails': { title: 'Save fails (click Save)', focus: FTL,
    fail: { save: { kind: 'in_use', detail: 'The game is still writing continue.sav. Try again in a moment.' } } },
  'save-refused': { title: 'Save refused (click Save)', focus: FTL, refuse: { save: { kind: 'game_running' } } },
  'load-fails': { title: 'Load fails (click Load)', focus: FTL,
    fail: { load: { kind: 'checkpoint_unreadable', detail: 'This checkpoint is damaged and cannot be read.' } } },
  'delete-fails': { title: 'Delete fails (delete a row)', focus: FTL,
    fail: { delete: { kind: 'delete_mismatch', detail: 'Checkpoint files changed on disk; nothing was deleted.' } } },
  'scan-fails': { title: 'Scan fails (click Scan for games)', refuse: { scan: { kind: 'io', detail: 'The Steam library folder is unreadable.' } } },

  trouble: { title: 'Every problem at once, one per game', setup: (preview) => {
    run(GUNGEON)(preview);
    preview.state.active_stack = [];
    block(FTL)(preview);
    preview.facts[CRUSADER_KINGS].location = accessNeeded(false);
    preview.facts[ISAAC].location = { kind: 'no_save_location' };
    preview.facts[RISK_OF_RAIN].target = { kind: 'target_unavailable', paths: ['/Volumes/Games/Risk of Rain Returns/Saves'] };
    preview.facts[SLAY_THE_SPIRE].hasData = false;
  } },
};
