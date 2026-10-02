# Website publication

- The monorepo's `website` branch is the release source. Publish only its committed
  `apps/web` snapshot, never the working tree or `main` directly.
- Prepare the snapshot with `node apps/web/prepare-release.mjs` from the monorepo.
  Run the Sites publishing workflow only in the separate returned checkout.
- Do not stage, commit, switch branches, or otherwise manage the monorepo's Git
  state. The user maintains the `website` branch. Publication may commit and push
  the separate Sites checkout when the user requests a website release.
- Reuse the Site ID in `.openai/hosting.json`. Preserve the selected audience and
  custom domains. Never store source credentials in files or command arguments.
- Publication is manual. Pushing either monorepo branch alone does not deploy.
