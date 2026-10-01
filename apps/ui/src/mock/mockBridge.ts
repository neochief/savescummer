// Dev-only in-browser stand-in for the Tauri host bridge (`pnpm dev` in a plain browser).
// Serves a snapshot of the dev demo host: demo-snapshot.json and art/<game id>/<kind>.*,
// reshaped by the `?scenario=` named in scenarios.ts.
import { HostError, platformName, type Bridge } from '../bridge';
import type { HistoryEntry, HistoryPage, HostState, Operation, SaveTarget, UiRequest } from '../types';
import snapshot from './demo-snapshot.json';
import { applyPolicy, type Facts } from './policy';
import { scenarios } from './scenarios';

const files = import.meta.glob<string>('./art/*/*', { eager: true, import: 'default', query: '?url' });
const art = (game: string, kind: string) =>
  Object.entries(files).find(([path]) => path.startsWith(`./art/${game}/${kind}.`))?.[1];

export function createMockBridge(): Bridge {
  const name = new URLSearchParams(location.search).get('scenario') ?? 'library';
  const scenario = scenarios[name] ?? scenarios.library;
  if (!scenarios[name]) console.warn(`[mock bridge] unknown scenario "${name}"; see /scenarios.html`);
  const state = structuredClone(snapshot.state) as unknown as HostState;
  const history = structuredClone(snapshot.history) as unknown as Record<string, HistoryEntry[]>;
  let seq = 0;
  // A set, like Tauri's listeners: React's dev double-mount unsubscribes the first listener after the second subscribes.
  const stateListeners = new Set<(state: HostState) => void>();
  const publish = () => { state.revision++; for (const listener of stateListeners) listener(structuredClone(state)); };
  const find = (id: string) => state.games.find((g) => g.id === id)!;
  const facts: Record<string, Facts> = {};
  const lock = (g: HostState['games'][number]) => applyPolicy(g, facts[g.id], state.phase);
  const ops = new Map<string, Operation>();
  const operation = (game: string, kind: keyof NonNullable<typeof scenario.fail>): Operation => {
    const id = `op-${++seq}`;
    const error = scenario.fail?.[kind];
    ops.set(id, { id, game, kind, status: error ? 'failed' : 'succeeded', error });
    return { id, game, kind, status: 'accepted' };
  };
  // Deletes run as soon as the UI commits them.
  const deletes = new Map<string, { done: Promise<Operation> }>();
  const dropDelete = (id: string) => { state.deletes = state.deletes.filter((op) => op.id !== id); deletes.delete(id); };
  const startDelete = (game: string, checkpoint: string): Operation => {
    const id = `op-${++seq}`;
    let finish!: (op: Operation) => void;
    const done = new Promise<Operation>((resolve) => { finish = resolve; });
    const error = scenario.fail?.delete;
    setTimeout(() => {
      if (!error) history[game] = history[game].filter((row) => row.checkpoint !== checkpoint);
      find(game).history_version++;
      dropDelete(id);
      publish();
      finish({ id, game, kind: 'delete', status: error ? 'failed' : 'succeeded', error });
    }, 400);
    deletes.set(id, { done });
    state.deletes = [...state.deletes, { id, game, kind: 'delete', status: 'running', checkpoint }];
    publish();
    return { id, game, kind: 'delete', status: 'accepted' };
  };

  // Baseline: every game idle and healthy; the scenario then states what differs.
  for (const g of state.games) {
    g.running = false; g.can_close = false;
    facts[g.id] = { hasData: true, hasSaves: Boolean(g.latest) };
  }
  state.active_stack = [];
  scenario.setup?.({ state, facts, game: find });
  if (scenario.focus && !state.active_stack.includes(scenario.focus)) state.active_stack = [scenario.focus, ...state.active_stack];
  state.games.forEach(lock);

  return {
    async request<T>(request: UiRequest): Promise<T> {
      console.debug('[mock bridge]', request);
      if (scenario.offline) return new Promise(() => undefined);
      await new Promise((r) => setTimeout(r, 120));
      const refusal = scenario.refuse?.[request.type];
      if (refusal) throw new HostError(refusal, 'Host rejected the request');
      switch (request.type) {
        case 'state': return structuredClone(state) as T;
        case 'play': case 'close_game': {
          const g = find(request.game);
          if (request.type === 'close_game' && !g.can_close) throw new Error('Closing this game is not enabled');
          g.running = request.type === 'play';
          g.can_close = g.running && Boolean(g.expert_mode);
          state.active_stack = g.running ? [g.id, ...state.active_stack.filter((id) => id !== g.id)] : state.active_stack.filter((id) => id !== g.id);
          lock(g);
          publish();
          return true as T;
        }
        case 'history': {
          const rows = history[request.game] ?? [];
          const start = Number(request.cursor ?? 0);
          const limit = request.limit ?? 100;
          const page: HistoryPage = { rows: rows.slice(start, start + limit), next: start + limit < rows.length ? String(start + limit) : undefined };
          return page as T;
        }
        case 'delete': return startDelete(request.game, request.checkpoint) as T;
        case 'save': case 'load': case 'revert': case 'retry': {
          const g = find(request.game);
          const op = operation(g.id, request.type);
          if (ops.get(op.id)!.error) return op as T;
          if (request.type === 'retry') { facts[g.id].blocked = facts[g.id].recovery = g.blocked = undefined; lock(g); }
          if (request.type === 'save') {
            const id = `mock-cp-${++seq}`;
            const at = new Date().toISOString();
            history[g.id].unshift({ id, kind: 'saved', at, checkpoint: id, cloud_replaced: false, actions: { load: true, revert: false, delete: true } });
            g.latest = { id, created_at: at };
            facts[g.id].hasSaves = true;
            lock(g);
          }
          g.history_version++;
          publish();
          return op as T;
        }
        case 'flush': return operation(request.game, 'flush') as T;
        case 'request_access': {
          const f = facts[request.game];
          for (const key of ['recovery', 'store', 'location', 'target'] as const) if (f[key]?.kind === 'access_needed') f[key] = undefined;
          lock(find(request.game));
          publish();
          return {} as T;
        }
        case 'configure': {
          if (request.save_location) {
            facts[request.game].location = facts[request.game].target = undefined;
            lock(find(request.game));
            publish();
          }
          if (request.expert_mode !== undefined) {
            const g = find(request.game);
            g.expert_mode = request.expert_mode;
            g.can_close = g.running && g.expert_mode;
            lock(g);
            publish();
          }
          return { game: request.game } as T;
        }
        case 'outcome': {
          const pending = deletes.get(request.operation);
          if (pending) return await pending.done as T;
          return (ops.get(request.operation) ?? { id: request.operation, kind: 'save', status: 'succeeded' } satisfies Operation) as T;
        }
        case 'set_label': {
          for (const rows of Object.values(history)) for (const row of rows) if (row.checkpoint === request.checkpoint) row.label = request.label;
          for (const g of state.games) if (g.latest?.id === request.checkpoint) { g.latest.label = request.label; g.labels_version++; publish(); }
          return {} as T;
        }
        case 'picker_start': return { path: request.path || null, exists: Boolean(request.path) } as T;
        case 'open_checkpoints': return { path: `${state.settings?.checkpoint_store}/${request.game}`, opened: !request.resolve_only } as T;
        case 'scan': return { new_games: 0 } as T;
        case 'add_game': {
          const existing = state.games.find((g) => g.executable === request.executable);
          if (existing) return { game: existing.id, existing: true } as T;
          const id = `custom-${++seq}`;
          state.games.push({ id, name: request.name, kind: 'custom', executable: request.executable, installed: true, running: false,
            save: { available: true }, load: { available: false }, restore: { available: true }, delete: { available: true }, flush: { available: true }, configure: { available: true }, retry: { available: false }, history_version: 1, labels_version: 0 });
          history[id] = [];
          facts[id] = { hasData: true, hasSaves: false };
          lock(find(id));
          publish();
          return { game: id } as T;
        }
        case 'save_set': {
          if (state.games.find((g) => g.id === request.game)?.kind === 'custom') {
            return { location: '~/Documents/Saves', active: [{ root: '~/Documents', filter: { kind: 'exact', value: 'Saves' } }] } as T;
          }
          const catalog: SaveTarget[] = [
            { root: '~/Documents/My Games/Example', filter: { kind: 'pattern', value: 'Players/*.plr' } },
            { root: '~/Library/Application Support', filter: { kind: 'exact', value: 'example' }, excludes: ['example/settings.ini'] },
          ];
          return { catalog, active: catalog } as T;
        }
        case 'flush_preview': return { saved: 12, recovery: 3, temporary: 1, size: 48_000_000, items: [] } as T;
        case 'settings': {
          const { type: _, ...changes } = request;
          Object.assign(state.settings!, changes);
          publish();
          return {} as T;
        }
        default: return {} as T;
      }
    },
    async report() {},
    async capture() {},
    async artwork(id, kind) {
      const url = art(id, kind);
      if (!url) throw new Error('artwork is not available');
      const blob = await (await fetch(url)).blob();
      return URL.createObjectURL(blob);
    },
    async openWebsite() { window.open('https://savescummer.app/', '_blank'); },
    async openSaveSearch(engine, game) {
      const question = encodeURIComponent(`What is the save game location of ${game} on ${platformName}`);
      window.open(engine === 'google' ? `https://www.google.com/search?q=${question}` : `https://chatgpt.com/?prompt=${question}`, '_blank');
    },
    async onState(callback) { stateListeners.add(callback); return () => { stateListeners.delete(callback); }; },
    async onStatus(callback) { setTimeout(() => callback('connected')); return () => undefined; },
    async onLabels() { return () => undefined; },
  };
}
