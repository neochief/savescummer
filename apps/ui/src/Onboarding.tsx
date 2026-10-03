import { useEffect, useRef, useState } from 'react';
import type { Bridge } from './bridge';
import { SleepySkeleton } from './SleepySkeleton';
import type { Onboarding, OnboardingAction, OnboardingKind, OnboardingRow } from './types';

// What each permission is, in the user's words, and whether the app works without it. Which rows apply, and their
// results, come from the host.
const rows: Record<OnboardingKind, { name: string; text: string; optional?: boolean }> = {
  game_access: { name: 'Allow access to your games',
    text: 'Needed to find your games and back up and restore your progress.' },
  login_approval: { name: 'Start at login', optional: true,
    text: 'Keeps your hotkeys ready whenever you play.' },
  shortcuts: { name: 'Allow keyboard shortcuts',
    text: 'Needed to save and load from inside your game.' },
};

const actions: Record<OnboardingAction, string> = {
  allow_access: 'Allow access',
  open_settings: 'Open settings',
  check_again: 'Check again',
  set_up: 'Set up',
};

/** The first-launch permission screen: one row per permission the host found the user can allow. Skip turns into
 * Continue once the host confirmed any of them; either one, like closing the window, finishes setup for good. */
export function OnboardingScreen({ onboarding, bridge, sound }: { onboarding: Onboarding; bridge: Bridge; sound: boolean }) {
  const [error, setError] = useState<string>();
  const [finishing, setFinishing] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { if (!onboarding.inspecting) heading.current?.focus(); }, [onboarding.inspecting]);
  // A desktop portal that's just starting can take seconds to answer; the note fades in only then.
  if (onboarding.inspecting) return <main className="onboarding" aria-busy="true">
    <p className="onboarding-loading" role="status">Getting ready…</p>
  </main>;
  const requesting = onboarding.rows.some((row) => row.status === 'requesting');
  const request = async (row: OnboardingRow) => {
    setError(undefined);
    try {
      await bridge.request({ type: 'request_onboarding_permission', session: onboarding.session, row: row.id });
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
  };
  const finish = async () => {
    setFinishing(true);
    try {
      await bridge.request({ type: 'finish_onboarding', session: onboarding.session });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setFinishing(false);
    }
  };
  return <main className="onboarding" aria-labelledby="onboarding-heading">
    <div className="onboarding-panel">
      <header className="onboarding-header">
        <div className="empty-character" aria-hidden="true"><SleepySkeleton className="character-art pokeable" sound={sound} /></div>
        <h1 id="onboarding-heading" ref={heading} tabIndex={-1}>Before you play</h1>
      </header>
      <ul className="onboarding-rows">
        {onboarding.rows.map((row) => {
          const { name, text, optional } = rows[row.kind];
          return <li key={row.id} className={`onboarding-row ${row.status}`}>
            <div className="onboarding-row-text">
              <strong id={`onboarding-${row.id}`}>{name}{optional && <span className="onboarding-optional"> optional</span>}</strong>
              <p>{text}</p>
              <p className="onboarding-message" role="status">{row.message}</p>
            </div>
            {row.status === 'granted'
              ? <span className="onboarding-allowed" aria-label={`${name}: allowed`}>
                <span className="onboarding-check" aria-hidden="true" />Allowed</span>
              : row.action && <button aria-describedby={`onboarding-${row.id}`} disabled={requesting}
                onClick={() => request(row)}>{row.status === 'requesting' ? 'Waiting' : actions[row.action]}</button>}
          </li>;
        })}
      </ul>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
      <footer>
        <button className="onboarding-exit" disabled={finishing} onClick={finish}>
          {onboarding.any_permission_confirmed ? 'Continue' : 'Skip'}</button>
      </footer>
    </div>
  </main>;
}
