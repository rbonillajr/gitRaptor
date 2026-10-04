---
id: ADR-TMC-003
title: "ADR-TMC-003 — Oplog de la Time Machine: operaciones, snapshots, diario de intención y recuperación"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-013, ADR-TMC-001, ADR-TMC-002, ADR-TMC-004, ADR-TMC-005, ADR-TMC-007]
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
