---
id: ADR-TMC-003
title: "ADR-TMC-003 — Oplog de la Time Machine: operaciones, snapshots, diario de intención y recuperación"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-09
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-011, ADR-GRP-013, ADR-TMC-001, ADR-TMC-002, ADR-TMC-004, ADR-TMC-005, ADR-TMC-007, ADR-CKP-002, ADR-CKP-003]
  stories: [US-TMC-002, US-TMC-003, US-TMC-006, US-TMC-007, US-TMC-008, US-TMC-009, US-TMC-010, US-TMC-011, US-TMC-016, US-TMC-019]
description: "Un oplog SQLite propio por repo en el perfil, solo por anexión: snapshots, operaciones con solicitante inmutable y un diario de estados que hace recuperable cualquier interrupción"
tags: [adr, time-machine, oplog, journal, recuperacion, caos, inmutabilidad, nfr-12, d-tmc-18]
published: true
---

# ADR-TMC-003 — Oplog de la Time Machine: operaciones, snapshots, diario de intención y recuperación

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-9 → (a) pila de undo y redo por worktree.

## Contexto

El timeline muestra qué cambió, cuándo, quién y con qué cobertura (US-TMC-006..008). Undo, redo y restauración son operaciones que se registran y se pueden deshacer (BR-TMC-WF-001). El registro de un undo (solicitante y sobre qué actuó) **no se reescribe** aunque cambie la atribución (D-TMC-18), mientras que el timeline muestra la **atribución vigente** (Q37), que el motor resuelve desde sesiones y registros append-only (ADR-GRP-013). Un `kill -9` a mitad de un snapshot o de un undo debe dejar el repo recuperable, el snapshot incompleto no cuenta y al volver se informa (NFR-12, BR-TMC-EDGE-003, US-TMC-019). Hay que interactuar con los locks de Git y con operaciones en curso (BR-TMC-EDGE-004).

**Pregunta**: ¿qué se registra, dónde, con qué garantías de inmutabilidad y cómo se recupera el sistema tras una interrupción?

## Decisión

### 1. Ubicación

**Un oplog SQLite por repo, propio de la Time Machine, junto a su almacén** (`<datos>/tm/<id-repo>/oplog.db`): WAL y sincronización completa, versión de esquema con migraciones en el binario y SQL parametrizado, como en ADR-GRP-006 § 4. Lo escribe solo el daemon. **No comparte archivo con el almacén del motor**: la corrupción de uno no arrastra al otro y, si se pierde el almacén del motor (Q26), los snapshots siguen restaurables. Los datos del motor (eventos, sesiones, atribución) se leen a través del propio daemon y nunca se copian.

### 2. Entidades

| Entidad | Qué guarda | Mutabilidad |
|---|---|---|
| **Snapshot** | Id, worktrees incluidos, **nivel** (`previo_garantizado`, `observacion`, `previo_hook`), ref del almacén, marca del motor que refleja (secuencia de ADR-GRP-013 hasta la que llega el estado leído), operación o evento que lo motivó, exclusiones (ignorados no, pero sí tamaño y submódulos), tamaño único | Fila inmutable; su estado (pendiente, completo, descartado, purga anunciada, purgado) va en el diario |
| **Operación** | Id, tipo (`protegida` con su subtipo, `undo`, `redo`, `restauracion`), ámbito (worktrees y refs), **solicitante congelado** (variante agente con nombre, origen y sesión, o "sin atribuir"; nunca "humano"), canal (CLI, TUI, MCP, hook), confirmación interactiva (sí/no), destino (operaciones o eventos deshechos, snapshot destino o undo que se rehace), snapshot previo, avisos (ya empujado, exclusiones, rutas en solape) | Fila inmutable: se escribe una vez |
| **Diario** | Transiciones de estado de cada operación y de cada snapshot, pasos del aplicador (ADR-TMC-002 § 3), locks de Git tomados (ruta, identidad del archivo —inodo y fecha de creación—, momento) y procesos hijo en curso | Solo anexión |
| **Aviso pendiente** | Interrupción o purga anunciada que hay que mostrar a un cliente, con su estado de entrega | Solo anexión |

La inmutabilidad se impone en el esquema: los triggers rechazan `UPDATE` y `DELETE` sobre operaciones, snapshots y diario. La purga (ADR-TMC-007) no borra filas: anota en el diario que el contenido se liberó. Como el mismo usuario puede editar el archivo, cada fila encadena el hash de la anterior y la cabeza se guarda también fuera del oplog; una cadena rota se declara hueco con su causa (SEC-TMC-09).

### 3. Estados de una operación

`intención → snapshot_previo → lista → aplicando(paso n) → terminada`, con salidas `rechazada` (permisos, solape o precondiciones; repo sin cambios), `abortada` (falló el snapshot previo; repo sin cambios) e `interrumpida` (proceso muerto o fallo detectado mientras aplicaba).

- **Orden de escritura**: (1) la intención se confirma en el oplog **antes** de tocar nada; (2) el snapshot previo se completa en el almacén y después en el oplog; (3) cada paso del aplicador se anota antes de ejecutarse; (4) `terminada` se anota al final. Un estado nunca se da por hecho sin la anotación previa.
- **Validez de un snapshot**: cuenta solo si su ref existe en el almacén **y** su fila tiene la transición `completo`. Si falta cualquiera de las dos, no figura en el timeline ni se ofrece para restaurar (US-TMC-019, escenario 1).
- **Timeline en vivo** (Enmienda 2026-10-04, Cockpit): las transiciones anotadas se publican en el stream; ver la sección final.

### 4. Undo, redo y "última operación"

- **Operación del timeline** = evento de Git del motor que cambia el estado del repo (commit, checkout, reset, merge, rebase, borrar rama o worktree, stash) u operación de la Time Machine. Las ediciones de archivos no son operaciones: forman parte del estado entre operaciones (glosario del context).
- **Estado previo a una operación**: si es de GitRaptor, su snapshot previo garantizado; si es de Git crudo, la última captura válida cuya marca del motor es anterior al evento (o el snapshot previo vía hook, si lo hay).
- **Operaciones de usuario** (Cockpit, MCP): el oplog registra su intención, su snapshot previo y su resultado; las ejecuta el ejecutor del daemon (ADR-TMC-002 § 5).
- **`undo`** deshace la operación más reciente del worktree que aún no está deshecha; **`redo`** rehace el último undo si es lo último del ámbito (BR-TMC-WF-001). Undos seguidos retroceden (TQ-9 → a) una operación más cada vez (pila por worktree), y una operación nueva en el ámbito invalida el redo.
- El undo deja el ámbito como estaba antes de la operación, incluidas las ediciones posteriores del mismo actor; esas ediciones quedan en el snapshot previo del undo y se recuperan con redo. Las de otro actor activan el solape (ADR-TMC-005).

### 5. Atribución: congelada frente a vigente

- El **solicitante** de una operación se congela al registrarla (D-TMC-18).
- El **actor** de los eventos y de los cambios entre capturas se resuelve **en cada consulta** con la atribución vigente del motor (ADR-GRP-013 § 2). Cuando el motor publica que la atribución de una sesión cambió, la Time Machine invalida sus cachés; no reescribe nada (Q37).
- La dependencia de P17 (retirar una corrección) afecta solo a la resolución vigente, no a este modelo; por eso el undo por agente sigue bloqueado (D-TMC-22).

### 6. Recuperación al arrancar el daemon

Antes de aceptar operaciones de la Time Machine en un repo, el daemon:

1. **Snapshots `pendiente`**: pasan a `descartado`. Las refs del almacén sin fila `completo` se borran (la operación que debían proteger nunca empezó). Los objetos sueltos los libera el mantenimiento del almacén (ADR-TMC-007).
2. **Operaciones en `intención` o `snapshot_previo`**: pasan a `abortada`. El repo no se tocó.
3. **Operaciones en `aplicando`**: pasan a `interrumpida`. **No se reanuda ni se revierte nada por cuenta propia**: la Time Machine solo escribe cuando un actor lo pide (BR-TMC-CONS-004). Su snapshot previo queda protegido de la purga y `raptor undo` devuelve el ámbito a él (US-TMC-019).
4. **Locks propios** (excepción explícita a "solo escribe cuando un actor lo pide", BR-TMC-CONS-004): un `index.lock` anotado en el diario se borra solo si existe con la misma identidad de archivo (inodo y fecha de creación) y ningún proceso hijo anotado sigue vivo; si sigue vivo, se espera con tiempo máximo. Es la única escritura en el repo que la recuperación hace por su cuenta: **libera un lock propio y no toca contenido**; sin ella, Git quedaría bloqueado para el usuario y sus agentes. **Un lock que no está en el diario nunca se borra.** Nota para el PO en el overview.
5. **Aviso**: cada interrupción genera un aviso pendiente que reciben el siguiente cliente que se conecta desde ese worktree y el timeline. Sin interrupciones no hay aviso (US-TMC-019, escenario 5).
6. **Purga a medias**: ver ADR-TMC-007 § 4.

## Alternativas consideradas

| Alternativa | En contra | Veredicto |
|---|---|---|
| **Tablas de la Time Machine dentro del almacén SQLite del motor** | Joins directos con eventos, pero acopla migraciones y dueños; la corrupción o pérdida del almacén del motor (Q26) se llevaría los snapshots | Descartada |
| **Oplog como commits del almacén Git** (estilo Jujutsu) | Atomicidad con los objetos, pero consultar el timeline por worktree, actor y periodo exige un índice aparte que acabaría siendo SQLite | Descartada en el MVP |
| **Atribución copiada en el oplog** | Contradice Q37: una corrección obligaría a reescribir filas | Descartada |
| **Reanudar o revertir solo al arrancar** | Escrituras que nadie pidió, sobre un repo en el que los agentes pueden haber seguido trabajando | Descartada |

## Consecuencias

- ✅ D-TMC-18 y Q37 se cumplen a la vez: el solicitante está congelado y el actor se resuelve en cada consulta.
- ✅ Toda interrupción tiene un estado nombrado y un camino de recuperación: el undo al snapshot previo.
- ✅ Los snapshots sobreviven a la pérdida del almacén del motor; lo que se pierde es la atribución, no el contenido.
- ⚠️ El timeline cruza dos almacenes. **Mitigación**: el daemon mantiene en memoria la atribución efectiva por sesión (ADR-GRP-013); se mide en INF-GRP-002.
- ⚠️ Las filas no se borran: el oplog crece con el uso (solo metadatos). Se mide en SPIKE-TMC-001.
- ⚠️ Tras una interrupción, el ámbito puede quedar a medio restaurar hasta que alguien pida el undo. Es intencionado y se avisa.

## Validación

1. **Caos** (INF-TMC-001): muerte forzada del daemon en cada transición y en cada paso del aplicador; al relanzar, el estado es el esperado, el repo coincide con el snapshot previo tras `raptor undo` y el aviso aparece una sola vez.
2. **Snapshot incompleto**: muerte entre crear la ref y confirmar la fila, y antes de crear la ref: en ningún caso figura como punto.
3. **Inmutabilidad**: un intento de `UPDATE` o `DELETE` sobre operaciones, snapshots o diario falla; una corrección de atribución posterior no cambia el solicitante registrado (US-TMC-008).
4. **Locks**: un `index.lock` ajeno presente al arrancar sigue ahí; el propio, anotado, se libera.
5. **Sin interrupciones**: un cierre ordenado no genera aviso y el historial queda completo.

## Referencias

- **Reglas**: BR-TMC-WF-001..003, BR-TMC-CONS-001, BR-TMC-CONS-005, BR-TMC-EDGE-003, BR-TMC-EDGE-004; D-TMC-18, D-TMC-19, D-TMC-22. Q26, Q34, Q37.
- **ADRs**: ADR-GRP-006, ADR-GRP-013; ADR-TMC-001, ADR-TMC-002, ADR-TMC-004, ADR-TMC-005, ADR-TMC-007.
- **Enablers**: TS-TMC-002, INF-TMC-001. **NFR**: NFR-01, NFR-12.

## Enmienda (2026-10-04, TS-TMC-002)

Aplicada desde la [Dev Spec de TS-TMC-002](../../requirements/features/time-machine/dev-specs/TS-TMC-002-oplog-diario.md). Precisa el modelo y no cambia ninguna decisión; el `status` sigue en `accepted`. Decisión del orquestador (2026-10-04), validada por el Arquitecto.

| Cambio | Dónde |
|---|---|
| Una operación en `lista` también pasa a `abortada` al recuperar: el repo no se tocó | § 6.2 |
| `rechazada` puede salir también de `snapshot_previo`: el solape se detecta después del snapshot | § 3 |
| La fila del snapshot `pendiente` se escribe **antes** de crear su ref en el almacén. La recuperación borra solo las refs de snapshots `pendiente` o `descartado`; una ref que el oplog no conoce se informa y se conserva (NFR-01) | § 3, § 6.1 |
| Sin almacén de snapshots disponible, la recuperación no decide nada que dependa de refs | § 6.1, § 6.6 |
| La recuperación del oplog no depende del almacén del motor: un repo cuyo almacén del motor no abre recupera igual su oplog (Q26) | § 1, § 6 |
| Un lock anotado se borra solo si la entrada no está en un hueco de la cadena, la ruta está dentro del directorio Git común, el nombre termina en `.lock` y el archivo es regular, con la misma identidad (inodo y fecha de creación; ver la enmienda de fix/ci-repo-intact) y sin hijos vivos. Se borra sin seguir enlaces, con un tope de espera compartido por todo el arranque. También se liberan los de operaciones `interrumpida` en arranques posteriores | § 6.4 |
| Cadena: versión de codificación por fila, génesis ligado al id del repo y cabeza en `oplog.head`. Se tolera solo la cabeza un lote por detrás; un oplog en cuarentena empieza con un hueco | § 2 |
| Las operaciones guardan la marca del motor, para intercalarse con el Git crudo sin depender del reloj. Hay una pila de undo y redo por worktree y otra de refs del repo; una operación `interrumpida` de cualquier tipo cuenta como hecha | § 4 |

## Enmienda (2026-10-04, fix/ci-repo-intact)

Decisión del orquestador (2026-10-04), validada por el Arquitecto. Corrige un fallo de § 6.4; el `status` sigue en `accepted`.

| Cambio | Dónde |
|---|---|
| La identidad de un lock anotado es **inodo + fecha de creación** (ns desde la época), no solo el inodo. ext4 y otros sistemas de archivos reutilizan al instante el inodo liberado, así que un lock ajeno creado en la misma ruta podía heredar el inodo del nuestro y la recuperación lo habría borrado. APFS no reutiliza inodos, por eso solo fallaba en Linux | § 2, § 6.4 |
| La fecha de creación se guarda en la columna `birth_ns` del diario (migración 2), que entra en el hash desde el formato de codificación 2. Las filas en formato 1 verifican con su consulta original | § 2 |
| Si la fecha de creación no está disponible (al anotar o al comprobar), el lock **no se borra** y se informa como `Unsupported`: ante la duda, fail-safe | § 6.4 |

**Riesgo residual:** en Linux la fecha de creación usa un reloj grueso (de 1 a 10 ms). Si alguien borra nuestro lock y otro Git crea uno en la misma ruta dentro del mismo tick y recibe el mismo inodo, ambos son indistinguibles. Pendiente: avisar al usuario con un notice cuando un lock propio se conserva por identidad desconocida.

## Enmienda (2026-10-04, Cockpit)

Aplicada desde DEP-CKP-5 de [CTX-CKP-001](../../requirements/features/cockpit/context.md), con [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) § 2 y [ADR-CKP-003](./ADR-CKP-003-arquitectura-tui.md) § 2 y § 4 (accepted 2026-10-04). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia el oplog, sus entidades, su inmutabilidad ni la recuperación. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| **Timeline en vivo**: cada transición del diario que cambia lo que muestra el timeline se publica como evento en el stream del repo, después de anotarse | § 3 | DEP-CKP-5; Q-CKP-19 |

- **Qué se publica**: el registro de una operación y su paso a `terminada`, `rechazada`, `abortada` o `interrumpida`; el paso de un snapshot a `completo`, `descartado`, purga anunciada y purgado; y cada aviso pendiente nuevo (§ 6). Los pasos intermedios del aplicador no se publican uno a uno.
- **Orden**: primero se anota en el oplog y después se publica, como el motor persiste antes de publicar (ADR-GRP-011 § 2). Un cliente nunca ve una transición que no esté en el oplog.
- **Para todos los clientes** del repo: dos TUIs ven el mismo timeline sin pedirlo otra vez (BR-CKP-CONS-004). El inicio y el fin de cada operación del catálogo llegan por la misma vía (ADR-CKP-002 § 2).
- **Atribución**: el evento lleva el solicitante congelado; el actor vigente lo sigue resolviendo cada consulta (§ 5). Un cambio de atribución ya se publica por ADR-GRP-013 § 6.
- **Fuera de NFR-04**: la Time Machine no tiene presupuesto de frescura propio en el MVP; la latencia se reporta sin gate.
- **Forma del evento** (tipo, campos y secuencia dentro del stream del repo): **pendiente, dueño: worker del canal (TS-GRP-004)**.

**Validación añadida**: con una TUI sin pantalla suscrita, un undo produce el evento de `terminada` después de su fila en el oplog; una muerte forzada antes de anotar no publica nada.

## Enmienda (2026-10-06, US-TMC-004)

Aplicada desde la [Dev Spec de US-TMC-004](../../requirements/features/time-machine/dev-specs/US-TMC-004-captura-continua-git-crudo.md). **Decisión del orquestador (2026-10-06), validada por el Arquitecto** (aprobada con ajustes, incorporados). No cambia el esquema del oplog ni su cadena. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| **Marca común**: la marca del motor de operaciones, undos y snapshots es la última secuencia de evento persistida del repo (ADR-GRP-013). Antes las operaciones guardaban la secuencia del bus, que no se compara con la de los eventos | § 2, § 4 | D2; bloqueante B2 |
| **Generación**: el almacén del motor guarda un id de generación (`store_meta`). La Time Machine anota en `tm/<repo>/engine-generation` desde qué fila del oplog vale cada generación. Las filas anteriores (otra generación, o la secuencia del bus) se ordenan antes de todo evento y nunca son destino de un evento | § 1 (Q26), § 4 | B2 |
| **Marca en calma**: la marca de una intención (operación protegida, undo) y la de una captura se toman cuando el motor ya persistió todo `git` terminado en los worktrees del ámbito (calma por estado, ADR-TMC-004 § 2). Sin calma en 2 s, el undo se rechaza con `repo-busy` y la operación del catálogo con `git-busy`: nunca se planifica con una marca vieja | § 3, § 4 | B1, B3 |
| **Git crudo en la pila**: `commit`, `merge`, `rebase`, `branch-update`, `branch-switch` y `reset` (nuevo tipo del motor: `reset` que no mueve rama) del worktree. Destino: la última captura disponible y verificada del worktree, de la generación actual, con marca anterior al evento. Actor: la sesión a la que el motor lo atribuyó. Si el evento más reciente es de un tipo que todavía no se deshace (`push`, `reconciled`, crear o borrar ramas o worktrees), el undo responde `raw-git-not-covered`: nunca lo salta | § 4 | US-TMC-004 |
| **`caused_by` por ancla**: al terminar una operación protegida o un undo, el daemon toma un **ancla**: una captura `observation` de su ámbito con `cause_operation` = la operación y una marca en calma. Los eventos del ámbito con `marca de la operación < seq ≤ marca del ancla` son su eco. Sin ancla, hasta la siguiente captura del worktree | § 4 | D5; ajuste 1 |

**Validación añadida**: el eco de un undo no entra en la pila (un segundo undo responde `nothing-to-undo`); dos operaciones del catálogo y dos undos siguen retrocediendo una a una con la captura continua activa (tests de US-TMC-002 en verde).

## Enmienda (2026-10-08, US-MCP-008)

**Decisión del orquestador (2026-10-08), validada por Arquitecto y PO** (cuota y claves por security-expert, K1 y O1). Origen: [DS-US-MCP-008](../../requirements/features/mcp/dev-specs/US-MCP-008-dev-spec.md), D3 y D4.

- **§ 2, Snapshot**: un nuevo nivel `manual` con las columnas `label`, `requester` (JSON, como `operations.requester`), `requester_session`, `worktree_key` (clave canónica del daemon: raíz resuelta y `(dev, inode)`, comparada por igualdad exacta, nunca con LIKE ni `instr` sobre `worktrees`) y `channel`. Un CHECK obliga a que estén todas presentes si y solo si el nivel es `manual`.
- **Migración 3 del oplog**: reconstruye `snapshots` y recrea los triggers append-only (UPDATE y DELETE siguen fallando). Copia previa con la API de backup, `verify_chain` después; una rotura nueva restaura la copia y cierra la Time Machine de ese repo con error tipado; una migración que falla se deshace entera. Un binario anterior ve `SchemaTooNew` y no pierde datos.
- **Cadena**: `FORMAT = 3`, global. Toda fila nueva se hashea con el formato 3 (para `snapshot` añade las cinco columnas); las filas de los formatos 1 y 2 conservan su `SELECT` y verifican igual.
- **Cuota contada en el oplog**: las ventanas y los techos cuentan toda fila `manual`, también las `discarded`, y la purga no devuelve cupo de 24 h (ADR-MCP-001, Enmienda (2026-10-08, US-MCP-008)).

## Enmienda (2026-10-09, US-TMC-019)

**Decisión del orquestador (2026-10-09), validada por Arquitecto y PO**; plan aprobado por el coordinador. Origen: el [Brief de US-TMC-019](../../dev-briefs/us-tmc-019-interruption-robustness.md), D2 a D5. No cambia el esquema del oplog ni su cadena: no hay migración nueva. El `status` sigue en `accepted`.

| Cambio | Dónde |
|---|---|
| **Entrega del aviso**: el aviso pendiente de § 6.5 se entrega con el método `timemachine.notices`, solo en el canal completo (no se ofrece por MCP). Pasa por la puerta del `.git` de la Time Machine (`tm_scope_for`, #226). Solo lo recibe un cliente que pasa esa puerta para el worktree del aviso: nunca otro worktree, otro repo ni una carpeta que el repo no registra | § 6.5 |
| **Una sola vez, al menos una vez**: se lee y se marca entregado en la misma sección crítica del lock del oplog, así que dos clientes no reciben el mismo aviso. Si marcarlo falla, el aviso se devuelve igualmente (se registra solo su id y el error, sin rutas). La entrega es por aviso: un aviso de repo entero (ámbito sin worktrees) lo recibe el primer cliente de cualquier worktree | § 6.5 |
| **Superficies en este corte**: `raptor undo`, `raptor restore` y `raptor timeline` lo muestran por stderr, también con `--json`. El timeline sigue mostrando la operación `interrupted`. TUI, MCP y el stream en vivo quedan diferidos; el undo por MCP (US-MCP-012) y el Deshacer de la TUI (US-CKP-014) deben entregarlo | § 6.5 |
| **Fallo detectado e informado en la respuesta**: § 6.5 cubre las interrupciones que encuentra la recuperación al arrancar. Una interrupción que el aplicador detecta y comunica al solicitante en la propia respuesta (`undo.interrupted`, `restore.interrupted`) no genera aviso pendiente | § 3, § 6.5 |
| **Avisos de purga**: no se entregan por este método; su entrega inicia la gracia de 24 h y es de US-TMC-016 (ADR-TMC-007 § 4.2) | § 6.6 |
| **Puntos de caos de la captura**: las capturas que no son el previo garantizado (observación y hook) tienen sus puntos `capture:pending` y `capture:ref` (INF-TMC-001), y un corte en cualquiera de ellos no deja un punto restaurable | § 3, § 6.1 |

**Validación añadida**: `apps/cli/tests/tm_interruption.rs` mata el daemon con `SIGKILL` en cada punto y comprueba con la huella de INF-GRP-001 que el repo queda intacto o recuperable, que el aviso llega una vez y solo al worktree que pasa la puerta, y que un arranque limpio no da aviso.

## Enmienda (2026-10-21, fix/tm-prior-point-binding)

**Decisión del orquestador (2026-10-04), validada por Arquitecto** (ajustes 1 a 7 del 2026-10-21). Origen: el [Brief de tm-prior-point-binding](../../dev-briefs/tm-prior-point-binding.md), D1 a D6. No cambia el esquema del oplog ni su cadena: no hay migración nueva y los hashes no cambian. El `status` sigue en `accepted`.

| Cambio | Dónde |
|---|---|
| **Git crudo en la pila, destino** (sustituye el destino de la enmienda de US-TMC-004): una sola regla (`prior_point::before_event`) elige el punto "antes de un evento" para el timeline y para el undo. Recibe **todos** los puntos con marca del worktree, también los no disponibles o alterados. Entre los elegibles gana el mayor `(marca, seq)`; la disponibilidad (`disponible` y no alterado, y luego `store.verify` en el undo) se filtra después, sobre el resultado. Si no queda ninguno, gana un punto más antiguo o `target-unavailable` | § 4 |
| **Elegible**: marca anterior al evento, de la generación actual (`seq >= suelo`), sin sombra (a) ni (b). **Sombra**: para cada previo `P` (`hook-prior`, `guaranteed-prior`), `f_P` es el primer evento crudo del mismo worktree con `seq > marca de P` (∞ si no hay). Un punto `Q` posterior a `P` queda ensombrecido si (a) `marca de Q < f_P`: su foto puede llevar ya el efecto del comando con una marca atrasada; o (b) `Q` es una foto con `mid_operation` y no hay ningún punto limpio `R` entre `P` y `Q` (limpio: un previo, o un punto sin `mid_operation` con `marca de R >= f_P`). Un previo no disponible sigue ensombreciendo. Un previo de una generación anterior no ensombrece a la actual (sus marcas son de otra numeración) | § 4 |
| **Fallar cerrado = más antiguo**: ante la duda se elige un punto anterior. Restaurar de más se recupera (el undo toma antes su propio previo garantizado, NFR-01); una foto a mitad de operación o posterior al evento pierde el estado previo | § 4 |
| **`mid_operation` en el `detail` de la entrada `complete`**: toda captura registra si había una operación de Git en curso (rebase, merge, cherry-pick, etc.) en alguno de sus worktrees al empezar o en el punto de validez. Una observación o un punto manual con `true` solo deja de ser "antes de un evento" por la sombra (b): cuando sigue a un previo sin un punto limpio en medio. Sin previo, como en el `merge --abort`, sigue siendo elegible. Un previo nunca se excluye por ello | § 2, § 4 |

**Esquema completo del `detail` de la entrada `complete`** (JSON, dentro de la cadena del journal):

| Campo | Tipo | Significado |
|---|---|---|
| `unique_size_bytes` | entero | Bytes nuevos que la captura añadió al almacén |
| `exclusions` | lista de `{ path, reason }` | Rutas que quedaron fuera (tamaño, submódulos, huecos del historial) con su motivo |
| `mid_operation` | booleano, opcional | Operación de Git en curso al inicio o al final de la captura. Ausente en las filas anteriores a esta enmienda, y se lee como `false` (el comportamiento de antes): tratarla como sucia dejaría el undo de los eventos viejos en `target-unavailable` |

**Límites abiertos de NFR-01**:

- **Eco tardío de un restore en la pila** (us_tmc_009, `a_restore_can_be_undone_back_to_the_exact_prior_state`): `external_events_in` liga el eco de una operación solo por la marca de su ancla, así que un evento que el motor persiste después del ancla queda como Git crudo y es la última operación de la pila. Esta enmienda no cambia el eco: solo hace que deshacer ese evento lleve al estado exacto anterior al restore (el ancla queda ensombrecida por su previo garantizado). Se corrige en otra rama, con un volcado de CI como prueba ([PR #256](https://github.com/rbonillajr/gitRaptor/pull/256)). **Cerrado** por la enmienda de `fix/tm-restore-calm-mark` (más abajo): la causa era una marca en calma que no esperaba a los refs ni a un `git` todavía en curso, no la regla del eco.
- **Enlace por ventana, no por proceso**: los eventos del motor no guardan el `git` que los produjo. Un comando con gancho que no emite eventos deja sus observaciones posteriores ensombrecidas hasta el primer evento del worktree (gana el previo, más antiguo), y un `git` ajeno en el mismo worktree puede cerrar antes de tiempo la ventana de un previo. Se mejora cuando el motor guarde `(pid, start_us)` por evento.
- **Foto a mitad de un rebase**: un rebase sin gancho puede seguir eligiendo la foto a mitad como "antes" de su evento final. Un rebase con gancho abortado tras una resolución parcial vuelve a su previo, no a las resoluciones, que siguen como punto propio y se recuperan. El tipo de evento no los distingue (`rebase (abort)` y `rebase (finish)` son los dos `Rebase`). Se mejora cuando el motor distinga el fin del abandono de una operación.

**Validación añadida**: `prior_point_tests.rs` (C1 a C5), `timeline_prior_point_tests.rs` (timeline y undo eligen lo mismo), `tests/tm_prior_point_undo.rs` (el undo de Git crudo toma su previo garantizado antes de restaurar) y `tests/tm_prior_point_condition.rs` (`mid_operation` a mitad de un rebase, en un worktree limpio y en una fila sin el campo).

## Enmienda (2026-10-21, fix/tm-restore-calm-mark)

**Decisión del orquestador (2026-10-04), validada por Arquitecto**. Origen: el [Brief de fix/tm-restore-calm-mark](../../dev-briefs/tm-restore-calm-mark.md), D1 a D8 (NFR-01: el undo tras un restore deshacía el eco tardío de la operación protegida anterior). No cambia el esquema del oplog, su cadena, el wire ni los códigos `repo-busy` y `git-busy`. El `status` sigue en `accepted`.

| Cambio | Dónde |
|---|---|
| **La Marca en calma espera también a los refs**: además de lo que pide la calma por estado (ADR-TMC-004 § 2), las ramas locales del directorio común que registró el daemon deben ser las del último lote que el motor persistió con ramas. Se comparan bajo el mismo lock que guarda la `seq` de ese lote, así que la marca devuelta cubre el lote cuyas ramas coinciden | § 4 |
| **Por qué**: `git` escribe el reflog de `HEAD` antes de renombrar el ref, así que el motor puede persistir un lote con el reflog nuevo y las ramas viejas. Además, `branch -D` y el `update-ref` del aplicador no escriben el reflog de `HEAD`. En los dos casos la calma solo por reflog era inmediata y el ancla o la intención tomaban una marca que no cubría los eventos de la operación anterior | § 4 |
| **Falla cerrado**: si el repo no abre o el listado de ramas falla, no hay calma (y, pasado el límite de 2 s, `repo-busy` o `git-busy`). Solo un repo cuyo motor nunca publicó ramas usa el respaldo por tiempo. La vista de refs del motor y la calma leen las ramas con el mismo helper, así que un ref corrupto da el mismo resultado en las dos | § 4 |
| **Despertar antes de esperar**: el motor despierta un repo dormido antes de esperar su calma; la espera termina con el primer lote del repo despierto, dentro del límite | § 4 |
| **Límite declarado: la barrera es por repo**: los commits de otro worktree retrasan la calma de este. Medido (`tm_calm_refs_load`, release, macOS): con otro worktree haciendo un commit cada 500, 200, 100, 50, 20 y 10 ms, ningún undo dio `repo-busy`; X_min ≤ 10 ms (el menor valor medido). Por debajo de esa cadencia, el `settle` puede agotar los 2 s y dar `repo-busy`. Barrera por worktree: diferida hasta que se vea `repo-busy` con la cadencia real de los agentes | § 4 |
| **Coste medido**: p95 de `settle` en reposo de 0,57 ms con 1 rama (presupuesto: 5 ms en release). Con unas 5000 ramas sueltas (sin empaquetar), 1,53 s por sondeo: un repo así gasta casi todo el límite de 2 s en cada espera. Queda como límite declarado | § 4 |
| **Marca confirmada (caso 2)**: la marca de una captura vale solo si, en su punto de validez, el motor vuelve a estar en calma con la misma `seq` dentro de una ventana corta (`CONFIRM_WINDOW`, 250 ms). El primer sondeo en calma decide: la misma marca vale; otra descarta; sin calma en la ventana, descarta. Así un no-calma momentáneo ajeno al contenido no descarta la captura, y no hay reintentos ni filas que gasten cuota. La ventana solo absorbe un no-calma de causa desconocida (un `git` ilegible, un parpadeo de la regla de tiempo): un `git` conocido en el repo (carpeta en el ámbito o redirección resuelta dentro) visto en cualquier sondeo descarta, aunque termine dentro de la ventana, porque `git apply` cambia archivos sin dejar reflog, ref ni índice. Es correcto porque un `git` que cambió el contenido durante la lectura, al final de esta, o está vivo (si se lee, se conoce y descarta) o terminó con su reflog persistido (la marca se movió) o sin persistir (sin calma); solo uno ilegible se espera, con la exposición ya aceptada para la gracia. Si no, la captura se descarta: la continua y el ancla reintentan, la captura manual se descarta, el previo de hook reintenta mientras le quede plazo (la denegación sale por plazo, nunca por un único descarte, y el reintento no vuelve a contar la cuota) y el previo garantizado se repite hasta 3 veces con una marca de intención nueva y después se rechaza con `git-busy` sin ejecutar el paso. Cada repetición del previo garantizado registra su propia intención (la anterior queda `aborted`): repetir solo el previo con la misma operación dejaría su marca vieja y haría pasar por eco de la operación el `git` crudo que provocó el descarte. Así ninguna captura guarda contenido posterior a un evento con una marca anterior a ese evento. Compatible con la regla del punto previo de #272: elimina las Observation con marca vieja fuera de la ventana de comando de un prior sin cambiar `PointFacts` | § 4 |
| **Inanición aceptada**: la `seq` es de todo el repo, así que la actividad sostenida de un worktree hermano descarta las capturas continuas de este. Tras 50 reintentos se registra `tm_capture_failed kind=discarded-exhausted`: se prefiere perder cobertura a sellar una marca falsa | § 4 |
| **Git en curso (caso 3)**: la calma exige además que no viva ningún `git` en el ámbito: la lista de procesos del usuario, un `git` cuyo `cwd` está bajo la raíz del worktree o bajo su directorio Git. Quedan fuera los hijos directos del daemon y los `git` de larga vida que no cambian un worktree (`fsmonitor--daemon`, `cat-file --batch*`; `git-credential-cache--daemon` no se llama `git`). Tampoco cuentan los subcomandos de solo lectura (`log`, `show`, `diff`, `blame`, `grep`, `ls-files`, `ls-tree`, `rev-parse`, `rev-list`, `cat-file`, `shortlog`, `describe`, `for-each-ref`, `help`, `version` y `config` en sus formas de lectura); `status`, `commit` y `rebase -i` sí. Lo que es cada proceso se lee en cada pasada, porque `exec` conserva su identidad. La razón: un `git` cambia el contenido antes de escribir su reflog, así que la calma por reflog solo cubre `git` terminados. Una lista de procesos que no se puede leer falla cerrado. Orden de comprobación, de lo barato a lo caro: reflog, `HEAD`, `index.lock`, ramas, llamadas de hook en vuelo y, solo si todo dice calma, procesos (p95 de una pasada con unos 760 procesos: 1,8 a 4,1 ms en release, macOS) | § 4 |
| **Exclusiones del previo de hook**: el `git` que disparó el hook sigue vivo mientras se toma su previo. Su `settle` y la confirmación de su marca no cuentan a los ancestros `git` del cliente del hook ni a su propia llamada de hook en vuelo. Con la cadena desconocida no se excluye nada: falla cerrado, `TimeLimit` y denegación de US-GRD-017. Mientras dura una llamada de hook, ningún otro `settle` de ese worktree da calma | § 4 |
| **Redirección**: un `git` de fuera del ámbito cuenta solo si su `-C`, `--git-dir`, `--work-tree`, `GIT_DIR`, `GIT_WORK_TREE` o `GIT_COMMON_DIR`, resueltos como lo hace `git`, caen dentro del repo (directorio común, un gitdir de `.git/worktrees/*` o un worktree registrado). Del lector solo sale sí o no (SEC-04). Un `git` que ya terminó no cuenta. Uno vivo con los argumentos ilegibles cuenta solo si su carpeta se lee y está en este repo (otro worktree o el directorio común), durante la ventana de gracia de `settle` (2 s); después se registra `tm_calm_probe_unreadable` una vez y deciden el reflog, las ramas y las llamadas de hook. Con la carpeta ilegible, o fuera del repo, nunca bloquea (⚠️ RIESGO NFR-01 abierto, release-plan, Huecos, fila 12) | § 4 |
| **Reflog por líneas completas**: se publica y se compara el reflog de `HEAD` hasta su último `\n`. Una línea a medio escribir no nombra evento ni cambia la calma hasta completarse | § 4 |
| **Límites declarados**: un `git` largo de solo lectura en el worktree (paginador, editor de `commit`, `gc --auto`) aplaza capturas y operaciones mientras vive. Un `git status` en bucle (IDE, prompt) puede impedir la calma hasta el límite; cada `settle` que agota su límite por un `git` lo registra una vez como `tm_calm_blocked_by_git`, con `pid` y subcomando, nunca el resto de la línea de órdenes. Windows: el `cwd` de otro proceso se lee por el PEB, así que la regla está activa, **sin validar en máquina real** (release-plan, Huecos, fila 12) | § 4 |

**Validación añadida**: `crates/core/tests/tm_calm_refs.rs` (un ref movido sin persistir, un commit cuyo reflog se persistió antes que su rama, un directorio común ilegible, `remove`, `pack-refs` y un ref corrupto), `tm_calm_refs_stack.rs` (el eco tardío de la operación protegida queda bajo el restore en la pila) y `tm_calm_dormant.rs` (un undo en un repo dormido con una rama movida no da `repo-busy`). Para la marca confirmada: `timemachine/capture_mark_tests.rs` (un evento persistido o pendiente entre la marca y la lectura descarta la captura; manual y previo de hook; actividad del hermano acotada y sin fin; ancla fallida), `timemachine/hook_prior_mark_tests.rs` (denegación por plazo) y `executor/prior_mark_tests.rs` (previo garantizado con `git-busy`). Para el `git` en curso: `timemachine/calm_git_alive_tests.rs`, `timemachine/calm_exclusions_tests.rs`, `timemachine/calm_redirect_tests.rs`, `timemachine/hook_prior_live_git_tests.rs` y `crates/core/tests/tm_calm_reset_in_hook.rs` (un `reset --hard` detenido en su hook `reference-transaction` no deja captura y, al terminar, la calma vuelve con una marca posterior al Reset).
