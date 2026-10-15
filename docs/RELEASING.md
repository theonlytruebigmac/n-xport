# Releasing a New Version

Releases are built automatically by the GitHub Actions workflow in
[`.github/workflows/release.yml`](../.github/workflows/release.yml) when a tag
matching `v*` is pushed. Use the bundled `scripts/bump-version.sh` helper so that
`package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` always
carry the same version. The app and installer take their version from
`tauri.conf.json`, but the command-line tool and the Rust crate report the one in
`Cargo.toml`, so a stale file shows up as a wrong version (the CI "Version files
in sync" step fails if they disagree).

### Before you tag

- **The signing secrets exist** in the repository's GitHub Actions secrets:
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The
  updater only accepts artifacts signed with the matching key.
- **CI is green on `main`** (worth a look, though the release enforces it too).

The release workflow runs five stages, and nothing is built unless the first two
pass:

1. **Verify version** checks that the three version files agree and that the tag
   is exactly `v` plus the app version (tagging `v1.2.1` while the files say
   `1.2.0` is rejected).
2. **Checks** runs the same workflow CI runs on every push (formatting, clippy,
   Rust and frontend tests, frontend build).
3. **Create draft release** makes the GitHub release as a draft, so nothing is
   visible to users or to the updater yet.
4. **Build** produces macOS (Apple Silicon and Intel), Linux, and Windows
   installers in parallel, all on GitHub-hosted runners (Linux on
   `ubuntu-22.04`), and uploads them to the draft. No self-hosted runner is
   needed.
5. **Publish release** runs only if every build succeeded. It first checks that
   `latest.json` has entries for macOS (both architectures), Linux, and Windows,
   then publishes the draft.

### Steps

1. Make sure your working tree is clean and on `main` with the latest changes:

   ```bash
   git checkout main
   git pull --ff-only
   git status   # should be clean
   ```

2. Bump the version. Pick one of:

   ```bash
   ./scripts/bump-version.sh patch    # 1.2.0 -> 1.2.1
   ./scripts/bump-version.sh minor    # 1.2.0 -> 1.3.0
   ./scripts/bump-version.sh major    # 1.2.0 -> 2.0.0
   ./scripts/bump-version.sh 1.2.3    # explicit version
   ```

   The script updates all three version files, refreshes `Cargo.lock` so it carries
   the new crate version, checks that each file took the new version (it stops with
   an error if not), and prompts:
   `Create git commit and tag vX.Y.Z? [y/N]`. Answer **`y`** — it will
   create a commit (`Bump version to X.Y.Z`) and a tag (`vX.Y.Z`).
3. Push the commit **and** the tag in the same step so the workflow kicks off:

   ```bash
   git push && git push --tags
   ```

   (or `git push origin main --follow-tags`)
4. Watch the build at
   [Actions → Release](https://github.com/theonlytruebigmac/n-xport/actions/workflows/release.yml).
   The workflow builds macOS (Apple Silicon and Intel), Linux, and Windows
   artifacts and signs them for the Tauri updater. The release stays a **draft**
   until every build has finished and `latest.json` is confirmed complete, then
   it is published automatically. Its body is a fixed one-line message.
5. Open the [Releases](https://github.com/theonlytruebigmac/n-xport/releases)
   page and edit the published release to add release notes. Installed copies
   of the app read `latest.json` from the latest release, so they will offer
   this version as an update as soon as it is published (not while it is a
   draft).

### If something goes wrong

- **Workflow didn't start.** The trigger is the tag push, not the commit.
  Run `git push --tags` (or re-push a specific tag:
  `git push origin vX.Y.Z`).
- **Verify or Checks failed.** Nothing was built or published. Fix the problem,
  then move the tag: delete it locally and on the remote (below), commit the fix
  and re-run the version bump or re-tag.
- **One platform's build failed.** Nothing is published: the release stays a
  draft with the files uploaded so far. Fix the cause (if it is a flaky runner,
  just retry) and use "Re-run failed jobs" on the Actions page; the publish step
  runs once all builds pass. If you abandon the release, delete the draft on the
  Releases page.
- **Publish failed on the `latest.json` check.** A build succeeded but its entry
  is missing from the manifest (parallel builds each merge into it). Re-run the
  failed build job, then the publish job.
- **A bad release is already published.** Installed apps may already see it as
  the latest update. Delete the release
  on GitHub (and fix forward with a new version) so `latest.json` points at the
  previous good release again. Copies that already updated stay on the bad
  version until the fixed one is released.
- **Wrong version tagged.** Delete the tag locally and on the remote
  before re-bumping:

  ```bash
  git tag -d vX.Y.Z
  git push origin :refs/tags/vX.Y.Z
  ```

- **Version files out of sync.** Re-run `scripts/bump-version.sh <explicit-version>`
  with the version you intended; it rewrites all three files from whatever
  each currently says. `./scripts/bump-version.sh --check` reports whether they agree
  (CI runs it on every push and pull request).
