import { expect, test } from 'vitest';
import { cleanPath, platformProblem, tooBroadProblem, type Platform } from './paths';

// The host shows and keeps the user's folders portably; each platform's own form must pass.
const portable: Record<Platform, { executable: string[]; location: string[] }> = {
  macOS: { executable: ['~/Applications/VoidWar.app', '/Applications/VoidWar.app'], location: ['~', '~/Library/Application Support/VoidWar/*.sav'] },
  Linux: {
    executable: ['~/Games/VoidWar/VoidWar', '~/.local/share/Steam/steamapps/common/VoidWar/VoidWar.exe'],
    location: ['~/.local/share/Steam/steamapps/compatdata/42/pfx/drive_c/users/steamuser/AppData/Roaming/VoidWar'],
  },
  Windows: {
    executable: ['%USERPROFILE%\\Games\\VoidWar\\VoidWar.exe', 'C:\\Games\\VoidWar\\VoidWar.exe', '\\\\nas\\games\\VoidWar.exe'],
    location: ['%APPDATA%\\VoidWar', '%LOCALAPPDATA%\\VoidWar\\*.sav', '%USERPROFILE%\\Documents\\My Games\\VoidWar', '%appdata%\\VoidWar'],
  },
};

for (const [platform, fields] of Object.entries(portable) as [Platform, typeof portable.macOS][]) {
  test(`${platform} accepts its own portable paths`, () => {
    for (const path of fields.executable) expect(platformProblem('executable', path, 'Void War', platform), path).toBeUndefined();
    for (const path of fields.location) expect(platformProblem('location', path, 'Void War', platform), path).toBeUndefined();
  });
}

test('macOS and Linux explain Windows folders, whether portable or full', () => {
  for (const platform of ['macOS', 'Linux'] as const) {
    expect(platformProblem('location', '%APPDATA%\\VoidWar', 'Void War', platform)).toContain('Folders like “%APPDATA%” only exist on Windows.');
    expect(platformProblem('location', '%USERPROFILE%\\Documents', 'Void War', platform)).toContain('“%USERPROFILE%”');
    expect(platformProblem('location', 'D:\\Saves\\VoidWar', 'Void War', platform)).toContain('“D:” drive');
    expect(platformProblem('executable', 'C:\\Games\\VoidWar.exe', 'Void War', platform)).toContain('This is a Windows location.');
  }
  expect(platformProblem('location', '%APPDATA%\\VoidWar', 'Void War', 'macOS')).toContain('Look up where Void War keeps saves on macOS.');
});

test('only a Mac turns away Windows programs, since Linux runs them through Proton', () => {
  expect(platformProblem('executable', '~/Downloads/VoidWar.exe', 'Void War', 'macOS')).toContain('Macs can’t open them');
  expect(platformProblem('executable', '~/Downloads/VoidWar.exe', 'Void War', 'Linux')).toBeUndefined();
});

test('Windows explains Mac and Linux paths, whether portable or full', () => {
  expect(platformProblem('location', '~/Library/Application Support/VoidWar', 'Void War', 'Windows')).toContain('on a Mac or Linux');
  expect(platformProblem('location', '$HOME/.local/share/VoidWar', 'Void War', 'Windows')).toContain('on a Mac or Linux');
  expect(platformProblem('executable', '/Applications/VoidWar.app', 'Void War', 'Windows')).toContain('This is a Mac or Linux location.');
  expect(platformProblem('executable', 'D:\\Downloads\\VoidWar.app', 'Void War', 'Windows')).toContain('“.app” is a Mac program');
});

test('a path pasted from Terminal or Explorer is read as the user meant it', () => {
  // Terminal escapes spaces and brackets; Finder's drag and zsh may quote instead.
  expect(cleanPath('~/Library/Application\\ Support/Hades', 'macOS')).toBe('~/Library/Application Support/Hades');
  expect(cleanPath('/Volumes/Games\\ \\(old\\)/Hades.app', 'macOS')).toBe('/Volumes/Games (old)/Hades.app');
  expect(cleanPath("  '~/Library/Application Support/Hades'  ", 'Linux')).toBe('~/Library/Application Support/Hades');
  // Explorer's “Copy as path”.
  expect(cleanPath('"C:\\Games\\Hades\\Hades.exe"', 'Windows')).toBe('C:\\Games\\Hades\\Hades.exe');
  expect(cleanPath('"%APPDATA%\\Hades"', 'Windows')).toBe('%APPDATA%\\Hades');
});

test('cleaning keeps a Windows path recognizable on a Mac, and never touches Windows backslashes', () => {
  expect(cleanPath('C:\\Games\\Hades\\Hades.exe', 'macOS')).toBe('C:\\Games\\Hades\\Hades.exe');
  expect(cleanPath('"C:\\Program Files (x86)\\Hades"', 'macOS')).toBe('C:\\Program Files (x86)\\Hades');
  expect(platformProblem('executable', cleanPath('"C:\\Games\\Hades.exe"', 'macOS'), 'Hades', 'macOS')).toContain('This is a Windows location.');
  expect(cleanPath('C:\\Games\\ Hades', 'Windows')).toBe('C:\\Games\\ Hades');
  // Only matching quotes around the whole path.
  expect(cleanPath('"~/Hades', 'macOS')).toBe('"~/Hades');
});

test('a shared folder the host refuses is named in plain words', () => {
  const refusal = (folder: string) => ({ kind: 'invalid_target', target_cause: { kind: 'too_broad' }, paths: ['/Users', folder] });
  expect(tooBroadProblem(refusal('~'), 'Hades')).toBe(
    'This is your whole home folder, and many apps keep files there. Pick the folder inside it where Hades keeps its saves.');
  expect(tooBroadProblem(refusal('%USERPROFILE%'), 'Hades')).toContain('your whole home folder');
  expect(tooBroadProblem(refusal('~/Library/Application Support'), 'Hades')).toContain('“~/Library/Application Support” is a folder');
  for (const root of ['/', 'C:\\', 'd:']) expect(tooBroadProblem(refusal(root), 'Hades'), root).toContain('This is a whole drive, and many apps keep files there.');
  expect(tooBroadProblem({ kind: 'invalid_target', target_cause: { kind: 'overlap', name: 'FTL' } }, 'Hades')).toBeUndefined();
});
