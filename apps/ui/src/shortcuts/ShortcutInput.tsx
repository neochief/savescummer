import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { capturedShortcut, displayShortcut, heldModifiers, shortcutDescription, shortcutError, shortcutWarning } from './shortcuts';
import './ShortcutInput.css';

/** Bolds the system action a message names, so it stands out. */
function shortcutMessage(text: string, shortcut: string) {
  const description = shortcutDescription(shortcut);
  const at = description ? text.indexOf(description) : -1;
  if (!description || at < 0) return text;
  return <>{text.slice(0, at)}<strong>{description}</strong>{text.slice(at + description.length)}</>;
}

/**
 * Looks like a button until clicked, then turns into a focused text field
 * that records the next combination; leaving the field cancels. While
 * recording, inline buttons put back the previous shortcut (the
 * default when there was none) or remove it. It checks what it holds as
 * soon as it's recorded: against `other`, the shortcut it must differ from,
 * and against the system's own shortcuts. `rejection` is the host's word,
 * which wins until the shortcut changes.
 */
export function ShortcutInput({ id, label, value, fallback, other, rejection, onChange, onRecording }: {
  id: string; label: string; value: string; fallback: string; other: string; rejection?: string;
  onChange: (shortcut: string) => void; onRecording: (recording: boolean) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState('');
  const button = useRef<HTMLButtonElement>(null);
  // Set when recording ends from the keyboard or an inline button, so focus returns to the field.
  const refocus = useRef(false);
  const restore = value || fallback;
  const restoreTitle = value ? `Keep ${displayShortcut(value)}` : `Use default ${displayShortcut(fallback)}`;
  const error = rejection ?? shortcutError(value, other);
  const warning = error ? undefined : shortcutWarning(value);
  const describedBy = error ? `${id}-error` : warning ? `${id}-warning` : undefined;

  useEffect(() => {
    if (recording || !refocus.current) return;
    refocus.current = false;
    button.current?.focus();
  }, [recording]);

  function record(on: boolean) {
    if (on === recording) return;
    setRecording(on);
    setHeld('');
    onRecording(on);
  }

  function finish(shortcut?: string) {
    if (shortcut !== undefined) onChange(shortcut);
    refocus.current = true;
    record(false);
  }

  function keyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Tab') return;
    event.preventDefault();
    event.stopPropagation();
    const bare = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (event.key === 'Escape') finish();
    else if (bare && (event.key === 'Backspace' || event.key === 'Delete')) finish('');
    else {
      const shortcut = capturedShortcut(event);
      if (shortcut) finish(shortcut);
      else setHeld(heldModifiers(event));
    }
  }

  return <><div className="dialog-field"><label id={`${id}-label`} htmlFor={id}>{label}</label><div className={`shortcut-input${recording ? ' recording' : value ? '' : ' unset'}`}
    onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) record(false); }}>
    {recording
      ? <input autoFocus id={id} className="shortcut-value" readOnly value={held} placeholder="Type shortcut"
        aria-labelledby={`${id}-label`} aria-invalid={Boolean(error)} aria-describedby={describedBy}
        onKeyDown={keyDown} onKeyUp={(event) => setHeld(heldModifiers(event))} />
      : <button ref={button} type="button" id={id} className="shortcut-value"
        aria-labelledby={`${id}-label ${id}`} aria-invalid={Boolean(error)} aria-describedby={describedBy}
        onClick={() => record(true)}>
        {value ? displayShortcut(value) : 'Record shortcut'}
      </button>}
    {recording && <span className="shortcut-actions">
      {/* Mouse-down keeps focus on the field, so recording doesn't end before the click lands. */}
      <button type="button" className="shortcut-restore" title={restoreTitle} aria-label={restoreTitle}
        onMouseDown={(event) => event.preventDefault()} onClick={() => finish(restore)}>
        <img className="icon" src="/icons/rotate-left.svg" alt="" aria-hidden="true" />
      </button>
      <button type="button" className="shortcut-clear" title="Remove shortcut" aria-label="Remove shortcut"
        onMouseDown={(event) => event.preventDefault()} onClick={() => finish('')}>
        <img className="icon" src="/icons/xmark.svg" alt="" aria-hidden="true" />
      </button>
    </span>}
  </div></div>
  {error && <p id={`${id}-error`} className="dialog-error shortcut-error" role="alert">{shortcutMessage(error, value)}</p>}
  {warning && <p id={`${id}-warning`} className="dialog-warning shortcut-warning" role="status">{shortcutMessage(warning, value)}</p>}
  </>;
}
