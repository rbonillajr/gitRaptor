---
id: DS-INF-MCP-001
title: "Dev Spec — Corpus de seguridad del MCP como suite de CI que bloquea el merge"
type: dev-spec
status: partially-implemented
feature: mcp
domain: MCP
created: 2026-10-09
updated: 2026-10-09
story: INF-MCP-001
scope: backend
frontend_surface: false
stack: rust
profile: backend-service
tooling: [cargo]
author: rust-architect
related:
  context: ../context.md
  story: ../technical-stories/INF-MCP-001-corpus-seguridad-mcp.md
  adrs: [ADR-MCP-001, ADR-CKP-002, ADR-GRP-005, ADR-GRP-001]
  api_spec: null
  design_spec: null
  contracts: []
must_read:
  - ../technical-stories/INF-MCP-001-corpus-seguridad-mcp.md
  - ../context.md
  - ../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md
  - ../../../../architecture/xplat-pendientes.md
  - ../../../../dev-briefs/inf-mcp-001-mcp-security-corpus.md
  - ../../../../../AGENTS.md
evidence: []
lineage:
  supersedes: []
  superseded_by: []
  migration_adr: null
  migration_guide: null
constitution_gates: []
validation:
  must_read_resolved: true
  gaps_blocking: 0
  ready_to_implement: true
  gaps_release: 0
  ready_to_release: false
tags: [mcp, seguridad, corpus, ci, nfr-02, sec-mcp, ola-3]
---

# DS-INF-MCP-001 · Corpus de seguridad del MCP como suite de CI

## Contexto rápido

El corpus lanza `raptor-mcp` por stdio, como lo haría Claude Code, contra un daemon de prueba en un perfil temporal y repos temporales, y ejecuta un conjunto versionado de ataques. Calcula un único KPI (el porcentaje del corpus rechazado, Q-MCP-18) y rompe el CI si un caso no se rechaza, si aparece un canario en stdout o stderr o si una respuesta sale de la allowlist.

Esta Dev Spec resume los § 3, § 4, § 6 y § 10 del Brief aprobado ([`inf-mcp-001-mcp-security-corpus.md`](../../../../dev-briefs/inf-mcp-001-mcp-security-corpus.md)), que es la fuente detallada (topología de ficheros, plan de pruebas, workflow).

⚠️ **ASSUMPTION**: no existe `architecture-constitution.md`; rigen `AGENTS.md` y los ADR de `must_read` (NFR-01, NFR-02).

---

## 📋 Índice

1. La forma (decisiones D1 a D13)
2. Contratos (formato del caso, módulo del testkit, runner)
3. Matriz de plataformas
4. Fuera de alcance (Not Built)
5. Decisiones del orquestador (2026-10-09)
6. Estado de la implementación

---

## ⚠️ Gaps y violaciones de la constitución

Ninguno bloqueante. Abiertos: el único `known_gap` es `jsonrpc-resources-list`; el tope de entrada de `raptor-mcp` (1 MiB y 32 niveles) ya se aplica (PR #225) y sus casos exigen rechazo. El nivel `engine` en Windows y la primera ejecución verde en Ubuntu siguen en XP-42.

---

## 🔭 La forma

| # | Decisión | Por qué |
|---|---|---|
| D1 | Lógica pura en `crates/testkit/src/mcp_corpus/` (modelo del caso, juez, escáner, esquema, informe) y runner en `apps/cli/tests/mcp_corpus/` | El juez se prueba sin binarios y con fallos inyectados, en los tres SO. El runner necesita `CARGO_BIN_EXE_raptor` y los crates de GitRaptor; el testkit no puede depender de ellos, así que el runner le pasa los topes (`Limits`) |
| D2 | Un test, un informe: `the_corpus_is_fully_rejected` ejecuta todos los casos en un pool de ≤ 4 hilos | El KPI es una razón sobre todo el corpus |
| D3 | Casos en JSON, un archivo por caso, con esquema cerrado | Una errata falla en voz alta en vez de desactivar un caso en silencio |
| D4 | Dos niveles: `server` (rechazado antes del motor; todos los SO) y `engine` (daemon real en perfil temporal; macOS y Linux) | Lo que se rechaza por esquema no necesita daemon. El ámbito, la allowlist y el rate limit los decide el daemon |
| D5 | Estado declarativo del daemon (`repo` y `other_repo` en `none`, `observed` o `enabled`, más `worktrees`, `symlinks`, `dirs` y `path_trap`), materializado con `raptor repo add` y `raptor mcp enable` bajo pty | Es lo que hace el desarrollador real. No se escribe el perfil a mano |
| D6 | Regla de veredicto única: la primera respuesta que no es un éxito debe casar con `expect`; si todas son éxitos, `NotRejected` | Vale para una llamada y para `repeat` (rate limit) |
| D7 | Escáner de secretos propio, sin gitleaks: canarios exactos con forma de secreto real plantados en repo, config, commit, `.env` y entorno, más un detector de formas de token conocidas | Cero falsos negativos sobre lo plantado y sin binario externo que fijar en CI |
| D8 | Comprobaciones comunes: stdout solo JSON-RPC, stderr solo códigos fijos, ningún canario ni forma de token, ningún carácter oculto de L-03, cada parte ≤ 24 KiB, un rechazo ≤ 80 tokens estimados, allowlist de campos, repo intacto y ninguna trampa de `PATH` disparada | SEC-MCP-05, 06, 08 y 09 |
| D9 | El comprobador de esquema falla cerrado: una palabra clave fuera del subconjunto soportado es un error | La allowlist no puede pasar en silencio si cambia `compact_schema` |
| D10 | Autopruebas de mutación en dos capas: puras (testkit) y reales (runner, con la respuesta alterada) | Un traversal que pasa rompe la suite y un campo no declarado rompe la allowlist |
| D11 | Gate nuevo `.github/workflows/mcp-security-corpus.yml`, solo Ubuntu, con filtro de rutas, KPI en el job summary y mínimo de casos ejecutados | macOS lo ejecuta en `lint and test (macos-latest)` (obligatorio) y Windows en `lint and test (windows-latest)`, sin bloquear |
| D12 | Todo el runner bajo `#![cfg(debug_assertions)]` | `GITRAPTOR_PROFILE_DIR` solo existe en debug; en release el arnés tocaría el perfil real (NFR-01) |
| D13 | `pending` obligatorio cuando un caso no corre en los tres SO, con la marca XP-42 | El informe lo cuenta aparte y nunca como rechazado |

## Contratos

### Formato del caso

Ruta: `apps/cli/tests/mcp_corpus/cases/<id>.json`; el nombre del archivo es el `id`. Esquema cerrado, claves:

| Clave | Resumen |
|---|---|
| `id`, `title`, `threats` | Identificador kebab-case (≤ 64), título en inglés y ids de OWASP o de reglas |
| `tier`, `platforms`, `pending` | Nivel (`server` o `engine`), plataformas (`engine`, `symlinks`, `path_trap` y `parent: "agent"` excluyen `windows`) y marca obligatoria cuando no son los tres SO |
| `setup`, `session` | Estado del daemon y sesión (cwd, entorno, padre). Las claves `GITRAPTOR_PROFILE_DIR` y `GITRAPTOR_AGENT_EXECUTABLES` están prohibidas (NFR-01) |
| `send`, `expect`, `forbidden` | Mensajes (`call`, `raw` o `message`), la respuesta esperada (`refusal`, `protocol_error`, `invalid_request` o `ignored`) y los textos que no pueden aparecer |
| `known_gap` | Opcional. Referencia a la historia que cierra un hueco conocido (ver "Decisiones del orquestador") |

Las ubicaciones usan anclas (`root`, `repo`, `other_repo`, `home`, `wt-<name>`) y los marcadores (`{root}`, `{repo}`, `{repo_id:repo}`, `{canary:<name>}`...) están acotados; uno desconocido es un error de carga.

### Módulo del testkit y runner

`crates/testkit/src/mcp_corpus/` expone el modelo (`Case`, `Tier`, `Platform`, `Expect`), el juez (`Verdict`), el escáner de canarios (`CANARY_NAMES`), el informe (`Outcome`, `Row`, `Report`) y los topes (`Limits`). El runner de `apps/cli/tests/mcp_corpus/` materializa el estado, lanza el binario, recoge stdout y stderr y entrega las observaciones al juez. Los 60 archivos de casos (43 `server` y 17 `engine` en el Brief) son datos y no exigen código.

## Matriz de plataformas

| Aspecto | macOS | Linux | Windows |
|---|---|---|---|
| Nivel `server` | Soportado; local y `lint and test (macos-latest)` | Soportado; gate nuevo y `lint and test (ubuntu-latest)` | Soportado; `lint and test (windows-latest)`, sin bloquear |
| Nivel `engine` | Soportado | Soportado de forma asumida; falta la primera ejecución verde | `Pending`, XP-42 |
| `symlinks`, `path_trap`, `parent: agent` | Soportado | Soportado | El cargador lo rechaza |
| Ruta UNC como cwd | — | — | Pendiente, XP-42 |

Si el primer run de Ubuntu muestra un fallo del daemon en Linux (no del arnés), el experto no lo arregla: deja los casos afectados en `["macos"]` con `pending: "XP-42"` y lo reporta; lo aprueba el coordinador.

## Fuera de alcance

- gitleaks, hasta que haga falta detectar secretos no plantados (revisión por release, SEC-MCP-12).
- Nivel `engine` en Windows, hasta validar XP-42.
- Job de macOS o Windows en el gate nuevo, y check obligatorio de `main` (decide Rene).
- PID reutilizado (`identity-unverified`), cliente directo `cli` bajo un agente (S-01, M4), 50 conexiones (SEC-MCP-03, US-MCP-009), repo de otro uid (lo cubre `mcp_status_full`) y respuesta de 3.000 archivos (éxito acotado, no rechazo).
- Rechazo del tope de 1 MiB y 32 niveles, cuando `raptor-mcp` lo aplique.
- Casos de `safe_commit`, `safe_rebase`, `create_worktree`, `explain_history`, `check_conflicts`, `undo` y `acknowledge`, con la historia dueña de cada herramienta.
- Fuzzing, el crate `jsonschema`, un test de nextest por caso y migrar `mcp_allowlist.rs` y `mcp_snapshot.rs` al runner.

## Decisiones del orquestador (2026-10-09)

1. **Escáner propio frente a gitleaks.** Validada por Arquitecto y aprobada por el coordinador. Los canarios tienen forma de secreto real (tokens de GitHub, GitLab, Slack y Anthropic, clave de AWS, cabecera de clave privada, contraseña en la URL de un remoto) y se siembran en repo, config de git y entorno. Prueba que no se filtra lo plantado ni una forma de token conocida; no prueba la ausencia de secretos no plantados, que queda para SEC-MCP-12 con gitleaks. Enmienda fechada en ADR-MCP-001 y en SEC-MCP-08.
2. **`known_gap`.** Campo opcional del caso con la historia que cierra el hueco (hoy, el tope de entrada: dueña US-MCP-005). `Outcome::KnownGap(ref)`: el informe lo cuenta aparte, fuera del KPI y del denominador, y lo muestra junto a él ("100 % de N; M huecos conocidos"). Si el caso pasa a rechazarse, la suite falla hasta quitar la marca. No relaja el contrato, no es riesgo aceptado y bloquea el corte de v0.1.0 (DEP-MCP-8) salvo excepción con ADR. No se toca `apps/mcp`.
3. **XP-42.** Nivel `engine` en Windows, UNC como cwd y primera ejecución verde del nivel `engine` en Ubuntu. Se toma XP-42 porque XP-40 y XP-41 están ocupados por otros PR abiertos; si al rebasar otro lo toma, el siguiente libre.
4. **Workflow no obligatorio.** `mcp-security-corpus.yml` corre solo en Ubuntu y no es check obligatorio de `main`; lo decide Rene.

## Estado de la implementación

Implementado en: PR #224.

Parcial. Implementados el arnés, el formato del caso, el gate de CI y los casos transversales de `status` y `snapshot`, ámbito, entorno, JSON-RPC y rate limit. Pendientes los casos de las herramientas futuras y los huecos listados en el estado del corpus de la historia. Los pendientes de Linux y Windows están en [`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md), fila XP-42.

## Enmiendas

| Fecha | Origen | Qué cambia | Ids |
|---|---|---|---|
| 2026-10-09 | Decisión del orquestador, validada por Arquitecto y aprobada por el coordinador | Escáner propio de canarios en lugar de gitleaks; marca `known_gap`; XP-42 | ADR-MCP-001 § 9, SEC-MCP-08, SEC-MCP-12 |
