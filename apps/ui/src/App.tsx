import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import Markdown from 'react-markdown';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Bridge } from './bridge';
import type { Failure, Game, HistoryEntry, HistoryPage, HostState, Operation, UiRequest } from './types';
import { AppDialog, formatBytes, type DialogKind } from './Dialogs';
import { displayShortcut } from './shortcuts';

type Action = 'save' | 'load' | 'revert' | 'delete' | 'flush';
type Feedback = { game: string; action: Action; target?: string; phase: 'busy' | 'success' | 'error'; message?: string };

const accessCategories: Record<string, string> = {
  documents: 'Documents', desktop: 'Desktop', downloads: 'Downloads',
  icloud_drive: 'iCloud Drive', volumes: 'removable or network drives',
  app_data: "other apps' data", app_bundles: 'app bundles',
};

function failureMessage(failure: Failure | undefined, fallback: string): string {
  if (!failure) return fallback;
  if (failure.kind === 'access_needed') {
    const location = failure.detail && accessCategories[failure.detail];
    return `SaveScummer needs permission to access ${location || "this game's files"}.`;
  }
  return failure.detail || failure.kind || fallback;
}

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

function Brand() {
  return <div className="brand">
    <img className="app-icon" src="/app-icon.svg" alt="" aria-hidden="true" />
    <span className="wordmark" aria-hidden="true"><span>Save</span><strong>Scummer</strong></span>
  </div>;
}

// Windows runs undecorated (tauri.windows.conf.json), so the window bar draws its own caption buttons. There is no
// Maximize: the window's width is capped (maxWidth), so it can't fill the screen.
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

function GameCard({ bridge, game, selected, onSelect, onConfigure }: {
  bridge: Bridge; game: Game; selected: boolean; onSelect: () => void; onConfigure: (opener: HTMLButtonElement) => void;
}) {
  const hero = useArtwork(bridge, game, game.artwork?.hero ? 'hero' : 'header');
  const logo = useArtwork(bridge, game, 'logo');
  return (
    <div className={`game-card-wrap ${selected ? 'selected' : ''} ${game.running ? 'running' : 'installed'}`}>
      <button className={`game-card ${selected ? 'selected' : ''} ${game.running ? 'running' : 'installed'}`}
        onClick={onSelect} aria-current={selected ? 'true' : undefined}
        aria-label={`${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}${game.running ? '' : ', Not running'}`}
        data-tooltip={`${game.name}${game.install_tag ? ` — ${game.install_tag}` : ''}${game.running ? '' : ' — Not running'}`}>
        {hero && <img className="game-art" src={hero} alt="" />}
        <span className="game-shade" />
        {logo ? <img className="game-logo" src={logo} alt="" /> : <span className="game-fallback">{game.name}</span>}
        {game.install_tag && <span className="install-tag">{game.install_tag}</span>}
      </button>
      {game.running && <span className="running-badge"><span className="running-dot" />RUNNING</span>}
      <div className="card-actions">
        <button className="card-action" onClick={(event) => onConfigure(event.currentTarget)}
          aria-label={`Configure ${game.name}`} title="Configure">
          <span className="card-action-icon gear" />
        </button>
      </div>
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
  const [scanFeedback, setScanFeedback] = useState<string>();
  const [labelsRevision, setLabelsRevision] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const scrollRef = useRef<HTMLDivElement>(null);
  const feedbackTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const arrivalTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const historyGame = useRef<string | undefined>(undefined);
  // Game whose history finished loading at least once; only refreshes of it can reveal new rows.
  const loadedGame = useRef<string | undefined>(undefined);
  const historyRef = useRef(history);
  historyRef.current = history;
  const previousActiveStack = useRef<string[]>([]);
  const deleteDeadlines = useRef(new Map<string, number>());
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
  const sidebarScrollRef = useRef<HTMLDivElement>(null);
  const sidebarTrackRef = useRef<HTMLDivElement>(null);
  const [sidebarScroll, setSidebarScroll] = useState({ top: 0, viewport: 0, content: 0, track: 0 });

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
    return () => { live = false; unlisteners.forEach((unlisten) => unlisten()); clearTimeout(feedbackTimer.current); clearTimeout(arrivalTimer.current); };
  }, [bridge]);

  const visibleGames = useMemo(() => state?.games.filter((game) => game.installed) || [], [state]);
  const selectedGame = visibleGames.find((game) => game.id === selected);
  const virtualItems = useMemo(() => groupHistory(history).flatMap(({ day, rows }) => [
    { key: `day-${day}`, day, row: undefined as HistoryEntry | undefined },
    ...rows.map((row) => ({ key: row.id, day, row })),
  ]), [history]);
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
    if (selected && !known(selected)) setSelected(undefined);
    else if (!selected && known(active)) setSelected(active);
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
    setHistory(rows);
  }, []);

  useEffect(() => {
    let live = true;
    if (historyGame.current !== selectedGame?.id) {
      historyGame.current = selectedGame?.id;
      loadedGame.current = undefined;
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
          message: failureMessage(outcome.error, 'Delete failed') });
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

  // Running games surface to the top of the one list, in active-stack order.
  const stackIndex = (game: Game) => { const index = state?.active_stack.indexOf(game.id) ?? -1; return index < 0 ? Infinity : index; };
  const running = visibleGames.filter((game) => game.running).sort((a, b) => stackIndex(a) - stackIndex(b));
  const installed = [...running, ...visibleGames.filter((game) => !game.running)];
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
  }, [measureSidebar, visibleGames.length, running.length]);
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
  // The host refuses Save, Load and Revert while a game that writes its progress only on exit runs: the actions give
  // way to a panel until it has exited.
  const exitFirst = selectedGame?.save.reason === 'game_running' || selectedGame?.load.reason === 'game_running';
  const working = Boolean(selectedGame?.busy) || (feedback?.game === selected && feedback?.phase === 'busy');
  // The lockdown disables the actions like an operation would, but nothing is working, so no wait cursor.
  const busy = exitFirst || working;
  const hostError = selectedGame?.blocked || selectedGame?.config_error ||
    (selectedGame?.last_result?.status === 'failed' ? selectedGame.last_result.error : undefined);
  const dialogTarget = (dialogGame && state?.games.find((game) => game.id === dialogGame)) || selectedGame;
  const addControl = <button key="add" onClick={(event) => openDialog('add', event.currentTarget)}><Icon name="add" />Add custom game</button>;
  const scanning = scanFeedback === 'Scanning…';
  const scanControl = <button key="scan" className={scanning ? 'scanning' : ''} onClick={scan} disabled={scanning}>
    <Icon name="scan" />{scanFeedback || 'Scan for games'}
  </button>;

  const noGames = !!state && visibleGames.length === 0;

  return (
    <div className={`app ${working ? 'is-busy' : ''} ${noGames ? 'no-games' : ''}`}>
      <header className="window-bar" data-tauri-drag-region>
        {!noGames && <Brand />}
        {!mac && <WindowControls />}
      </header>
      <aside className="sidebar">
        {noGames && <Brand />}
        <div className="sidebar-surface">
          {installed.length > 0 && <h2 className="library-heading">INSTALLED</h2>}
          <div className={`library-scroll ${installed.length ? 'has-games' : ''}`} style={fadeStyle(sidebarScroll.top, sidebarScrollable ? sidebarScroll.content - sidebarScroll.viewport - sidebarScroll.top : 0)}>
            <div className="sidebar-content" ref={sidebarScrollRef} onScroll={measureSidebar}>
              {installed.length > 0 && <div className="library-panel">
                {installed.map((game) => <GameCard key={game.id} bridge={bridge} game={game} selected={game.id === selected} onSelect={() => selectGame(game.id)}
                  onConfigure={(opener) => openDialog('configure', opener, game.id)} />)}
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
      </aside>
      <main className="main">
        <p className="sr-only" role="status">{feedback?.phase === 'busy' ? `${feedback.action} in progress`
          : feedback?.phase === 'success' ? `${feedback.action} complete` : ''}</p>
        {status !== 'connected' && <p className="connection" role="status">{status.startsWith('reconnecting') ? 'Reconnecting to host…' : 'Connecting to host…'}</p>}
        {selectedGame ? <>
          <div className={`action-band ${exitFirst ? 'exit-first' : ''}`} aria-label="Checkpoint actions">
            <ActionButton action="save" game={selectedGame} feedback={feedback} busy={busy} now={now}
              shortcut={state?.settings?.save_shortcut} onClick={() => runAction('save')} />
            <ActionButton action="load" game={selectedGame} feedback={feedback} busy={busy} now={now}
              shortcut={state?.settings?.load_shortcut} onClick={() => runAction('load')}
              onJump={() => selectedGame.latest && jumpToCheckpoint(selectedGame.latest.id)} />
            {exitFirst && <ExitFirst />}
          </div>
          {feedback?.game === selected && feedback?.phase === 'error' && <p className="action-error-block" role="alert">{feedback.message}</p>}
          {!(feedback?.game === selected && feedback?.phase === 'error') && hostError &&
            <p className="action-error-block" role="alert">{failureMessage(hostError, 'Game unavailable')}</p>}
          {selectedGame.info && <div className="game-info">
            <button onClick={() => setExpandedInfo((value) => ({ ...value, [selectedGame.id]: !value[selectedGame.id] }))}
              aria-expanded={Boolean(expandedInfo[selectedGame.id])} aria-controls={`game-info-${selectedGame.id}`}>
              <Icon name="info" />Info
            </button>
            {expandedInfo[selectedGame.id] && <div id={`game-info-${selectedGame.id}`} className="game-info-content">
              <Markdown>{selectedGame.info}</Markdown>
            </div>}
          </div>}
          <div className="history-frame" style={fadeStyle(scrollTop, 0)}>
            <div key={selectedGame.id} className={`history history-${historyDirection}${revealing ? ' revealing' : ''}`}
              ref={scrollRef} aria-label={`${selectedGame.name} history`} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
              {historyError && <p className="history-error" role="alert">{historyError}</p>}
              {virtualItems.length === 0 && !historyError && <p className="empty-history">Saves will appear here.</p>}
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
                  return <div className="virtual-item" style={{ top, height: itemHeight(row), ...reveal }} key={key}><HistoryRow row={row} busy={busy} locked={exitFirst} now={now} bridge={bridge}
                    primary={row.kind === 'saved' && row.checkpoint === selectedGame.latest?.id}
                    flash={flash === row.id} arrived={arrived.has(row.id)} deleteOperation={pending} deadline={pending && deleteDeadlines.current.get(pending.id)}
                    feedback={feedback?.game === selected ? feedback : undefined}
                    onLoad={() => row.checkpoint && runAction('load', row.checkpoint)}
                    onRevert={() => row.checkpoint && runAction('revert', row.checkpoint)}
                    onJump={() => row.restored && jumpToCheckpoint(row.restored)}
                    onDelete={() => row.checkpoint && deleteCheckpoint(row.checkpoint)}
                    onCancelDelete={() => pending && cancelDelete(pending.id)} /></div>;
                })}
                {next && <button className="load-more" style={{ top: historyTotal }} onClick={loadMore}>Show older history</button>}
              </div>
            </div>
          </div>
        </> : <div className="empty-library">{!state ? 'Waiting for the host…' : visibleGames.length ? 'No known games are running.' : 'No games found.'}</div>}
      </main>
      {dialog && <AppDialog key={`${dialog}-${dialogTarget?.id || ''}`} kind={dialog} game={dialogTarget} state={state} bridge={bridge} opener={dialogOpener.current}
        close={() => { setFlushOpener(undefined); setDialog(undefined); }} onAdded={(id) => { setDialog(undefined); setSelected(id); }}
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
  const current: Feedback | undefined = feedback?.game === game.id && feedback.action === action && !feedback.target ? feedback
    : game.busy?.kind === action ? { game: game.id, action, phase: 'busy' } : undefined;
  const label = current?.phase === 'error' ? 'FAILED' : action.toUpperCase();
  const reason = game[action].reason;
  const title = current?.message || (reason === 'access_needed'
    ? failureMessage(game.config_error || game.blocked, 'SaveScummer needs permission to access this game’s files.')
    : reason === 'no_game_data' ? 'No game data yet'
      : reason === 'no_saves' ? 'No saves yet' : reason);
  // The Load button shows the latest checkpoint on its right. A clickable card can't sit inside the button,
  // so the card is laid over it and the button reserves the card's measured width.
  const cardRef = useRef<HTMLSpanElement>(null);
  const [cardWidth, setCardWidth] = useState(0);
  useEffect(() => {
    const card = cardRef.current;
    if (!card || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => setCardWidth(card.offsetWidth));
    observer.observe(card);
    return () => observer.disconnect();
  }, [action]);
  return <div className="action-slot" style={action === 'load' ? { '--card-width': `${cardWidth}px` } as React.CSSProperties : undefined}>
    {available && <span className="shortcut-tab">
      {displayShortcut(shortcut || `${mac ? 'Alt' : 'Ctrl'}+${action === 'save' ? 'F5' : 'F9'}`)}
    </span>}
    <button className={`main-button ${action} ${current?.phase || ''}`} disabled={!available || busy}
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
    {action === 'load' && <span className="load-card" ref={cardRef}>{game.latest
      ? <CheckpointCard className={`load-badge ${!available ? 'unavailable' : ''}`} at={game.latest.created_at} now={now} fullDate
        label={game.latest.label} action={onJump && { icon: 'crosshairs', text: 'Locate', ariaLabel: 'Jump to latest checkpoint', onClick: onJump }} />
      : <span className={`load-badge empty ${!available ? 'unavailable' : ''}`}>No saves yet</span>}</span>}
  </div>;
}

/** Covers Save and Load while the game runs: its progress reaches the disk only when it exits. */
function ExitFirst() {
  return <section className="exit-first-panel" role="status" aria-label="Exit the game first">
    <strong>Save and Exit the game</strong>
    <span className="exit-first-sub">to save or load any checkpoints</span>
    <p>We can only intercept the game progress after the game saves it to disk.</p>
  </section>;
}

function HistoryRow({ row, primary, busy, locked, now, bridge, feedback, flash, arrived, deleteOperation, deadline,
  onLoad, onRevert, onJump, onDelete, onCancelDelete }: {
  row: HistoryEntry; primary: boolean; busy: boolean; locked: boolean; now: number; bridge: Bridge; feedback?: Feedback; flash: boolean; arrived: boolean;
  deleteOperation?: Operation; deadline?: number;
  onLoad: () => void; onRevert: () => void; onJump: () => void; onDelete: () => void; onCancelDelete: () => void;
}) {
  const name = row.kind.charAt(0).toUpperCase() + row.kind.slice(1).replace('_', ' ');
  const date = new Date(row.at);
  if (row.kind === 'game_started' || row.kind === 'game_closed') {
    return <div id={`entry-${row.id}`} className="history-event" aria-label={`${name} ${date.toLocaleTimeString(undefined, { hour12: false })}`}>
      <Icon name={row.kind} /><span>{name}</span>
    </div>;
  }
  const active = feedback?.target === row.checkpoint && (feedback?.action === 'load' || feedback?.action === 'revert') ? feedback : undefined;
  const operation = row.actions.load ? 'load' : 'revert';
  const rowLabel = active?.phase === 'error' ? 'FAILED' : operation.toUpperCase();
  const chip = row.label || (row.kind === 'loaded' && row.saved_at ? new Date(row.saved_at).toLocaleTimeString(undefined, { hour12: false }) : undefined)
    || (row.kind === 'reverted' && row.reverted_at ? new Date(row.reverted_at).toLocaleTimeString(undefined, { hour12: false }) : undefined);
  const count = Math.max(1, Math.ceil(((deadline || Date.now() + (deleteOperation?.remaining_ms || 0)) - now) / 1000));
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
      {(row.actions.load || row.actions.revert) && <button disabled={busy || !!deleteOperation} className={`row-button ${row.actions.load ? '' : 'revert'} ${locked ? 'locked' : ''}`}
        onClick={row.actions.load ? onLoad : onRevert}
        aria-label={`${row.actions.load ? 'Load save' : 'Revert restore'} from ${date.toLocaleString()}`}>
        {active?.phase === 'busy' || active?.phase === 'success'
          ? <Icon key={active.phase} name={active.phase} className="icon-only" />
          : <><Icon name="load" />{rowLabel}</>}
      </button>}
      {row.actions.delete && <button disabled={busy || !!deleteOperation} className="delete-button" onClick={onDelete}
        aria-label={`Delete checkpoint from ${date.toLocaleString()}`} title="Delete checkpoint"><Icon name="trash-can" /></button>}
      {deleteOperation && <div className="delete-status">
        {deleteOperation.status === 'counting_down' && <><span>Deleting in {count}</span><button className="cancel-delete" onClick={onCancelDelete}>CANCEL</button></>}
        {deleteOperation.status === 'waiting' && <><span>Waiting to delete…</span><button className="cancel-delete" onClick={onCancelDelete}>CANCEL</button></>}
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
