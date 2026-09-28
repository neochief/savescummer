import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App, relativeAge } from './App';
import { displayShortcut, shortcutError, shortcutWarning } from './shortcuts';
import type { Bridge } from './bridge';
import type { Game, HistoryEntry, HistoryPage, HostState, Operation, UiRequest } from './types';

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
  settingsError?: string;
  requests: UiRequest[] = [];
  stateListener?: (state: HostState) => void;
  statusListener?: (status: string) => void;

  async request<T>(request: UiRequest): Promise<T> {
    this.requests.push(request);
    if (request.type === 'state') return this.state as T;
    if (request.type === 'history') return this.pages[request.game] as T;
    if (request.type === 'save' || request.type === 'load' || request.type === 'delete' || request.type === 'revert') {
      return { id: 'op-1', kind: request.type, status: 'accepted' } as T;
    }
    if (request.type === 'cancel_delete' || request.type === 'set_label') return {} as T;
    if (request.type === 'add_game') return { game: 'custom-1' } as T;
    if (request.type === 'save_set') return { location: '/old/saves', active: [{ root: '/old/saves' }] } as T;
    if (request.type === 'flush_preview') return { saved: 1, recovery: 0, temporary: 0, size: 4096, items: [] } as T;
    if (request.type === 'settings' && this.settingsError) throw new Error(this.settingsError);
    if (request.type === 'settings' || request.type === 'configure' || request.type === 'open_checkpoints') return {} as T;
    return this.outcome as Promise<T>;
  }
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

test('host rejection is shown on the initiating control', async () => {
  const bridge = new FakeBridge();
  bridge.outcome = Promise.resolve({ id: 'op-1', kind: 'load', status: 'failed', error: { kind: 'io', detail: 'copy failed' } });
  render(<App bridge={bridge} />);
  const button = await screen.findByRole('button', { name: 'load Game A' });
  fireEvent.click(button);
  expect(await screen.findByText('FAILED')).toBeTruthy();
  expect(screen.getByRole('alert').textContent).toContain('copy failed');
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

test('More menu opens checkpoints folder through host and configures paths', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  await screen.findByText('First checkpoint');
  fireEvent.click(await screen.findByRole('button', { name: 'More game actions' }));
  fireEvent.click(screen.getByRole('menuitem', { name: 'Open checkpoints folder' }));
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'open_checkpoints', game: 'a' }));
  fireEvent.click(screen.getByRole('button', { name: 'More game actions' }));
  fireEvent.click(screen.getByRole('menuitem', { name: 'Configure paths…' }));
  expect(await screen.findByRole('dialog', { name: 'Configure paths — Game A' })).toBeTruthy();
  await waitFor(() => expect(bridge.requests).toContainEqual({ type: 'save_set', game: 'a' }));
});

test('More menu supports arrow navigation and Escape returns focus', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  const opener = await screen.findByRole('button', { name: 'More game actions' });
  opener.focus();
  fireEvent.keyDown(opener, { key: 'ArrowDown' });
  const first = screen.getByRole('menuitem', { name: 'Open checkpoints folder' });
  fireEvent.keyDown(opener, { key: 'ArrowDown' });
  expect(document.activeElement).toBe(first);
  fireEvent.keyDown(first, { key: 'ArrowDown' });
  expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Configure paths…' }));
  fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
  expect(screen.queryByRole('menu')).toBeNull();
  expect(document.activeElement).toBe(opener);
});

test('a pointer-opened More menu closes on Escape', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  const opener = await screen.findByRole('button', { name: 'More game actions' });
  fireEvent.click(opener);
  expect(document.activeElement).toBe(opener);
  expect(screen.getByRole('menu')).toBeTruthy();
  fireEvent.keyDown(document, { key: 'Escape' });
  expect(screen.queryByRole('menu')).toBeNull();
});

test('closing a dialog restores focus to its opener', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  const opener = await screen.findByRole('button', { name: 'Settings' });
  fireEvent.click(opener);
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Close dialog' }));
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(document.activeElement).toBe(opener);
});

test('closing Configure returns focus to the More button after its menu unmounts', async () => {
  const bridge = new FakeBridge();
  render(<App bridge={bridge} />);
  const opener = await screen.findByRole('button', { name: 'More game actions' });
  fireEvent.click(opener);
  fireEvent.click(screen.getByRole('menuitem', { name: 'Configure paths…' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Close dialog' }));
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

test('Flush previews host data and sends only the game after confirmation', async () => {
  const bridge = new FakeBridge();
  bridge.state.games[0].has_history = true;
  render(<App bridge={bridge} />);
  expect(await screen.findByText('First checkpoint')).toBeTruthy();
  fireEvent.click(await screen.findByRole('button', { name: 'More game actions' }));
  fireEvent.click(screen.getByRole('menuitem', { name: 'Flush checkpoints…' }));
  expect(await screen.findByText('Saved backups')).toBeTruthy();
  expect(bridge.requests).toContainEqual({ type: 'flush_preview', game: 'a', limit: 30 });
  expect(bridge.requests.some((request) => request.type === 'flush')).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Flush' }));
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
