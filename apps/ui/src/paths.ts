import { platformName } from './bridge';

export type Platform = 'macOS' | 'Windows' | 'Linux';

export const executableExamples: Record<Platform, string> = {
  macOS: '/Applications/VoidWar.app', Windows: 'C:\\Games\\VoidWar\\VoidWar.exe', Linux: '~/Games/VoidWar/VoidWar',
};

export type PathField = 'executable' | 'location';

/**
 * Why a typed path belongs to another platform, in words a non-technical player follows.
 * The host keeps the user's folders portable (`~` on macOS and Linux, `%APPDATA%` and
 * the like on Windows), so those forms are this platform's own.
 */
export function platformProblem(field: PathField, value: string, game: string, platform: Platform = platformName) {
  const path = value.trim();
  if (!path) return undefined;
  if (platform === 'Windows') {
    if (/^(\/(?!\/)|~|\$HOME)/.test(path)) return field === 'executable'
      ? `This is a Mac or Linux location. On Windows, locations start with a drive letter, like ${executableExamples.Windows}. Click Change and pick the game’s .exe file.`
      : `This is where the game keeps saves on a Mac or Linux. On Windows, locations start with a drive letter, like C:\\Users\\you\\Documents\\My Games. Look up where ${game} keeps saves on Windows.`;
    if (field === 'executable' && /\.app[\\/]?$/i.test(path)) return '“.app” is a Mac program, and Windows can’t open it. Pick the game’s .exe file instead.';
    return undefined;
  }
  const mac = platform === 'macOS';
  const variable = path.match(/%[a-z_]+%/i)?.[0];
  const drive = path.match(/^([a-z]):/i)?.[1].toUpperCase();
  if (variable || drive || path.includes('\\')) {
    const why = variable ? `Folders like “${variable}” only exist on Windows.`
      : drive ? `${mac ? 'Macs don’t' : 'Linux doesn’t'} have a “${drive}:” drive.`
        : `Locations on ${mac ? 'a Mac' : 'Linux'} use “/”, not “\\”.`;
    return field === 'executable'
      ? `This is a Windows location. ${why} Click Change and pick the game${mac ? ' in your Applications folder' : ''}.`
      : `This is where the game keeps saves on Windows. ${why} Look up where ${game} keeps saves on ${platform}.`;
  }
  // Linux runs Windows games through Proton or Wine, so only a Mac rejects them.
  const windowsProgram = mac && field === 'executable' && path.match(/\.(exe|bat|msi)$/i)?.[0].toLowerCase();
  if (windowsProgram) return `“${windowsProgram}” files are Windows programs, and Macs can’t open them. `
    + 'Pick the Mac version of the game instead. Its name usually ends with “.app”.';
  return undefined;
}
