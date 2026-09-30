import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { tauriBridge, type Bridge } from './bridge';
import './style.css';

if (navigator.platform.includes('Mac')) document.documentElement.classList.add('macos');

if ('__TAURI_INTERNALS__' in window) {
  document.addEventListener('contextmenu', (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) {
      event.preventDefault();
      return;
    }

    const input = target.closest('input');
    const isTextInput = input && [
      'text', 'search', 'email', 'url', 'tel', 'password', 'number',
    ].includes(input.type);

    if (!isTextInput && !target.closest('textarea') && !target.isContentEditable) {
      event.preventDefault();
    }
  });
}

function render(bridge: Bridge) {
  createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App bridge={bridge} />
    </React.StrictMode>,
  );
}

// Plain-browser `pnpm dev` has no Tauri host; use the in-memory mock bridge instead.
if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
  import('./mock/mockBridge').then(({ createMockBridge }) => render(createMockBridge()));
} else {
  render(tauriBridge);
}
