# Releasing

A release is a version tag. Pushing `vX.Y.Z` makes GitHub Actions build the Windows installer and draft a GitHub Release; you read the draft and publish it.

## Cut a release

1. Make sure `master` is green.
2. On a branch, set the version everywhere and review the diff:
   ```
   node scripts/version.mjs set 0.2.0
   git diff
   ```
   (It rewrites `Cargo.toml`, `app/src-tauri/tauri.conf.json`, `app/package.json`, `app/package-lock.json` and `Cargo.lock`.) Run `cargo build` once so nothing else in the lockfile moves, then commit as "Release 0.2.0".
3. Open a PR, merge it.
4. Tag the merge commit and push the tag:
   ```
   git checkout master && git pull
   git tag v0.2.0
   git push origin v0.2.0
   ```
5. Watch the **Release** run in GitHub Actions. It runs the full CI checks, refuses to continue if the tag and the files disagree, builds the installer and creates a **draft** release with the installer and its `.sha256`.
6. Open the draft, edit the generated notes, and publish.

## Try a release first

Use a pre-release tag such as `v0.2.0-rc1` (set the version to `0.2.0-rc1` first). Any tag containing `-` is published as a pre-release. Install it on a machine or Windows profile that has never had the app, download Stockfish from Settings, and review a game. Then set the version to the real one and tag again.

## When it goes wrong

- *"... says 0.1.0, expected 0.2.0"*: the files and the tag disagree. Delete the tag (`git push origin :refs/tags/v0.2.0` and `git tag -d v0.2.0`), fix the version, tag again.
- *Installer built but the draft was not created*: re-run the failed job; the build is repeatable. If a draft exists for that tag, delete the draft first.
- *Windows SmartScreen warning*: expected until the installer is code-signed (not done yet).

## Updating the Stockfish version

The in-app download is pinned in `crates/core/src/engine_install.rs` (tag, asset name and SHA-256). To move to a newer Stockfish, change those three constants, then run `cargo test -p chess-analyzer-core --test stockfish installs_the_real_release -- --ignored` to prove the new release downloads, verifies and starts. Update the button label in `SettingsScreen.tsx` and the setup scripts' default version to match.
