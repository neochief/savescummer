import { useEffect, useRef, useState, type FormEvent, type MouseEvent, type ReactNode } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { HostError, platformName, type Bridge } from './bridge';
import { cleanPath, executableExamples, patternExamples, platformProblem, tooBroadProblem, type PathField } from './paths';
import type { FlushPreview, Game, HostState, SaveSet, SaveTarget } from './types';
import { version } from '../package.json';
import { shortcutError } from './shortcuts/shortcuts';
import { ShortcutInput } from './shortcuts/ShortcutInput';

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

const executableExample = executableExamples[platformName];

function SearchLinks({ game, bridge, onError }: { game: string; bridge: Bridge; onError: (message: string) => void }) {
  const search = (engine: 'google' | 'chatgpt') => (event: MouseEvent) => {
    event.preventDefault();
    bridge.openSaveSearch(engine, game).catch((failure) => onError(message(failure)));
  };
  return <><a href="#" onClick={search('google')}>Google</a> or <a href="#" onClick={search('chatgpt')}>ChatGPT</a></>;
}

/** `hideTitle` drops the header bar; the title stays for screen readers and only the close button shows. */
function DialogFrame({ title, hideTitle = false, kind, close, opener, children }: {
  title: ReactNode; hideTitle?: boolean; kind: DialogKind; close: () => void; opener?: HTMLElement | null; children: ReactNode;
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
    {hideTitle ? <h2 id={`dialog-title-${kind}`} className="sr-only">{title}</h2>
      : <header><h2 id={`dialog-title-${kind}`}>{title}</h2></header>}
    {children}
    {/* After the content so it comes last in tab order; CSS pins it to the header's corner. */}
    <button type="button" className="dialog-close" onClick={close} aria-label="Close dialog"><img className="icon" src="/icons/xmark.svg" alt="" aria-hidden="true" /></button>
  </dialog>;
}

function Footer({ close, submit, busy, disabled = false, destructive = false }: { close: () => void; submit: string; busy: boolean; disabled?: boolean; destructive?: boolean }) {
  return <footer>
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
  return <DialogFrame title="About" hideTitle kind="about" close={close} opener={opener}>
    <div className="dialog-body about">
      <img className="about-icon" src="/app-icon.svg" alt="" aria-hidden="true" />
      <span className="wordmark" aria-label="SaveScummer"><span>Save</span><strong>Scummer</strong></span>
      <p className="about-version">Version {version}</p>
      <p className="about-copyright">© {copyrightYears()} Alexander Shvets. All rights reserved.</p>
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
  onAdded: (id: string, existing: boolean) => void; onFlushed: (operation: string) => void; onFlush?: (opener: HTMLElement) => void;
}) {
  const [name, setName] = useState(game?.kind === 'custom' ? game.name : '');
  const [executable, setExecutable] = useState(kind === 'configure' ? game?.executable || '' : '');
  const [resetExecutable, setResetExecutable] = useState(false);
  const [expertMode, setExpertMode] = useState(game?.expert_mode ?? false);
  const [location, setLocation] = useState('');
  const [focused, setFocused] = useState<PathField>();
  // The host refused the save location as a folder many apps share.
  const [tooBroad, setTooBroad] = useState<string>();
  // A field's platform problem shows once the user leaves it or submits, not while typing.
  const [checked, setChecked] = useState<Partial<Record<PathField, boolean>>>({});
  const [saveSet, setSaveSet] = useState<SaveSet>();
  const [checkpointsPath, setCheckpointsPath] = useState<string>();
  const [preview, setPreview] = useState<FlushPreview>();
  const [details, setDetails] = useState(false);
  const [sounds, setSounds] = useState(state?.settings?.play_sounds ?? true);
  const [startup, setStartup] = useState(state?.settings?.launch_on_startup ?? false);
  const [flushOld, setFlushOld] = useState(state?.settings?.flush_old_checkpoints ?? true);
  const defaultShortcuts = navigator.platform.includes('Mac') ? { save: 'Alt+F5', load: 'Alt+F9' } : { save: 'Ctrl+F5', load: 'Ctrl+F9' };
  // An empty shortcut is one the user removed.
  const [saveShortcut, setSaveShortcut] = useState(state?.settings?.save_shortcut ?? defaultShortcuts.save);
  const [loadShortcut, setLoadShortcut] = useState(state?.settings?.load_shortcut ?? defaultShortcuts.load);
  const [rejections, setRejections] = useState<{ save?: string; load?: string }>({});
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  // A catalog game with no location of its own shows the catalog's paths, read-only.
  const catalogTargets = kind === 'configure' && game?.kind !== 'custom' && location === '' && focused !== 'location' && saveSet?.catalog?.length
    ? saveSet.catalog : undefined;
  const activeIndex = (target: SaveTarget) => (saveSet?.active || [])
    .findIndex((active) => active.root === target.root && JSON.stringify(active.filter) === JSON.stringify(target.filter));
  const openSaves = (target: number) => game && bridge.request({ type: 'open_saves', game: game.id, target }).catch((failure) => setError(message(failure)));
  const gameName = game ? `${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}` : '';
  const searchName = (kind === 'add' ? name.trim() : game?.name) || 'GAME';
  const typed = { executable: cleanPath(executable), location: cleanPath(location) };
  // Only what the user typed is checked: a value the host already holds stays as it is.
  const problems = {
    executable: typed.executable !== (game?.executable || '') ? platformProblem('executable', typed.executable, searchName) : undefined,
    location: !catalogTargets && typed.location !== (saveSet?.location || '') ? platformProblem('location', typed.location, searchName) : undefined,
  };
  const shown = (field: PathField) => (checked[field] ? problems[field] : undefined) ?? (field === 'location' ? tooBroad : undefined);
  // A new game is named after its program, unless the user already named it.
  const nameAfter = (path: string) => setName((current) => current.trim() ? current : path.split(/[/\\]/).pop()!.replace(/\.[^.]+$/, ''));
  // Leaving a field shows the path as it will be used.
  const leave = (field: PathField) => {
    setFocused(undefined);
    setChecked((value) => ({ ...value, [field]: true }));
    if (field === 'executable' && typed.executable !== executable) setExecutable(typed.executable);
    if (field === 'location' && typed.location !== location) setLocation(typed.location);
    // A typed or pasted program names the game like a picked one, once the host confirms it exists.
    if (field === 'executable' && kind === 'add' && !name.trim() && typed.executable && !problems.executable) {
      const path = typed.executable;
      bridge.request<{ path: string | null; exists?: boolean }>({ type: 'picker_start', path })
        .then((result) => { if (result.exists) nameAfter(path); }, () => undefined);
    }
  };

  useEffect(() => {
    if (kind !== 'settings') return;
    return () => { bridge.capture(false).catch(() => undefined); };
  }, [bridge, kind]);

  function changeShortcut(which: 'save' | 'load', shortcut: string) {
    if (which === 'save') setSaveShortcut(shortcut);
    else setLoadShortcut(shortcut);
    setRejections((current) => ({ ...current, [which]: undefined }));
  }

  // Hotkeys pause while a field records, so pressing the current one doesn't fire it.
  const recordShortcut = (recording: boolean) => bridge.capture(recording).catch((failure) => { if (recording) setError(message(failure)); });

  async function browse(which: 'executable' | 'location') {
    try {
      // The picker starts where the field points, as far as that exists.
      const current = which === 'executable' ? typed.executable : catalogTargets ? targetPath(catalogTargets[0]) : typed.location;
      const start = current ? await bridge.request<{ path: string | null }>({ type: 'picker_start', path: current })
        .then((result) => result.path ?? undefined, () => undefined) : undefined;
      const picked = await open({ title: which === 'executable' ? 'Choose game executable' : 'Choose save folder',
        directory: which === 'location', multiple: false, fileAccessMode: 'scoped', ...(start && { defaultPath: start }) });
      if (typeof picked !== 'string') return;
      setChecked((value) => ({ ...value, [which]: true }));
      if (which === 'location') setLocation(picked);
      else {
        setExecutable(picked);
        setResetExecutable(false);
        if (kind === 'add') nameAfter(picked);
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
      // The fields already show why.
      if (shortcutError(saveShortcut, loadShortcut) || shortcutError(loadShortcut, saveShortcut)) return;
    }
    if ((kind === 'add' || kind === 'configure') && (problems.executable || problems.location)) {
      setChecked({ executable: true, location: true });
      document.getElementById(problems.executable ? 'game-executable' : 'save-location')?.focus();
      return;
    }
    setError(undefined);
    setBusy(true);
    try {
      if (kind === 'add') {
        if (!name.trim() || !typed.executable || !typed.location) throw new Error('Fill in the name, executable, and save location.');
        const result = await bridge.request<{ game: string; existing?: boolean }>({ type: 'add_game', name: name.trim(), executable: typed.executable, save_location: typed.location });
        onAdded(result.game, Boolean(result.existing));
      } else if (kind === 'configure' && game) {
        const custom = game.kind === 'custom';
        await bridge.request({ type: 'configure', game: game.id,
          name: custom ? name.trim() : undefined,
          executable: !resetExecutable && typed.executable !== (game.executable || '') ? typed.executable : undefined,
          save_location: typed.location || undefined,
          reset_executable: resetExecutable, reset_save_location: !custom && !typed.location && Boolean(saveSet?.location),
          expert_mode: expertMode !== (game.expert_mode ?? false) ? expertMode : undefined,
        });
        close();
      } else if (kind === 'settings') {
        await bridge.request({ type: 'settings', play_sounds: sounds,
          launch_on_startup: state?.settings?.launch_on_startup_available && startup !== state.settings.launch_on_startup ? startup : undefined,
          save_shortcut: saveShortcut, load_shortcut: loadShortcut, flush_old_checkpoints: flushOld });
        close();
      } else if (kind === 'flush' && game) {
        const accepted = await bridge.request<{ id: string }>({ type: 'flush', game: game.id });
        close();
        onFlushed(accepted.id);
      }
    } catch (failure) {
      const broad = failure instanceof HostError ? tooBroadProblem(failure.failure, searchName) : undefined;
      if (broad) {
        setTooBroad(broad);
        document.getElementById('save-location')?.focus();
        return;
      }
      const detail = message(failure);
      if (kind === 'settings' && detail.startsWith('Save shortcut:')) setRejections({ save: detail.slice(14).trim() });
      else if (kind === 'settings' && detail.startsWith('Load shortcut:')) setRejections({ load: detail.slice(14).trim() });
      else setError(detail);
    } finally {
      setBusy(false);
    }
  }

  // The game name reads dimmer than the action, like the guidance panel's title detail.
  const title = kind === 'settings' ? 'Settings' : kind === 'add' ? 'Add custom game'
    : <>{kind === 'configure' ? 'Configure' : 'Flush checkpoints'}{' '}<span className="dialog-title-detail">{gameName}</span></>;
  // Configure opens once its paths have arrived, so its contents don't rearrange while it's visible.
  if (kind === 'configure' && !((saveSet || error) && checkpointsPath !== undefined)) return null;
  return <DialogFrame title={title} kind={kind} close={close} opener={opener}>
    <form onSubmit={submit}>
      <div className="dialog-body">
        {kind === 'settings' && <>
          <div className="dialog-group">
          <ShortcutInput id="save-shortcut" label="Save shortcut" value={saveShortcut} fallback={defaultShortcuts.save} other={loadShortcut}
            rejection={rejections.save} onChange={(shortcut) => changeShortcut('save', shortcut)} onRecording={recordShortcut} />
          <ShortcutInput id="load-shortcut" label="Load shortcut" value={loadShortcut} fallback={defaultShortcuts.load} other={saveShortcut}
            rejection={rejections.load} onChange={(shortcut) => changeShortcut('load', shortcut)} onRecording={recordShortcut} />
          </div>
          <div className="dialog-group">
          <label className="dialog-check"><input type="checkbox" checked={sounds} onChange={(event) => setSounds(event.target.checked)} />Play sounds</label>
          <label className="dialog-check"><input type="checkbox" checked={startup} disabled={!state?.settings?.launch_on_startup_available}
            onChange={(event) => setStartup(event.target.checked)} />Launch on startup</label>
          {state?.settings?.launch_on_startup_needs_approval && <p className="dialog-hint">Enable SaveScummer in Login Items to allow startup.</p>}
          <label className="dialog-check"><input type="checkbox" checked={flushOld} onChange={(event) => setFlushOld(event.target.checked)} />Flush checkpoints older than 30 days</label>
          </div>
        </>}
        {(kind === 'add' || kind === 'configure') && <>
          <div className="dialog-group">
          {kind === 'configure' && game?.kind === 'custom' && <div className="dialog-field"><label htmlFor="game-name">Name</label><input id="game-name" className="dialog-name" value={name} onChange={(event) => setName(event.target.value)} required /></div>}
          <div className="dialog-field"><label htmlFor="game-executable">Game executable</label><span className="dialog-input"><input id="game-executable" value={executable}
            onFocus={() => setFocused('executable')} onBlur={() => leave('executable')} aria-invalid={Boolean(shown('executable'))}
            onChange={(event) => { setExecutable(event.target.value); setResetExecutable(false); setChecked((value) => ({ ...value, executable: false })); }}
            disabled={kind === 'configure'} required={kind === 'add'} />
              {kind === 'configure' && game && <button type="button" className="dialog-icon-button" aria-label="Open game executable" title="Show in folder"
                disabled={resetExecutable || executable !== (game.executable || '')}
                onClick={() => bridge.request({ type: 'open_executable', game: game.id }).catch((failure) => setError(message(failure)))}><EyeIcon /></button>}</span>
            <button type="button" onClick={() => browse('executable')}>{kind === 'add' ? 'Choose' : 'Change'}</button>
          </div>
          {shown('executable') ? <p className="dialog-problem" role="alert">{shown('executable')}</p> : <>
          {kind === 'add' && <p className="dialog-hint">Example: {executableExample}</p>}
          {kind === 'configure' && game && game.kind !== 'custom' && <p className="dialog-below">
            {focused !== 'executable' && !resetExecutable && (game.executable_overridden || executable !== (game.executable || ''))
              && <button type="button" className="dialog-reset" onClick={() => { setExecutable(game.executable || ''); setResetExecutable(true); }}>Reset</button>}
          </p>}
          </>}
          {kind === 'add' && <div className="dialog-field"><label htmlFor="game-name">Name</label><input id="game-name" className="dialog-name" value={name} onChange={(event) => setName(event.target.value)} required /></div>}
          {catalogTargets ? <>
            <div className="dialog-field dialog-paths"><label htmlFor="save-location">Where the game keeps its save files</label>
              <span className="dialog-path-list">{catalogTargets.map((target, i) => {
                const path = targetPath(target);
                const active = activeIndex(target);
                return <span className="dialog-input" key={path}><input id={i === 0 ? 'save-location' : undefined} aria-label={i === 0 ? undefined : 'Where the game keeps its save files'} value={path} disabled />
                  <button type="button" className="dialog-icon-button" aria-label="Open save location" title="Show in folder" disabled={active < 0}
                    onClick={() => openSaves(active)}><EyeIcon /></button></span>;
              })}</span>
              <button type="button" onClick={() => browse('location')}>{kind === 'add' ? 'Choose' : 'Change'}</button>
            </div>
          </> : <>
            <div className="dialog-field"><label htmlFor="save-location">Where the game keeps its save files</label><span className="dialog-input"><input id="save-location" value={location}
              onFocus={() => setFocused('location')} onBlur={() => leave('location')} aria-invalid={Boolean(shown('location'))}
              onChange={(event) => { setLocation(event.target.value); setChecked((value) => ({ ...value, location: false })); setTooBroad(undefined); }} required={kind === 'add' || game?.kind === 'custom'} />
                {kind === 'configure' && game && <button type="button" className="dialog-icon-button" aria-label="Open save location" title="Show in folder"
                  disabled={!saveSet?.location || location !== saveSet.location || !saveSet.active.length} onClick={() => openSaves(0)}><EyeIcon /></button>}</span>
              <button type="button" onClick={() => browse('location')}>{kind === 'add' ? 'Choose' : 'Change'}</button>
            </div>
            {shown('location') ? <p className="dialog-problem" role="alert">{shown('location')} Ask <SearchLinks game={searchName} bridge={bridge} onError={setError} />.</p>
            : <p className="dialog-below">
              {focused === 'location' ? <span className="dialog-hint">A folder, a file, or a pattern such as {patternExamples[platformName]}</span>
                : kind === 'configure' && game?.kind !== 'custom' && location.trim() !== ''
                  ? <button type="button" className="dialog-reset" onClick={() => setLocation('')}>Reset</button>
                  : <span className="dialog-hint">When not sure, ask <SearchLinks game={searchName} bridge={bridge} onError={setError} />{' '}
                    “What is the save game location of {searchName} on {platformName}”
                    <span className="dialog-warning"><strong>Warning:</strong> Online answers can be wrong, hallucinated, or contain someone else's username or Steam ID.
                    Don't use them as is: find that folder on your computer first, then use its real path.</span></span>}
            </p>}
          </>}
          {saveSet?.catalog_problem && <p className="dialog-hint">{saveSet.catalog_problem}</p>}
          {kind === 'configure' && game && <div className="dialog-field"><label htmlFor="checkpoints-store">Checkpoints store</label>
            <span className="dialog-input"><input id="checkpoints-store" value={checkpointsPath} disabled />
              <button type="button" className="dialog-icon-button" aria-label="Open checkpoints store" title="Show in folder"
                onClick={() => bridge.request({ type: 'open_checkpoints', game: game.id }).catch((failure) => setError(message(failure)))}><EyeIcon /></button></span>
            {onFlush && <button type="button" className="flush-button"
              disabled={busy || !game.flush.available || (!game.has_history && !game.checkpoints_size)} onClick={(event) => onFlush(event.currentTarget)}>
              <span className="dialog-button-icon trash" />Flush{game.checkpoints_size ? ` (${formatBytes(game.checkpoints_size)})` : ''}
            </button>}
          </div>}
          </div>
          {kind === 'configure' && game && <div className="dialog-group dialog-expert-mode">
            <label className="dialog-check"><input type="checkbox" checked={expertMode} onChange={(event) => setExpertMode(event.target.checked)} />
              Expert mode</label>
            <ul className="dialog-expert-mode-note">
              <li><strong>Allow saving and loading while the game is running.</strong> Beware: this won’t work as expected for LOTS of games that keep progress in memory. For those games, we can only save or load progress while it’s on disk and the game is stopped. But some games can be fooled into saving progress mid-game. This depends on the game and takes expert save-scumming skills to figure out.</li>
              <li><strong>Allow terminating a running game from its game card.</strong> This is much faster than quitting through the game’s menus, so it’s super efficient for save scumming. But it may also prevent some games from saving properly and cause problems. Knowing which games are safe to terminate takes expert save-scumming skills.</li>
            </ul>
          </div>}
        </>}
        {kind === 'flush' && <>
          <p>Permanently delete all SaveScummer checkpoints and history for this game?<br />The game’s own saves stay untouched.</p>
          {preview ? <><dl className="flush-counts">
            {preview.saved > 0 && <><dt>Checkpoints</dt><dd>{preview.saved}</dd></>}
            {preview.recovery > 0 && <><dt>Recovery points</dt><dd>{preview.recovery}</dd></>}
            {preview.temporary > 0 && <><dt>Incomplete checkpoints</dt><dd>{preview.temporary}</dd></>}
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
        disabled={!!game && ((kind === 'flush' && !game.flush.available) || (kind === 'configure' && !game.configure.available))} />
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
