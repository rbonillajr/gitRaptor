---
id: ADR-TMC-001
title: "ADR-TMC-001 — Almacén de snapshots: repo Git privado por repo en el perfil"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-TMC-002, ADR-TMC-003, ADR-TMC-006, ADR-TMC-007]
  stories: [US-TMC-001, US-TMC-004, US-TMC-005, US-TMC-009, US-TMC-016, US-TMC-018, US-TMC-020]
description: "Los snapshots viven en un repo Git privado por repo dentro del perfil, con objetos propios y una referencia por snapshot; nunca en el repo del usuario"
tags: [adr, time-machine, snapshots, almacen, perfil, cero-perdida-de-datos, d-tmc-11]
published: true
---

# ADR-TMC-001 — Almacén de snapshots: repo Git privado por repo en el perfil

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-1 → (c) almacén autocontenido en el perfil; TQ-4 → (a) gitoxide preaprobado solo en el almacén; TQ-5 → (b) límites de tamaño y cuotas; TQ-15 → (a) repos anidados excluidos; TQ-16 → (a) lista de credenciales excluida; TQ-17 → (a) `forget` aplazado.

> **Restricciones activas**: no hay `architecture-constitution.md` en la cascada. ⚠️ **ASSUMPTION**: rigen como constitución ADR-GRP-001 (gitoxide para leer, Git CLI para escribir) y ADR-GRP-002 (crates), igual que en motor-local. Fuente: inline; formalizar con `/aadd-architect --init-constitution`.

## Contexto

Un snapshot es el working tree completo de uno o varios worktrees (modificados, preparados y sin seguimiento, **sin ignorados**, D-TMC-16) más el estado de ramas y worktrees (BR-TMC-CONS-002). D-TMC-11 y BR-TMC-CONS-004 imponen cuatro garantías: (1) no se empujan al remoto por accidente, ni con `push --mirror`, `push --all` o un refspec `refs/*:refs/*`; (2) ni `git gc` ni `prune` los borran, tampoco los commits que un `reset --hard` deja inalcanzables; (3) un agente que trabaja en el working tree no los altera; (4) guardar un snapshot no cambia el estado observable del repo (índice, HEAD, ramas, status, stash, `git log --all`).

Hechos que acotan la decisión: el motor solo observa (Q21, ADR-GRP-009) y el arnés INF-GRP-001 exige cero diferencias en el repo; nadie ejecuta programas del usuario al leer (ADR-GRP-009); el único escritor del perfil es el daemon (ADR-GRP-005, ADR-GRP-006); el overhead debe quedar bajo 200 ms (NFR-04, ADR-TMC-006); y retirar snapshots (ADR-TMC-007) no debe tocar el repo.

**Pregunta**: ¿dónde y en qué forma se guardan los snapshots para cumplir las cuatro garantías sin frenar a los agentes?

## Decisión

**Cada repo observado tiene un almacén de snapshots propio: un repo Git *bare* privado en la carpeta de datos del perfil (`<datos>/tm/<id-repo>/store.git`, con el UUID de ADR-GRP-006). Contiene sus propios objetos, sin `alternates` hacia el repo. Cada snapshot es un commit del almacén con una referencia propia. La Time Machine nunca escribe objetos ni refs en el repo del usuario para guardar un snapshot.**

### 1. Forma de un snapshot

- **Commit del almacén** cuyo árbol contiene, por cada worktree incluido, `wt/<clave-worktree>/files` (contenido bruto del working tree) y `wt/<clave-worktree>/index` (lo preparado, entradas en etapa 0), más un blob `meta` con: HEAD de cada worktree (simbólico o separado), puntas de `refs/heads/*` y de `refs/stash`, lista de worktrees (ruta, rama, bloqueo), marcas del índice que un árbol no guarda (intent-to-add, skip-worktree, entradas en conflicto) y la lista de exclusiones con su motivo.
- **Padres del commit** = los commits distintos a los que apuntan esas ramas, HEAD y stash. Así la alcanzabilidad dentro del almacén conserva el historial que el snapshot necesita, igual que hace `git stash` con sus padres.
- **Referencia** `refs/tm/snap/<id-snapshot>` en el almacén. Crear esa referencia es el **punto de validez** del contenido; el snapshot cuenta cuando, además, su fila del oplog está confirmada (ADR-TMC-003).
- **Qué worktrees entran**: una captura por observación incluye el worktree que cambió; un snapshot previo garantizado incluye todos los worktrees del ámbito de la operación (ADR-TMC-004). El estado de refs es siempre el del repo entero.

### 2. Contenido: bytes brutos, sin programas del usuario

- **Working tree**: se guarda el contenido **tal cual está en disco**, sin filtros `clean`, sin conversión de fin de línea ni `ident`. Se reutiliza el blob del índice solo si el archivo está limpio según el stat **y** su ruta no tiene atributos de conversión. Restaurar escribe esos mismos bytes (ADR-TMC-002): la ida y la vuelta son exactas y no se ejecuta nada configurado por el usuario, igual que exige ADR-GRP-009 al motor.
- **Git LFS y otros filtros**: el snapshot guarda el contenido real del working tree (el archivo ya convertido por `smudge`) y, en `index`, el puntero que tiene el índice. Restaurar devuelve el archivo real sin ejecutar `smudge`. Coste: un archivo LFS grande ocupa su tamaño real en el almacén (ver § 4).
- **Ignorados**: excluidos con las reglas de ignore de Git, leídas en solo lectura (`.gitignore`, `info/exclude`, `core.excludesFile`), las mismas que usa el motor (ADR-GRP-010). Un archivo con seguimiento que coincide con un patrón de ignore se incluye, como en Git.
- **Credenciales sin seguimiento ni ignorar** (TQ-16 → a; cambia D-TMC-16, que actualiza el PO): una lista cerrada de nombres de credenciales se trata como ignorada y se declara, con opción en el perfil para incluirla (SEC-TMC-06). Borrar contenido ya capturado (`forget`) queda aplazado a una US futura (TQ-17 → a).
- **Modos y enlaces**: archivo normal, ejecutable y enlace simbólico (el destino se guarda como contenido). No se guardan propietario, permisos fuera del bit de ejecución, atributos extendidos, fechas ni directorios vacíos (modelo de Git). En Windows los enlaces siguen `core.symlinks` del repo.
- **Submódulos**: se guarda el gitlink (commit del índice y HEAD del submódulo leído en solo lectura). **El contenido del working tree del submódulo no entra**: es otro repo, con su propia Time Machine si se observa. El snapshot lo registra como exclusión "submódulo" y la restauración lo avisa.
- **Repos anidados sin seguimiento** (un directorio con su propio `.git` dentro del worktree): se excluyen y se declaran (TQ-15 → a) como "repo anidado"; nunca se escriben ni se borran al restaurar (SEC-TMC-04).
- **Archivos grandes**: el snapshot previo garantizado los incluye siempre (US-TMC-020, escenario 3). En la captura por observación, los archivos de más de 50 MB se excluyen y la captura queda marcada como parcial con la lista (TQ-5 → b). ⚠️ **ASSUMPTION**: la cifra de 50 MB la ajusta SPIKE-TMC-001.

### 3. Objetos: autocontenido, siembra y anclaje

- **Siembra**: al activar la Time Machine en un repo, el daemon puebla el almacén en segundo plano con el historial alcanzable desde las ramas locales. Usa enlaces duros a los packs del repo si están en el mismo sistema de archivos y cumplen las condiciones de propiedad y permisos de SEC-TMC-06; si no, copia. Hasta que termina, un snapshot previo garantizado paga la copia de lo que le falta (ADR-TMC-006 lo excluye del p95).
- **Anclaje incremental**: cuando el motor publica un commit nuevo o un movimiento de ref (ADR-GRP-010, ADR-GRP-013), el daemon copia al almacén los objetos que aún no tiene, fuera de la ruta crítica. Los blobs preparados, que solo alcanza el índice, se copian al capturar. Un snapshot nunca depende de un objeto que solo está en el repo.
- **Índice temporal propio**: los árboles se construyen con un índice temporal del almacén (`GIT_INDEX_FILE` dentro del perfil), **nunca** con el índice del usuario.
- **Escritura en el almacén**: con Git CLI y argv fijo, según la capa de escritura de ADR-TMC-002. Si SPIKE-TMC-001 demuestra que no cabe en 200 ms, se escribe en el almacén con gitoxide: excepción a ADR-GRP-001 preaprobada y limitada al almacén, nunca al repo del usuario (TQ-4 → a).

### 4. Configuración y aislamiento del almacén

- Configuración fijada por la Time Machine: sin remotos, sin hooks (`core.hooksPath` a un directorio vacío del perfil), `gc.auto=0` (mantenimiento propio, ADR-TMC-007) y sin reflogs.
- Carpeta 0700 y archivos 0600, con propietario comprobado al abrir (ADR-GRP-006 § 1). Contiene **contenido del usuario**, a diferencia del almacén del motor, que solo guarda metadatos (ADR-GRP-006 § 4); ver SEC-TMC-01.
- **El almacén es entrada no confiable**: el mismo usuario (o un agente) puede manipularlo. Objetos verificados por hash y árbol y `meta` revalidados antes de restaurar (SEC-TMC-09). Un almacén ilegible, corrupto o cambiado fuera del daemon se aparta (renombrado, nunca borrado) y la Time Machine de ese repo empieza de cero con un hueco declarado. No afecta a otros repos.
- `tm/` queda excluido de las copias de seguridad del SO (SEC-TMC-06) y sujeto a las cuotas de disco de SEC-TMC-12 (TQ-5 → b; ⚠️ **ASSUMPTION**: las cifras las ajusta SPIKE-TMC-001).

## Alternativas consideradas

| Alternativa | A favor | En contra | Veredicto |
|---|---|---|---|
| **A. Refs ocultas en el repo** (`refs/gitraptor/*`), objetos en el repo y oplog en el perfil | Sin copia de objetos; restaurar no copia nada; es lo que hacen Jujutsu, GitButler y Entire | `push --mirror` y `refs/*:refs/*` las empujan, y sin hooks propios (Q22) no hay forma de impedirlo; aparecen en `git log --all`, `for-each-ref` y en los IDE; escriben en `.git` en cada captura, lo que rompe el criterio binario de INF-GRP-001; un agente puede borrarlas con `update-ref -d` o `for-each-ref \| xargs` | Descartada: incumple la garantía 1 y la 4 |
| **B. Almacén en el perfil con `alternates` hacia el repo** | Sin push ni visibilidad; casi sin disco | `reflog expire --expire-unreachable=now` más `gc --prune=now` tras un `reset --hard` borra objetos que el almacén necesita: el snapshot queda corrupto. Es justo el escenario 2 de US-TMC-018 | Descartada: incumple la garantía 2 |
| **C. Almacén autocontenido en el perfil + oplog en el perfil** (elegida) | Cumple las cuatro garantías por construcción; una captura no toca el repo, así que un `kill -9` durante un snapshot no deja nada en él; la purga no toca el repo | Disco: hasta el tamaño del historial por repo (mitigado con enlaces duros); siembra inicial; restaurar un commit perdido exige copiar objetos de vuelta al repo | **Elegida** |
| **D. Copias de archivos** (tar o rsync por snapshot) | Simple | Sin deduplicación ni historial; restaurar refs exige Git de todos modos | Descartada |

**Trade-off principal**: se paga disco y una siembra a cambio de garantías que no dependen de la configuración del usuario, de sus hooks ni de lo que haga un agente con Git.

## Consecuencias

- ✅ Garantías 1, 3 y 4 por construcción: el almacén no es un remoto ni una ref del repo, no está en el working tree y el repo no cambia al capturar.
- ✅ Garantía 2: el `gc` del repo no ve el almacén, y el del almacén lo gobierna solo la Time Machine (ADR-TMC-007).
- ✅ El arnés de INF-GRP-001 sigue siendo binario con la Time Machine activa: fuera de un undo, una redo o una restauración, el repo no cambia.
- ✅ Deduplicación entre snapshots y worktrees, porque los objetos se direccionan por contenido.
- ⚠️ **Disco**: el almacén puede llegar al tamaño del historial del repo más el contenido único capturado. **Mitigación**: enlaces duros en la siembra, retención (ADR-TMC-007) y tamaño expuesto en diagnóstico; SPIKE-TMC-001 lo mide. Coste aceptado por Rene (TQ-1 → c).
- ⚠️ **El perfil pasa a contener código del usuario**, incluidos secretos en archivos **no** ignorados. **Mitigación**: permisos (SEC-TMC-01) y documentación del riesgo R5. Impacto en ADR-GRP-006 (pendiente de integración).
- ⚠️ Repo y perfil en volúmenes distintos: sin enlaces duros, la siembra copia. Se mide.
- ⚠️ Submódulos y metadatos de archivo (propietario, xattrs, fechas) no se protegen. Se declaran en el snapshot y en la restauración.

## Validación

Repos y perfiles temporales; nunca este repo. El arnés INF-TMC-001 cubre 1 a 4 y US-TMC-018 los convierte en criterio de aceptación.

1. **Push**: con snapshots guardados, `push --mirror`, `push --all` y `push 'refs/*:refs/*'` a un remoto bare temporal: el remoto no recibe ningún objeto ni ref del almacén.
2. **Mantenimiento**: tras `reset --hard HEAD~3`, `reflog expire --expire=now --all` y `gc --prune=now --aggressive` en el repo, todos los snapshots se restauran bit a bit.
3. **Agente**: checkout, reset, `clean -fdx` y commits en el worktree no cambian el árbol ni la ref de un snapshot.
4. **Estado observable**: la huella de `.git`, del working tree, de `for-each-ref`, de `log --all`, de `status` y de `stash list` es idéntica antes y después de guardar un snapshot.
5. **Ida y vuelta**: archivos con CRLF, `ident`, LFS (con y sin smudge), ejecutables, enlaces y nombres Unicode vuelven idénticos; un repo canario con `filter.*.clean` y hooks que dejan rastro no deja rastro.
6. **Ignorados**: `.env` y `node_modules/` no aparecen en ningún árbol del almacén.

## Referencias

- **Reglas**: BR-TMC-CONS-002, BR-TMC-CONS-004; D-TMC-11, D-TMC-16. **Historias**: US-TMC-001, 004, 005, 009, 016, 018, 020.
- **ADRs**: ADR-GRP-001, ADR-GRP-002, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010; ADR-TMC-002 (quién escribe), ADR-TMC-003 (oplog), ADR-TMC-006 (presupuesto), ADR-TMC-007 (purga).
- **Enablers**: TS-TMC-001, INF-TMC-001, SPIKE-TMC-001. **NFR**: NFR-01, NFR-03, NFR-04, NFR-05. **Seguridad**: SEC-TMC-01, 06, 09, 12.
