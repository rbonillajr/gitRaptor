---
id: DS-US-MCP-002
title: "Dev Spec — US-MCP-002: el desarrollador habilita y retira repos de la allowlist del MCP"
type: dev-spec
status: implemented
feature: mcp
domain: MCP
created: 2026-10-07
updated: 2026-10-08
related:
  stories: [US-MCP-002, US-MCP-003]
  adrs: [ADR-MCP-001, ADR-GRP-005, ADR-GRP-006, ADR-GRP-016, ADR-GRD-007]
  rules: [BR-MCP-WF-006, BR-MCP-CONS-003, BR-MCP-AUTH-004]
  nfrs: [NFR-01, NFR-02, SEC-03]
tags: [mcp, allowlist, opt-in, comando-reservado, ola-1]
---

# Dev Spec — US-MCP-002: el desarrollador habilita y retira repos de la allowlist del MCP

Plano compacto (AADD ligero) de [US-MCP-002](../user-stories/US-MCP-002-habilitar-repo-allowlist.md). El contrato ya está decidido en [ADR-MCP-001](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md) § 3 y en la Enmienda (2026-10-05, MCP) de [ADR-GRP-006](../../../../architecture/decisions/ADR-GRP-006-perfil-ubicacion-almacenamiento.md): la allowlist es una **marca del repo observado**, la escribe solo el daemon con los comandos reservados `mcp.enable` y `mcp.disable`, y retirar el repo la borra en la misma transacción. Esta spec fija lo que el ADR dejó a la Dev Spec (nombres finales, forma, dónde vive el código) y la extensión sigue [ADR-GRP-016](../../../../architecture/extender-sin-archivos-compartidos.md).

## 1. Decisiones

Las marcadas con † son **Decisión del orquestador (2026-10-07), validada por Arquitecto/PO**; el resto aplica el ADR. Plan aprobado por el coordinador con cuatro ajustes (D9 y DS-US-MCP-003 D8).

| # | Decisión |
|---|---|
| D1 | Módulo del contrato nuevo `mcp` (`crates/api/src/methods/mcp.rs`): `mcp.enable` y `mcp.disable` **reservados, sin marca MCP**; `mcp.allowlist` de lectura, no reservado y **nunca por MCP** (ADR-MCP-001 § 3: "la consulta es de lectura para CLI/TUI y Guardrails; nunca por MCP"). Sin bloque de errores propio: un repo no observado responde con el código y el motivo que ya existen (`repo-rejected`, `not-observed`). |
| D2 | Parámetros `{path}` (cualquier carpeta del repo, como `repo.add`): validación léxica, **luego** autorización y auditoría del comando reservado, y solo después se lee el disco (SEC-03; un agente no puede hacer que el daemon sondee el sistema de archivos). Resultado `{repo_id, enabled, changed}`; repetir es idempotente (`changed: false`). |
| D3 | Almacén: migración del índice que añade a `repos` `mcp_enabled_ms` y `mcp_enabled_by` (NULL = fuera de la allowlist). `enable` es un `UPDATE … WHERE state = 'observed'`: la invariante allowlist ⊆ observados se cumple en la misma sentencia. `retire` pone a NULL la marca **en la misma sentencia** que retira (cascada); volver a añadir el repo también la deja a NULL (el opt-in se repite). `mcp_enabled_by` guarda el tipo de cliente que la puso (`cli`). |
| D4 | Escritor único: los comandos llegan al bucle del daemon por el `Control` del canal, como `repo.add`/`repo.retire`. El daemon mantiene en memoria el conjunto habilitado (`McpRepos`, cargado del índice al arrancar) y lo pasa a los backends de operaciones protegidas y del undo en lugar de `NoMcpRepos`. |
| D5 † | Aviso de la cascada: `repo.retire` no cambia de forma (cambiarla pediría una capacidad, ADR-GRP-016). `raptor repo retire` consulta `mcp.allowlist` antes de retirar y, si el repo estaba habilitado y la retirada fue efectiva, avisa "`<repo>` también salió de la allowlist del MCP" (en/es). Si la consulta falla o el motor no la ofrece, la retirada sigue sin aviso (ajuste del Arquitecto). |
| D6 | CLI: `raptor mcp enable [ruta]`, `raptor mcp disable [ruta]` (por defecto, la carpeta actual) y `raptor mcp list`. Mensajes en `apps/cli/i18n/{en,es}/mcp.txt`. Un agente que lo intenta recibe el rechazo de comando reservado (`daemon-descendant` / `not-a-terminal`, ya en main) y la allowlist no cambia. |
| D8 | Los métodos nuevos llevan `since(9)`: los clientes de los protocolos 5 a 8 conservan su contrato (`crates/api/tests/legacy_protocols.rs`). La guía de ADR-GRP-016 dice "sin `.since`", pero eso cambiaría lo que ve un cliente de 5 a 8; queda anotado en el PR. |
| D9 | Ajustes del coordinador: la migración es solo hacia delante e idempotente, y un perfil anterior abre, migra y conserva sus repos sin habilitar ninguno; un agente es rechazado por la CLI y por una conexión MCP, ambos probados. |
| D7 † | El diagnóstico "MCP no instalado" en el estado de protección y la publicación del cambio de estado de protección en el stream son de **US-GRD-016** (la historia ya lo delega: "aquí solo se comprueba que la capa MCP queda activa o inactiva"). Aquí "capa MCP activa" se observa como: el repo está en `mcp.allowlist` y `mcp.status` le responde (US-MCP-003). |

## 2. Código

| Pieza | Ubicación |
|---|---|
| Métodos, parámetros y resultados | `crates/api/src/methods/mcp.rs`, `crates/api/src/mcp.rs` |
| Migración y consultas de la marca | `crates/core/src/profile/schema.rs`, `crates/core/src/profile/index.rs`, `crates/core/src/profile/store.rs` |
| Conjunto en memoria `McpRepos` (implementa `McpAllowlist`) | `crates/core/src/timemachine/protected/scope.rs` |
| Atención en el daemon (bucle) | `crates/core/src/daemon/mcp.rs` (nuevo), `Control` en `daemon/shutdown.rs`, dos brazos en el bucle |
| Atención en el canal | `crates/core/src/channel/conn.rs` (brazos junto a `repo.*`) |
| CLI | `apps/cli/src/commands/mcp.rs`, `apps/cli/src/mcp.rs`, aviso en `apps/cli/src/commands/repo.rs` |

## 3. Pruebas

| Escenario | Prueba |
|---|---|
| Habilitar un repo observado: figura en la allowlist; nada cambia en el repo | `apps/cli/tests/mcp_allowlist.rs::the_developer_enables_an_observed_repo` |
| Observar no habilita | `mcp_allowlist.rs::observing_a_repo_does_not_enable_it` |
| No se habilita un repo no observado ("observa el repo primero") | `mcp_allowlist.rs::a_repo_that_is_not_observed_cannot_be_enabled` |
| Retirar de la observación saca de la allowlist, con aviso; volver a añadir no la restaura | `mcp_allowlist.rs::retiring_the_repo_takes_it_out_of_the_allowlist` |
| Quitar de la allowlist y seguir observado | `mcp_allowlist.rs::disabling_keeps_the_repo_observed` |
| Un agente no puede habilitar (CLI desde su shell, también bajo pty, y conexión MCP) | `mcp_allowlist.rs::an_agent_cannot_enable_a_repo` |
| Cascada, invariante y migración de un perfil anterior | `crates/core/tests/profile_mcp_allowlist.rs` |
| Contrato: reservado, nunca por MCP; protocolos 5 a 8 intactos | `crates/api/src/methods/mcp.rs::only_status_is_offered_over_mcp`, `crates/api/tests/legacy_protocols.rs` |

## 4. Pendientes

- Diagnóstico "MCP no instalado" y evento del estado de protección: US-GRD-016 (D7).
- Linux y Windows: las pruebas de proceso son de macOS (pty con `script`). **Pendiente: etapa de validación multiplataforma.**

## Estado de la implementación (2026-10-08)

Implementado en: PR #140, #159.

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
