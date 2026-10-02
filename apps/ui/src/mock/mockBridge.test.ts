import { afterEach, expect, test, vi } from 'vitest';
import type { HostState, Operation } from '../types';
import { createMockBridge } from './mockBridge';

afterEach(() => {
  window.history.replaceState(null, '', '/');
  vi.restoreAllMocks();
});

test.each([
  ['save', 'save-start', 'save-complete'],
  ['load', 'load-start', 'load-complete'],
] as const)('browser preview plays %s start and finish once', async (kind, start, finish) => {
  const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue(undefined);
  const bridge = createMockBridge();
  const accepted = await bridge.request<Operation>({ type: kind, game: 'steam-212680', ...(kind === 'load' ? { checkpoint: 'cp' } : {}) });
  expect((play.mock.contexts.at(-1) as HTMLAudioElement).src).toContain(`/sounds/${start}.wav`);
  await bridge.request<Operation>({ type: 'outcome', operation: accepted.id });
  expect((play.mock.contexts.at(-1) as HTMLAudioElement).src).toContain(`/sounds/${finish}.wav`);
  expect(play).toHaveBeenCalledTimes(2);
  await bridge.request<Operation>({ type: 'outcome', operation: accepted.id });
  expect(play).toHaveBeenCalledTimes(2);
});

test('browser preview respects Play sounds for Save and Load', async () => {
  const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue(undefined);
  const bridge = createMockBridge();
  await bridge.request<HostState>({ type: 'state' });
  await bridge.request({ type: 'settings', play_sounds: false });
  const accepted = await bridge.request<Operation>({ type: 'save', game: 'steam-212680' });
  await bridge.request<Operation>({ type: 'outcome', operation: accepted.id });
  expect(play).not.toHaveBeenCalled();
});
