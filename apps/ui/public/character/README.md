# Skeleton character artwork

These three SVGs are the editable, static artwork for the UI's empty-state skeletons:

| SVG | Animation component |
| --- | --- |
| `no-games-found.svg` | `../../src/SleepySkeleton.tsx` |
| `no-game-selected.svg` | `../../src/PointingSkeleton.tsx` |
| `no-checkpoints.svg` | `../../src/CrouchingSkeleton.tsx` |

## Keep the moving eyes in sync

All three SVGs and their animation components **must use the common eye rig in `../../src/skeletonEyes.ts`**. Keep the required eye layer names and nesting identical to that module's `eyeLayers` and `eyeProblems` contract. Each animation must use its shared `prepareEyes`, `findEyes`, `pointerGaze`, `easeGaze`, and `gazeOffset` helpers for moving the red eyes. Character-specific eyelids, brows, timing, and reactions can differ, but changes to the shared gaze behavior must be reflected in all three animations.

When editing an SVG, keep its rig guides and pivots named and present, and update its paired component if a guide or path changes. Guides should remain visible shapes in an `opacity="0"` group so they survive an Affinity Designer export. The source SVG must still render correctly without JavaScript.

Run `pnpm test` and `pnpm build` from `apps/ui` after changing an SVG, an animation, or the shared eye code. `src/skeletonEyes.test.tsx` checks the common eye contract across all three drawings; each component also has its own rig tests.
