import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import type { Bridge } from './bridge';
import type { Game, Guidance } from './types';
import { failureMessage } from './messages';

const content: Record<Guidance['kind'], [string, string, string]> = {
  blocked: ['Recover', 'the interrupted operation', 'SaveScummer will check the interrupted operation and finish or undo changes when safe.'],
  access_needed: ['Allow access', 'to this game’s saves', ''],
  no_save_location: ['Choose', 'this game’s save folder', ''],
  invalid_target: ['Fix', 'the save location', ''],
  target_unavailable: ['Save location unavailable', '', 'Reconnect the drive or restore access to the saves folder.'],
  game_running: ['Exit the game', 'to save or load checkpoints', 'Save copies progress after the game writes it on exit.\nLoad replaces it for the game to read on relaunch.'],
  play_first: ['Play first', 'to save progress', 'Start the game, make some progress, then save and exit.'],
  no_game_data: ['No game data to save', '', 'You can still load a checkpoint.'],
  no_saves: ['No checkpoints yet', '', 'Go and play the game first.'],
};

export function GuidancePanel({ game, bridge, retrying, onRetry, onConfigure }: {
  game: Game; bridge: Bridge; retrying: boolean; onRetry: () => void; onConfigure: (button: HTMLButtonElement) => void;
}) {
  const [asking, setAsking] = useState(false);
  const [browsing, setBrowsing] = useState(false);
  const [error, setError] = useState<string>();
  const guidance = game.guidance;
  if (!guidance) return null;
  const [action, detail, text] = content[guidance.kind];
  const title = detail ? `${action} ${detail}` : action;
  const denied = guidance.failure?.access?.denied;
  const requestAccess = async () => {
    setAsking(true); setError(undefined);
    try {
      await bridge.request({ type: denied ? 'open_access_settings' : 'request_access', game: game.id });
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setAsking(false); }
  };
  const browse = async () => {
    setBrowsing(true); setError(undefined);
    try {
      const picked = await open({ title: 'Choose save folder', directory: true, multiple: false, fileAccessMode: 'scoped' });
      if (typeof picked !== 'string') return;
      await bridge.request({ type: 'configure', game: game.id, save_location: picked,
        reset_executable: false, reset_save_location: false });
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBrowsing(false); }
  };
  const message = guidance.kind === 'blocked' && !game.retry.available && !retrying
    ? failureMessage(game.retry.failure, text)
    : guidance.kind === 'invalid_target' ? failureMessage(guidance.failure, text) : text;
  const coverage = guidance.save && guidance.load ? 'both' : guidance.save ? 'save' : 'load';
  return <section className={`guidance-panel covers-${coverage}`} data-kind={guidance.kind} role="status"
    aria-label={guidance.kind === 'game_running' ? 'Exit the game first' : title}>
    <div className="guidance-content">
      <strong>{action}{detail && <>{' '}<span className="guidance-title-detail">{detail}</span></>}</strong>
      {(message || error) && <div className="guidance-text">
        {message && <p className="guidance-description">{message}</p>}
        {error && <p role="alert">{error}</p>}
      </div>}
      {guidance.remedy && <div className="guidance-buttons">
        {guidance.kind === 'no_save_location' && guidance.remedy === 'configure'
          ? <button disabled={!game.configure.available || browsing} onClick={browse}>{browsing ? 'Browsing' : 'Browse'}</button>
          : guidance.remedy === 'configure' && <button disabled={!game.configure.available} onClick={(event) => onConfigure(event.currentTarget)}>Configure</button>}
        {guidance.remedy === 'retry' && <button disabled={!game.retry.available || retrying} onClick={onRetry}>
          {retrying ? 'Recovering' : 'Try recovery again'}</button>}
        {guidance.remedy === 'request_access' && <button disabled={asking} onClick={requestAccess}>
          {asking ? 'Waiting for access' : denied ? 'Open System Settings' : 'Allow access'}</button>}
      </div>}
    </div>
  </section>;
}
