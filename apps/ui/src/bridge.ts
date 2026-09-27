import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { HostResponse, UiRequest } from './types';

export interface Bridge {
  request<T>(request: UiRequest): Promise<T>;
  report(selected: string | null, focused: boolean): Promise<void>;
  capture(capturing: boolean): Promise<void>;
  artwork(game: string, kind: 'hero' | 'logo' | 'header' | 'icon'): Promise<string>;
  onState(callback: (state: import('./types').HostState) => void): Promise<UnlistenFn>;
  onStatus(callback: (status: string) => void): Promise<UnlistenFn>;
  onLabels(callback: (game: string) => void): Promise<UnlistenFn>;
}

export const tauriBridge: Bridge = {
  async request<T>(request: UiRequest): Promise<T> {
    const response = await invoke<HostResponse<T>>('host_request', { request });
    if (!response.ok || response.result === undefined) {
      throw new Error(response.error?.detail || response.error?.kind || 'Host rejected the request');
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
