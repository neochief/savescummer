import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { failureMessage } from './messages';
import type { Failure, HostResponse, UiRequest } from './types';

/** A request the host refused, with its reasons for callers that explain them in place. */
export class HostError extends Error {
  constructor(readonly failure: Failure | undefined, fallback: string) {
    super(failureMessage(failure, fallback));
  }
}

export const platformName = navigator.platform.startsWith('Mac') ? 'macOS' : navigator.platform.startsWith('Win') ? 'Windows' : 'Linux';

export interface Bridge {
  request<T>(request: UiRequest): Promise<T>;
  report(selected: string | null, focused: boolean): Promise<void>;
  capture(capturing: boolean): Promise<void>;
  artwork(game: string, kind: 'hero' | 'logo' | 'header' | 'icon'): Promise<string>;
  openWebsite(): Promise<void>;
  openSaveSearch(engine: 'google' | 'chatgpt', game: string): Promise<void>;
  onState(callback: (state: import('./types').HostState) => void): Promise<UnlistenFn>;
  onStatus(callback: (status: string) => void): Promise<UnlistenFn>;
  onLabels(callback: (game: string) => void): Promise<UnlistenFn>;
}

export const tauriBridge: Bridge = {
  async request<T>(request: UiRequest): Promise<T> {
    const response = await invoke<HostResponse<T>>('host_request', { request });
    if (!response.ok || response.result === undefined) {
      throw new HostError(response.error, 'Host rejected the request');
    }
    return response.result;
  },
  report(selected, focused) {
    return invoke<void>('report_ui', { selected, focused });
  },
  capture(capturing) {
    return invoke<void>('set_shortcut_capture', { capturing });
  },
  async artwork(game, kind) {
    const data = await invoke<{ mime: string; bytes: number[] }>('artwork', { game, kind });
    return URL.createObjectURL(new Blob([new Uint8Array(data.bytes)], { type: data.mime }));
  },
  openWebsite() {
    return invoke<void>('open_website');
  },
  openSaveSearch(engine, game) {
    return invoke<void>('open_save_search', { engine, game });
  },
  async onState(callback) {
    return listen('host-state', (event) => callback(event.payload as import('./types').HostState));
  },
  async onStatus(callback) {
    return listen('host-status', (event) => callback(event.payload as string));
  },
  async onLabels(callback) {
    return listen('host-labels', (event) => callback(event.payload as string));
  },
};
