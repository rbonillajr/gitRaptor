---
title: Getting started with GitRaptor (early access)
type: guide
status: proposed
created: 2026-10-28
updated: 2026-10-28
domain: GRP
related: [ADR-GRP-014, INF-GRP-004]
---

# Getting started with GitRaptor (early access)

> **Alpha, macOS only, unsigned.** The binaries are not signed nor notarized. Spanish version: [primeros-pasos.md](primeros-pasos.md).

This guide takes you from zero to GitRaptor protecting a repo and connected to Claude Code. About 10 minutes.

## 1. Install

Pick the version of the [latest alpha](https://github.com/rbonillajr/gitRaptor/releases) (`v0.1.0-alpha.N`) and run:

```sh
curl -fsSL https://github.com/rbonillajr/gitRaptor/releases/download/v0.1.0-alpha.N/install.sh | sh
```

The script downloads the archive for your Mac (Apple silicon or Intel), **verifies the SHA256 before extracting** and installs `raptor`, `raptor-mcp` and `raptor-hook` into `~/.local/bin`. It does not touch your `PATH`: if `~/.local/bin` is not on it, add it to your shell profile.

- To install into `/usr/local/bin`: `sh install.sh --system` (uses `sudo` only to copy the 3 files).
- **macOS quarantine.** If you downloaded the archive with a browser, macOS flags it and Gatekeeper would block it. If the script finds that flag it removes it from the 3 binaries and **tells you** (the build is not notarized; you are trusting the SHA256). If there is no flag, it does nothing.

Check the install:

```sh
raptor --version
```

## 2. Observe a repo

```sh
cd ~/path/to/your/repo
raptor repo add
```

With no argument it uses the current folder; it accepts the root of any of its worktrees. `raptor repo discovered` lists the repos GitRaptor found in your code folders that nobody observes yet.

## 3. Protect the repo (Guardrails)

```sh
raptor guard install
```

It explains what it installs and **asks your permission**. After that, force-push and deleting the base branch are denied, even with plain Git. `raptor guard status` shows the state and `raptor guard uninstall` removes it and leaves the repo as it was.

## 4. Open the TUI

```sh
raptor
```

It shows live agents and worktrees, predicted conflicts and the timeline. Quit with `q`.

## 5. Connect Claude Code

```sh
raptor mcp install            # registers the MCP server in Claude Code (user scope)
raptor mcp enable             # in the repo folder: lets agents use it
raptor mcp list               # repos agents may use
```

Restart Claude Code. `raptor mcp enable` is reserved to the developer: an agent cannot run it on its own.

## 6. If something goes wrong

GitRaptor saves a snapshot before every operation that changes your repo. **Do not try to fix it by hand before looking here.**

```sh
raptor undo                   # undoes the last operation of the worktree you are in
raptor timeline               # what changed, when and who (human or agent)
raptor restore <id>           # returns the worktree to a point of the timeline
```

The state right before `undo` or `restore` is saved too, so you can go back. Note: `undo` acts locally; if you already pushed, the remote is not touched.

To report a problem, open an issue with the output of `raptor --version` and `raptor status`.

## 7. Uninstall

```sh
raptor guard uninstall        # in each protected repo
raptor mcp uninstall          # removes the MCP server from Claude Code
raptor daemon disable         # if you enabled start at login
raptor daemon stop            # stops the engine
sh install.sh --uninstall     # removes only raptor, raptor-mcp and raptor-hook
```

`install.sh --uninstall` **does not delete your data** (on macOS, `~/Library/Application Support/gitraptor`: profile and timeline). Delete it only if you accept losing the history; otherwise a reinstall picks it up again. If you installed into a non-default folder, add `--install-dir <path>`.
