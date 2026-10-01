import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { App, relativeAge } from './App';
import { copyrightYears } from './Dialogs';
import { displayShortcut } from './shortcuts/shortcuts';
import { HostError, type Bridge } from './bridge';
import type { Failure, Game, HistoryEntry, HistoryPage, HostState, Operation, SaveSet, SaveTarget, UiRequest } from './types';

const dialogOpen = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: dialogOpen }));

afterEach(() => { cleanup(); dialogOpen.mockReset(); });
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});

const game = (id: string, name: string): Game => ({
  id, name, installed: true, running: false,
  save: { available: true }, load: { available: true }, restore: { available: true },
  delete: { available: true }, flush: { available: true }, configure: { available: true }, retry: { available: false },
  latest: { id: `cp-${id}`, created_at: '2026-09-27T12:00:00Z' },
  history_version: 1, labels_version: 0,
});

const row = (id: string, label: string): HistoryEntry => ({
  id, kind: 'saved', at: '2026-09-27T12:00:00Z', checkpoint: id,
  label, cloud_replaced: false, actions: { load: true, revert: false, delete: true },
});

class FakeBridge implements Bridge {
  state: HostState = {
    instance: 'test-host', revision: 1, phase: 'ready', active_stack: ['a'], deletes: [],
    games: [game('a', 'Game A'), game('b', 'Game B')],
  };
  pages: Record<string, HistoryPage> = {
    a: { rows: [row('cp-a', 'First checkpoint')] },
    b: { rows: [row('cp-b', 'Second checkpoint')] },
  };
  outcome: Promise<Operation> = Promise.resolve({ id: 'op-1', kind: 'save', status: 'succeeded' });
  scanResult: Promise<{ new_games: number }> = Promise.resolve({ new_games: 0 });
  settingsError?: string;
  addRefusal?: Failure;
  requests: UiRequest[] = [];
  stateListener?: (state: HostState) => void;
  statusListener?: (status: string) => void;

  async request<T>(request: UiRequest): Promise<T> {
    this.requests.push(request);
    if (request.type === 'state') return this.state as T;
    if (request.type === 'scan') return this.scanResult as Promise<T>;
    if (request.type === 'history') return this.pages[request.game] as T;
    if (request.type === 'save' || request.type === 'load' || request.type === 'delete' || request.type === 'revert' || request.type === 'retry') {
      return { id: 'op-1', kind: request.type, status: 'accepted' } as T;
    }
    if (request.type === 'set_label') return {} as T;
    if (request.type === 'add_game') {
      if (this.addRefusal) throw new HostError(this.addRefusal, 'Host rejected the request');
      return { game: 'custom-1' } as T;
    }
    if (request.type === 'open_checkpoints') return { path: `/store/${request.game}`, opened: !request.resolve_only } as T;
    if (request.type === 'save_set') return this.saveSet as T;
    if (request.type === 'flush_preview') return { saved: 1, recovery: 0, temporary: 0, size: 4096, items: [] } as T;
    if (request.type === 'settings' && this.settingsError) throw new Error(this.settingsError);
    if (request.type === 'settings' || request.type === 'configure') return {} as T;
    return this.outcome as Promise<T>;
  }
  saveSet: SaveSet = { location: '/old/saves', active: [{ root: '/old', filter: { kind: 'exact', value: 'saves' } }] };
  report = vi.fn(async () => undefined);
  capture = vi.fn(async () => undefined);
  artwork = vi.fn(async () => 'blob:fake');
  openWebsite = vi.fn(async () => undefined);
  openSaveSearch = vi.fn(async () => undefined);
  async onState(callback: (state: HostState) => void) { this.stateListener = callback; return () => { this.stateListener = undefined; }; }
  async onStatus(callback: (status: string) => void) { this.statusListener = callback; return () => { this.statusListener = undefined; }; }
  async onLabels() { return () => undefined; }
}

test('selecting another game reads that game’s real history page', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  expect(await screen.findByText('First checkpoint', {}, { timeout: 5000 })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: /Game B, Not running/ }));
  expect(await screen.findByText('Second checkpoint', {}, { timeout: 5000 })).toBeTruthy();
  expect(screen.queryByText('First checkpoint')).toBeNull();
  expect(bridge.requests).toContainEqual({ type: 'history', game: 'b', limit: 100 });
});

test('cards show Play while stopped and expose Terminate only when the host permits it', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].can_play = true;
  render(<App bridge={bridge} />);
  const play = await screen.findByRole('button', { name: 'Play Game A' });
  fireEvent.click(play);
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'play', game: 'a' }));

  act(() => bridge.stateListener?.({ ...bridge.state, revision: 2,
    games: [{ ...bridge.state.games[0], running: true, can_close: false }, bridge.state.games[1]] }));
  expect(screen.queryByRole('button', { name: 'Play Game A' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Terminate Game A' })).toBeNull();

  act(() => bridge.stateListener?.({ ...bridge.state, revision: 3,
    games: [{ ...bridge.state.games[0], running: true, can_close: true }, bridge.state.games[1]] }));
  fireEvent.click(screen.getByRole('button', { name: 'Terminate Game A' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'close_game', game: 'a' }));

  act(() => bridge.stateListener?.({ ...bridge.state, revision: 4,
    games: [bridge.state.games[0], bridge.state.games[1]] }));
  expect(screen.getByRole('button', { name: 'Play Game A' })).toBeTruthy();
});

test('play-first guidance launches the game through the shared play action', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].guidance = { kind: 'play_first', save: true, load: true };
  bridge.state.games[0].can_play = true;
  render(<App bridge={bridge} />);

  const panel = await screen.findByRole('status', { name: 'Play the game first to save progress' });
  const play = within(panel).getByRole('button', { name: 'Play game' }) as HTMLButtonElement;
  fireEvent.click(play);
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'play', game: 'a' }));
  expect(play.disabled).toBe(true);
  expect((screen.getByRole('button', { name: 'Play Game A' }) as HTMLButtonElement).disabled).toBe(true);
});

test('Info reveals the full game instructions only when requested', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].info = 'Context: Keep this run.\n\nHow progress is saved: The save is overwritten.';
  render(<App bridge={bridge} />);
  const button = await screen.findByRole('button', { name: 'Info' });
  expect(button.getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByText(/How progress is saved/)).toBeNull();

  fireEvent.click(button);
  expect(button.getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByText(/How progress is saved: The save is overwritten/)).toBeTruthy();

  fireEvent.click(button);
  expect(button.getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByText(/How progress is saved/)).toBeNull();
});

test('a game starting or closing does not replace the selected view', async () => {
  const bridge = new FakeBridge();
  bridge.state.games = [{ ...game('a', 'Game A'), running: true }, game('b', 'Game B')];
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getByRole('button', { name: 'Game A' }));

  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2, active_stack: ['b', 'a'],
    games: bridge.state.games.map((value) => ({ ...value, running: true })) }));
  expect(screen.getByRole('button', { name: 'Game A' }).getAttribute('aria-current')).toBe('true');

  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 3, active_stack: ['b'],
    games: [game('a', 'Game A'), { ...game('b', 'Game B'), running: true }] }));
  expect(screen.getByRole('button', { name: /Game A, Not running/ }).getAttribute('aria-current')).toBe('true');
});

test('focusing another already running game selects it', async () => {
  const bridge = new FakeBridge();
  bridge.state.games = bridge.state.games.map((value) => ({ ...value, running: true }));
  bridge.state.active_stack = ['a', 'b'];
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2, active_stack: ['b', 'a'] }));
  expect(screen.getByRole('button', { name: 'Game B' }).getAttribute('aria-current')).toBe('true');
});

test('Save stays busy until the host outcome succeeds', async () => {
  const bridge = new FakeBridge();
  let finish!: (operation: Operation) => void;
  bridge.outcome = new Promise((resolve) => { finish = resolve; });
  render(<App bridge={bridge} />);
  const button = await screen.findByRole('button', { name: 'save Game A' });
  fireEvent.click(button);
  await waitFor(() => expect(button.classList.contains('busy')).toBe(true));
  expect((button as HTMLButtonElement).disabled).toBe(true);
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'outcome', operation: 'op-1' }));
  expect(screen.queryByText('DONE')).toBeNull();
  finish({ id: 'op-1', kind: 'save', status: 'succeeded' });
  await waitFor(() => expect(button.classList.contains('success')).toBe(true));
  expect((button as HTMLButtonElement).disabled).toBe(false);
});

test('a running game that saves on exit covers the actions and hides row loads until it has exited', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  const locked = { available: false, reason: 'game_running' };
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2,
    games: bridge.state.games.map((game) => game.id === 'a' ? { ...game, running: true, save: locked, load: locked, restore: locked, guidance: { kind: 'game_running', save: true, load: true } } : game) }));
  const panel = screen.getByRole('status', { name: 'Exit the game first' });
  expect(panel.querySelector('strong')?.textContent).toContain('Exit the game');
  expect((screen.getByRole('button', { name: 'save Game A' }) as HTMLButtonElement).disabled).toBe(true);
  expect(document.querySelectorAll('.row-button:not(.locked)')).toHaveLength(0);

  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 3 }));
  expect(screen.queryByRole('status', { name: 'Exit the game first' })).toBeNull();
  expect(document.querySelectorAll('.row-button.locked')).toHaveLength(0);
});

test('Configure turns Expert mode on', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0] = { ...bridge.state.games[0], executable: '/games/a.exe', expert_mode: false };
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getByRole('button', { name: 'Configure Game A' }));
  const dialog = await screen.findByRole('dialog', { name: 'Configure Game A' });
  const check = within(dialog).getByRole('checkbox', { name: 'Expert mode' }) as HTMLInputElement;
  expect(check.checked).toBe(false);
  expect(within(dialog).getByText('Allow terminating a running game from its game card.')).toBeTruthy();
  fireEvent.click(check);
  fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(bridge.requests).toContainEqual(expect.objectContaining({ type: 'configure', game: 'a', expert_mode: true })));
});

test('stable guidance survives Delete while history follows the current host gate', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].guidance = { kind: 'game_running', save: true, load: true };
  bridge.state.games[0].restore = { available: false, reason: 'game_running' };
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2, games: [
    { ...bridge.state.games[0], save: { available: false, reason: 'busy' },
      busy: { id: 'delete-1', kind: 'delete', status: 'running' } }, bridge.state.games[1],
  ] }));
  expect(screen.getByRole('status', { name: 'Exit the game first' })).toBeTruthy();
  expect((screen.getByRole('button', { name: /Load save from/ }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 3, games: [game('a', 'Game A'), game('b', 'Game B')] }));
  expect((screen.getByRole('button', { name: /Load save from/ }) as HTMLButtonElement).disabled).toBe(false);
  expect(bridge.requests.filter((r) => r.type === 'history')).toHaveLength(1);
});

test('Retry uses a host operation and keeps recovery guidance while running', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].guidance = { kind: 'blocked', save: true, load: true, remedy: 'retry' };
  bridge.state.games[0].retry = { available: true };
  let finish!: (operation: Operation) => void;
  bridge.outcome = new Promise((resolve) => { finish = resolve; });
  render(<App bridge={bridge} />);
  await screen.findByRole('status', { name: 'Recover the interrupted operation' });
  expect(document.querySelectorAll('.action-slot .shortcut-tab')).toHaveLength(0);
  fireEvent.click(await screen.findByRole('button', { name: 'Try recovery again' }));
  expect(await screen.findByRole('button', { name: 'Recovering' })).toBeTruthy();
  expect(screen.getByRole('status', { name: 'Recover the interrupted operation' })).toBeTruthy();
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'retry', game: 'a' }));
  finish({ id: 'op-1', kind: 'retry', status: 'succeeded' });
  await waitFor(() => expect((screen.getByRole('button', { name: 'Try recovery again' }) as HTMLButtonElement).disabled).toBe(false));
});

test('permission remedies use host commands, including the denied settings page', async () => {
  const bridge = new FakeBridge();
  const access = { category: 'documents', denied: false, settings_url: 'host-owned' };
  bridge.state.games[0].guidance = { kind: 'access_needed', save: true, load: true, remedy: 'request_access',
    failure: { kind: 'access_needed', access } };
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Allow access' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'request_access', game: 'a' }));
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2, games: [
    { ...bridge.state.games[0], guidance: { kind: 'access_needed', save: true, load: true, remedy: 'request_access',
      failure: { kind: 'access_needed', access: { ...access, denied: true } } } }, bridge.state.games[1],
  ] }));
  fireEvent.click(await screen.findByRole('button', { name: 'Open System Settings' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'open_access_settings', game: 'a' }));
});

test('Browse saves a missing game location directly', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].guidance = { kind: 'no_save_location', save: true, load: true, remedy: 'configure' };
  dialogOpen.mockResolvedValue('/games/a/saves');
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Browse' }));
  expect(dialogOpen).toHaveBeenCalledWith({ title: 'Choose save folder', directory: true,
    multiple: false, fileAccessMode: 'scoped' });
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'configure', game: 'a',
    save_location: '/games/a/saves', reset_executable: false, reset_save_location: false }));
  expect(screen.queryByRole('dialog', { name: /Configure/ })).toBeNull();
});

test('a Load-only empty state leaves Save usable', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].load = { available: false, reason: 'no_saves' };
  bridge.state.games[0].guidance = { kind: 'no_saves', save: false, load: true };
  bridge.state.games[0].latest = undefined;
  render(<App bridge={bridge} />);
  const panel = await screen.findByRole('status', { name: 'No checkpoints yet' });
  expect(panel.classList.contains('covers-load')).toBe(true);
  expect(panel.textContent).toContain('Go and play the game first.');
  expect(document.querySelector('.action-slot:first-child .shortcut-tab')).toBeTruthy();
  expect(document.querySelector('.action-slot:nth-child(2) .shortcut-tab')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'save Game A' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'save', game: 'a' }));
});

test('guidance hides only the shortcut for its covered action', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].guidance = { kind: 'no_game_data', save: true, load: false };
  render(<App bridge={bridge} />);
  await screen.findByRole('status', { name: 'No game data to save' });
  const save = document.querySelector('.action-slot:first-child')!;
  const load = document.querySelector('.action-slot:nth-child(2)')!;
  expect(save.hasAttribute('inert')).toBe(true);
  expect((save.querySelector('button') as HTMLButtonElement).disabled).toBe(true);
  expect(save.querySelector('.shortcut-tab')).toBeNull();
  expect(load.hasAttribute('inert')).toBe(false);
  expect((load.querySelector('button') as HTMLButtonElement).disabled).toBe(false);
  expect(load.querySelector('.shortcut-tab')).toBeTruthy();
});

test('host rejection is shown on the initiating control', async () => {
  const bridge = new FakeBridge();
  bridge.outcome = Promise.resolve({ id: 'op-1', kind: 'load', status: 'failed', error: { kind: 'io', detail: 'copy failed' } });
  render(<App bridge={bridge} />);
  const button = await screen.findByRole('button', { name: 'load Game A' });
  fireEvent.click(button);
  expect(await screen.findByText('FAILED')).toBeTruthy();
  expect(screen.getByRole('alert').textContent).toContain('copy failed');
});

test('a disabled Save has no shortcut and guarded app data has a readable error', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].save = { available: false, reason: 'access_needed' };
  bridge.state.games[0].guidance = { kind: 'access_needed', save: true, load: true, remedy: 'request_access', failure: { kind: 'access_needed', detail: 'app_data' } };
  render(<App bridge={bridge} />);
  const button = await screen.findByRole('button', { name: 'save Game A' });
  expect((button as HTMLButtonElement).disabled).toBe(true);
  expect(button.closest('.action-slot')?.querySelector('.shortcut-tab')).toBeNull();
  expect(screen.getByRole('status', { name: 'Allow access to this game’s saves' }).querySelector('.guidance-description')).toBeNull();
});

test('an access error from an operation is translated for the error block', async () => {
  const bridge = new FakeBridge();
  bridge.outcome = Promise.resolve({ id: 'op-1', kind: 'save', status: 'failed', error: { kind: 'access_needed', detail: 'app_data' } });
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'save Game A' }));
  expect((await screen.findByRole('alert')).textContent).toBe("SaveScummer needs permission to access other apps' data.");
});

test('relative age uses five-second steps and stops at 24 hours', () => {
  const now = new Date('2026-09-27T12:00:00').getTime();
  const before = (seconds: number) => new Date(now - seconds * 1000);
  expect(relativeAge(before(59), now)).toBe('55s ago');
  expect(relativeAge(before(60), now)).toBe('1m ago');
  expect(relativeAge(before(3599), now)).toBe('59m ago');
  expect(relativeAge(before(3600), now)).toBe('1h ago');
  expect(relativeAge(before(86399), now)).toBe('23h ago');
  expect(relativeAge(before(86400), now)).toBe('');
  expect(relativeAge(new Date(now + 1000), now)).toBe('');
});

test('delete hides the row immediately and Undo restores it with the row animation', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].info = 'Game instructions';
  bridge.pages.a.rows.push({ ...row('cp-older', 'Older checkpoint'), at: '2026-09-27T11:00:00Z' });
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getAllByRole('button', { name: /Delete checkpoint from/ })[0]);
  expect(screen.queryByText('First checkpoint')).toBeNull();
  const undo = screen.getByRole('button', { name: 'Undo' });
  const controls = undo.parentElement;
  expect(controls?.firstElementChild).toBe(screen.getByRole('button', { name: 'Info' }));
  expect(controls?.lastElementChild).toBe(undo);
  expect(undo.classList.contains('undo-button')).toBe(true);
  expect(bridge.requests).not.toContainEqual({ type: 'delete', game: 'a', checkpoint: 'cp-a' });
  fireEvent.click(undo);
  expect(screen.getByText('First checkpoint').closest('.history-row')?.classList.contains('arrived')).toBe(true);
  expect(screen.getByText('First checkpoint').closest('.history-row')?.classList.contains('flash')).toBe(true);
  expect([...document.querySelectorAll('.history-row')].map((element) => element.textContent).join('|'))
    .toMatch(/First checkpoint.*\|.*Older checkpoint/);
  expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull();
  expect(bridge.requests).not.toContainEqual({ type: 'delete', game: 'a', checkpoint: 'cp-a' });
});

test('only the most recently hidden row can be undone', async () => {
  const bridge = new FakeBridge();
  bridge.pages.a.rows.push({ ...row('cp-older', 'Older checkpoint'), at: '2026-09-27T11:00:00Z' });
  bridge.outcome = new Promise(() => undefined);
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getAllByRole('button', { name: /Delete checkpoint from/ })[0]);
  fireEvent.click(screen.getByRole('button', { name: /Delete checkpoint from/ }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'delete', game: 'a', checkpoint: 'cp-a' }));
  expect(screen.queryByText('First checkpoint')).toBeNull();
  expect(screen.queryByText('Older checkpoint')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
  expect(screen.getByText('Older checkpoint')).toBeTruthy();
  expect(screen.queryByText('First checkpoint')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull();
});

test('Undo expires after five seconds even when hovered', async () => {
  const bridge = new FakeBridge();
  bridge.outcome = new Promise(() => undefined);
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  vi.useFakeTimers();
  try {
    fireEvent.click(screen.getByRole('button', { name: /Delete checkpoint from/ }));
    act(() => vi.advanceTimersByTime(2500));
    const undo = screen.getByRole('button', { name: 'Undo' });
    fireEvent.mouseEnter(undo);
    act(() => vi.advanceTimersByTime(2499));
    fireEvent.mouseLeave(undo);
    expect(undo).toBe(screen.getByRole('button', { name: 'Undo' }));
    expect(bridge.requests).not.toContainEqual({ type: 'delete', game: 'a', checkpoint: 'cp-a' });
    act(() => vi.advanceTimersByTime(1));
    expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull();
    expect(bridge.requests).toContainEqual({ type: 'delete', game: 'a', checkpoint: 'cp-a' });
  } finally {
    vi.useRealTimers();
  }
});

test('saved checkpoint labels are edited through the host', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Edit checkpoint label' }));
  const input = screen.getByRole('textbox', { name: 'Checkpoint label' });
  fireEvent.change(input, { target: { value: '  Before boss  ' } });
  fireEvent.keyDown(input, { key: 'Enter' });
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'set_label', checkpoint: 'cp-a', label: 'Before boss' }));
});

test('Add custom game submits entered paths through the host bridge', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Add custom game' }));
  expect((screen.getByLabelText('Game executable') as HTMLInputElement).value).toBe('');
  fireEvent.change(screen.getByLabelText('Game executable'), { target: { value: '/games/example' } });
  fireEvent.change(screen.getByLabelText('Where the game keeps its save files'), { target: { value: '/games/example/saves' } });
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Example' } });
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'add_game', name: 'Example', executable: '/games/example', save_location: '/games/example/saves' }));
});

test('Add custom game explains a Windows path on another platform once the field is left, and does not submit it', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Add custom game' }));
  const location = screen.getByLabelText('Where the game keeps its save files');
  fireEvent.change(screen.getByLabelText('Game executable'), { target: { value: '/games/example' } });
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Example' } });
  fireEvent.change(location, { target: { value: 'C:\\Users\\me\\Saved Games\\Example' } });
  expect(screen.queryByRole('alert')).toBeNull();
  fireEvent.blur(location);
  expect(screen.getByRole('alert').textContent).toContain('This is where the game keeps saves on Windows.');
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));
  expect(bridge.requests.some((request) => request.type === 'add_game')).toBe(false);
  fireEvent.change(location, { target: { value: '/games/example/saves' } });
  expect(screen.queryByRole('alert')).toBeNull();
});

test('Add custom game sends portable home paths as typed, without calling them another platform’s', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Add custom game' }));
  const executable = screen.getByLabelText('Game executable');
  const location = screen.getByLabelText('Where the game keeps its save files');
  fireEvent.change(executable, { target: { value: '~/Games/Example/Example' } });
  fireEvent.blur(executable);
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Example' } });
  fireEvent.change(location, { target: { value: '~/.local/share/Example/*.sav' } });
  fireEvent.blur(location);
  expect(screen.queryByRole('alert')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'add_game', name: 'Example',
    executable: '~/Games/Example/Example', save_location: '~/.local/share/Example/*.sav' }));
});

test('Add custom game reads a Terminal-escaped or quoted paste as the path it names', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Add custom game' }));
  const executable = screen.getByLabelText('Game executable') as HTMLInputElement;
  const location = screen.getByLabelText('Where the game keeps its save files') as HTMLInputElement;
  fireEvent.change(executable, { target: { value: '"~/Games/Hades/Hades"' } });
  fireEvent.blur(executable);
  expect(executable.value).toBe('~/Games/Hades/Hades');
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Hades' } });
  fireEvent.change(location, { target: { value: '~/.local/share/Supergiant\\ Games/Hades' } });
  fireEvent.blur(location);
  expect(location.value).toBe('~/.local/share/Supergiant Games/Hades');
  expect(screen.queryByRole('alert')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'add_game', name: 'Hades',
    executable: '~/Games/Hades/Hades', save_location: '~/.local/share/Supergiant Games/Hades' }));
});

test('Add custom game explains a save location the host finds too broad under that field', async () => {
  const bridge = new FakeBridge();
  bridge.addRefusal = { kind: 'invalid_target', target_cause: { kind: 'too_broad' }, paths: ['/home', '~'] };
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Add custom game' }));
  fireEvent.change(screen.getByLabelText('Game executable'), { target: { value: '~/Games/Hades/Hades' } });
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Hades' } });
  const location = screen.getByLabelText('Where the game keeps its save files');
  fireEvent.change(location, { target: { value: '~' } });
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));
  const alert = await screen.findByRole('alert');
  expect(alert.textContent).toContain('This is your whole home folder, and many apps keep files there. Pick the folder inside it where Hades keeps its saves.');
  expect(location.getAttribute('aria-invalid')).toBe('true');
  expect(screen.getByRole('dialog', { name: 'Add custom game' })).toBeTruthy();
  fireEvent.change(location, { target: { value: '~/.local/share/Hades' } });
  expect(screen.queryByRole('alert')).toBeNull();
});

test('Configure shows the host’s portable paths and returns them untouched', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0] = { ...bridge.state.games[0], kind: 'custom', executable: '~/Games/A/a', executable_overridden: true };
  bridge.saveSet = { location: '~/Saves/A/*.sav', active: [{ root: '~/Saves/A', filter: { kind: 'pattern', value: '*.sav' } }] };
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getByRole('button', { name: 'Configure Game A' }));
  const dialog = await screen.findByRole('dialog', { name: 'Configure Game A' });
  const executable = within(dialog).getByLabelText('Game executable') as HTMLInputElement;
  const location = within(dialog).getByLabelText('Where the game keeps its save files') as HTMLInputElement;
  await waitFor(() => expect(location.value).toBe('~/Saves/A/*.sav'));
  expect(executable.value).toBe('~/Games/A/a');
  fireEvent.blur(executable);
  fireEvent.blur(location);
  expect(within(dialog).queryByRole('alert')).toBeNull();
  fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(bridge.requests).toContainEqual(expect.objectContaining({ type: 'configure', game: 'a',
    executable: undefined, save_location: '~/Saves/A/*.sav' })));
});

test('the card cog configures that card, not the selected game, and shows its checkpoints store', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  const cog = screen.getByRole('button', { name: 'Configure Game B' });
  fireEvent.click(cog);
  const dialog = await screen.findByRole('dialog', { name: 'Configure Game B' });
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'save_set', game: 'b' }));
  const store = within(dialog).getByLabelText('Checkpoints store') as HTMLInputElement;
  await waitFor(() => expect(store.value).toBe('/store/b'));
  expect(store.disabled).toBe(true);
  expect(bridge.requests).not.toContainEqual({ type: 'open_checkpoints', game: 'b' });
  fireEvent.click(within(dialog).getByRole('button', { name: 'Open checkpoints store' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'open_checkpoints', game: 'b' }));
  expect(screen.getByText('First checkpoint')).toBeTruthy();
  fireEvent.click(within(dialog).getByRole('button', { name: 'Close dialog' }));
  expect(document.activeElement).toBe(cog);
});

test('Configure offers Reset only where a path differs from the default', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getByRole('button', { name: 'Configure Game A' }));
  const dialog = await screen.findByRole('dialog', { name: 'Configure Game A' });
  const location = within(dialog).getByLabelText('Where the game keeps its save files') as HTMLInputElement;
  await waitFor(() => expect(location.value).toBe('/old/saves'));
  const resets = () => within(dialog).queryAllByRole('button', { name: 'Reset' });
  expect(resets()).toHaveLength(1);
  fireEvent.click(resets()[0]);
  expect(location.value).toBe('');
  expect(resets()).toHaveLength(0);
  fireEvent.change(within(dialog).getByLabelText('Game executable'), { target: { value: '/games/a.exe' } });
  expect(resets()).toHaveLength(1);
  fireEvent.click(resets()[0]);
  expect(resets()).toHaveLength(0);
});

test('Configure lists the catalog save paths when no location is set', async () => {
  const bridge = new FakeBridge();
  const catalog: SaveTarget[] = [
    { root: '/lib/Application Support', filter: { kind: 'exact', value: 'game' }, excludes: ['game/settings.ini'] },
    { root: '/docs/Game', filter: { kind: 'pattern', value: 'Worlds/*.wld' } },
  ];
  bridge.saveSet = { catalog, active: catalog };
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getByRole('button', { name: 'Configure Game A' }));
  const dialog = await screen.findByRole('dialog', { name: 'Configure Game A' });
  const paths = () => within(dialog).queryAllByLabelText('Where the game keeps its save files').map((input) => (input as HTMLInputElement).value);
  await waitFor(() => expect(paths()).toEqual(['/lib/Application Support/game', '/docs/Game/Worlds/*.wld']));
  fireEvent.click(within(dialog).getAllByRole('button', { name: 'Open save location' })[1]);
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'open_saves', game: 'a', target: 1 }));
  expect(within(dialog).queryByRole('button', { name: 'Reset' })).toBeNull();
});

test('Configure offers executable Reset when the host reports an override', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0] = { ...bridge.state.games[0], executable: '/custom/a.exe', executable_overridden: true };
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getByRole('button', { name: 'Configure Game A' }));
  const dialog = await screen.findByRole('dialog', { name: 'Configure Game A' });
  await waitFor(() => expect((within(dialog).getByLabelText('Where the game keeps its save files') as HTMLInputElement).value).toBe('/old/saves'));
  expect(within(dialog).getAllByRole('button', { name: 'Reset' })).toHaveLength(2);
});

test('only the most recent save row is primary; loads and other rows are muted', async () => {
  const bridge = new FakeBridge();
  const loaded: HistoryEntry = { ...row('cp-recovery', 'Loaded one'), kind: 'loaded', actions: { load: false, revert: true, delete: true } };
  bridge.pages.a = { rows: [loaded, row('cp-a', 'Latest'), row('cp-old', 'Older')] };
  const { container } = render(<App bridge={bridge} />);
  await screen.findByText('Older');
  const rows = [...container.querySelectorAll('.history-row')];
  expect(rows.map((element) => element.classList.contains('primary'))).toEqual([false, true, false]);
});

test('closing a dialog restores focus to its opener', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  const opener = await screen.findByRole('button', { name: 'Settings' });
  fireEvent.click(opener);
  expect(document.activeElement).toBe(screen.getByRole('dialog'));
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(document.activeElement).toBe(opener);
});

test('the sidebar logo opens About, whose Website button opens the site', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'About SaveScummer' }));
  const about = screen.getByRole('dialog', { name: 'About' });
  expect(about.textContent).toMatch(/Version \d+\.\d+\.\d+/);
  expect(about.textContent).toContain('Oleksandr Shvets. All rights reserved.');
  fireEvent.click(within(about).getByRole('button', { name: 'Website' }));
  expect(bridge.openWebsite).toHaveBeenCalledOnce();
  fireEvent.click(within(about).getByRole('button', { name: 'Close' }));
  expect(screen.queryByRole('dialog')).toBeNull();
});

const saveButton = () => screen.getByRole('button', { name: /^Save shortcut/ });
const saveField = () => screen.getByRole('textbox', { name: 'Save shortcut' });

test('copyright years start at 2026 and extend to the current year', () => {
  expect(copyrightYears(new Date(2026, 5, 1))).toBe('2026');
  expect(copyrightYears(new Date(2028, 0, 1))).toBe('2026–2028');
});

test('Settings captures, saves, and shows host-owned shortcuts', async () => {
  const bridge = new FakeBridge();
  bridge.state.settings = { play_sounds: true, launch_on_startup: false, launch_on_startup_available: true,
    checkpoint_store: '/tmp/checkpoints', save_shortcut: 'Ctrl+F5', load_shortcut: 'Ctrl+F9' };
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
  fireEvent.click(saveButton());
  fireEvent.keyDown(saveField(), { code: 'F6', key: 'F6', ctrlKey: true });
  expect(saveButton().textContent).toBe(displayShortcut('Ctrl+F6'));
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'settings', play_sounds: true,
    launch_on_startup: undefined, save_shortcut: 'Ctrl+F6', load_shortcut: 'Ctrl+F9', flush_old_checkpoints: true }));
  expect(bridge.capture).toHaveBeenCalledWith(true);
  await waitFor(() => expect(bridge.capture).toHaveBeenCalledWith(false));
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2,
    settings: { ...bridge.state.settings!, save_shortcut: 'Ctrl+F6' } }));
  expect(screen.getByText(displayShortcut('Ctrl+F6'))).toBeTruthy();
});

test('Settings rejects duplicate shortcuts and Cancel keeps saved values', async () => {
  const bridge = new FakeBridge();
  bridge.state.settings = { play_sounds: true, launch_on_startup: false, launch_on_startup_available: true,
    checkpoint_store: '/tmp/checkpoints', save_shortcut: 'Ctrl+F5', load_shortcut: 'Ctrl+F9' };
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
  fireEvent.click(saveButton());
  fireEvent.keyDown(saveField(), { code: 'F9', key: 'F9', ctrlKey: true });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(screen.getAllByText('Choose a different shortcut.').length).toBeGreaterThan(0);
  expect(bridge.requests.some((request) => request.type === 'settings')).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  expect(screen.getByRole('button', { name: /^Save shortcut/ }).textContent).toBe(displayShortcut('Ctrl+F5'));
});

test('a shortcut can be removed, and restoring an empty one brings back the default', async () => {
  const bridge = new FakeBridge();
  bridge.state.settings = { play_sounds: true, launch_on_startup: false, launch_on_startup_available: true,
    checkpoint_store: '/tmp/checkpoints', save_shortcut: 'Ctrl+F6', load_shortcut: 'Ctrl+F9' };
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
  fireEvent.click(saveButton());
  fireEvent.keyDown(saveField(), { code: 'ShiftLeft', key: 'Shift', shiftKey: true, ctrlKey: true });
  expect((saveField() as HTMLInputElement).value).toBe(displayShortcut('Ctrl+Shift+'));
  fireEvent.blur(saveField());
  expect(saveButton().textContent).toBe(displayShortcut('Ctrl+F6'));

  fireEvent.click(saveButton());
  fireEvent.click(screen.getByRole('button', { name: `Keep ${displayShortcut('Ctrl+F6')}` }));
  expect(saveButton().textContent).toBe(displayShortcut('Ctrl+F6'));

  fireEvent.click(saveButton());
  fireEvent.click(screen.getByRole('button', { name: 'Remove shortcut' }));
  expect(saveButton().textContent).toBe('Record shortcut');
  fireEvent.click(saveButton());
  const fallback = navigator.platform.includes('Mac') ? 'Alt+F5' : 'Ctrl+F5';
  fireEvent.click(screen.getByRole('button', { name: `Use default ${displayShortcut(fallback)}` }));
  expect(saveButton().textContent).toBe(displayShortcut(fallback));

  fireEvent.click(saveButton());
  fireEvent.keyDown(saveField(), { code: 'Backspace', key: 'Backspace' });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'settings', play_sounds: true,
    launch_on_startup: undefined, save_shortcut: '', load_shortcut: 'Ctrl+F9', flush_old_checkpoints: true }));
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2,
    settings: { ...bridge.state.settings!, save_shortcut: '' } }));
  expect(document.querySelectorAll('.action-slot .shortcut-tab')).toHaveLength(1);
});

test('Settings blocks major conflicts and shows a savable warning for minor conflicts', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
  fireEvent.click(saveButton());
  const mac = navigator.platform.includes('Mac');
  fireEvent.keyDown(saveField(), { code: 'KeyC', key: 'c', ctrlKey: !mac, metaKey: mac });
  expect(screen.getByRole('alert').textContent).toContain('reserved for Copy');
  expect(screen.getByText('Copy', { selector: 'strong' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(bridge.requests.some((request) => request.type === 'settings')).toBe(false);

  fireEvent.click(saveButton());
  fireEvent.keyDown(saveField(), { code: mac ? 'KeyN' : 'KeyG', key: mac ? 'n' : 'g', metaKey: true });
  expect(screen.getByText(/May interfere with/).textContent).toContain(mac ? 'New window' : 'Game Bar');
  expect(screen.getByText(mac ? 'New window or document' : 'Open Game Bar', { selector: 'strong' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(bridge.requests.some((request) => request.type === 'settings')).toBe(true));
});

test('host rejection stays beside the shortcut and keeps Settings open', async () => {
  const bridge = new FakeBridge();
  bridge.settingsError = 'Save shortcut: Alt+F6 is unavailable: another app uses it';
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
  fireEvent.click(saveButton());
  fireEvent.keyDown(saveField(), { code: 'F6', key: 'F6', altKey: true });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(await screen.findByText('Alt+F6 is unavailable: another app uses it')).toBeTruthy();
  expect(saveButton().getAttribute('aria-invalid')).toBe('true');
  expect(screen.getByRole('dialog', { name: 'Settings' })).toBeTruthy();
});

test('large histories mount only the visible rows', async () => {
  const bridge = new FakeBridge();
  bridge.pages.a = { rows: Array.from({ length: 1000 }, (_, index) => ({
    ...row(`cp-${index}`, `Checkpoint ${index}`),
    at: new Date(Date.parse('2026-09-27T12:00:00Z') - index * 1000).toISOString(),
  })) };
  const { container } = render(<App bridge={bridge} />);
  expect(await screen.findByText('Checkpoint 0')).toBeTruthy();
  expect(container.querySelectorAll('.history-row').length).toBeLessThan(30);
  const history = container.querySelector('.history') as HTMLElement;
  Object.defineProperty(history, 'scrollTop', { value: 6400, writable: true });
  fireEvent.scroll(history);
  await waitFor(() => expect(screen.getByText('Checkpoint 100')).toBeTruthy());
  expect(container.querySelectorAll('.history-row').length).toBeLessThan(30);
});

test('Flush opens over Configure, previews host data, and sends only the game after confirmation', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].has_history = true;
  render(<App bridge={bridge} />);
  expect(await screen.findByText('First checkpoint')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Configure Game A' }));
  const configure = await screen.findByRole('dialog', { name: 'Configure Game A' });
  const flushButton = within(configure).getByRole('button', { name: /^Flush/ });
  fireEvent.click(flushButton);
  const flush = await screen.findByRole('dialog', { name: 'Flush checkpoints Game A' });
  expect(await within(flush).findByText('Checkpoints')).toBeTruthy();
  expect(bridge.requests).toContainEqual({ type: 'flush_preview', game: 'a', limit: 30 });
  expect(bridge.requests.some((request) => request.type === 'flush')).toBe(false);
  fireEvent.click(within(flush).getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByRole('dialog', { name: 'Flush checkpoints Game A' })).toBeNull();
  expect(screen.getByRole('dialog', { name: 'Configure Game A' })).toBeTruthy();
  expect(document.activeElement).toBe(flushButton);
  fireEvent.click(flushButton);
  const again = await screen.findByRole('dialog', { name: 'Flush checkpoints Game A' });
  await within(again).findByText('Checkpoints');
  fireEvent.click(within(again).getByRole('button', { name: 'Flush' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'flush', game: 'a' }));
});

test('no-games layout keeps Scan, Add, Settings in keyboard order', async () => {
  const bridge = new FakeBridge();
  bridge.state.games = [];
  bridge.state.active_stack = [];
  const { container } = render(<App bridge={bridge} />);
  await waitFor(() => expect(container.querySelector('.no-games')).toBeTruthy());
  expect([...container.querySelectorAll('.library-controls button')].map((button) => button.textContent)).toEqual([
    'Scan for games', 'Add custom game', 'Settings',
  ]);
});

test('running games surface to the top of the one list in active-stack order; stopped games keep the host order', async () => {
  const bridge = new FakeBridge();
  // The host sends stopped games by last focus, not by name.
  bridge.state.games = [game('c', 'Game C'), game('b', 'Game B'), game('a', 'Game A'), game('d', 'Game D')];
  bridge.state.games[1].running = true;
  bridge.state.games[3].running = true;
  bridge.state.active_stack = ['d', 'b'];
  bridge.pages = { a: { rows: [] }, b: { rows: [] }, c: { rows: [] }, d: { rows: [] } };
  const { container } = render(<App bridge={bridge} />);
  await screen.findByRole('button', { name: 'Game D' });
  const cards = [...container.querySelectorAll('.library-panel .game-card-wrap')];
  expect(cards.map((card) => card.querySelector('.game-card')!.getAttribute('aria-label'))).toEqual([
    'Game D', 'Game B', 'Game C, Not running', 'Game A, Not running',
  ]);
  expect(cards.map((card) => Boolean(card.querySelector('.running-badge')))).toEqual([true, true, false, false]);
  expect(container.querySelectorAll('.sidebar h2')).toHaveLength(1);
});

test('the game list scrolls and its indicator follows scrolling', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].running = true;
  const { container } = render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  const scroller = container.querySelector('.sidebar-content') as HTMLDivElement;
  const track = container.querySelector('.sidebar-scrollbar') as HTMLDivElement;
  expect(scroller.querySelector('.library-panel')).toBeTruthy();
  expect(scroller.contains(container.querySelector('.library-controls'))).toBe(false);

  Object.defineProperties(scroller, {
    clientHeight: { configurable: true, value: 300 },
    scrollHeight: { configurable: true, value: 900 },
    scrollTop: { configurable: true, writable: true, value: 0 },
  });
  Object.defineProperty(track, 'clientHeight', { configurable: true, value: 160 });
  fireEvent.resize(window);
  const thumb = await waitFor(() => {
    const value = track.querySelector('.sidebar-scrollbar-thumb') as HTMLDivElement;
    expect(value).toBeTruthy();
    return value;
  });
  expect(parseFloat(thumb.style.height)).toBeCloseTo(160 / 3);
  scroller.scrollTop = 300;
  fireEvent.scroll(scroller);
  expect(parseFloat(thumb.style.top)).toBeCloseTo((160 - 160 / 3) / 2);
});

test('Scan icon spins only while a manual scan is running', async () => {
  const bridge = new FakeBridge();
  let finish!: (result: { new_games: number }) => void;
  bridge.scanResult = new Promise((resolve) => { finish = resolve; });
  render(<App bridge={bridge} />);

  const button = await screen.findByRole('button', { name: 'Scan for games' });
  expect(button.classList.contains('scanning')).toBe(false);
  fireEvent.click(button);
  expect(button.classList.contains('scanning')).toBe(true);
  expect(button.textContent).toBe('Scanning…');

  finish({ new_games: 0 });
  await waitFor(() => expect(button.classList.contains('scanning')).toBe(false));
  expect(button.textContent).toBe('No new games');
});
