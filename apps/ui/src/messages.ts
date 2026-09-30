import type { Failure } from './types';

const accessCategories: Record<string, string> = {
  documents: 'Documents', desktop: 'Desktop', downloads: 'Downloads',
  icloud_drive: 'iCloud Drive', volumes: 'removable or network drives',
  app_data: "other apps' data", app_bundles: 'app bundles',
};

export function failureMessage(failure: Failure | undefined, fallback: string): string {
  if (!failure) return fallback;
  if (failure.kind === 'access_needed') {
    const category = failure.access?.category || failure.detail || '';
    return `SaveScummer needs permission to access ${accessCategories[category] || "this game's files"}.`;
  }
  if (failure.kind === 'invalid_target') {
    const cause = failure.target_cause;
    const path = failure.paths?.[0];
    const descriptions: Record<string, string> = {
      too_broad: 'Choose a location containing only this game’s saves.',
      executable: 'This location includes the game’s executable.',
      reserved_name: 'Names ending in .ssnew or .ssold are reserved for recovery.',
      unresolved_link: 'The save location’s link cannot be resolved.',
      changed_link: 'The save location’s link points somewhere different now.',
      not_directory: 'The save location must be a folder.',
    };
    const explanation = cause?.kind === 'overlap' ? `This location overlaps ${cause.name || 'another game'}’s saves.`
      : descriptions[cause?.kind || ''] || 'Choose a valid save location.';
    return path ? `${explanation} (${path})` : explanation;
  }
  const descriptions: Record<string, string> = {
    game_running: 'Save and exit the game before continuing.',
    busy: 'Another operation is in progress.', blocked: 'Finish recovery of the interrupted operation first.',
    no_game_data: 'The game has no saves on disk yet.', no_saves: 'There are no usable saved checkpoints yet.',
    no_save_location: 'Browse for the game’s save folder.',
    target_unavailable: 'Reconnect the drive or restore access to the saves folder.',
    store_unavailable: 'The checkpoint store is unavailable. Reconnect its drive to continue.',
  };
  return descriptions[failure.kind] || failure.detail || fallback;
}
