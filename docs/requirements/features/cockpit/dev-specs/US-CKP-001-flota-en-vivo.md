---
id: DS-US-CKP-001
title: "Dev Spec — La flota en vivo en la TUI"
type: dev-spec
status: approved
feature: cockpit
domain: GRP
story: US-CKP-001
created: 2026-10-06
updated: 2026-10-06
related:
  adrs: [ADR-CKP-003, ADR-GRP-011, ADR-GRP-003, ADR-GRP-013]
  stories: [US-CKP-001, US-CKP-002, US-GRP-007, US-GRP-009]
  technical: [INF-CKP-001, TS-CKP-004, TS-CKP-005, INF-GRP-002, TS-GRP-004]
  nfrs: [NFR-04, NFR-05, NFR-09, NFR-10]
  rules: [BR-CKP-CALC-001, BR-CKP-CONS-001, BR-CKP-CONS-003, BR-CKP-VAL-002, BR-CKP-TIME-001, BR-CKP-WF-001]
tags: [cockpit, tui, flota, worktrees-en-vivo, sesiones, frescura, nfr-04, tema, osc11, esqueleto-andante]
---

# Dev Spec — US-CKP-001: la flota en vivo en la TUI

Blueprint compacto de [US-CKP-001](../user-stories/US-CKP-001-flota-en-vivo.md) (hito M1, criterios 1 y 6). Se apoya en lo que ya está en main: el esqueleto de la TUI (INF-CKP-001), la biblioteca de componentes (TS-CKP-005), la paleta A con la detección del fondo (TS-CKP-004), el canal N1–N7 (TS-GRP-004), las sesiones (US-GRP-007), el registro de agentes (US-GRP-009) y el banco de frescura (INF-GRP-002). No crea widgets: compone los que hay y amplía tres en la biblioteca.

**Decisiones del orquestador (2026-10-06), validadas por Arquitecto y PO.** Los ajustes que pidieron están incorporados y se marcan en cada decisión.

## 1. Ubicación en el código

| Archivo | Qué cambia |
|---|---|
| `crates/theme/src/detect.rs` | `color_mode` y `symbol_set`: profundidad de color y juego de símbolos desde el entorno (funciones puras, entorno inyectable) |
| `apps/cli/src/term.rs` | `theme(flag, no_color, ascii)`: arma el `Theme` antes del lector de eventos (OSC 11 incluido) |
| `apps/cli/src/main.rs` | `--theme`, `--no-color` y `--ascii` en `raptor` y en `raptor tui` |
| `apps/cli/src/model.rs` | `Ui.theme`; `RepoView` con `name`, `base`, `worktrees`, `sessions`, `detection` y `upsert`; `GlobalView.attention`; `EngineMsg::Sessions` |
| `apps/cli/src/present/{ingest,i18n}.rs` | Ingesta de worktrees, sesiones y atención (el único punto de saneado); textos en/es de la flota |
| `apps/cli/src/client/mod.rs` | Tras la instantánea del repo, `sessions.list` (§ 3, D1) |
| `apps/cli/src/tui/update.rs` | Aplica `worktree.state`, `session.state` y la lista de sesiones |
| `apps/cli/src/tui/view.rs` | Compone StatusBar, AgentList y KeyHints; snapshots en terminal oscura y clara |
| `apps/cli/src/tui/widgets/{agent_list,layout}.rs`, `tui/style.rs` | Ampliaciones de la biblioteca (D2) |
| `apps/cli/src/tui/{metrics,app}.rs` | `last_render_ns`: la marca final del banco de punta a punta |
| `apps/cli/tests/live_fleet.rs` | Un test por escenario con el daemon real y repos temporales |
| `apps/cli/benches/engine.rs`, `crates/testkit/src/freshness.rs` | Escenario `tui-modify` del banco INF-GRP-002 (D4) |

Fronteras de ADR-CKP-003 (V5, V6): la TUI no importa el motor, Git ni políticas; `link` sigue siendo la única excepción. `tests/tui_boundaries.rs` no cambia y pasa.

## 2. La fila (BR-CKP-WF-001, BR-CKP-CALC-001, BR-CKP-CONS-003)

Una fila por worktree publicado. El principal va primero y el resto sigue el orden en que los publica el motor (el orden por atención es de US-CKP-002).

| Columna | Fuente | Sin dato publicado |
|---|---|---|
| Estado + agente | Sesión presente del worktree (Activa `●`, Inactiva `◐`). Desempate fijo: Activa, luego Inactiva, luego el `state_since` más reciente, luego `session_id`. Con más de una: "claude-1 +1" | Sin sesión presente: "Tú u otro (sin atribuir)", sin símbolo. Sin lista de sesiones o con `detection_available: false`: "agente no disponible" (nunca "sin agente") |
| Rama | `HeadView` | `HEAD` separado: "HEAD separado" |
| Archivos | `counts.total()` (preparados, sin preparar y sin seguimiento) | — |
| ↑↓ | `DivergenceView` contra la rama base | Recuento acotado: "↑10000+". `BaseMissing`, `NoBase`, `NoCommits` y `Unreadable`: su motivo, nunca 0 |
| Actividad | DEP-CKP-4 | "no disponible" en todas las filas |

- Worktree ilegible: `⚠` y el motivo ("falta la carpeta", "Git no confía en él", "ilegible ahora").
- El título del panel nombra la referencia del ↑↓ y su límite: "Flota · ↑↓ respecto a main (copia local, antigüedad no disponible)". **Ajuste del PO**: el motor cuenta contra la rama base, no contra el remoto, así que la etiqueta nombra la base. La antigüedad de la copia local no se publica: queda como dependencia del motor (nueva, sin dueño todavía).
- La sesión se empareja con su worktree por un hash de la ruta en crudo, calculado en la ingesta, y nunca por el texto saneado. **Ajuste del Arquitecto**.
- Las sesiones Terminadas, "último agente: X (terminó hace N)" y su ventana de 24 h son de US-CKP-002. Consecuencia aceptada: hasta entonces, una fila que solo tiene una sesión Terminada se muestra "sin atribuir".
- Cabecera: repo, conexión (con el estado del motor, "desactualizado" y la respuesta a la última tecla), "actúas como…" y ⚡/⛔ del resumen de atención del ámbito global. Mientras el motor no los cuenta (`AttentionCount::Unavailable`), se pinta "–", que significa "no disponible" (**ajuste del PO**). Los publicarán el predictor (TS-CKP-001), Guardrails y US-GRP-005.

## 3. Decisiones

| # | Decisión | Validación |
|---|---|---|
| D1 | **Sesiones sin cambiar el contrato.** La instantánea del repo trae los worktrees, pero no las sesiones. Tras la instantánea (y la suscripción) del ámbito repo, el hilo del canal pide `sessions.list {repo_id, include_ended: false}` y la entrega como `EngineMsg::Sessions`. `update` fusiona la lista y cada `session.state` con un mismo upsert por `session_id`: gana el `state_since` posterior y, si empatan, la Terminada (no se reabre). Converge en cualquier orden, porque la lista se toma después de suscribirse desde N+1 y todo evento > N llega después. Una instantánea nueva (hueco, `resync`, reconexión) vacía las sesiones y el canal las vuelve a pedir. Alternativa descartada: añadir las sesiones a `RepoSnapshot`, porque cambia el contrato y el estado compartido del bus en `crates/core`. Queda como deuda en la enmienda de ADR-CKP-003 § 4, con dueño TS-GRP-004 | Arquitecto, con ajustes: `include_ended: false` (con `true`, la página de 500 podría dejar fuera una sesión presente antigua), fusionar en vez de reemplazar, desempate a favor de la Terminada, volver a pedir en cada instantánea y "no disponible" sin detección |
| D2 | **Ampliar la biblioteca, sin widgets nuevos.** `AgentState::NoAgent` (sin símbolo, nombre atenuado). Columnas de nombre (14–26) y actividad (11–14) que crecen hasta el texto más ancho de **todas** las filas, para que no salten al hacer scroll. Las columnas ⚡/⛔ solo ocupan sitio cuando alguna fila tiene una marca. `StatusBarModel.conflicts/blocked: Option<u32>`, con `Glyphs::unknown` (`–`, fallback ASCII `-`) para `None` | Arquitecto: el guion sale del tema, no de un literal (va en `Glyphs`, el juego estructural que elige el mismo `SymbolSet`, como `…`) |
| D3 | **Tema al arrancar.** `--theme` > `GITRAPTOR_THEME` > OSC 11 > `COLORFGBG` > oscura (`resolve` de TS-CKP-004), antes de `ratatui::try_init` y del lector de crossterm. Profundidad: `--no-color`, `NO_COLOR` no vacío o `TERM=dumb` dan sin color; `COLORTERM=truecolor\|24bit`, truecolor; `TERM=*256color`, 256; si no, 16. Símbolos: `--ascii`, o una locale (`LC_ALL` > `LC_CTYPE` > `LANG`) que no sea UTF-8 (`C` y `POSIX` incluidas), dan ASCII. ⚠️ **ASSUMPTION**: sin ninguna locale, Unicode | Arquitecto: lógica pura en `crates/theme`, E/S en `term`; una variable vacía cuenta como ausente; sin TTY ni color no se envía OSC 11 (ya lo hacía `query_background`) |
| D4 | **Gate de frescura de punta a punta** (criterio 6 de M1; NFR-04, 500 ms p95). Escenario `tui-modify` del banco INF-GRP-002: la `App` real sobre `TestBackend` 120×40, conectada al daemon aislado por el canal real, con el repo de 100K commits y 10 worktrees. Cada muestra escribe el archivo del banco en `wt-3` (`t0`) y avanza la TUI hasta que el frame pintado muestra el nuevo recuento (`t_render`). El presupuesto de 500 ms más el exceso de holgura del temporizador **falla en `reference`** y en `ci` solo se reporta: allí bloquea el techo de regresión con la confirmación 2 de 3. La etapa propia de la TUI (100 ms p95) **falla en los dos modos**. Techos `ci` provisionales: los de `modify` + 100 ms, hasta calibrarlos con sus propias corridas. Linux en cada PR y macOS en main (workflow `engine-bench.yml`, sin cambios) | Arquitecto, con ajuste: el gate de punta a punta no puede bloquear en `ci` (enmienda de calibración de ADR-GRP-011) y un escenario sin techo calibrado falla |
| D5 | **Sin navegación todavía.** La lista no tiene selección: con 10 worktrees caben de sobra en 80×24. La selección y el scroll llegan con la primera historia que actúa sobre una fila | — (no es una decisión de diseño nueva: la biblioteca ya los soporta) |

## 4. Escenarios y tests

| Escenario | Test (`apps/cli/tests/live_fleet.rs`, daemon real) | Unitarios |
|---|---|---|
| Una fila por worktree, con el agente primero | `one_row_per_worktree_with_the_agent_first`: "shop" con `feat-pagos` ↑3 ↓1, `claude-1` registrado y 2 archivos | `view::tests::one_row_per_worktree_with_the_agent_first`, snapshots `fleet_{80x24,120x40}_{dark,light}` |
| Un cambio se ve en menos de medio segundo | `a_change_reaches_the_row_live` (llega en vivo, sin tiempos fijos); la cifra p95 la da el banco (`tui-modify`) | `tui_loop` (100 ms, sintético) |
| Lo no atribuido nunca se presenta como "humano" | `a_commit_without_an_agent_is_unattributed` | `a_worktree_without_an_agent_is_unattributed_never_human`, `unknown_detection_is_not_presented_as_no_agent` |
| Un campo que el motor no publica no se calcula | `an_unpublished_field_is_not_computed_by_the_tui`: la TUI corre sola en un proceso hijo con `PATH` = trampa de `git` (auditoría limitada a su PID), y `lsof` sobre ese PID comprueba que no tiene abierto nada del perfil salvo el socket | `unpublished_activity_is_not_available`, `unpublished_attention_counts_are_not_zero` |
| Texto no confiable no altera la terminal | `untrusted_text_is_painted_inert`: Git no admite caracteres de control en el nombre de una rama y el motor rechaza uno en el nombre de un agente (el test lo comprueba), así que el texto no confiable que sí llega es la carpeta de un worktree con la secuencia | `a_malicious_branch_is_painted_inert` (la rama del escenario, sobre la vista) |

## 5. Pendiente

- Etapa de validación multiplataforma: los tests de proceso son de macOS (`script`, `lsof`). El banco corre en Linux en el PR. La detección de la locale y de la profundidad de color está probada solo como función pura.
- Calibrar los techos `ci` de `tui-modify` con tres corridas por runner, como en INF-GRP-002.
- ~~Dependencias del motor: la antigüedad de la copia local del remoto (nueva) y DEP-CKP-4 (última actividad).~~ Cubiertas por la enmienda del § 6, con la simplificación que allí se declara.
- Sesiones en la instantánea del repo (deuda de D1, dueño TS-GRP-004).

## 6. Enmienda (2026-10-06): hallazgos del primer dogfooding

Rene usó la TUI por primera vez el 2026-10-06 (`bitacora/orquestador/TRASPASO.md`, "Hallazgos del dogfooding", puntos 1 y 4). Esta enmienda corrige cuatro defectos. **Decisión del orquestador (2026-10-06), validada por Arquitecto** en la forma del contrato (E1–E3); E4 y E5 los fija el encargo.

| # | Defecto | Cambio |
|---|---|---|
| E1 | La columna Actividad decía "no disponible" en todas las filas | El motor publica `WorktreeView.last_activity_utc_ms` (DEP-CKP-4). La TUI pinta la antigüedad ("hace 2 min") con el reloj de `Msg::Tick`, que ahora repinta una vez por segundo mientras hay datos. Calcular la antigüedad a partir del dato publicado es presentación, no derivación (BR-CKP-CALC-001). Un worktree sin cambios desde que arrancó el motor sigue diciendo "no disponible" |
| E2 | El título decía "antigüedad no disponible" | El motor publica `RepoView.fetched_utc_ms` y `WorktreeStateData.fetched_utc_ms`, que son el mtime de `<common dir>/FETCH_HEAD`, recortado a "ahora" si está en el futuro. El título dice "(copia local, fetch hace 3 h)" o, sin `FETCH_HEAD`, "(copia local, sin fetch)" |
| E3 | Contrato | Capacidad nueva `scope.activity` (ADR-GRP-016, sin subir el protocolo). Es la primera posterior al 9. Sin ella, el daemon quita los campos del snapshot (`engine.snapshot`, `scope.snapshot`), del resultado de `repo.add` y de cada `worktree.state` (bus, `Outbox::shape`). El cliente de `crates/core` la pide al conectar, así que la CLI, la TUI y `raptor-mcp` la reciben. La TUI la consulta en el `hello` (`Link::has`): sin ella, la actividad y el fetch dicen "no disponible", nunca "sin fetch" |
| E4 | Todos los agentes se llamaban "Claude Code" | El nombre lleva la carpeta del worktree: "Claude Code · dehotspot" o "claude-1 · feat-pagos +1" (texto `AgentInWorktree` del catálogo). La carpeta es el último componente de la raíz publicada y pasa por `SafeText` en la ingesta |
| E5 | Fuera de un repo, la TUI pedía `raptor repo add` aunque ya había repos observados | El hilo del canal avisa `ConnEvent::Unlocated` cuando `repo.locate` no encuentra la carpeta. Con un repo observado, `update` lo abre (`Cmd::Open`). Con varios, muestra el selector `RepoPicker` (↑↓ o k/j, Enter). Solo sin ningún repo observado sugiere `raptor repo add`. El repo elegido se recuerda entre reconexiones. Las teclas de lista solo aparecen en las pistas mientras el selector está abierto |

**Regla de actividad (Arquitecto)**: cuenta un cambio del commit o la rama del head, de los recuentos o la lista de cambios, o de la legibilidad del worktree. **No cuenta** el ↑↓ (un fetch lo mueve sin que nadie toque el worktree) ni los cambios de sesión. Un worktree nuevo en un repo que ya tenía worktrees cuenta como actividad. **Simplificación declarada**: el valor se deriva en memoria al publicar (`observe::stamp_activity`, en la misma sección crítica del bus, `EventBus::publish_with`). Todavía no se deriva de los eventos del almacén ni lleva la marca del hueco, así que vale "no disponible" hasta el primer cambio de cada ejecución del motor. El desfase local no viaja: la TUI solo muestra antigüedades relativas.

| Escenario | Test |
|---|---|
| E1, E2 | `view::tests::published_activity_and_fetch_show_their_age` (en/es), `a_repo_never_fetched_says_so`, `ages_use_the_largest_whole_unit`; snapshots `fleet_{80x24,120x40}_{dark,light}` con los datos publicados |
| Regla de actividad | `crates/core/tests/observe.rs` (`activity::*`): un cambio y un commit cuentan; el ↑↓ solo no cuenta; la primera lectura no sabe nada; `FETCH_HEAD` nunca en el futuro |
| E3 | `crates/core/tests/channel_capabilities.rs`: `scope_activity_carries_the_last_fetch_and_activity_only_to_who_accepted_it` (con y sin la capacidad, en el snapshot y en `worktree.state`). `accept_grants_…` pasa a conexiones crudas, porque el cliente de la librería ya acepta al conectar |
| E4 | `view::tests::agents_of_the_same_kind_are_told_apart_by_their_worktree` |
| E5 | `outside_a_repo_the_only_observed_one_opens`, `outside_a_repo_with_none_observed_it_says_how_to_add_one`, `outside_a_repo_the_developer_chooses_among_several`; snapshot `repo_picker_80x24` (y su texto en es) |

Pendiente de esta enmienda:

- Derivar la última actividad de los eventos del almacén al arrancar, con la marca del hueco (ADR-GRP-013, Enmienda 2026-10-04).
- Un `fetch` que no escribe `FETCH_HEAD` no se ve.
- El selector usa la lista de repos del snapshot global. Si se añade un repo mientras está abierto, aparece en el siguiente resync.
- `replace:9` (ADR-GRP-016): un reemplazo con el mismo protocolo sigue contándose como caída. No se afina aquí.
- Las KeyHints y la navegación de historias posteriores (US-CKP-002+) quedan fuera.

## 7. Enmienda (2026-10-06): la fila "sin agente"

La fila "Unattributed (you/other)" con `detached HEAD` no se entendía (`bitacora/orquestador/TRASPASO.md`, "Propuesta acordada con Rene (2026-10-06)"). Los textos y los glifos los fijó Rene. La forma del contrato es **Decisión del orquestador (2026-10-06)**: no se consultó al Arquitecto por la cuota y queda abierta a su revisión en el PR.

| # | Cambio |
|---|---|
| F1 | Una fila sin sesión de agente dice `No agent · <worktree>` / `Sin agente · <worktree>` (texto `NoAgent` del catálogo), con el glifo `○` (`symbol.agent.done`, ASCII `o`). Toda la fila va en el tono tenue (`text.muted`), salvo el HEAD sin rama, que sigue en aviso. Se lee sin color porque el glifo y el nombre lo dicen. Con la detección desconocida, la fila dice "agente no disponible", sin glifo (`AgentState::Unknown`) |
| F2 | HEAD sin rama: `39e852f (no branch)` / `39e852f (sin rama)`. El motor publica `WorktreeView.detached_at` (el hash completo, solo con HEAD separado) bajo la capacidad `scope.activity`, que ya existía (E3), y `without_activity` lo quita. La TUI acepta solo un hash hexadecimal y muestra 7 dígitos. Sin el dato, dice `(no branch)` y nunca inventa un hash |
| F3 | Etiqueta `temporary` / `temporal` tras la rama, en tono tenue, para un worktree bajo la carpeta temporal del sistema (`std::env::temp_dir()` y, en Unix, `/tmp`, escritas y resueltas). La TUI la calcula en la ingesta a partir de la raíz publicada. Si la columna no deja `BRANCH_MIN` para la rama, la etiqueta se omite |
| F4 | Leyenda en las KeyHints, después de las acciones y solo si alguna fila no tiene agente y cabe entera: `○ no agent: changes by you or another tool` / `○ sin agente: cambios tuyos o de otra herramienta`. El glifo sale del tema |

Decisiones (orquestador, 2026-10-06):

- **`detached_at` bajo `scope.activity`, sin capacidad nueva.** Es otro dato de presentación para el Cockpit con la misma regla de forma: un cliente sin la capacidad no lo ve, porque sus tipos rechazan campos desconocidos. Una capacidad propia habría añadido otro indicador al bus (`Outbox`) sin ganar nada.
- **`○` reutiliza `symbol.agent.done`.** El estado "terminado" no se pinta hoy en la flota, porque una sesión terminada deja la fila sin agente. Un token `symbol.agent.none` propio queda pendiente para el design system.

| Escenario | Test |
|---|---|
| F1, F4 | `view::tests::a_worktree_without_an_agent_says_no_agent_never_human` (en/es), `without_a_row_without_an_agent_there_is_no_legend`, `unknown_detection_is_not_presented_as_no_agent`; `tests/live_fleet.rs` con el motor real |
| F2, F3 | `view::tests::a_scratch_worktree_without_a_branch_says_so` (en/es, con y sin el hash publicado); `present::ingest::tests::{only_a_commit_hash_is_shortened, a_worktree_under_the_temporary_folder_is_temporary}`; `crates/core/tests/observe.rs` (`detached_and_unborn_heads_are_reported_as_such`); `gitraptor-api` `without_activity_drops_the_detached_commit` |
| Pantalla | snapshots `fleet_no_agent_100x24_{en,es}`, `fleet_{80x24,120x40}_{dark,light}`, `agentlist__fleet_without_agents` (Unicode y ASCII) |

Pendiente de esta enmienda:

- La etiqueta `external` / `externo` (worktree fuera de las rutas habituales) y el panel de detalle de `enter` (ruta, desde cuándo, qué significa) son de US-CKP-002.
- El catálogo une agente y worktree con un `·` literal (`AgentInWorktree`, `NoAgent`). En el juego ASCII no se sustituye.
- Token propio `symbol.agent.none` en `packages/design-tokens`.
- Sin asignación automática del dueño: si algún día se ofrece "asignar a un agente", lo confirma el humano.
- Solo se verificó en macOS. La detección de la carpeta temporal en Windows (`%TEMP%`, rutas `\\?\`) y en Linux queda para la validación en máquinas reales.
