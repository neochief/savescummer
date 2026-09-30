// Dev-only in-browser stand-in for the Tauri host bridge (`pnpm dev` in a plain browser).
// Serves a snapshot of the dev demo host: demo-snapshot.json and art/<game id>/<kind>.*.
import type { Bridge } from '../bridge';
import type { HistoryEntry, HistoryPage, HostState, Operation, SaveTarget, UiRequest } from '../types';
import snapshot from './demo-snapshot.json';

const files = import.meta.glob<string>('./art/*/*', { eager: true, import: 'default', query: '?url' });
const art = (game: string, kind: string) =>
  Object.entries(files).find(([path]) => path.startsWith(`./art/${game}/${kind}.`))?.[1];

export function createMockBridge(): Bridge {
  const state = structuredClone(snapshot.state) as unknown as HostState;
  const history = structuredClone(snapshot.history) as unknown as Record<string, HistoryEntry[]>;
  let seq = 0;
  // A set, like Tauri's listeners: React's dev double-mount unsubscribes the first listener after the second subscribes.
  const stateListeners = new Set<(state: HostState) => void>();
  const publish = () => { state.revision++; for (const listener of stateListeners) listener(structuredClone(state)); };
  const find = (id: string) => state.games.find((g) => g.id === id)!;
  // Deletes run as soon as the UI commits them.
  const deletes = new Map<string, { done: Promise<Operation> }>();
  const dropDelete = (id: string) => { state.deletes = state.deletes.filter((op) => op.id !== id); deletes.delete(id); };
  const startDelete = (game: string, checkpoint: string): Operation => {
    const id = `op-${++seq}`;
    let finish!: (op: Operation) => void;
    const done = new Promise<Operation>((resolve) => { finish = resolve; });
    setTimeout(() => {
      history[game] = history[game].filter((row) => row.checkpoint !== checkpoint);
      find(game).history_version++;
      dropDelete(id);
      publish();
      finish({ id, game, kind: 'delete', status: 'succeeded' });
    }, 400);
    deletes.set(id, { done });
    state.deletes = [...state.deletes, { id, game, kind: 'delete', status: 'running', checkpoint }];
    publish();
    return { id, game, kind: 'delete', status: 'accepted' };
  };

  // Dev fixtures explicitly supply the same capabilities as the real host.
  const lock = (g: HostState['games'][number]) => {
    g.delete = { available: true }; g.flush = { available: true }; g.configure = { available: true };
    g.retry = { available: false };
    g.save = { available: true }; g.load = { available: Boolean(g.latest) }; g.restore = { available: true };
    g.guidance = g.latest ? undefined : { kind: 'no_saves', save: false, load: true };
    if (!g.running || g.expert_mode) return;
    g.save = g.load = g.restore = { available: false, reason: 'game_running' };
    g.guidance = { kind: 'game_running', save: true, load: true };
  };
  state.games.forEach(lock); state.games.forEach((g) => { g.running = false; }); state.active_stack = []; // TEMP-IDLE

  return {
    async request<T>(request: UiRequest): Promise<T> {
      console.debug('[mock bridge]', request);
      await new Promise((r) => setTimeout(r, 120));
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
        case 'save': case 'load': case 'revert': {
          const g = find(request.game);
          if (request.type === 'save') {
            const id = `mock-cp-${++seq}`;
            const at = new Date().toISOString();
            history[g.id].unshift({ id, kind: 'saved', at, checkpoint: id, cloud_replaced: false, actions: { load: true, revert: false, delete: true } });
            g.latest = { id, created_at: at };
            lock(g);
          }
          g.history_version++;
          publish();
          return { id: `op-${++seq}`, game: g.id, kind: request.type, status: 'accepted' } as T;
        }
        case 'configure': {
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
          return { id: request.operation, kind: 'save', status: 'succeeded' } satisfies Operation as T;
        }
        case 'set_label': {
          for (const rows of Object.values(history)) for (const row of rows) if (row.checkpoint === request.checkpoint) row.label = request.label;
          for (const g of state.games) if (g.latest?.id === request.checkpoint) { g.latest.label = request.label; g.labels_version++; publish(); }
          return {} as T;
        }
        case 'open_checkpoints': return { path: `${state.settings?.checkpoint_store}/${request.game}`, opened: !request.resolve_only } as T;
        case 'scan': return { new_games: 0 } as T;
        case 'add_game': {
          const id = `custom-${++seq}`;
          state.games.push({ id, name: request.name, kind: 'custom', executable: request.executable, installed: true, running: false,
            save: { available: true }, load: { available: false }, restore: { available: true }, delete: { available: true }, flush: { available: true }, configure: { available: true }, retry: { available: false }, history_version: 1, labels_version: 0 });
          history[id] = [];
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
    async onState(callback) { stateListeners.add(callback); return () => { stateListeners.delete(callback); }; },
    async onStatus(callback) { setTimeout(() => callback('connected')); return () => undefined; },
    async onLabels() { return () => undefined; },
  };
}
