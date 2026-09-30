import { useRef, useState, type KeyboardEvent } from 'react';
import { capturedShortcut, displayShortcut, heldModifiers } from './shortcuts';

/**
 * Looks like a button until clicked, then records the next combination.
 * While recording, inline buttons put back the previous shortcut (the
 * default when there was none) or remove it.
 */
export function ShortcutInput({ id, value, fallback, invalid, describedBy, onChange, onRecording }: {
  id: string; value: string; fallback: string; invalid?: boolean; describedBy?: string;
  onChange: (shortcut: string) => void; onRecording: (recording: boolean) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState('');
  const button = useRef<HTMLButtonElement>(null);
  const restore = value || fallback;
  const restoreTitle = value ? `Keep ${displayShortcut(value)}` : `Use default ${displayShortcut(fallback)}`;

  function record(on: boolean) {
    if (on === recording) return;
    setRecording(on);
    setHeld('');
    onRecording(on);
  }

  function finish(shortcut: string) {
    onChange(shortcut);
    record(false);
    button.current?.focus();
  }

  function keyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (!recording || event.key === 'Tab') return;
    event.preventDefault();
    event.stopPropagation();
    const bare = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (event.key === 'Escape') record(false);
    else if (bare && (event.key === 'Backspace' || event.key === 'Delete')) finish('');
    else {
      const shortcut = capturedShortcut(event);
      if (shortcut) finish(shortcut);
      else setHeld(heldModifiers(event));
    }
  }

  const text = recording ? held || <span className="shortcut-placeholder">Type shortcut</span> : value ? displayShortcut(value) : 'Record shortcut';
  return <div className={`shortcut-input${recording ? ' recording' : value ? '' : ' unset'}`}
    onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) record(false); }}>
    <button ref={button} type="button" id={id} className="shortcut-value" data-shortcut-field
      aria-labelledby={`${id}-label ${id}`} aria-invalid={invalid} aria-describedby={describedBy}
      onClick={() => record(true)} onKeyDown={keyDown} onKeyUp={(event) => { if (recording) setHeld(heldModifiers(event)); }}>
      {text}
    </button>
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
  </div>;
}
