export interface Failure {
  kind: string;
  detail?: string;
  game?: string;
}

export interface Operation {
  id: string;
  game?: string;
  kind: string;
  status: 'accepted' | 'running' | 'counting_down' | 'waiting' | 'succeeded' | 'failed' | 'cancelled';
  error?: Failure;
  remaining_ms?: number;
  checkpoint?: string;
}

export interface Artwork {
  hero?: string;
  logo?: string;
  header?: string;
  icon?: string;
}

export interface Game {
  id: string;
  name: string;
  install_tag?: string;
  installed: boolean;
  running: boolean;
  /** Save, Load and Revert are refused while the game runs (`game_running`): it writes its progress only on exit. */
  wait_for_exit?: boolean;
  kind?: 'known' | 'custom';
  executable?: string;
  executable_overridden?: boolean;
  info?: string;
  checkpoints_size?: number;
  has_history?: boolean;
  blocked?: Failure;
  config_error?: Failure;
  save: { available: boolean; reason?: string };
  load: { available: boolean; reason?: string };
  latest?: { id: string; label?: string; created_at: string };
  busy?: Operation;
  last_result?: Operation;
  artwork?: Artwork;
  history_version: number;
  labels_version: number;
}

export interface HostState {
  instance: string;
  revision: number;
  phase: 'starting' | 'ready' | 'shutting_down';
  games: Game[];
  active_stack: string[];
  deletes: Operation[];
  settings?: { play_sounds: boolean; launch_on_startup: boolean; launch_on_startup_available: boolean; launch_on_startup_needs_approval?: boolean;
    checkpoint_store: string; save_shortcut?: string; load_shortcut?: string };
}

export interface SaveTarget {
  root: string;
  filter: { kind: 'all' } | { kind: 'exact' | 'pattern'; value: string };
  excludes?: string[];
}

export interface SaveSet {
  catalog?: SaveTarget[];
  location?: string;
  active: SaveTarget[];
  catalog_problem?: string;
}

export interface FlushPreview {
  saved: number;
  recovery: number;
  temporary: number;
  size: number;
  items: Array<{ path: string; kind: string; label?: string }>;
  next?: string;
}

export interface HistoryEntry {
  id: string;
  kind: 'saved' | 'loaded' | 'reverted' | 'game_started' | 'game_closed';
  at: string;
  checkpoint?: string;
  restored?: string;
  label?: string;
  saved_at?: string;
  reverted_at?: string;
  removed_files?: number;
  cloud_replaced: boolean;
  actions: { load: boolean; revert: boolean; delete: boolean };
}

export interface HistoryPage {
  rows: HistoryEntry[];
  next?: string;
}

export type UiRequest =
  | { type: 'state' }
  | { type: 'history'; game: string; cursor?: string; limit?: number }
  | { type: 'save'; game: string }
  | { type: 'load'; game: string; checkpoint?: string }
  | { type: 'revert'; game: string; checkpoint: string }
  | { type: 'delete'; game: string; checkpoint: string }
  | { type: 'cancel_delete'; operation: string }
  | { type: 'set_label'; checkpoint: string; label?: string }
  | { type: 'scan' }
  | { type: 'outcome'; operation: string }
  | { type: 'add_game'; name: string; executable: string; save_location: string }
  | { type: 'configure'; game: string; name?: string; executable?: string; save_location?: string; reset_executable: boolean; reset_save_location: boolean; wait_for_exit?: boolean }
  | { type: 'save_set'; game: string }
  | { type: 'settings'; play_sounds?: boolean; launch_on_startup?: boolean; save_shortcut?: string; load_shortcut?: string }
  | { type: 'flush_preview'; game: string; cursor?: string; limit?: number }
  | { type: 'flush'; game: string }
  | { type: 'open_checkpoints'; game: string; resolve_only?: boolean }
  | { type: 'open_executable'; game: string }
  | { type: 'open_saves'; game: string; target: number };

export interface HostResponse<T> {
  v: number;
  re: string;
  ok: boolean;
  result?: T;
  error?: Failure;
}
