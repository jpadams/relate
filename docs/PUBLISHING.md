# Publishing checklist (USER-ACTION-REQUIRED)

> **This is an operational runbook, not an automated step.** Every item below is a
> deliberate, user-driven action (making a repository public, pushing tags,
> enabling Pages, uploading release assets). Nothing here should be automated by a
> tool or agent. Work top to bottom — later steps assume earlier ones succeeded.

The repository ships with the machinery for a checksum-verified, fail-closed
release already in place (the `release.yml` workflow).
The WASM and Swift surfaces were withdrawn in ADR-0065; WASM returned as npm
packages (ADR-0084) and Swift as a SwiftPM package whose static XCFramework the
release attaches (ADR-0433). The docs site no longer has an interactive
playground. What remains is operational: point
everything at the public `relatedb/relate` repository and turn it on.

## 0. Preconditions

- [ ] `main` is green: `make lint` and `make test` pass; the spec-harness lane (ADR-0482) reports language coverage on the release PR — reporting-only, never a gate.
- [ ] The repository's existing `v*` tags came across with the transfer — the
  release workflow computes the next version from them plus the chosen bump
  (ADR-0067; see `RELEASING.md`). Verify with `git tag --list 'v*'`.

## 1. Make the repository public at `relatedb/relate`

The release workflow is **fail-closed to `relatedb/relate`**: the
`release-owner` job (`.github/workflows/release.yml`) refuses to publish
official artifacts from any other repository
(`github.repository != 'relatedb/relate'`). So the canonical home
**must** be `relatedb/relate`.

- [ ] Ensure the repository lives at `github.com/relatedb/relate` (transfer
  it there if it is currently under a personal account).
- [ ] Make it **public**.
- [ ] Confirm the `origin` remote used for pushing tags points at
  `relatedb/relate` (a fork's tags cannot publish official artifacts).

## 2. Cut the first public release

- [ ] Dispatch the Release workflow (manual dispatch only — it computes the
  version from the existing `v*` tags, builds, tags, and publishes; there are
  no hand-pushed tags, ADR-0067):
  `gh workflow run release.yml -f bump=patch`

## 3. Verify the release workflow and its assets

The workflow builds and uploads, then creates the GitHub Release. Confirm each
artifact landed on that first release:

- [ ] CLI binaries (default) for Linux amd64/arm64, macOS amd64/arm64, Windows amd64.
- [ ] `SHA256SUMS` covering all CLI binaries (the checksum manifest the README
  install section links to), with its first line
  `# relate-release-version: vX.Y.Z` naming the release tag (ADR-0457).
- [ ] `SHA256SUMS.sig` over those exact bytes (ADR-0115).
- [ ] Python wheels (and sdist).
- [ ] PyPI: after trusted publishing is configured for environment `pypi`, the
  `pypi` job uploads wheels as **`relate-db`**. Register that project
  (not `relate`, which is already taken) and enable trusted publishing
  for `relatedb/relate` before the first tag. The job is fail-closed
  until that environment exists.
- [ ] npm (ADR-0278): the `@relate` npm scope exists and the releaser has
  publish rights.
- [ ] Licensing: the project's licensing decision is resolved and the
  `license` fields of all seven npm packages (`@relate/relate`,
  `@relate/relate-wasm`, and the five platform packages) match the
  decision in force at publish time (Apache-2.0 per ADR-0055 today).
  Re-enabling npm publishing means removing the `if: false` guards from the
  `npm-publish` and `npm-publish-main` jobs in `release.yml` — do both in
  the same change.
- [ ] npm: the first publish of `@relate/relate` and
  `@relate/relate-wasm` is done manually — npm trusted publishing
  needs the packages to exist before it can be configured.
- [ ] Swift (ADR-0433): `RelateFFI.xcframework.zip` is attached to the
  release, and the tagged `Package.swift` carries `releaseTag` = the tag and
  `releaseChecksum` = `swift package compute-checksum` of that zip (the
  `version` job stamps both into the release commit). A smoke: a fresh
  package depending on `.package(url: …, exact: "<version>")` resolves and
  builds.
- [ ] npm: trusted publishing is configured on npmjs.com for
  `relatedb/relate`, workflow `release.yml`, for all seven packages
  (main, wasm, and the five platform packages). The release workflow's npm
  jobs are fail-closed until this is done.
- [ ] Slack (optional): set the `SLACK_WEBHOOK_URL` repository secret to
  enable the release announcement — the release run's final job
  (`slack-announce`) posts it after the website deploy and the self-update
  canary (ADR-0465); without the secret that job logs a skip and the
  release stays green.

  If a publish job dies mid-loop, re-running it hits E403 on the
  already-published tarballs — publish the remaining tarballs manually
  instead.

The release payload is the CLI binaries and their checksums, the Python wheels,
the FFI archives, the npm tarballs, and the Swift XCFramework zip. The
XCFramework is not code-signed yet (no Apple Developer identity in the release
secrets; ADR-0433).

## 4. Enable GitHub Pages

The homepage is a static page on the `gh-pages` branch — there is no
build step for it. The release workflow publishes `distribution/install.sh` and
`releases.json` (ADR-0250) to the gh-pages root; `update-website.yml` keeps the
page content in sync with each release. Those jobs run concurrently, so each
pushes through `.github/scripts/push-gh-pages.sh`, which rebases onto the
others' commits. `.github/workflows/deploy-pages.yml` assembles the live site
from that branch plus the playground (built from `main`) and the cookbook
(built from `docs/cookbook/` at the latest release tag, ADR-0130), and
deploys it with `actions/deploy-pages`. The release run calls it after every
gh-pages write (ADR-0330); a commit to `gh-pages` is not live until it runs.

- [ ] Enable GitHub Pages for `relatedb/relate` (source: **GitHub
  Actions**, so `deploy-pages.yml` publishes).
- [ ] Point `https://relatedb.ai/` at the site (a custom domain on the
  Pages deployment today; the separate `relatedb/relate-web` repository
  per ADR-0517 §3 once that handoff lands) and confirm the site loads
  at `https://relatedb.ai/`.
- [ ] Confirm the install script is served at
  `https://relatedb.ai/install.sh`.

## 5. Confirm the absolute URLs resolve

Several `README.md` and docs links use the public absolute forms
(`https://relatedb.ai/...` and
`https://github.com/relatedb/relate/releases/...`). These resolve **only
after** the repo is public (steps 1–3) and Pages is live (step 4). Spot-check:

- [ ] The README install-table download links (`.../releases/latest/download/...`).
- [ ] The `SHA256SUMS` link and the checksum-verification flow in the README.
- [ ] The install script at
  `https://relatedb.ai/install.sh`.
- [ ] The docs links from the README (Python/Rust library references,
  architecture, and the new [Limitations](limitations.md)
  page).
- [ ] The ~20 absolute `https://github.com/relatedb/relate/blob/main/...`
  links in the docs. Link checkers skip them while the host is not yet
  public, and they are pinned to `blob/main` paths, so a renamed target would
  silently 404. Once the repo is public, open each one (or run a link check
  without the not-yet-public host exclusion) and confirm every target
  resolves.
- [ ] Turn on the cookbook's GitHub links (ADR-0133): delete the not-yet-public
  entries from `lychee.toml`'s `exclude` list on `main` (the Pages deploy
  hands `main`'s list to the cookbook build, so no release is needed),
  redeploy Pages, and confirm the cookbook pages' source and companion links
  resolve. (The navigation's GitHub link is already on, ADR-0288.)

Once every box is checked, the first public release is fully published:
binaries and checksums downloadable, and the documentation site live.
