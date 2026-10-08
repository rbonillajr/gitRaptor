---
mode: bulk
generated: 2026-10-04T00:00Z
updated: 2026-10-05
generator: product-owner
total_artifacts: 19
expanded: 19
approved: 0
blocked:
  - US-MCP-014
architecture_gate: []
---

# User Stories — INDEX: Servidor MCP

> Generado en modo Bulk (modelo plano, modo lean del equipo). Cada historia vive en su archivo de `user-stories/`. Esta tabla solo enlaza y marca el Status.

---

## Contexto del Feature

**Feature**: Servidor MCP (F-001-05)
**Epic**: E-001 — MVP Fase 1: Cockpit + Time Machine + Guardrails (CLI/TUI + MCP)
**Prioridad**: Alta (BR-14, BR-15 y BR-16 Must; NFR-02)
**Estado**: En Análisis. **Sin bloqueos de arquitectura desde el 2026-10-05** (D-23): ADR-MCP-001, ADR-CKP-001 y ADR-CKP-002 están aceptados y las enmiendas DEP-MCP están aplicadas. Las escrituras siguen dependiendo de sus habilitadores (TS-CKP-002, TS-CKP-003). Solo US-MCP-014 sigue bloqueada, por producto (cola de confirmación, US-GRD-015).

**Enlace a contexto completo**: [`context.md`](./context.md) (CTX-MCP-001, aprobado por Rene Bonilla)
**Reglas de negocio**: [`business-rules.md`](./business-rules.md) (BR-MCP-001, 48 reglas) · **Diseño**: no aplica (sin superficie visual; los mensajes en/es siguen la guía de contenido del [design system](../../../design-system/README.md))

---

## Análisis de División (Sistema de 4 Pasos)

- **Paso 1 — Flujo**: actor principal = desarrollador orquestador; actores secundarios = el agente (Claude Code) y el agente sin soporte completo. Resultado buscado = que el agente opere el repo con herramientas que no pierden trabajo ni se saltan las reglas. Camino mínimo = instalar → habilitar el repo → el agente pide `status` de su repo.
- **Paso 2 — Puntos de resultado**: cada historia deja una respuesta de herramienta, un rechazo con motivo y acción, o un cambio en el repo, la allowlist o el timeline que se comprueba desde fuera.
- **Paso 3 — Patrón aplicado**: camino feliz primero, en el orden de entrega de Q-MCP-19. El esqueleto andante es US-MCP-003 (agente → servidor → motor → respuesta, con ámbito y allowlist). Después, por capacidad acumulable: respuestas seguras y registro → confused deputy → primera escritura y su endurecimiento → lecturas que dependen de otras features → rebase y worktree.
- **Paso 4 — Filtro**: todas pasan (usuario real · resultado observable · cabe en una rama corta de un agente · comportamiento, no código). Ninguna supera 6 escenarios. El esqueleto andante ya se partió en tres (D-2). US-MCP-003 se queda con 7 reglas porque el esqueleto las necesita juntas: sin ámbito, allowlist, rechazo sin datos y motor bajo demanda no hay una sola llamada `status` segura que entregar. Son pocas las que dan escenarios propios (CONS-001 se verifica en la Dev Spec), así que la historia sigue cabiendo en una rama corta. Es la única que pasa de 6 reglas.

> **Criterio de bloqueo** (Decisión del orquestador (2026-10-04), validada por PO):
> - **Bloqueo de arquitectura**: falta un ADR o una enmienda que fija el contrato. Se lista en `blocked_by` de cada ficha y se levanta al aceptarse el artefacto, sin cambiar la historia. ADR-MCP-001 (DEP-MCP-1) **no existe** y bloquea 18 historias (todas menos US-MCP-002): fija ámbito y `expect_worktree`, herramienta → operación, allowlist de campos, errores, límites, cancelación e instalación. El transporte (stdio) y las capacidades (solo `tools`) ya los fija Q-MCP-13. ADR-CKP-002 (catálogo de operaciones y ejecutor; **propuesto** y en revisión en la rama docs/arch-cockpit, aún no en main) bloquea todas las escrituras del catálogo: US-MCP-007, 008, 009, 010, 011, 013, 018 y 019. DEP-MCP-2 y DEP-MCP-5 se citan "vía ADR-CKP-002", no como bloqueos aparte. DEP-MCP-3, DEP-MCP-4, DEP-MCP-6 y DEP-MCP-8 bloquean las historias que nombran, igual que ADR-CKP-001 (propuesto) a `check_conflicts`. Dos huecos sin artefacto también bloquean: el dueño del lado del canal del confused deputy (US-MCP-007) y la interfaz de captura manual de la Time Machine (US-MCP-008).
> - **Bloqueo de producto**: depende de una historia de otra feature bloqueada por una decisión de producto o de seguridad. Solo US-MCP-014 (cola de confirmación US-GRD-015, bloqueada a su vez por el Cockpit y el factor fuera de banda del SO, Q-GRD-19 / D5, ADR sin crear). Es la única en `blocked` del índice.
> - **No son bloqueos**: las historias de otras features que existen (US-GRP-*, US-TMC-*, US-GRD-*, TS-*) son dependencias de historia. Los habilitadores y las historias del Cockpit todavía no están en main y se citan como propuesta con su rama: TS-CKP-001 (predictor), TS-CKP-002 (catálogo y ejecutor) y TS-CKP-003 (decisión única heredada por los hooks), en docs/arch-cockpit; US-CKP-015 (rebase), US-CKP-018 (crear worktree) y US-CKP-023 (cola en el Cockpit), en docs/stories-cockpit. No van en el frontmatter `related` hasta que lleguen a main. DEP-MCP-9 (cwd de otro proceso en Linux y Windows) es "Pendiente: etapa de validación multiplataforma": se verifica en macOS.
>
> **Transversales (no son historias)**:
> - Windows y Linux: pendientes de la etapa de validación multiplataforma (DEP-MCP-9, S-MCP-4). El MVP se verifica en macOS.
> - Cero pérdida de datos (NFR-01): toda escritura es operación protegida; se exige en todos los escenarios de escritura.
> - Seguridad por release (DEP-MCP-8): revisión OWASP / MCP Top 10 antes de cada release y el **corpus de seguridad** (traversal, refs maliciosas, UNC, inyección de argumentos, confused deputy), que verifica las reglas VAL y BR-MCP-AUTH-005 a través de US-MCP-005, 007, 010, 017 y 019.
> - **Resuelto en la Fase 2 (2026-10-05)**: el Arquitecto creó INF-MCP-001 ([technical-stories.md](./technical-stories.md)) y la puerta MCP Top 10 (SEC-MCP-n en `non-functional.md`). Texto original: el KPI "100 % del corpus de seguridad rechazado" (Q-MCP-18) no tiene historia que lo entregue. Se propone el enabler **INF-MCP-001** (propuesto, no creado): el corpus como suite de CI que bloquea el merge, con los casos de US-MCP-005, 007, 010, 017 y 019. Y la revisión MCP Top 10 como puerta de release en `non-functional.md` (SEC-MCP-n, DEP-MCP-8, security-expert), que incluya el *tool shadowing* por otros servidores MCP de la misma sesión y el consumo sin límite. El PO no crea ese enabler: lo decide el Arquitecto.
> - Cifras de topes, tiempos, rate limit y cuota: fijadas en ADR-MCP-001 § 6 (D-22); la Dev Spec solo puede endurecerlas. El tiempo máximo de una escritura en la capa `mcp` sigue siendo 300 s (ADR-CKP-002 § 6).
> - Cómo se distingue al humano en un comando reservado: transversal (lo define el Arquitecto; ADR-GRD-007).

### Decisiones del orquestador (2026-10-04), validadas por PO

El Arquitecto las revisará en Fase 2.

| # | Decisión | Motivo |
|---|----------|--------|
| D-1 | ADR-MCP-001 es un bloqueo de arquitectura común a 18 historias, distinto del bloqueo de producto de US-MCP-014. US-MCP-002 no lo tiene (v1.1, A5): es CLI y perfil, y la bloquean DEP-MCP-4 y la mitad de DEP-MCP-3 sobre la allowlist. | Sin el contrato no hay Dev Spec, pero ninguna historia cambia cuando el ADR se acepte. |
| D-2 | El esqueleto andante se parte en tres: US-MCP-003 (ámbito, allowlist, motor bajo demanda), US-MCP-004 (contenido completo de `status` y "no disponible") y US-MCP-005 (respuestas seguras y catálogo cerrado). | Con todo junto superaba 10 reglas; así cada parte cabe en una rama corta y US-MCP-003 llega antes. |
| D-3 | `snapshot` (US-MCP-008) empieza después de que la Dev Spec de `safe_commit` (US-MCP-009) fije el flujo de escritura (BR-MCP-WF-001). Ya no van en paralelo (v1.1, A4). | `safe_commit` tiene correspondencia completa en ADR-CKP-002; para el snapshot faltan la operación normalizada de Guardrails y la interfaz de captura manual de la Time Machine. |
| D-4 | `safe_commit` se reparte en tres historias: camino feliz (009), rutas y archivos (010) y worktree correcto y precondiciones (011). | La propuesta tenía 9 reglas en dos historias; con tres, ninguna de las tres pasa de 6 reglas ni de 6 escenarios. |
| D-5 | La medición del MCP es su propia historia (US-MCP-015, Should). La demo del BRD § 13 es la prueba de aceptación de la feature, no una historia. | Los KPIs son observables y verificables por separado; la demo cruza features (ver abajo). |
| D-6 | `create_worktree` se escribe **sin** parámetro de ruta, solo con la plantilla del desarrollador, como fija ADR-CKP-002 (hallazgo de seguridad H-02). En v1.1 se enmendaron BR-MCP-VAL-001 y BR-MCP-ELIG-004 (BR-MCP-001 v0.2). Q-MCP-7 ("ruta opcional") queda estrechada por esa enmienda; context.md no se edita. | ADR-CKP-002 cerró la cuestión; la regla debe decir lo mismo que la historia. |
| D-7 | `check_conflicts` y `explain_history` se dejan en la ola 4 (Q-MCP-19), pero no dependen de ninguna escritura. US-MCP-016 se puede adelantar cuando se levanten ADR-CKP-001 y DEP-MCP-6; US-MCP-017, en cuanto exista ADR-MCP-001 (D-12). | Son lecturas; solo esperan datos de otras features. |

### Decisiones del orquestador (2026-10-04), validadas por Arquitecto/PO (v1.1)

Ajustes del Juez AADD (reservas) y del Arquitecto.

| # | Decisión | Motivo |
|---|----------|--------|
| D-8 | US-MCP-013, 016, 017 y 019 pasan a `priority: high`. | Son Must (BR-14); la prioridad del frontmatter debe coincidir con el mapa. |
| D-9 | Las escrituras dependen de TS-CKP-002 (catálogo y ejecutor) y TS-CKP-003 (decisión única heredada por los hooks): 007, 008, 009, 018 y 019. US-MCP-016 depende de TS-CKP-001 (predictor). Hay relación con historias del Cockpit: 018 con US-CKP-015, 019 con US-CKP-018 y 014 con US-CKP-023. Todos son propuestas en otras ramas y sustituyen a los "Cockpit F-001-02: … (historias en curso)". | Dar a la flota IDs concretos en vez de capacidades genéricas. |
| D-10 | Cada operación del catálogo tiene una historia dueña: 009 es dueña de `commit`, 008 de `snapshot` y 018 del modo `atomic` de `rebase-onto-base`. 019 comparte `create-worktree` con US-CKP-018: la implementa la primera que entre. | Evitar dos implementaciones de la misma operación. |
| D-11 | US-MCP-011 va antes que US-MCP-010: las dos tocan los parámetros de `safe_commit`. | Un solo contrato de parámetros, fijado por la primera. |
| D-12 | US-MCP-017 deja de estar bloqueada por DEP-MCP-6. TS-TMC-004 ya expone la consulta del timeline para el MCP en el contrato del canal, y los eventos del motor los fija ADR-GRP-013, ya aceptado. La parte de timeline pasa a dependencia de historia (TS-TMC-004). | Comprobado en TS-TMC-004 ("Exponer los comandos de snapshot, undo, redo, restauración y consulta del timeline para CLI, TUI y MCP"). **Corrección (2026-10-05)**: en main `timemachine.timeline` no tiene marca MCP (`crates/api/src/methods.rs`); la vista MCP del timeline la fija ADR-GRP-013, Enmienda (2026-10-05, MCP), y la añade US-MCP-017. |
| D-13 | El lado del canal del confused deputy no tiene dueño. ADR-CKP-002 se lo asigna a TS-GRP-004, que ya está mergeada y lo deja fuera de su alcance. El lado del canal es marcar la conexión de un proceso lanzado por el ejecutor y rechazarle lo reservado en describir, preparar, ejecutar y cancelar. Es un bloqueo de US-MCP-007; la propuesta del Arquitecto es TS-CKP-002. | Hueco de seguridad crítico (R-MCP-1) que no puede quedar implícito. |
| D-14 | US-MCP-008 se bloquea también por la interfaz de captura manual de la Time Machine (etiqueta, cuota y nivel). Está pendiente en ADR-CKP-002 y su dueño es la Time Machine. | Sin ella no hay snapshot manual con cuota propia. |
| D-15 | Escenarios nuevos y reescritos. 018: tiempo máximo vencido (abort, `time-limit`) y HEAD separado. 009: hook que supera el tiempo máximo. 007: rechazo en el acto de una operación del catálogo, de Cancelar y de confirmaciones pedidas por un hook del ejecutor; atribución observable en el registro de auditoría; se quita el escenario del catálogo, que duplicaba a US-MCP-005. 019: padre con enlace simbólico, ubicación según la plantilla y ramas maliciosas (`refs/heads/x`, hex de 40 caracteres, `@{`, bidi). 005: el catálogo se describe solo por lo observable, sin nombrar el transporte. | Reservas del Juez (J3) y del Arquitecto (A8). Ninguna ficha pasa de 6 escenarios. |
| D-16 | Pendiente para ADR-MCP-001: ADR-CKP-002 § 12 exige que el agente enumere los avisos del plan al ejecutar. Las fichas suponen herramientas de una sola llamada, y el escenario de rama ya empujada de US-MCP-018 (BR-MCP-EDGE-007) depende de lo que se decida. | Que el contrato del servidor resuelva el flujo de avisos antes de la Dev Spec de 018. |

### Decisiones del orquestador (2026-10-05), validadas por Arquitecto/PO (v1.2, ADR-MCP-001)

| # | Decisión | Motivo |
|---|----------|--------|
| D-17 | `snapshot` no es una operación gobernada: no se añade a BR-VAL-002 (BR-MCP-001 v0.3). | No escribe en el repo ni lanza `git`; no tendría capa hooks. La protegen allowlist, solicitante, cuota y rate limit. |
| D-18 | `undo` no pasa por Guardrails en el MVP; US-TMC-021 queda como relación de Fase 2 en US-MCP-012. | La política del undo es Fase 2; rige la regla base de ADR-TMC-005. |
| D-19 | Avisos del plan: parámetro `acknowledge` que los nombra exactamente, en una segunda llamada; sin él, rechazo sin efectos (ADR-MCP-001 § 4.3). Cierra D-16; US-MCP-018 reescribe su escenario de rama ya empujada. | ADR-CKP-002 § 2 exige enumerar los avisos; el agente los ve antes de aceptarlos. |
| D-20 | Se levanta el hueco de D-13: el lado del canal del confused deputy ya está en main (`daemon-descendant`); el rechazo en preparar, ejecutar y cancelar llega con TS-CKP-002, del que US-MCP-007 ya depende. | Comprobado en `crates/core/src/channel/conn.rs`. |
| D-21 | Captura manual de la Time Machine: nivel `manual`, etiqueta, 5/min y ≤ 20 por solicitante y worktree en 24 h, fuera de la pila de `undo`, sin borrar nunca para hacer sitio ni desplazar previos (ADR-TMC-004, Enmienda (2026-10-05, MCP)). Cierra el bloqueo de arquitectura de D-14. | Destino de DEP-MCP-2. |
| D-22 | Cifras de S-MCP-1 fijadas en ADR-MCP-001 § 6; la Dev Spec solo puede endurecerlas. | Respuestas por debajo del umbral de 50.000 caracteres de Claude Code. |
| D-23 | Se levantan todos los bloqueos de arquitectura (`blocked_by` de las fichas): ADR-MCP-001, ADR-CKP-001 y ADR-CKP-002 aceptados; DEP-MCP-3, 4, 6 y 8 aplicadas. Solo queda el bloqueo de producto de US-MCP-014. | Criterio de bloqueo de este índice: se levanta al aceptarse el artefacto. |
| D-25 | Perfil `mcp` por solicitante (S-01): un agente que usa la CLI `raptor` o habla directo con el socket recibe la vista MCP y la allowlist. La atribución sale del proceso, nunca del cwd: el desarrollador en el worktree de un agente conserva su vista. US-CKP-011 ("`raptor status` igual que la TUI") vale para el desarrollador; anotado para el PO del Cockpit (no se edita aquí). | Hallazgo High de seguridad; refuerza Q-MCP-3. |
| D-26 | Por MCP, un `undo` que movería la rama base confirmada o una ref protegida se rechaza antes de cualquier efecto, hasta US-TMC-021 (S-04; BR-MCP-ELIG-005). | Endurecimiento coherente con BR-TMC-AUTH-001. |
| D-24 | S-MCP-3 verificado en macOS con una prueba local (Claude Code 2.1.284): cwd = directorio de arranque de la sesión, padre `claude`, entorno heredado (ADR-MCP-001, "Evidencia de S-MCP-3"). | La documentación oficial no lo documenta. |

---

## Índice de Historias

| ID | Título | Descripción (1 línea) | Status |
|----|--------|-----------------------|--------|
| [US-MCP-001](./user-stories/US-MCP-001-instalar-en-claude-code.md) | El desarrollador conecta Claude Code a GitRaptor con un solo comando y lo retira igual de fácil | Desarrollador quiere `raptor mcp install`/`uninstall` idempotentes, solo para Claude Code | implemented (PR #70) |
| [US-MCP-002](./user-stories/US-MCP-002-habilitar-repo-allowlist.md) | El desarrollador decide qué repos pueden usar los agentes por el MCP | Desarrollador quiere una allowlist opt-in, reservada, ⊆ observados y en cascada | implemented (PR #140, #159) |
| [US-MCP-003](./user-stories/US-MCP-003-status-del-repo-del-agente.md) | Un agente consulta por MCP el estado del repo en el que trabaja, y de ningún otro | Esqueleto andante: `status` con ámbito por cwd, allowlist y motor bajo demanda | implemented (PR #140, #159 (ajuste en #153)) |
| [US-MCP-004](./user-stories/US-MCP-004-status-completo-y-no-disponible.md) | Un agente sabe por MCP quién más trabaja en su repo, contra qué base y con qué protección | Agente quiere el `status` completo y que un repo no disponible no dé datos | expanded |
| [US-MCP-005](./user-stories/US-MCP-005-respuestas-acotadas-y-seguras.md) | Un agente recibe respuestas acotadas que no pueden darle órdenes ni filtrar secretos | Desarrollador quiere respuestas acotadas, texto no confiable, errores estables y rate limit | implemented (PR #157, #159) |
| [US-MCP-006](./user-stories/US-MCP-006-registrar-agente.md) | Un agente sin soporte completo se declara por MCP y su trabajo queda a su nombre | Desarrollador quiere `register_agent`/`unregister_agent` y que "sin atribuir" solo lea | expanded |
| [US-MCP-007](./user-stories/US-MCP-007-confused-deputy.md) | Un hook lanzado por una escritura de un agente no puede usar los poderes del desarrollador | Desarrollador quiere cerrar el confused deputy antes de cualquier escritura | expanded |
| [US-MCP-008](./user-stories/US-MCP-008-snapshot-manual.md) | Un agente guarda por MCP un punto de recuperación antes de un cambio arriesgado | Desarrollador quiere `snapshot` manual con etiqueta, cuota y rate limit propios | expanded |
| [US-MCP-009](./user-stories/US-MCP-009-safe-commit.md) | Un agente commitea por MCP con las reglas del repo y con un punto de recuperación previo | Desarrollador quiere `safe_commit` decidido por Guardrails y con snapshot previo | expanded |
| [US-MCP-010](./user-stories/US-MCP-010-safe-commit-rutas.md) | Un agente commitea solo los archivos que nombra y nunca fuera de su worktree | Desarrollador quiere rutas literales, sin ignorados y rutas explícitas en compartido | expanded |
| [US-MCP-011](./user-stories/US-MCP-011-escritura-en-el-worktree-correcto.md) | La escritura de un agente cae en el worktree que espera, o no ocurre | Desarrollador quiere `expect_worktree` y rechazo con operación en curso o HEAD separado | expanded |
| [US-MCP-012](./user-stories/US-MCP-012-undo-de-lo-propio.md) | Un agente deshace por MCP su última operación sin tocar el trabajo de otros | Desarrollador quiere `undo` de lo propio que se detiene ante solape | expanded |
| [US-MCP-013](./user-stories/US-MCP-013-confirmacion-sin-cola.md) | Un agente sabe que una acción necesita al desarrollador y a quién acudir | Desarrollador quiere "pedir confirmación" como denegar con acción mientras no hay cola | expanded |
| [US-MCP-014](./user-stories/US-MCP-014-confirmacion-pendiente-en-cola.md) | La acción de riesgo de un agente queda en espera del desarrollador sin bloquear al agente | Desarrollador quiere "pendiente" con id cuando exista la cola (bloqueada: US-GRD-015) | expanded |
| [US-MCP-015](./user-stories/US-MCP-015-medicion-del-mcp.md) | El desarrollador mide cuánto paró y cuánto recuperó el MCP | Desarrollador quiere bloqueos por capa, undos por MCP e indicadores de seguridad | expanded |
| [US-MCP-016](./user-stories/US-MCP-016-check-conflicts.md) | Un agente sabe con quién va a chocar antes de que choque | Agente quiere la predicción publicada para su worktree, sin contenido | expanded |
| [US-MCP-017](./user-stories/US-MCP-017-explain-history.md) | Un agente entiende qué pasó en su repo sin ver mensajes ni contenido | Agente quiere eventos y timeline acotados, con filtro y paginación | expanded |
| [US-MCP-018](./user-stories/US-MCP-018-safe-rebase-atomico.md) | Un agente se pone al día con la rama base y, si choca, su rama queda como estaba | Desarrollador quiere `safe_rebase` atómico con abort automático | expanded |
| [US-MCP-019](./user-stories/US-MCP-019-create-worktree.md) | Un agente prepara un worktree nuevo desde la rama base para otro trabajo | Desarrollador quiere `create_worktree` desde la base confirmada, sin elegir ruta | expanded |

---

## Mapa para la flota

> Un agente por historia y por worktree (D3). "Depende de" = historias que deben estar integradas antes de empezar. Las historias de motor-local se nombran US-GRP-NNN. Todas salvo US-MCP-002 tienen además el bloqueo de arquitectura ADR-MCP-001, que no se repite en "Externas". Los IDs de reglas omiten el prefijo `BR-MCP-`. TS-CKP-* y US-CKP-* son propuestas en otras ramas (docs/arch-cockpit y docs/stories-cockpit).

| ID | Reglas cubiertas | Depende de | Externas | Prioridad | Valor en una línea |
|----|------------------|------------|----------|-----------|--------------------|
| US-MCP-001 | WF-007, EDGE-009 | — | CLI de Claude Code (S-MCP-5); SEC-14 | Must | El MCP se instala en un paso y se retira sin rastro |
| US-MCP-002 | WF-006, CONS-003, AUTH-004 (allowlist reservada) | US-GRP-001, US-GRP-006, US-GRD-004 | DEP-MCP-4 y la mitad de DEP-MCP-3 sobre la allowlist (sin ADR-MCP-001: CLI y perfil) | Must | Observar no expone: el desarrollador habilita cada repo |
| US-MCP-003 | CALC-001, ELIG-006, ELIG-001, EDGE-004, CONS-001, TIME-003, EDGE-001 | US-MCP-002, US-GRP-001, US-GRP-007, TS-GRP-003, TS-GRP-004, TS-TMC-004 (draft, complejidad alta: ruta del esqueleto) | DEP-MCP-9 (no bloquea) | Must | El agente sabe dónde está y quién es, sin ver otros repos |
| US-MCP-004 | CALC-003, EDGE-005, EDGE-002 (lectura) | US-MCP-003, US-GRP-003, US-GRP-005, US-GRP-011, US-GRP-012, US-GRD-004, US-GRD-014 | SEC-11, ADR-GRP-009 | Must | El agente se coordina con datos del motor |
| US-MCP-005 | CALC-002, VAL-006, CONS-004, CONS-005, VAL-005 (parámetros), TIME-001 (conexión) | US-MCP-003 | DEP-MCP-8 | Must | El MCP no es un vector de ataque |
| US-MCP-006 | WF-005, VAL-004 (nombre), AUTH-002 | US-MCP-003, US-GRP-007, US-GRP-009 | ADR-GRP-005 § 6.6 | Must | Toda escritura de un agente tiene autor |
| US-MCP-007 | AUTH-005, AUTH-004 | US-MCP-002, US-MCP-003, TS-CKP-002, TS-CKP-003 | ADR-CKP-002 (propuesto), DEP-MCP-3; **dueño del lado del canal sin asignar** (propuesta del Arquitecto: TS-CKP-002; D-13) | Must | Ningún agente usa los poderes del desarrollador a través de sus hooks |
| US-MCP-008 | ELIG-005 (snapshot), VAL-004 (etiqueta), TIME-001 (cuota), AUTH-002, EDGE-008 (snapshot) | US-MCP-005, US-MCP-006, US-MCP-007, US-MCP-009 (Dev Spec), US-TMC-001, US-TMC-016, TS-TMC-004, TS-CKP-002, TS-CKP-003 | ADR-CKP-002 (propuesto; operación normalizada del snapshot pendiente); interfaz de captura manual de la Time Machine (pendiente en ADR-CKP-002, D-14). Dueña de `snapshot` | Must | El agente guarda un punto sin poder llenar el disco |
| US-MCP-009 | WF-001, ELIG-002, VAL-003, AUTH-001, CONS-002, TIME-002 | US-MCP-005, US-MCP-006, US-MCP-007, US-TMC-001, TS-TMC-004, US-GRD-001, US-GRD-007, US-GRD-009, TS-CKP-002, TS-CKP-003 | ADR-CKP-002 (propuesto; DEP-MCP-2 y DEP-MCP-5 vía ADR-CKP-002). Dueña de `commit` | Must | La vía cómoda del agente es la que no pierde trabajo |
| US-MCP-010 | VAL-001, EDGE-010, EDGE-003 (commit) | US-MCP-009, US-MCP-011, US-GRP-011 | ADR-CKP-002 (propuesto) | Must | Ni traversal ni secretos ni trabajo ajeno en un commit |
| US-MCP-011 | VAL-005 (`expect_worktree`), EDGE-006, AUTH-003 (escritura), EDGE-008, EDGE-002 (commit) | US-MCP-009, US-TMC-015 | ADR-CKP-002 (propuesto; precondiciones del ejecutor) | Must | Un subagente nunca escribe en el worktree de su padre |
| US-MCP-012 | ELIG-005 (undo), AUTH-003 (undo), EDGE-003 (solape), AUTH-002 | US-MCP-007, US-MCP-009, US-TMC-002, US-TMC-012, US-TMC-013, US-TMC-021 | — (undo fuera del catálogo: ADR-TMC-005) | Must | El agente corrige sus errores sin borrar los de otros |
| US-MCP-013 | WF-004 (denegar) | US-MCP-009, US-GRD-007 | ADR-CKP-002 (propuesto) | Must | "Pedir confirmación" se respeta desde la primera escritura |
| US-MCP-014 | WF-004 (pendiente) | US-MCP-013 | **Bloqueada**: US-GRD-015 (cola), a su vez bloqueada por el Cockpit F-001-02 y el factor fuera de banda del SO (Q-GRD-19, D5; ADR sin crear); DEP-MCP-7. Relación: US-CKP-023 (cola en el Cockpit) | Should | El humano decide lo arriesgado sin frenar al agente |
| US-MCP-015 | CONS-006 | US-MCP-009, US-MCP-012, US-MCP-013, US-GRD-005, US-TMC-006 | — | Should | Los KPIs del MCP se miden |
| US-MCP-016 | CALC-004 | US-MCP-003, US-MCP-005, TS-CKP-001 | ADR-CKP-001 (propuesto), DEP-MCP-6 | Must | El agente evita el choque antes de que ocurra |
| US-MCP-017 | CALC-005, VAL-002 (filtro) | US-MCP-003, US-MCP-005, US-TMC-006, US-TMC-007, TS-TMC-004 (consulta del timeline) | — (DEP-MCP-6 ya no la bloquea; eventos por ADR-GRP-013, aceptado; D-12) | Must | El agente entiende qué pasó sin ver contenido |
| US-MCP-018 | ELIG-003, WF-002, WF-003, EDGE-007, EDGE-002 (rebase), EDGE-003 (rebase) | US-MCP-007, US-MCP-009, US-MCP-011, US-GRD-001, US-GRD-008, US-GRD-014, TS-CKP-002, TS-CKP-003 | ADR-CKP-002 (propuesto; DEP-MCP-2 y DEP-MCP-5 vía ADR-CKP-002); avisos del plan pendientes en ADR-MCP-001 (D-16). Dueña del modo `atomic` de `rebase-onto-base`; relación: US-CKP-015 | Must | Ningún worktree queda a medias por un rebase del agente |
| US-MCP-019 | ELIG-004, VAL-002, EDGE-002 (worktree), VAL-001 (sin ruta) | US-MCP-007, US-MCP-009, US-MCP-011, US-GRD-014, TS-CKP-002, TS-CKP-003 | ADR-CKP-002 (propuesto; solo plantilla, H-02). Comparte `create-worktree` con US-CKP-018: la implementa la primera que entre | Must | El trabajo en paralelo empieza aislado y en un sitio predecible |

### Orden y paralelismo (DAG)

```
[ADR-MCP-001 ─ bloqueo de arquitectura de todas salvo MCP-002]
[ADR-CKP-002 aceptado ─► TS-CKP-002 ─► TS-CKP-003 ─ habilitadores de las escrituras]

MCP-001 (independiente)

US-GRP-001, US-GRP-006, US-GRD-004 ─► MCP-002 [DEP-MCP-3, DEP-MCP-4] ─► MCP-003 ─┬─► MCP-004
  (+ US-GRP-007, TS-GRP-003/004, TS-TMC-004 ← draft, complejidad alta)          ├─► MCP-005 ─┬─► MCP-016 (+ TS-CKP-001) [ADR-CKP-001, DEP-MCP-6]
                                                                                 │            └─► MCP-017 (+ TS-TMC-004)
                                                                                 ├─► MCP-006 (+ US-GRP-009)
                                                                                 └─► MCP-007 (+ 002, TS-CKP-002/003) [ADR-CKP-002, DEP-MCP-3, dueño del lado del canal]

MCP-005, MCP-006, MCP-007 ─► MCP-009 (+ US-TMC-001, US-GRD-001, 007, 009, TS-CKP-002/003) [ADR-CKP-002]
                                   ├─► MCP-008 (tras la Dev Spec de 009; + US-TMC-001, 016) [ADR-CKP-002, captura manual TMC]
                                   ├─► MCP-011 ─┬─► MCP-010
                                   │            ├─► MCP-018 (+ US-GRD-008, 014; rel. US-CKP-015)
                                   │            └─► MCP-019 (+ US-GRD-014; comparte con US-CKP-018)
                                   ├─► MCP-012 (+ US-TMC-002, 012, 013, 021)
                                   └─► MCP-013 (+ US-GRD-007) ─► MCP-014 [US-GRD-015; rel. US-CKP-023]
MCP-009, MCP-012, MCP-013 ─► MCP-015 (+ US-GRD-005, US-TMC-006)

MCP-002, MCP-003, MCP-009 ─► US-GRD-016 (Guardrails) queda desbloqueada
```

- **Ola 1** (entrega 1 de Q-MCP-19: canal, allowlist, `status`, install y registro): US-MCP-001 y US-MCP-002 en paralelo; luego US-MCP-003 (esqueleto andante); luego US-MCP-004, US-MCP-005 y US-MCP-006 en paralelo.
- **Ola 2** (entrega 2: confused deputy): US-MCP-007. Requisito previo de toda escritura.
- **Ola 3** (entrega 3: snapshot/undo y `safe_commit`): US-MCP-009 primero. Cuando su Dev Spec fije el flujo de escritura, US-MCP-008, US-MCP-011, US-MCP-012 y US-MCP-013 en paralelo. Después US-MCP-010 (tras 011) y US-MCP-015. US-MCP-014 queda bloqueada (US-GRD-015).
- **Ola 4** (entrega 4: lecturas que dependen de otras features): US-MCP-016 y US-MCP-017. Solo dependen de la ola 1. US-MCP-017 se puede adelantar en cuanto exista ADR-MCP-001 con la enmienda de ADR-GRP-013 (vista MCP del timeline, aplicada el 2026-10-05); US-MCP-016, cuando se levanten ADR-CKP-001 y DEP-MCP-6 (D-7, D-12).
- **Ola 5** (entrega 5: rebase y worktree): US-MCP-018 y US-MCP-019 en paralelo.

> **Secuencias por contrato compartido**: 003 → 004/005 (contrato de respuesta de `status`), 005 → 016/017 (respuesta acotada y paginación), 009 → 008/011 (flujo de escritura), 011 → 010 (parámetros de `safe_commit`) y 011 → 018/019 (`expect_worktree` y precondiciones del ejecutor). En cada caso el contrato lo fija la Dev Spec de la historia que va primero.
>
> **Ruta crítica de las escrituras**: ADR-CKP-002 aceptado → TS-CKP-002 → TS-CKP-003 → MCP-007 → MCP-009 → MCP-011 → MCP-018.
>
> **Ruta del esqueleto andante**: ADR-MCP-001 → MCP-002 → MCP-003. TS-TMC-004 (Time Machine; draft, complejidad alta) bloquea a MCP-003 en esta ruta: es el eslabón que más puede retrasar la ola 1.

### US-GRD-016 (Guardrails) queda desbloqueada por esta feature

US-GRD-016 ("Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo") espera las herramientas y la allowlist. La desbloquean US-MCP-002 (allowlist y capa MCP), US-MCP-003 (canal y ámbito) y US-MCP-009 (primera escritura con decisión de Guardrails, BR-MCP-AUTH-001). Su estado no cambia en este índice.

**Discrepancia para Guardrails** (no se edita aquí): los escenarios de US-GRD-016 piden por "una herramienta MCP" operaciones que el catálogo del MCP no tiene (push, force-push, `reset --hard`, merge, borrar rama o worktree; Q-MCP-1). Por MCP solo existen commit, rebase, crear worktree y snapshot. El PO de Guardrails debe acotar esos escenarios a las herramientas del catálogo y dejar el resto a la capa de hooks, como ya prevé la demo en dos partes (Q-MCP-18, ajuste del PO: "demo sin herramienta de push").

### Prueba de aceptación de la feature: demo del BRD § 13 en dos partes

No es criterio de ninguna historia. Se ejecuta cuando estén integradas US-MCP-002, US-MCP-003, US-MCP-009, US-GRD-001 y US-GRD-016:

1. **Por MCP**: en un repo habilitado, con una regla del equipo que deniega la operación, `safe_commit` (o `safe_rebase` de la rama base, tras US-MCP-018) se rechaza antes de ejecutar, con la regla y la acción.
2. **Por Git crudo**: el mismo agente intenta un force-push con Git directo y el hook lo para con la misma decisión. Las dos decisiones quedan en el registro con su capa (US-MCP-015).

---

## Cobertura de reglas (regla → historias)

Los IDs omiten el prefijo `BR-MCP-`. Fuente: el campo `covers` de cada ficha.

| Regla | Historias | Regla | Historias |
|-------|-----------|-------|-----------|
| VAL-001 | US-MCP-010, 019 | WF-001 | US-MCP-009 |
| VAL-002 | US-MCP-017, 019 | WF-002 | US-MCP-018 |
| VAL-003 | US-MCP-009 | WF-003 | US-MCP-018 |
| VAL-004 | US-MCP-006, 008 | WF-004 | US-MCP-013, 014 |
| VAL-005 | US-MCP-005, 011 | WF-005 | US-MCP-006 |
| VAL-006 | US-MCP-005 | WF-006 | US-MCP-002 |
| CALC-001 | US-MCP-003 | WF-007 | US-MCP-001 |
| CALC-002 | US-MCP-005 | AUTH-001 | US-MCP-009 |
| CALC-003 | US-MCP-004 | AUTH-002 | US-MCP-006, 008, 012 |
| CALC-004 | US-MCP-016 | AUTH-003 | US-MCP-011, 012 |
| CALC-005 | US-MCP-017 | AUTH-004 | US-MCP-002, 007 |
| ELIG-001 | US-MCP-003 | AUTH-005 | US-MCP-007 |
| ELIG-002 | US-MCP-009 | CONS-001 | US-MCP-003 |
| ELIG-003 | US-MCP-018 | CONS-002 | US-MCP-009 |
| ELIG-004 | US-MCP-019 | CONS-003 | US-MCP-002 |
| ELIG-005 | US-MCP-008, 012 | CONS-004 | US-MCP-005 |
| ELIG-006 | US-MCP-003 | CONS-005 | US-MCP-005 |
| TIME-001 | US-MCP-005, 008 | CONS-006 | US-MCP-015 |
| TIME-002 | US-MCP-009 | EDGE-001 | US-MCP-003 |
| TIME-003 | US-MCP-003 | EDGE-002 | US-MCP-004, 011, 018, 019 |
| EDGE-006 | US-MCP-011 | EDGE-003 | US-MCP-010, 012, 018 |
| EDGE-007 | US-MCP-018 | EDGE-004 | US-MCP-003 |
| EDGE-008 | US-MCP-008, 011 | EDGE-005 | US-MCP-004 |
| EDGE-009 | US-MCP-001 | EDGE-010 | US-MCP-010 |

**Resultado**: las 48 reglas tienen al menos una historia. Solo una variante la cubre únicamente una historia bloqueada por producto: la respuesta "pendiente" de BR-MCP-WF-004 (US-MCP-014); su variante "denegar" la entrega US-MCP-013. BR-MCP-AUTH-001 se ejercita además en US-MCP-013 y US-MCP-018, BR-MCP-TIME-001 en US-MCP-009 y US-MCP-018, BR-MCP-EDGE-008 en US-MCP-018, y la matriz BR-MCP-ELIG-001 en cada herramienta de escritura. BR-MCP-VAL-001 y BR-MCP-ELIG-004 ya dicen, desde la enmienda BR-MCP-001 v0.2, que `create_worktree` no acepta ruta por MCP (D-6). Q-MCP-7, en context.md, queda estrechada por esa enmienda.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-04 | PO (AADD) para Rene Bonilla | Versión inicial en modo Bulk (modelo plano, modo lean): 19 historias en 5 olas según Q-MCP-19, mapa para la flota, DAG y cobertura de las 48 reglas. Bloqueo de arquitectura común: ADR-MCP-001. Bloqueo de producto: US-MCP-014 (US-GRD-015). Decisiones D-1 a D-7 del orquestador, validadas por PO |
| 1.1 | 2026-10-04 | PO (AADD) para Rene Bonilla | Reservas del Juez AADD y ajustes del Arquitecto (D-8 a D-16; D-1, D-3, D-4, D-6 y D-7 actualizadas). Frontmatter de las 19: `ado` y `related.adrs`. Prioridad high en 013, 016, 017 y 019. US-MCP-002 sin ADR-MCP-001. ADR-CKP-002 en `blocked_by` de todas las escrituras del catálogo; DEP-MCP-2 y DEP-MCP-5 vía ADR-CKP-002. Habilitadores y relaciones del Cockpit con IDs propuestos (TS-CKP-001/002/003, US-CKP-015/018/023), sustituyendo los "historias en curso". Dueños de operación del catálogo. Hueco del lado del canal del confused deputy (007) y captura manual de la Time Machine (008) como bloqueos. 008 tras la Dev Spec de 009; 010 tras 011; 017 sin DEP-MCP-6. Escenarios nuevos o reescritos en 005, 007, 009, 018 y 019. Enmienda BR-MCP-001 v0.2 (create_worktree sin ruta; snapshot pendiente en ELIG-001). Pendientes para la Fase 2: INF-MCP-001 (propuesto, no creado), la puerta MCP Top 10 y los avisos del plan en ADR-MCP-001. Ruta crítica nueva. Siguen cubiertas las 48 reglas |
| 1.2 | 2026-10-05 | Orquestador (Fase 2, ADR-MCP-001) para Rene Bonilla | D-17 a D-26 validadas por Arquitecto/PO (D-25 y D-26 por la revisión de seguridad). D-12 corregida (timeline sin marca MCP en main). Sin bloqueos de arquitectura; solo US-MCP-014 sigue bloqueada (producto). US-MCP-018: escenario de rama ya empujada con `acknowledge`. US-MCP-008: sin decisión de Guardrails y con la captura manual de ADR-TMC-004. US-MCP-012: US-TMC-021 como relación de Fase 2. US-MCP-007: lado del canal resuelto. INF-MCP-001 creado en technical-stories.md |
