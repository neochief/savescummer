import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import type { Bridge } from './bridge';
import type { FlushPreview, Game, HostState, SaveSet, SaveTarget } from './types';
import { version } from '../package.json';
import { capturedShortcut, displayShortcut, shortcutDescription, shortcutError, shortcutWarning } from './shortcuts';

export type DialogKind = 'settings' | 'add' | 'configure' | 'flush' | 'about';

function EyeIcon() {
  return <img className="icon" src="/icons/eye.svg" alt="" aria-hidden="true" />;
}

function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

/** The full path a save target covers: its root plus the exact name or glob. */
function targetPath({ root, filter }: SaveTarget) {
  if (filter.kind === 'all') return root;
  const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/';
  return root.replace(/[\\/]+$/, '') + sep + filter.value.replaceAll('/', sep);
}

function shortcutMessage(text: string, shortcut: string) {
  const description = shortcutDescription(shortcut);
  const at = description ? text.indexOf(description) : -1;
  if (!description || at < 0) return text;
  return <>{text.slice(0, at)}<strong>{description}</strong>{text.slice(at + description.length)}</>;
}

function DialogFrame({ title, kind, close, opener, children }: {
  title: ReactNode; kind: DialogKind; close: () => void; opener?: HTMLElement | null; children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    const returnFocus = opener || (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    dialog?.showModal();
    dialog?.focus();
    return () => { dialog?.close(); returnFocus?.focus(); };
  }, []);
  return <dialog ref={ref} tabIndex={-1} className={`dialog dialog-${kind}`} aria-labelledby={`dialog-title-${kind}`}
    onCancel={(event) => { event.preventDefault(); close(); }}
    onClick={(event) => { if (event.target === ref.current) close(); }}>
    <header><h2 id={`dialog-title-${kind}`}>{title}</h2><button type="button" className="dialog-close" onClick={close} aria-label="Close dialog"><img className="icon" src="/icons/xmark.svg" alt="" aria-hidden="true" /></button></header>
    {children}
  </dialog>;
}

function Footer({ close, submit, busy, disabled = false, destructive = false, extra }: { close: () => void; submit: string; busy: boolean; disabled?: boolean; destructive?: boolean; extra?: ReactNode }) {
  return <footer>
    {extra}
    {destructive ? <><button type="submit" name="intent" value="cancel" onClick={close}>Cancel</button><button className="dialog-primary danger" type="submit" name="intent" value="flush" disabled={busy || disabled}>{busy ? 'Working…' : submit}</button></>
      : <><button className="dialog-primary" type="submit" disabled={busy || disabled}>{busy ? 'Saving…' : submit}</button><button type="button" onClick={close}>Cancel</button></>}
  </footer>;
}

/** Copyright years run from the first release to the current year: "2026", then "2026–2027" and on. */
export function copyrightYears(now = new Date()) {
  const year = now.getFullYear();
  return year > 2026 ? `2026–${year}` : '2026';
}

export function AboutDialog({ bridge, close, opener }: { bridge: Bridge; close: () => void; opener?: HTMLElement | null }) {
  const [error, setError] = useState<string>();
  return <DialogFrame title="About" kind="about" close={close} opener={opener}>
    <div className="dialog-body about">
      <img className="about-icon" src="/app-icon.svg" alt="" aria-hidden="true" />
      <span className="wordmark" aria-label="SaveScummer"><span>Save</span><strong>Scummer</strong></span>
      <p className="about-version">Version {version}</p>
      <p className="about-copyright">© {copyrightYears()} Oleksandr Shvets. All rights reserved.</p>
      {error && <p className="dialog-error" role="alert">{error}</p>}
    </div>
    <footer>
      <button type="button" className="dialog-primary" onClick={() => bridge.openWebsite().catch((failure) => setError(message(failure)))}>Website</button>
      <button type="button" onClick={close}>Close</button>
    </footer>
  </DialogFrame>;
}

export function AppDialog({ kind, game, state, bridge, close, opener, onAdded, onFlushed, onFlush }: {
  kind: DialogKind; game?: Game; state?: HostState; bridge: Bridge; close: () => void; opener?: HTMLElement | null;
  onAdded: (id: string) => void; onFlushed: (operation: string) => void; onFlush?: (opener: HTMLElement) => void;
}) {
  const [name, setName] = useState(game?.kind === 'custom' ? game.name : '');
  const [executable, setExecutable] = useState(kind === 'configure' ? game?.executable || '' : '');
  const [resetExecutable, setResetExecutable] = useState(false);
  const [expertMode, setExpertMode] = useState(game?.expert_mode ?? false);
  const [location, setLocation] = useState('');
  const [focused, setFocused] = useState<'executable' | 'location'>();
  const [saveSet, setSaveSet] = useState<SaveSet>();
  const [checkpointsPath, setCheckpointsPath] = useState<string>();
  const [preview, setPreview] = useState<FlushPreview>();
  const [details, setDetails] = useState(false);
  const [sounds, setSounds] = useState(state?.settings?.play_sounds ?? true);
  const [startup, setStartup] = useState(state?.settings?.launch_on_startup ?? false);
  const [saveShortcut, setSaveShortcut] = useState(state?.settings?.save_shortcut || (navigator.platform.includes('Mac') ? 'Alt+F5' : 'Ctrl+F5'));
  const [loadShortcut, setLoadShortcut] = useState(state?.settings?.load_shortcut || (navigator.platform.includes('Mac') ? 'Alt+F9' : 'Ctrl+F9'));
  const [shortcutErrors, setShortcutErrors] = useState<{ save?: string; load?: string }>({});
  const saveWarning = shortcutErrors.save ? undefined : shortcutWarning(saveShortcut);
  const loadWarning = shortcutErrors.load ? undefined : shortcutWarning(loadShortcut);
  const [capturing, setCapturing] = useState<'save' | 'load'>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  // A catalog game with no location of its own shows the catalog's paths, read-only.
  const catalogTargets = kind === 'configure' && game?.kind !== 'custom' && location === '' && focused !== 'location' && saveSet?.catalog?.length
    ? saveSet.catalog : undefined;
  const catalogExcludes = (catalogTargets || []).flatMap((target) => (target.excludes || [])
    .map((exclude) => target.filter.kind === 'exact' && exclude.startsWith(`${target.filter.value}/`) ? exclude.slice(target.filter.value.length + 1) : exclude));
  const activeIndex = (target: SaveTarget) => (saveSet?.active || [])
    .findIndex((active) => active.root === target.root && JSON.stringify(active.filter) === JSON.stringify(target.filter));
  const openSaves = (target: number) => game && bridge.request({ type: 'open_saves', game: game.id, target }).catch((failure) => setError(message(failure)));
  const gameName = game ? `${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}` : '';

  useEffect(() => {
    if (kind !== 'settings') return;
    return () => { bridge.capture(false).catch(() => undefined); };
  }, [bridge, kind]);

  function captureKey(event: ReactKeyboardEvent<HTMLInputElement>, which: 'save' | 'load') {
    if (event.key === 'Tab' || event.key === 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    const shortcut = capturedShortcut(event);
    if (!shortcut) return;
    if (which === 'save') setSaveShortcut(shortcut);
    else setLoadShortcut(shortcut);
    setShortcutErrors((current) => ({ ...current, [which]: undefined }));
  }

  async function browse(which: 'executable' | 'location') {
    try {
      const picked = await open({ title: which === 'executable' ? 'Choose game executable' : 'Choose save folder',
        directory: which === 'location', multiple: false, fileAccessMode: 'scoped' });
      if (typeof picked !== 'string') return;
      if (which === 'location') setLocation(picked);
      else {
        setExecutable(picked);
        setResetExecutable(false);
        if (kind === 'add' && !name.trim()) setName(picked.split(/[/\\]/).pop()!.replace(/\.[^.]+$/, ''));
      }
    } catch (failure) { setError(message(failure)); }
  }

  useEffect(() => {
    if (!game || kind !== 'configure') return;
    let live = true;
    bridge.request<SaveSet>({ type: 'save_set', game: game.id })
      .then((value) => { if (live) { setSaveSet(value); setLocation(value.location || ''); } })
      .catch((failure) => { if (live) setError(message(failure)); });
    bridge.request<{ path: string }>({ type: 'open_checkpoints', game: game.id, resolve_only: true })
      .then((value) => { if (live) setCheckpointsPath(value.path); })
      .catch(() => { if (live) setCheckpointsPath(''); });
    return () => { live = false; };
  }, [bridge, game?.id, kind]);

  useEffect(() => {
    if (!game || kind !== 'flush') return;
    let live = true;
    bridge.request<FlushPreview>({ type: 'flush_preview', game: game.id, limit: 30 })
      .then((value) => { if (live) setPreview(value); })
      .catch((failure) => { if (live) setError(message(failure)); });
    return () => { live = false; };
  }, [bridge, game?.id, kind]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (kind === 'flush' && (event.nativeEvent as SubmitEvent).submitter?.getAttribute('value') !== 'flush') { close(); return; }
    if (kind === 'settings') {
      const errors = { save: shortcutError(saveShortcut, loadShortcut), load: shortcutError(loadShortcut, saveShortcut) };
      if (errors.save || errors.load) { setShortcutErrors(errors); return; }
    }
    setError(undefined);
    setBusy(true);
    try {
      if (kind === 'add') {
        if (!name.trim() || !executable.trim() || !location.trim()) throw new Error('Fill in the name, executable, and save location.');
        const result = await bridge.request<{ game: string }>({ type: 'add_game', name: name.trim(), executable: executable.trim(), save_location: location.trim() });
        onAdded(result.game);
      } else if (kind === 'configure' && game) {
        const custom = game.kind === 'custom';
        await bridge.request({ type: 'configure', game: game.id,
          name: custom ? name.trim() : undefined,
          executable: !resetExecutable && executable.trim() !== (game.executable || '') ? executable.trim() : undefined,
          save_location: location.trim() || undefined,
          reset_executable: resetExecutable, reset_save_location: !custom && !location.trim() && Boolean(saveSet?.location),
          expert_mode: expertMode !== (game.expert_mode ?? false) ? expertMode : undefined,
        });
        close();
      } else if (kind === 'settings') {
        await bridge.request({ type: 'settings', play_sounds: sounds,
          launch_on_startup: state?.settings?.launch_on_startup_available && startup !== state.settings.launch_on_startup ? startup : undefined,
          save_shortcut: saveShortcut, load_shortcut: loadShortcut });
        close();
      } else if (kind === 'flush' && game) {
        const accepted = await bridge.request<{ id: string }>({ type: 'flush', game: game.id });
        close();
        onFlushed(accepted.id);
      }
    } catch (failure) {
      const detail = message(failure);
      if (kind === 'settings' && detail.startsWith('Save shortcut:')) setShortcutErrors({ save: detail.slice(14).trim() });
      else if (kind === 'settings' && detail.startsWith('Load shortcut:')) setShortcutErrors({ load: detail.slice(14).trim() });
      else setError(detail);
    } finally {
      setBusy(false);
    }
  }

  // The game name reads dimmer than the action, like the guidance panel's title detail.
  const title = kind === 'settings' ? 'Settings' : kind === 'add' ? 'Add custom game'
    : <>{kind === 'configure' ? 'Configure' : 'Flush checkpoints'}<span className="dialog-title-detail"> — {gameName}</span></>;
  // Configure opens once its paths have arrived, so its contents don't rearrange while it's visible.
  if (kind === 'configure' && !((saveSet || error) && checkpointsPath !== undefined)) return null;
  return <DialogFrame title={title} kind={kind} close={close} opener={opener}>
    <form onSubmit={submit}>
      <div className="dialog-body">
        {kind === 'settings' && <>
          <div className="dialog-field"><label htmlFor="save-shortcut">Save shortcut</label><input id="save-shortcut" data-shortcut-field="save"
            value={displayShortcut(saveShortcut)} readOnly aria-invalid={Boolean(shortcutErrors.save)} aria-describedby={shortcutErrors.save ? 'save-shortcut-error' : saveWarning ? 'save-shortcut-warning' : undefined}
            className={capturing === 'save' ? 'capturing' : ''}
            onFocus={() => { setCapturing('save'); bridge.capture(true).catch((failure) => setError(message(failure))); }}
            onBlur={(event) => { setCapturing(undefined); if (!(event.relatedTarget instanceof HTMLElement && event.relatedTarget.dataset.shortcutField)) bridge.capture(false).catch(() => undefined); }}
            onKeyDown={(event) => captureKey(event, 'save')} /></div>
          {shortcutErrors.save && <p id="save-shortcut-error" className="dialog-error shortcut-error" role="alert">{shortcutMessage(shortcutErrors.save, saveShortcut)}</p>}
          {saveWarning && <p id="save-shortcut-warning" className="dialog-warning shortcut-warning" role="status">{shortcutMessage(saveWarning, saveShortcut)}</p>}
          <div className="dialog-field"><label htmlFor="load-shortcut">Load shortcut</label><input id="load-shortcut" data-shortcut-field="load"
            value={displayShortcut(loadShortcut)} readOnly aria-invalid={Boolean(shortcutErrors.load)} aria-describedby={shortcutErrors.load ? 'load-shortcut-error' : loadWarning ? 'load-shortcut-warning' : undefined}
            className={capturing === 'load' ? 'capturing' : ''}
            onFocus={() => { setCapturing('load'); bridge.capture(true).catch((failure) => setError(message(failure))); }}
            onBlur={(event) => { setCapturing(undefined); if (!(event.relatedTarget instanceof HTMLElement && event.relatedTarget.dataset.shortcutField)) bridge.capture(false).catch(() => undefined); }}
            onKeyDown={(event) => captureKey(event, 'load')} /></div>
          {shortcutErrors.load && <p id="load-shortcut-error" className="dialog-error shortcut-error" role="alert">{shortcutMessage(shortcutErrors.load, loadShortcut)}</p>}
          {loadWarning && <p id="load-shortcut-warning" className="dialog-warning shortcut-warning" role="status">{shortcutMessage(loadWarning, loadShortcut)}</p>}
          <label className="dialog-check"><input type="checkbox" checked={sounds} onChange={(event) => setSounds(event.target.checked)} />Play sounds</label>
          <label className="dialog-check"><input type="checkbox" checked={startup} disabled={!state?.settings?.launch_on_startup_available}
            onChange={(event) => setStartup(event.target.checked)} />Launch on startup</label>
          {state?.settings?.launch_on_startup_needs_approval && <p className="dialog-hint">Enable SaveScummer in Login Items to allow startup.</p>}
        </>}
        {(kind === 'add' || kind === 'configure') && <>
          {kind === 'configure' && game?.kind === 'custom' && <div className="dialog-field"><label htmlFor="game-name">Name</label><input id="game-name" value={name} onChange={(event) => setName(event.target.value)} required /></div>}
          <div className="dialog-field"><label htmlFor="game-executable">Game executable</label><span className="dialog-input"><input id="game-executable" value={executable}
            onFocus={() => setFocused('executable')} onBlur={() => setFocused(undefined)}
            onChange={(event) => { setExecutable(event.target.value); setResetExecutable(false); }} required />
              {kind === 'configure' && game && <button type="button" className="dialog-icon-button" aria-label="Open game executable" title="Show in folder"
                disabled={resetExecutable || executable !== (game.executable || '')}
                onClick={() => bridge.request({ type: 'open_executable', game: game.id }).catch((failure) => setError(message(failure)))}><EyeIcon /></button>}</span>
            <button type="button" onClick={() => browse('executable')}>Change</button>
          </div>
          {kind === 'configure' && game && game.kind !== 'custom' && <p className="dialog-below">
            {focused !== 'executable' && !resetExecutable && (game.executable_overridden || executable !== (game.executable || ''))
              && <button type="button" className="dialog-reset" onClick={() => { setExecutable(game.executable || ''); setResetExecutable(true); }}>Reset</button>}
          </p>}
          {catalogTargets ? <>
            <div className="dialog-field dialog-paths"><label htmlFor="save-location">Save location</label>
              <span className="dialog-path-list">{catalogTargets.map((target, i) => {
                const path = targetPath(target);
                const active = activeIndex(target);
                return <span className="dialog-input" key={path}><input id={i === 0 ? 'save-location' : undefined} aria-label={i === 0 ? undefined : 'Save location'} value={path} disabled />
                  <button type="button" className="dialog-icon-button" aria-label="Open save location" title="Show in folder" disabled={active < 0}
                    onClick={() => openSaves(active)}><EyeIcon /></button></span>;
              })}</span>
              <button type="button" onClick={() => browse('location')}>Change</button>
            </div>
            {catalogExcludes.length > 0 && <p className="dialog-below"><span className="dialog-hint">Except {catalogExcludes.join(', ')}</span></p>}
          </> : <>
            <div className="dialog-field"><label htmlFor="save-location">Save location</label><span className="dialog-input"><input id="save-location" value={location}
              onFocus={() => setFocused('location')} onBlur={() => setFocused(undefined)}
              onChange={(event) => setLocation(event.target.value)} required={kind === 'add' || game?.kind === 'custom'} />
                {kind === 'configure' && game && <button type="button" className="dialog-icon-button" aria-label="Open save location" title="Show in folder"
                  disabled={!saveSet?.location || location !== saveSet.location || !saveSet.active.length} onClick={() => openSaves(0)}><EyeIcon /></button>}</span>
              <button type="button" onClick={() => browse('location')}>Change</button>
            </div>
            <p className="dialog-below">
              {focused === 'location' ? <span className="dialog-hint">A folder, a file, or a pattern such as D:\Game\saves\*.sav</span>
                : kind === 'configure' && game?.kind !== 'custom' && location.trim() !== ''
                  && <button type="button" className="dialog-reset" onClick={() => setLocation('')}>Reset</button>}
            </p>
          </>}
          {saveSet?.catalog_problem && <p className="dialog-hint">{saveSet.catalog_problem}</p>}
          {kind === 'configure' && game && <div className="dialog-field"><label htmlFor="checkpoints-store">Checkpoints store</label>
            <span className="dialog-input"><input id="checkpoints-store" value={checkpointsPath} disabled />
              <button type="button" className="dialog-icon-button" aria-label="Open checkpoints store" title="Show in folder"
                onClick={() => bridge.request({ type: 'open_checkpoints', game: game.id }).catch((failure) => setError(message(failure)))}><EyeIcon /></button></span>
          </div>}
          {kind === 'configure' && game && <div className="dialog-expert-mode">
            <label className="dialog-check"><input type="checkbox" checked={expertMode} onChange={(event) => setExpertMode(event.target.checked)} />
              Expert mode</label>
            <ul className="dialog-expert-mode-note">
              <li><strong>Allow saving and loading while the game is running.</strong> Beware: this won’t work as expected for LOTS of games that keep progress in memory. For those games, we can only save or load progress while it’s on disk and the game is stopped. But some games can be fooled into saving progress mid-game. This depends on the game and takes expert save-scumming skills to figure out.</li>
              <li><strong>Allow terminating a running game from its game card.</strong> This is much faster than quitting through the game’s menus, so it’s super efficient for save scumming. But it may also prevent some games from saving properly and cause problems. Knowing which games are safe to terminate takes expert save-scumming skills.</li>
            </ul>
          </div>}
          {kind === 'add' && <div className="dialog-field"><label htmlFor="game-name">Name</label><input id="game-name" value={name} onChange={(event) => setName(event.target.value)} required /></div>}
        </>}
        {kind === 'flush' && <>
          <p>Permanently delete all backups and clear this game's history?<br />Your current game data will be kept.</p>
          {preview ? <><dl className="flush-counts">
            {preview.saved > 0 && <><dt>Saved backups</dt><dd>{preview.saved}</dd></>}
            {preview.recovery > 0 && <><dt>Recovery points</dt><dd>{preview.recovery}</dd></>}
            {preview.temporary > 0 && <><dt>Incomplete copies</dt><dd>{preview.temporary}</dd></>}
            <dt>Total</dt><dd>{formatBytes(preview.size)}</dd>
          </dl>{preview.items.length > 0 && <button type="button" className="details-toggle" onClick={() => setDetails(!details)} aria-expanded={details}>{details ? '▾' : '▸'} Details</button>}
            {details && preview.items.length > 0 && <div className="flush-details">{preview.items.map((item) => <div key={item.path}><span>{item.kind}</span> {item.path} {item.label}</div>)}
              {preview.next && <button type="button" onClick={async () => {
                try { const page = await bridge.request<FlushPreview>({ type: 'flush_preview', game: game!.id, cursor: preview.next, limit: 30 });
                  setPreview({ ...page, items: [...preview.items, ...page.items] }); } catch (failure) { setError(message(failure)); }
              }}>Show more</button>}</div>}
          </> : !error && <p>Checking checkpoints…</p>}
        </>}
        {error && <p className="dialog-error" role="alert">{error}</p>}
      </div>
      <Footer close={close} submit={kind === 'add' ? 'Add' : kind === 'flush' ? 'Flush' : 'Save'} busy={busy || (kind === 'flush' && !preview) || (kind === 'configure' && !saveSet)} destructive={kind === 'flush'}
        disabled={!!game && ((kind === 'flush' && !game.flush.available) || (kind === 'configure' && !game.configure.available))}
        extra={kind === 'configure' && game && onFlush && <button type="button" className="dialog-flush"
          disabled={busy || !game.flush.available || (!game.has_history && !game.checkpoints_size)} onClick={(event) => onFlush(event.currentTarget)}>
          <span className="dialog-button-icon trash" />Flush checkpoints{game.checkpoints_size ? ` (${formatBytes(game.checkpoints_size)})` : ''}
        </button>} />
    </form>
  </DialogFrame>;
}

export function formatBytes(bytes: number) {
  const base = navigator.platform.includes('Win') ? 1024 : 1000;
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= base && unit < units.length - 1) { value /= base; unit += 1; }
  return `${unit === 0 ? Math.round(value) : value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}
