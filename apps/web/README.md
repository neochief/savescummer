# Website releases

The Hades and Hades II popups use `dist/character/popup-hug.svg`, synced from the
editable master in `apps/ui/public/character/popup-hug.svg`. After changing that
master or the shared `apps/ui/src/skeletonEyes.ts` eye rig, run
`node apps/web/sync-character.mjs`. Use `--check` to verify the release copies match.
The controller in `dist/character/popup-hug.js` paints the torso behind the panel,
anchors the hands to its edges, and respects reduced-motion preferences.

The website stays in this monorepo. Publish only the committed `apps/web` snapshot
from the local `website` branch. Pushing `main` does not deploy the website.
Publication is manual; this setup installs no push-triggered deployment job.

When a website release is ready, update and commit the `website` branch yourself.
Then ask Codex to publish the website release using Sites.

```sh
node apps/web/prepare-release.mjs
```

This preparation step uses read-only Git commands to export a pinned commit from
`website` to a fresh temporary directory. It excludes uncommitted edits and files
outside `apps/web`. It adds the registered Site ID from the local hosting manifest
if that ID has not yet been committed on the release branch. It returns the source
commit, checkout path, and deployment archive path, and records them in
`release.json` beside the checkout.

Continue publication in that separate checkout using the installed Sites hosting
workflow. Obtain a fresh Sites source credential, pass it through the workflow's
hidden standard input, and use the returned source commit and archive to save and
deploy a Sites version. Credentials must never be stored in this repository.
Sites uses its own source repository and configured branch; `website` is the
release selection branch in this monorepo.

The deployment workflow may commit and push only its separate Sites checkout.
Do not stage, commit, switch branches, or otherwise manage Git state in this
monorepo when publishing. Keep the existing Site ID in `.openai/hosting.json` and
reuse it for every release.
