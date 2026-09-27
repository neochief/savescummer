type KeyPress = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>;

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

export function shortcutError(shortcut: string, other: string): string | undefined {
  if (!shortcut) return 'Press a shortcut.';
  if (shortcut === other) return 'Choose a different shortcut.';
  if (/^[A-Z0-9]$/.test(shortcut) || /^Shift\+[A-Z0-9]$/.test(shortcut)) {
    return 'Letters and numbers need Ctrl, Alt, or Meta.';
  }
  return undefined;
}
