# Spec: checkpoint guidance by game state

## Problem

The game screen has two main buttons, **Save** and **Load**. When one or both
can't be used, a guidance panel covers them and explains why. The panel can
also carry one button of its own.

While the game runs, SaveScummer locks Save and Load (outside expert mode).
Today every running state shows the same message:

> **Exit the game** to save or load checkpoints
> Save copies progress after the game writes it on exit.
> Load replaces it for the game to read on relaunch.

This is wrong when there is nothing to save or load. A new user who has just
launched the game for the first time, and is still in its menu, is told to
exit the game. Exiting would do nothing for them: the game hasn't written any
progress, and there are no checkpoints to load.

There's a second problem once the game is closed. When the game has written
progress but no checkpoint exists yet, the panel says "No checkpoints yet — Go
and play the game first." The user has just played, and Save is ready, so the
message points them the wrong way. For games that write progress only on exit,
this is the first screen the user sees after their first session.

## What the host knows

The host already tracks the two facts needed to tell these cases apart. While
the game runs, the single "game running" guidance ignores them.

- **Progress:** the game's save location contains at least one file
  SaveScummer would back up. The check uses the same filters and excludes as
  Save. Settings, logs and crash dumps don't count. Files that are both
  settings and real progress, such as profile files holding unlocks, do count.
  The check runs again while the game runs, so a game that writes during play
  updates it live.
- **Checkpoints:** SaveScummer holds at least one usable saved checkpoint for
  this game.

What the host can't know is whether the user is in the game's menu or in the
middle of a run. Texts for the "no progress" states therefore stay
conditional: they say what happens once the game writes progress, without
claiming that it will.

Some limits on what the two facts mean:

- **No progress doesn't mean a new user.** A permadeath game such as NEO
  Scavenger deletes its save when the character dies. A returning player then
  looks exactly like a new one. Having checkpoints is the reliable sign that
  the user has used SaveScummer with this game before.
- **Progress doesn't mean a run is in progress.** Profile files that hold
  unlocks can exist with no current run. Texts for the "has progress" states
  therefore don't say "your run".

## States and texts

The first part of each headline is shown bright and the rest dimmer. A line
break in the body text means a second line.

### While the game runs

These states apply only when the running game locks Save and Load, which
happens outside expert mode. In all of them, both buttons are locked and the
panel covers both. The panel has no button: Stop is on the game card, under
expert controls. SaveScummer never offers to close the game itself, because
closing it from outside could lose progress the game hasn't written yet.

If the game writes progress during play, the message moves from R1 to R3, or
from R2 to R4, while the game is still running. For games that write only on
exit, R1 and R2 stay up until the game exits, and their wording allows for
that.

**R1. No progress, no checkpoints.** A new user, or a game that hasn't written
anything yet.
> **Play a bit** before your first checkpoint
> Quit normally when you're done. If the game saved progress,
> you can make a checkpoint here.

**R2. No progress, has checkpoints.** A returning user whose run ended (death
or wipe) or hasn't started. Load is the useful action here.
> **Quit the game** to load a checkpoint
> Your checkpoints are still here. Load one after the game closes,
> then relaunch.

**R3. Has progress, no checkpoints.** There is something to save but nothing
to load.
> **Quit the game** to make your first checkpoint
> There's progress to keep. Hit Save once the game closes.

**R4. Has progress, has checkpoints.** Both actions are available once the game closes.
> **Quit the game** to save or load
> Let it close normally first. Then make a checkpoint,
> or load one before relaunching.

### Game closed

**N1. No progress, no checkpoints.** Most likely the user's first time with
the app. The panel covers both buttons and carries **Play game**. It repeats
Run from the game card, and that's deliberate: it shows a first-time user the
next step.
> **Nothing to save yet**
> Play a bit, then quit normally. If the game saved progress,
> you can make a checkpoint here.

The body avoids the old "then save and exit", which suggested saving inside
the game.

**N2. No progress, has checkpoints.** The panel covers Save; Load is
available. No panel button: Run is on the game card, and Load sits next to
the panel.
> **Nothing to back up yet**
> Your checkpoints are still here. Hit Load to bring one back.

**N3. Has progress, no checkpoints.** The panel covers Load; Save is
available. No panel button: Save sits right next to the panel. This replaces
the old "No checkpoints yet — Go and play the game first."
> **You've got progress** worth keeping
> Hit Save to make your first checkpoint.

**N4. Has progress, has checkpoints.** No panel. Save and Load are both
available.

## Out of scope

- **The hotkey notification while the game runs** ("Save and exit {name} to
  save or load checkpoints.") stays as it is. The user pressed the hotkey on
  purpose from inside the game, so a short reminder is enough.
- **Higher-priority guidance** comes first and is unchanged: recovery after an
  interrupted operation, access needed, missing or invalid save location, and
  save location unavailable.

## Buttons per state

| State | Save | Load | Panel button |
|---|---|---|---|
| R1–R4 (game running) | locked | locked | none |
| N1: no progress, no checkpoints | off | off | **Play game** |
| N2: no progress, has checkpoints | off | available | none |
| N3: has progress, no checkpoints | available | off | none |
| N4: has progress, has checkpoints | available | available | no panel |
