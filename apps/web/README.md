# Website

The website has a Vite + Tailwind CSS 4 build. SG UI supplies native HTML button
and input styles; the site does not need React at runtime.

## Develop

Use Node.js 22.12+ and npm. From `apps/web`:

```sh
npm ci
npm run dev
```

Open the website at http://127.0.0.1:41988/.

## Source organization

- `index.html`: page structure, metadata, and Tailwind layout utilities.
- `src/games.js`: gallery and popup interactions, including keyboard dismissal.
- `src/artwork.js`: deferred game artwork and compact in-card store links.
- `src/reveal.js`: staggered card reveals, with reduced-motion support.
- `src/gallery-classes.js`: literal Tailwind classes for generated gallery markup.
- `src/styles.css`: Tailwind imports, brand tokens, fonts, and illustration details.
- `src/cards.css`: feature-card colors, shadows, tilt, glow, and reflection effects.
- `src/popup.css`: skeleton popup geometry, unfolding animation, and small-screen adjustments.
- `public/`: source artwork, fonts, and catalog data.
- `dist/`: generated build output, ignored by Git.

Use Tailwind utilities for layout and text. Use SG UI `btn`, size, and appearance
classes for page controls; in-card links use compact store controls.
SG UI buttons and links wrap their content in a span so
SG UI can render its decorative faces. Import package source CSS, rather than its
full precompiled stylesheet, to compile it with the site's theme. Specialized
illustration geometry remains CSS; do not recreate ordinary controls in CSS.

The gallery creates all cards immediately but defers image URLs until a single
IntersectionObserver sees a card within 400 pixels of the viewport. Collapsed
gallery clipping keeps hidden rows from loading; expansion and scrolling load
newly nearby rows. Opening a popup loads its card and popup artwork immediately.
Cards fade down into place once on entering the viewport, staggered by 80 ms in
gallery order. Keyboard focus reveals a card immediately; reduced motion skips
the animation.

## Build and verify

```sh
npm run build
npm test
node --test prepare-release.test.mjs
```

The browser tests cover the catalog, deferred artwork, staggered reveals,
SG UI buttons, footer, keyboard navigation, and skeleton popups at 320, 390, 768,
and 1440 pixels, against development and production servers. Install Chromium
once with `npx playwright install chromium` if it is not already available.

Build before testing: `npm test` tests the current `dist/`.
`npm run preview` serves that output at the same local address (stop the dev server
first). Tailwind and its Vite plugin are pinned to 4.3.3; SG UI is pinned to 0.1.2.

`python3 apps/web/update-games.py` regenerates `public/games-data.js`; then rebuild.
Do not edit or commit files in `dist/`. Commit the website source and lockfile
yourself when preparing the `website` branch for publication.

## Website releases

The Hades and Hades II popups use `public/character/popup-hug.svg`, synced from the
editable master in `apps/ui/public/character/popup-hug.svg`. After changing that
master or the shared `apps/ui/src/skeletonEyes.ts` eye rig, run
`node apps/web/sync-character.mjs`. Use `--check` to verify the source copies match,
then rebuild to copy them into `dist/`.
The controller in `public/character/popup-hug.js` paints the torso behind the panel,
anchors the hands to its edges, and respects reduced-motion preferences.

The website stays in this monorepo. Publish only output built from the committed
`apps/web` source on the local `website` branch. Pushing `main` does not deploy
the website.
Publication is manual; this setup installs no push-triggered deployment job.

When a website release is ready, update and commit the `website` branch yourself.
Then ask Codex to publish the website release using Sites.

```sh
node apps/web/prepare-release.mjs
```

This preparation step uses read-only Git commands to export a pinned commit from
`website` to a fresh temporary directory. It excludes uncommitted edits and files
outside `apps/web`. It adds the registered Site ID from the local hosting manifest
if that ID has not yet been committed on the release branch, runs `npm ci`, and
builds `dist/` in that temporary checkout, then removes the installed dependencies
from the publication checkout. Preparation requires Node.js 22.12+,
npm, and access to the dependency registry or a populated npm cache. The `website`
branch must contain the source, `package.json`, and `package-lock.json` before
preparation can succeed. It returns the source
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
