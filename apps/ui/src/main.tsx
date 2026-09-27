import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { tauriBridge } from './bridge';
import './style.css';

if (navigator.platform.includes('Mac')) document.documentElement.classList.add('macos');

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App bridge={tauriBridge} />
  </React.StrictMode>,
);
