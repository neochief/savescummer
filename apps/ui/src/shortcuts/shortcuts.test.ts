import { expect, test } from 'vitest';
import { shortcutError, shortcutWarning } from './shortcuts';

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
