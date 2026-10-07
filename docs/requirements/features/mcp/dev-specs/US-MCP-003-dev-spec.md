---
id: DS-US-MCP-003
title: "Dev Spec — US-MCP-003: herramienta status del MCP, con el ámbito del repo del agente"
type: dev-spec
status: approved
feature: mcp
domain: MCP
created: 2026-10-07
updated: 2026-10-07
related:
  stories: [US-MCP-003, US-MCP-002, US-MCP-004]
  adrs: [ADR-MCP-001, ADR-GRP-005, ADR-TMC-005, ADR-GRP-016]
  rules: [BR-MCP-CALC-001, BR-MCP-ELIG-006, BR-MCP-ELIG-001, BR-MCP-EDGE-004, BR-MCP-CONS-001, BR-MCP-TIME-003, BR-MCP-EDGE-001]
  nfrs: [NFR-02, SEC-12, SEC-MCP-07]
tags: [mcp, status, ambito, esqueleto-andante, ola-1]
---

# Dev Spec — US-MCP-003: herramienta `status` del MCP, con el ámbito del repo del agente

Plano compacto (AADD ligero) de [US-MCP-003](../user-stories/US-MCP-003-status-del-repo-del-agente.md). El contrato lo fija [ADR-MCP-001](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md) § 2 (ámbito por el cwd del proceso, resuelto en el daemon) y § 3 (allowlist). Aquí `status` devuelve lo mínimo del esqueleto: repo, worktree del llamante y solicitante. El contenido completo es US-MCP-004 y los topes, US-MCP-005.

## 1. Decisiones

Las marcadas con † son **Decisión del orquestador (2026-10-07), validada por Arquitecto/PO**; el resto aplica el ADR. D1 queda registrada como Enmienda (2026-10-07) de ADR-MCP-001.

| # | Decisión |
|---|---|
| D1 † | Método nuevo `mcp.status` en el módulo `mcp` (no reservado, **con marca MCP**), sin parámetros: el repo y el worktree **nunca** vienen del agente. ADR-MCP-001 § 4 apuntaba a `engine.snapshot` + `requester.resolve`; ampliar `McpSnapshot` cambia una forma que ya existe y pediría una capacidad (ADR-GRP-016), y dos llamadas abren una ventana entre el ámbito y el solicitante. Un método nuevo existe para toda conexión de protocolo 9 y aplica la allowlist en el daemon, en una sola llamada. `engine.snapshot` no cambia. |
| D2 | Ámbito: el daemon lee el cwd del proceso par (`process_cwd`), lo canonicaliza y elige el worktree observado **más profundo** que lo contiene, comparando por componentes (`/w/repo` no contiene `/w/repo-x`), entre todos los worktrees publicados del repo (principal y enlazados). La identidad del par se comprueba antes y después de leer el cwd (`requester::resolve`): si cambió, `identity-unverified`. |
| D3 | Rechazos, sin ningún dato del repo, con el código congelado `scope-refused` y su `reason`: cwd ilegible o fuera de un worktree observado → `not-observed` (falla cerrado, ADR-MCP-001 § 2); repo observado fuera de la allowlist → `not-allowlisted`. La comprobación de la allowlist va **después** de la del worktree y la respuesta no lleva `repo_id`. |
| D4 | Resultado `McpStatus` con allowlist de campos (SEC-12): `repo_id`, `repo_state`, `worktree` (solo el **nombre** de la carpeta del worktree, texto no confiable acotado, nunca la ruta), `main`, `requester` (el `Actor` que ya expone `requester.resolve` por MCP) y `action`: `register-to-write` si es "sin atribuir", ausente si es un agente. |
| D5 | `raptor-mcp` anuncia **una** herramienta, `status`, sin parámetros, con `outputSchema` y `structuredContent` más un bloque de texto con el mismo JSON. Se conecta al daemon **en la primera llamada** (perfil `mcp`, `ensure_daemon` con el `raptor` instalado junto a `raptor-mcp`), nunca al iniciar. Sin `chdir`, sin leer `CLAUDE_PROJECT_DIR`. |
| D6 † | Errores de la herramienta (ajuste del Arquitecto: `no-working-folder` también da `not-in-observed-worktree`, y `apps/mcp` no lleva textos en español) (`isError: true`, `structuredContent` `{reason, action}`, códigos estables en inglés): `repo-not-enabled` → acción `ask-the-developer-to-run: raptor mcp enable`; `not-in-observed-worktree` → `start-the-session-inside-an-observed-repo`; `engine-unavailable` ("GitRaptor no está en marcha y no se pudo arrancar") → `check-the-gitraptor-installation`; `identity-unverified` y `internal` sin datos. La traducción en/es de esos textos al agente queda para US-MCP-005 (respuestas acotadas), igual que los topes. |
| D8 | Ajustes del coordinador y del Arquitecto: el cwd se canonicaliza antes de buscar el worktree y la allowlist (un enlace simbólico hacia un repo no habilitado se rechaza); el contenido de un rechazo es solo `{reason, action}`, probado; y `engine.snapshot` por MCP deja `caller_repo` vacío fuera de la allowlist, para que la clave de un repo no habilitado no salga por ninguna vía. |
| D7 † | Producto (PO): un agente en un repo **no** habilitado recibe solo el rechazo con su acción: ni `repo_id`, ni rama, ni ruta. "Sin atribuir" lee igual que un agente y además recibe `register-to-write`. |

## 2. Código

| Pieza | Ubicación |
|---|---|
| `mcp.status`, `McpStatus`, `McpStatusAction` | `crates/api/src/methods/mcp.rs` |
| Ámbito por cwd y respuesta; `engine.snapshot` filtrado por la allowlist | `crates/core/src/channel/conn.rs` (brazo junto a `repo.*`), resolución en `crates/core/src/channel/mcp_scope.rs` (nuevo) |
| Herramienta `status` y conexión perezosa | `apps/mcp/src/server.rs`, `apps/mcp/src/engine.rs` (nuevo) |

## 3. Pruebas

Todas en `apps/cli/tests/mcp_allowlist.rs` salvo indicación.

| Escenario | Prueba |
|---|---|
| Desde una subcarpeta de un worktree enlazado: repo, worktree y solicitante; desde el principal, `main` | `status_from_a_subfolder_names_the_repo_and_the_worktree` |
| Un `cd` del agente no cambia el ámbito; un enlace simbólico hacia otro repo resuelve al repo real | `the_scope_is_the_servers_real_folder` (el cwd es el de `raptor-mcp`, que nunca cambia de carpeta) |
| Fuera de la allowlist: rechazo sin datos | `observing_a_repo_does_not_enable_it` |
| Fuera de un repo observado: rechazo sin datos | `outside_an_observed_repo_no_data_is_returned` |
| "Sin atribuir" lee y recibe `register-to-write` | `status_from_a_subfolder_names_the_repo_and_the_worktree` (el test no corre bajo Claude Code) |
| El motor arranca con la primera llamada, no con la sesión | `the_engine_starts_with_the_first_call` + `apps/mcp/tests/handshake.rs::the_handshake_does_not_start_the_engine` |
| Motor no arrancable: acción y ningún dato | `an_engine_that_cannot_start_gives_the_action` |
| `engine.snapshot` por MCP no nombra un repo fuera de la allowlist | `the_mcp_snapshot_names_only_an_enabled_repo` |
| Resolución del worktree más profundo, por componentes | `crates/core/src/channel/mcp_scope.rs` (test unitario) |
| Superficie constante (`tools/list` con solo `status`, sin argumentos) | `apps/mcp/tests/handshake.rs`, `apps/mcp/src/server.rs` (tests) |
| Correspondencia de rechazos del daemon con los de la herramienta | `apps/mcp/src/engine.rs` (test) |

## 4. Pendientes

- Textos de motivo y acción en en/es (p. ej. "repo no habilitado para el MCP", "regístrate para escribir", "GitRaptor no está en marcha y no se pudo arrancar", "revisa la instalación de GitRaptor"): US-MCP-005 (D6, ajuste del PO). Mientras tanto, las pruebas comprueban el código estable, no el texto literal del escenario.
- Atribución "claude-1" de extremo a extremo bajo una sesión real de Claude Code: la resolución del solicitante ya está probada en `requester`; la prueba de proceso de esta historia corre como "sin atribuir".
- `cargo-deny` de las features de red de `rmcp` (SEC-07, SEC-MCP-09), heredado de DS-US-MCP-001: sigue pendiente, fuera del alcance de esta tarea.
- Leer el cwd de otro proceso en Linux y Windows (DEP-MCP-9). **Pendiente: etapa de validación multiplataforma.**
