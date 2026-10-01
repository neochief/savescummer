# Path field

One `PathField` component for the Add and Configure dialogs' **game executable** and
**single save location**. Today both are inline JSX in `apps/ui/src/Dialogs.tsx`, with
their state spread across `GameDialog`.

Out of scope:
- **Catalog save paths** get their own `SavePathList` (see below).
- **The checkpoints store** stays inline: a disabled input with 👁 and the Flush button,
  and no picker.

## Behaviour

The field is **read-only with paste** (`<input readOnly>`, not `disabled`):

- **Focus, select and copy work** as in any input.
- **Ctrl/Cmd+V replaces the whole value.** `onPaste` cancels the default action, takes the clipboard text,
  runs it through `cleanPath` (quotes from "Copy as path", Terminal escapes) and calls
  `onPick`. Multi-line text is refused, or only its first line is used.
- **Right-click → Paste is greyed out** on read-only fields. That's accepted.
- **Pasted, picked and edited values all behave alike.** They arrive whole, so errors show at once.
  There's no "check on blur" step outside edit mode.
- **Ctrl+Z doesn't undo a paste.** Reset or picking again covers that.

### Buttons inside the field

- **👁 Show in folder:** as today, enabled only when the value is what the host holds.
- **✏️ Edit:** only when `editable` and the field has a value. In this version that's
  the save location only. Never the executable: it's only chosen, never typed.

### Button beside the field

- **Choose (Add) / Change (Configure)** opens the file picker. The picker starts where
  the field points: the UI asks the host with `picker_start` (already implemented). The
  host expands `~` and `%APPDATA%`, cuts a pattern back to its folder, and walks up to the
  nearest existing folder. An existing file opens with that file selected.

### Edit mode (save location)

- **Clicking ✏️** makes the field typeable, focuses it and puts the cursor at the end, so
  `/*.sav` can be appended. The pencil turns into ✓ (Done), and 👁 is off.
- **The pattern hint** ("A folder, a file, or a pattern such as …") shows only in edit mode.
- **Enter, ✓ or blur keeps the change:** `cleanPath`, then `onPick`, then back to read-only.
- **Escape restores** the value from before editing.
- **While typing,** errors wait until the edit is finished (today's `checked` logic, now
  only inside edit mode).
- **Labels:** aria-label "Edit save location", title "Edit", and "Done" for the ✓.

### Discoverability and accessibility

- **Placeholder:** "Paste a path or click Choose" ("…Change" in Configure). The same text
  goes in the accessible description (`aria-describedby`), together with `description`.
- **Problems** set `aria-invalid` and show as `role="alert"` below the field, in place of
  `description`.

### Styling

- **Read-only here doesn't mean inactive.** Use normal text colour and a focus ring, not
  the greyed `disabled` look. The cursor is the default arrow, not a text cursor, except
  in edit mode.
- **One container for the line below** the field. The Add dialog's "Example: …" line moves
  by a few pixels to match the others.

### Required fields

Browsers skip `required` on read-only inputs. On submit, `GameDialog` checks for empty
fields itself, as it already does, and also focuses and marks the empty field the way it
does for problems.

## Props

```ts
type PathFieldProps = {
  id: string;
  label: string;
  value: string;
  onPick: (path: string) => void;          // paste, picker, or a finished edit
  editable?: boolean;                      // shows ✏️ Edit
  error?: ReactNode;                       // may hold links (save location's "Ask Google or ChatGPT")
  description?: ReactNode | ((editing: boolean) => ReactNode);
  directory?: boolean;
  dialogTitle: string;
  browseLabel: string;                     // 'Choose' | 'Change'
  onReveal?: () => void;                   // no handler → no 👁
  revealDisabled?: boolean;
  revealLabel?: string;
  bridge: Bridge;                          // picker_start
  onError: (message: string) => void;      // picker / lookup failures
};
```

### What `PathField` owns

- **Layout:** the label, the `dialog-input` wrapper, 👁, ✏️/✓, and the Choose/Change
  button.
- **Input handling:** paste, the edit-mode state, `cleanPath`, the `picker_start` lookup
  and `open()`.
- **Problem and description display**, including `aria-invalid` and `aria-describedby`.

### What stays in `GameDialog`

- **Validation:** `platformProblem`, the "too broad" refusal, and which errors to show.
- **Reset, per field:**
  - the executable has `resetExecutable` and the override flag;
  - the save location just clears the field;
  - custom games never get Reset.
- **Hint text:** the executable example, and the Google/ChatGPT links with the warning.
- **Side effects of a pick:** in Add, picking an executable fills an empty Name from the
  file name, and pasting does the same.

## `SavePathList`

Shows the catalog's save paths when they aren't a single exact path (wildcards, or more
than one path).

- **Rows:** read-only, each with its own 👁, enabled when that row is active.
- **Picker:** one Change button, starting from the first path. Picking replaces the list
  with a single user path, so the dialog switches to `PathField`.
- **Props:** `label`, `paths: string[]`, `onReveal(index)`, `revealDisabled(index)`,
  plus the same `onPick`, `directory`, `dialogTitle`, `browseLabel`, `bridge` and `onError`.

**To confirm:** exactly one catalog path without a wildcard shows in a `PathField`. That
field shows the catalog path, with Reset hidden until the user changes it. Today any
catalog targets show as the list.

## Before building

Check in WebKitGTK (Linux) and WKWebView (macOS), and ideally WebView2 (Windows):

- Ctrl/Cmd+V fires `paste` on a focused `readOnly` input.
- Selecting and copying in the input still work under the global `body { user-select: none }`.

If either fails, rethink the approach before going further.

## Tests

- **Rewrite the `App.test.tsx` cases** that type into these fields (`fireEvent.change`) to
  use paste (`fireEvent.paste` with `clipboardData`) or the picker mock.
- **New cases:**
  - paste cleans the path and shows errors at once;
  - multi-line paste is refused;
  - Edit → type → Enter keeps the change, and Escape restores the old value;
  - ✏️ hidden on an empty field and on the executable;
  - the picker starts from `picker_start`;
  - submitting with an empty field focuses it.

## Later

Dropping a file from the file manager onto the field: Tauri reports dropped files with
their real paths, so a drop could call the same `onPick`.
