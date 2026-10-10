---
id: TD-TMC-001
title: "Previo de hook: plazo real, cupo barato y cubo «sin atribuir» acotado, antes de que un fallo deniegue (US-GRD-017)"
type: td
status: draft
feature: time-machine
domain: GRP
priority: medium
complexity: medium
created: 2026-10-09
updated: 2026-10-09
related:
  adrs: [ADR-TMC-004, ADR-TMC-006, ADR-TMC-005]
  stories: [US-TMC-005, US-GRD-017]
  specs: []
ado:
  id: null
  url: null
tags: [time-machine, deuda-tecnica, previo-hook, plazo, cupo, sin-atribuir, nfr-01]
---

## TD-TMC-001: Previo de hook: plazo real, cupo barato y cubo «sin atribuir» acotado, antes de que un fallo deniegue (US-GRD-017)

**Valor**: cuando US-GRD-017 convierta un previo `failed` en denegación, el plazo del previo de hook es un tope real, un agente en bucle no deja sin conexiones a los demás y la persona no comparte cupo con agentes no detectados.

### Descripción

**Como** responsable de la Time Machine
**Quiero** que el previo de hook respete su plazo, compruebe el cupo antes de tomar locks y cuente la actividad «sin atribuir» con un techo propio
**Para** que el aviso de US-TMC-005 («avisar y dejar pasar») no se convierta en denegación injusta ni en agotamiento de conexiones al pasar a US-GRD-017

> Dev Spec: dev-specs/TD-TMC-001-previo-hook-plazo-y-cupo-sin-atribuir.md | Pendiente

**Origen.** Revisión de código (rust-code-reviewer) y revisión de seguridad (security-expert) del PR de US-TMC-005 (Brief `docs/dev-briefs/us-tmc-005-pre-hook-snapshot.md`, D5 y D6), sin hallazgos Critical ni High. Los Medium y Low quedan como deuda con dueño. **Deben cerrarse antes o dentro de US-GRD-017**: hoy un `failed` solo avisa; con GRD-017 deniega, y entonces los tres Medium dejan de ser incómodos y pasan a bloquear trabajo legítimo.

**Hoy.**

- **M-01 / F1 (Medium).** El plazo de 5 s no es un tope: la captura (`store/capture.rs`) toma el writer del almacén (`store/mod.rs`) y el lock del oplog sin plazo. Un previo garantizado largo del ejecutor (archivo nuevo de 1 GB, unos 7 s) retiene al previo de hook más allá del plazo de la llamada (`CALL_TIMEOUT`, 10 s). El hook deniega con `internal-error` en vez de avisar y dejar pasar (D11) y la captura escribe después una fila `pending` tardía que gasta cupo.
- **M-02 (Medium).** El cupo se comprueba al final, tras asentar, reservar y tomar el lock de grabación. Un agente en bucle llena las 32 conexiones del daemon y deja sin lock a los demás solicitantes.
- **M-03 (Medium).** El cubo «sin atribuir» lo comparten la persona y los agentes no detectados (R-GRD-2), no tiene techo por repo y se reinicia cuando cambia el inode del worktree (la clave es dispositivo, inode y raíz). En Windows la sonda de volumen no tiene suelo de disco (pendiente multiplataforma).
- **L-01.** Una fila `hook-prior` con el solicitante ilegible se lee como no manipulada (`oplog/query.rs`, rama `(None, None) => (None, true)`) y el comentario que lo justifica es falso.
- **L-02.** El cwd del par se lee por PID sin volver a comprobar `start_us`, de modo que la reutilización de un PID puede dar el cwd de otro proceso.
- **F2 / F3 / F4 / I-01 / I-02 (Low/Info).** La comprobación de cupo del previo de hook duplica la del comando manual; `verify()` corre dos veces; el test del margen compara con 10 s literal en vez de con `gitraptor_api::client::CALL_TIMEOUT`; el tipo de error cambia al reutilizar un fallo anotado; la raíz del repo se verifica más de una vez.

**Decisión pendiente del Arquitecto (M-03).** Techo propio «sin atribuir» por repo y clave por raíz canónica, a cerrar con Q-GRD-37 y el cupo de BR-TMC-CONS-003 (⚠️ **ASSUMPTION** de cifras, a revisar con uso real). Se recoge en el ADR-TMC-004 (cobertura) o en una enmienda del ADR-TMC-005 (solicitante), no en la Dev Spec.

### Alcance Técnico

- **Acotar** la espera de los locks del almacén y del oplog al plazo restante del previo de hook, de modo que el plazo sea un tope real también bajo un previo garantizado largo.
- **Comprobar** el plazo agotado antes de escribir la fila `pending`, para que un previo abandonado no gaste cupo ni deje una fila tardía.
- **Hacer** el precheck barato de cupo antes de asentar, reservar o tomar locks, igual que el comando manual, y admitir como máximo una espera en cola por solicitante y almacén.
- **Definir** un techo propio de la actividad «sin atribuir» por repo y claves de cupo por raíz canónica del worktree, de modo que cambiar el inode no reinicie el cubo (decisión del Arquitecto con Q-GRD-37).
- **Añadir** el suelo de disco del previo de hook en Windows, hoy ausente.
- **Leer** el solicitante crudo de una fila `hook-prior` y tratar su ausencia como manipulación.
- **Comprobar** de nuevo el inicio del proceso al leer el cwd del par, con un único ayudante que reutilicen todos los puntos de uso.
- **Unificar** la comprobación de cupo del previo de hook con la del comando manual, y evitar la doble verificación, la doble comprobación de la raíz y el margen de test fijo.
- **Conservar** el tipo de error original al reutilizar un fallo anotado.
- **Fuera de alcance**: convertir `failed` en denegación (US-GRD-017), nuevos hooks que pidan previo, cambiar las cifras de cupo por uso real, la cola de confirmación y la validación en Linux y Windows en máquinas reales (etapa posterior).

### Plan de Verificación

#### Pruebas Automatizadas

- Con el writer retenido más que el plazo, el previo de hook devuelve `failed` a tiempo (antes de la llamada del cliente), el hook avisa y deja pasar, y no queda ninguna fila `pending` tardía ni cupo gastado.
- Con 32 llamadas de un mismo solicitante en bucle, las de otros solicitantes siguen obteniendo lock; el exceso se rechaza como cupo agotado sin tomar locks.
- Un cambio de inode del worktree no reinicia el cubo «sin atribuir»; el techo por repo se aplica a la persona y a los agentes no detectados juntos, y los agentes detectados conservan sus cupos.
- Una fila `hook-prior` con el solicitante ilegible se marca como manipulada en el timeline.
- Un PID reutilizado (mismo PID, otro `start_us`) no produce el cwd del proceso nuevo.
- El margen del test sigue al plazo de llamada del cliente si este cambia.
- La huella del repo no cambia (suite de huella de INF-GRP-001) y el arnés de caos de INF-TMC-001 pasa con el previo de hook retenido.

#### Verificación Manual / Sandbox

- En un repo temporal, retener el writer con un previo garantizado grande (archivo nuevo de 1 GB) y lanzar un rebase con Git crudo y los hooks instalados: el hook avisa y deja pasar a los 5 s, sin denegar.
