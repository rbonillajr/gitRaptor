---
id: DS-US-CKP-022
title: "Dev Spec — US-CKP-022: ver crecer la rama de cada agente sobre la base"
type: dev-spec
status: spec-ready
created: 2026-10-28
updated: 2026-11-10
story: US-CKP-022
feature: cockpit
domain: GRP
scope: fullstack
frontend_surface: true
stack: rust
profile: backend-service
tooling: [cargo]
related:
  context: ../context.md
  story: ../user-stories/US-CKP-022-grafo-carriles.md
  adrs: [ADR-CKP-003, ADR-GRP-001, ADR-GRP-005, ADR-GRP-009, ADR-GRP-013, ADR-GRP-016]
  api_spec: null
  design_spec: null
  contracts: [./US-CKP-012-ver-diff.md]
must_read:
  - ../user-stories/US-CKP-022-grafo-carriles.md
  - ../business-rules.md
  - ./US-CKP-012-ver-diff.md
  - ./US-CKP-005-terminal-pequena-sin-color.md
  - ./US-CKP-001-flota-en-vivo.md
  - ./US-CKP-003-estados-motor-conexion.md
  - ./TS-CKP-005-biblioteca-componentes-tui.md
  - ../../../../architecture/decisions/ADR-CKP-003-arquitectura-tui.md
  - ../../../../architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md
  - ../../../../architecture/decisions/ADR-GRP-009-frontera-solo-lectura-git.md
  - ../../../../architecture/decisions/ADR-GRP-013-modelo-eventos-atribucion.md
  - ../../../../architecture/decisions/ADR-GRP-016-extension-registro-capacidades.md
  - ../../../../architecture/extender-sin-archivos-compartidos.md
  - ../../../../design-system/README.md
  - ../../../../../crates/api/src/messages.rs
  - ../../../../../crates/api/src/methods/mod.rs
  - ../../../../../crates/api/src/untrusted.rs
  - ../../../../../crates/api/src/methods/query.rs
  - ../../../../../crates/api/src/methods/diff.rs
  - ../../../../../crates/api/src/query.rs
  - ../../../../../crates/api/src/diff.rs
  - ../../../../../crates/core/src/channel/conn.rs
  - ../../../../../crates/core/src/channel/query.rs
  - ../../../../../crates/core/src/channel/diff.rs
  - ../../../../../crates/core/src/channel/server.rs
  - ../../../../../crates/core/src/channel/mod.rs
  - ../../../../../crates/core/src/observe.rs
  - ../../../../../crates/core/tests/diff_query.rs
  - ../../../../../crates/core/src/daemon/mod.rs
  - ../../../../../crates/git/src/reader.rs
  - ../../../../../crates/git/src/guard_paths.rs
  - ../../../../../apps/cli/src/client/mod.rs
  - ../../../../../apps/cli/src/client/tests/diff.rs
  - ../../../../../apps/cli/src/tui/app.rs
  - ../../../../../apps/cli/src/tui/diff.rs
  - ../../../../../apps/cli/src/tui/view/diff_panel.rs
  - ../../../../../apps/cli/src/model.rs
  - ../../../../../apps/cli/src/model/agent_colors.rs
  - ../../../../../apps/cli/src/present/ingest.rs
  - ../../../../../apps/cli/src/tui/update.rs
  - ../../../../../apps/cli/src/tui/view.rs
  - ../../../../../apps/cli/src/tui/keymap.rs
  - ../../../../../apps/cli/src/tui/fleet_order.rs
  - ../../../../../apps/cli/src/tui/widgets/graph_lanes.rs
  - ../../../../../apps/cli/src/tui/widgets/layout.rs
  - ../../../../../apps/cli/tests/live_fleet.rs
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
tags: [cockpit, tui, grafo, carriles, consultas-bajo-demanda, n8, dep-ckp-2, solo-lectura]
---

# DS-US-CKP-022 · Ver crecer la rama de cada agente sobre la base

## Contexto rápido

Al terminar, el desarrollador ve bajo la flota un carril por rama de agente, con su color y su ↑↓ frente a la base confirmada, que se actualiza solo. Con `g` abre el grafo a pantalla completa: por carril, los 50 commits más recientes desde su merge-base, cada uno con su hash corto, su asunto, su hora y "sin atribuir", y "22 más" para el resto. Si la base no existe, no hay carriles y la vista lo dice. En 80×24 el panel del grafo es lo primero que desaparece, y `g` sigue funcionando. Hoy no puede: `view.rs:100-103` llama a `layout` con `BodyPlan { alerts, ..BodyPlan::default() }` (`graph: false`), el widget `GraphLanes` (`tui/widgets/graph_lanes.rs`, solo lo usa la galería) no tiene quien lo alimente y el protocolo no tiene consulta de commits de una rama (DEP-CKP-2).

Es solo lectura: no hay operación del catálogo, ni Guardrails, ni snapshot. La consulta reutiliza las piezas de consultas bajo demanda que US-CKP-012 dejó en `main` (#360; N8 de ADR-CKP-003 § 4): `QueryPool`, `QueryTable` y `QueryToken` con el ayudante `QueryTable::run` (`crates/core/src/channel/query.rs`), `query.cancel` con `query-busy` y `query-cancelled` (`crates/api/src/methods/query.rs`), `Link::submit`/`offers`, `Incoming::Reply` y la tabla `Queries` del hilo del canal (`apps/cli/src/client/mod.rs:338-470`) y `Cmd::CancelQuery`. Ninguna se redefine aquí; `channel/diff.rs` (`start`, `:114-155`) es el modelo que se copia.

**Revisión 2026-11-10: validada por Arquitecto contra `main` tras US-CKP-012 (#360).** Cambios: G1, G2 y N3 cerrados; referencias `archivo:línea` actualizadas; T003 y T004 reescritas con la forma real del ayudante de `query.rs`, de `LinkCmd::Diff`/`Query` del hilo del canal y de `DiffFailure`; T006 pinta la vista en `tui/view/graph_panel.rs` como `diff_panel.rs`; `.since(CAPABILITIES_PROTOCOL)` como `diff.worktree`; decisiones D14 a D20 nuevas; contrato revisado para el cliente TS de US-DSK-005 (sin choques de nombres en `main`); `blocked_tasks` retirado y `gaps_blocking: 0`.

El repo no tiene `architecture-constitution.md` en la cascada. Las restricciones activas son las de [AGENTS.md](../../../../../AGENTS.md) y los ADRs: `unsafe_code = forbid` (ADR-GRP-001), métodos en el archivo de su módulo y capacidades aditivas (ADR-GRP-016), lecturas solo con `gix` sin filtros (ADR-GRP-009 § 1) y texto del repo como `Untrusted` hasta `SafeText` (ADR-CKP-003 § 8). Ninguna tarea las viola.

| Término | Qué es aquí |
|---|---|
| Carril | La rama de un worktree dibujada desde su merge-base con la base confirmada (glosario de [context.md](../context.md)) |
| Panel del grafo | La región del cuerpo con un carril por fila, alimentada por el stream; colapsa primero |
| Vista del grafo | La pantalla completa que abre `g`, alimentada por la consulta `graph.lanes` |
| Ventana | Los commits de un carril que se muestran: los 50 más recientes (S-CKP-2) |

### Decisiones

Revisión del Arquitecto y del PO (2026-10-28). D1 a D3, D6 a D8 y D10 a D13: **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**; D12 sigue como ⚠️ supuesto medible (N4). D4, D5 y D9: **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)**. D14 a D20 (revisión contra `main`): **Decisión del orquestador (2026-11-10), validada por Arquitecto**.

| # | Decisión | Alternativa descartada y por qué |
|---|---|---|
| D1 | **Dos niveles.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. El *panel* del cuerpo pinta un carril por fila con lo que ya publica el stream: rama, sesión y `DivergenceView` (`messages.rs:267`). No hace consulta y es en vivo por construcción. La *vista* a pantalla completa (`g`) pide los commits con `graph.lanes`. El panel es la región que cede espacio (D10); la vista no depende del tamaño | Un solo nivel con la consulta: el panel tendría que pedir commits con cada evento de cualquier agente, y en 80×24 no habría dónde mirar un commit |
| D2 | **DEP-CKP-2 no existe en `main`: se crea.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. El lector solo cuenta commits (`ahead_behind_commits`, `reader.rs:603`) y ningún método del protocolo los lista (comprobado el 2026-11-10: ni `graph.` ni `GraphLanes` en `crates/` ni en `apps/`, salvo el nombre de la galería). Módulo `graph` con el método `graph.lanes` en `crates/api/src/methods/graph.rs` y sus tipos en `crates/api/src/graph.rs`. Método nuevo = capacidad aditiva que el cliente descubre en `hello.methods` (ADR-GRP-016 § 1); ninguna forma existente cambia. `mcp: false`. **Sin bloque de errores propio**: todo lo "no calculable" va en el resultado (D5, D3), y la cancelación y el exceso de consultas son los códigos del módulo `query` de DS-US-CKP-012 D1 | Un código `graph-unavailable`: tomaría el bloque `-33180` de la lista compartida por un caso que el resultado ya expresa |
| D3 | **Qué es un carril.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. Cada worktree del repo, salvo el que tiene sacada la rama base (ese es el tronco). Rama → carril con su nombre; `HEAD` separado → carril con el nombre de la carpeta; rama sin commits → carril "sin commits"; worktree no disponible → carril con su motivo y sin commits. Mismo orden que la flota (`fleet_order::order_key`, DS-US-CKP-002), así cada carril queda a la altura de su fila | Solo worktrees de agentes: una rama humana que se aleja también decide cuándo integrar, y la historia pide "un carril por worktree" |
| D4 | **Commits de un carril.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)**. `rev_walk([HEAD]).with_hidden([punta de la base])`, del más reciente al más antiguo por fecha de commit. El recorrido sale de una función privada nueva `only_in_walk(tip, hidden)` en `reader.rs`, que pasan a usar `count_only_in` (`reader.rs:620-640`) y `lane`: el total de la vista sale del mismo código que el ↑ del stream, con la misma política de `commit-graph`. Esa política es la que hoy tiene `count_only_in` (por defecto); pasarla a `use_commit_graph(false)`, como hace `guard_paths.rs:207`, cambiaría las dos a la vez y queda fuera de esta historia. Los primeros 50; el total se sigue contando hasta `MAX_DIVERGENCE_WALK` (10.000, `messages.rs:248`) con `exact`. El merge-base que se muestra es el primero de `merge_bases` (`reader.rs:571`), como en `worktree_diff` (`crates/git/src/diff.rs:240`). `count_only_in` recorre en el orden por defecto de `gix`; `lane` añade el orden por fecha de commit sobre el mismo `only_in_walk`, que no cambia el conjunto contado | Un recorrido propio en `lane` con otra política de `commit-graph`: el total de la vista podría no coincidir con el ↑ del panel. Orden topológico con líneas de merge: es un cliente Git completo, que BR-CKP-CALC-004 excluye |
| D5 | **Base.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)**. Solo la base confirmada (BR-CKP-CALC-004). El nombre y si está confirmada salen del `BaseBranchView` del repo (`messages.rs:241`) en el mismo estado del motor que publica `engine.snapshot` (`ctx.bus.snapshot()`, como `resolve` de `channel/diff.rs:69-111`): panel y vista no pueden discrepar (`repo_base`, `daemon/mod.rs:1390`, es privada de `daemon` y alimenta ese estado). Sin confirmar → sin carriles y "Base main sin confirmar: confirma la rama base para ver los carriles", con la acción "confírmala con `raptor guard confirm`" (Q-CKP-49). Base inválida → "Sin rama base: no calculable". Rama base sin ref → "Rama base develop no encontrada: no calculable". Nunca se usa otra rama (BR-CKP-EDGE-001). El panel aplica la misma regla con lo que publica el stream (`DivergenceView::BaseMissing`, `NoBase` y `BaseStatusView`) | Dibujar contra la base resuelta sin confirmar: el ↑ mezclaría commits de una rama que el desarrollador no eligió, y DS-US-CKP-012 D5 ya trata la base sin confirmar como "no calculable". Leer la base con `repo_base` desde el canal: es privada y abriría una segunda fuente |
| D6 | **Datos de cada commit.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. `oid` (40 hex; la TUI muestra 7), `summary` (primera línea del mensaje, hasta 256 bytes, como `Untrusted`), `committed_utc_ms` y `merge` (dos o más padres). Ni autor ni correo: la flota se mira en pantalla compartida (`model.rs:457`) y la autoría declarada tiene su capacidad (`events.authorship`, ADR-GRP-013 Enmienda). Sin campo de actor (D7) | Sin asunto: un carril de hashes no deja reconocer un commit; el asunto ya se lee con `git log` en el mismo worktree y `graph.lanes` no sale al MCP |
| D7 | **Actor por commit: "sin atribuir" en el MVP.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. La relación commit→evento está "pendiente de motor-local" (ADR-GRP-013, línea de DEP-CKP-2). La respuesta no lleva actor y la vista escribe "sin atribuir" en cada commit. Cuando motor-local la publique, entra como campo opcional `actor` detrás de la capacidad `graph.commit-actor` (aditiva, ADR-GRP-016 § 1) sin tocar el resto (Q-CKP-50) | Unir ya los commits con los eventos `commit` por `new_commit` (`messages.rs:614`): un rebase o un `--amend` solo deja evento de la punta, y los intermedios saldrían sin atribuir sin decir por qué; esa decisión es de motor-local |
| D8 | **Color por carril, nunca por commit.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. El del primer agente de la sesión del worktree, con el mismo `AgentColors` (`model/agent_colors.rs`) y la misma elección que la fila de la flota (`model.ui.colors.index` del primer agente, `view.rs:658`). Sin sesión → `LaneOwner::Unattributed` en tenue con "carpeta · sin agente". La paleta es `agent.1`…`agent.8` de DSYS-GRP-001 (Okabe-Ito, variante clara y oscura) | Un color por commit según su actor: con D7 todos serían tenues y se perdería quién lleva cada rama |
| D9 | **Vista en vivo, con coste acotado.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)**. (1) Abierta, la vista se marca desactualizada solo con un `git.event` del repo o con un cambio en el stream de rama, divergencia, base o conjunto de worktrees; un snapshot sin esos cambios no cuenta. El stream no lleva el oid de `HEAD` (`HeadView`, `messages.rs:305`), así que un `--amend` solo se ve por `git.event`, que se queda como disparador. (2) Desactualizada, se vuelve a pedir si no hay otra en vuelo y pasó `max(REFRESH_MIN, 2 × duración de la última respuesta)` desde la última petición: la vista no ocupa más de ~50 % del pool de un hilo de DS-012 D4. (3) El daemon guarda en `channel/graph.rs` una caché por carril con clave `(oid de HEAD, punta de la base)`, acotada por D12 (≤ 560 KiB por repo): un refresco solo recorre los carriles que cambiaron. Mientras llega, se ve la anterior con "actualizando…". El desplazamiento se ancla al primer commit visible (`oid`); si desapareció, a la cabecera de su carril | La foto de DS-US-CKP-012 D9: la historia es "ver crecer la rama". Refrescar cada 1 s sin caché ni espaciado: con consultas de hasta 2 s satura el pool de un hilo y el `diff.worktree` de otra TUI recibe `query-busy` |
| D10 | **Espacio** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. (BR-CKP-EDGE-005). `view` pasa `BodyPlan { alerts, graph: true, .. }` cuando el repo tiene al menos un carril o un mensaje de base; `alerts` sigue siendo la altura del panel de conflictos (`view.rs:97-102`, US-CKP-006/008). `layout` no cambia (DS-US-CKP-005 D2, `layout.rs:92-99`): el grafo es la primera región que cede. Sin región, la pista `g grafo` sigue en la barra y abre la vista a pantalla completa | Mostrar siempre una línea mínima del grafo: en 80×24 con alertas se la quitaría a la lista |
| D11 | **Tecla `g`** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. (libre en `keymap.rs:84-145`, comprobado el 2026-11-10: `q`, `Ctrl-c`, `r`, `k`/`j`, `y`/`s`, `n`, `?`, `f` y `d` están tomadas; `m`, `u` y `o` las reservan otras DS de cockpit). Abre y cierra la vista; `Esc` también la cierra; `↑`/`↓` (`k`/`j`) desplazan una línea; `q` sale de la TUI. Con la vista abierta, las teclas de fila no actúan. Motor sin `graph.lanes` → nota "el motor no ofrece el grafo; reinícialo" y ningún `Cmd` | Reutilizar `Enter`: abre el repo en el selector (DS-US-CKP-012 D11) |
| D12 | **Topes** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. (⚠️ **ASSUMPTION**, § Valores numéricos). 32 carriles (`lanes_total` y `cut: lanes`), 50 commits por carril, plazo de 2 s (el de DS-US-CKP-012 D4), 256 bytes de asunto, respuesta ≤ 768 KiB. Un carril que no termina en plazo sale `too-slow`; un tope nunca es un error. Peor caso: 32 × 50 × ~350 B ≈ 560 KiB | Error al pasar un tope: el desarrollador no vería nada justo con la flota grande |
| D13 | **"N más" no se despliega en el MVP.** **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO**. El carril termina en una fila tenue "22 más"; con total no exacto, "al menos 9.950 más". No hay paginación (Q-CKP-51) | Paginar con cursor: otro parámetro y otro estado en la TUI para un caso que la historia no pide |
| D14 | **`graph.lanes` se declara `method(GRAPH_LANES, false, false).since(CAPABILITIES_PROTOCOL)`**, igual que `diff.worktree` y `query.cancel` (`methods/diff.rs:18`, `methods/query.rs:22`) y los 24 métodos nuevos de `main`. **Decisión del orquestador (2026-11-10), validada por Arquitecto** | Así no aparece en `hello.methods` de un cliente de protocolo 5 a 8, que no conoce las consultas bajo demanda | Sin `.since(...)`, como decía T001 y la guía `extender-sin-archivos-compartidos.md:43`: la guía va por detrás del código (fuera de alcance de esta rama; se anota en el PR) |
| D15 | **N3: enum propio `GraphFailure { Busy, Cancelled, Lost, Refused }`** en `model.rs`, junto a `DiffFailure` (`model.rs:236`), y su función `graph_failure(code)` en `client/mod.rs` (copia de `diff_failure`, `:391`, sin la rama de `DIFF_UNAVAILABLE`). `DiffFailure` no se toca. **Decisión del orquestador (2026-11-10), validada por Arquitecto** | `graph.lanes` no tiene código propio (D2): `Unavailable(DiffUnavailable)` no le aplica. Con dos consultas, cuatro variantes repetidas cuestan menos que reescribir `tui/diff.rs` y sus pruebas en una rama ajena; la tercera consulta (US-CKP-021) extrae el `QueryFailure` común | Generalizar `DiffFailure` a `QueryFailure`: toca el código y los tests de US-CKP-012 sin cambio de comportamiento. Reutilizar `DiffFailure`: la TUI tendría que tratar un `Unavailable` imposible |
| D16 | **Nombres sin choque.** En `crates/git` el motivo es `GitLaneGap { NoMergeBase, TooSlow }` (no `LaneGap`), porque `channel/graph.rs` importa a la vez `gitraptor_api::graph::LaneGap`; igual que `GitLane` y `GitLaneCommit`. Los tipos del contrato no chocan con nada de `main`: el widget usa `GraphModel`, `LaneModel`, `LaneOwner` y `Base` (`graph_lanes.rs:14-41`). **Decisión del orquestador (2026-11-10), validada por Arquitecto** | Un alias en cada `use` es fácil de olvidar y confunde al leer | Mantener `LaneGap` en los dos crates |
| D17 | **Caché de D9 como campo de `ServerCtx`** (`channel/server.rs:114`, junto a `query`): `pub graph_cache: super::graph::LaneCache` y su `Default` en la construcción (`:244`). **Decisión del orquestador (2026-11-10), validada por Arquitecto** | Es donde ya vive el `QueryPool` de DS-012 y muere con el canal; dos líneas en un archivo de construcción | Un `static` en `graph.rs`: los tests de `crates/core` levantan varios daemons en el mismo proceso y compartirían caché |
| D18 | **Plazo inyectable en pruebas: se reutiliza `ChannelConfig.test_diff_deadline`** (`channel/mod.rs:126-131`, solo `debug_assertions`) como plazo de toda consulta bajo demanda; T003 cambia solo su comentario ("the deadline of on-demand queries"), no su nombre. **Decisión del orquestador (2026-11-10), validada por Arquitecto** | Provoca `too-slow` en `graph_query` con pocos milisegundos, como `diff_query.rs:78`, sin un campo nuevo en una configuración compartida | Un campo `test_graph_deadline`: otra línea en `ChannelConfig` y en su `Default` por la misma necesidad |
| D19 | **Repo desconocido o no listo**, con la regla de `resolve` de `channel/diff.rs:76-87`: `repo_id` mal formado → `INVALID_PARAMS`; desconocido con el motor en `Observing` → `NOT_FOUND`; desconocido antes de `Observing`, o repo no `Observed` o sin worktrees → resultado con `base: not-computable {repo-unavailable}`. La validación de `repo_id` es la de `channel/diff.rs:51` (`valid_repo_id` pasa a `pub(super)` y `graph.rs` la llama). **Decisión del orquestador (2026-11-10), validada por Arquitecto** | Misma respuesta que `diff.worktree` para el mismo repo; sin código propio (D2) | Responder `NOT_FOUND` siempre: la TUI que arranca antes que el motor vería un rechazo por un repo que está a punto de aparecer |
| D20 | **Vista y diff son excluyentes.** La vista del grafo se pinta en `tui/view/graph_panel.rs` (como `view/diff_panel.rs`) y `fleet_shown` (`view.rs:231`) exige también `ui.graph.is_none()`. Con el diff abierto, `g` no hace nada; con el grafo abierto, `d` tampoco (las teclas van primero a la vista abierta, como `update.rs:118-125` con el diff). **Decisión del orquestador (2026-11-10), validada por Arquitecto** | Una sola vista a pantalla completa a la vez; el panel del grafo y el de conflictos siguen conviviendo en la flota | Abrir el grafo encima del diff: dos peticiones en vuelo de la misma conexión y `query-busy` seguro (1 por conexión) |

## 📋 Índice

> **Para aprobar:** [Contexto rápido](#contexto-rápido) · [⚠️ Gaps](#️-gaps-y-violaciones-de-la-constitución) · [🔭 La forma](#-la-forma) · [El trabajo de un vistazo](#el-trabajo-de-un-vistazo).
> **Para implementar:** [🚀 Plan](#-plan-de-implementación), en orden. Las secciones `_(ref)_` se abren desde la tarea que las cita.

| Sección | Propósito |
|---------|-----------|
| [Contexto rápido](#contexto-rápido) | Qué se construye, por qué y las decisiones |
| [⚠️ Gaps](#️-gaps-y-violaciones-de-la-constitución) | Qué impide empezar |
| [🔭 La forma](#-la-forma) | Piezas y flujo |
| [🚀 Plan de implementación](#-plan-de-implementación) | T001…T007 |
| [Estructura de ficheros](#estructura-de-ficheros) _(ref)_ | Qué se crea y qué se toca |
| [Contratos compartidos](#contratos-compartidos) _(ref)_ | Tipos y firmas |
| [Contrato de API](#contrato-de-api) _(ref)_ | Método, cable, errores y topes |
| [Estrategia de pruebas y cobertura](#estrategia-de-pruebas-y-cobertura) _(ref)_ | Escenario → test |
| [Gate de seguridad](#gate-de-seguridad) | Texto no confiable, solo lectura, MCP |
| [Cómo extender](#cómo-extender) | Actor por commit, más datos por commit |
| [Enmiendas que implica](#enmiendas-que-implica) | ADR-CKP-003 N8 y DEP-CKP-2 |
| [Fuera de alcance](#fuera-de-alcance) | Lo diferido y su dueño |
| [Notas del autor](#notas-del-autor) _(ref)_ | Riesgos y preguntas abiertas |

---

## ⚠️ Gaps y violaciones de la constitución

| ID | Qué falta | Severidad | Alcance | Acción | Owner |
|----|-----------|-----------|---------|--------|-------|
| ~~G1~~ | ~~Dependencia de orden con DS-US-CKP-012~~ **Resuelto (2026-11-10)**: US-CKP-012 está en `main` (#360) con `QueryPool`, `QueryTable` y `QueryTable::run` (`channel/query.rs`), `query.cancel`, `query-busy` y `query-cancelled` (`methods/query.rs`), `Link::submit`/`offers`, `Incoming::Reply`, la tabla `Queries` y `LinkCmd::CancelQuery` (`client/mod.rs`) y `Cmd::CancelQuery` (`model.rs:677`) | Informativo (resuelto) | — | — | — |
| ~~G2~~ | ~~Alertas sin cablear en la vista~~ **Resuelto (2026-11-10)**: US-CKP-006 y US-CKP-008 están implementadas y `view.rs:97-102` ya pasa `alerts` con la altura del panel de conflictos (`view/conflict_panel.rs`, `layout::ALERTS_MAX`). El reparto con alertas lo prueba `layout` (`the_body_goes_by_priority_at_the_reference_sizes`, `layout.rs:300`); T006 añade `tui::view::tests::graph::the_graph_yields_before_the_conflict_panel_at_80x24` con un `PredictionModel` de prueba. El escenario 5 de punta a punta sigue comprobando lista y grafo (provocar una predicción en el arnés no aporta) | Informativo (resuelto) | — | — | — |
| G3 | **Perfil por solicitante** (S-01): el canal ya filtra por perfil (`offered`, `conn.rs:841-846`: `profile == Full` o `m.mcp`), así que `mcp: false` cierra `graph.lanes` a `raptor-mcp`. Sigue el residuo de DS-012 G1: una TUI lanzada por un agente es una conexión `Full` y ve el asunto de los commits; el agente ya los lee con `git log` en el worktree, y Q-CKP-8 solo prohíbe el MCP | Informativo | — | Ninguna en esta DS | — |

Nada bloquea: T001 a T007 se pueden construir ya. Todo lo que se reutiliza existe en `main` (verificado el 2026-11-10 sobre `905c66b5`): `rev_walk` con ocultos (`reader.rs:628-629`, `guard_paths.rs:206-207`), `merge_bases` (`reader.rs:571`), `resolve_ref` (`reader.rs:499`), `Count` (`reader.rs:148`), `observe::open_worktree` (`crates/core/src/observe.rs:617`), `repo_base`, el widget `GraphLanes`, `layout`, `AgentColors`, `Glyphs` de carril (`style.rs:38-41`), `Text::Ago` (`present/i18n.rs:89`) y las piezas de consulta de G1.

---

## 🔭 La forma

```mermaid
sequenceDiagram
    autonumber
    participant U as update
    participant H as hilo del canal (TUI)
    participant L as lector de la conexión
    participant P as QueryPool (query.rs)
    participant G as crates/git lane
    Note over U: panel: carriles desde el stream (T006), sin consulta
    U->>H: Cmd::Graph {ticket, repo_id} al pulsar g (T005)
    H->>L: link.submit graph.lanes {repo_id}, id 57; Queries guarda (ticket, 57, Query::Graph) (T004)
    L->>L: graph::start: valida, resuelve base confirmada y worktrees, queries.register (T003)
    L->>P: ctx.query.submit(job) (T003)
    P->>G: dentro de QueryTable::run: lane(head, base_tip, topes, stop) por carril (T002)
    G-->>P: merge-base, 50 commits, total
    P-->>H: QueryTable::run empuja la respuesta id 57 al Outbox (T003)
    H->>U: route → Incoming::Reply → Query::Graph.answer → EngineMsg::Graph {ticket, result} (T004)
    U->>U: GraphState listo, vista a pantalla completa (T005, T006)
    loop mientras la vista está abierta
        Note over U: git.event o cambio en el stream → desactualizada, espaciado de D9 → Cmd::Graph
    end
    opt Esc o g con una petición en vuelo
        U->>H: Cmd::CancelQuery {ticket} (DS-012)
    end
```

---

## 🚀 Plan de implementación

### El trabajo de un vistazo

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Escribir en rojo las pruebas de la historia y los stubs del contrato | — | `crates/api/src/{graph,methods/graph}.rs`, `apps/cli/tests/tui_graph.rs`, `crates/core/tests/graph_query.rs` |
| T002 | Leer los commits de un carril con `gix`, sin filtros y con topes | T001 | `crates/git/src/lane.rs` |
| T003 | Servir `graph.lanes` desde el pool de consultas | T001, T002 | `crates/core/src/channel/{graph,mod,conn,server,diff}.rs` |
| T004 | Pedir y cancelar el grafo desde el hilo del canal de la TUI | T001 | `apps/cli/src/client/mod.rs`, `client/tests/graph.rs`, `tui/app.rs` |
| T005 | Añadir la tecla `g`, el estado de la vista y su refresco | T001 | `apps/cli/src/tui/{graph,update,keymap}.rs`, `model.rs`, `present/{ingest,i18n}.rs` |
| T006 | Pintar el panel y la vista del grafo con textos en/es | T005 | `apps/cli/src/present/{graph,graph_text}.rs`, `tui/{view,help}.rs`, `tui/view/graph_panel.rs` |
| T007 | Cerrar la historia | T003, T004, T006 | `docs` |

### En qué orden

T001 → {T002, T004, T005} → T003 (tras T002) y T006 (tras T005) → T007. T002/T003 (motor) y T004/T005/T006 (TUI) no comparten archivos y pueden ir en paralelo.

### T001 — Escribir en rojo las pruebas de la historia y los stubs del contrato

**Objetivo.** Los cinco escenarios de la historia, en rojo, contra tipos del contrato que ya compilan.

**Ubicación.**
- `crates/api/src/graph.rs` (**CREATE**): los tipos y las constantes de § Contratos compartidos, como `crates/api/src/diff.rs`
- `crates/api/src/methods/graph.rs` (**CREATE**): `GRAPH_LANES`, `GROUP` sin `error_block` ni `errors` (`..Group::new("graph")`)
- `crates/api/src/methods/mod.rs` (**MODIFY**): `mod graph;`, `pub use graph::*;` y `&graph::GROUP` en `GROUPS` tras `&diff::GROUP` (3 líneas, ADR-GRP-016)
- `crates/api/src/lib.rs` (**MODIFY**): `pub mod graph;` entre `discovery` y `guard` (la tabla de la guía dice "nada" para `lib.rs`, pero un módulo de tipos nuevo siempre ha entrado así: `diff`, `query`)
- `apps/cli/tests/tui_graph.rs` y `crates/core/tests/graph_query.rs` (**CREATE**; el segundo con el arnés de `crates/core/tests/diff_query.rs`)

**Reglas**
- `method(GRAPH_LANES, false, false).since(CAPABILITIES_PROTOCOL)`, como `diff.worktree` (D14).
- Arnés de `apps/cli/tests/live_fleet.rs` (`#![cfg(all(target_os = "macos", debug_assertions))]`, `gitraptor_testkit::Fixture`, `raptor-fake-agent`, `App` sobre `TestBackend`). Nunca este repo ni el perfil real (NFR-01).
- Datos de § 9.3.

- **Depende:** —
- **Refs:** US-CKP-022 escenarios 1 a 5; D2
- **Aceptación:** `cargo test -p gitraptor-api` en verde (`architecture.rs` incluido) y `cargo test -p gitraptor-cli --test tui_graph` compila y falla por "no implementado"

### T002 — Leer los commits de un carril con `gix`, sin filtros y con topes

**Objetivo.** `RepoReader::lane` devuelve el merge-base, la ventana de commits y el total de una rama frente a la base, sin escribir nada.

**Ubicación.**
- `crates/git/src/lane.rs` (**CREATE**): `LaneLimits`, `GitLane`, `GitLaneCommit`, `GitLaneGap` (D16), `impl RepoReader { pub fn lane(..) }`
- `crates/git/src/reader.rs` (**MODIFY**): `fn only_in_walk(tip, hidden)` `pub(crate)`, que devuelve la plataforma de `rev_walk([tip]).with_hidden([hidden])`; `count_only_in` (`:620-640`) pasa a usarla sin cambiar su resultado
- `crates/git/src/lib.rs` (**MODIFY**): `pub mod lane;` y una línea en el doc comment de la frontera ("[`lane`] lista los commits de una rama sobre la base, para el grafo")

**Reglas**
- Recorrido según D4 con `only_in_walk(head, base_tip)`, la misma que usa `count_only_in`, con orden por fecha de commit (`Sorting::ByCommitTime`), más reciente primero. Se guardan los primeros `max_commits`; el resto solo se cuenta hasta `max_walk` (`Count::Exact` o `Count::AtLeast`, `reader.rs:148`).
- `merge_base`: el primero de `merge_bases(base_tip, head)`, como `worktree_diff` (`diff.rs:240`). Vacío → `Ok(Err(GitLaneGap::NoMergeBase))`; no se recorre nada.
- Por commit: `summary` = bytes del mensaje hasta el primer `\n`, cortados a `max_summary_bytes` en límite de carácter UTF-8 tras `from_utf8_lossy`; `committed_s` de la firma del committer; `merge` = dos o más padres. Nunca autor ni correo (D6).
- `stop()` se mira cada 64 commits; si devuelve `true`, `Ok(Err(GitLaneGap::TooSlow))`: nunca una ventana parcial presentada como completa.
- Ningún `Command::new`: `crates/git/tests/static_check.rs` sigue en verde.

- **Depende:** T001
- **Refs:** D4, D6, D12; ADR-GRP-009 § 1
- **Aceptación:** `cargo test -p gitraptor-git --lib lane` y `cargo test -p gitraptor-git --test repo_intact`

### T003 — Servir `graph.lanes` desde el pool de consultas

**Objetivo.** El daemon responde los carriles de un repo a la conexión que los pidió, sin bloquear su lector ni el stream.

**Ubicación.**
- `crates/core/src/channel/graph.rs` (**CREATE**): `resolve`, `start` y `compute` con la forma de `channel/diff.rs`; `LaneCache` (D9, D17); mapeo a `gitraptor_api::graph`
- `crates/core/src/channel/mod.rs` (**MODIFY**): `mod graph;` junto a `mod diff;` (`:18`); comentario de `test_diff_deadline` (`:126-128`) según D18
- `crates/core/src/channel/conn.rs` (**MODIFY**): brazo `methods::GRAPH_LANES` en `dispatch` (`:882`), justo después del de `DIFF_WORKTREE` (`:994-1001`) y con su misma forma
- `crates/core/src/channel/server.rs` (**MODIFY**): campo `graph_cache` en `ServerCtx` (`:114`) y su `Default` (`:244`) (D17)
- `crates/core/src/channel/diff.rs` (**MODIFY**): `valid_repo_id` (`:51`) pasa a `pub(super)` (D19)

**Reglas**
- **El brazo** (copia de `:994-1001`): `if let Err(error) = super::graph::start(self.ctx, &self.queries, &self.outbox, request) { self.reply::<()>(&request.id, Err(error)); }`. Una conexión MCP no llega: `dispatch` responde `METHOD_NOT_FOUND` a lo no ofrecido (`conn.rs:898`, `offered` en `:841-846`). El cierre de la conexión ya cancela todo (`conn.queries.cancel_all()`, `:250`).
- **`start`** (copia de `diff::start`, `diff.rs:114-155`): `resolve(ctx, request)?` → `queries.register(&request.id).map_err(|b| b.error())?` (`query-busy` si ya hay una en vuelo) → `job` que llama a `table.run(&outbox, id, &token, |token| compute(&resolved, deadline, &|| token.is_cancelled()))` y escribe la línea de log → `ctx.query.submit(job)`; si devuelve `Busy`, `queries.finish(&request.id)` y `Err(busy.error())`. `run` ya responde `query-cancelled`, convierte un pánico en `INTERNAL "query failed"` y libera el hueco; `graph.rs` no los repite.
- **`resolve`** (en el hilo lector, solo datos propios para el `job`): `GraphParams` → `valid_repo_id` o `INVALID_PARAMS`; `ctx.bus.snapshot()` y el repo por `repo_id` según D19. Base del `BaseBranchView` del repo (D5): `status != Confirmed` → `BaseUnconfirmed {name}`, sin nombre → `NoBase`; si no, `RefName::new("refs/heads/<name>")`. Carriles según D3 a partir de `repo.worktrees` en el orden de `engine.snapshot` (la TUI los reordena): se excluye el worktree que tiene sacada la base; cada uno guarda `root`, `main` y `admin_name`, como `Resolved` de `diff.rs:57-67`. Más de 32 → los 32 primeros, `cut: lanes` y `lanes_total` con el total.
- **`compute`** (en el pool): `base_tip` con `resolve_ref` sobre el repo común; `None` → `BaseMissing {name}` y `lanes` vacío. Por carril: `crate::observe::open_worktree(common_dir, root, main, admin_name)` (sin seguir enlaces, `observe.rs:611-617`, como `diff.rs:162`); `ReadError::{Unavailable, Untrusted, NotARepository}` o `WorktreeStatus::Unavailable` en el estado → `worktree-unavailable`; `head()` sin commit → `no-commits`; si no, caché o `lane(head, base_tip, …)`; `GitLaneGap` → su motivo; otro `ReadError` → `unreadable`. Un `ReadError` al abrir el repo común → `INTERNAL "graph failed"`.
- **Caché** (D9, D17): `LaneCache` = `Mutex<HashMap<repo_id, (base_tip, HashMap<root, ((head, base_tip), GitLane)>)>>`; un acierto no recorre nada; un `base_tip` distinto vacía el repo; se poda a los carriles de la última respuesta, así nunca pasa de los topes de D12 (≤ 560 KiB por repo). No se persiste.
- **Plazo**: `GRAPH_DEADLINE_MS` (2 s) desde que empieza el trabajo; en `debug_assertions`, `ctx.config.test_diff_deadline` si existe (D18). Pasado, los carriles que faltan salen `too-slow`. Tamaño serializado ≤ `MAX_GRAPH_RESPONSE_BYTES` (768 KiB, con el margen de sobre de `diff.rs:37`): si no cabe, se quitan commits del final de los carriles más largos y `cut: bytes`.
- La respuesta no pasa por el logger ni por la auditoría (como `diff.worktree`). La línea de log es `graph_lanes` con `repo`, `ms`, `lanes` y `cut`; nada más.

- **Depende:** T001, T002
- **Refs:** D2 a D5, D9, D12, D14 a D19; DS-US-CKP-012 D1 a D4
- **Aceptación:** `cargo test -p gitraptor-core --test graph_query`

### T004 — Pedir y cancelar el grafo desde el hilo del canal de la TUI

**Objetivo.** El hilo del canal envía `graph.lanes`, sigue con el stream y entrega la respuesta a `update`.

**Ubicación.**
- `apps/cli/src/client/mod.rs` (**MODIFY**), siempre junto a lo de `Diff`:
  - `LinkCmd::Graph { ticket, repo_id }` tras `LinkCmd::Diff` (`:174-179`)
  - `Query::Graph` en el enum `Query` (`:352-354`, su comentario ya prevé "a second kind of query"), con sus brazos en `Query::answer` (`:363-377`: `GraphLanes` o `graph_failure`) y `Query::lost` (`:380-387`: `GraphFailure::Lost`)
  - `graph_failure(code) -> GraphFailure` y `graph_msg(ticket, result)`, copias de `diff_failure` y `diff_msg` (`:391-411`) sin `DIFF_UNAVAILABLE` (D15)
  - En la sesión, el brazo `LinkCmd::Graph` tras el de `Diff` (`:602-630`): `link.submit(methods::GRAPH_LANES, to_value(&GraphParams { repo_id }))` → `queries.inflight.push(Inflight { ticket, id, query: Query::Graph })`; `Err(Lost)` → `GraphFailure::Lost` y fin de la sesión; otro error → `GraphFailure::Refused`
  - Sin conexión, el brazo `LinkCmd::Graph { ticket, .. }` junto a `LinkCmd::Diff` (`:310-315`) responde `GraphFailure::Lost`
  - Al empezar la sesión, `ConnEvent::Graph(link.offers(methods::GRAPH_LANES))` tras `ConnEvent::Diff` (`:503-505`)
- `apps/cli/src/client/tests/graph.rs` (**CREATE**) con el patrón de `client/tests/diff.rs`, y `mod graph;` junto a `mod diff;` (`client/mod.rs:1044`)
- `apps/cli/src/tui/app.rs` (**MODIFY**): `Cmd::Graph { ticket, repo_id }` → `LinkCmd::Graph` junto a `Cmd::Diff` (`:214-223`)

**Reglas**
- `route`, `deliver`, `Queries::take`, `link.settle` y `LinkCmd::CancelQuery` (`:418-470`, `:631-645`) no cambian: ya sirven a cualquier `Query`.
- Una reconexión entrega `Err(GraphFailure::Lost)` por cada petición en vuelo (lo hace `Query::lost` al cerrar la sesión).
- El hilo no sanea: entrega el contrato y la ingesta (T006) lo convierte en `SafeText`.

- **Depende:** T001
- **Refs:** D2, D15; DS-US-CKP-012 D13, T005
- **Aceptación:** `cargo test -p gitraptor-cli --lib client::tests::graph` (pide, responde, `query-busy`, `query-cancelled`, conexión perdida, motor sin el método)

### T005 — Añadir la tecla `g`, el estado de la vista y su refresco

**Objetivo.** El desarrollador pulsa `g` y `update` pide el grafo, lo mantiene al día mientras está abierto y lo cierra con `Esc` o `g`.

**Ubicación.**
- `apps/cli/src/tui/graph.rs` (**CREATE**): `GraphState`, `open`, `close`, `apply_reply`, `scroll`, `mark_stale`, `tick`
- `apps/cli/src/tui/mod.rs` (**MODIFY**): `pub mod graph;`
- `apps/cli/src/tui/keymap.rs` (**MODIFY**): `Action::Graph` con `Key::Char('g')` y `Text::KeyGraph`
- `apps/cli/src/model.rs` (**MODIFY**), cada pieza junto a su par de US-CKP-012: `Ui.graph: Option<GraphState>` (junto a `Ui.diff`, `:125`), `RepoView.base_confirmed: bool` (tras `base`, `:353`; hoy no existe), `EngineReplica.graph: bool` (tras `diff`, `:266`), `GraphFailure` (D15, tras `DiffFailure`, `:236`), `EngineMsg::Graph` (tras `EngineMsg::Diff`, `:605`), `ConnEvent::Graph(bool)` (tras `ConnEvent::Diff`, `:633`), `Cmd::Graph` (tras `Cmd::Diff`, `:671`), `Notice::GraphUnsupported` (tras `Notice::DiffUnsupported`)
- `apps/cli/src/present/ingest.rs` (**MODIFY**): `repo()` (`:53`) rellena `base_confirmed` con `snapshot.repo.base.status == BaseStatusView::Confirmed`
- `apps/cli/src/present/i18n.rs` (**MODIFY**): `Text::KeyGraph` y el texto de `GraphUnsupported`, en en y es
- `apps/cli/src/tui/update.rs` (**MODIFY**): `Action::Graph`; con `Ui.graph` abierto, `Up`/`Down`/`Later`/`Graph` van a `graph::scroll`/`graph::close`, como el bloque del diff (`:118-125`), y las demás teclas de fila (incluida `d`) no actúan (D20); `EngineMsg::Graph` a `graph::apply_reply` (como `:463`); `EngineMsg::{Snapshot, Event}` del repo a `graph::mark_stale` solo si es un `git.event` o cambian rama, divergencia, base o conjunto de worktrees (D9)

**Reglas**
- `g` sin repo en pantalla, o con `Ui.diff` abierto, no hace nada (D20). Motor sin `graph.lanes` (`EngineReplica.graph == false`) → `Notice::GraphUnsupported` y ningún `Cmd` (D11). Si no, `Ui.graph = Some(Loading { ticket })` y `Cmd::Graph`. El `ticket` sale de `model.ui.next_ticket`, el contador que usa `tui/diff.rs:60-61`, así un ticket nunca se repite entre diff y grafo.
- `Esc` o `g` con la vista abierta: `Ui.graph = None` y, si hay petición en vuelo, `Cmd::CancelQuery`.
- `mark_stale` marca `stale`. `tick` (en cada `update`, con el reloj monótono de la TUI) emite `Cmd::Graph` si `stale`, no hay petición en vuelo y pasó `max(REFRESH_MIN, 2 × last_duration)` desde la última (D9). `apply_reply` guarda `last_duration` (de la petición a la respuesta).
- `apply_reply`: un `ticket` que no es el actual se descarta; `Ok` sustituye el resultado y conserva el ancla de D9; `busy` deja `stale` para el siguiente `tick`; `lost` y `refused` muestran su mensaje y conservan el último resultado si lo hay.
- `q` sigue saliendo con la vista abierta.

- **Depende:** T001
- **Refs:** D9, D11, D15, D20; DS-US-CKP-012 D11, T006
- **Aceptación:** `cargo test -p gitraptor-cli --lib tui::graph`

### T006 — Pintar el panel y la vista del grafo con textos en/es

**Objetivo.** El panel en vivo en su región y la vista a pantalla completa de D1, con la pista `g` en la barra y en `?`.

**Ubicación.**
- `apps/cli/src/present/graph.rs` (**CREATE**): `panel(model, say) -> Option<GraphModel>` desde el stream; `rows(state, model, say) -> Vec<GraphRow>` desde `GraphLanes`
- `apps/cli/src/present/graph_text.rs` (**CREATE**): `GraphText`, `render(self, lang)` con `match` exhaustivo en/es y `every()`, el patrón de `present/prediction_text.rs`
- `apps/cli/src/present/mod.rs` (**MODIFY**): `pub mod graph; pub mod graph_text;`
- `apps/cli/src/tui/view.rs` (**MODIFY**): el `BodyPlan` de `:100-103` gana `graph` según D10 y conserva `alerts`; pinta `GraphModel` en `regions.graph`; `fleet_shown` (`:231`) exige `ui.graph.is_none()` y, con `Ui.graph` abierto, el cuerpo entero es la vista (D20); `mod graph_panel;` junto a `mod diff_panel;` (`:813`)
- `apps/cli/src/tui/view/graph_panel.rs` (**CREATE**): la vista a pantalla completa, con el patrón de `view/diff_panel.rs`
- `apps/cli/src/tui/help.rs` (**MODIFY**): `g grafo` en el grupo de la flota; `↑↓ desplazar · Esc cerrar` en el de la vista
- `apps/cli/src/tui/snapshots/graph_{panel_100x30,view_80x24}_{en,es,ascii}.snap` (**CREATE**)

**Reglas**
- **Panel**: un `LaneModel` por carril (D3) en el orden de la flota. `owner` según D8; `ahead`/`behind` de `DivergenceView::Counted` (con `exact: false`, la cifra con `+`); `conflict: false` (N5). `base`: `Base::Found(nombre)` con base confirmada; si no, `Base::Missing(mensaje de D5)` y `lanes` vacío. `GraphModel` y `graph_lanes.rs` no cambian.
- **Vista**: título "grafo de shop sobre main · a las 14:03:12" y, con `stale` o petición en vuelo, "actualizando…". Por carril, una cabecera con el glifo de carril en su color, "feat-pagos · claude-1" y "72 commits desde a1b2c3d"; luego una fila por commit: nodo en el color del carril, hash de 7, asunto, "sin atribuir" en tenue (D7) y la hora relativa con `Text::Ago`; `merge` → "(merge)". Al final, "22 más" o "al menos N más" en tenue (D13). Carril no calculable → su cabecera y el motivo ("no disponible", "sin commits", "sin merge-base con main", "tardó demasiado", "no se pudo leer"). `cut` → "truncado: más de 32 carriles" / "truncado: respuesta demasiado grande".
- Base no calculable: ningún carril, solo el mensaje de D5, exacto: "Rama base develop no encontrada: no calculable"; sin confirmar, "Base main sin confirmar: confirma la rama base para ver los carriles" y la acción con `raptor guard confirm`.
- Rutas, ramas y asuntos solo como `SafeText` (`SafeText::from_untrusted`, SEC-12). Cadenas enteras en el catálogo, sin concatenar frases. Solo tokens (`agent.n`, `git.branch.base`, `text.muted`, `status.warning`) y glifos de `Glyphs`.

- **Depende:** T005
- **Refs:** D1, D3, D5, D7, D8, D10, D13, D20; DSYS-GRP-001 § 3 (GraphLanes)
- **Aceptación:** `cargo test -p gitraptor-cli --lib tui::view::tests::graph` (incluye `the_graph_yields_before_the_conflict_panel_at_80x24`, G2), `cargo test -p gitraptor-cli --lib present::graph_text` y `cargo test -p gitraptor-cli --test tui_graph`

### T007 — Cerrar la historia

**Objetivo.** La historia apunta a lo implementado y esta DS pasa a `implemented`.

**Ubicación.** `docs/requirements/features/cockpit/user-stories/US-CKP-022-grafo-carriles.md` (**MODIFY**), esta DS (**MODIFY**) y las líneas de § Enmiendas que implica

**Reglas**
- La historia pasa a `implemented` solo con los cinco escenarios en verde.
- No se edita `docs/ARTIFACTS.md` ni el backlog a mano.

- **Depende:** T003, T004, T006
- **Refs:** N8; DEP-CKP-2
- **Aceptación:** `cargo fmt --all --check`, `cargo clippy --workspace --exclude gitraptor-desktop --all-targets -- -D warnings` y `cargo test --workspace --exclude gitraptor-desktop` en verde (AGENTS.md); el job `desktop` de CI no se ve afectado porque `apps/desktop` no cambia

---

## Estructura de ficheros

| Archivo | Acción | Tarea |
|---|---|---|
| `crates/api/src/graph.rs` | CREATE | T001 |
| `crates/api/src/methods/graph.rs` | CREATE | T001 |
| `crates/api/src/methods/mod.rs`, `lib.rs` | MODIFY (3 líneas y 1 línea) | T001 |
| `crates/git/src/lane.rs` | CREATE | T002 |
| `crates/git/src/reader.rs` | MODIFY (`only_in_walk`) | T002 |
| `crates/git/src/lib.rs` | MODIFY (1 línea `pub mod` y 1 de doc) | T002 |
| `crates/core/src/channel/graph.rs` | CREATE | T003 |
| `crates/core/src/channel/{mod,conn,server}.rs` | MODIFY (1 línea `mod` y 1 comentario, 1 brazo, 1 campo y su `Default`) | T003 |
| `crates/core/src/channel/diff.rs` | MODIFY (`valid_repo_id` a `pub(super)`) | T003 |
| `apps/cli/src/client/mod.rs`, `tui/app.rs` | MODIFY (`LinkCmd::Graph`, `Query::Graph` y sus brazos) | T004 |
| `apps/cli/src/client/tests/graph.rs` | CREATE | T004 |
| `apps/cli/src/tui/graph.rs` | CREATE | T005 |
| `apps/cli/src/model.rs`, `present/{ingest,i18n}.rs`, `tui/{mod,keymap,update}.rs` | MODIFY | T005 |
| `apps/cli/src/present/{graph,graph_text}.rs`, `tui/view/graph_panel.rs` | CREATE | T006 |
| `apps/cli/src/tui/{view,help}.rs`, `present/mod.rs` | MODIFY | T006 |
| `apps/cli/tests/tui_graph.rs`, `crates/core/tests/graph_query.rs` | CREATE | T001 |

---

## Contratos compartidos

### Tipos y datos compartidos

```rust
// crates/api/src/graph.rs (CREATE)
pub struct GraphParams { pub repo_id: String }
pub struct GraphLanes {
    pub computed_at_utc_ms: i64,
    pub base: GraphBase,
    pub lanes: Vec<Lane>,                 // vacío si la base no es Ready
    pub lanes_total: u32,
    pub cut: Option<GraphCut>,
}
pub enum GraphBase {
    Ready { name: UntrustedName },
    NotComputable { reason: BaseGap },
}
pub enum BaseGap {
    BaseUnconfirmed { name: UntrustedName },
    NoBase,
    BaseMissing { name: UntrustedName },
    RepoUnavailable,
}
pub enum GraphCut { Lanes, Bytes }
pub struct Lane {
    pub worktree: Untrusted,              // raíz, para casar con WorktreeRow.key en la TUI
    pub head: LaneHead,
    pub state: LaneState,
}
pub enum LaneHead { Branch { name: UntrustedName }, Detached }
pub enum LaneState {
    Ready { merge_base: String, commits: Vec<LaneCommit>, total: CommitCountView },
    NotComputable { reason: LaneGap },
}
pub enum LaneGap { WorktreeUnavailable, NoCommits, NoMergeBase, TooSlow, Unreadable }
pub struct LaneCommit {
    pub oid: String,                      // hex completo del oid (40 con SHA-1)
    pub summary: Untrusted,               // primera línea, ≤ 256 bytes
    pub committed_utc_ms: i64,
    pub merge: bool,
}

pub const MAX_GRAPH_LANES: u32 = 32;
pub const MAX_LANE_COMMITS: usize = 50;
pub const MAX_LANE_SUMMARY_BYTES: usize = 256;
pub const GRAPH_DEADLINE_MS: u64 = 2_000;          // el valor de DIFF_DEADLINE_MS
pub const MAX_GRAPH_RESPONSE_BYTES: usize = 768 * 1024;
```

Todos `Serialize, Deserialize, JsonSchema` con `deny_unknown_fields`; los enums con `rename_all = "kebab-case"`. `CommitCountView` es el de `messages.rs:287`. `Untrusted` y `UntrustedName` son los de `untrusted.rs`. Ningún tipo lleva actor, sesión, autor ni correo (D6, D7). La cola y el límite por conexión no se redeclaran: son `QUERY_QUEUE` y `QUERIES_PER_CONNECTION` de `crates/api/src/diff.rs:22-24`, que ya usa `channel/query.rs`.

**Para el cliente TS (US-DSK-005, verificado 2026-11-10).** El backend de Tauri deserializa `gitraptor_api::graph::GraphLanes` y lo traduce a su `GraphDto` en camelCase (`ingest::graph`, DS-US-DSK-005 T007): el contrato se consume desde Rust, no desde TS, así que basta con que los tipos sean `pub`, `Deserialize` y que `Untrusted::raw()` dé la ruta para `worktreeKey`. Los motivos casan uno a uno con los de DS-US-DSK-005 (`base-unconfirmed`, `no-base`, `base-missing`, `repo-unavailable`; `worktree-unavailable`, `no-commits`, `no-merge-base`, `too-slow`, `unreadable`) y `committed_utc_ms` (< 2^53) y `total.count` (≤ 10.000) caben en un `number`. Ni `graph`, ni `GraphLanes`, ni `Lane*` existen en `crates/api`, `crates/core` ni `apps/desktop` (el único `GraphLanes` de `main` es el nombre de la ficha de la galería de la TUI).

```rust
// crates/git/src/lane.rs (CREATE)
pub struct LaneLimits { pub max_commits: usize, pub max_walk: u64, pub max_summary_bytes: usize }
pub struct GitLane { pub merge_base: String, pub commits: Vec<GitLaneCommit>, pub total: Count }
pub struct GitLaneCommit { pub id: String, pub summary: String, pub committed_s: i64, pub merge: bool }
pub enum GitLaneGap { NoMergeBase, TooSlow }      // D16
impl RepoReader {
    pub fn lane(&self, head: &str, base_tip: &str, limits: &LaneLimits, stop: &dyn Fn() -> bool)
        -> Result<Result<GitLane, GitLaneGap>, ReadError>;
}
```

### Ciclos de vida (DI)

_No aplica — Rust sin contenedor de dependencias._ El trabajo de `graph.lanes` vive en el `QueryPool` de DS-012 mientras dura la consulta. `GraphState` vive en `Model.ui` mientras la vista está abierta; no se guarda entre ejecuciones. El panel se recalcula en cada `view` desde el modelo.

### Firmas del stack

```rust
// apps/cli/src/model.rs (MODIFY)
pub struct Ui { /* … */ pub graph: Option<GraphState> }
pub struct RepoView { /* … */ pub base_confirmed: bool }
pub enum Cmd { /* … */ Graph { ticket: u64, repo_id: String } }
pub enum EngineMsg { /* … */ Graph { ticket: u64, result: Result<Box<GraphLanes>, GraphFailure> } }
pub enum ConnEvent { /* … */ Graph(bool) }
pub struct EngineReplica { /* … */ pub graph: bool }
/// D15: el `DiffFailure` de `model.rs:236` sin `Unavailable`.
pub enum GraphFailure { Busy, Cancelled, Lost, Refused }

// apps/cli/src/tui/graph.rs (CREATE)
pub const REFRESH_MIN: std::time::Duration = std::time::Duration::from_secs(1);
pub struct GraphState {
    pub in_flight: Option<u64>,           // ticket
    pub last_request: Option<std::time::Instant>,
    pub last_duration: std::time::Duration,   // espaciado de D9
    pub stale: bool,
    pub shown: Option<Shown>,             // último resultado, ya saneado
    pub failure: Option<GraphFailure>,
    pub anchor: Anchor,                   // D9
}
pub enum Anchor { Top, Commit { lane: u64, oid: String }, Lane(u64) }
pub fn open(model: &mut Model) -> Vec<Cmd>;
pub fn close(model: &mut Model) -> Vec<Cmd>;
pub fn mark_stale(model: &mut Model);
pub fn tick(model: &mut Model, now: std::time::Instant) -> Vec<Cmd>;
pub fn apply_reply(model: &mut Model, ticket: u64, result: Result<Box<GraphLanes>, GraphFailure>);

// apps/cli/src/present/graph.rs (CREATE)
pub fn panel(model: &Model, say: Say) -> Option<GraphModel>;   // None: sin carriles ni mensaje
```

---

## Contrato de API

| Pieza | Nombre | Dónde | Notas |
|---|---|---|---|
| Método | `graph.lanes` | `crates/api/src/methods/graph.rs` (**CREATE**): `pub const GRAPH_LANES: &str = "graph.lanes"`, `method(GRAPH_LANES, false, false).since(CAPABILITIES_PROTOCOL)` (D14) | `GraphParams` → `GraphLanes`; respuesta asíncrona (DS-012 D2); `mcp: false`. **No se relaja sin presupuesto propio por solicitante** (nota del 2026-11-10, I-01 de la revisión de seguridad de DS-US-DSK-009): la cola global del daemon tiene 4 consultas, compartida con `diff.worktree`; ofrecerlo a `raptor-mcp` sin presupuesto propio dejaría que un agente la agotara |
| Cancelación | `query.cancel` | DS-US-CKP-012 | Sin cambio |
| Códigos | `query-busy`, `query-cancelled` | bloque de `query` (DS-012) | Sin códigos propios (D2) |
| Capacidad | — | — | Método nuevo: se descubre en `hello.methods`. El actor por commit futuro irá tras `graph.commit-actor` (D7) |

```json
→ {"jsonrpc": "2.0", "id": 57, "method": "graph.lanes", "params": {"repo_id": "r1"}}
← {"jsonrpc": "2.0", "id": 57, "result": {
    "computed_at_utc_ms": 1793091600000,
    "base": {"ready": {"name": {"untrusted": "main"}}},
    "lanes_total": 2,
    "lanes": [
      {"worktree": {"untrusted": "/w/feat-pagos"}, "head": {"branch": {"name": {"untrusted": "feat-pagos"}}},
       "state": {"ready": {"merge_base": "a1b2c3d…", "total": {"count": 72, "exact": true},
         "commits": [{"oid": "9f8e7d6…", "summary": {"untrusted": "Add payment retry"},
                      "committed_utc_ms": 1793091500000, "merge": false}]}}},
      {"worktree": {"untrusted": "/w/feat-login"}, "head": {"branch": {"name": {"untrusted": "feat-login"}}},
       "state": {"not-computable": {"reason": "no-merge-base"}}}]}}

← {"jsonrpc": "2.0", "id": 58, "result": {"computed_at_utc_ms": 1793091600000,
    "base": {"not-computable": {"reason": {"base-missing": {"name": {"untrusted": "develop"}}}}},
    "lanes_total": 0, "lanes": []}}
```

**Reglas del contrato**
- La respuesta solo va a la conexión que la pidió: nunca al bus, a otra conexión ni a `raptor-mcp`. No se registra ni se persiste.
- Un tope superado nunca es un error: `cut` y `lanes_total` lo dicen; un carril sin terminar es `too-slow`.
- `total` cuenta el mismo conjunto que `DivergenceView.ahead` contra la misma base (D4).

### Forma del error y del cuerpo de respuesta

| Causa | Respuesta | Cuándo |
|---|---|---|
| Parámetros mal formados o `repo_id` no válido | `INVALID_PARAMS` (-32602) | al momento |
| `repo_id` desconocido con el motor en `Observing` | `NOT_FOUND` (-32009) | al momento |
| `repo_id` desconocido antes de `Observing`, repo no `Observed` o sin worktrees | `result` con `base: not-computable {repo-unavailable}` (D19) | al terminar |
| 1 en vuelo en la conexión o cola llena | `query-busy {limit}` | al momento |
| Cancelada por `query.cancel` o cierre de la conexión | `query-cancelled` | al terminar el trabajo |
| Conexión MCP | `METHOD_NOT_FOUND` (`mcp: false`) | al momento |
| `ReadError` al abrir el repo común | `INTERNAL` "graph failed"; el texto no se publica | al terminar |
| Rate limit de la conexión | `RATE_LIMITED` (-32005) | al momento |
| Base sin confirmar, sin base, sin ref | `result` con `base: not-computable` | al terminar |
| Worktree no disponible, sin commits, sin merge-base, plazo, lectura | `result` con el carril `not-computable` | al terminar |

La TUI presenta cada caso por su clave en `GraphText`, nunca el `message` del daemon (N7).

### Forma de la configuración

_No aplica — los topes son constantes de GitRaptor (§ Valores numéricos); no hay clave de perfil ni de repo nueva._

### Valores numéricos

| Concepto | Valor | Fuente |
|---------|-------|--------|
| Commits visibles por carril | 50 | ⚠️ **ASSUMPTION** S-CKP-2; lo afina diseño (Q-CKP-51) |
| Recorrido máximo para el total | 10.000 | `MAX_DIVERGENCE_WALK` (`messages.rs:261`) |
| Carriles por respuesta | 32 | ⚠️ **ASSUMPTION** (D12) |
| Asunto por commit | ≤ 256 bytes | ⚠️ **ASSUMPTION** (D6) |
| Plazo por consulta | 2 s | DS-US-CKP-012 D4 |
| Respuesta serializada | ≤ 768 KiB | DS-US-CKP-012 D4; `MAX_MESSAGE_BYTES` 1 MiB |
| Espaciado del refresco de la vista | `max(1 s, 2 × última duración)` | ⚠️ **ASSUMPTION** (D9); ≤ ~50 % del pool de DS-012 D4 |
| Caché de carriles por repo | ≤ 560 KiB | D9 y D12 |
| Comprobación del token | cada 64 commits | ⚠️ **ASSUMPTION** |
| Nodos por carril en el panel | 6 | `MAX_NODES` (`graph_lanes.rs:51`), sin cambio |
| Altura mínima del panel | 6 filas | `GRAPH_MIN` (`layout.rs:23`), sin cambio |

---

## Modelo de datos

_No aplica — nada se persiste: los carriles viven en la respuesta y en `Model.ui.graph` mientras la vista está abierta._

---

## Estrategia de pruebas y cobertura

### 9.1 Pirámide de pruebas

| Nivel | Dónde | Qué |
|---|---|---|
| Unit | `crates/git/src/lane.rs`, `tui/graph.rs`, `present/graph{,_text}.rs` | Recorrido, topes, refresco, ancla, textos |
| Integración (daemon + Git real) | `crates/core/tests/graph_query.rs` | Base, carriles, cancelación, MCP |
| E2E (TUI + daemon) | `apps/cli/tests/tui_graph.rs` (macOS, debug) | Escenarios de la historia |
| Snapshots | `apps/cli/src/tui/snapshots/` | Panel 100×30 y vista 80×24, en/es/ASCII |

### 9.2 Umbrales de cobertura

Cada variante de `BaseGap`, `LaneGap` y `GraphCut` con su test y su texto (`every()` en `graph_text`). Sin cifra de cobertura de líneas en el repo.

### 9.3 Datos de prueba

Repo `shop` con `gitraptor_testkit::Fixture`: `main` confirmada y sacada en el worktree principal; `feat-pagos` (sesión de `raptor-fake-agent` como claude-1) con 72 commits desde su merge-base; `feat-login` (claude-2) con 3. Para el escenario 4, base del equipo `develop` sin esa rama. Nunca este repo (NFR-01).

### 9.4 Comportamientos críticos verificados

| Escenario | Test | Nivel |
|---|---|---|
| 1 Un carril por worktree | `tui_graph::one_lane_per_worktree_from_its_merge_base`: dos carriles con "claude-1" y "claude-2", cada uno en su `agent.n` (el mismo índice que su fila), sin carril para `main` | E2E |
| 2 Ventana acotada | `tui_graph::a_lane_shows_50_commits_and_22_more`: 50 filas de commit de `feat-pagos` y "22 más" | E2E |
| 3 Sin commit→evento | `tui_graph::every_commit_is_unattributed_and_keeps_the_lane_color`: cada fila dice "sin atribuir"; el nodo lleva el color del carril | E2E |
| 4 Base que no existe | `tui_graph::missing_base_shows_no_lanes_and_says_why`: ningún carril, "Rama base develop no encontrada: no calculable" en la vista y en el panel | E2E |
| 5 El grafo cede primero | `tui_graph::the_graph_yields_space_first_at_80x24`: a 80×24 con 12 filas en la flota, sin región de grafo, la lista con al menos `LIST_MIN` filas y `g` en la barra; `g` abre la vista. Alertas: G2 | E2E |

**Daemon (`crates/core/tests/graph_query.rs`):** `lanes_total_matches_the_ahead_of_the_stream`; `unconfirmed_base_has_no_lanes`; `the_base_worktree_is_not_a_lane`; `unavailable_and_unborn_worktrees_are_lanes_with_a_reason`; `cancel_answers_query_cancelled`; `mcp_connection_does_not_see_graph_lanes`; `responses_are_never_logged_nor_persisted` (asuntos con un token plantado; gitleaks sobre perfil y logs con 0 hallazgos); `limits_cut_and_never_fail` (`lanes`, `bytes`, plazo → `too-slow`).

**Git (`crates/git`):** `lane_walks_head_minus_base_newest_first`, `lane_total_is_exact_until_the_walk_limit`, `lane_without_merge_base_is_a_gap`, `summary_is_the_first_line_cut_on_a_char_boundary`, `lane_total_equals_count_only_in` (mismo `only_in_walk`, mismo total que el ↑), `repo_intact::lane_writes_nothing`.

**TUI (unidad):** `tui::graph::tests::refresh_spacing_is_twice_the_last_duration_and_one_in_flight`, `tui::graph::tests::a_snapshot_without_changes_does_not_mark_stale`, `graph_query::second_refresh_hits_the_lane_cache`, `tui::graph::tests::anchor_survives_new_commits_on_top`, `tui::graph::tests::a_reply_for_an_old_ticket_is_dropped`, `tui::graph::tests::closing_while_loading_cancels`, `present::graph::tests::panel_follows_fleet_order_and_session_color`, `present::graph::tests::summaries_with_controls_are_sanitized` (`\x1b]52;…`, RLO, U+2028), `present::graph_text::tests::every_text_has_both_languages`, `tui::view::tests::graph::the_graph_yields_before_the_conflict_panel_at_80x24` (G2), `tui::graph::tests::g_does_nothing_with_the_diff_open` (D20).

**Hilo del canal (`client::tests::graph`):** `a_graph_reply_reaches_update_by_ticket`, `busy_and_cancelled_map_to_graph_failure`, `a_lost_connection_answers_lost`, `an_engine_without_graph_lanes_is_announced` (D15).

### 9.5 Plataformas

macOS en el PR. Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

---

## Gate de seguridad

- [ ] **Texto no confiable (SEC-12, NFR-02)**: rutas, ramas y asuntos llegan como `Untrusted`/`UntrustedName` y se pintan solo como `SafeText` tras el saneado de ADR-CKP-003 § 8. El modelo y la vista no tienen `Untrusted` (`tui_boundaries.rs`).
- [ ] **Sin ejecución (NFR-02, SEC-09)**: ni shell ni `git` hijo; `gix` sin filtros; el recorrido es el mismo `only_in_walk` del ↑ (D4). El único parámetro es un `repo_id` opaco.
- [ ] **Solo lectura (NFR-01)**: el lector no escribe índice, refs, config ni objetos; `repo_intact::lane_writes_nothing`.
- [ ] **Fuera del MCP**: `graph.lanes` con `mcp: false`; no pasa por el bus, el logger, la auditoría ni el perfil. Residuo G3.
- [ ] **Denegación de servicio (SEC-08)**: la consulta comparte la cuota de DS-012 (1 en vuelo por conexión, cola de 4), plazo de 2 s, topes de D12, espaciado y caché de D9 y el rate limit de la conexión.

---

## Cómo extender

- **Actor por commit** (cuando motor-local publique commit→evento): campo opcional `actor: Option<Actor>` en `LaneCommit` con `skip_serializing_if`, capacidad `graph.commit-actor` en `methods/graph.rs` y su texto en `GraphText`. Sin la capacidad, el daemon omite el campo (patrón de `without_authorship`, `messages.rs:657`).
- **Filtro de carriles** (G1 de DS-US-DSK-005, NFR-13.4): si el Arquitecto decide que `graph.lanes` acepte un filtro opcional de carriles, `GraphParams` lleva `deny_unknown_fields` y un daemon anterior rechazaría el campo: entra detrás de una capacidad `graph.lane-filter` en `methods/graph.rs`, nunca como campo suelto.
- **Otro dato por commit** (autoría declarada): igual, detrás de `events.authorship`; nunca correos.
- **Otra consulta bajo demanda** (timeline de US-CKP-021): la receta de DS-US-CKP-012 § Cómo extender.

## Enmiendas que implica

No aplicadas por esta DS; las aplica T007.

- **ADR-CKP-003 § 4, N8**: "DEP-CKP-2 aplicado por DS-US-CKP-022 (`graph.lanes`), con la forma de DS-US-CKP-012".
- **ADR-GRP-005 (Enmienda 2026-10-04, Cockpit)**: "consulta de commits base..rama: DS-US-CKP-022 § Contrato de API".
- **ADR-GRP-013, línea de DEP-CKP-2**: sin cambio de fondo; añadir "el grafo muestra 'sin atribuir' y reserva la capacidad `graph.commit-actor` (DS-US-CKP-022 D7)".

## Fuera de alcance

| Ítem | Dónde se cubre | Gate |
|---|---|---|
| Actor por commit | Motor-local (DEP-CKP-2 opcional, ADR-GRP-013); Q-CKP-50 | D7 |
| ⚡ de conflicto previsto en el carril | Sin dueño desde que US-CKP-006 se implementó sin cablear `LaneModel.conflict` (N5) | — |
| Desplegar "N más" o paginar | Sin historia (D13, Q-CKP-51) | — |
| Ver el diff de un commit del carril | US-CKP-016 | — |
| Grafo por el MCP o la CLI | Sin historia; nunca contenido al MCP (Q-CKP-8) | `mcp_connection_does_not_see_graph_lanes` |

## Notas del autor

| ID | Nota | Acción | Owner |
|----|------|--------|-------|
| N1 | El panel cuenta desde el stream y la vista desde la consulta: pueden diferir un instante tras un commit. El ↑ del panel y el total de la vista cuentan el mismo conjunto (D4), así que convergen en el siguiente refresco | Ninguna | — |
| N2 | El dispatch de `conn.rs` es compartido (guía, § Qué sigue siendo compartido): T003 añade un brazo tras el de `DIFF_WORKTREE` (`:994`). `server.rs` y `channel/mod.rs` también se tocan (D17, D18), una línea cada vez | Rebasar con cuidado | Orquestador |
| ~~N3~~ | ~~`QueryFailure` genérico o enum propio~~ **Cerrada (2026-11-10)**: `DiffFailure` quedó en `main` con `Unavailable(DiffUnavailable)`; se crea `GraphFailure` (D15) | — | — |
| N5 | `LaneModel.conflict` (`graph_lanes.rs:30`, pinta ⚡ en `:118`) quedó sin cablear: US-CKP-006 está implementada y no lo hizo porque el panel no existía. Esta historia no lo pide y deja `false` | Decidir si entra como historia pequeña (marcar el carril con `RepoView.prediction`) o se retira el campo | PO |
| N6 | La guía `extender-sin-archivos-compartidos.md:43` dice "sin `.since(...)`" y el código de `main` declara todos los métodos nuevos con `.since(CAPABILITIES_PROTOCOL)` (D14) | Corregir la guía en una rama `docs/` | Arquitecto |
| N4 | Los topes de D12 no están medidos; si INF-GRP-002 muestra que 32 × 50 filas pasan el presupuesto de pintado (100 ms p95), se bajan aquí. Peor caso a medir (Arquitecto): 32 carriles × recorrido de 10.000 commits en 2 s; con la caché de D9 solo se paga la primera vez | Medir en el banco | Arquitecto |

### Preguntas abiertas

| ID | Pregunta | Por qué importa | Para |
|----|----------|-----------------|------|
| ~~Q-CKP-49~~ | ~~Con la base sin confirmar, ¿el grafo no muestra carriles o los dibuja contra la base resuelta marcada "sin confirmar"?~~ **Cerrada.** Decisión del orquestador (2026-10-28), validada por PO: sin carriles hasta confirmar la base, con el texto "Base main sin confirmar: confirma la rama base para ver los carriles" y cómo confirmarla (D5) | BR-CKP-CALC-004 dice "base confirmada" | PO |
| ~~Q-CKP-50~~ | ~~¿Se espera a que motor-local publique commit→evento o se une ya cada commit con el evento `commit` de su `new_commit`?~~ **Cerrada.** Decisión del orquestador (2026-10-28), validada por Arquitecto y PO: se espera a motor-local (D7); la capacidad aditiva `graph.commit-actor` queda reservada | Los intermedios de un rebase o un `--amend` saldrían sin atribuir sin decir por qué | PO y Arquitecto |
| ~~Q-CKP-51~~ | ~~¿Valen 50 commits por carril (S-CKP-2) y "N más" sin desplegar (D13)?~~ **Cerrada.** Decisión del orquestador (2026-10-28), validada por PO: se aceptan 50 commits y "N más" sin desplegar | Desplegar obligaría a paginar la consulta | PO (diseño) |

## Estado

- Hecho: D1 a D13 validadas por Arquitecto y PO (2026-10-28); Q-CKP-49 a Q-CKP-51 cerradas. Revisión contra `main` (`905c66b5`, 2026-11-10) tras US-CKP-012 (#360): G1, G2 y N3 cerrados, D14 a D20 añadidas, T003 a T006 con la forma real de `main`. `spec-ready`, sin tareas bloqueadas.
- Pendiente: medir el peor caso de N4 (Arquitecto); N5 (PO) y N6 (Arquitecto) no bloquean.
- Siguiente paso: T001 en rojo; luego T002/T003 (motor) y T004/T005/T006 (TUI) en paralelo.
