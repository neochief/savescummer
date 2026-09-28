import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { App, relativeAge } from './App';
import { displayShortcut, shortcutError, shortcutWarning } from './shortcuts';
import type { Bridge } from './bridge';
import type { Game, HistoryEntry, HistoryPage, HostState, Operation, SaveSet, SaveTarget, UiRequest } from './types';

afterEach(cleanup);
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});

const game = (id: string, name: string): Game => ({
  id, name, installed: true, running: false,
  save: { available: true }, load: { available: true },
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
  requests: UiRequest[] = [];
  stateListener?: (state: HostState) => void;
  statusListener?: (status: string) => void;

  async request<T>(request: UiRequest): Promise<T> {
    this.requests.push(request);
    if (request.type === 'state') return this.state as T;
    if (request.type === 'scan') return this.scanResult as Promise<T>;
    if (request.type === 'history') return this.pages[request.game] as T;
    if (request.type === 'save' || request.type === 'load' || request.type === 'delete' || request.type === 'revert') {
      return { id: 'op-1', kind: request.type, status: 'accepted' } as T;
    }
    if (request.type === 'cancel_delete' || request.type === 'set_label') return {} as T;
    if (request.type === 'add_game') return { game: 'custom-1' } as T;
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
  async onState(callback: (state: HostState) => void) { this.stateListener = callback; return () => { this.stateListener = undefined; }; }
  async onStatus(callback: (status: string) => void) { this.statusListener = callback; return () => { this.statusListener = undefined; }; }
  async onLabels() { return () => undefined; }
}

test('selecting another game reads that game’s real history page', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  expect(await screen.findByText('First checkpoint')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: /Game B, Not running/ }));
  expect(await screen.findByText('Second checkpoint')).toBeTruthy();
  expect(screen.queryByText('First checkpoint')).toBeNull();
  expect(bridge.requests).toContainEqual({ type: 'history', game: 'b', limit: 100 });
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
  expect(await screen.findByText('SAVING…')).toBeTruthy();
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'outcome', operation: 'op-1' }));
  expect(screen.queryByText('DONE')).toBeNull();
  finish({ id: 'op-1', kind: 'save', status: 'succeeded' });
  expect(await screen.findByText('DONE')).toBeTruthy();
});

test('a running game that saves on exit covers the actions and hides row loads until it has exited', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  const locked = { available: false, reason: 'game_running' };
  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 2,
    games: bridge.state.games.map((game) => game.id === 'a' ? { ...game, running: true, save: locked, load: locked } : game) }));
  const panel = screen.getByRole('status', { name: 'Exit the game first' });
  expect(within(panel).getByText('Save and Exit the game')).toBeTruthy();
  expect((screen.getByRole('button', { name: 'save Game A' }) as HTMLButtonElement).disabled).toBe(true);
  expect(document.querySelectorAll('.row-button:not(.locked)')).toHaveLength(0);

  await act(async () => bridge.stateListener?.({ ...bridge.state, revision: 3 }));
  expect(screen.queryByRole('status', { name: 'Exit the game first' })).toBeNull();
  expect(document.querySelectorAll('.row-button.locked')).toHaveLength(0);
});

test('Configure turns waiting for the game to close off', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0] = { ...bridge.state.games[0], executable: '/games/a.exe', wait_for_exit: true };
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(screen.getByRole('button', { name: 'Configure Game A' }));
  const dialog = await screen.findByRole('dialog', { name: 'Configure — Game A' });
  const check = within(dialog).getByRole('checkbox', { name: /Wait for the game to close/ }) as HTMLInputElement;
  expect(check.checked).toBe(true);
  fireEvent.click(check);
  fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(bridge.requests).toContainEqual(expect.objectContaining({ type: 'configure', game: 'a', wait_for_exit: false })));
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
  bridge.state.games[0].blocked = { kind: 'access_needed', detail: 'app_data' };
  render(<App bridge={bridge} />);
  const button = await screen.findByRole('button', { name: 'save Game A' });
  expect((button as HTMLButtonElement).disabled).toBe(true);
  expect(button.closest('.action-slot')?.querySelector('.shortcut-tab')).toBeNull();
  expect(screen.getByRole('alert').textContent).toBe("SaveScummer needs permission to access other apps' data.");
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

test('delete countdown and cancel follow host state', async () => {
  const bridge = new FakeBridge();
  bridge.outcome = new Promise(() => undefined);
  render(<App bridge={bridge} />);
  const remove = await screen.findByRole('button', { name: /Delete checkpoint from/ });
  fireEvent.click(remove);
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'delete', game: 'a', checkpoint: 'cp-a' }));
  bridge.stateListener?.({ ...bridge.state, revision: 2, deletes: [{
    id: 'op-1', game: 'a', checkpoint: 'cp-a', kind: 'delete', status: 'counting_down', remaining_ms: 3000,
  }] });
  expect(await screen.findByText('Deleting in 3')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'CANCEL' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'cancel_delete', operation: 'op-1' }));
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
  fireEvent.change(screen.getByLabelText('Save location'), { target: { value: '/games/example/saves' } });
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Example' } });
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'add_game', name: 'Example', executable: '/games/example', save_location: '/games/example/saves' }));
});

test('the card cog configures that card, not the selected game, and shows its checkpoints store', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  const cog = screen.getByRole('button', { name: 'Configure Game B' });
  fireEvent.click(cog);
  const dialog = await screen.findByRole('dialog', { name: 'Configure — Game B' });
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
  const dialog = await screen.findByRole('dialog', { name: 'Configure — Game A' });
  const location = within(dialog).getByLabelText('Save location') as HTMLInputElement;
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
  const dialog = await screen.findByRole('dialog', { name: 'Configure — Game A' });
  const paths = () => within(dialog).queryAllByLabelText('Save location').map((input) => (input as HTMLInputElement).value);
  await waitFor(() => expect(paths()).toEqual(['/lib/Application Support/game', '/docs/Game/Worlds/*.wld']));
  expect(within(dialog).getByText('Except settings.ini')).toBeTruthy();
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
  const dialog = await screen.findByRole('dialog', { name: 'Configure — Game A' });
  await waitFor(() => expect((within(dialog).getByLabelText('Save location') as HTMLInputElement).value).toBe('/old/saves'));
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

test('Settings captures, saves, and shows host-owned shortcuts', async () => {
  const bridge = new FakeBridge();
  bridge.state.settings = { play_sounds: true, launch_on_startup: false, launch_on_startup_available: true,
    checkpoint_store: '/tmp/checkpoints', save_shortcut: 'Ctrl+F5', load_shortcut: 'Ctrl+F9' };
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
  const save = screen.getByRole('textbox', { name: 'Save shortcut' });
  fireEvent.focus(save);
  fireEvent.keyDown(save, { code: 'F6', key: 'F6', ctrlKey: true });
  expect((save as HTMLInputElement).value).toBe(displayShortcut('Ctrl+F6'));
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'settings', play_sounds: true,
    launch_on_startup: undefined, save_shortcut: 'Ctrl+F6', load_shortcut: 'Ctrl+F9' }));
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
  const save = screen.getByRole('textbox', { name: 'Save shortcut' });
  fireEvent.focus(save);
  fireEvent.keyDown(save, { code: 'F9', key: 'F9', ctrlKey: true });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(screen.getAllByText('Choose a different shortcut.').length).toBeGreaterThan(0);
  expect(bridge.requests.some((request) => request.type === 'settings')).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  expect((screen.getByRole('textbox', { name: 'Save shortcut' }) as HTMLInputElement).value).toBe(displayShortcut('Ctrl+F5'));
});

test('shortcut conflicts block major keys and warn for minor keys on both platforms', () => {
  expect(shortcutError('Ctrl+C', 'Ctrl+F9', 'windows')).toContain('Copy');
  expect(shortcutError('Meta+L', 'Ctrl+F9', 'windows')).toContain('Lock');
  expect(shortcutError('Meta+Q', 'Alt+F9', 'macos')).toContain('Quit');
  expect(shortcutError('Shift+Meta+4', 'Alt+F9', 'macos')).toContain('Capture');
  expect(shortcutError('Meta+G', 'Ctrl+F9', 'windows')).toBeUndefined();
  expect(shortcutWarning('Meta+G', 'windows')).toContain('Game Bar');
  expect(shortcutError('Meta+N', 'Alt+F9', 'macos')).toBeUndefined();
  expect(shortcutWarning('Meta+N', 'macos')).toContain('New window');
});

test('Settings blocks major conflicts and shows a savable warning for minor conflicts', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
  const save = screen.getByRole('textbox', { name: 'Save shortcut' });
  fireEvent.focus(save);
  const mac = navigator.platform.includes('Mac');
  fireEvent.keyDown(save, { code: 'KeyC', key: 'c', ctrlKey: !mac, metaKey: mac });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(screen.getByRole('alert').textContent).toContain('reserved for Copy');
  expect(screen.getByText('Copy', { selector: 'strong' })).toBeTruthy();
  expect(bridge.requests.some((request) => request.type === 'settings')).toBe(false);

  fireEvent.keyDown(save, { code: mac ? 'KeyN' : 'KeyG', key: mac ? 'n' : 'g', metaKey: true });
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
  const save = screen.getByRole('textbox', { name: 'Save shortcut' });
  fireEvent.focus(save);
  fireEvent.keyDown(save, { code: 'F6', key: 'F6', altKey: true });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(await screen.findByText('Alt+F6 is unavailable: another app uses it')).toBeTruthy();
  expect(save.getAttribute('aria-invalid')).toBe('true');
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
  const configure = await screen.findByRole('dialog', { name: 'Configure — Game A' });
  const flushButton = within(configure).getByRole('button', { name: 'Flush checkpoints…' });
  fireEvent.click(flushButton);
  const flush = await screen.findByRole('dialog', { name: 'Flush checkpoints — Game A' });
  expect(await within(flush).findByText('Saved backups')).toBeTruthy();
  expect(bridge.requests).toContainEqual({ type: 'flush_preview', game: 'a', limit: 30 });
  expect(bridge.requests.some((request) => request.type === 'flush')).toBe(false);
  fireEvent.click(within(flush).getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByRole('dialog', { name: 'Flush checkpoints — Game A' })).toBeNull();
  expect(screen.getByRole('dialog', { name: 'Configure — Game A' })).toBeTruthy();
  expect(document.activeElement).toBe(flushButton);
  fireEvent.click(flushButton);
  const again = await screen.findByRole('dialog', { name: 'Flush checkpoints — Game A' });
  await within(again).findByText('Saved backups');
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

test('running games surface to the top of the one list in active-stack order with a badge', async () => {
  const bridge = new FakeBridge();
  bridge.state.games = [game('a', 'Game A'), game('b', 'Game B'), game('c', 'Game C'), game('d', 'Game D')];
  bridge.state.games[1].running = true;
  bridge.state.games[3].running = true;
  bridge.state.active_stack = ['d', 'b'];
  bridge.pages = { a: { rows: [] }, b: { rows: [] }, c: { rows: [] }, d: { rows: [] } };
  const { container } = render(<App bridge={bridge} />);
  await screen.findByRole('button', { name: 'Game D' });
  const cards = [...container.querySelectorAll('.library-panel .game-card-wrap')];
  expect(cards.map((card) => card.querySelector('.game-card')!.getAttribute('aria-label'))).toEqual([
    'Game D', 'Game B', 'Game A, Not running', 'Game C, Not running',
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
