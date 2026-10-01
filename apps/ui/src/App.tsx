import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import Markdown from 'react-markdown';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Bridge } from './bridge';
import type { Game, HistoryEntry, HistoryPage, HostState, Operation, UiRequest } from './types';
import { AboutDialog, AppDialog, formatBytes, type DialogKind } from './Dialogs';
import { displayShortcut } from './shortcuts/shortcuts';
import { failureMessage } from './messages';
import { GuidancePanel } from './GuidancePanel';
import { SleepySkeleton } from './SleepySkeleton';
import { PointingSkeleton } from './PointingSkeleton';
import { CrouchingSkeleton } from './CrouchingSkeleton';

type Action = 'save' | 'load' | 'revert' | 'delete' | 'flush' | 'retry';
type Feedback = { game: string; action: Action; target?: string; phase: 'busy' | 'success' | 'error'; message?: string };
type PendingUndo = { game: string; checkpoint: string; row: HistoryEntry; index: number; timer: ReturnType<typeof setTimeout> };
type UndoNotice = { game: string; checkpoint: string; exit?: 'fade' | 'right' };
const deleteKey = (game: string, checkpoint: string) => `${game}:${checkpoint}`;

const icons: Record<string, string> = {
  save: 'flag', load: 'rotate-left', saved: 'flag', loaded: 'rotate-left', reverted: 'rotate-left',
  game_started: 'circle-play', game_closed: 'circle-stop', tag: 'tag', scan: 'arrows-rotate',
  add: 'plus', settings: 'gear', success: 'check', busy: 'arrows-rotate', clock: 'clock',
  info: 'circle-info',
};

function Icon({ name, className = '' }: { name: string; className?: string }) {
  return <img className={`icon ${className}`} src={`/icons/${icons[name] || name}.svg`} alt="" aria-hidden="true" />;
}

// Scroll shadows fade in over the first 20px of scroll distance (0.1 at 1px, full at 20px).
const mac = navigator.platform.includes('Mac');

/** The app logo, muted like the idle game cards until hovered; opens the About dialog. */
function BrandButton({ onClick }: { onClick: (opener: HTMLButtonElement) => void }) {
  return <button className="brand" aria-label="About SaveScummer" onClick={(event) => onClick(event.currentTarget)}>
    <img className="app-icon" src="/app-icon.svg" alt="" aria-hidden="true" />
    <span className="wordmark" aria-hidden="true"><span>Save</span><strong>Scummer</strong></span>
  </button>;
}

// Empty-state skeleton leaning out of an arched window; every character SVG shares the same canvas and pose height.
function Character({ name, sound = true }: { name: string; sound?: boolean }) {
  return <div className="empty-character" aria-hidden="true">
    {name === 'no-games-found' ? <SleepySkeleton className="character-art pokeable" sound={sound} />
      : name === 'no-game-selected' ? <PointingSkeleton className="character-art" />
      : name === 'no-checkpoints' ? <CrouchingSkeleton className="character-art pokeable" sound={sound} />
      : <img className="character-art" src={`/character/${name}.svg`} alt="" />}
  </div>;
}

// Windows and Linux run undecorated (tauri.windows.conf.json, tauri.linux.conf.json), so the window bar draws its own caption buttons. There is no
// Maximize: the window's width is fixed by matching minWidth and maxWidth, so it can't fill the screen.
function WindowControls() {
  const run = (action: 'minimize' | 'close') => () => {
    if ('__TAURI_INTERNALS__' in window) getCurrentWindow()[action]().catch(() => undefined);
  };
  return <div className="window-controls">
    <button aria-label="Minimize window" onClick={run('minimize')}>
      <svg viewBox="0 0 10 10"><path d="M0 5.5h10" /></svg>
    </button>
    <button className="close" aria-label="Close window" onClick={run('close')}>
      <svg viewBox="0 0 10 10"><path d="M.5.5l9 9M9.5.5l-9 9" /></svg>
    </button>
  </div>;
}

function fadeStyle(above: number, below: number) {
  const opacity = (distance: number) => distance < 1 ? 0 : Math.min(1, 0.1 + 0.9 * (distance - 1) / 19);
  return { '--fade-top': opacity(above), '--fade-bottom': opacity(below) } as React.CSSProperties;
}

// History items have fixed heights: day headers and game start/close dividers are short, everything else is a full row.
const isGameEvent = (row?: HistoryEntry) => row?.kind === 'game_started' || row?.kind === 'game_closed';
const itemHeight = (row?: HistoryEntry) => !row || isGameEvent(row) ? 32 : 64;
function itemTops(rows: (HistoryEntry | undefined)[]) {
  const tops: number[] = [];
  let total = 0;
  for (const row of rows) { tops.push(total); total += itemHeight(row); }
  return { tops, total };
}
/** Index of the item covering y (the last item starting at or above it). */
function itemAt(tops: number[], y: number) {
  let low = 0, high = tops.length - 1;
  while (low < high) { const mid = (low + high + 1) >> 1; if (tops[mid] <= y) low = mid; else high = mid - 1; }
  return low;
}

function scrollMotion(): ScrollBehavior {
  return typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth';
}

function matchesGameName(name: string, filter: string) {
  const query = filter.trim().toLocaleLowerCase();
  if (!query) return true;
  const word = /[\p{L}\p{N}]/u;
  const capital = /\p{Lu}/u;
  const lowerOrNumber = /[\p{Ll}\p{N}]/u;
  for (let index = 0; index < name.length; index++) {
    if (!word.test(name[index])) continue;
    const previous = name[index - 1];
    if (previous && word.test(previous) && !(lowerOrNumber.test(previous) && capital.test(name[index]))) continue;
    if (name.slice(index).toLocaleLowerCase().startsWith(query)) return true;
  }
  return false;
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

function GameCard({ bridge, game, selected, pending, error, onSelect, onConfigure, onLifecycle }: {
  bridge: Bridge; game: Game; selected: boolean; pending: boolean; error?: string;
  onSelect: () => void; onConfigure: (opener: HTMLButtonElement) => void; onLifecycle: (type: 'play' | 'close_game') => void;
}) {
  const hero = useArtwork(bridge, game, game.artwork?.hero ? 'hero' : 'header');
  const logo = useArtwork(bridge, game, 'logo');
  const [playAnimation, setPlayAnimation] = useState(false);
  useEffect(() => {
    if (!playAnimation) return;
    const timer = setTimeout(() => setPlayAnimation(false), 900);
    return () => clearTimeout(timer);
  }, [playAnimation]);
  const runningFace = <span className="running-badge-face"><span className="running-icon" />RUNNING</span>;
  return (
    <div className={`game-card-wrap ${selected ? 'selected' : ''} ${game.running ? 'running' : 'installed'}`} data-game={game.id}>
      <button className={`game-card ${selected ? 'selected' : ''} ${game.running ? 'running' : 'installed'}`}
        onClick={(event) => { if (event.detail < 2) onSelect(); }}
        onDoubleClick={() => {
          if (game.running) {
            // Undo the first click's selection toggle; a double-click on a running card is a lifecycle gesture.
            onSelect();
            if (!pending && game.expert_mode && game.can_close) onLifecycle('close_game');
          } else if (!pending && game.can_play !== false) {
            setPlayAnimation(true);
            onLifecycle('play');
          }
        }}
        aria-current={selected ? 'true' : undefined}
        aria-label={`${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}${game.running ? '' : ', Not running'}`}
        data-tooltip={`${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}${game.running ? '' : ' — Not running'}`}>
        {hero && <img className="game-art" src={hero} alt="" />}
        <span className="game-shade" />
        {logo ? <img className="game-logo" src={logo} alt="" /> : <span className="game-fallback">{game.name}</span>}
        {game.install_tag && <span className="install-tag">{game.install_tag}</span>}
        {playAnimation && <span className="game-play-animation" aria-hidden="true">
          <span className="game-play-animation-icon" />
        </span>}
      </button>
      <div className="card-lifecycle-actions">
        {/* When the host allows closing, the badge itself turns into the close button on hover and focus. */}
        {game.running && game.can_close ? <button className="running-badge" disabled={pending} onClick={() => onLifecycle('close_game')}
          aria-label={`Terminate ${game.name}`} title="Terminate game">
          {runningFace}<span className="running-badge-face close"><span className="card-action-icon stop" />CLOSE</span>
        </button>
        : game.running ? <span className="running-badge">{runningFace}</span>
        : <button className="card-action" disabled={pending || game.can_play === false} onClick={() => onLifecycle('play')}
          aria-label={`Play ${game.name}`} title={game.can_play === false ? 'No executable configured' : 'Play game'}>
          <span className="card-action-icon play" />
        </button>}
      </div>
      <div className="card-actions">
        <button className="card-action" onClick={(event) => onConfigure(event.currentTarget)}
          aria-label={`Configure ${game.name}`} title="Configure">
          <span className="card-action-icon gear" />
        </button>
      </div>
      {error && <p className="card-lifecycle-error" role="alert">{error}</p>}
    </div>
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
  const [cardPending, setCardPending] = useState<{ game: string; running: boolean; token: number }>();
  const [cardError, setCardError] = useState<{ game: string; message: string }>();
  const cardToken = useRef(0);
  const [scanFeedback, setScanFeedback] = useState<{ label: string; phase: 'scanning' | 'success' | 'error' }>();
  const [labelsRevision, setLabelsRevision] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const scrollRef = useRef<HTMLDivElement>(null);
  const feedbackTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const arrivalTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const cardTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const scanTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const historyGame = useRef<string | undefined>(undefined);
  // Game whose history finished loading at least once; only refreshes of it can reveal new rows.
  const loadedGame = useRef<string | undefined>(undefined);
  // Whether the selected game's first history page has arrived; the empty state waits for it to avoid flicker.
  const [historyLoaded, setHistoryLoaded] = useState(false);
  const historyRef = useRef(history);
  historyRef.current = history;
  const previousActiveStack = useRef<string[]>([]);
  const pendingUndo = useRef<PendingUndo | undefined>(undefined);
  const [undoNotice, setUndoNotice] = useState<UndoNotice>();
  const undoExitTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // A game Add found already in the library; told once, until another game is selected.
  const [alreadyAdded, setAlreadyAdded] = useState<string>();
  useEffect(() => { setAlreadyAdded((current) => current === selected ? current : undefined); }, [selected]);
  const [hiddenDeletes, setHiddenDeletes] = useState<ReadonlySet<string>>(new Set());
  const [flash, setFlash] = useState<string>();
  const [arrived, setArrived] = useState<ReadonlySet<string>>(new Set());
  // True briefly after a game's history first loads, so its visible rows cascade in.
  const [revealing, setRevealing] = useState(false);
  const revealTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [dialog, setDialog] = useState<DialogKind>();
  const [dialogGame, setDialogGame] = useState<string>();
  const dialogOpener = useRef<HTMLElement | null>(null);
  // Flush can be opened from Configure; it stacks on top and returns focus to its button there.
  const [flushOpener, setFlushOpener] = useState<HTMLElement>();
  const [historyDirection, setHistoryDirection] = useState<'up' | 'down'>('down');
  const [expandedInfo, setExpandedInfo] = useState<Record<string, boolean>>({});
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(600);
  const [filterOpen, setFilterOpen] = useState(false);
  const [gameFilter, setGameFilter] = useState('');
  const filterInputRef = useRef<HTMLInputElement>(null);
  const sidebarScrollRef = useRef<HTMLDivElement>(null);
  const sidebarTrackRef = useRef<HTMLDivElement>(null);
  const [sidebarScroll, setSidebarScroll] = useState({ top: 0, viewport: 0, content: 0, track: 0 });

  const runLifecycle = async (game: Game, type: 'play' | 'close_game') => {
    const token = ++cardToken.current;
    setCardError(undefined);
    setCardPending({ game: game.id, running: type === 'play', token });
    try {
      await bridge.request({ type, game: game.id });
      // The process monitor, rather than the request, confirms the new state.
      clearTimeout(cardTimer.current);
      cardTimer.current = setTimeout(() => setCardPending((current) => current?.token === token ? undefined : current), 5000);
    } catch (error) {
      setCardError({ game: game.id, message: error instanceof Error ? error.message : String(error) });
      setCardPending((current) => current?.token === token ? undefined : current);
    }
  };

  useEffect(() => {
    if (!cardPending) return;
    if (state?.games.find((game) => game.id === cardPending.game)?.running === cardPending.running) {
      setCardPending(undefined);
    }
  }, [cardPending, state]);

  const openDialog = (kind: DialogKind, opener: HTMLElement | null, game?: string) => {
    dialogOpener.current = opener;
    setDialogGame(game);
    setDialog(kind);
  };

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);

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
    return () => {
      live = false;
      unlisteners.forEach((unlisten) => unlisten());
      for (const timer of [feedbackTimer, arrivalTimer, flashTimer, cardTimer, scanTimer, revealTimer]) clearTimeout(timer.current);
    };
  }, [bridge]);

  const visibleGames = useMemo(() => state?.games.filter((game) => game.installed) || [], [state]);
  const selectedGame = visibleGames.find((game) => game.id === selected);
  const virtualItems = useMemo(() => groupHistory(history.filter((row) => !row.checkpoint || !hiddenDeletes.has(deleteKey(selected || '', row.checkpoint)))).flatMap(({ day, rows }) => [
    { key: `day-${day}`, day, row: undefined as HistoryEntry | undefined },
    ...rows.map((row) => ({ key: row.id, day, row })),
  ]), [history, hiddenDeletes, selected]);
  const { tops: itemTop, total: historyTotal } = useMemo(() => itemTops(virtualItems.map((item) => item.row)), [virtualItems]);
  const firstVisible = Math.max(0, itemAt(itemTop, scrollTop) - 5);
  const lastVisible = Math.min(virtualItems.length, itemAt(itemTop, scrollTop + viewportHeight) + 6);
  // Each day gets a lane spanning its rows, so its header can stick until the next day's lane pushes it out.
  const dayLanes = useMemo(() => {
    const lanes: Array<{ key: string; day: string; start: number; end: number }> = [];
    virtualItems.forEach((item, index) => {
      if (!item.row) lanes.push({ key: item.key, day: item.day, start: index, end: index + 1 });
      else lanes[lanes.length - 1].end = index + 1;
    });
    return lanes;
  }, [virtualItems]);

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
    // A newly active game gets selected, but a selection the person cleared stays cleared until the active game changes.
    if (selected && !known(selected)) setSelected(known(active) ? active : undefined);
    else if (!selected && known(active) && active !== formerTop) setSelected(active);
    else if (focusedAnotherRunningGame && known(active)) {
      setHistoryDirection(visibleGames.findIndex((game) => game.id === active) < visibleGames.findIndex((game) => game.id === selected) ? 'up' : 'down');
      setSelected(active);
    }
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

  const receiveHistory = useCallback((game: string, rows: HistoryEntry[]) => {
    if (loadedGame.current === game) {
      const known = new Set(historyRef.current.map((row) => row.id));
      const fresh = rows.filter((row) => !known.has(row.id));
      if (fresh.length) {
        clearTimeout(arrivalTimer.current);
        setArrived(new Set(fresh.map((row) => row.id)));
        arrivalTimer.current = setTimeout(() => setArrived(new Set()), 3000);
      }
    } else {
      clearTimeout(revealTimer.current);
      setRevealing(true);
      revealTimer.current = setTimeout(() => setRevealing(false), 1000);
    }
    loadedGame.current = game;
    setHistoryLoaded(true);
    setHistory(rows);
  }, []);

  useEffect(() => {
    let live = true;
    if (historyGame.current !== selectedGame?.id) {
      historyGame.current = selectedGame?.id;
      loadedGame.current = undefined;
      setHistoryLoaded(false);
      setArrived(new Set());
      setHistory([]);
      setNext(undefined);
      if (scrollRef.current) scrollRef.current.scrollTop = 0;
      setScrollTop(0);
    }
    setHistoryError(undefined);
    if (selectedGame) {
      bridge.request<HistoryPage>({ type: 'history', game: selectedGame.id, limit: 100 })
        .then((page) => { if (live) { receiveHistory(selectedGame.id, page.rows); setNext(page.next); } })
        .catch((error) => { if (live) setHistoryError(String(error)); });
    }
    return () => { live = false; };
  }, [bridge, receiveHistory, selectedGame?.id, selectedGame?.history_version, selectedGame?.labels_version, labelsRevision, state?.instance]);

  const loadMore = useCallback(() => {
    if (!selected || !next) return;
    bridge.request<HistoryPage>({ type: 'history', game: selected, cursor: next, limit: 100 })
      .then((page) => { setHistory((rows) => [...rows, ...page.rows]); setNext(page.next); })
      .catch((error) => setHistoryError(String(error)));
  }, [bridge, selected, next]);

  const selectGame = useCallback((id: string) => {
    setGameFilter('');
    setFilterOpen(false);
    filterInputRef.current?.blur();
    // Clicking the selected card again clears the selection.
    if (id === selected) {
      setSelected(undefined);
      setFeedback(undefined);
      return;
    }
    const oldPosition = visibleGames.findIndex((game) => game.id === selected);
    const newPosition = visibleGames.findIndex((game) => game.id === id);
    setHistoryDirection(newPosition < oldPosition ? 'up' : 'down');
    setSelected(id);
    setFeedback(undefined);
  }, [selected, visibleGames]);

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
    const items = groupHistory(rows).flatMap(({ rows: entries }) => [undefined, ...entries]);
    const top = itemTops(items).tops[items.indexOf(target)];
    const viewport = scrollRef.current;
    if (viewport && (top < viewport.scrollTop || top + itemHeight(target) > viewport.scrollTop + viewport.clientHeight)) {
      viewport.scrollTo({ top: Math.max(0, top - viewport.clientHeight / 2), behavior: scrollMotion() });
    }
    clearTimeout(flashTimer.current);
    flashTimer.current = setTimeout(() => setFlash(undefined), 1800);
  }, [bridge, history, next, selectedGame]);

  const runAction = useCallback(async (action: 'save' | 'load' | 'revert' | 'retry', checkpoint?: string) => {
    if (!selectedGame) return;
    clearTimeout(feedbackTimer.current);
    const game = selectedGame.id;
    setFeedback({ game, action, target: checkpoint, phase: 'busy' });
    try {
      const request: UiRequest = action === 'retry' ? { type: 'retry', game } : action === 'save'
        ? { type: 'save', game }
        : action === 'load' ? { type: 'load', game, checkpoint }
          : { type: 'revert', game, checkpoint: checkpoint! };
      const accepted = await bridge.request<Operation>(request);
      const outcome = await bridge.request<Operation>({ type: 'outcome', operation: accepted.id });
      if (outcome.status !== 'succeeded') throw new Error(failureMessage(outcome.error, `${action} failed`));
      setFeedback({ game, action, target: checkpoint, phase: 'success' });
      bridge.request<HistoryPage>({ type: 'history', game, limit: 100 }).then((page) => {
        if (historyGame.current !== game) return;
        receiveHistory(game, page.rows);
        setNext(page.next);
      }).catch(() => undefined);
      scrollRef.current?.scrollTo?.({ top: 0, behavior: scrollMotion() });
      feedbackTimer.current = setTimeout(() => setFeedback(undefined), 1600);
    } catch (error) {
      setFeedback({ game, action, target: checkpoint, phase: 'error', message: String(error instanceof Error ? error.message : error) });
    }
  }, [bridge, selectedGame, receiveHistory]);

  const scan = useCallback(async () => {
    clearTimeout(scanTimer.current);
    setScanFeedback({ label: 'Scanning…', phase: 'scanning' });
    try {
      const result = await bridge.request<{ new_games: number }>({ type: 'scan' });
      const count = result.new_games;
      setScanFeedback({ label: count ? `${count} game${count === 1 ? '' : 's'} found` : 'No new games', phase: 'success' });
    } catch (error) {
      setScanFeedback({ label: String(error instanceof Error ? error.message : error), phase: 'error' });
    }
    scanTimer.current = setTimeout(() => setScanFeedback(undefined), 2400);
  }, [bridge]);

  const commitDelete = useCallback(async (game: string, checkpoint: string) => {
    const key = deleteKey(game, checkpoint);
    try {
      const accepted = await bridge.request<Operation>({ type: 'delete', game, checkpoint });
      const outcome = await bridge.request<Operation>({ type: 'outcome', operation: accepted.id });
      if (outcome.status !== 'succeeded') throw new Error(failureMessage(outcome.error, 'Delete failed'));
      if (historyGame.current === game) setHistory((rows) => rows.filter((row) => row.checkpoint !== checkpoint));
    } catch (error) {
      setFeedback({ game, action: 'delete', target: checkpoint, phase: 'error',
        message: String(error instanceof Error ? error.message : error) });
    } finally {
      setHiddenDeletes((hidden) => { const next = new Set(hidden); next.delete(key); return next; });
    }
  }, [bridge]);

  const dismissUndoNotice = useCallback((game: string, checkpoint: string, exit: 'fade' | 'right') => {
    clearTimeout(undoExitTimer.current);
    setUndoNotice((notice) => notice?.game === game && notice.checkpoint === checkpoint
      ? { ...notice, exit } : notice);
    undoExitTimer.current = setTimeout(() => setUndoNotice((notice) =>
      notice?.game === game && notice.checkpoint === checkpoint ? undefined : notice), 260);
  }, []);

  const deleteCheckpoint = useCallback((checkpoint: string) => {
    if (!selectedGame) return;
    const game = selectedGame.id;
    const index = historyRef.current.findIndex((row) => row.checkpoint === checkpoint);
    if (index < 0) return;
    if (pendingUndo.current) {
      clearTimeout(pendingUndo.current.timer);
      void commitDelete(pendingUndo.current.game, pendingUndo.current.checkpoint);
    }
    clearTimeout(undoExitTimer.current);
    setFeedback(undefined);
    setHiddenDeletes((hidden) => new Set(hidden).add(deleteKey(game, checkpoint)));
    const timer = setTimeout(() => {
      pendingUndo.current = undefined;
      dismissUndoNotice(game, checkpoint, 'fade');
      void commitDelete(game, checkpoint);
    }, 5000);
    pendingUndo.current = { game, checkpoint, row: historyRef.current[index], index, timer };
    setUndoNotice({ game, checkpoint });
  }, [selectedGame, commitDelete, dismissUndoNotice]);

  const undoCheckpointDelete = useCallback(() => {
    const pending = pendingUndo.current;
    if (!pending) return;
    clearTimeout(pending.timer);
    pendingUndo.current = undefined;
    dismissUndoNotice(pending.game, pending.checkpoint, 'right');
    setHiddenDeletes((hidden) => { const next = new Set(hidden); next.delete(deleteKey(pending.game, pending.checkpoint)); return next; });
    if (historyGame.current !== pending.game) return;
    const row = pending.row;
    const restored = [...historyRef.current];
    if (!restored.some((item) => item.id === row.id)) restored.splice(Math.min(pending.index, restored.length), 0, row);
    setHistory(restored);
    clearTimeout(arrivalTimer.current);
    setArrived(new Set([row.id]));
    arrivalTimer.current = setTimeout(() => setArrived(new Set()), 1800);
    setFlash(row.id);
    clearTimeout(flashTimer.current);
    flashTimer.current = setTimeout(() => setFlash(undefined), 1800);
    const visible = restored.filter((item) => !item.checkpoint ||
      item.checkpoint === pending.checkpoint || !hiddenDeletes.has(deleteKey(pending.game, item.checkpoint)));
    const items = groupHistory(visible).flatMap(({ rows }) => [undefined, ...rows]);
    const top = itemTops(items).tops[items.indexOf(row)];
    window.requestAnimationFrame?.(() => {
      const viewport = scrollRef.current;
      if (viewport && (top < viewport.scrollTop || top + itemHeight(row) > viewport.scrollTop + viewport.clientHeight)) {
        viewport.scrollTo({ top: Math.max(0, top - viewport.clientHeight / 2), behavior: scrollMotion() });
      }
    });
  }, [hiddenDeletes, dismissUndoNotice]);

  useEffect(() => () => {
    clearTimeout(undoExitTimer.current);
    if (pendingUndo.current) {
      clearTimeout(pendingUndo.current.timer);
      void commitDelete(pendingUndo.current.game, pendingUndo.current.checkpoint);
      pendingUndo.current = undefined;
    }
  }, [commitDelete]);

  const finishFlush = useCallback(async (game: string, operation: string) => {
    setFeedback({ game, action: 'flush', phase: 'busy' });
    try {
      const outcome = await bridge.request<Operation>({ type: 'outcome', operation });
      if (outcome.status !== 'succeeded') throw new Error(failureMessage(outcome.error, 'Flush failed'));
      setFeedback({ game, action: 'flush', phase: 'success' });
      feedbackTimer.current = setTimeout(() => setFeedback(undefined), 1600);
    } catch (error) {
      setFeedback({ game, action: 'flush', phase: 'error', message: String(error instanceof Error ? error.message : error) });
    }
  }, [bridge]);

  // Running games surface to the top of the one list, in active-stack order;
  // the rest keep the host's order (last focused first).
  const stackIndex = (game: Game) => { const index = state?.active_stack.indexOf(game.id) ?? -1; return index < 0 ? Infinity : index; };
  const running = visibleGames.filter((game) => game.running).sort((a, b) => stackIndex(a) - stackIndex(b));
  const installed = [...running, ...visibleGames.filter((game) => !game.running)];
  const filteredGames = installed.filter((game) => matchesGameName(game.name, gameFilter));
  const resetFilter = () => {
    setGameFilter('');
    setFilterOpen(false);
    filterInputRef.current?.blur();
  };
  useLayoutEffect(() => { if (filterOpen) filterInputRef.current?.focus(); }, [filterOpen]);
  // When the order changes, each card slides from where it was to its new place (FLIP), clipped by the list.
  const libraryPanelRef = useRef<HTMLDivElement>(null);
  const cardTops = useRef(new Map<string, number>());
  const libraryOrder = filteredGames.map((game) => game.id).join('\n');
  const shownLibraryOrder = useRef(libraryOrder);
  useLayoutEffect(() => {
    const previous = cardTops.current;
    cardTops.current = new Map();
    const reordered = shownLibraryOrder.current !== libraryOrder;
    shownLibraryOrder.current = libraryOrder;
    const animate = reordered && typeof Element.prototype.animate === 'function' && !matchMedia('(prefers-reduced-motion: reduce)').matches;
    for (const card of libraryPanelRef.current?.querySelectorAll<HTMLElement>('[data-game]') ?? []) {
      const top = card.offsetTop;
      const before = previous.get(card.dataset.game!);
      cardTops.current.set(card.dataset.game!, top);
      if (!animate || before === undefined || before === top) continue;
      // A card still sliding from an earlier reorder continues from where it is drawn now.
      const drawnShift = parseFloat(getComputedStyle(card).translate.split(' ')[1]) || 0;
      card.getAnimations().filter((animation) => animation.id === 'reorder').forEach((animation) => animation.cancel());
      // Rising cards pass over the ones they overtake.
      const zIndex = before > top ? 1 : 0;
      card.animate({ translate: [`0 ${before + drawnShift - top}px`, '0 0'], zIndex: [zIndex, zIndex] }, { id: 'reorder', duration: 400, easing: 'cubic-bezier(.2, .8, .2, 1)' });
    }
  });
  const measureSidebar = useCallback(() => {
    const scroller = sidebarScrollRef.current;
    const track = sidebarTrackRef.current;
    if (!scroller || !track) return;
    const next = { top: scroller.scrollTop, viewport: scroller.clientHeight, content: scroller.scrollHeight, track: track.clientHeight };
    setSidebarScroll((previous) => previous.top === next.top && previous.viewport === next.viewport &&
      previous.content === next.content && previous.track === next.track ? previous : next);
  }, []);
  useEffect(() => {
    const scroller = sidebarScrollRef.current;
    const track = sidebarTrackRef.current;
    if (!scroller || !track) return;
    measureSidebar();
    window.addEventListener('resize', measureSidebar);
    if (typeof ResizeObserver === 'undefined') return () => window.removeEventListener('resize', measureSidebar);
    const observer = new ResizeObserver(measureSidebar);
    observer.observe(scroller);
    observer.observe(track);
    if (scroller.firstElementChild) observer.observe(scroller.firstElementChild);
    return () => { observer.disconnect(); window.removeEventListener('resize', measureSidebar); };
  }, [measureSidebar, visibleGames.length, filteredGames.length]);
  // Wheel and trackpad scroll the game list one card per gesture: any movement steps to the next card
  // edge, and further events are ignored until the input pauses (so momentum does not skip cards).
  useEffect(() => {
    const scroller = sidebarScrollRef.current;
    if (!scroller) return;
    let target: number | undefined;
    let targetAt = 0;
    let gestureTimer: ReturnType<typeof setTimeout> | undefined;
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey || Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
      const max = scroller.scrollHeight - scroller.clientHeight;
      if (max <= 0) return;
      event.preventDefault();
      const inGesture = gestureTimer !== undefined;
      clearTimeout(gestureTimer);
      gestureTimer = setTimeout(() => { gestureTimer = undefined; }, 150);
      if (inGesture) return;
      // Continue from a step still animating; otherwise from where the list actually is.
      const from = target !== undefined && performance.now() - targetAt < 600 ? target : scroller.scrollTop;
      const panel = scroller.querySelector<HTMLElement>('.library-panel');
      const inset = panel ? parseFloat(getComputedStyle(panel).paddingTop) : 0;
      const edges = [...scroller.querySelectorAll<HTMLElement>('.library-panel .game-card-wrap')]
        .map((card) => Math.min(max, card.offsetTop - scroller.offsetTop - inset));
      const next = event.deltaY > 0 ? edges.find((edge) => edge > from + 1) ?? max
        : [...edges].reverse().find((edge) => edge < from - 1) ?? 0;
      target = Math.max(0, Math.min(max, next));
      targetAt = performance.now();
      scroller.scrollTo({ top: target, behavior: scrollMotion() });
    };
    scroller.addEventListener('wheel', onWheel, { passive: false });
    return () => { scroller.removeEventListener('wheel', onWheel); clearTimeout(gestureTimer); };
  }, []);
  const sidebarScrollable = sidebarScroll.content > sidebarScroll.viewport + 1 && sidebarScroll.track > 0;
  const sidebarThumbHeight = sidebarScrollable
    ? Math.min(sidebarScroll.track, Math.max(24, sidebarScroll.track * sidebarScroll.viewport / sidebarScroll.content)) : 0;
  const sidebarThumbTop = sidebarScrollable
    ? (sidebarScroll.track - sidebarThumbHeight) * Math.min(1, Math.max(0,
      sidebarScroll.top / (sidebarScroll.content - sidebarScroll.viewport))) : 0;
  const dragSidebarThumb = (event: React.PointerEvent<HTMLDivElement>) => {
    const scroller = sidebarScrollRef.current;
    if (!scroller || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const thumb = event.currentTarget;
    const startY = event.clientY;
    const startTop = scroller.scrollTop;
    const ratio = (scroller.scrollHeight - scroller.clientHeight) / Math.max(1, sidebarScroll.track - sidebarThumbHeight);
    thumb.setPointerCapture(event.pointerId);
    thumb.classList.add('dragging');
    const move = (e: PointerEvent) => { scroller.scrollTop = startTop + (e.clientY - startY) * ratio; };
    const end = () => {
      thumb.classList.remove('dragging');
      thumb.removeEventListener('pointermove', move);
      thumb.removeEventListener('pointerup', end);
      thumb.removeEventListener('pointercancel', end);
    };
    thumb.addEventListener('pointermove', move);
    thumb.addEventListener('pointerup', end);
    thumb.addEventListener('pointercancel', end);
  };
  const pageSidebar = (event: React.PointerEvent<HTMLDivElement>) => {
    const scroller = sidebarScrollRef.current;
    if (!scroller || event.button !== 0) return;
    const above = event.clientY < event.currentTarget.getBoundingClientRect().top + sidebarThumbTop;
    scroller.scrollBy({ top: (above ? -1 : 1) * scroller.clientHeight * 0.9, behavior: scrollMotion() });
  };
  const working = Boolean(selectedGame?.busy) || (feedback?.game === selected && feedback?.phase === 'busy');
  const hostError = selectedGame?.last_result?.status === 'failed' ? selectedGame.last_result.error : undefined;
  const dialogTarget = (dialogGame && state?.games.find((game) => game.id === dialogGame)) || selectedGame;
  const addControl = <button key="add" onClick={(event) => openDialog('add', event.currentTarget)}><Icon name="add" />Add custom game</button>;
  const scanning = scanFeedback?.phase === 'scanning';
  const scanControl = <button key="scan" className={scanning ? 'scanning' : scanFeedback?.phase === 'success' ? 'scan-success' : ''} onClick={scan} disabled={scanning}>
    <Icon name={scanFeedback?.phase === 'success' ? 'success' : 'scan'} />{scanFeedback?.label || 'Scan for games'}
  </button>;

  const noGames = !!state && visibleGames.length === 0;

  return (
    <div className={`app ${working ? 'is-busy' : ''} ${noGames ? 'no-games' : ''}`}>
      <header className="window-bar" data-tauri-drag-region>
        {!mac && <WindowControls />}
      </header>
      <aside className="sidebar">
        {noGames && <Character name="no-games-found" sound={state?.settings?.play_sounds ?? true} />}
        <div className="sidebar-surface">
          {installed.length > 0 && <h2 className="library-heading">
            {filterOpen ? <span className="library-filter" onBlur={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget) && !gameFilter) resetFilter();
            }}>
              <svg className="library-search-icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="6.75" cy="6.75" r="4.5" /><path d="m10.2 10.2 4 4" /></svg>
              <input ref={filterInputRef} type="text" aria-label="Filter installed games"
                value={gameFilter} onChange={(event) => setGameFilter(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Escape') { event.stopPropagation(); resetFilter(); }
                  if (event.key === 'Enter' && filteredGames[0]) {
                    event.preventDefault();
                    if (filteredGames[0].id === selected) resetFilter();
                    else selectGame(filteredGames[0].id);
                  }
                }} />
              <button type="button" aria-label="Clear game filter" onClick={resetFilter}><Icon name="xmark" /></button>
            </span> : <button type="button" className="library-filter-toggle" aria-label="Filter installed games"
              onClick={() => setFilterOpen(true)}>
              <svg className="library-search-icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="6.75" cy="6.75" r="4.5" /><path d="m10.2 10.2 4 4" /></svg>
              INSTALLED
            </button>}
          </h2>}
          <div className={`library-scroll ${installed.length ? 'has-games' : ''}`} style={fadeStyle(sidebarScroll.top, sidebarScrollable ? sidebarScroll.content - sidebarScroll.viewport - sidebarScroll.top : 0)}>
            <div className="sidebar-content" ref={sidebarScrollRef} onScroll={measureSidebar}>
              {installed.length > 0 && <div className="library-panel" ref={libraryPanelRef}>
                {filteredGames.map((game) => <GameCard key={game.id} bridge={bridge} game={game} selected={game.id === selected}
                  pending={cardPending?.game === game.id} error={cardError?.game === game.id ? cardError.message : undefined}
                  onSelect={() => selectGame(game.id)} onLifecycle={(type) => runLifecycle(game, type)}
                  onConfigure={(opener) => openDialog('configure', opener, game.id)} />)}
                {filteredGames.length === 0 && <p className="library-no-matches">No matching games</p>}
              </div>}
            </div>
            {installed.length > 0 && <div className="library-frame" aria-hidden="true" />}
            <div className={`sidebar-scrollbar ${sidebarScrollable ? 'active' : ''}`} ref={sidebarTrackRef} aria-hidden="true"
              onPointerDown={sidebarScrollable ? pageSidebar : undefined}>
              {sidebarScrollable && <div className="sidebar-scrollbar-thumb" style={{ height: sidebarThumbHeight, top: sidebarThumbTop }}
                onPointerDown={dragSidebarThumb} />}
            </div>
          </div>
          <div className="library-controls">
            {visibleGames.length ? [addControl, scanControl] : [scanControl, addControl]}
            <button onClick={(event) => openDialog('settings', event.currentTarget)}><Icon name="settings" />Settings</button>
          </div>
        </div>
        <BrandButton onClick={(opener) => openDialog('about', opener)} />
      </aside>
      <main className="main">
        <p className="sr-only" role="status">{feedback?.phase === 'busy' ? `${feedback.action} in progress`
          : feedback?.phase === 'success' ? `${feedback.action} complete` : ''}</p>
        {status !== 'connected' && <p className="connection" role="status">{status.startsWith('reconnecting') ? 'Reconnecting to host…' : 'Connecting to host…'}</p>}
        {state?.store?.available === false && <p className="store-notice" role="alert">The checkpoint store is unavailable. Reconnect its drive to continue.</p>}
        {selectedGame ? <>
          <div className={`action-band ${selectedGame.guidance?.save ? 'covers-save' : ''} ${selectedGame.guidance?.load ? 'covers-load' : ''}`} aria-label="Checkpoint actions">
            <ActionButton action="save" game={selectedGame} feedback={feedback} busy={working} now={now}
              shortcut={state?.settings?.save_shortcut} onClick={() => runAction('save')} />
            <ActionButton action="load" game={selectedGame} feedback={feedback} busy={working} now={now}
              shortcut={state?.settings?.load_shortcut} onClick={() => runAction('load')}
              onJump={() => selectedGame.latest && jumpToCheckpoint(selectedGame.latest.id)} />
            <GuidancePanel key={selectedGame.id} game={selectedGame} bridge={bridge}
              retrying={selectedGame.busy?.kind === 'retry' || (feedback?.game === selected && feedback?.action === 'retry' && feedback?.phase === 'busy')}
              playPending={cardPending?.game === selectedGame.id}
              onRetry={() => runAction('retry')} onPlay={() => runLifecycle(selectedGame, 'play')}
              onConfigure={(button) => openDialog('configure', button)} />
          </div>
          {feedback?.game === selected && feedback?.phase === 'error' && <p className="action-error-block" role="alert">{feedback.message}</p>}
          {!(feedback?.game === selected && feedback?.phase === 'error') && hostError &&
            <p className="action-error-block" role="alert">{failureMessage(hostError, 'Game unavailable')}</p>}
          {alreadyAdded === selectedGame.id && <p className="library-notice" role="status">{selectedGame.name} was already in your library.</p>}
          {selectedGame.info && <div className="game-info">
            <div className="game-info-controls">
              <button className="info-button" onClick={() => setExpandedInfo((value) => ({ ...value, [selectedGame.id]: !value[selectedGame.id] }))}
                aria-expanded={Boolean(expandedInfo[selectedGame.id])} aria-controls={`game-info-${selectedGame.id}`}>
                <Icon name="info" />Info
              </button>
            </div>
            {expandedInfo[selectedGame.id] && <div id={`game-info-${selectedGame.id}`} className="game-info-content">
              <Markdown>{selectedGame.info}</Markdown>
            </div>}
          </div>}
          <div className="history-frame" style={fadeStyle(scrollTop, 0)}>
            <div key={selectedGame.id} className={`history history-${historyDirection}${revealing ? ' revealing' : ''}`}
              ref={scrollRef} aria-label={`${selectedGame.name} history`} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
              {historyError && <p className="history-error" role="alert">{historyError}</p>}
              {historyLoaded && virtualItems.length === 0 && !historyError && <div className="empty-history"><Character name="no-checkpoints" sound={state?.settings?.play_sounds ?? true} /><p>Checkpoints will appear here when you Save them.</p></div>}
              <div className="history-virtual" style={{ height: historyTotal + (next ? 52 : 0) }}>
                {dayLanes.filter((lane) => lane.end > firstVisible && lane.start < lastVisible).map(({ key, day, start, end }) => {
                  const top = itemTop[start];
                  const reveal = { '--reveal-index': Math.min(Math.max(start - firstVisible, 0), 12) } as React.CSSProperties;
                  return <div className="day-lane virtual-item" style={{ top, height: (itemTop[end] ?? historyTotal) - top, ...reveal }} key={key}>
                    <div className="day" style={{ height: itemHeight() }}><h2>{dayHeading(day, now)}</h2></div>
                  </div>;
                })}
                {virtualItems.slice(firstVisible, lastVisible).map(({ key, row }, offset) => {
                  const top = itemTop[firstVisible + offset];
                  const reveal = { '--reveal-index': Math.min(offset, 12) } as React.CSSProperties;
                  if (!row) return null;
                  const pending = state?.deletes.find((item) => item.checkpoint === row.checkpoint && item.game === selected);
                  return <div className="virtual-item" style={{ top, height: itemHeight(row), ...reveal }} key={key}><HistoryRow row={row} game={selectedGame} busy={working} now={now} bridge={bridge}
                    primary={row.kind === 'saved' && row.checkpoint === selectedGame.latest?.id}
                    flash={flash === row.id} arrived={arrived.has(row.id)} deleteOperation={pending}
                    feedback={feedback?.game === selected ? feedback : undefined}
                    onLoad={() => row.checkpoint && runAction('load', row.checkpoint)}
                    onRevert={() => row.checkpoint && runAction('revert', row.checkpoint)}
                    onJump={() => row.restored && jumpToCheckpoint(row.restored)}
                    onDelete={() => row.checkpoint && deleteCheckpoint(row.checkpoint)} /></div>;
                })}
                {next && <button className="load-more" style={{ top: historyTotal }} onClick={loadMore}>Show older history</button>}
              </div>
            </div>
          </div>
        </> : state && visibleGames.length ? <div className="empty-selection"><Character name="no-game-selected" /><p>{running.length ? 'Select a game to see its checkpoints.' : 'No known games are running.'}</p></div>
          : <div className="empty-library">{!state ? 'Waiting for the host…' : 'No games found.'}</div>}
      </main>
      {undoNotice && <div key={`${undoNotice.game}:${undoNotice.checkpoint}`} className={`undo-panel${undoNotice.exit ? ` exiting-${undoNotice.exit}` : ''}`}
        role="status" aria-label="Checkpoint removed">
        <span>Checkpoint removed</span>
        {!undoNotice.exit && <button className="undo-button" onClick={undoCheckpointDelete}>Undo</button>}
      </div>}
      {dialog === 'about' && <AboutDialog bridge={bridge} opener={dialogOpener.current} close={() => setDialog(undefined)} />}
      {dialog && dialog !== 'about' && <AppDialog key={`${dialog}-${dialogTarget?.id || ''}`} kind={dialog} game={dialogTarget} state={state} bridge={bridge} opener={dialogOpener.current}
        close={() => { setFlushOpener(undefined); setDialog(undefined); }} onAdded={(id, existing) => { setDialog(undefined); setSelected(id); setAlreadyAdded(existing ? id : undefined); }}
        onFlushed={(operation) => dialogTarget && finishFlush(dialogTarget.id, operation)} onFlush={setFlushOpener} />}
      {dialog && flushOpener && dialogTarget && <AppDialog key={`flush-over-${dialogTarget.id}`} kind="flush" game={dialogTarget} state={state} bridge={bridge}
        opener={flushOpener} close={() => setFlushOpener(undefined)} onAdded={() => undefined}
        onFlushed={(operation) => finishFlush(dialogTarget.id, operation)} />}
    </div>
  );
}

type CardAction = { icon: string; text: string; ariaLabel: string; onClick: () => void };

/** A checkpoint's time and age beside its optional label. With an action, the whole card is a button that shows
 * the action (Locate, Edit label…) over its muted contents on hover. */
function CheckpointCard({ at, now, fullDate = false, label, labelSlot, action, title, className = '' }: {
  at: string; now: number; fullDate?: boolean; label?: string; labelSlot?: React.ReactNode;
  action?: CardAction; title?: string; className?: string;
}) {
  const time = new Date(at).toLocaleTimeString(undefined, { hour12: false });
  const day = fullDate ? badgeDay(at, now) : undefined;
  const content = <>
    <span className={`checkpoint-when ${day ? 'dated' : ''}`}>
      <time dateTime={at}>{day ?? time}</time>
      <small>{day ? time : relativeAge(new Date(at), now)}</small>
    </span>
    {(labelSlot || label) && <span className="checkpoint-label">{labelSlot ?? <span className="checkpoint-label-text">{label}</span>}</span>}
  </>;
  if (!action) return <span className={`checkpoint-card ${className}`} title={title}>{content}</span>;
  return <button className={`checkpoint-card actionable ${className}`} onClick={action.onClick} aria-label={action.ariaLabel} title={title ?? label}>
    {content}
    <span className="checkpoint-action" aria-hidden="true"><Icon name={action.icon} /><span className="checkpoint-action-text">{action.text}</span></span>
  </button>;
}

function ActionButton({ action, game, feedback, busy, now, shortcut, onClick, onJump }: {
  action: 'save' | 'load'; game: Game; feedback?: Feedback; busy: boolean; now: number; shortcut?: string;
  onClick: () => void; onJump?: () => void;
}) {
  const available = game[action].available;
  const covered = Boolean(game.guidance?.[action]);
  const current: Feedback | undefined = feedback?.game === game.id && feedback.action === action && !feedback.target ? feedback
    : game.busy?.kind === action ? { game: game.id, action, phase: 'busy' } : undefined;
  const label = current?.phase === 'error' ? 'FAILED' : action.toUpperCase();
  const title = current?.message || failureMessage(game[action].failure, game[action].reason || '');
  // An empty shortcut was removed in Settings: no tab then.
  const tab = shortcut ?? `${mac ? 'Alt' : 'Ctrl'}+${action === 'save' ? 'F5' : 'F9'}`;
  // Keep the clickable checkpoint card separate from the Load button so the labels stay aligned.
  return <div className="action-slot" inert={covered}>
    {available && !covered && tab && <span className="shortcut-tab">{displayShortcut(tab)}</span>}
    <button className={`main-button ${action} ${current?.phase || ''}`} disabled={!available || busy || covered}
      onClick={onClick} title={title || undefined} aria-label={`${action} ${game.name}`}>
      {/* The hidden widest labels keep the button from resizing as SAVE turns into SAVING… or FAILED,
          while the visible icon and text stay centered together at a fixed gap. */}
      <span className="main-content" data-busy={`${action === 'save' ? 'SAVING' : 'LOADING'}…`} data-failed="FAILED">
        {/* Busy shows only a spinning icon, then a checkmark that pops in once done. */}
        {current?.phase === 'busy' || current?.phase === 'success'
          ? <span className="main-face icon-only" key={current.phase}><Icon name={current.phase} /></span>
          : <span className="main-face"><Icon name={action} />{label}</span>}
      </span>
    </button>
    {action === 'load' && <span className="load-card">{game.latest
      ? <CheckpointCard className={`load-badge ${!available ? 'unavailable' : ''}`} at={game.latest.created_at} now={now} fullDate
        label={game.latest.label} action={onJump && { icon: 'crosshairs', text: 'Locate', ariaLabel: 'Jump to latest checkpoint', onClick: onJump }} />
      : !game.guidance?.load && <span className={`load-badge empty ${!available ? 'unavailable' : ''}`}>No saves yet</span>}</span>}
  </div>;
}

function HistoryRow({ row, game, primary, busy, now, bridge, feedback, flash, arrived, deleteOperation,
  onLoad, onRevert, onJump, onDelete }: {
  row: HistoryEntry; game: Game; primary: boolean; busy: boolean; now: number; bridge: Bridge; feedback?: Feedback; flash: boolean; arrived: boolean;
  deleteOperation?: Operation;
  onLoad: () => void; onRevert: () => void; onJump: () => void; onDelete: () => void;
}) {
  const name = row.kind.charAt(0).toUpperCase() + row.kind.slice(1).replace('_', ' ');
  const date = new Date(row.at);
  if (row.kind === 'game_started' || row.kind === 'game_closed') {
    return <div id={`entry-${row.id}`} className="history-event" aria-label={`${name} ${date.toLocaleTimeString(undefined, { hour12: false })}`}>
      <Icon name={row.kind} /><span>{name}</span>
    </div>;
  }
  const active = feedback?.target === row.checkpoint && (feedback?.action === 'load' || feedback?.action === 'revert') ? feedback : undefined;
  const operation = row.kind === 'saved' ? 'load' : 'revert';
  const locked = !game.restore.available && !busy;
  const rowLabel = active?.phase === 'error' ? 'FAILED' : operation.toUpperCase();
  const chip = row.label || (row.kind === 'loaded' && row.saved_at ? new Date(row.saved_at).toLocaleTimeString(undefined, { hour12: false }) : undefined)
    || (row.kind === 'reverted' && row.reverted_at ? new Date(row.reverted_at).toLocaleTimeString(undefined, { hour12: false }) : undefined);
  return <div id={`entry-${row.id}`} className={`history-row ${primary ? 'primary' : 'secondary'} ${flash ? 'flash' : ''} ${arrived ? 'arrived' : ''}`}>
    <div className="history-stamp">
      {row.kind === 'saved' && row.checkpoint
        ? <EditableCheckpointCard bridge={bridge} checkpoint={row.checkpoint} at={row.at} now={now} label={row.label} />
        : <CheckpointCard at={row.at} now={now} label={row.label}
          action={row.restored ? { icon: 'crosshairs', text: 'Locate', ariaLabel: `Jump to ${chip}`, onClick: onJump } : undefined} />}
    </div>
    <div className="history-main"><Icon name={row.kind} /><span>{name}</span></div>
    <div className="history-note">
      {row.kind === 'loaded' && row.removed_files ? `Removed ${row.removed_files} newer save${row.removed_files === 1 ? '' : 's'}, kept in the recovery point` : ''}
      {row.kind === 'loaded' && row.removed_files && row.cloud_replaced ? '; ' : ''}
      {row.kind === 'loaded' && row.cloud_replaced ? 'Steam Cloud replaced the restored save' : ''}
    </div>
    <div className={`history-actions ${deleteOperation ? 'deleting' : ''}`}>
      {(row.actions.load || row.actions.revert) && <button disabled={!game.restore.available || busy || !!deleteOperation} className={`row-button ${row.actions.load ? '' : 'revert'} ${locked ? 'locked' : ''}`}
        onClick={row.actions.load ? onLoad : onRevert}
        aria-label={`${row.actions.load ? 'Load save' : 'Revert restore'} from ${date.toLocaleString()}`}>
        {active?.phase === 'busy' || active?.phase === 'success'
          ? <Icon key={active.phase} name={active.phase} className="icon-only" />
          : <><Icon name="load" />{rowLabel}</>}
      </button>}
      {row.actions.delete && <button disabled={!game.delete.available || !!deleteOperation} className="delete-button" onClick={onDelete}
        aria-label={`Delete checkpoint from ${date.toLocaleString()}`} title="Delete checkpoint"><Icon name="trash-can" /></button>}
      {deleteOperation && <div className="delete-status">
        {deleteOperation.status === 'waiting' && <span>Waiting to delete…</span>}
        {deleteOperation.status === 'running' && <span><Icon name="busy" />Deleting…</span>}
      </div>}
    </div>
  </div>;
}

/** A saved checkpoint's card whose label is edited in place. */
function EditableCheckpointCard({ bridge, checkpoint, at, now, label }: {
  bridge: Bridge; checkpoint: string; at: string; now: number; label?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(label || '');
  const [error, setError] = useState<string>();
  const original = useRef(label || '');
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const input = useRef<HTMLTextAreaElement>(null);

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
  // The editor starts as one centred line and grows to at most two as the text wraps.
  useLayoutEffect(() => {
    const field = input.current;
    if (!field) return;
    field.style.height = 'auto';
    field.style.height = `${Math.min(field.scrollHeight, 32)}px`;
  }, [draft, editing]);

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
  // Editing happens in place: the label part of the card becomes a pressed-in text field.
  if (editing) return <CheckpointCard at={at} now={now} labelSlot={<span className="label-chip editing">
    <textarea ref={input} rows={1} value={draft} maxLength={100} aria-label="Checkpoint label"
      onChange={(event) => setDraft(event.target.value.replace(/[\r\n]+/g, ' '))}
      onBlur={finish} onKeyDown={(event) => {
        if (event.key === 'Enter') { event.preventDefault(); finish(); }
        if (event.key === 'Escape') cancel();
      }} />
    <button className="label-confirm" aria-label="Confirm checkpoint label" onMouseDown={(event) => event.preventDefault()} onClick={finish}><Icon name="check" /></button>
  </span>} />;
  return <>
    <CheckpointCard at={at} now={now} label={label} title={error || label}
      action={{ icon: 'pencil', text: label ? 'Edit label' : 'Add label', ariaLabel: 'Edit checkpoint label',
        onClick: () => { original.current = label || ''; setDraft(label || ''); setEditing(true); } }} />
    {error && <span className="label-error" role="alert">{error}</span>}
  </>;
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
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  const yesterdayKey = `${yesterday.getFullYear()}-${String(yesterday.getMonth() + 1).padStart(2, '0')}-${String(yesterday.getDate()).padStart(2, '0')}`;
  const date = new Date(`${day}T12:00:00`);
  const name = day === todayKey ? 'Today' : day === yesterdayKey ? 'Yesterday'
    : now - date.getTime() < 7 * 24 * 3600 * 1000 && now >= date.getTime() ? date.toLocaleDateString(undefined, { weekday: 'long' }) : undefined;
  return <><Icon name="calendar" />{name && <span className="day-name">{name}</span>}<span className="day-date">{day}</span></>;
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

/** The day of a checkpoint older than today, shown above its time; undefined for today's, which show their age instead. */
function badgeDay(value: string, now: number) {
  const date = new Date(value);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) return undefined;
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) return 'Yesterday';
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
