---
id: TS-TMC-005
title: "El undo espera, con plazo propio, la captura que da su punto base B en vez de responder `overlap-unverified` de inmediato"
type: ts
status: implemented
feature: time-machine
domain: GRP
priority: medium
complexity: medium
milestone: M2
created: 2026-11-08
updated: 2026-11-10
related:
  adrs: [ADR-TMC-005, ADR-TMC-004, ADR-TMC-002, ADR-TMC-003]
  stories: [US-TMC-012, US-TMC-011, US-TMC-010]
  specs: [DS-US-TMC-012, DS-US-TMC-011]
ado:
  id: null
  url: null
tags: [time-machine, undo, solape, overlap-unverified, punto-base, captura-continua, nfr-01, d7]
---

## TS-TMC-005: El undo espera, con plazo propio, la captura que da su punto base B en vez de responder `overlap-unverified` de inmediato

**Valor**: un agente que hace `raptor undo` justo después de un `git reset --hard` crudo recupera su trabajo a la primera, también en una máquina cargada, sin que el undo deshaga más de lo que deshace hoy tras un reintento.

### Descripción

**Como** responsable de la Time Machine
**Quiero** que, cuando falta el punto base B de un cambio de agente pero su captura ya está en camino, el undo espere esa captura con un plazo propio y una señal explícita
**Para** que `overlap-unverified` (D7 de DS-US-TMC-012) quede para los casos en que B de verdad no se puede obtener, y no para una carrera con la captura continua

> Implementado en: PR #350 (2026-11-09, sin certificar). Plan: [`ts-tmc-005.2-plan.md`](../../../../dev-briefs/ts-tmc-005.2-plan.md).
>
> Dev Spec: no hace falta una propia. El cambio entra en los Requisitos Técnicos de US-TMC-012 y lo implementa `/implement` desde su Brief, con esta TS como contrato.
>
> **Depende de**: US-TMC-012 (solape, implementada) y la captura continua de US-TMC-004. **Relacionada**: US-TMC-011 (undo por agente, PR #325 en curso) y US-TMC-010 (`undo --since`), que eligen B con la misma función. **ADRs**: ADR-TMC-005, Nota (2026-11-08, TS-TMC-005).

**Origen.** PR #317, sección «Pendiente» (W120, PRs #317 y #318). El test `apps/cli/tests/raw_git_undo.rs` y `undo_overlap_same_session` esperan la captura antes del undo porque, sin esa espera, en un runner cargado el undo llega antes que la captura y se rechaza. **Decisión del orquestador (2026-11-08), validada por Arquitecto.**

**Hoy.** El cambio que se deshace es el evento del reset (`OpRef::GitEvent(seq)`). Su base B es la captura con `cause_event_seq = seq` o, si no la hay, el primer punto limpio con marca ≥ `seq` (`overlap::base_point_of`). El motor persiste el evento y la captura continua lo toma «en el acto», pero antes espera a que el motor esté en calma (hasta 1 s, `CAPTURE_SETTLE_LIMIT`), captura un worktree cada vez y cede ante actividad nueva. Si el undo planifica antes, B no existe, la referencia es un agente y la fase 1 devuelve `overlap-unverified`. El reintento funciona porque, para entonces, la captura ya existe.

**Decisión y alternativas.**

| Opción | Gana | Cuesta | Veredicto |
|---|---|---|---|
| (a) Dejarlo como está: falla cerrado y el cliente reintenta | Ningún cambio; cumple NFR-01 | Cada agente tiene que saber que `overlap-unverified` justo después de Git crudo se reintenta; un agente que no lo sabe da su trabajo por perdido o pide ayuda a la persona. Rompe el criterio de salida 3 de M1 en máquinas cargadas | Descartada |
| **(b) Esperar B con plazo propio y señal explícita; si vence, `overlap-unverified`** | El primer intento obtiene el mismo B que obtendría el reintento: misma semántica y ningún riesgo nuevo de deshacer de más | Hasta 2 s más de latencia del undo en el peor caso, solo cuando B está en camino; una señal nueva entre la captura continua y el planificador | **Elegida** |
| (c) Usar como B el previo del propio undo | Sin espera | El previo contiene el reset **y todo lo posterior**: ediciones de la persona o de otro agente entre el reset y el undo pasan a ser «el cambio deshecho» y se sobrescriben. Desaparecen los intervalos posteriores a B, así que la detección del solape queda vacía y no puede ver nada. Además, el previo se toma después de la fase 1, que necesita B antes: invierte el orden de ADR-TMC-005 § 4 | Descartada: viola NFR-01 (deshace de más) |

**Por qué (c) no separa el cambio del agente de lo posterior.** El previo es una foto del estado actual. No distingue qué parte la produjo el reset y qué parte vino después, porque una edición sin Git es «sin atribuir» y no deja evento (ADR-GRP-013; D3 de DS-US-TMC-012). La única frontera fiable entre «lo que hizo el reset» y «lo que vino después» es un punto capturado cerca del evento. G4 de DS-US-TMC-012 ya acepta que una edición ajena entre el evento y su captura cuente como parte de lo deshecho, pero esa ventana la fija la captura (milisegundos o segundos). Con (c), la ventana iría del reset al momento del undo, sin límite.

**Por qué (b) no amplía G4.** El instante de la captura lo decide la captura continua, no la espera: esperar no cambia qué estado queda en B, solo evita preguntar antes de que exista. B es idéntico al que encuentra el reintento de (a).

### Alcance Técnico

- **Señal de captura sellada.** La captura continua publica, por repo y worktree, cada punto que sella (marca y `cause_event_seq`) y cada captura pendiente que abandona (`MAX_RETRIES` agotado, repo olvidado). Puede ser un canal o una variable de condición en el daemon. El planificador se suscribe; **nunca hay `sleep` ni sondeo**.
- **Cuándo se espera.** Solo si se cumplen las tres condiciones: (1) `base_point_of` no encuentra B, (2) la referencia es un agente (con «sin atribuir» B no hace falta, D7) y (3) la captura continua tiene pendiente o en curso una captura del worktree que puede producir B (un evento con seq ≥ el del cambio deshecho). Sin motor, sin captura pendiente o con la captura abandonada, la respuesta es `overlap-unverified` de inmediato, como hoy.
- **Plazo propio**, `BASE_WAIT` = 2 s (⚠️ **ASSUMPTION**: calma de 1 s + una captura del perfil `M` + reintentos de `SETTLE`; se mide en T-uso). No sale de los 1,5 s de las dos fases (`OVERLAP_BUDGET`): ese plazo empieza a contar cuando ya hay B, para que la espera no recorte el cálculo. Al vencer: `overlap-unverified`, sin cambios en el repo, con el rechazo en el oplog como hoy.
  - **Medida (2026-11-09, macOS, máquina compartida con carga media de 9 a 12): no concluyente; `BASE_WAIT` sigue en 2 s como ASSUMPTION.** Se corrió 13 veces `raw_git_undo.rs::an_agent_recovers_its_work_after_a_raw_reset_hard`, que imprime el `base_wait_ms` de `tm_base_wait`. En las 3 ejecuciones que llegaron al undo del agente no hubo espera: no se registró ningún `tm_base_wait`, porque la captura ya estaba sellada (el undo del desarrollador sin confirmar va antes). Las otras 10 fallaron antes del undo por la causa de entorno ya conocida: `:280` («never captured», la captura previa al reset) o `:370` (reset «sin atribuir», TD-GRP-007). No se midió una ejecución en reposo porque la máquina no lo estuvo. Los tests de `crates/core` retienen la captura 500 ms (CA-1) y 5 s (CA-2) y confirman la mecánica, no la cifra. Queda pendiente medir en un runner en reposo y en uno cargado: ese test (solo macOS) y los dos de CA-1 escriben `tm_base_wait` en el resumen del job de CI (`GITHUB_STEP_SUMMARY`), también en Windows, y la cifra que salga se anota en el PR de esta TS.
  - **Enmienda del contrato (2026-11-09, aprobada por el coordinador del PR #350).** En el CI de Windows el undo tarda por sí solo de 2,3 a 2,5 s, sin esperar: sin motor no hay espera posible y aun así tardó 2,27 s. CA-3 medía la espera por el tiempo total del undo y caía por la lentitud del runner, no por esperar. Ahora la mide por lo que registra el undo: `not-pending` con `base_wait_ms` < 100 cuando la captura se abandonó, y ninguna línea `tm_base_wait` sin motor. La intención del criterio no cambia. CA-1 añade a su mensaje de fallo el tiempo del undo y los `tm_base_wait` registrados, para saber si en Windows vence `BASE_WAIT` (`expired`) o el plazo del solape. Si es `expired`, `BASE_WAIT` se fija con la cifra medida y CA-2 la sigue.
  - **Supuesto cuestionado en Windows (2026-11-10).** En el run 42767991117 del PR #352 (job `lint and test (windows-latest)`, suite de 129 s) `ca6_the_base_after_waiting_is_the_one_a_later_retry_gets` falló: el undo devolvió `Overlap { conflicts: [], last_point: None, unverified: true }` en lugar de terminar, tras retener la captura solo 500 ms. La hipótesis más consistente es que `BASE_WAIT` (2 s, de tiempo de pared) venció bajo carga; no está confirmada, porque el log del fallo no trae la línea `tm_base_wait`. Si se confirma, no es solo un problema del test: un usuario real en un Windows cargado (procesos git, antivirus, timer de ~15,6 ms) recibiría `overlap-unverified` por lo mismo. Es seguro (rechaza sin tocar el repo, NFR-01), pero degrada la recuperación. **`BASE_WAIT` no se cambia aquí** y sigue siendo ASSUMPTION; queda pendiente fijarlo con la cifra de `tm_base_wait` de los resúmenes de CI en Windows (idealmente varias corridas y con carga). El PR #356 acota el reintento de CA-6 a `expired` y deja el log en el mensaje de fallo. **CA-1 y CA-5 tienen el mismo riesgo latente**: retienen la captura 500 ms con el mismo `BASE_WAIT` y exigen que el undo termine; no fallaron en ese run, pero pueden hacerlo igual.
- **Dónde se espera.** En la fase 1, en el sitio del orden de ADR-TMC-005 § 4, con el cerrojo de escritura del repo ya tomado (la captura continua no lo usa, y soltarlo dejaría cambiar el conjunto deshecho y obligaría a canjear la confirmación dos veces) y sin retener el lock del oplog ni el writer del almacén: la captura los necesita para sellar el punto, así que esperar con ellos tomados sería un interbloqueo. Tras la señal, el planificador vuelve a leer operaciones y snapshots y aplica `base_point_of` sin cambios (generación, punto limpio, utilizable). **Decisión del orquestador (2026-11-09), validada por Arquitecto.**
- **Un solo camino.** La espera vive junto a la elección de B que comparten el undo simple, `undo --since` (US-TMC-010), el undo por agente (US-TMC-011) y el redo. No se duplica por comando.
- **Observabilidad.** Log del daemon con `base_wait_ms` y el resultado (`found`, `expired`, `not-pending`, `abandoned`). Sin rutas ni ramas (SEC-12). Por MCP la respuesta no cambia: motivo y recuentos.
- **Tests.** `raw_git_undo.rs` y `undo_overlap_same_session` dejan de esperar la captura antes del undo en el paso del agente; la espera pasa a ser responsabilidad del daemon.
- **Fuera de alcance**: cambiar D7 (sin B sigue fallando cerrado), usar el previo como base, atribuir ediciones (G2), acelerar la captura continua y el solape de la restauración (US-TMC-024).

### Plan de Verificación

#### Pruebas Automatizadas

| # | Criterio | Verificación | Gate de CI |
|---|---|---|---|
| CA-1 | Un agente que hace `raptor undo` justo tras un `git reset --hard` crudo, sin esperar la captura, recupera su trabajo a la primera | `apps/cli/tests/raw_git_undo.rs::an_agent_recovers_its_work_after_a_raw_reset_hard` sin la espera previa al undo, más un test de `crates/core` con la captura retenida 500 ms por la capa de fallos (`CaptureLayer`) | `cargo test` del job de CI (Linux) |
| CA-2 | Con la captura retenida más que `BASE_WAIT`, el undo responde `overlap-unverified` en ≤ `BASE_WAIT` + `OVERLAP_BUDGET`, sin cambios en el repo y con el rechazo en el oplog | Test de `crates/core` con la captura retenida 5 s; huella del repo intacta (INF-GRP-001) | `cargo test` |
| CA-3 | Sin captura pendiente (abandonada, o sin motor) no hay espera: `overlap-unverified` en menos de 100 ms más el plan | Test de `crates/core` con la captura abandonada por `MAX_RETRIES` | `cargo test` |
| CA-4 | Con referencia «sin atribuir» no se espera nunca | Test de `crates/core`: undo «sin atribuir» con la captura retenida, sin `base_wait_ms` > 0 | `cargo test` |
| CA-5 | La espera no retiene el lock del oplog ni el writer del almacén: la captura sella mientras el undo espera | El test de CA-1 terminaría por plazo si hubiera interbloqueo; además, una aserción de que el lock está libre durante la espera | `cargo test` |
| CA-6 | B obtenido tras esperar es el mismo que obtiene un reintento posterior (NFR-01: no deshace de más) | Test de `crates/core` que compara el `snapshot_id` de B de los dos caminos sobre el mismo historial | `cargo test` |
| CA-7 | Sin `sleep` en el camino de producción | Test `crates/core/src/timemachine/base_wait_tests.rs::ca7_*`: lee `base_wait.rs` y el tramo entre `// base-wait: begin` y `// base-wait: end` de `undo.rs` y `undo_agent.rs`, y falla con `sleep`, `park_timeout` o `try_recv`. Sustituye al clippy o grep de CI (2026-11-09): el workspace no tiene `disallowed-methods` y la carpeta ya tiene `sleep` ajenos a esta TS | `cargo test` |
