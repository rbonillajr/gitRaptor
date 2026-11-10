---
id: TD-GRP-006
title: "Bajo carga, los eventos de FSEvents del directorio Git llegan tarde o no llegan y us_grp_010 falla también en main"
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
  adrs: [ADR-GRP-010, ADR-GRP-012, ADR-TMC-004]
  stories: [TD-GRP-005, US-GRP-010, TD-GRP-010]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, observador, fsevents, carga, tests-inestables]
---

## TD-GRP-006: Bajo carga, los eventos de FSEvents del directorio Git llegan tarde o no llegan y us_grp_010 falla también en main

**Implementado en: PR #359** (2026-11-10, SIN CERTIFICAR por el modo rápido; 10 de 10 de `us_grp_010` con carga 21–26). Plan: [`td-grp-006-fsevents-under-load.2-plan.md`](../../../../dev-briefs/td-grp-006-fsevents-under-load.2-plan.md).

**Valor**: la Time Machine registra a tiempo los commits aunque la máquina esté cargada, y `us_grp_010` deja de fallar por azar en el CI y en local.

### Descripción

**Como** desarrollador que trabaja con agentes en una máquina cargada
**Quiero** que el motor vea a tiempo las escrituras en el directorio Git aunque el observador del SO se retrase o pierda eventos
**Para** que la Time Machine muestre la atribución actual sin depender de la suerte

**Origen: PR de TD-GRP-005 (rama `fix/detect-sample-window`), 2026-11-09.** Medido en la máquina de dogfooding (macOS, carga 34–40).

**Medición**:

- `crates/core/tests/us_grp_010.rs::the_time_machine_sees_the_current_attribution` falla 2 de 10, también con el código de `main`.
- En la corrida que falla no llega ningún evento FS del directorio `.git` (ningún `git_dir_touched` del router). Solo un sondeo (`poll_repo` de la Time Machine o el sondeo de respaldo) ve moverse el índice, y el commit no aparece antes del plazo del test.
- En otras corridas los eventos del directorio Git llegaron entre 530 ms y 1,4 s después de la escritura.

**Diagnóstico** (Arquitecto, 2026-11-09): no es una carrera de S3, es latencia o pérdida de FSEvents bajo carga (observador, ADR-GRP-010). TD-GRP-005 ya cubre lo que dependía de S3: el aviso de escritura de la lectura del repo con tope de 3 s y la barrera de muestreo.

### Diagnóstico con evidencia (2026-11-10)

**CI** (unas 1000 corridas desde el 2026-10-28, los 3 sistemas operativos):
- `us_grp_010`, `observe_classes` y `observe_tiers`: 0 fallos.
- Los fallos de atribución recurrentes de `us_tmc_003` y `us_tmc_011` salen `ambiguous` con `foreign_daemon=2`. Es la regla S3 (TD-GRP-007, TD-GRP-008), no FSEvents.
- En ubuntu los avisos de lectura llegan con una antigüedad de 74–79 ms: el evento llega a tiempo.

**Local** (Mac de 10 núcleos, con otros workers compilando):
- `the_time_machine_sees_the_current_attribution` x10 con carga 9–19: **3 en verde y 7 en rojo**.
- En los fallos no llega ningún evento FS del directorio Git. La única lectura que ve el commit es el `poll_repo` de la Time Machine, entre 2,5 s y 10 s después de la escritura.
- S3 da `no-sighting`: cuando llega la lectura, el `git` del agente (vivo 1 s por el hook) ya terminó.

**Sonda de FSEvents crudo**: un `Stream` de `gitraptor-macsys` con latencia 0 y sin código del motor. Hace 30 escrituras y espera 45 s.

| Carga | Llegan | p50 | Nunca llegan |
|---|---|---|---|
| ~14 | 25 de 30 | 4,8 s | 5 |
| ~47 | 7 de 30 | 10,1 s | 23 |
| ~70 | 5 de 30 | 4,3 s | 25 |

- **Ningún evento trae flag de pérdida** (`MustScanSubDirs`, `UserDropped`, `KernelDropped`): la pérdida es silenciosa.
- El cuello está en `fseventsd`, no en el router ni en el adaptador.

**Separación de los casos**:
- **Llega tarde** (de 0,5 a 10 s): latencia de `fseventsd` bajo carga.
- **No llega**: pérdida silenciosa de `fseventsd`.
- **Cota de pared ajustada en el test**: `us_grp_010` necesita que el evento llegue mientras el `git` está vivo (1 s). Bajo carga no ocurre.

**Red de seguridad del producto**:
- **Sondeo de respaldo cada 30 s**: lee refs, reflogs y la huella de `HEAD`, del índice y de los marcadores. Los eventos Git se reconstruyen desde el reflog: ninguno se pierde, solo se retrasa.
- **Reconciliación completa** de cada worktree cada 5 min (15 min en ahorro).
- **Lectura inmediata por `Control::Wake` → `poll_repo`**. La piden el asentamiento de la Time Machine, `scope.snapshot`/`scope.subscribe` de un repo, el guard y el MCP.
- **Validez de la captura**: sale de leer el estado (`GitState::read`, `ValidityGuard`), nunca de los eventos.
- **Punto previo de las operaciones** (hook, undo, operaciones de raptor): es síncrono y no depende de FSEvents.

### Decisión

**Decisión del orquestador (2026-11-10), validada por el Arquitecto.**

1. **NFR-01 no está en riesgo.** Un evento perdido nunca pierde datos: la captura se valida leyendo el estado, los eventos Git se reconstruyen desde el reflog en el sondeo y las operaciones de raptor no dependen de FSEvents. Hay dos límites conocidos que no cambian aquí:
   - **RPO de las ediciones**: si el SO pierde todos los eventos del worktree, las ediciones sin cambio en Git pasan de Q=1 s/M=5 s a esperar la reconciliación de 5 min. Se anota en ADR-TMC-004.
   - **Atribución S3 bajo carga**: si falla, falla hacia el lado seguro. El commit sale `Unattributed`, nunca atribuido a quien no es.
2. **Medición**: `ObserverHooks::git_dir_written` recibe de dónde vino la lectura (`ReadVia { Event, Wake, Backup }`), y el log `s3_read_notice` gana `via=event|wake|backup`.
   - `via=event` con `age_ms` da la latencia del evento del directorio Git. Es una cota superior, porque incluye la ventana del router.
   - `via=wake|backup` cuenta los eventos que faltaron o que llegaron después de la lectura.
   - El p50/p99 se saca del log.
   - SEC-04: un enum, enteros y el id del repo, ninguna ruta.
   - Los eventos del worktree quedan sin medir: es un hueco conocido.
3. **El sondeo de respaldo de 30 s no cambia.** Un sondeo más rápido no recupera S3, porque en un commit real el `git` termina en milisegundos. La línea de tiempo ya se lee al momento por Wake, y cada sondeo extra son despertares de CPU que cuestan energía. Si en el dogfooding sale alta la proporción de `via=backup`, la decisión se reabre.
4. **`us_grp_010` por señal** (patrón de #353 y #356):
   - El hook `post-commit` escribe un marcador y espera un archivo "go", con una guarda de cuelgue.
   - El test, al ver el marcador, fuerza la lectura del repo con `scope.snapshot` del repo (Wake → `poll_repo`).
   - Cuando el evento aparece en la línea de tiempo, el test suelta el hook.
   - Si aun así sale sin atribuir (lectura a más de 3 s en una máquina cargada), el commit se repite **contando intentos (máximo 3)**, sin restar nunca al reloj.
   - El mensaje de fallo imprime `age_ms` y `via` de cada intento.
   - El test prueba que la Time Machine sigue la atribución actual de un commit que S3 atribuyó. No prueba que el motor lea a tiempo bajo carga.
5. **Tests de caja blanca del observador**: `suites/watch.rs`, `suites/observe_tiers.rs` y `observe_classes.rs` esperan un evento FS con cotas de 10–15 s. Tienen 0 fallos en CI y pasan a **TD-GRP-010**.
6. **Enmiendas** (2026-11-10):
   - ADR-GRP-010: la pérdida silenciosa de FSEvents; la cota de latencia la ponen Wake y el sondeo.
   - ADR-TMC-004: el límite del RPO de las ediciones.
   - ADR-GRP-012 no cambia.

### Alcance Técnico

- **Medir** la latencia y las pérdidas de los eventos del directorio Git bajo carga, con un contador o una traza entera en el router (SEC-04: sin rutas).
- **Decidir** con la medición si cambia el sondeo de respaldo, la espera del test o ambos.
- **Estabilizar** `us_grp_010` por señal, sin `sleep` fijos.
- **Enmendar** ADR-GRP-010 y ADR-TMC-004 con lo medido.
- **Fuera de alcance**: la regla S3 (ADR-GRP-012), lo ya cerrado en TD-GRP-005 y los tests de caja blanca del observador (TD-GRP-010).
- **Linux/Windows**: pendiente de la etapa de validación multiplataforma.

### Plan de Verificación

- **Integración bajo carga**: `us_grp_010` pasa 10 de 10 seguidas con la suite de otro crate corriendo en paralelo (con el alcance de la decisión 4).
- **Traza**: `s3_read_notice` lleva `via=event|wake|backup`. Un test comprueba el `via` de una lectura por ventana, por `PollNow` y por el temporizador, sin rutas (SEC-04).
- **No regresión**: TD-GRP-005 y US-GRP-010 siguen en verde.
