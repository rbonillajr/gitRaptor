---
id: ADR-GRD-002
title: Operaciones interceptables y límites de la capa de hooks
type: adr
status: proposed
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRD-001, ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, CTX-GRD-001, BR-GRD-001]
tags: [guardrails, hooks-git, reference-transaction, pre-push, pre-rebase, interceptabilidad, br-edge-003, no-verify, presupuesto-latencia, refs-reemplazo, normalizacion-refs]
---

# ADR-GRD-002 — Operaciones interceptables y límites de la capa de hooks

## Contexto

BR-EDGE-003 y Q-GRD-8 exigen **declarar** qué operaciones del catálogo (BR-VAL-002: commit, push, force-push, `reset --hard`, borrar rama, rebase, merge, crear worktree y borrar worktree) no puede impedir la capa de hooks con Git crudo, y mostrarlo en el estado de protección (US-GRD-004). La lista tiene que coincidir con lo que se observa al probar esas operaciones. US-GRD-007 cubre por Git directo solo las operaciones que esa lista declara interceptables.

Hay otros tres requisitos:

- **Momento de la evaluación**: una operación cuenta como **impedida** solo si se evalúa **antes de cualquier efecto**.
- **Latencia**: la evaluación añade < 100 ms en repos medianos (contexto § 6, confirmado por Rene Bonilla el 2026-10-04).
- **Saltos voluntarios**: Git permite saltarse los hooks a propósito. Es el riesgo R-GRD-1, aceptado.

## Decisión

**La capa de hooks gobierna cada operación con el hook que corre antes de sus efectos, y usa `reference-transaction` en estado `prepared` como segunda línea, que `--no-verify` no salta. Toda ref que no esté en una lista explícita de excepciones es gobernada. Las lecturas en las que se basa la decisión ignoran los objetos de reemplazo. La lista publicada de lo que no se puede impedir es un dato versionado en el binario, verificado por SPIKE-GRD-001 y por la regresión de INF-GRD-001.**

### 1. Matriz (hipótesis que confirma SPIKE-GRD-001)

**Momento**:

- **A**: el hook corre antes de cualquier efecto. La operación queda impedida.
- **B**: el hook corre después de efectos parciales en el working tree o el índice. Protege la ref, pero la operación no cuenta como impedida.
- **C**: no hay hook antes del efecto.

| Operación (BR-VAL-002) | Hook principal | Segunda línea | Momento | Se publica como | Salto voluntario conocido |
|---|---|---|---|---|---|
| **Commit** | `pre-commit` (contenido), `commit-msg` (formato) | `reference-transaction` `prepared` sobre `refs/heads/*`: evalúa el rango `viejo..nuevo` | A | Impedible | `--no-verify` salta el principal, no la segunda línea |
| **Push** | `pre-push` (refs y objetos por la entrada estándar) | — | A | Impedible | `--no-verify`; plumbing `send-pack` (por confirmar) |
| **Force-push** | `pre-push`: una ref es forzada si el objeto remoto no es ancestro del local **sin objetos de reemplazo ni grafts y sin fiarse del commit-graph para los padres**, o si el objeto remoto falta o la historia es superficial (H-05) | — | A | Impedible | `--no-verify` |
| **Borrar rama (local)** | `reference-transaction` `prepared`, con valor nuevo igual a ceros sobre `refs/heads/*` | — | A | Impedible | `--no-verify` no la salta |
| **Borrar rama (remota)** | `pre-push` con el objeto local igual a ceros | — | A | Impedible | `--no-verify` |
| **`reset --hard`** | Ninguno antes de reescribir el working tree | `reference-transaction` si mueve la rama | C / B | **No impedible**. Mitigación: Time Machine | — |
| **Rebase** | `pre-rebase` | `reference-transaction` al mover la rama al final | A (principal) | Impedible | `--no-verify` salta el principal; `pull --rebase` (por confirmar) |
| **Merge** | `pre-merge-commit` (después de escribir el resultado en el working tree) | `reference-transaction` | B | **No impedible**: la rama destino no cambia, pero queda una fusión en curso | `--no-verify` en el principal |
| **Crear worktree** | Con rama nueva: `reference-transaction` al crear la rama (por confirmar). Sin rama nueva: después de crear sus archivos de administración | `post-checkout` (después; solo informa) | A con rama nueva / B o C sin ella | Con rama nueva, impedible; sin ella, **no impedible** (hipótesis) | — |
| **Borrar worktree** | Ninguno | — | C | **No impedible** | — |

### 2. Saltos: cerrados y declarados (I-01)

| Salto | Estado | Dónde |
|---|---|---|
| `--no-verify` en `pre-commit`, `pre-push`, `pre-rebase` y `pre-merge-commit` | **Declarado** | Lista publicada (§ 3) |
| `-c core.hooksPath=…`, `GIT_CONFIG_COUNT/KEY/VALUE`, `GIT_CONFIG_PARAMETERS` en un solo comando | **Declarado** (no detectable) | Lista publicada |
| Editar `.git/config`, o borrar o alterar los dispatchers | **Detectado** como pérdida | ADR-GRD-005 |
| Escribir a mano en `refs/` o en `packed-refs` | **Declarado** | Lista publicada |
| Plumbing (`send-pack`, `update-ref` fuera de una transacción) | **Declarado** según SPIKE-GRD-001 | Lista publicada |
| Canal del daemon suplantado por el entorno (`XDG_RUNTIME_DIR`, `HOME`) | **Cerrado** | Ruta del canal fijada en el dispatcher y servidor autenticado (ADR-GRD-003 § 4, SEC-GRD-16) |
| `GIT_DIR`/`GIT_WORK_TREE` cruzados hacia otro repo | **Cerrado** | Directorio común fijado en el dispatcher (ADR-GRD-001 § 2, SEC-GRD-19) |
| Objetos de reemplazo o grafts que ocultan un force-push o cambian la configuración leída | **Cerrado** | § 1 y ADR-GRD-004 (SEC-GRD-17) |
| Alias de mayúsculas o Unicode de una rama protegida (`Main`, `ｍain`, formas no NFC) | **Cerrado** | § 4 (SEC-GRD-18) |

### 3. Lista publicada (BR-EDGE-003)

- **Dato versionado en el binario**: la tabla resultante, con la versión de Git y el SO en los que se verificó, en `crates/policy`, junto al catálogo.
- **Exposición**: en el estado de protección por el canal (ADR-GRD-005), como lista de códigos de operación o de salto, cada uno con su motivo (C, B, "salto voluntario declarado"). El cliente traduce el texto (NFR-10).
- **Contenido de partida, sujeto a SPIKE-GRD-001**:
  - **No impedibles**: `reset --hard`, merge, borrar worktree y crear worktree sin rama nueva.
  - **Saltos declarados**: los del § 2.
- **Coherencia**: la regresión de INF-GRD-001 ejecuta cada operación y cada salto con Git crudo en los tres SO, y comprueba que el resultado coincide con la lista (US-GRD-004).

### 4. Refs gobernadas, entrada y normalización (M-01; H-06)

- **Refs no gobernadas = lista explícita** de prefijos y pseudo-refs. **Cualquier otra ref es gobernada.**
  - Prefijos: `refs/remotes/`, `refs/tags/`, `refs/notes/`, `refs/stash`, `refs/bisect/`, `refs/rewritten/`, `refs/prefetch/`.
  - Pseudo-refs: `ORIG_HEAD`, `FETCH_HEAD`, `AUTO_MERGE` y las actualizaciones de un `HEAD` separado.
  - **Cuidado con `refs/remotes/`**: que sus actualizaciones no pasen por el hook no las hace inofensivas para la configuración. ADR-GRD-004 § 4 trata su forja.
- **Normalización antes de clasificar**:
  - `HEAD`, `main-worktree/HEAD` y `worktrees/<id>/HEAD`, cuando son simbólicos, se resuelven a la rama a la que apuntan.
  - Las refs simbólicas se resuelven a su destino.
  - Los nombres de rama se comparan en **forma NFC** y, si el sistema de archivos del repo no distingue mayúsculas, **plegados a minúsculas**. Si un nombre normalizado coincide con una rama protegida o con la rama base sin ser idéntico, la decisión es **deny** por ambigüedad (SEC-GRD-18).
- **Entrada estricta, en streaming**:
  - Se lee **toda** la entrada estándar.
  - Cada línea se valida: formato, longitud del identificador de objeto y `check-ref-format`. Solo hay tope por línea.
  - Una línea que supera el tope o es malformada hace que la transacción se trate **como gobernada** y se evalúe en el daemon (fail-closed si no se puede evaluar). Nunca se trata por la vía rápida (SEC-GRD-07).
- **Vía rápida**: si todas las líneas son de refs no gobernadas, `raptor hook` sale con 0 sin contactar con el daemon. En `committed` y `aborted` el dispatcher sale sin lanzar procesos si no hay hook previo (ADR-GRD-001 § 2, L-01).
- **Operación normalizada**: el cliente del hook traduce el hook y su entrada a la operación del catálogo y a sus **transiciones exactas** (ref, valor viejo, valor nuevo), y la envía a ADR-GRD-003.
- **Hechos de contenido** (rutas, líneas, mensaje): solo se calculan si una política activa los necesita. Las lecturas ignoran los objetos de reemplazo.

### 5. Presupuesto

- **< 100 ms p95 por evaluación gobernada**, desde que arranca el dispatcher hasta que sale, con el daemon en marcha y en un repo mediano. ⚠️ **ASSUMPTION**: p95, como NFR-04 de motor-local.
- **Vía rápida** (refs no gobernadas): ⚠️ **ASSUMPTION**: < 30 ms p95 en macOS y Linux. En Windows el objetivo lo fija SPIKE-GRD-001.
- **Fuera del presupuesto**: el arranque del daemon bajo demanda, la espera de la cola (US-GRD-015) y el snapshot previo vía hook (US-GRD-017).
- **Si SPIKE-GRD-001 no lo cumple en un SO**, se revisa ADR-GRD-001: dispatcher nativo, o sin `reference-transaction` en ese SO, con el borrado local de ramas declarado como no impedible allí.

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
- ⚠️ Varias filas son hipótesis. **Mitigación**: SPIKE-GRD-001 las confirma antes de cerrar las Dev Specs de US-GRD-002 y US-GRD-004.

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
