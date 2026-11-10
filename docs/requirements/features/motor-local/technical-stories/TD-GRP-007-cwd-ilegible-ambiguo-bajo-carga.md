---
id: TD-GRP-007
title: "Bajo carga, un cwd ilegible en la muestra S3 deja la atribución en ambigua aunque el git del agente esté vivo"
type: td
status: implemented
feature: motor-local
domain: GRP
priority: medium
complexity: medium
created: 2026-11-09
updated: 2026-11-10
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

Implementado en: PR #352

### Decisión (2026-11-09)

**Decisión del orquestador (2026-11-09), validada por el Arquitecto con ajustes.** Detalle en la [Enmienda 2026-11-09 de ADR-GRP-012](../../../../architecture/decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md) (`git` inerte del propio daemon).

- **La evidencia desmiente la hipótesis del cwd ilegible.** En el CI (run 42714973247) y en la reproducción del PR #346, el evento sale `ambiguous` por **`foreign_daemon=1`**: un `git` del propio daemon (la captura de la Time Machine) vivo en la ventana. `s3_cwd_unreadable` es un contador global acumulado, no del evento. En `classify`, un `git` ajeno o de sesión con el cwd ilegible ya se ignora. El run 42694853372 se canceló sin logs del job de macOS.
- **No se adopta** la distinción `ESRCH` frente a `EPERM`/`EACCES`: no cambia los fallos observados, y para un `git` del daemon «ya terminó» no prueba nada.
- **Regla nueva** (con las condiciones de `security-review`, aceptadas por el Arquitecto). Un `git` del daemon deja de ser ajeno solo si cumple todo esto: es hijo directo del daemon, su `exe` es el Git que resolvió el motor, su cwd es legible y está en el repo, y su argv encaja exactamente en una plantilla de las capas de lectura o de escritura de la Time Machine que no mueve refs. Se cuenta en `daemon_inert`.
- **Ventana de escritura del daemon**: mientras el aplicador de la Time Machine o el ejecutor de operaciones de usuario escriben en el repo (guarda RAII), todo evento cuya ventana S3 se solape es ajeno. Se cuenta en `daemon_write_window`. Cierra el caso del restore que terminó su `update-ref` y deja vivos `git` inertes.
- **El otro modo bajo carga** (FSEvents no entrega, `no-sighting` con el aviso de lectura descartado por el tope de 3 s, visto 2 veces en 14 corridas en esta Mac con carga 18+) es de TD-GRP-006, no de esta ficha.

### Alcance Técnico

- ~~**Medir**: qué errores devuelve la lectura del cwd en `s3_cwd_unreadable` bajo carga (terminó vs. sin permiso), con un contador por causa (SEC-04: sin rutas).~~ Sustituido por la decisión de arriba: la causa medida es `foreign_daemon`.
- **Implementar** la plantilla exacta del argv del daemon (`gitraptor-git`), el booleano `inert`, la regla del `git` inerte, la ventana de escritura del daemon y los contadores `daemon_inert` y `daemon_write_window` en `s3_evidence`.
- **Reactivar en macOS** los dos tests de `us_tmc_003.rs` ignorados por el PR #347.
- **Fuera de alcance**: relajar la regla para procesos con permiso denegado, cualquier cambio a un `git` ajeno, el tope de 3 s del aviso de lectura y la latencia de FSEvents (TD-GRP-006).
- **Linux/Windows**: pendiente de la etapa de validación multiplataforma.

### Plan de Verificación

- **Unitario (plantilla, `gitraptor-git`)**: encaja el argv real de cada subcomando inerte. No encajan `update-ref`, `worktree list`, `config`, `--exec-path`, un `-c` de más, una opción que falta o cambia de orden, una ruta relativa, un argumento de más en la escritura, `--output` en la lectura ni un argv vacío. Fitness sobre `daemon_git_argvs()`.
- **Unitario (`classify`)**: un hijo directo del daemon con el Git del motor, una plantilla inerte y el cwd en el repo, junto a un `git` de sesión que escribe, atribuye. Son ajenos: otro `exe`, ningún Git resuelto, un argv sin plantilla, `(Daemon, None)`, un nieto, por encima del tope de lecturas y un inerte junto a un `update-ref` del daemon.
- **Unitario (ventana)**: una ventana abierta o cerrada que se solapa da `Ambiguous` y suma `daemon_write_window`; una de otro repo o anterior a la ventana S3 no cuenta; la guarda cierra la ventana al soltarse y ante un pánico.
- **Integración**: un undo por agente abre una ventana de escritura en su repo.
- **SEC-04**: `s3_evidence` solo lleva enteros, ahora también `daemon_inert` y `daemon_write_window`.
- ~~**Integración bajo carga**: `us_tmc_003`, `us_tmc_003_agents` y `us_tmc_011` pasan 10 de 10 con carga.~~ **Enmendado (D1 = A, decisión del coordinador, 2026-11-09):** el job de macOS del PR sale verde 3 veces con los dos tests de `us_tmc_003` activos, y el PR recoge las cifras de las corridas con carga en local de `us_tmc_003`, `us_tmc_011` y `td_tmc_003`. Un fallo `no-sighting` (FSEvents no entrega, `daemon_write_window=0`) cuenta contra TD-GRP-006, no contra esta ficha. Un fallo de atribución sí cuenta contra esta.
- **Caso del coordinador**: `td_tmc_003::another_actors_commit_after_an_own_echo_still_stops_the_undo` pasa, en ubuntu y bajo carga (GRP007-ECHO).
- **NFR-01**: ningún commit ajeno se atribuye al agente (corpus de `git` del humano concurrente).
