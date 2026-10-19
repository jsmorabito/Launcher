---
name: dev-environment
description: Reference for the Launcher monorepo structure, SDK workspace linking, lockfile discipline, CI workflows, and release flow. Use this skill whenever working on anything related to pnpm workspace setup, the launcher-sdk dependency, lockfile errors, SDK or launcher version bumps, CI configuration, release workflow, or questions about how local dev differs from CI. Also triggers for "how does the workspace work", "how do I update the SDK version", "where do I run pnpm install", or any confusion about the monorepo layout.
---

# Launcher Development Environment

## Repository Structure

Launcher lives in a single monorepo at **`github.com/Xoshbin/asyar`** (cloned locally at `~/develop/Launcher-Project/`):

```
launcher/                       ← monorepo root (this IS the GitHub repo)
├── package.json             ← root scripts (gen:all, release:sdk) + engines
├── pnpm-workspace.yaml      ← workspace members, overrides, supported architectures
├── pnpm-lock.yaml           ← the ONLY lockfile (workspace-wide)
├── setup.mjs                ← one-command dev setup (clones bundled extensions, installs deps, builds the SDK, verifies)
├── scripts/                 ← workspace orchestration: dev.mjs, build.mjs, check.mjs, build-all.mjs
├── docs/                    ← developer documentation (Diátaxis: tutorials/, how-to/, reference/, explanation/, guide/)
├── .github/workflows/       ← CI lives at the root
│   ├── test-and-lint.yml
│   ├── release-launcher.yml
│   ├── release-sdk.yml
│   └── codeql.yml
├── launcher-app/          ← the Tauri desktop app (package name is "launcher", not "launcher-app")
│   ├── package.json         ← declares "launcher-sdk": "^4.x.x" (real NPM version)
│   ├── src-tauri/Cargo.toml ← Rust side, version-synced with package.json
│   └── scripts/             ← release.js, the two generators, CI-facing *.test.mjs guards
├── launcher-sdk/               ← the SDK (in-tree subdir, NOT a separate repo)
│   ├── cli/                 ← the `launcher` CLI, published with the SDK
│   └── scripts/release.mjs  ← SDK release helper
├── launcher-ext-builder/       ← Bun-compiled sidecar binary for the AI extension builder (Claude Agent SDK)
├── browser-companions/      ← browser extensions for the browser bridge
├── benchmarks/              ← Raycast-vs-Launcher benchmark harness
└── extensions/              ← gitignored; setup.mjs clones the dogfood extensions here
```

Workspace members are `launcher-app`, `launcher-sdk`, `extensions/*`, and
`launcher-app/src/extensions/*`.

**Key shift from the pre-2026-05-24 multi-repo setup:** `launcher-app` and `launcher-sdk` are no longer their own GitHub repos. They're subdirectories of the monorepo. The old `Xoshbin/asyar-sdk` and `Xoshbin/asyar-meta` repos are archived.

## SDK Dependency: workspace-linked everywhere

Both local dev AND CI now use the in-tree SDK. `pnpm-workspace.yaml` contains:

```yaml
overrides:
  launcher-sdk: 'workspace:*'
```

This forces every package that declares `"launcher-sdk": "^4.x.x"` to resolve to the local `launcher-sdk/` source. The launcher's `package.json` still declares a real NPM version (currently `"launcher-sdk": "^4.6.0"`) for clarity, but the override ensures the workspace copy wins. After `pnpm install`, `launcher-app/node_modules/launcher-sdk` is a symlink to `../../launcher-sdk`.

**The override lives in `pnpm-workspace.yaml`, not in the root `package.json`.** pnpm 10 moved `overrides` (along with `allowBuilds` and `supportedArchitectures`) out of `package.json`. Editing a `pnpm.overrides` block in `package.json` does nothing — there isn't one.

**No more "Wait for SDK on NPM" step in CI** — the SDK is built locally during `pnpm install` via its `prepare` hook. The old workflow that polled npm for the SDK version was deleted during the migration.

## Lockfile Discipline

**There is exactly one lockfile: `pnpm-lock.yaml` at the monorepo root.** It captures the full workspace install (launcher + SDK + sample extensions). The per-package lockfiles left over from the multi-repo era have been deleted — do not recreate one by running `pnpm install` inside a subdirectory.

### The Rule

Whenever any `package.json` in the workspace changes, regenerate the lockfile from the **monorepo root**:

```bash
cd ~/develop/Launcher-Project
pnpm install
```

Commit the updated `pnpm-lock.yaml` (root) alongside the `package.json` change.

### `ERR_PNPM_OUTDATED_LOCKFILE` in CI

CI runs `pnpm install --frozen-lockfile`. If the root lockfile is stale relative to any package.json in the workspace, the install fails. Fix: `pnpm install` at the monorepo root locally, commit the updated lockfile.

## Release Flows

The launcher and SDK release **independently** via separate tag prefixes. **Always use the release scripts — never tag manually.**

### Launcher release (`v*` tag → GitHub Releases + asyar.org updater)

```bash
# From the monorepo root:
pnpm --filter launcher-app run release <patch|minor|major|beta>

# Or from inside the launcher:
cd launcher-app
pnpm run release <patch|minor|major|beta>
```

The script reads the SDK version from the local `launcher-sdk/package.json` (no NPM
call), bumps `launcher-app/package.json` (version + declared `launcher-sdk` dep),
`src-tauri/Cargo.toml`, `Cargo.lock`, and the `scaffoldService.ts` offline
fallback, syncs the root lockfile, then creates a `release/vX.Y.Z` branch, opens
a PR, and pushes the `vX.Y.Z` tag. The tag triggers `release-launcher.yml`
(build matrix → publish → asyar.org notify). The Rust `SUPPORTED_SDK_VERSION` is
injected at compile time by `src-tauri/build.rs`, not by the release script.
Add `--dry-run` to either release command to preview without pushing. If a
release CI run fails, re-run the workflow via _Run workflow_ (workflow_dispatch)
with the same tag — no new version needed.

### SDK release (`sdk-v*` tag → npm)

```bash
# Convenience alias from the monorepo root:
pnpm release:sdk <patch|minor|major|beta>

# Or from inside the SDK:
cd launcher-sdk
pnpm run release <patch|minor|major|beta>
```

Bumps `launcher-sdk/package.json`, syncs the root lockfile, commits, tags `sdk-vX.Y.Z`, pushes. `release-sdk.yml` builds the SDK and runs `npm publish`. A GitHub Release is created (Pre-release if the tag contains a hyphen).

**Required secret:** `NPM_TOKEN` (Automation type, with publish rights for `launcher-sdk`) configured at GitHub repo Settings → Secrets and variables → Actions.

### Tag namespace summary

| Tag prefix | Belongs to | Workflow               |
| ---------- | ---------- | ---------------------- |
| `v*`       | Launcher   | `release-launcher.yml` |
| `sdk-v*`   | SDK        | `release-sdk.yml`      |

Historical SDK tags from before the migration (`v1.x.x`, `v2.x.x`) were renamed to `sdk-v1.x.x`/`sdk-v2.x.x` so they fit the new convention.

## Quick Reference

| Question                                                                                              | Answer                                                                                                                  |
| ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Where to run `pnpm install`                                                                           | Always the monorepo root                                                                                                |
| Why does the launcher's `package.json` say `"launcher-sdk": "^4.6.0"` but I see workspace-linked source? | `overrides` in `pnpm-workspace.yaml` forces `workspace:*`                                                               |
| `ERR_PNPM_OUTDATED_LOCKFILE` in CI                                                                    | Run `pnpm install` at the monorepo root, commit the root `pnpm-lock.yaml`                                               |
| How to release the launcher                                                                           | `pnpm --filter launcher-app run release patch` (or `minor`/`major`/`beta`)                                            |
| How to release the SDK                                                                                | `pnpm release:sdk patch` (or `minor`/`major`/`beta`)                                                                    |
| Where are the CI workflows                                                                            | `.github/workflows/` at the monorepo root                                                                               |
| Where are the launcher's old workflows                                                                | Removed during migration — `release-launcher.yml` replaced them; the launcher's nested `.github/workflows/` was deleted |
| Old `launcher-sdk` or `launcher-meta` GitHub repos                                                          | Archived; do not push to them. All work happens in `Xoshbin/asyar`.                                                     |
