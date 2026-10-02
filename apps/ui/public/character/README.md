# Skeleton character artwork

These three SVGs are the editable, static artwork for the UI's empty-state skeletons:

| SVG | Animation component |
| --- | --- |
| `no-games-found.svg` | `../../src/SleepySkeleton.tsx` |
| `no-game-selected.svg` | `../../src/PointingSkeleton.tsx` |
| `no-checkpoints.svg` | `../../src/CrouchingSkeleton.tsx` |

`popup-hug.svg` is the editable master for the web version's Hades and Hades II popup character. Its eyes follow the same shared rig and are included in the contract tests. Run `node apps/web/sync-character.mjs` from the repository root after editing it or the shared eye code to update the self-contained website release assets.

## Keep the moving eyes in sync

All three SVGs and their animation components **must use the common eye rig in `../../src/skeletonEyes.ts`**. Keep the required eye layer names and nesting identical to that module's `eyeLayers` and `eyeProblems` contract. Each animation must use its shared `prepareEyes`, `findEyes`, `pointerGaze`, `easeGaze`, and `gazeOffset` helpers for moving the red eyes. Character-specific eyelids, brows, timing, and reactions can differ, but changes to the shared gaze behavior must be reflected in all three animations.

When editing an SVG, keep its rig guides and pivots named and present, and update its paired component if a guide or path changes. Guides should remain visible shapes in an `opacity="0"` group so they survive an Affinity Designer export. The source SVG must still render correctly without JavaScript.

Run `pnpm test` and `pnpm build` from `apps/ui` after changing an SVG, an animation, or the shared eye code. `src/skeletonEyes.test.tsx` checks the common eye contract across all four drawings; each existing component also has its own rig tests.

## Panel-hugging artwork

Open `popup-hug.svg` in a vector editor. It contains only vector shapes, with a transparent background and the source image's relative placement. Left/right mean the viewer's left/right. The cropped viewBox is `120 230 1010 590`; geometry retains the source image's coordinate system.

| Moving group | Pivot / attachment | Contents |
| --- | --- | --- |
| `torso` | `pivot-torso`, `anchor-panel-left` | Separate ribs, collarbone, neck |
| `head-pose` inside `head` | `pivot-head` | Skull, brows, nose, both complete eyes |
| `left-arm` | `pivot-left-shoulder` | Upper arm and the complete child forearm chain |
| `left-forearm` | `pivot-left-elbow` | Forearm and child hand |
| `left-hand` | `pivot-left-wrist`, `anchor-grip-left` | Three independently rotating fingers |
| `right-hand` | `anchor-grip-right` | Independent placement at the opposite panel edge |
| `right-hand-pose` | `pivot-right-wrist` | Local hand rotation and three fingers |
| `left-finger-{inner,middle,outer}` / `right-finger-{inner,middle,outer}` | `pivot-` + group name | One complete, rounded finger bone per group; `tip-` + group name marks its end |

Rotate the outer `left-arm` group at the shoulder so its forearm and hand follow; rotate `left-forearm` at the elbow for the next joint. `left-upper-arm` contains just the upper-arm artwork. Complete overlapping bone silhouettes keep the joints covered through modest hugging gestures. These are rigid illustrated bones, not a deformable mesh; large joint rotations need pose-specific adjustment. The unseen right arm and lower body are intentionally absent from this cropped pose.

Keep the torso behind the UI panel and the head, left arm and right hand in front. The torso ends at the source panel's left edge, x=379. Align `anchor-panel-left` to that edge; the left grip anchor is `(379,734)` and the right grip anchor is `(1033,390)`. Move the independent right hand when panel width changes. For panel height changes, use the left arm joints to place its grip, and reposition the whole character as needed. Preserve aspect ratio; do not stretch the skull or bones to fit.

Read pivots/anchors in the target group's coordinate space through `getScreenCTM()` before applying animation, as `findEyes` does. Keep the lowercase layer names unchanged. All helper circles and ellipses live in named `rig-*` groups at **0% opacity**. Raise opacity to inspect them, then return it to zero; hiding the layers causes Affinity to omit them on export. An Affinity round trip has not been verified for this new drawing.

The eyes retain the drawn red crescents in `iris-static-*`. At integration time, call `prepareEyes(svg, { rimWidth: 0, lidDepth: 4 })`, then the shared `findEyes`, `pointerGaze`, `easeGaze` and `gazeOffset` helpers. The rig replaces those crescents with clipped circular irises; gaze bounds and neutral points are editable helper shapes in `rig-head`. Prefix IDs and update all fragment references when inserting multiple instances into one page. The SVG itself has no scripts or external dependencies.
