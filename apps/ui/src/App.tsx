import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Markdown from 'react-markdown';
import type { Bridge } from './bridge';
import type { Game, HistoryEntry, HistoryPage, HostState, Operation, UiRequest } from './types';
import { AppDialog, formatBytes, type DialogKind } from './Dialogs';
import { displayShortcut } from './shortcuts';

type Action = 'save' | 'load' | 'revert' | 'delete' | 'flush';
type Feedback = { game: string; action: Action; target?: string; phase: 'busy' | 'success' | 'error'; message?: string };

const icons: Record<string, string> = {
  save: 'flag', load: 'rotate-left', saved: 'flag', loaded: 'rotate-left', reverted: 'rotate-left',
  game_started: 'circle', game_closed: 'circle', tag: 'tag', more: 'ellipsis', scan: 'arrows-rotate',
  add: 'plus', settings: 'gear', success: 'check', busy: 'arrows-rotate', clock: 'clock',
};

function Icon({ name, className = '' }: { name: string; className?: string }) {
  return <img className={`icon ${className}`} src={`/icons/${icons[name] || name}.svg`} alt="" aria-hidden="true" />;
}

function useArtwork(bridge: Bridge, game: Game, kind: 'hero' | 'logo' | 'header') {
  const [url, setUrl] = useState<string>();
  const file = game.artwork?.[kind];
  useEffect(() => {
    let active = true;
    let objectUrl: string | undefined;
    setUrl(undefined);
    if (file) {
      bridge.artwork(game.id, kind).then((value) => {
        objectUrl = value;
        if (active) setUrl(value);
        else URL.revokeObjectURL(value);
      }).catch(() => undefined);
    }
    return () => {
      active = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [bridge, game.id, kind, file]);
  return url;
}

function GameCard({ bridge, game, selected, onSelect }: {
  bridge: Bridge; game: Game; selected: boolean; onSelect: () => void;
}) {
  const hero = useArtwork(bridge, game, game.artwork?.hero ? 'hero' : 'header');
  const logo = useArtwork(bridge, game, 'logo');
  return (
    <button className={`game-card ${selected ? 'selected' : ''} ${game.running ? 'running' : 'installed'}`}
      onClick={onSelect} aria-current={selected ? 'true' : undefined}
      aria-label={`${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}${game.running ? '' : ', Not running'}`}
      title={`${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}${game.running ? '' : ' — Not running'}`}>
      {hero && <img className="game-art" src={hero} alt="" />}
      <span className="game-shade" />
      {logo ? <img className="game-logo" src={logo} alt="" /> : <span className="game-fallback">{game.name}</span>}
      {game.install_tag && <span className="install-tag">{game.install_tag}</span>}
    </button>
  );
}

export function App({ bridge }: { bridge: Bridge }) {
  const [state, setState] = useState<HostState>();
  const [status, setStatus] = useState('connecting');
  const [selected, setSelected] = useState<string>();
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [next, setNext] = useState<string>();
  const [historyError, setHistoryError] = useState<string>();
  const [feedback, setFeedback] = useState<Feedback>();
  const [scanFeedback, setScanFeedback] = useState<string>();
  const [labelsRevision, setLabelsRevision] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const scrollRef = useRef<HTMLDivElement>(null);
  const feedbackTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const historyGame = useRef<string | undefined>(undefined);
  const previousActiveStack = useRef<string[]>([]);
  const deleteDeadlines = useRef(new Map<string, number>());
  const [flash, setFlash] = useState<string>();
  const [dialog, setDialog] = useState<DialogKind>();
  const [moreOpen, setMoreOpen] = useState(false);
  const moreRef = useRef<HTMLDivElement>(null);
  const [expandedInfo, setExpandedInfo] = useState<Record<string, boolean>>({});
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(600);

  useEffect(() => { setMoreOpen(false); }, [selected]);
  useEffect(() => {
    if (!moreOpen) return;
    const outside = (event: PointerEvent) => {
      if (!moreRef.current?.contains(event.target as Node)) setMoreOpen(false);
    };
    document.addEventListener('pointerdown', outside);
    return () => document.removeEventListener('pointerdown', outside);
  }, [moreOpen]);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    const deadlines = new Map<string, number>();
    for (const operation of state?.deletes || []) {
      if (operation.remaining_ms !== undefined) deadlines.set(operation.id, Date.now() + operation.remaining_ms);
    }
    deleteDeadlines.current = deadlines;
  }, [state?.deletes]);

  useEffect(() => {
    let live = true;
    const unlisteners: Array<() => void> = [];
    Promise.all([
      bridge.onState((value) => { if (live) { setState(value); setStatus('connected'); } }),
      bridge.onStatus((value) => { if (live) { setStatus(value); if (value !== 'connected') setState(undefined); } }),
      bridge.onLabels(() => { if (live) setLabelsRevision((n) => n + 1); }),
    ]).then((items) => {
      if (live) unlisteners.push(...items);
      else items.forEach((unlisten) => unlisten());
      return bridge.request<HostState>({ type: 'state' });
    }).then((value) => { if (live) { setState(value); setStatus('connected'); } }).catch(() => undefined);
    return () => { live = false; unlisteners.forEach((unlisten) => unlisten()); clearTimeout(feedbackTimer.current); };
  }, [bridge]);

  const visibleGames = useMemo(() => state?.games.filter((game) => game.installed) || [], [state]);
  const selectedGame = visibleGames.find((game) => game.id === selected);
  const virtualItems = useMemo(() => groupHistory(history).flatMap(({ day, rows }) => [
    { key: `day-${day}`, day, row: undefined as HistoryEntry | undefined },
    ...rows.map((row) => ({ key: row.id, day, row })),
  ]), [history]);
  const firstVisible = Math.max(0, Math.floor(scrollTop / 64) - 5);
  const lastVisible = Math.min(virtualItems.length, Math.ceil((scrollTop + viewportHeight) / 64) + 5);

  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const measure = () => setViewportHeight(element.clientHeight || 600);
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [selectedGame?.id]);
  useEffect(() => {
    if (!state) return;
    const active = state.active_stack[0];
    const formerTop = previousActiveStack.current[0];
    const known = (id?: string) => visibleGames.some((game) => game.id === id);
    const focusedAnotherRunningGame = formerTop && active !== formerTop
      && previousActiveStack.current.includes(active)
      && visibleGames.some((game) => game.id === formerTop && game.running);
    if (selected && !known(selected)) setSelected(undefined);
    else if (!selected && known(active)) setSelected(active);
    else if (focusedAnotherRunningGame && known(active)) setSelected(active);
    previousActiveStack.current = state.active_stack;
  }, [state, selected, visibleGames]);

  useEffect(() => {
    if (!state) return;
    const report = () => bridge.report(selected || null, document.hasFocus()).catch(() => undefined);
    report();
    window.addEventListener('focus', report);
    window.addEventListener('blur', report);
    return () => { window.removeEventListener('focus', report); window.removeEventListener('blur', report); };
  }, [bridge, selected, state?.instance]);

  useEffect(() => {
    let live = true;
    if (historyGame.current !== selectedGame?.id) {
      historyGame.current = selectedGame?.id;
      setHistory([]);
      setNext(undefined);
      if (scrollRef.current) scrollRef.current.scrollTop = 0;
      setScrollTop(0);
    }
    setHistoryError(undefined);
    if (selectedGame) {
      bridge.request<HistoryPage>({ type: 'history', game: selectedGame.id, limit: 100 })
        .then((page) => { if (live) { setHistory(page.rows); setNext(page.next); } })
        .catch((error) => { if (live) setHistoryError(String(error)); });
    }
    return () => { live = false; };
  }, [bridge, selectedGame?.id, selectedGame?.history_version, selectedGame?.labels_version, labelsRevision, state?.instance]);

  const loadMore = useCallback(() => {
    if (!selected || !next) return;
    bridge.request<HistoryPage>({ type: 'history', game: selected, cursor: next, limit: 100 })
      .then((page) => { setHistory((rows) => [...rows, ...page.rows]); setNext(page.next); })
      .catch((error) => setHistoryError(String(error)));
  }, [bridge, selected, next]);

  const selectGame = useCallback((id: string) => {
    setSelected(id);
    setFeedback(undefined);
  }, []);

  const jumpToCheckpoint = useCallback(async (checkpoint: string) => {
    if (!selectedGame) return;
    let rows = history;
    let cursor = next;
    while (!rows.some((row) => row.checkpoint === checkpoint) && cursor) {
      const page = await bridge.request<HistoryPage>({ type: 'history', game: selectedGame.id, cursor, limit: 100 });
      rows = [...rows, ...page.rows];
      cursor = page.next;
      setHistory(rows);
      setNext(cursor);
    }
    const target = rows.find((row) => row.checkpoint === checkpoint);
    if (!target) return;
    setFlash(target.id);
    const items = groupHistory(rows).flatMap(({ day, rows: entries }) => [`day-${day}`, ...entries.map((entry) => entry.id)]);
    const top = items.indexOf(target.id) * 64;
    const viewport = scrollRef.current;
    if (viewport && (top < viewport.scrollTop || top + 64 > viewport.scrollTop + viewport.clientHeight)) {
      viewport.scrollTo({ top: Math.max(0, top - viewport.clientHeight / 2), behavior: 'smooth' });
    }
    setTimeout(() => setFlash(undefined), 1800);
  }, [bridge, history, next, selectedGame]);

  const runAction = useCallback(async (action: 'save' | 'load' | 'revert', checkpoint?: string) => {
    if (!selectedGame) return;
    clearTimeout(feedbackTimer.current);
    const game = selectedGame.id;
    setFeedback({ game, action, target: checkpoint, phase: 'busy' });
    try {
      const request: UiRequest = action === 'save'
        ? { type: 'save', game }
        : action === 'load' ? { type: 'load', game, checkpoint }
          : { type: 'revert', game, checkpoint: checkpoint! };
      const accepted = await bridge.request<Operation>(request);
      const outcome = await bridge.request<Operation>({ type: 'outcome', operation: accepted.id });
      if (outcome.status !== 'succeeded') throw new Error(outcome.error?.detail || outcome.error?.kind || `${action} failed`);
      setFeedback({ game, action, target: checkpoint, phase: 'success' });
      scrollRef.current?.scrollTo?.({ top: 0, behavior: 'smooth' });
      feedbackTimer.current = setTimeout(() => setFeedback(undefined), 1600);
    } catch (error) {
      setFeedback({ game, action, target: checkpoint, phase: 'error', message: String(error instanceof Error ? error.message : error) });
    }
  }, [bridge, selectedGame]);

  const scan = useCallback(async () => {
    setScanFeedback('Scanning…');
    try {
      const result = await bridge.request<{ new_games: number }>({ type: 'scan' });
      const count = result.new_games;
      setScanFeedback(count ? `${count} game${count === 1 ? '' : 's'} found` : 'No new games');
    } catch (error) {
      setScanFeedback(String(error instanceof Error ? error.message : error));
    }
    setTimeout(() => setScanFeedback(undefined), 2400);
  }, [bridge]);

  const deleteCheckpoint = useCallback(async (checkpoint: string) => {
    if (!selectedGame) return;
    const game = selectedGame.id;
    setFeedback(undefined);
    try {
      const accepted = await bridge.request<Operation>({ type: 'delete', game, checkpoint });
      const outcome = await bridge.request<Operation>({ type: 'outcome', operation: accepted.id });
      if (outcome.status === 'failed') {
        setFeedback({ game, action: 'delete', target: checkpoint, phase: 'error',
          message: outcome.error?.detail || outcome.error?.kind || 'Delete failed' });
      }
    } catch (error) {
      setFeedback({ game, action: 'delete', target: checkpoint, phase: 'error',
        message: String(error instanceof Error ? error.message : error) });
    }
  }, [bridge, selectedGame]);

  const cancelDelete = useCallback(async (operation: string) => {
    try {
      await bridge.request<Operation>({ type: 'cancel_delete', operation });
    } catch (error) {
      if (selectedGame) setFeedback({ game: selectedGame.id, action: 'delete', phase: 'error',
        message: String(error instanceof Error ? error.message : error) });
    }
  }, [bridge, selectedGame]);

  const finishFlush = useCallback(async (operation: string) => {
    if (!selectedGame) return;
    const game = selectedGame.id;
    setFeedback({ game, action: 'flush', phase: 'busy' });
    try {
      const outcome = await bridge.request<Operation>({ type: 'outcome', operation });
      if (outcome.status !== 'succeeded') throw new Error(outcome.error?.detail || outcome.error?.kind || 'Flush failed');
      setFeedback({ game, action: 'flush', phase: 'success' });
      feedbackTimer.current = setTimeout(() => setFeedback(undefined), 1600);
    } catch (error) {
      setFeedback({ game, action: 'flush', phase: 'error', message: String(error instanceof Error ? error.message : error) });
    }
  }, [bridge, selectedGame]);

  const running = visibleGames.filter((game) => game.running);
  const installed = visibleGames.filter((game) => !game.running);
  const busy = Boolean(selectedGame?.busy) || (feedback?.game === selected && feedback?.phase === 'busy');
  const hostError = selectedGame?.blocked || selectedGame?.config_error ||
    (selectedGame?.last_result?.status === 'failed' ? selectedGame.last_result.error : undefined);
  const addControl = <button key="add" onClick={() => setDialog('add')}><Icon name="add" />Add custom game</button>;
  const scanControl = <button key="scan" onClick={scan} disabled={scanFeedback === 'Scanning…'}>
    <Icon name={scanFeedback === 'Scanning…' ? 'busy' : 'scan'} />{scanFeedback || 'Scan for games'}
  </button>;

  return (
    <div className={`app ${busy ? 'is-busy' : ''} ${state && visibleGames.length === 0 ? 'no-games' : ''}`}>
      <aside className="sidebar">
        <div className="titlebar" data-tauri-drag-region>
          <img className="app-icon" src="/app-icon.svg" alt="" />
          <span className="wordmark"><span>Save</span><strong>Scummer</strong></span>
        </div>
        {running.length > 0 && <div className="running-panel">
          <h2><span className="running-dot" />RUNNING</h2>
          {running.map((game) => <GameCard key={game.id} bridge={bridge} game={game} selected={game.id === selected} onSelect={() => selectGame(game.id)} />)}
        </div>}
        <div className="library-panel">
          {installed.length > 0 && <>
            <h2>INSTALLED</h2>
            {installed.map((game) => <GameCard key={game.id} bridge={bridge} game={game} selected={game.id === selected} onSelect={() => selectGame(game.id)} />)}
          </>}
          <div className="library-controls">
            {visibleGames.length ? [addControl, scanControl] : [scanControl, addControl]}
            <button onClick={() => setDialog('settings')}><Icon name="settings" />Settings</button>
          </div>
        </div>
      </aside>
      <main className="main">
        <div className="main-title titlebar" data-tauri-drag-region />
        {status !== 'connected' && <p className="connection" role="status">{status.startsWith('reconnecting') ? 'Reconnecting to host…' : 'Connecting to host…'}</p>}
        {selectedGame ? <>
          <div className="action-band" aria-label="Checkpoint actions">
            <ActionButton action="save" game={selectedGame} feedback={feedback} busy={busy} now={now}
              shortcut={state?.settings?.save_shortcut} onClick={() => runAction('save')} />
            <ActionButton action="load" game={selectedGame} feedback={feedback} busy={busy} now={now}
              shortcut={state?.settings?.load_shortcut} onClick={() => runAction('load')}
              onJump={() => selectedGame.latest && jumpToCheckpoint(selectedGame.latest.id)} />
            <div className="more-wrap" ref={moreRef} onKeyDown={(event) => { if (event.key === 'Escape') { setMoreOpen(false); event.stopPropagation(); } }}>
              <button className="more-button" onClick={() => setMoreOpen(!moreOpen)} aria-label="More game actions" aria-expanded={moreOpen} title="More game actions"><Icon name="more" /></button>
              {moreOpen && <div className="more-menu" role="menu">
                <button role="menuitem" disabled={busy} onClick={() => {
                  setMoreOpen(false);
                  bridge.request({ type: 'open_checkpoints', game: selectedGame.id }).catch((error) =>
                    setFeedback({ game: selectedGame.id, action: 'flush', phase: 'error', message: String(error) }));
                }}><Icon name="folder" />Open checkpoints folder</button>
                <button role="menuitem" disabled={busy} onClick={() => { setMoreOpen(false); setDialog('configure'); }}><Icon name="sliders" />Configure paths…</button>
                <hr />
                <button role="menuitem" disabled={busy || (!selectedGame.has_history && !selectedGame.checkpoints_size)} onClick={() => { setMoreOpen(false); setDialog('flush'); }}>
                  <Icon name="trash-can" />Flush checkpoints{selectedGame.checkpoints_size ? ` (${formatBytes(selectedGame.checkpoints_size)})` : ''}…
                </button>
              </div>}
            </div>
          </div>
          {feedback?.game === selected && feedback?.phase === 'error' && <p className="action-error-block" role="alert">{feedback.message}</p>}
          {!(feedback?.game === selected && feedback?.phase === 'error') && hostError &&
            <p className="action-error-block" role="alert">{hostError.detail || hostError.kind}</p>}
          {selectedGame.info && <div className={`instructions ${expandedInfo[selectedGame.id] ? 'expanded' : ''}`}>
            <div className="instructions-text"><Markdown>{selectedGame.info}</Markdown></div>
            <button onClick={() => setExpandedInfo((value) => ({ ...value, [selectedGame.id]: !value[selectedGame.id] }))}
              aria-label={`${expandedInfo[selectedGame.id] ? 'Collapse' : 'Expand'} game instructions`}
              aria-expanded={Boolean(expandedInfo[selectedGame.id])}><Icon name={expandedInfo[selectedGame.id] ? 'angle-up' : 'angle-down'} /></button>
          </div>}
          <div className="history" ref={scrollRef} aria-label={`${selectedGame.name} history`} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
            {historyError && <p className="history-error" role="alert">{historyError}</p>}
            {virtualItems.length === 0 && !historyError && <p className="empty-history">Saves will appear here.</p>}
            <div className="history-virtual" style={{ height: virtualItems.length * 64 + (next ? 52 : 0) }}>
              {virtualItems.slice(firstVisible, lastVisible).map(({ key, day, row }, offset) => {
                const top = (firstVisible + offset) * 64;
                if (!row) return <div className="day virtual-item" style={{ top }} key={key}><h2>{dayHeading(day, now)}</h2></div>;
                const pending = state?.deletes.find((item) => item.checkpoint === row.checkpoint && item.game === selected);
                return <div className="virtual-item" style={{ top }} key={key}><HistoryRow row={row} busy={busy} now={now} bridge={bridge}
                  flash={flash === row.id} deleteOperation={pending} deadline={pending && deleteDeadlines.current.get(pending.id)}
                  feedback={feedback?.game === selected ? feedback : undefined}
                  onLoad={() => row.checkpoint && runAction('load', row.checkpoint)}
                  onRevert={() => row.checkpoint && runAction('revert', row.checkpoint)}
                  onJump={() => row.restored && jumpToCheckpoint(row.restored)}
                  onDelete={() => row.checkpoint && deleteCheckpoint(row.checkpoint)}
                  onCancelDelete={() => pending && cancelDelete(pending.id)} /></div>;
              })}
              {next && <button className="load-more" style={{ top: virtualItems.length * 64 }} onClick={loadMore}>Show older history</button>}
            </div>
          </div>
        </> : <div className="empty-library">{!state ? 'Waiting for the host…' : visibleGames.length ? 'No known games are running.' : 'No games found.'}</div>}
      </main>
      {dialog && <AppDialog key={`${dialog}-${selectedGame?.id || ''}`} kind={dialog} game={selectedGame} state={state} bridge={bridge}
        close={() => setDialog(undefined)} onAdded={(id) => { setDialog(undefined); setSelected(id); }} onFlushed={finishFlush} />}
    </div>
  );
}

function ActionButton({ action, game, feedback, busy, now, shortcut, onClick, onJump }: {
  action: 'save' | 'load'; game: Game; feedback?: Feedback; busy: boolean; now: number; shortcut?: string;
  onClick: () => void; onJump?: () => void;
}) {
  const available = game[action].available;
  const current: Feedback | undefined = feedback?.game === game.id && feedback.action === action && !feedback.target ? feedback
    : game.busy?.kind === action ? { game: game.id, action, phase: 'busy' } : undefined;
  const label = current?.phase === 'busy' ? `${action === 'save' ? 'SAVING' : 'LOADING'}…`
    : current?.phase === 'success' ? 'DONE'
    : current?.phase === 'error' ? 'FAILED'
    : action.toUpperCase();
  return <div className="action-slot">
    {(action !== 'load' || available) && <span className="shortcut-tab">
      {displayShortcut(shortcut || `${navigator.platform.includes('Mac') ? 'Alt' : 'Ctrl'}+${action === 'save' ? 'F5' : 'F9'}`)}
    </span>}
    <button className={`main-button ${action} ${current?.phase || ''}`} disabled={!available || busy}
      onClick={onClick} title={current?.message || game[action].reason || undefined} aria-label={`${action} ${game.name}`}>
      <Icon name={current?.phase === 'busy' ? 'busy' : current?.phase === 'success' ? 'success' : action} />
      <span>{label}</span>
    </button>
    {action === 'load' && <button className={`load-badge ${!available ? 'unavailable' : ''}`}
      onClick={onJump} disabled={!game.latest} aria-label="Jump to latest checkpoint">
      {game.latest ? <><span className="badge-time"><Icon name="clock" />{formatBadgeTime(game.latest.created_at, now)}<span className="badge-age">{relativeAge(new Date(game.latest.created_at), now)}</span></span>
        {game.latest.label && <span className="badge-label"><Icon name="tag" />{game.latest.label}</span>}</> : 'No saves yet'}
    </button>}
  </div>;
}

function HistoryRow({ row, busy, now, bridge, feedback, flash, deleteOperation, deadline,
  onLoad, onRevert, onJump, onDelete, onCancelDelete }: {
  row: HistoryEntry; busy: boolean; now: number; bridge: Bridge; feedback?: Feedback; flash: boolean;
  deleteOperation?: Operation; deadline?: number;
  onLoad: () => void; onRevert: () => void; onJump: () => void; onDelete: () => void; onCancelDelete: () => void;
}) {
  const name = row.kind.replace('_', ' ');
  const date = new Date(row.at);
  const active = feedback?.target === row.checkpoint && (feedback?.action === 'load' || feedback?.action === 'revert') ? feedback : undefined;
  const operation = row.actions.load ? 'load' : 'revert';
  const rowLabel = active?.phase === 'busy' ? `${operation === 'load' ? 'LOADING' : 'REVERTING'}…`
    : active?.phase === 'success' ? 'DONE' : active?.phase === 'error' ? 'FAILED' : operation.toUpperCase();
  const chip = row.label || (row.kind === 'loaded' && row.saved_at ? new Date(row.saved_at).toLocaleTimeString(undefined, { hour12: false }) : undefined)
    || (row.kind === 'reverted' && row.reverted_at ? new Date(row.reverted_at).toLocaleTimeString(undefined, { hour12: false }) : undefined);
  const count = Math.max(1, Math.ceil(((deadline || Date.now() + (deleteOperation?.remaining_ms || 0)) - now) / 1000));
  return <div id={`entry-${row.id}`} className={`history-row ${flash ? 'flash' : ''}`}>
    <div className="history-time"><time dateTime={row.at}>{date.toLocaleTimeString(undefined, { hour12: false })}</time><small>{relativeAge(date, now)}</small></div>
    <div className="history-description">
      <div className="history-main"><Icon name={row.kind} /><span>{name}</span>
        {row.kind === 'saved' && row.checkpoint && <LabelChip bridge={bridge} checkpoint={row.checkpoint} label={row.label} />}
        {row.kind !== 'saved' && chip && (row.restored
          ? <button className="label-chip" onClick={onJump} title={chip} aria-label={`Jump to ${chip}`}><Icon name="tag" />{chip}</button>
          : <span className="label-chip" title={chip}><Icon name="tag" />{chip}</span>)}
      </div>
      {row.kind === 'loaded' && <div className="history-note">
        {row.removed_files ? `Removed ${row.removed_files} newer save${row.removed_files === 1 ? '' : 's'}, kept in the recovery point` : ''}
        {row.removed_files && row.cloud_replaced ? '; ' : ''}
        {row.cloud_replaced ? 'Steam Cloud replaced the restored save' : ''}
      </div>}
    </div>
    <div className={`history-actions ${deleteOperation ? 'deleting' : ''}`}>
      {deleteOperation?.status === 'counting_down' && <><span>Deleting in {count}</span><button className="cancel-delete" onClick={onCancelDelete}>CANCEL</button></>}
      {deleteOperation?.status === 'waiting' && <><span>Waiting to delete…</span><button className="cancel-delete" onClick={onCancelDelete}>CANCEL</button></>}
      {deleteOperation?.status === 'running' && <span><Icon name="busy" />Deleting…</span>}
      {!deleteOperation && (row.actions.load || row.actions.revert) && <button disabled={busy} className="row-button"
        onClick={row.actions.load ? onLoad : onRevert}
        aria-label={`${row.actions.load ? 'Load save' : 'Revert restore'} from ${date.toLocaleString()}`}>
        <Icon name={active?.phase === 'busy' ? 'busy' : active?.phase === 'success' ? 'success' : 'load'} />{rowLabel}
      </button>}
      {!deleteOperation && row.actions.delete && <button disabled={busy} className="delete-button" onClick={onDelete}
        aria-label={`Delete checkpoint from ${date.toLocaleString()}`} title="Delete checkpoint"><Icon name="trash-can" /></button>}
    </div>
  </div>;
}

function LabelChip({ bridge, checkpoint, label }: { bridge: Bridge; checkpoint: string; label?: string }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(label || '');
  const [error, setError] = useState<string>();
  const original = useRef(label || '');
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const input = useRef<HTMLInputElement>(null);

  const persist = useCallback((value: string) => {
    const normalized = value.replace(/[\r\n]+/g, ' ').trim().slice(0, 100);
    queue.current = queue.current.then(async () => {
      await bridge.request({ type: 'set_label', checkpoint, label: normalized || undefined });
      setError(undefined);
    }).catch((failure) => setError(String(failure instanceof Error ? failure.message : failure)));
  }, [bridge, checkpoint]);

  useEffect(() => {
    if (!editing) return;
    input.current?.focus();
    timer.current = setTimeout(() => persist(draft), 500);
    return () => clearTimeout(timer.current);
  }, [draft, editing, persist]);

  const finish = () => {
    clearTimeout(timer.current);
    persist(draft);
    setEditing(false);
  };
  const cancel = () => {
    clearTimeout(timer.current);
    persist(original.current);
    setDraft(original.current);
    setEditing(false);
  };
  if (editing) return <span className="label-editor">
    <input ref={input} value={draft} maxLength={100} aria-label="Checkpoint label"
      onChange={(event) => setDraft(event.target.value.replace(/[\r\n]+/g, ' '))}
      onBlur={finish} onKeyDown={(event) => { if (event.key === 'Enter') finish(); if (event.key === 'Escape') cancel(); }} />
    <button aria-label="Confirm checkpoint label" onMouseDown={(event) => event.preventDefault()} onClick={finish}><Icon name="check" /></button>
  </span>;
  return <><button className="label-chip editable" aria-label="Edit checkpoint label" title={error || label || 'Add label'}
    onClick={() => { original.current = label || ''; setDraft(label || ''); setEditing(true); }}>
    <Icon name="tag" />{label || 'Add label…'}
  </button>{error && <span className="label-error" role="alert">{error}</span>}</>;
}

export function groupHistory(rows: HistoryEntry[]) {
  const groups: Array<{ day: string; rows: HistoryEntry[] }> = [];
  for (const row of rows) {
    const date = new Date(row.at);
    const day = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
    if (groups.at(-1)?.day !== day) groups.push({ day, rows: [] });
    groups.at(-1)!.rows.push(row);
  }
  return groups;
}

function dayHeading(day: string, now: number) {
  const today = new Date(now);
  const todayKey = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
  if (day === todayKey) return `Today ${day}`;
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  const yesterdayKey = `${yesterday.getFullYear()}-${String(yesterday.getMonth() + 1).padStart(2, '0')}-${String(yesterday.getDate()).padStart(2, '0')}`;
  if (day === yesterdayKey) return `Yesterday ${day}`;
  const date = new Date(`${day}T12:00:00`);
  if (now - date.getTime() < 7 * 24 * 3600 * 1000 && now >= date.getTime()) {
    return `${date.toLocaleDateString(undefined, { weekday: 'long' })} ${day}`;
  }
  return day;
}

export function relativeAge(date: Date, now: number): string {
  const seconds = Math.floor((now - date.getTime()) / 1000);
  if (seconds < 0 || seconds >= 24 * 3600) return '';
  const today = new Date(now);
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  const start = yesterday.getTime();
  if (date.getTime() < start) return '';
  if (seconds < 60) return `${Math.floor(seconds / 5) * 5}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}

function formatBadgeTime(value: string, now: number) {
  const date = new Date(value);
  const time = date.toLocaleTimeString(undefined, { hour12: false });
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) return time;
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) return `Yesterday ${time}`;
  const day = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
  return `${day} ${time}`;
}
