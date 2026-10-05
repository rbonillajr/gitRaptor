---
id: API-GRP-IPC
title: "Contrato del canal local (JSON-RPC 2.0)"
type: api-spec
status: draft
domain: GRP
feature: motor-local
created: 2026-10-04
updated: 2026-10-05
related:
  adrs: [ADR-GRP-005, ADR-GRP-011, ADR-GRP-013]
  stories: [TS-GRP-004, US-GRP-001, US-GRP-012]
  specs: [DS-TS-GRP-004, DS-US-GRP-001, DS-US-GRP-012]
  deps: [DEP-CKP-6]
tags: [ipc, json-rpc, contrato, eventos, mcp, seguridad]
---

# Contrato del canal local

La fuente de verdad es el código de `crates/api`. Este documento es su resumen para quien consume el canal: CLI/TUI, `raptor-mcp`, el Cockpit y las historias que añaden métodos. Lo fija [ADR-GRP-005](../decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md) § 5 y lo implementa [TS-GRP-004](../../requirements/features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md). La descripción OpenRPC que cita el overview queda pendiente; `schemars` ya genera el JSON Schema de cada tipo.

## Transporte y marco

- Socket Unix `raptor.sock` en la carpeta de ejecución del perfil, con la carpeta en 0700 y el socket en 0600. Solo acepta clientes del mismo uid. El cliente comprueba que el servidor también es de su uid. En Windows, named pipe: pendiente.
- Un mensaje JSON por línea (`\n`), de 1 MiB como máximo y con una profundidad máxima de 32. No se aceptan batches. Todos los tipos rechazan campos desconocidos.
- `PROTOCOL_VERSION = 3` (`API_VERSION` 3.0.0). Cliente y daemon son compatibles solo si hablan la misma versión. La 2 (US-GRP-001) añade el estado de los worktrees a `RepoView`; la 3 (US-GRP-012), la rama base del repo y el ahead/behind de cada worktree.

## Handshake

El primer mensaje es `hello`; si no llega en 2 s, el daemon cierra la conexión. La forma de `hello`, `IncompatibleData` y `daemon.replace` **no cambia entre versiones**.

```json
{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":2,"client":"cli","client_version":"0.0.0"}}
```

El resultado trae `protocol`, `binary_version`, `instance_id` (del perfil, ADR-GRP-006 § 4), `daemon_pid`, `profile` (`full` o `mcp`), `max_message_bytes` y `methods`: los métodos que puede llamar esa conexión. Si la versión no coincide, el daemon responde `-32002` con `{daemon_protocol, binary_version}`. Si el cliente es más nuevo, puede enviar `daemon.replace`.

## Métodos

| Método | Reservado | MCP | Resultado / notas |
|---|---|---|---|
| `ping` | No | Sí | `"pong"` |
| `engine.snapshot` | No | Sí | `Snapshot { run_id, seq, engine, daemon, repos }`, cada repo con sus `worktrees` (ver más abajo); en MCP, `McpSnapshot { run_id, seq, engine_state, caller_repo }` (allowlist de campos) |
| `events.subscribe` | No | Sí | `{ from_seq?, run_id? }` → `{ subscription, from_seq }`. Hasta 4 por conexión |
| `events.unsubscribe` | No | Sí | `{ subscription }` → `bool` |
| `audit.list` | No | No | `{ after_id?, limit? }` → `{ entries: [AuditEntry] }`. Como máximo 500 por página |
| `daemon.stop` | **Sí** | No | `{ stopping: true }`; luego el daemon cierra las conexiones |
| `daemon.replace` | Solo si no viene del binario instalado | Sí | `{ protocol }` (más nuevo que el del daemon) |
| `repo.add` | **Sí** | No | `{ path }` (raíz de un worktree o directorio Git, sin búsqueda hacia arriba) → `{ outcome: new\|already-observed\|reactivated, repo: RepoView }`. Autoriza y audita **antes** de leer la ruta (US-GRP-001) |
| `repo.retire` | **Sí** | No | `{ repo_id }` → `{ retired }`. Deja de observar y conserva los datos (US-GRP-001; US-GRP-006 añade el historial y el hueco) |
| `attribution.correct`, `attribution.withdraw-correction` | **Sí** | No | Declarados. Los implementa US-GRP-010 |
| `registration.withdraw` | **Sí** | No | Declarado. Lo implementa US-GRP-009 |

Un comando reservado lo decide **solo el daemon**, con la identidad del par y sin fiarse de nada que declare el cliente: ascendencia sin agentes y sin el propio daemon, líder de sesión y terminal de control (ADR-GRP-005 § 6 y su Enmienda TS-GRP-004). Cada intento, aceptado o no, queda en la auditoría append-only y se publica como evento `reserved.audit`. Un método declarado responde `-32004` con `{implemented_by}` después de autorizar y auditar.

## Errores

| Código | Significado |
|---|---|
| `-32700`, `-32600`, `-32601`, `-32602`, `-32603` | JSON-RPC estándar (análisis, petición, método, parámetros, interno) |
| `-32001` | Falta el `hello` (la conexión se cierra) |
| `-32002` | Versión de protocolo incompatible |
| `-32003` | Comando reservado rechazado; `data.reason`: `agent-ancestry`, `session-leader-agent`, `daemon-descendant`, `no-controlling-terminal`, `identity-unverified`, `not-available-to-mcp` o `unsupported` |
| `-32004` | Declarado, pero sin implementar todavía |
| `-32005` | Rate limit (100/s, ráfaga de 200). La conexión sigue abierta |
| `-32006` | Límite de conexiones o de suscripciones |
| `-32007` | No se puede continuar la suscripción: hay que tomar una instantánea nueva |
| `-32008` | Repo rechazado por lo que nombra (no por quién lo pide); `data.reason`: `not-a-repo`, `untrusted`, `unreadable` o `unknown-repo` |

## Eventos

Notificación `events.event` con `{ subscription, event }`. El evento lleva:

```json
{"seq":42,"kind":"git.event","version":1,"wall_ms":1791148066018,
 "timings":{"batch_id":7,"t_recv":…,"t_flush":…,"t_computed":…,"t_persisted":…,"t_published":…},
 "data":{…}}
```

- `seq` es único en el daemon y estrictamente creciente. Vuelve a empezar en cada arranque, y por eso existe `run_id`.
- Los eventos de cambio llevan siempre `timings`, en nanosegundos del reloj monótono común (`gitraptor_api::clock::monotonic_ns`, ADR-GRP-011 § 3). El cliente añade `t_client_recv` con el mismo reloj.
- Tipos propios del motor (sin `timings`): `engine.state`, el primer evento de cada ejecución y cada transición de BR-WF-002; `daemon.stopping`; `reserved.audit`; y `repo.observation` `{repo_id, observed, state, path}` al añadir o retirar un repo (US-GRP-001).
- `worktree.state` (US-GRP-001, de cambio): `{repo_id, worktrees}` con todos los worktrees del repo reconciliado.
- A `raptor-mcp` solo le llegan `engine.state` y `daemon.stopping` (allowlist, SEC-12); el resto lleva rutas o auditoría.
- Tipos de cambio declarados, con `data` definido por su historia: `git.event` (US-GRP-002), `gap.recorded` (US-GRP-005), `session.state` (US-GRP-007) y `attribution.changed` (US-GRP-010).
- **Arranque coherente (DEP-CKP-6)**: `engine.snapshot` devuelve `seq = N`. Después, `events.subscribe { from_seq: N + 1, run_id }` no pierde ni repite eventos: el daemon guarda los últimos 1024. Si el `run_id` ya no es el del daemon o `N + 1` salió del buffer, llega `events.resync` con su `reason` (`daemon-restarted` o `replay-unavailable`).
- **Cliente lento**: si su cola de 1024 mensajes se llena, recibe `events.resync { reason: slow-consumer }` y se le desconecta. Nunca frena al productor.

## Estado de un worktree (US-GRP-001)

```json
{"path":{"untrusted":"/w/demo-feat"},"main":false,"admin_name":{"untrusted":"demo-feat"},
 "status":{"state":"ready","head":{"kind":"branch","name":{"untrusted":"feat-login"}},
           "counts":{"staged":0,"unstaged":1,"untracked":0},
           "changes":[{"path":{"untrusted":"login.txt"},"area":"unstaged","kind":"modified"}]}}
```

- `head`: `branch {name}`, `unborn {name}` (rama sin commits) o `detached`. `status` también puede ser `{"state":"unavailable","reason":"missing"|"untrusted"|"unreadable"}`; la semántica de los estados especiales es de US-GRP-003.
- Limpio = todos los `counts` a cero. `changes` está ordenado y acotado a 200 rutas y 32 KiB por worktree. Si un mensaje pasa de 768 KiB, se vacían las listas y se conservan los conteos.
- Lo recalcula una reconciliación completa al añadir el repo y al arrancar el motor. Los cambios en vivo son de US-GRP-002.

## Rama base y ahead/behind (US-GRP-012)

```json
"base":{"name":{"untrusted":"main"},"status":"unconfirmed"}
"divergence":{"state":"counted","ahead":{"count":3,"exact":true},"behind":{"count":1,"exact":true}}
```

- `RepoView.base`: la rama base confirmada del almacén por repo (`confirmed`) o, sin confirmación, `main` (`unconfirmed`). `invalid` (sin `name`) solo llega con la configuración del equipo de US-GRP-016. Añadir el repo nunca confirma nada (ADR-GRD-004 § 3.5).
- `divergence` va dentro de `status` `ready`: `counted {ahead, behind}`, cada lado `{count, exact}` (`exact: false` si el recorrido llegó al tope de 10 000); `base-missing` (no existe `refs/heads/<base>`; no se usa ninguna otra ref, Q42); `no-base`; `no-commits`; o `unreadable`.
- Se calcula en la reconciliación y otra vez en cada `engine.snapshot` de una conexión completa, con la punta de la base leída en ese momento y la rama de cada fila (DS-US-GRP-012 D5). Un `worktree.state` lleva el valor de su reconciliación hasta que US-GRP-002 lo publique en vivo.

## Texto no confiable y actor

- Todo texto que viene del repo o de un agente viaja como `{"untrusted": "…"}`, con `truncated` y `lossy` cuando aplican. Los clientes muestran solo `Untrusted::sanitized()`, que quita escapes ANSI, OSC y DCS, controles, bidi y caracteres de ancho cero (SEC-12).
- El actor es `{"actor":"agent","kind":"claude-code"|"other","name"?,"origin":"detected"|"registered"}` o `{"actor":"unattributed"}`. No existe la variante "humano" (ADR-GRP-013 § 6).

## Pendiente

- Descripción OpenRPC generada desde los tipos.
- Windows (named pipe).
- Consultas bajo demanda del Cockpit (DEP-CKP-2 y DEP-CKP-3, enmienda de ADR-GRP-005 § 5).
- Campos de última actividad (DEP-CKP-4) y timeline (DEP-CKP-5).
- `caller_repo` real en macOS (F-001-05).
