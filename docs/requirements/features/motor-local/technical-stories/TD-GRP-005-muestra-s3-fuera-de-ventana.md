---
id: TD-GRP-005
title: "La muestra S3 de un lote de avisos se sella con el aviso más antiguo y cae fuera de la ventana del evento"
type: td
status: draft
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-08
related:
  adrs: [ADR-GRP-012]
  stories: [US-GRP-007, TS-GRP-008, US-TMC-011]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, deteccion, atribucion, s3, carga, tests-inestables]
---

## TD-GRP-005: La muestra S3 de un lote de avisos se sella con el aviso más antiguo y cae fuera de la ventana del evento

**Valor**: un commit que un agente hace mientras su `git` sigue vivo sale atribuido al agente también con la máquina cargada. Así, `raptor undo --agent` (US-TMC-011) y la demo de M2 no dependen de la suerte.

### Descripción

**Como** desarrollador que trabaja con agentes en una máquina cargada
**Quiero** que el motor atribuya al agente los commits que su `git` hizo mientras S3 lo podía ver
**Para** poder deshacer lo de ese agente (US-TMC-011) sin que sus commits salgan como "sin atribuir"

> Dev Spec: Pendiente

**Origen: PR #325 (US-TMC-011), 2026-11-08.** La demo de M2 (`apps/cli/tests/m2_demo_undo_agent.rs`) falló en el CI de macOS y en local, unas 3 de cada 5 veces con carga de 12 a 35, porque un commit del agente salió `Unattributed`.

**Medición** (`daemon.log` de la demo, capturado durante la corrida). El commit sale `s3_evidence … outcome=no-sighting`, con todos los contadores a 0 (`sessions_wt=0`, `foreign_*=0`, `gits_after_notice=0`). Ocurre aunque el test haga las tres cosas siguientes, todas por señal:

- espera a que `sessions.list` muestre la sesión detectada;
- separa `git add` del commit y espera a que el snapshot muestre el archivo staged;
- retiene el `git` del commit en un hook `reference-transaction` hasta que el evento está registrado.

**Sin muestra no hay prueba.** Con `no-sighting`, la sesión única del worktree es solo una pista (enmienda de ADR-GRP-012), y el evento queda `Unattributed`.

**Causa probable** (`crates/core/src/detect/mod.rs`):

1. `sample_loop` muestrea el primer aviso enseguida. Mientras esa muestra dura, que bajo carga es lenta (tabla de procesos, cwd, argv), los avisos se acumulan en la cola.
2. De los acumulados se queda **con el primero de cada repo** (`if !pending.iter().any(|n| n.repo_id == more.repo_id)`) y descarta los siguientes.
3. `sample` sella cada avistamiento con el `t_recv` de ese aviso.
4. `evidence()` solo cuenta los avistamientos con `t_recv` en `[t_recv del lote − s3_lead (100 ms), t_flush]`.
5. Resultado: el aviso de las escrituras del commit, que llega después del de los objetos o del índice, se descarta. La única muestra queda sellada con un `t_recv` anterior a la ventana del lote del evento. Por eso no cuenta, aunque viera el `git` vivo.
6. Además, el router (`watch/mod.rs`, `probed`) avisa solo en la primera escritura en el directorio de Git de cada lote del observador.

**Tests que la sufren** (las mismas esperas de atribución, inestables bajo carga):

- `apps/cli/tests/m2_demo_undo_agent.rs`
- `apps/cli/tests/raw_git_undo.rs`
- `apps/cli/tests/undo_overlap_process.rs`
- `apps/cli/tests/undo_overlap_same_session.rs`
- `apps/cli/tests/timeline_process.rs`
- `crates/core/tests/us_tmc_011.rs` (p. ej. `another_agents_commit_stays`, "the timeline never showed 1 events (agent: true)")

### Alcance Técnico

- **Arreglo propuesto**: que ningún aviso se quede sin muestra en su ventana. Dos opciones, a decidir con el Arquitecto:
  - **Opción (a)**: una sola lectura de la tabla de procesos por muestra, pero un avistamiento por **cada** `t_recv` distinto de los avisos en cola del mismo repo, cada uno con su propio `started_before`.
  - **Opción (b)**: sellar el avistamiento con el `t_recv` **más reciente** de la cola y revisar que `started_before` siga siendo correcto. El comentario actual dice que el más antiguo es "el más permisivo", y hay que comprobarlo.
- **Revisar `probed`** en `watch/mod.rs`: que la escritura de la ref de un commit dispare su propio aviso aunque el lote ya haya avisado por los objetos.
- **Sin cambiar la regla de ADR-GRP-012**: un `git` ajeno sigue dando `ambiguous`, y `no-sighting` sigue sin atribuir.
- **Fuera de alcance**: el ámbito worktree de S3 y la redirección (TD-GRP-004), el umbral de inactividad de las sesiones y cambiar los tests previos (salvo quitar esperas que dejen de hacer falta).

### Plan de Verificación

- **Test del detector por señal, sin `sleep` fijos**:
  - dos avisos del mismo repo en cola mientras una muestra está en curso;
  - el evento del segundo tiene avistamiento en su ventana;
  - hoy da `no-sighting`.
- **Integración**: la demo de M2 y `raw_git_undo` pasan 10 de 10 seguidas bajo carga (por ejemplo, con la suite de otros crates corriendo a la vez).
- **No regresión**: TS-GRP-008 (`measure_quick_commits_with_a_foreign_git_in_another_worktree`) no baja su proporción de commits atribuidos.
