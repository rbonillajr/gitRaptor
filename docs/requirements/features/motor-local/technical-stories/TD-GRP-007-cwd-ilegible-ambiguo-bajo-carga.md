---
id: TD-GRP-007
title: "Bajo carga, un cwd ilegible en la muestra S3 deja la atribución en ambigua aunque el git del agente esté vivo"
type: td
status: draft
feature: motor-local
domain: GRP
priority: medium
complexity: medium
created: 2026-11-09
updated: 2026-11-09
source: inline
related:
  adrs: [ADR-GRP-012]
  stories: [TD-GRP-005, US-TMC-011, US-TMC-003]
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

### Alcance Técnico

- **Medir**: qué errores devuelve la lectura del cwd en `s3_cwd_unreadable` bajo carga (terminó vs. sin permiso), con un contador por causa (SEC-04: sin rutas).
- **Decidir** con el Arquitecto (enmienda de ADR-GRP-012 si cambia la regla) si el proceso terminado se descarta.
- **Fuera de alcance**: relajar la regla para procesos con permiso denegado.
- **Linux/Windows**: pendiente de la etapa de validación multiplataforma.

### Plan de Verificación

- **Unitario**: un proceso que termina entre la lista y la lectura no cuenta como ilegible; uno sin permiso sigue contando.
- **Integración bajo carga**: `us_tmc_003`, `us_tmc_003_agents` y `us_tmc_011` pasan 10 de 10 con carga.
- **NFR-01**: ningún commit ajeno se atribuye al agente (corpus de `git` del humano concurrente).
