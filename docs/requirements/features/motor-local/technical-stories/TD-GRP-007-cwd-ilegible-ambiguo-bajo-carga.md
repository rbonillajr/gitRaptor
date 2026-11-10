---
id: TD-GRP-007
title: "Bajo carga, un cwd ilegible en la muestra S3 deja la atribución en ambigua aunque el git del agente esté vivo"
type: td
status: spec-ready
feature: motor-local
domain: GRP
priority: medium
complexity: medium
created: 2026-11-09
updated: 2026-11-09
source: inline
related:
  adrs: [ADR-GRP-012]
  stories: [TD-GRP-005, TD-GRP-006, US-TMC-011, US-TMC-003]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, deteccion, s3, carga, tests-inestables]
---

## TD-GRP-007: Bajo carga, un cwd ilegible en la muestra S3 deja la atribución en ambigua aunque el git del agente esté vivo

**Valor**: la atribución de los commits de un agente no depende de la suerte con la máquina cargada, sin abrir el riesgo residual de S3.

### Descripción

**Como** desarrollador que deshace el trabajo de un agente con `undo --agent`
**Quiero** que el motor atribuya al agente sus commits aunque la máquina esté cargada
**Para** no ver commits "sin atribuir" ni tests inestables, sin que un `git` ajeno se atribuya al agente

**Origen: PR #346 (rama `fix/tmc-agent-harness-attribution-wait`), 2026-11-09.** Reproducido en macOS con 3 procesos `yes` de carga.

**Evidencia**: tras arreglar el arnés (el `git` del agente simulado se retiene por señal hasta que el motor registra el evento), queda 1 fallo de 3 corridas de `us_tmc_003`, `us_tmc_003_agents` y `us_tmc_011` (`a_new_commit_invalidates_the_redo`):

```
s3_evidence event=commit outcome=ambiguous samples=4 s3_cwd_unreadable=2 scope=worktree
  sessions_wt=3 foreign_daemon=1 gits_after_notice=2
```

El `git` del agente estaba vivo (el hook lo retiene), pero el detector no pudo leer el cwd de 2 procesos de la muestra y declaró la atribución ambigua; el evento quedó sin atribuir.

**Decisión del dueño (2026-11-09): no se toca el detector en el PR #346.** Que un cwd ilegible vuelva ambigua la atribución es lo conservador de ADR-GRP-012 (ante la duda, sin atribuir): un proceso ilegible podría ser un `git` del humano, y relajarlo arriesga atribuir al agente trabajo ajeno, que `undo --agent` desharía de más.

### Pregunta para el Arquitecto

¿Se puede distinguir "el proceso ya terminó" (la lectura falla por `ESRCH`; no es evidencia de nada) de "no tengo permiso para leer su cwd" (`EPERM`/`EACCES`; sí es duda) sin abrir el riesgo residual de S3? Y, si el cwd ilegible es de un proceso que terminó entre la lista y la lectura, ¿basta con descartarlo, o hay que confirmar con una segunda lectura?

### Decisión (2026-11-09)

**Decisión del orquestador (2026-11-09), validada por el Arquitecto con ajustes.** Detalle en la [Enmienda 2026-11-09 de ADR-GRP-012](../../../../architecture/decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md) (`git` inerte del propio daemon).

- **La evidencia desmiente la hipótesis del cwd ilegible.** En el CI (run 42714973247) y en la reproducción del PR #346, el evento sale `ambiguous` por **`foreign_daemon=1`**: un `git` del propio daemon (la captura de la Time Machine) vivo en la ventana. `s3_cwd_unreadable` es un contador global acumulado, no del evento. En `classify`, un `git` ajeno o de sesión con el cwd ilegible ya se ignora. El run 42694853372 se canceló sin logs del job de macOS.
- **No se adopta** la distinción `ESRCH` frente a `EPERM`/`EACCES`: no cambia los fallos observados, y para un `git` del daemon «ya terminó» no prueba nada.
- **Regla nueva**: un `git` del daemon con el cwd legible en el repo, el argv leído y un subcomando de la lista cerrada de inertes (solo lectura más `merge-base`, `update-index`, `pack-objects`, `index-pack`, `repack` y `prune`), con toda la cadena hasta el daemon también inerte, no cuenta como ajeno. Se cuenta en `daemon_inert`. Todo lo demás del daemon sigue siendo ajeno.
- **El otro modo bajo carga** (FSEvents no entrega, `no-sighting` con el aviso de lectura descartado por el tope de 3 s, visto 2 veces en 14 corridas en esta Mac con carga 18+) es de TD-GRP-006, no de esta ficha.

### Alcance Técnico

- ~~**Medir**: qué errores devuelve la lectura del cwd en `s3_cwd_unreadable` bajo carga (terminó vs. sin permiso), con un contador por causa (SEC-04: sin rutas).~~ Sustituido por la decisión de arriba: la causa medida es `foreign_daemon`.
- **Implementar** el booleano `inert` de la lectura del argv, la regla del `git` inerte del daemon con la cadena inerte y el contador `daemon_inert` en `s3_evidence`.
- **Reactivar en macOS** los dos tests de `us_tmc_003.rs` ignorados por el PR #347.
- **Fuera de alcance**: relajar la regla para procesos con permiso denegado, cualquier cambio a un `git` ajeno, el tope de 3 s del aviso de lectura y la latencia de FSEvents (TD-GRP-006).
- **Linux/Windows**: pendiente de la etapa de validación multiplataforma.

### Plan de Verificación

- **Unitario (`git_call`)**: inertes `update-index`, `pack-objects`, `index-pack`, `repack`, `prune` y `merge-base`. No inertes `pack-refs`, `gc`, `maintenance`, `update-ref`, `worktree list`, `config` de escritura, `--output`, un alias, `--shallow-file` antes del subcomando y un argv vacío.
- **Unitario (`classify`)**: un `git` inerte del daemon da `Ignored` y suma `daemon_inert`; uno no inerte o ilegible, `(Daemon, None)` con argv inerte y uno por encima del tope de lecturas son ajenos; la cadena daemon → `commit` → `pack-objects` es ajena y daemon → `repack` → `pack-objects` es inerte; un inerte del daemon más un `git` de sesión que escribe atribuyen; un inerte más un `update-ref` del daemon dan `Ambiguous`.
- **Fitness**: cada `WriteSubcommand` es inerte si y solo si no es `update-ref`.
- **SEC-04**: `s3_evidence` solo lleva enteros, ahora con `daemon_inert`.
- **Integración bajo carga**: `us_tmc_003`, `us_tmc_003_agents` y `us_tmc_011` pasan 10 de 10 con carga.
- **NFR-01**: ningún commit ajeno se atribuye al agente (corpus de `git` del humano concurrente).
