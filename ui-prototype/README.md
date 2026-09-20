# Save Scummer UI prototype

Open `index.html` directly in a modern browser to use the interactive prototype.
It contains the preview runtime, icons and styles; no build step or server is required.

`save-scummer-main.html` is the editable HTML/CSS/JavaScript fragment used to generate
the standalone preview. It is the design reference for the Qt UI described in
[`PLAN.md`](../PLAN.md). After editing the fragment, regenerate `index.html` with
the Visualize skill's `scripts/render.py` export helper.

Select a game row to show its controls and instructions. Expand **Other games**
to view the rest of the library, including the detected-but-never-run example.
Save, Load and menu actions are simulated and do not modify game files.
