import conflicts from './shortcut-conflicts.json';

type KeyPress = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>;
type Platform = 'windows' | 'macos';

function currentPlatform(): Platform {
  return navigator.platform.includes('Mac') ? 'macos' : 'windows';
}

function shortcutConflict(shortcut: string, platform: Platform) {
  return conflicts[platform].find((entry) => entry.key === shortcut);
}

export function shortcutDescription(shortcut: string): string | undefined {
  return shortcutConflict(shortcut, currentPlatform())?.description;
}

export function capturedShortcut(event: KeyPress): string | undefined {
  const key = /^(F(?:[1-9]|1[0-2])|Key[A-Z]|Digit[0-9])$/.exec(event.code)?.[1];
  if (!key) return undefined;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  if (event.metaKey) parts.push('Meta');
  parts.push(key.startsWith('Key') ? key.slice(3) : key.startsWith('Digit') ? key.slice(5) : key);
  return parts.join('+');
}

export function displayShortcut(shortcut: string): string {
  if (!navigator.platform.includes('Mac')) return shortcut.replace('Meta+', 'Win+');
  return shortcut.replaceAll('Ctrl+', '⌃').replaceAll('Alt+', '⌥')
    .replaceAll('Shift+', '⇧').replaceAll('Meta+', '⌘');
}

export function shortcutError(shortcut: string, other: string, platform = currentPlatform()): string | undefined {
  if (!shortcut) return 'Press a shortcut.';
  if (shortcut === other) return 'Choose a different shortcut.';
  if (/^[A-Z0-9]$/.test(shortcut) || /^Shift\+[A-Z0-9]$/.test(shortcut)) {
    return 'Letters and numbers need Ctrl, Alt, or Meta.';
  }
  const conflict = shortcutConflict(shortcut, platform);
  if (conflict?.severity === 'major') {
    return `This shortcut is reserved for ${conflict.description} on ${platform === 'macos' ? 'macOS' : 'Windows'}.`;
  }
  return undefined;
}

export function shortcutWarning(shortcut: string, platform = currentPlatform()): string | undefined {
  const conflict = shortcutConflict(shortcut, platform);
  if (conflict?.severity !== 'minor') return undefined;
  return `May interfere with ${conflict.description} on ${platform === 'macos' ? 'macOS' : 'Windows'}. You can still choose it.`;
}
