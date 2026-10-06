---
id: ADR-GRD-002
title: Operaciones interceptables y límites de la capa de hooks
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-06
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRD-001, ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, CTX-GRD-001, BR-GRD-001, SPIKE-GRD-001]
tags: [guardrails, hooks-git, reference-transaction, pre-push, pre-rebase, interceptabilidad, br-edge-003, no-verify, presupuesto-latencia, refs-reemplazo, normalizacion-refs, reftable, pack-refs, spike-grd-001]
---

# ADR-GRD-002 — Operaciones interceptables y límites de la capa de hooks

> **Estado**: aceptado por Rene Bonilla el 2026-10-04. Enmendado el 2026-10-04 con los resultados de SPIKE-GRD-001 en macOS (ver "Enmienda (2026-10-04, SPIKE-GRD-001)") y el 2026-10-05 por US-GRD-001 (forma del dispatcher nativo y coste en Windows, ver "Enmienda (2026-10-05, US-GRD-001)") y el 2026-10-06 por XP-30 (renombrado sobre la base con reftable y procesos de hook por versión de Git, ver "Enmienda (2026-10-06, XP-30)"); la matriz del spike en Linux y la validación funcional en Windows siguen pendientes.

## Contexto

BR-EDGE-003 y Q-GRD-8 exigen **declarar** qué operaciones del catálogo (BR-VAL-002: commit, push, force-push, `reset --hard`, borrar rama, rebase, merge, crear worktree y borrar worktree) no puede impedir la capa de hooks con Git crudo, y mostrarlo en el estado de protección (US-GRD-004). La lista tiene que coincidir con lo que se observa al probar esas operaciones. US-GRD-007 cubre por Git directo solo las operaciones que esa lista declara interceptables.

Hay otros tres requisitos:

- **Momento de la evaluación**: una operación cuenta como **impedida** solo si se evalúa **antes de cualquier efecto**.
- **Latencia**: la evaluación añade < 100 ms en repos medianos (contexto § 6, confirmado por Rene Bonilla el 2026-10-04).
- **Saltos voluntarios**: Git permite saltarse los hooks a propósito. Es el riesgo R-GRD-1, aceptado.

## Decisión

**La capa de hooks gobierna cada operación con el hook que corre antes de sus efectos, y usa `reference-transaction` en estado `prepared` como segunda línea, que `--no-verify` no salta. Toda ref que no esté en una lista explícita de excepciones es gobernada. Las lecturas en las que se basa la decisión ignoran los objetos de reemplazo. La lista publicada de lo que no se puede impedir es un dato versionado en el binario, verificado por SPIKE-GRD-001 y por la regresión de INF-GRD-001.**

### 1. Matriz (confirmada en macOS por SPIKE-GRD-001; Linux y Windows pendientes)

**Momento**:

- **A**: el hook corre antes de cualquier efecto. La operación queda impedida.
- **B**: el hook corre después de efectos parciales en el working tree o el índice. Protege la ref, pero la operación no cuenta como impedida.
- **C**: no hay hook antes del efecto.

| Operación (BR-VAL-002) | Hook principal | Segunda línea | Momento | Se publica como | Salto voluntario conocido |
|---|---|---|---|---|---|
| **Commit** | `pre-commit` (contenido), `commit-msg` (formato) | `reference-transaction` `prepared` sobre `refs/heads/*`: evalúa el rango `viejo..nuevo` | A | Impedible | `--no-verify` salta el principal, no la segunda línea |
| **Push** | `pre-push` (refs y objetos por la entrada estándar) | — | A | Impedible | `--no-verify`; plumbing `send-pack`, que no ejecuta `pre-push` (declarado; Enmienda 2026-10-04) |
| **Force-push** | `pre-push`: una ref es forzada si el objeto remoto no es ancestro del local **sin objetos de reemplazo ni grafts y sin fiarse del commit-graph para los padres**, o si el objeto remoto falta o la historia es superficial (H-05) | — | A | Impedible. **Se deniega de más**: un push fast-forward desde un clon superficial cuenta como forzado (F07; Enmienda 2026-10-04) | `--no-verify` |
| **Borrar rama (local)** | `reference-transaction` `prepared`, con valor nuevo igual a ceros sobre `refs/heads/*`, salvo el *prune* de `pack-refs` (§ 4) | — | A | Impedible | `--no-verify` no la salta |
| **Borrar o reescribir la rama base renombrando** (`branch -m base x`, `branch -M x base`) (Enmienda 2026-10-04) | `reference-transaction` `prepared` (backend de archivos) | — | A con archivos (`-M x base` deja `x` borrada: B, § 2) / **C con reftable** (solo corre después la transacción de `HEAD`; Enmienda 2026-10-06) | Impedible con archivos. **No impedible en repos reftable**: el renombrado no ejecuta ningún hook (D10, D11) | — |
| **Borrar rama (remota)** | `pre-push` con el objeto local igual a ceros | — | A | Impedible | `--no-verify` |
| **`reset --hard`** | Ninguno antes de reescribir el working tree | `reference-transaction` si mueve la rama | C / B | **No impedible**. Mitigación: Time Machine | — |
| **Rebase** | `pre-rebase` (también en `pull --rebase`, después del `fetch` y antes de tocar el working tree) | `reference-transaction` al mover la rama al final | A (principal) | Impedible | `--no-verify` salta el principal |
| **Merge** | `pre-merge-commit` (después de escribir el resultado en el working tree). Fast-forward: solo `reference-transaction`, con el working tree ya actualizado | `reference-transaction` | B (también fast-forward) | **No impedible**: la rama destino no cambia, pero queda una fusión en curso o el working tree actualizado | `--no-verify` en el principal |
| **Crear worktree** | Con rama nueva: `reference-transaction` al crear la rama, antes de crear la administración. Sin rama nueva: `reference-transaction` sobre el `HEAD` del worktree nuevo, con la administración ya creada; si deniega, Git la revierte | `post-checkout` (después; solo informa) | A con rama nueva / A efectiva sin ella | Con rama nueva, impedible. Sin ella, **no impedible por la política**: el hook solo ve una línea de `HEAD` y no puede reconocer la operación (motivo `no-reconocible`, § 3) | — |
| **Borrar worktree** | Ninguno | — | C | **No impedible** | — |

### 2. Saltos: cerrados y declarados (I-01)

| Salto | Estado | Dónde |
|---|---|---|
| `--no-verify` en `pre-commit`, `pre-push`, `pre-rebase` y `pre-merge-commit` | **Declarado** | Lista publicada (§ 3) |
| `-c core.hooksPath=…`, `GIT_CONFIG_COUNT/KEY/VALUE`, `GIT_CONFIG_PARAMETERS` en un solo comando | **Declarado** (no detectable) | Lista publicada |
| Editar `.git/config`, o borrar o alterar los dispatchers | **Detectado** como pérdida | ADR-GRD-005 |
| Escribir a mano en `refs/` o en `packed-refs` | **Declarado** | Lista publicada |
| Plumbing `send-pack` (no ejecuta `pre-push`) | **Declarado** (confirmado por SPIKE-GRD-001). `update-ref`, `update-ref -d` y `commit-tree` **no** son un salto: pasan por `reference-transaction` | Lista publicada |
| Renombrar sobre la rama base con el backend de archivos (`branch -M x base`) (Enmienda 2026-10-04) | **Cerrado con efecto parcial**: se deniega, pero Git ya borró `x` en otra transacción y `HEAD` queda apuntando a una rama que no existe. El motivo de la denegación (`renombrado-sobre-base`, ADR-GRD-003 § 3) lleva el oid para recuperarla; la Time Machine cubre el caso por observación | Lista publicada (B) |
| Renombrar o reescribir la rama base en un repo **reftable** (`branch -m/-M`) (Enmienda 2026-10-04) | **Declarado** solo en repos reftable: no pasa por ningún hook. Mitigación después del hecho (ver Enmienda) | Lista publicada del repo |
| Canal del daemon suplantado por el entorno (`XDG_RUNTIME_DIR`, `HOME`) | **Cerrado** | Ruta del canal fijada en el dispatcher y servidor autenticado (ADR-GRD-003 § 4, SEC-GRD-16) |
| `GIT_DIR`/`GIT_WORK_TREE` cruzados hacia otro repo | **Cerrado** | Directorio común fijado en el dispatcher (ADR-GRD-001 § 2, SEC-GRD-19) |
| Objetos de reemplazo o grafts que ocultan un force-push o cambian la configuración leída | **Cerrado** | § 1 y ADR-GRD-004 (SEC-GRD-17) |
| Alias de mayúsculas o Unicode de una rama protegida (`Main`, `ｍain`, formas no NFC) | **Cerrado** | § 4 (SEC-GRD-18) |

### 3. Lista publicada (BR-EDGE-003)

- **Dato versionado en el binario**: la tabla resultante, con la versión de Git y el SO en los que se verificó, en `crates/policy`, junto al catálogo.
- **Exposición**: en el estado de protección por el canal (ADR-GRD-005), como lista de códigos de operación o de salto, cada uno con su motivo: C, B, "salto voluntario declarado" o **`no-reconocible`** (interceptable, pero la política no puede reconocer la operación por la entrada del hook; Enmienda 2026-10-04). El cliente traduce el texto (NFR-10).
- **Depende del repo** (Enmienda 2026-10-04): la lista se calcula por SO, versión de Git y **backend de refs** (`extensions.refStorage`). El backend se comprueba al instalar y en cada comprobación de estado, porque puede cambiar después (`git refs migrate`).
- **Contenido verificado en macOS por SPIKE-GRD-001** (Linux y Windows pendientes):
  - **No impedibles**: `reset --hard`, merge (también fast-forward), borrar worktree, crear worktree sin rama nueva (`no-reconocible`) y, **solo en repos reftable**, renombrar o reescribir la rama base con `branch -m/-M`.
  - **Saltos declarados**: los del § 2.
  - **Se deniega de más** (apartado propio; Enmienda 2026-10-04): el push fast-forward desde un clon superficial (F07) y, en repos reftable, una rama que solo difiere de la base en mayúsculas (D21), por la regla de ambigüedad.
- **Coherencia**: la regresión de INF-GRD-001 ejecuta cada operación y cada salto con Git crudo en los tres SO, y comprueba que el resultado coincide con la lista (US-GRD-004).

### 4. Refs gobernadas, entrada y normalización (M-01; H-06)

- **Refs no gobernadas = lista explícita** de prefijos y pseudo-refs. **Cualquier otra ref es gobernada.**
  - Prefijos: `refs/remotes/`, `refs/tags/`, `refs/notes/`, `refs/stash`, `refs/bisect/`, `refs/rewritten/`, `refs/prefetch/`.
  - Pseudo-refs (Enmienda 2026-10-04): **regla de gitglossary(7)** en lugar de una lista cerrada: todo nombre de una sola componente en mayúsculas, fuera de `refs/`, salvo `HEAD` (así entran `ORIG_HEAD`, `FETCH_HEAD`, `AUTO_MERGE`, `CHERRY_PICK_HEAD`, `REBASE_HEAD`, `MERGE_HEAD`, `REVERT_HEAD` y `BISECT_HEAD`), también en sus formas por worktree (`main-worktree/<PSEUDO>`, `worktrees/<id>/<PSEUDO>`); y las actualizaciones de un `HEAD` separado.
  - **Cuidado con `refs/remotes/`**: que sus actualizaciones no pasen por el hook no las hace inofensivas para la configuración. ADR-GRD-004 § 4 trata su forja.
- **Normalización antes de clasificar**:
  - `HEAD`, `main-worktree/HEAD` y `worktrees/<id>/HEAD`, cuando son simbólicos, se resuelven a la rama a la que apuntan. **Imprescindible en Git 2.38**, que entrega un borrado a través de `HEAD` como `HEAD` (D09). La resolución toma el destino de la línea `ref:` si existe y, si no, el `HEAD` del `GIT_DIR` de la transacción, **nunca el del cwd**: al crear un worktree, la línea `HEAD` del worktree nuevo llega con el cwd del principal y no es una actualización del `HEAD` del principal (Enmienda 2026-10-04).
  - Las refs simbólicas se resuelven a su destino.
  - Los nombres de rama se comparan en **forma NFC** y, si el sistema de archivos del repo no distingue mayúsculas, **plegados a minúsculas**. Si un nombre normalizado coincide con una rama protegida o con la rama base sin ser idéntico, la decisión es **deny** por ambigüedad (SEC-GRD-18). **Se aplica a todas las líneas** de `pre-push` y de `reference-transaction` (creaciones y actualizaciones, no solo borrados): `branch -f Main x` reescribe `main` en APFS (D21b) y un push a `refs/heads/Main` llega como creación (F11). NFC es necesario: sin `core.precomposeUnicode`, Git borra la rama NFC con su nombre NFD (D19b). En repos reftable el plegado se mantiene y produce falsos positivos asumidos (§ 3) (Enmienda 2026-10-04).
- **Estados de `reference-transaction`** (Enmienda 2026-10-04): **se evalúa solo en `prepared`**. Cualquier otro estado (`preparing`, desde Git 2.54; `committed`; `aborted`; y cualquier estado futuro) sale en el dispatcher sin evaluar, o solo encadena si hay hook previo (ADR-GRD-001 § 2).
- **Forma de la entrada** (Enmienda 2026-10-04): se acepta `ref:<destino>` como valor viejo o nuevo (refs simbólicas, Git 2.4x en adelante). El borrado se detecta **solo por el valor nuevo cero**: el viejo puede ser cero, y un borrado produce dos invocaciones `prepared` (la de `packed-refs` y la de la ref suelta).
- ***Prune* de `pack-refs`** (Enmienda 2026-10-04; SPIKE-GRD-001 § 3.3): con el backend de archivos, una línea `viejo 0 ref` **no** es un borrado si `viejo ≠ 0`, el archivo suelto `<common>/<ref>` contiene `viejo` y `packed-refs` contiene exactamente `viejo ref`. Sin esta excepción, Guardrails rompe `gc`, `gc --auto` y `maintenance`. Un borrado explícito siempre emite antes la transacción de `packed-refs` con `0 0 ref`, que sí se deniega. La excepción se aplica en la **vía rápida del binario**, sin contactar con el daemon; el código fijo del `sh` la replica solo como respaldo (ADR-GRD-001 § 3).
- **Entrada estricta, en streaming**:
  - Se lee **toda** la entrada estándar.
  - Cada línea se valida: formato, longitud del identificador de objeto y `check-ref-format`. Solo hay tope por línea.
  - Una línea que supera el tope o es malformada hace que la transacción se trate **como gobernada** y se evalúe en el daemon (fail-closed si no se puede evaluar). Nunca se trata por la vía rápida (SEC-GRD-07).
- **Vía rápida**: si todas las líneas son de refs no gobernadas, o son *prunes* de `pack-refs`, `raptor hook` sale con 0 sin contactar con el daemon. En cualquier estado distinto de `prepared` el dispatcher sale sin lanzar procesos si no hay hook previo (ADR-GRD-001 § 2, L-01).
- **Operación normalizada**: el cliente del hook traduce el hook y su entrada a la operación del catálogo y a sus **transiciones exactas** (ref, valor viejo, valor nuevo), y la envía a ADR-GRD-003.
- **Hechos de contenido** (rutas, líneas, mensaje): solo se calculan si una política activa los necesita. Las lecturas ignoran los objetos de reemplazo.

### 5. Presupuesto

- **< 100 ms p95 por evaluación gobernada**, desde que arranca el dispatcher hasta que sale, con el daemon en marcha y en un repo mediano. ⚠️ **ASSUMPTION**: p95, como NFR-04 de motor-local.
- **Vía rápida** (refs no gobernadas): ⚠️ **ASSUMPTION**: < 30 ms p95 en macOS y Linux. En Windows el objetivo lo fija SPIKE-GRD-001.
- **Fuera del presupuesto**: el arranque del daemon bajo demanda, la espera de la cola (US-GRD-015) y el snapshot previo vía hook (US-GRD-017).
- **Si SPIKE-GRD-001 no lo cumple en un SO**, se revisa ADR-GRD-001: dispatcher nativo, o sin `reference-transaction` en ese SO, con el borrado local de ramas declarado como no impedible allí.
- **Coste por comando** (Enmienda 2026-10-04; SPIKE-GRD-001 § 8): el presupuesto por evaluación se cumple en macOS (vía rápida en `sh`: unos 8–11 ms p50, Δp95 < 20 ms), pero un comando lanza varios procesos de hook (commit en Git 2.56 con el conjunto completo: 12; `rebase` de 3 commits: 91), y el coste es el número de procesos. Por eso hay dos ejes:
  - **Por evaluación**: los objetivos de arriba, sin cambios.
  - **Por comando**: coste = evaluaciones gobernadas × su presupuesto (< 100 ms p95) + invocaciones que no evalúan × coste de la vía mínima. ⚠️ **ASSUMPTION**: ≤ 5 ms p95 por invocación que no evalúa con el dispatcher nativo (ADR-GRD-001 § 2). **Techo visible para el usuario** (PO; context.md § 6, Q-GRD-30): un commit o un cambio de rama habitual añade ⚠️ **ASSUMPTION** ≤ 150 ms p95 en total (una evaluación más unos 7 procesos), pendiente de que Rene Bonilla lo confirme. **Gate de CI determinista**: número de procesos de hook por comando (commit, `switch`, `rebase`, `fetch`, `stash`) y versión de Git, contra una tabla de referencia en INF-GRD-001. La latencia absoluta se mide en un runner en reposo, no en un portátil (el mismo `fetch` dio 17 s y 28 s).
  - **Operaciones masivas** (una transacción por ref: `fetch` sin `--atomic` en Git 2.38 y 2.50; `pack-refs` de `gc --auto` y de la maintenance con muchas refs sueltas, en todas las versiones): coste lineal en el número de refs (+17 a +39 s por 1.000 refs en macOS con `sh`). **No hay mitigación dentro de la capa de hooks**, porque Git lanza el hook por transacción. **Decisión del orquestador (2026-10-04), validada por Arquitecto y PO**: se declara (documentación y explicación del permiso), con una pendiente fija: coste por transacción ≤ una invocación de la vía mínima (sin binario completo ni daemon para refs no gobernadas y *prunes*); el dispatcher nativo divide el coste por invocación entre 2 y 3. No se quita `reference-transaction` (es la única línea del borrado local de la rama base) ni se crea un estado nuevo: como mucho, un aviso informativo. Posterior al MVP: que el daemon empaquete las refs sin hooks (ampliaría la capa de escritura y necesita su propio ADR).

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| Solo los hooks que cita el BRD (`pre-commit`, `pre-push`, `reference-transaction`) | El rebase no tendría hook antes de sus efectos, y el formato de commit no tendría `commit-msg` |
| `reference-transaction` como único hook | No ve el force-push ni el borrado remoto, y en merge y reset llega tarde |
| Refs gobernadas = lista explícita (solo `refs/heads/*`) | Cualquier namespace nuevo, o una ref que se normaliza a una rama, escaparía por la vía rápida (M-01) |
| Respetar los objetos de reemplazo, como hace Git por defecto | Un `git replace --graft` haría pasar un push que reescribe `main` sin `--force` (H-05) |
| Comparar los nombres de rama byte a byte | `Main` en macOS o Windows, o una forma Unicode no NFC, se saltaría la protección (H-06) |
| Publicar la lista como texto en la documentación | No se puede verificar y se desincroniza con las versiones de Git |
| Tratar el momento B como impedido | El estado de protección mentiría (BR-WF-002) |
| Envoltorio de `git` en el PATH | Lo controla el agente y modifica la máquina fuera del repo (Q17) |

## Consecuencias

- ✅ El escenario central del MVP (force-push y borrado de la rama base, US-GRD-001) se resuelve en el momento A. Además, ni un graft, ni un alias de mayúsculas, ni un canal suplantado lo saltan.
- ✅ La lista publicada es un dato que la CI comprueba e incluye los saltos cerrados y los declarados.
- ✅ Una entrada anómala nunca toma la vía rápida.
- ⚠️ **`reset --hard`, merge y borrar worktree no se impiden con Git crudo.** Es el límite declarado (Q-GRD-8, R-GRD-1); la mitigación es la Time Machine.
- ⚠️ **`git push --no-verify --force` salta la protección de force-push**: se declara en la lista (R-GRD-1).
- ⚠️ **Coste**: con "todo lo demás gobernado", los namespaces poco comunes van al daemon. SPIKE-GRD-001 mide su frecuencia.
- ⚠️ Varias filas eran hipótesis. SPIKE-GRD-001 las confirmó o corrigió en macOS (Enmienda 2026-10-04); Linux y Windows siguen pendientes antes de cerrar las Dev Specs de US-GRD-002 y US-GRD-004.
- ⚠️ **Reftable** (Enmienda 2026-10-04): en esos repos, renombrar o reescribir la rama base con `branch -m/-M` no se puede impedir. Se declara y se mitiga después del hecho. El daño es local y recuperable: `pre-push` sigue impidiendo borrar o forzar la rama base en el remoto.
- ⚠️ **Coste por comando** (Enmienda 2026-10-04): las operaciones masivas tienen un coste lineal declarado y el dispatcher nativo pasa a ser necesario para `reference-transaction` desde Git 2.54 (ADR-GRD-001 § 2).

## Validación

1. **Matriz**: por cada fila, en los tres SO y con Git 2.38 y la última estable, ejecutar la operación con Git crudo y comprobar el momento y el resultado (SPIKE-GRD-001; luego la regresión de INF-GRD-001).
2. **Saltos**: cada salto del § 2 se comporta como declara la lista. Los cerrados se rechazan.
3. **Force-push**: un push forzado sobre una rama propia, sobre la rama base, con `--force-with-lease`, con un objeto remoto ausente, en un clon superficial y **tras un `git replace --graft` que hace parecer fast-forward un push que reescribe `main`**: todos se detectan como forzados (US-GRD-001, H-05).
4. **Borrado de la rama base y alias**:
   - Con `git branch -D main`, `git update-ref -d refs/heads/main` y `git push origin :main`, se deniega.
   - Con `--no-verify`, el borrado local sigue denegado.
   - En macOS y Windows, `git branch -D Main` y una variante Unicode no NFC de `main` también se deniegan (H-06).
5. **Normalización**: un borrado a través de `HEAD` simbólico, de `worktrees/<id>/HEAD` y de una ref simbólica que apunta a `refs/heads/main` se deniega.
6. **Entrada**: una línea de 10 MB, un identificador inválido y una ref de un namespace desconocido van al daemon y nunca salen por la vía rápida.
7. **Vía rápida**: un `fetch` de 1.000 refs y un `tag` no contactan con el daemon y cumplen el presupuesto.
8. **Presupuesto**: p95 < 100 ms por evaluación gobernada en los tres SO (INF-GRD-001).
9. **Lista expuesta**: el estado de protección devuelve la lista y coincide con los puntos 1 y 2 (US-GRD-004).
10. ***Prune* de `pack-refs`** (Enmienda 2026-10-04): `pack-refs --all` y `gc` con la rama base suelta se permiten sin contactar con el daemon; `branch -D` y `update-ref -d` de la base, suelta, empaquetada o ambas, se deniegan (SPIKE-GRD-001 D06, D07, D12–D14).
11. **Estados y entrada** (Enmienda 2026-10-04): con Git ≥ 2.54, `preparing` no evalúa; `ref:<destino>` no es una línea malformada; `CHERRY_PICK_HEAD` y `REBASE_HEAD` no van al daemon durante un rebase; la línea `HEAD` de `git worktree add` no se resuelve contra el cwd.
12. **Reftable** (Enmienda 2026-10-04): en cada versión de Git de la matriz con reftable, `branch -m main x` y `branch -M feat main`; mientras pasen sin hook, la fila sigue en la lista del repo reftable; si Git lo corrige, la regresión falla y la fila se retira.
13. **Coste por comando** (Enmienda 2026-10-04): el número de procesos de hook por comando y versión coincide con la tabla de referencia de INF-GRD-001.

## Referencias

- **Reglas**: BR-EDGE-003, BR-VAL-002, BR-CONS-002, BR-WF-002; Q-GRD-8; R-GRD-1.
- **Historias**: US-GRD-001, US-GRD-004, US-GRD-007, US-GRD-016, US-GRD-017.
- **ADRs de otros frentes**: ADR-GRP-011 (criterio p95, motor-local, en `main`); ADR-TMC-004 § 3 (snapshot `previo_hook`, time-machine, en `main`).
- **Enablers**: SPIKE-GRD-001, INF-GRD-001.
- **Seguridad**: SEC-GRD-07, 16, 17, 18, 19.
- **Git**: githooks(5), git-replace(1), gitglossary(7) (pseudo-refs).

## Revisión de seguridad (2026-10-04)

| Hallazgo | Cómo se cubre |
|---|---|
| H-05 · `replace --graft` oculta un force-push | § 1 (fila force-push): sin objetos de reemplazo ni grafts y sin fiarse del commit-graph; ausente o superficial = forzado; Validación 3 |
| H-06 · Alias de mayúsculas o Unicode | § 4: NFC y plegado de mayúsculas; deny ante ambigüedad; Validación 4; caso en SPIKE-GRD-001 |
| M-01 · Refs gobernadas y entrada | § 4: lista explícita de no gobernadas (todo lo demás es gobernado), parser en streaming con tope por línea, tope superado = gobernada, normalización de refs simbólicas y de `HEAD`; Validación 5 y 6 |
| I-01 · Saltos cerrados y declarados | § 2: tabla incorporada a la lista publicada y a R-GRD-1 |
| L-01 · Coste de `committed`/`aborted` | § 4: salida temprana del dispatcher (ADR-GRD-001 § 2) |
| J13 · Referencias rotas en el frontmatter | `related` solo con IDs existentes |

## Enmienda (2026-10-04, SPIKE-GRD-001)

Aplicada desde las recomendaciones de [SPIKE-GRD-001-resultados.md](../../requirements/features/guardrails/research/SPIKE-GRD-001-resultados.md) (§ 9), que se midieron **solo en macOS** (Git 2.38.5, 2.50.1 y 2.56.0, esta también con reftable). El `status` sigue en `accepted`. Linux, Windows y el coste en Windows siguen pendientes de la Validación y bloquean el merge de US-GRD-001. Cada resolución es una **decisión del orquestador (2026-10-04), validada por el Arquitecto y el PO**.

| Enmienda | Resolución | Dónde |
|---|---|---|
| E-02-1 · Matriz | Aceptada. Crear worktree sin rama nueva: A efectiva (Git revierte), pero se publica como **no impedible** con el motivo nuevo `no-reconocible`; se revisa cuando una política necesite reconocer esa operación. Merge fast-forward: B. `pull --rebase`: A por `pre-rebase`. `send-pack`: salto declarado | § 1, § 2, § 3 |
| E-02-2 · Estados | Aceptada: solo `prepared` evalúa; `preparing`, `committed`, `aborted` y cualquier estado futuro salen o solo encadenan | § 4 |
| E-02-3 · Entrada | Aceptada: `ref:<destino>`, borrado solo por valor nuevo cero, `HEAD` desde la línea `ref:` o el `GIT_DIR` de la transacción, nunca el cwd | § 4 |
| E-02-4 · Reftable | **Aceptado como límite conocido** (ver abajo) | § 1, § 2, § 3, Consecuencias, Validación 12 |
| E-02-5 · *Prune* de `pack-refs` | Aceptada, con ajuste del Arquitecto: la excepción vive en la vía rápida del **binario**, sin daemon; en `sh` solo como respaldo, porque releer `packed-refs` en `sh` por transacción crece al cuadrado con 1.000 refs | § 4, Validación 10 |
| E-02-6 · Pseudo-refs | Aceptada con ajuste: regla de gitglossary, también en las formas `main-worktree/` y `worktrees/<id>/` | § 4 |
| E-02-7 · Saltos nuevos | Aceptada: `branch -M x base` con archivos se cierra con efecto parcial (B), declarado; el motivo `renombrado-sobre-base` lleva el oid de la rama origen para recuperarla (es recuperación, no una vía de excepción: SEC-GRD-06 lo permite). Con reftable, ver abajo | § 2; ADR-GRD-003 (Enmienda) |
| E-02-8 · Clon superficial | Aceptada: falso positivo asumido (fail-closed), publicado en el apartado "se deniega de más" de la lista, con el motivo `historia-superficial`; sugerir `git fetch --unshallow` está permitido | § 1, § 3; ADR-GRD-003 (Enmienda) |
| E-02-9 · Presupuesto | Aceptada con ajuste: dos ejes (por evaluación y por comando). El objetivo fijo de +50 ms por comando que se barajó **se descarta** (con Git 2.56, `reference-transaction` solo ya lanza 7 procesos en un commit). Se adopta la fórmula, el gate de procesos por comando y la latencia en un runner en reposo, más el techo de ≤ 150 ms p95 por commit o cambio de rama habitual que pide el PO (⚠️ ASSUMPTION, por confirmar por Rene) | § 5, Validación 13 |
| E-02-10 · Alias | Aceptada: plegado y NFC en todas las líneas; falsos positivos en reftable asumidos y publicados | § 3, § 4 |
| Coste de las transacciones masivas (17–39 s / 1.000 refs) | Declarado, con una pendiente fija (≤ una invocación de la vía mínima por transacción), vía rápida en el binario y dispatcher nativo. Sin quitar `reference-transaction`. El "infrecuente" no se asume: los usuarios objetivo tienen muchas ramas de agentes | § 5 |

**Reftable: decisión.** Con `extensions.refStorage=reftable` (Git ≥ 2.45), `git branch -m main x` y `git branch -M feat main` no ejecutan ningún hook (SPIKE-GRD-001 D10, D11). Se descartan "no se instala" (privaría al repo de la protección contra el force-push y el borrado, que sí funcionan) y reportarlo aguas arriba como solución (puede hacerse, pero no cierra el hueco en las versiones de Git soportadas; queda fuera de esta enmienda). La decisión:

1. **Límite conocido de la capa de hooks**, publicado en la lista de ese repo (US-GRD-004) como no impedible, solo en repos reftable, y dicho en la explicación del permiso al instalar (US-GRD-001). El backend se vuelve a comprobar en cada comprobación de estado (ADR-GRD-005, Enmienda).
2. **Mitigación después del hecho**:
   - **MVP**: en `branch -m main x`, los commits siguen en `x` y el motor ya indica que la rama base no existe (Q42 de motor-local). En `branch -M feat main`, la base se **reescribe**: la Time Machine ancla los commits por observación (ADR-TMC-004, nivel b).
   - **Pendiente en otros frentes** (tabla de [non-functional-guardrails.md](../non-functional-guardrails.md)): comprobar que el observador (ADR-GRP-010) vigila las refs de `reftable/` y que el nivel (b) de ADR-TMC-004 dispara con esos eventos. Si no lo hacen, no hay detección ni mitigación.
   - **Posterior al MVP** (backlog, Could, PO): detección propia de Guardrails de la rama base que desaparece o se mueve sin fast-forward, aviso con causa en el Cockpit y recuperación guiada con la Time Machine.
3. **Regresión**: INF-GRD-001 prueba el renombrado con reftable en cada versión de Git, para retirar la fila si Git lo corrige (Validación 12).

Ajuste respecto a la recomendación del coordinador: el aviso en el Cockpit y la recuperación guiada pasan a después del MVP (PO), y el detector no es solo Q42 porque `-M feat main` reescribe la base sin hacerla desaparecer (Arquitecto).

## Enmienda (2026-10-05, US-GRD-001)

**Decisión del orquestador (2026-10-05), validada por Arquitecto.** El `status` sigue en `accepted`.

- **Vía rápida en el dispatcher** (§ 4): las refs no gobernadas, las pseudo-refs, los valores `ref:` de `HEAD` y los *prunes* de `pack-refs` se permiten en el propio dispatcher nativo, sin arrancar `raptor` (pendiente fija del § 5: una invocación de la vía mínima por transacción). La clasificación es un único archivo solo-`std` que compilan `crates/policy` y el dispatcher. Lo que no entiende del todo (una línea malformada, un `HEAD` que hay que resolver) siempre va a `raptor hook`, que valida de forma estricta.
- **`HEAD` creado** (§ 4): una línea de `HEAD` con valor viejo cero es el `HEAD` de un worktree nuevo, no una actualización del principal; no es gobernada (motivo `no-reconocible`).
- **Coste en Windows** (§ 5, medido el 2026-10-05, § 14 de los resultados del spike): con el dispatcher nativo, ≈ 6 ms por invocación que no evalúa (commit con el conjunto mínimo +40 ms p50); el `fetch` de 1.000 refs nuevas añade ≈ +13 s (con `sh`, +130 s). El objetivo ⚠️ ≤ 5 ms p95 por invocación queda algo por encima en esa máquina y se declara.
- **Latencia en macOS** (US-GRD-001, informe): commit +36 ms p50 / +41 ms p95; evaluación gobernada +26 / +31 ms; vía rápida +12 / +14 ms (portátil, no en reposo).

## Enmienda (2026-10-06, XP-30)

**Decisión del orquestador (2026-10-06), validada por Arquitecto.** El `status` sigue en `accepted`. La garantía a los usuarios no cambia: el renombrado sobre la base en un repo reftable sigue publicado como no impedible (`RenameBaseReftable`, Q-GRD-28), así que no hace falta pasar por el PO.

- **Renombrar sobre la base con reftable** (`branch -M feat main`, § 1 y § 3; SPIKE-GRD-001 D11): **no es un cambio de Git 2.56**. Con 2.50.1 (macOS), 2.55.0 (CI) y 2.56.0 (contenedor Linux), el renombrado (borrar `feat` y reescribir `main`) **no ejecuta ningún hook**. Después solo corre una transacción, la del symref `0000… ref:refs/heads/main HEAD`, cuando `main` ya está reescrita y `feat` borrada. El producto nunca la evalúa (vía rápida de los valores `ref:`, Enmienda 2026-10-05), así que para Guardrails el caso sigue siendo C, como dice el § 1.
- **El veredicto del ejecutor de INF-GRD-001 depende de su sonda**: deniega cualquier entrada que contenga `refs/heads/main`, también esa línea de `HEAD`, y por eso mide **B** en todas esas versiones. La guarda del spike solo denegaba borrados de `main`, no denegó esa línea y midió C con 2.56.0. La fila "C para ≥ 2.56.0" se heredó de ahí. La lista de referencia del ejecutor tiene ahora **una sola fila** reftable para `rename-over-base`: B desde 2.50.1 en adelante. Si la sonda cambia (por ejemplo, para comparar solo el nombre de la ref), la fila deja de valer y hay que volver a medir.
- **Procesos de hook por comando** (§ 5, Validación 13): la tabla de referencia añade filas exactas medidas en el contenedor Linux: 2.56.0, idéntica a 2.55.0, y 2.38.5 y 2.43.0, idénticas entre sí, sin las transacciones de symref ni `preparing`. Se siguen usando versiones exactas para que una actualización del runner nunca cambie el gate en silencio.
- **Pruebas de extremo a extremo con la matriz**: el daemon resuelve Git con un `PATH` fijo, así que en el contenedor usaba el Git de la distro y no el de la etapa de la matriz. En las compilaciones de depuración, `GITRAPTOR_TEST_GIT` fija el único Git que el daemon puede resolver, de forma estricta y sin otros candidatos. Release no lo lee (SEC-06).

## Enmienda (2026-10-06, US-GRD-018)

Fila **Commit** del § 1 para la política de autoría (BR-AUTH-005; [DS-US-GRD-018](../../requirements/features/guardrails/dev-specs/US-GRD-018-autoria-commits-persona-y-agente.md), § 5.3). **Decisión del orquestador (2026-10-06), validada por el Arquitecto.** El `status` sigue en `accepted`.

- `pre-commit` corta `human-author` con `deny` antes del mensaje; `commit-msg` evalúa los trailers del mensaje (también en el commit de fusión de `merge`, donde Git lo ejecuta).
- La segunda línea (`reference-transaction` `prepared`) evalúa, con actor agente, todo commit nuevo único con forma de commit, fusión o amend, salvo que el `git` antecesor sea con certeza `rebase`, `cherry-pick`, `revert` o `am`. Así cubre `--no-verify`, los alias y `commit-tree` + `update-ref`.
- **Residuo declarado**: un agente que escribe con `update-ref` un rango de varios commits nuevos escapa a la segunda línea. Rebase, cherry-pick, revert y `am` no se evalúan por autoría: los gobiernan `permissions` y las demás reglas.
