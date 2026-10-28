---
id: ADR-TMC-002
title: "ADR-TMC-002 — Escritor de la Time Machine: componente del daemon, capa de escritura acotada y protocolo de aplicación"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-27
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-TMC-001, ADR-TMC-003, ADR-TMC-005, ADR-TMC-006, ADR-CKP-002]
  stories: [US-TMC-001, US-TMC-002, US-TMC-003, US-TMC-009, US-TMC-010, US-TMC-011, US-TMC-014, US-TMC-015, US-TMC-019]
description: "Las escrituras internas (almacén y aplicador de undo, redo y restauración) las hace solo el componente Time Machine del daemon, con una capa de escritura separada en crates/git, sin hooks ni filtros; las operaciones de usuario las ejecuta el daemon fuera de esa capa"
tags: [adr, time-machine, escritura, daemon, crates-git, restauracion, locks, sin-shell, nfr-02, nfr-07]
published: true
---

# ADR-TMC-002 — Escritor de la Time Machine: componente del daemon, capa de escritura acotada y protocolo de aplicación

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-2 → (a) sin crate nuevo; TQ-3 → (a) las operaciones de usuario las ejecuta el ejecutor del daemon; TQ-10 → (a) sin rollback automático; TQ-12 → notas a motor-local aprobadas (pendientes de integración); TQ-13 → (a) escrituras internas sin hooks, filtros, firma ni config de sistema/global.

## Contexto

El motor solo observa (Q21) y ADR-GRP-009 le prohíbe cualquier escritura, lock o programa del usuario. La Time Machine **sí** escribe, y solo cuatro cosas: snapshots en su almacén (ADR-TMC-001) y undo, redo y restauración en el repo cuando un actor lo pide (BR-TMC-CONS-004). Las escrituras deben ser recuperables (NFR-01), sin shell y con argv fijo (NFR-02), con el Git del sistema ≥ 2.38 y respetando la configuración y los hooks del usuario (NFR-07); no pueden atribuirse al motor (Q21). ADR-GRP-001 fija gitoxide para leer y Git CLI para escribir; ADR-GRP-002 asigna "oplog/snapshots" a `crates/core` y la escritura a `crates/git`; ADR-GRP-006 hace del daemon el único escritor del perfil.

Hay dos tipos de escritura que no deben confundirse:

- **Escrituras internas de la Time Machine**: el almacén y el aplicador de undo, redo y restauración.
- **Operaciones de usuario lanzadas por GitRaptor**: merge, rebase, commit o descartar un worktree desde el Cockpit (F-001-02) o el MCP (F-001-05). La Time Machine solo las protege con un snapshot previo (ADR-TMC-004); no las ejecuta.

**Pregunta**: ¿qué proceso y qué código ejecutan las escrituras internas, con qué límites y en qué orden, y dónde quedan las operaciones de usuario?

## Decisión

### 1. Proceso y ubicación del código

- **El daemon (`raptor daemon`, ADR-GRP-005) aloja el motor (solo lectura) y la Time Machine (escritor interno).** Los clientes (CLI/TUI, `raptor-mcp`) nunca escriben en el repo ni en el almacén: piden por el canal.
- **Código**: módulo `timemachine` en `crates/core` (snapshots, oplog, planificador, aplicador, recuperación, purga). **Sin crate nuevo**: ADR-GRP-002 no cambia (TQ-2 → a).
- **Capa de escritura de la Time Machine en `crates/git`, separada de la de lectura**, con lista cerrada de operaciones tipadas. Solo la usa `timemachine`; ni el observador del motor ni el ejecutor de operaciones de usuario la alcanzan (visibilidad de módulo, frontera de Nx y comprobación estática en CI equivalente a ADR-GRP-009 § Validación 5). La nota que lo aclara en ADR-GRP-009 está aprobada (TQ-12) y pendiente de integración en `docs/arch-motor-local`.
- **Escritor del almacén con gitoxide** (Enmienda 2026-10-04, SPIKE-TMC-001). Al activarse el escalón 3 de ADR-TMC-006 § 5, la escritura del almacén se hace en el proceso con gitoxide. Vive **dentro de esta capa, como submódulo propio**, sin crate nuevo (TQ-2 → a). Sus reglas:
  - Recibe un **tipo de acceso que solo puede abrir el almacén validado** (`<datos>/tm/<id-repo>/store.git`, ADR-TMC-001). El tipo de acceso al repo del usuario no ofrece ninguna operación de escritura con gitoxide.
  - **Comprobación estática en CI**: las funciones de escritura de gitoxide solo aparecen en ese submódulo.
  - Gitoxide se abre **aislado**: sin la configuración de sistema ni la global, y sin lanzar `git` por su cuenta.

### 2. Reglas de las escrituras internas

Heredan ADR-GRP-009 § 3 y añaden lo que fijan **SEC-TMC-02** (invocación sin código configurable y allowlist sin porcelana) y **SEC-TMC-14** (separadores de opciones y validación de refs):

- **Sin hooks, filtros, firma ni transportes**, con configuración de sistema y global neutralizadas y `--git-dir`/`--work-tree` explícitos. Motivos: no ejecutar código del usuario, que un hook no altere una restauración y que un hook de Guardrails que pide un snapshot (ADR-TMC-004 § 3) no entre en recursión. **Refina NFR-07 ("respeta los hooks del usuario") para las escrituras internas (TQ-13 → a).**
- **Sin conversiones**: los archivos del working tree los escribe la Time Machine con los bytes guardados (ADR-TMC-001 § 2), nunca con `checkout` ni `smudge`.
- **Sin red**: ninguna operación de remoto ni credenciales (BR-TMC-EDGE-001, SEC-TMC-05). Los objetos pasan entre almacén y repo como pack, sin transporte.
- **Raíz del worktree** tomada del estado validado del daemon, nunca de un parámetro ni de `core.worktree`.
- **Binario de Git** (Enmienda 2026-10-04, E6). Las invocaciones de Git CLI de esta capa (lo que se lleva al repo del usuario y el mantenimiento del almacén) usan **el mismo binario que ya resuelve el daemon** (ADR-GRP-009 § 4), no una segunda resolución:
  - Una sola vez, al arrancar, con ruta absoluta, y comprobando ≥ 2.38 sobre ese binario.
  - En macOS, nunca el *shim* `/usr/bin/git`: cuesta unos 17 ms extra por proceso.
  - **Tampoco se ejecuta `xcrun`**, que puede abrir el diálogo de instalación de las herramientas, algo que prohíbe ADR-GRP-009 § 4. El directorio del desarrollador se resuelve leyendo el disco (`readlink /var/db/xcode_select_link`) y la ruta real de Git se toma de ahí.
  - Cambiar esa resolución para el motor y el ejecutor es una nota pendiente para ADR-GRP-009 § 4 y ADR-GRP-001.
- Añadir una operación a la lista exige revisar este ADR; el detalle está en la Dev Spec de TS-TMC-003. La escritura del almacén con gitoxide (§ 1) y el `repack` y el `prune` del mantenimiento (ADR-TMC-007 § 4) forman parte de la lista desde la Enmienda de 2026-10-04.

### 3. Protocolo de aplicación (undo, redo, restauración)

El planificador calcula un **estado destino** por worktree y por ref (ADR-TMC-003, ADR-TMC-005). Para el undo por agente, el destino es **selectivo**: solo unas rutas y ramas, partiendo del previo (Enmienda 2026-10-28, US-TMC-011). El aplicador lo lleva al repo en este orden y anota cada paso en el diario (ADR-TMC-003):

1. **Precondiciones** (sin cambios si fallan): ninguna operación de Git en curso en un worktree afectado (`rebase-merge/`, `rebase-apply/`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `BISECT_LOG`, `sequencer/`; BR-TMC-EDGE-004); ningún lock de Git presente (`index.lock`, `HEAD.lock`, locks de refs), que se reporta como "Git ocupado" y **nunca se borra**; repo disponible (`safe.directory`); árbol destino revalidado (SEC-TMC-04, SEC-TMC-09). Qué es un "worktree afectado" y cuándo un lock ajeno da "Git ocupado" antes de llegar aquí: Enmienda (2026-10-27, US-TMC-015), al final.
2. **Snapshot previo garantizado** del ámbito (BR-TMC-CONS-001). Si falla, la operación no se ejecuta.
3. **Locks**: lock por repo de la Time Machine en el daemon (una aplicación a la vez; Enmienda 2026-10-04, Cockpit: es el cerrojo de escritura del repo que comparte con el ejecutor de operaciones) y `index.lock` de cada worktree afectado, creado en exclusiva y anotado en el diario (ruta e inodo). Mientras dura, los comandos de Git de un agente sobre ese worktree fallan rápido en vez de mezclarse.
4. **Objetos**: si el destino apunta a commits que el repo ya no tiene, se copian del almacén al repo (escritura explícita e inocua).
5. **Refs**: una sola transacción con valor anterior esperado para cada rama y HEAD. Si un agente movió una ref desde la planificación, la transacción falla entera y se detiene sin cambios.
6. **Archivos**: apertura relativa a la raíz sin escapar de ella (SEC-TMC-04) y reemplazo por **intercambio atómico**: lo desplazado se compara con el snapshot previo; si difiere, se deshace el intercambio y la ruta se reporta como solape; lo desplazado se guarda en el almacén antes de borrarlo (SEC-TMC-11). Las rutas que sobran se borran sin atravesar enlaces; los ignorados y las exclusiones declaradas nunca se escriben ni se borran.
7. **Índice**: el índice destino se construye en un temporal y se instala escribiéndolo en el `index.lock` propio y renombrándolo, con el protocolo de Git. Esto libera el lock.
8. **Cierre**: se libera el lock del repo y la operación queda `terminada` con sus avisos.

**Fallo detectado a mitad** (disco lleno, archivo bloqueado en Windows): no hay rollback automático. La operación queda `interrumpida`, igual que tras un `kill -9`, y se informa de que `raptor undo` vuelve al snapshot del paso 2 (ADR-TMC-003; TQ-10 → a). Hay un único camino de recuperación, y es el que prueba el arnés de caos.

### 4. Aviso "ya empujado" (BR-TMC-EDGE-001, US-TMC-014)

Al planificar, por cada commit que la operación quita de una rama local, se comprueba **en solo lectura y sin red** si es alcanzable desde alguna `refs/remotes/*`. Si lo es, la operación procede en local y el resultado lleva el aviso con la ref remota y la antigüedad de ese dato. Rama sin remoto o commit no publicado: sin aviso ni error. La Time Machine nunca hace push ni force-push.

### 5. Operaciones de usuario lanzadas por GitRaptor

- Las operaciones del Cockpit y del MCP pasan por la **operación protegida** de ADR-TMC-004 § 1: la Time Machine aporta la intención, el snapshot previo y el registro, y nada más.
- Las **ejecuta el ejecutor de operaciones del daemon**, propiedad de F-001-02 y F-001-05, **no la capa de escritura de la Time Machine**. Respetan la configuración y los hooks del usuario (NFR-07) y siguen las reglas de argv fijo y sin shell de NFR-02. Así se cumple BR-TMC-CONS-004: la Time Machine solo escribe snapshot, undo, redo y restauración (TQ-3 → a).
- El catálogo de operaciones, con sus parámetros y su ámbito, es un **contrato de interfaz de F-001-02 y F-001-05**. Cada operación declara su ámbito y si es destructiva (ADR-TMC-007 § 2). (Enmienda 2026-10-04, Cockpit: el catálogo es el de ADR-CKP-002.)

## Alternativas consideradas

| Alternativa | En contra | Veredicto |
|---|---|---|
| **Escribe la CLI** (proceso del cliente) | Varios escritores del almacén y del oplog (rompe ADR-GRP-006); el solicitante se resolvería en el cliente, que ADR-GRP-005 § 6 declara no confiable | Descartada |
| **Crate nuevo `crates/timemachine`** | Separa mejor, pero enmienda ADR-GRP-002 y duplica la capa de invocación de Git que ADR-GRP-009 quiere única | Descartada (TQ-2 → a) |
| **Restaurar con `checkout`/`restore` de Git** | Ejecuta `smudge`, filtros y `post-checkout`; no restaura lo sin seguimiento; la ida y la vuelta no es exacta | Descartada |
| **Las operaciones de usuario por la capa de la Time Machine** | Sin hooks del usuario (rompe NFR-07) y la Time Machine pasaría a escribir más que sus cuatro casos (BR-TMC-CONS-004) | Descartada |
| **Restaurar sin locks de Git** | Un `git add` de un agente a mitad de la restauración mezcla índices | Descartada |
| **Rollback automático ante un fallo detectado** | Otra escritura justo cuando el entorno falla; dos caminos de recuperación que probar | Descartada (TQ-10 → a) |

## Consecuencias

- ✅ El motor sigue sin escribir y la Time Machine solo escribe sus cuatro casos: cada escritura tiene un dueño y una capa comprobada en CI (Q21, BR-TMC-CONS-004).
- ✅ Un solo escritor del almacén, del oplog y del perfil (ADR-GRP-006). El solicitante se resuelve donde ADR-GRP-005 lo exige: en el daemon.
- ✅ Las escrituras internas no ejecutan hooks, filtros ni red: comportamiento determinista y sin superficie para un repo hostil.
- ✅ El intercambio atómico (SEC-TMC-11) cierra la ventana entre la comparación y el reemplazo. En un sistema de archivos sin intercambio atómico, la ruta se reporta como "no restaurable con garantía".
- ⚠️ **Las escrituras internas no ejecutan los hooks del usuario** (`reference-transaction`, `post-checkout`). Refina NFR-07 para ese caso (TQ-13 → a); el PO lo deja escrito en el requerimiento. Las operaciones de usuario sí los ejecutan (§ 5).
- ⚠️ El índice instalado no lleva información de stat: el primer `git status` tras restaurar rehace la comparación de contenido.
- ⚠️ En Windows, un archivo abierto por un editor puede impedir el reemplazo: la operación queda interrumpida y se recupera con undo. Se mide en INF-TMC-001.

## Validación

1. **Frontera**: la comprobación estática de CI falla si el motor o el ejecutor de operaciones de usuario importan la capa de escritura de la Time Machine, o si se añade una operación de remoto o de porcelana. También falla si una función de escritura de gitoxide aparece fuera del submódulo del almacén (Enmienda).
2. **Sin código configurable**: repo canario de SEC-TMC-02: 0 marcadores tras snapshot, undo, redo y restauración.
3. **Precondiciones**: con rebase o merge a medias, o con `index.lock` ajeno, la operación se rechaza, el repo no cambia y el lock ajeno sigue ahí (US-TMC-015).
4. **Concurrencia**: un agente simulado que mueve una rama durante la aplicación hace fallar la transacción sin cambios; uno que escribe un archivo durante el intercambio provoca que se deshaga el intercambio y esa ruta se reporte como solape (SEC-TMC-11).
5. **Rutas hostiles**: corpus de SEC-TMC-04: 0 escrituras fuera del worktree.
6. **Ya empujado**: commit alcanzable desde `refs/remotes/origin/*`, con aviso; sin remoto, sin aviso; captura de red: 0 conexiones (US-TMC-014).
7. **Caos**: muerte del daemon en cada paso, de 3 a 7 (INF-TMC-001).

## Referencias

- **Reglas**: BR-TMC-CONS-001, BR-TMC-CONS-004, BR-TMC-WF-001, BR-TMC-WF-003, BR-TMC-EDGE-001, BR-TMC-EDGE-003, BR-TMC-EDGE-004; D-TMC-14, D-TMC-20. Q21, Q22.
- **ADRs**: ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009; ADR-TMC-001, ADR-TMC-003, ADR-TMC-004, ADR-TMC-005.
- **Seguridad**: SEC-TMC-02, 04, 05, 09, 11, 14. **Enablers**: TS-TMC-003, TS-TMC-004, INF-TMC-001. **NFR**: NFR-01, NFR-02, NFR-07, NFR-12.

## Enmienda (2026-10-04, SPIKE-TMC-001)

Aplicada desde § 7 de [SPIKE-TMC-001-resultados.md](../../requirements/features/time-machine/research/SPIKE-TMC-001-resultados.md), medido **solo en macOS**. El `status` sigue en `accepted`. Decisión del orquestador (2026-10-04), validada por el Arquitecto, que pidió ajustes y están incorporados.

| Cambio | Dónde | Fuente |
|---|---|---|
| La escritura del almacén con gitoxide (escalón 3) vive en un submódulo propio de esta capa: tipo de acceso que solo abre el almacén validado, comprobación estática de CI y gix aislado | § 1, § 2, Validación 1 | E1; ADR-TMC-006 § 5; revisión del Arquitecto |
| Git CLI de esta capa: el binario que ya resuelve el daemon, una vez y con ruta absoluta; en macOS ni el shim `/usr/bin/git` ni `xcrun` (resolución leyendo el disco). Nota pendiente para el motor y el ejecutor | § 2 | E6; Resultados § 2.1 y § 4; revisión del Arquitecto |
| `repack` y `prune` del almacén en la lista cerrada | § 2 | E10; ADR-TMC-007 § 4 |

## Enmienda (2026-10-04, TS-TMC-003)

Aplicada desde la implementación de [TS-TMC-003](../../requirements/features/time-machine/dev-specs/TS-TMC-003-escritura-aplicador.md), verificada **solo en macOS** con Git 2.50.1. El `status` sigue en `accepted`. Decisión del orquestador (2026-10-04), validada por el Arquitecto, que pidió registrar aquí el cambio del paso 5.

| Cambio | Dónde | Motivo |
|---|---|---|
| **`HEAD` fuera de la transacción de refs.** Las ramas y `refs/stash` van en una sola transacción `update-ref --stdin -z` con valor anterior. Cada `HEAD` se verifica antes y se escribe después con el protocolo de lock de Git (`HEAD.lock` exclusivo, comparación del contenido esperado bajo el lock, `fsync` y rename). Si falla tras la transacción, la operación queda `interrumpida` (§ 3) | § 3, paso 5 | `symref-update` exige Git 2.46 (> 2.38); con `HEAD.lock` tomado, `update-ref` no puede mover la rama a la que apunta `HEAD`; y `update-ref` rechaza `worktrees/<id>/HEAD`, así que el `HEAD` de un worktree enlazado tampoco cabe en la transacción. Comprobado. El `HEAD` restaurado no deja entrada de reflog (aviso tipado) |
| **`refs/stash` nunca se borra** y solo mueve su cima | § 3, paso 5 | Borrar la ref borra su reflog, la pila entera (NFR-01) |
| **Lista cerrada** con sus opciones fijas y los datos solo por stdin: `update-ref --stdin -z`, `update-index -z --index-info`, `update-index --skip-worktree -z --stdin`, `pack-objects --revs --stdout`, `index-pack --stdin --strict --keep`, `repack -d --geometric=2`, `prune --expire=1.hour.ago` | § 2 | SEC-TMC-02, SEC-TMC-14; `--keep` evita que un `gc` ajeno tire el pack antes de que las refs lo alcancen |
| **`safe.directory`**: la confianza la decide antes la capa de lectura con la configuración del usuario; la capa de escritura, que neutraliza la global, pasa `-c safe.directory=<ruta validada>` | § 2 | Sin ello, `GIT_CONFIG_GLOBAL` vacío rechazaría repos que el usuario declaró de confianza |
| **`ready → rechazada`**: una precondición que falla bajo los locks del aplicador, antes de cualquier cambio, termina la operación como `rechazada` | § 3, pasos 1 y 3; ADR-TMC-003 | Distinguir "no se tocó nada" de una interrupción |
| **Toda escritura sobre el repo del usuario vive en `crates/git/src/tm_write/`** (intercambio, borrado, `HEAD.lock`, `index.lock`, administración de worktrees), con comprobación estática | § 1, Validación 1 | ADR-GRP-002. Excepción conocida: la liberación de locks anotados de la recuperación (TS-TMC-002), pendiente de moverse |

## Enmienda (2026-10-04, Cockpit)

Aplicada desde DEP-CKP-7 de [CTX-CKP-001](../../requirements/features/cockpit/context.md), con [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) § 1, § 5 y § 6 (accepted 2026-10-04) y Q-CKP-19. **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia la capa de escritura de la Time Machine, el protocolo de aplicación ni que las operaciones de usuario las ejecute el ejecutor del daemon. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| El lock por repo del aplicador es el **cerrojo de escritura del repo**, compartido con el ejecutor de operaciones: un merge y un undo nunca se mezclan | § 3, paso 3 | ADR-CKP-002 § 5; Q-CKP-19 |
| El catálogo de operaciones de usuario es el de ADR-CKP-002 (versión 1: ocho operaciones, con su ámbito, su clase y sus marcas Cockpit y MCP) | § 5 | DEP-CKP-7; ADR-CKP-002 § 1 |
| Nota E6 cerrada para el ejecutor: usa el mismo binario de Git resuelto, una vez y con ruta absoluta | Enmienda (SPIKE-TMC-001) | ADR-CKP-002 § 6; ADR-GRP-009 (Enmienda, Cockpit) |

- **Cerrojo compartido**: vive en un módulo neutro de `crates/core` que usan el aplicador y el ejecutor; la clave es el directorio común del repo. No se usa un cerrojo por worktree, porque un merge toca dos worktrees y refs del repo. Las peticiones esperan en orden de llegada y se ven como "en cola" en todos los clientes (ADR-CKP-002 § 5). **Estado en main**: la Enmienda (2026-10-04, TS-TMC-003) implementó el lock del aplicador en `timemachine`, con clave la ruta del almacén (única por repo y perfil) y sin espera; la Dev Spec de TS-CKP-002 lo reutiliza como este cerrojo y le añade la cola.
- **Sin interbloqueo con los hooks**: la evaluación de un hook de Guardrails nunca espera a este cerrojo (ADR-GRD-003, Enmienda (2026-10-04, Cockpit)). Las escrituras internas de esta capa siguen sin hooks (§ 2).
- **Validación añadida**: un undo pedido mientras el ejecutor integra en el mismo repo espera en cola y después revalida su plan; nunca se aplican a la vez.

## Enmienda (2026-10-27, US-TMC-015)

**Decisión del orquestador (2026-10-27), validada por Arquitecto y PO**. Origen: [DS-US-TMC-015](../../requirements/features/time-machine/dev-specs/US-TMC-015-operacion-git-en-curso.md), § Enmiendas propuestas, D1 a D5, D7 y D-TMC-28. Precisa el paso 1 del § 3 y la Validación 3; no cambia el orden del protocolo, la lista de marcadores ni que un lock ajeno nunca se borra. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| "Worktree afectado": los worktrees del plan; con el ámbito de refs acotado del undo, los marcadores del worktree principal solo cuentan si está en el ámbito; la rama de un rebase o un bisect en un worktree fuera del ámbito cuenta como en uso | § 3, paso 1 | D2, D3; D-TMC-28 |
| "Git ocupado" (`git-busy`) también cuando un lock ajeno impide la calma del motor | § 3, paso 1 | D4 |
| El aviso nombra la operación en curso y el worktree | Validación 3 | D1 |

- **Marcadores** (D2): con el ámbito de refs acotado (undo, redo, `undo --since`), un marcador de operación en curso cuenta solo en los worktrees del plan. Los locks del directorio común cuentan siempre. La restauración, que mueve todas las ramas, mantiene los marcadores del worktree principal: una operación en curso en `main` la sigue deteniendo (asimetría aceptada en el MVP: detiene de más, nunca escribe de más).
- **Rama en uso** (D3): la rama que reescribe un rebase o que dejó un bisect en un worktree fuera del ámbito tiene `HEAD` separado en ese worktree, pero está en uso: undo y restauración responden `ref-in-use` y no la mueven.
- **Regla de negocio** (D-TMC-28, BR-TMC-EDGE-004): una operación de Git en curso en otro worktree no bloquea el undo de este, salvo que use una rama que el undo movería.
- **Lock ajeno y calma** (D4): si la calma del motor no llega en su límite (ADR-TMC-003, Enmienda 2026-10-21, `fix/tm-restore-calm-mark`) y hay un lock ajeno en el worktree pedido o en el directorio común, la respuesta es `git-busy`; si no lo hay, `repo-busy`, como antes. Si el worktree no se puede abrir para comprobarlo, `repo-busy` (falla cerrado). Un lock que aparece y desaparece puede dar `repo-busy`, que también deja el repo igual. El lock sigue sin borrarse nunca.
- **Aviso** (D1, D5): el rechazo nombra la operación en curso y el worktree, en un campo opcional de la respuesta detrás de una capacidad; un cliente sin ella recibe la respuesta de antes. El oplog guarda solo el código del rechazo.
- **Herencia** (D7): redo y undo selectivo siguen esta regla: precondiciones antes del previo y repetidas bajo los locks del aplicador, con el mismo detalle.

**Validación 3, ampliada** (definida en DS-US-TMC-015 § 9.4; tests por implementar): el rechazo nombra la operación y el worktree; un merge a medias en el worktree principal no bloquea el undo de un agente en otro worktree; la rama que reescribe un rebase en otro worktree no se mueve (`ref-in-use`); un `index.lock`, un lock de rama o `packed-refs.lock` ajenos dan `git-busy` en undo y en restauración, con los mismos bytes tras el rechazo.

## Enmienda (2026-10-28, US-TMC-011)

**Decisión del orquestador (2026-10-27), validada por Arquitecto** (revisión de segunda opinión el 2026-10-28). Origen: [DS-US-TMC-011](../../requirements/features/time-machine/dev-specs/US-TMC-011-undo-por-agente.md) E2, D3, D7 y D10. Añade un modo al § 3; no cambia el orden del protocolo, sus precondiciones, el intercambio atómico ni la protección de último instante. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| El aplicador admite un destino selectivo (`ApplyPlan.selective`): escribe solo las rutas (archivo e índice), las ramas y el `HEAD` indicados, cada uno con su valor, y deja el resto del worktree como está en el previo | § 3 | E2; D3 |
| El modo selectivo y las rutas conservadas (`PlanWorktree.keep`, Enmienda 2026-10-27 de ADR-TMC-005) son excluyentes | § 3, paso 6 | E2 |
| El modo selectivo nunca escribe una ruta que el previo excluye: el undo se rechaza antes del aplicador (`not-selectable`) | § 3, paso 2 y 6 | D7; NFR-01 |

- **Por qué un modo y no un snapshot sintético** (D3): el destino del undo por agente no es un punto capturado (es "lo de antes" solo en lo que tocó el agente). Fabricar un punto en el almacén ensuciaría el timeline con estados que nadie capturó; el modo selectivo conserva el intercambio, la protección y el índice del protocolo.
- **De dónde sale el contenido** (D7): del previo garantizado del undo, con las entradas del destino aplicadas encima; el destino del plan (`target_snapshot`) es el propio previo, así que el undo no depende de que el punto de la operación más antigua siga en el almacén. Los commits que las ramas del destino necesiten y el repo no tenga siguen el paso 4, desde el almacén.
- **Refs** (paso 5): exactamente las ramas del destino, con valor esperado el del previo; `RefScope::Only` con esas ramas.
- **Rutas** (D10): salen de árboles del repo leídos con gitoxide, nunca de la petición, y pasan la misma apertura relativa a la raíz y revalidación (SEC-TMC-04). Una ruta que no se puede escribir tal cual (no UTF-8, demasiado larga) rechaza el undo; nunca se recorta.
- **Recuperación**: como todo undo, el selectivo tiene su previo garantizado; redo vuelve a él (NFR-01).

**Validación añadida** (definida en DS-US-TMC-011 T005 y § 9.4; tests por implementar): el modo selectivo escribe solo sus rutas, borra una ruta ausente en su destino, mueve solo sus ramas, y con `keep` o con una ruta excluida no cambia nada.
