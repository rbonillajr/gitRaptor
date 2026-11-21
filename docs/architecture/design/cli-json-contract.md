---
id: DOC-CLI-JSON-CONTRACT
title: "Contrato JSON de la CLI raptor para agentes"
type: reference
status: generated
---

# Contrato JSON de la CLI `raptor` para agentes

> Generado por la prueba de `crates/api/src/cli_contract.rs` desde `crates/api/schemas/cli-contract.json`. No lo edites a mano: regenéralo con `GITRAPTOR_BLESS_CLI_CONTRACT=1 cargo test -p gitraptor-api cli_contract`.

Versión del contrato: `1.0`. Toda salida `--json` es una línea con `{"schema_version", "data"}` o `{"schema_version", "error"}`; `error` lleva `code`, `message`, `params` y `action`. Un cambio incompatible sube el mayor; añadir sube el menor. Las salidas del desarrollador llevan el sobre pero su `data` no tiene esquema congelado.


## status@agent

- Subcomando: `raptor status --json`
- Perfil: mcp (agente)
- Versión: `1.0`
- Códigos de error posibles: `engine-unavailable` (salida 1), `repo-not-enabled` (salida 1), `not-in-observed-worktree` (salida 1), `repo-unavailable` (salida 1), `identity-unverified` (salida 1), `rate-limited` (salida 1), `confirmation-required` (salida 1), `aborted` (salida 1), `internal` (salida 1), `command-unavailable` (salida 1), `reserved-command` (salida 1), `command-failed` (salida 1), `invalid-args` (salida 2)

| Campo | Tipo | Requerido | Valores |
|---|---|---|---|
| `action` | null \| string | no | "register-to-write" |
| `branch` | null \| object | no |  |
| `branch.lossy` | boolean | no |  |
| `branch.truncated` | boolean | no |  |
| `branch.untrusted` | string | no |  |
| `here` | null \| object | no |  |
| `here.ahead` | integer \| null | no |  |
| `here.at_least` | boolean | no |  |
| `here.behind` | integer \| null | no |  |
| `here.changes` | null \| object | no |  |
| `here.changes.cursor` | string | no |  |
| `here.changes.total` | integer | no |  |
| `here.sessions` | array | no |  |
| `here.sessions[]` | object | sí |  |
| `here.sessions[].actor` | object | sí |  |
| `here.sessions[].actor.actor` | string | sí | "agent", "unattributed" |
| `here.sessions[].actor.kind` | string | no | "claude-code", "other" |
| `here.sessions[].actor.name` | null \| object | no |  |
| `here.sessions[].actor.name.lossy` | boolean | no |  |
| `here.sessions[].actor.name.truncated` | boolean | no |  |
| `here.sessions[].actor.name.untrusted` | string | no |  |
| `here.sessions[].actor.origin` | string | no | "detected", "registered" |
| `here.sessions[].state` | string | sí | "active", "ended", "inactive" |
| `here.sessions_total` | integer \| null | no |  |
| `here.uncounted` | null \| string | no | "base-missing", "no-base", "no-commits", "unreadable" |
| `main` | boolean | sí |  |
| `page` | null \| object | no |  |
| `page.cursor` | null \| string | no |  |
| `page.of` | null \| object | no |  |
| `page.of.lossy` | boolean | no |  |
| `page.of.truncated` | boolean | no |  |
| `page.of.untrusted` | string | no |  |
| `page.paths` | array | no |  |
| `page.paths[]` | object | sí |  |
| `page.paths[].lossy` | boolean | no |  |
| `page.paths[].truncated` | boolean | no |  |
| `page.paths[].untrusted` | string | sí |  |
| `page.total` | integer | no |  |
| `page.truncated` | boolean | no |  |
| `page.worktrees` | array | no |  |
| `page.worktrees[]` | object | sí |  |
| `page.worktrees[].ahead` | integer \| null | no |  |
| `page.worktrees[].at_least` | boolean | no |  |
| `page.worktrees[].behind` | integer \| null | no |  |
| `page.worktrees[].branch` | null \| object | no |  |
| `page.worktrees[].branch.lossy` | boolean | no |  |
| `page.worktrees[].branch.truncated` | boolean | no |  |
| `page.worktrees[].branch.untrusted` | string | no |  |
| `page.worktrees[].changes` | null \| object | no |  |
| `page.worktrees[].changes.cursor` | string | no |  |
| `page.worktrees[].changes.total` | integer | no |  |
| `page.worktrees[].main` | boolean | no |  |
| `page.worktrees[].name` | object | sí |  |
| `page.worktrees[].name.lossy` | boolean | no |  |
| `page.worktrees[].name.truncated` | boolean | no |  |
| `page.worktrees[].name.untrusted` | string | sí |  |
| `page.worktrees[].sessions` | array | no |  |
| `page.worktrees[].sessions[]` | object | sí |  |
| `page.worktrees[].sessions[].actor` | object | sí |  |
| `page.worktrees[].sessions[].actor.actor` | string | sí | "agent", "unattributed" |
| `page.worktrees[].sessions[].actor.kind` | string | no | "claude-code", "other" |
| `page.worktrees[].sessions[].actor.name` | null \| object | no |  |
| `page.worktrees[].sessions[].actor.name.lossy` | boolean | no |  |
| `page.worktrees[].sessions[].actor.name.truncated` | boolean | no |  |
| `page.worktrees[].sessions[].actor.name.untrusted` | string | no |  |
| `page.worktrees[].sessions[].actor.origin` | string | no | "detected", "registered" |
| `page.worktrees[].sessions[].state` | string | sí | "active", "ended", "inactive" |
| `page.worktrees[].sessions_total` | integer \| null | no |  |
| `page.worktrees[].unavailable` | null \| string | no | "missing", "unreadable", "untrusted" |
| `page.worktrees[].uncounted` | null \| string | no | "base-missing", "no-base", "no-commits", "unreadable" |
| `repo` | null \| object | no |  |
| `repo.base` | object | no |  |
| `repo.base.name` | null \| object | no |  |
| `repo.base.name.lossy` | boolean | no |  |
| `repo.base.name.truncated` | boolean | no |  |
| `repo.base.name.untrusted` | string | no |  |
| `repo.base.state` | string | no | "confirmed", "invalid", "pending", "unconfirmed" |
| `repo.diagnostics` | array | no |  |
| `repo.diagnostics[]` | string | sí | "base-change-pending", "base-change-pending-invalid", "base-unconfirmed", "config-unreadable", "floor-relax-pending", "leftover-folder", "template-outdated" |
| `repo.engine` | null \| string | no | "dormant", "reconciling", "waiting-for-git" |
| `repo.fetch_age_s` | integer \| null | no |  |
| `repo.gaps` | array | no |  |
| `repo.gaps[]` | object | sí |  |
| `repo.gaps[].from_s_ago` | integer | sí |  |
| `repo.gaps[].to_s_ago` | integer \| null | no |  |
| `repo.gaps_total` | integer \| null | no |  |
| `repo.protection` | string | no | "full", "mcp-only" |
| `repo.protection_lost` | null \| string | no | "binary-missing", "dispatcher-altered", "dispatcher-missing", "dispatcher-not-executable", "folder-missing", "hookspath-changed", "repo-moved" |
| `repo.sessions_unknown` | boolean | no |  |
| `repo.worktrees` | null \| object | no |  |
| `repo.worktrees.cursor` | string | no |  |
| `repo.worktrees.total` | integer | no |  |
| `repo_id` | string | sí |  |
| `repo_state` | string | sí | "observed", "unavailable" |
| `requester` | object | sí |  |
| `requester.actor` | string | sí | "agent", "unattributed" |
| `requester.kind` | string | no | "claude-code", "other" |
| `requester.name` | null \| object | no |  |
| `requester.name.lossy` | boolean | no |  |
| `requester.name.truncated` | boolean | no |  |
| `requester.name.untrusted` | string | no |  |
| `requester.origin` | string | no | "detected", "registered" |
| `worktree` | object | sí |  |
| `worktree.lossy` | boolean | no |  |
| `worktree.truncated` | boolean | no |  |
| `worktree.untrusted` | string | sí |  |

## undo@agent

- Subcomando: `raptor undo --json`
- Perfil: mcp (agente)
- Versión: `1.0`
- Códigos de error posibles: `engine-unavailable` (salida 1), `repo-not-enabled` (salida 1), `not-in-observed-worktree` (salida 1), `repo-unavailable` (salida 1), `identity-unverified` (salida 1), `rate-limited` (salida 1), `confirmation-required` (salida 1), `aborted` (salida 1), `internal` (salida 1), `command-unavailable` (salida 1), `reserved-command` (salida 1), `command-failed` (salida 1), `invalid-args` (salida 2)

| Campo | Tipo | Requerido | Valores |
|---|---|---|---|
| `actor` | object | sí |  |
| `actor.actor` | string | sí | "agent", "unattributed" |
| `actor.kind` | string | no | "claude-code", "other" |
| `actor.name` | null \| object | no |  |
| `actor.name.lossy` | boolean | no |  |
| `actor.name.truncated` | boolean | no |  |
| `actor.name.untrusted` | string | no |  |
| `actor.origin` | string | no | "detected", "registered" |
| `kept` | integer | no |  |
| `not_restored` | integer | sí |  |
| `operation_id` | string | sí |  |
| `prior_snapshot_id` | string | sí |  |
| `undone_count` | integer | no |  |
| `undone_operation_id` | string | sí |  |

## redo@agent

- Subcomando: `raptor redo --json`
- Perfil: mcp (agente)
- Versión: `1.0`
- Códigos de error posibles: `engine-unavailable` (salida 1), `repo-not-enabled` (salida 1), `not-in-observed-worktree` (salida 1), `repo-unavailable` (salida 1), `identity-unverified` (salida 1), `rate-limited` (salida 1), `confirmation-required` (salida 1), `aborted` (salida 1), `internal` (salida 1), `command-unavailable` (salida 1), `reserved-command` (salida 1), `command-failed` (salida 1), `invalid-args` (salida 2)

| Campo | Tipo | Requerido | Valores |
|---|---|---|---|
| `actor` | object | sí |  |
| `actor.actor` | string | sí | "agent", "unattributed" |
| `actor.kind` | string | no | "claude-code", "other" |
| `actor.name` | null \| object | no |  |
| `actor.name.lossy` | boolean | no |  |
| `actor.name.truncated` | boolean | no |  |
| `actor.name.untrusted` | string | no |  |
| `actor.origin` | string | no | "detected", "registered" |
| `kept` | integer | no |  |
| `not_restored` | integer | sí |  |
| `operation_id` | string | sí |  |
| `prior_snapshot_id` | string | sí |  |
| `redone_undo_id` | string | sí |  |
| `removed` | integer | sí |  |
| `written` | integer | sí |  |

## agent-register

- Subcomando: `raptor agent register <agente> --json`
- Perfil: mcp (agente) y desarrollador
- Versión: `1.0`
- Códigos de error posibles: `engine-unavailable` (salida 1), `repo-not-enabled` (salida 1), `not-in-observed-worktree` (salida 1), `repo-unavailable` (salida 1), `identity-unverified` (salida 1), `rate-limited` (salida 1), `confirmation-required` (salida 1), `aborted` (salida 1), `internal` (salida 1), `command-unavailable` (salida 1), `reserved-command` (salida 1), `command-failed` (salida 1), `invalid-args` (salida 2)

| Campo | Tipo | Requerido | Valores |
|---|---|---|---|
| `agent` | string | sí |  |
| `agent_name` | null \| object | no |  |
| `agent_name.lossy` | boolean | no |  |
| `agent_name.truncated` | boolean | no |  |
| `agent_name.untrusted` | string | no |  |
| `origin` | string | sí |  |
| `outcome` | string | sí |  |
| `repo_id` | string | sí |  |
| `session_id` | string | sí |  |
| `support` | string | sí |  |

## daemon-status@agent

- Subcomando: `raptor daemon status --json`
- Perfil: mcp (agente)
- Versión: `1.0`
- Códigos de error posibles: `engine-unavailable` (salida 1), `repo-not-enabled` (salida 1), `not-in-observed-worktree` (salida 1), `repo-unavailable` (salida 1), `identity-unverified` (salida 1), `rate-limited` (salida 1), `confirmation-required` (salida 1), `aborted` (salida 1), `internal` (salida 1), `command-unavailable` (salida 1), `reserved-command` (salida 1), `command-failed` (salida 1), `invalid-args` (salida 2)

| Campo | Tipo | Requerido | Valores |
|---|---|---|---|
| `engine_state` | string | sí | "no-repos", "observing", "waiting-for-git" |
| `requester` | null \| object | no |  |
| `requester.actor` | object | no |  |
| `requester.actor.actor` | string | sí | "agent", "unattributed" |
| `requester.actor.kind` | string | no | "claude-code", "other" |
| `requester.actor.name` | null \| object | no |  |
| `requester.actor.name.lossy` | boolean | no |  |
| `requester.actor.name.truncated` | boolean | no |  |
| `requester.actor.name.untrusted` | string | no |  |
| `requester.actor.origin` | string | no | "detected", "registered" |
| `requester.confirmable` | boolean | no |  |
| `requester.layer` | string | no | "cockpit", "mcp" |
| `requester.state` | string | no | "resolved", "unverified" |

## agent-withdraw

- Subcomando: `raptor agent withdraw --json`
- Perfil: desarrollador (el agente recibe reserved-command)
- Versión: `1.0`
- Códigos de error posibles: `engine-unavailable` (salida 1), `repo-not-enabled` (salida 1), `not-in-observed-worktree` (salida 1), `repo-unavailable` (salida 1), `identity-unverified` (salida 1), `rate-limited` (salida 1), `confirmation-required` (salida 1), `aborted` (salida 1), `internal` (salida 1), `command-unavailable` (salida 1), `reserved-command` (salida 1), `command-failed` (salida 1), `invalid-args` (salida 2)

| Campo | Tipo | Requerido | Valores |
|---|---|---|---|
| `repo_id` | string | sí |  |
| `session_id` | string | sí |  |

## Códigos estables

Familia de salida: 0 éxito, 1 rechazo o fallo, 2 uso (`invalid-args`).

- `repo-not-enabled` (salida 1)
- `not-in-observed-worktree` (salida 1)
- `repo-unavailable` (salida 1)
- `engine-unavailable` (salida 1)
- `identity-unverified` (salida 1)
- `rate-limited` (salida 1)
- `time-limit` (salida 1)
- `result-too-large` (salida 1)
- `internal` (salida 1)
- `unattributed` (salida 1)
- `operation-in-progress` (salida 1)
- `git-busy` (salida 1)
- `quota-exceeded` (salida 1)
- `invalid-text` (salida 1)
- `state-changed` (salida 1)
- `outcome-unknown` (salida 1)
- `worktree-missing` (salida 1)
- `repo-other-owner` (salida 1)
- `worktree-untrusted` (salida 1)
- `invalid-cursor` (salida 1)
- `confirmation-required` (salida 1)
- `detached-head` (salida 1)
- `nothing-to-commit` (salida 1)
- `path-not-found` (salida 1)
- `policy-denied` (salida 1)
- `aborted` (salida 1)
- `failed-unchanged` (salida 1)
- `failed-changed` (salida 1)
- `invalid-path` (salida 1)
- `path-outside-worktree` (salida 1)
- `too-many-paths` (salida 1)
- `path-ignored` (salida 1)
- `other-session-present` (salida 1)
- `prediction-unavailable` (salida 1)
- `command-unavailable` (salida 1)
- `reserved-command` (salida 1)
- `invalid-args` (salida 2)
- `command-failed` (salida 1)
