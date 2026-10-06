---
id: SPIKE-GRD-001-RES
title: "Resultados de SPIKE-GRD-001: interceptabilidad, coexistencia y coste de la capa de hooks (macOS)"
type: research
status: done
feature: guardrails
domain: GRP
spike: SPIKE-GRD-001
created: 2026-10-04
updated: 2026-10-05
related:
  adrs: [ADR-GRD-001, ADR-GRD-002]
  stories: [US-GRD-001, US-GRD-002, US-GRD-004]
  spikes: [SPIKE-GRD-001]
tags: [guardrails, spike, hooks-git, reference-transaction, pre-push, force-push, rama-base, husky, lefthook, pre-commit, worktrees, reftable, coste]
---

# Resultados de SPIKE-GRD-001 (macOS)

> **Alcance de esta entrega**: macOS 26.6.2 (arm64, APFS sin distinción de mayúsculas), con Git **2.38.5** (mínima, NFR-07), **2.50.1** (la de Apple) y **2.56.0** (última estable). La 2.56.0 se probó además con **reftable**. **Linux y Windows quedan sin verificar** (procedimiento al final). El prototipo y la evidencia están en [`spikes/hook-interceptability/`](../../../../../spikes/hook-interceptability/). Cada afirmación remite a un caso de una suite (`01-…` a `06-…`) y a su TSV en `results/<so>-git<versión>/`.
>
> **Estado (2026-10-04): Done para macOS.** Las 17 enmiendas del § 9 y la decisión sobre reftable se **aplicaron el 2026-10-04** como secciones "Enmienda (2026-10-04, SPIKE-GRD-001)" de ADR-GRD-001 y ADR-GRD-002 (con efectos en ADR-GRD-003 y ADR-GRD-005) y como Q-GRD-28 a Q-GRD-31 en el requerimiento. La resolución de cada una está en el § 13. Linux, Windows y el coste en Windows siguen pendientes y bloquean el merge de US-GRD-001. **Actualización (2026-10-05)**: el coste en Windows está medido en una máquina Windows real (§ 14); la matriz en Linux y Windows sigue pendiente.

## 1. Veredicto

| Pregunta del SPIKE | Resultado en macOS |
|---|---|
| **Force-push y borrado de la rama base** (bloquea el merge de US-GRD-001) | **Cerrado: impedibles en el momento A**, en las tres versiones, también desde worktrees enlazados, con `--force-with-lease`, `+refspec`, `--mirror`, comodines, `push.default`, alias `Main`, objeto remoto ausente y `replace --graft`. Hay **cinco condiciones** para que el guard funcione: la excepción del *prune* de `pack-refs` (§ 3.3), la normalización de `HEAD` en 2.38, evaluar solo en `prepared` (2.54 añade `preparing`), denegar por ambigüedad cualquier alias de mayúsculas (no solo los borrados) y la normalización NFC. Hay **una excepción que no se puede cerrar con hooks**: con **reftable**, `git branch -m main otra` borra la rama base sin que corra ningún hook |
| Matriz de ADR-GRD-002 § 1 | **Confirmada en lo esencial**, con cuatro correcciones: crear worktree **sin** rama nueva sí se puede impedir (Git revierte), el merge fast-forward es B, `pull --rebase` ejecuta `pre-rebase` en A para las refs gobernadas y `send-pack` no ejecuta `pre-push` (§ 2) |
| Saltos | `--no-verify` no salta `reference-transaction` (**confirmado**). `-c core.hooksPath`, `GIT_CONFIG_COUNT/KEY/VALUE`, `send-pack` y la edición a mano de `refs/` saltan todo (**confirmado**). Aparecen dos saltos nuevos: renombrar sobre la rama base y renombrar con reftable (§ 4) |
| Coexistencia con gestores | Las hipótesis de husky y pre-commit, **confirmadas**. La de lefthook, **corregida**: lefthook 2.x **se niega a instalar** con `core.hooksPath` definido. Solo escribe en la carpeta de Guardrails con `--force`, y `--reset-hooks-path` borra la clave (§ 6) |
| Cobertura de worktrees | **Confirmada** con la ruta absoluta. Una ruta relativa, `config.worktree`, `include` e `includeIf` (`gitdir:` y `onbranch:`) la rompen, como ya preveía ADR-GRD-001 § 5 (§ 5) |
| Configuración byte a byte | Es idéntica en los casos que escribe `git config`. **Difiere** con un valor previo escrito a mano (formato y comentario) y con un `config` sin salto de línea final (§ 5.1) |
| Coste | En esta máquina, **cada invocación de un dispatcher `sh` cuesta unos 8–11 ms y un binario nativo, 1–4 ms**, antes de cualquier evaluación. Un commit con el conjunto completo de dispatchers lanza entre 6 y 12 procesos (+73 a +103 ms). Las operaciones con una transacción por ref se disparan: un `fetch` de 1.000 refs nuevas (2.38 y 2.50) o el `pack-refs` de `gc --auto` con 1.000 ramas sueltas (todas las versiones) suman **+17 a +39 s**. **El presupuesto por evaluación se cumple; el coste por comando no, si se instala el conjunto completo** (§ 8) |

## 2. Matriz confirmada (ADR-GRD-002 § 1)

**Método** (`suites/01-matrix.sh`): por cada operación, en un repo temporal nuevo con un dispatcher sonda para cada nombre de hook:

1. Se registra qué hooks corren, en qué orden y qué había cambiado ya respecto al estado previo cuando corrió cada uno (r = refs locales sin `refs/remotes/`, i = índice, w = working tree, s = operación en curso o administración de worktrees).
2. Se repite la operación con el hook gobernante devolviendo 1 y se mide el efecto residual (además, R = refs del remoto).

Una celda es **A** solo si el hook que deniega corre sin ningún cambio previo **y** la operación denegada no deja efecto. La traza de cada caso está en `results/*/01-matrix-detail/<caso>.tsv`.

| Operación (BR-VAL-002) | Hook gobernante probado | 2.38.5 | 2.50.1 | 2.56.0 | 2.56.0 reftable | Casos | Hipótesis del ADR |
|---|---|---|---|---|---|---|---|
| Commit | `pre-commit`, `commit-msg` | A | A | A | A | `commit`, `commit-msg`, `commit-a-rejected` | ✅ Confirmada. `commit -a` rechazado no toca el índice real |
| Commit `--no-verify` | `reference-transaction` `prepared` | A | A | A | A | `commit-no-verify` | ✅ La segunda línea no se salta |
| Push | `pre-push` | A | A | A | A | `push` | ✅ |
| Force-push (`-f`, `+ref`, `--force-with-lease`, `--mirror`) | `pre-push` | A | A | A | A | `force-push*`, `push-mirror` | ✅ |
| Borrar rama remota (`--delete`, `:rama`) | `pre-push` | A | A | A | A | `delete-remote*` | ✅ |
| Borrar rama local (`branch -D`, `update-ref -d`, empaquetada o suelta) | `reference-transaction` `prepared` | A | A | A | A | `delete-local*`, `update-ref-d*` | ✅ |
| Renombrar rama (`branch -m`) | `reference-transaction` `prepared` | A | A | A | **C** | `rename-branch` | ⚠️ **Nuevo**: con reftable el renombrado **no ejecuta el hook** |
| Mover una rama (`branch -f`) | `reference-transaction` `prepared` | A | A | A | A | `overwrite-branch` | — (no está en el catálogo) |
| `reset --hard` con cambio de rama | `reference-transaction` | B (iw) | B (iw) | B (iw) | B | `reset-hard` | ✅ No impedible |
| `reset --hard` sin mover la rama | — | C | C | C | C | `reset-hard-head` | ✅ |
| Rebase | `pre-rebase` | A | A | A | A | `rebase` | ✅ |
| Rebase: segunda línea | `reference-transaction` | B | B | B | B | `rebase-reftx` | ✅ Protege la ref, pero llega tarde |
| `pull --rebase` | `pre-rebase` | A | A | A | B (r)¹ | `pull-rebase` | ✅ **Resuelto el "por confirmar"**: `pull --rebase` ejecuta `pre-rebase` después del `fetch` (que solo cambia `refs/remotes/`) y antes de tocar el working tree |
| Merge con commit | `pre-merge-commit` | B (iws) | B (iws) | B (iws) | B | `merge`, `merge-reftx` | ✅ No impedible |
| Merge fast-forward | `reference-transaction` | B (iw) | B (iw) | B (iw) | B | `merge-ff` | ⚠️ **Precisión**: el working tree se actualiza antes del hook. No impedible |
| `merge --no-commit` | — | C | C | C | C | `merge-no-commit` | ✅ |
| Crear worktree con rama nueva | `reference-transaction` (rama nueva) | A | A | A | A | `worktree-add-b` | ✅ **Confirmada**: el hook corre antes de crear los archivos de administración |
| Crear worktree sin rama nueva (rama existente o `--detach`) | `reference-transaction` (`HEAD` del worktree nuevo) | A² | A² | A² | A² | `worktree-add-exist`, `worktree-add-detach` | ⚠️ **Corrige la hipótesis**: el hook corre con la administración ya creada, pero al denegar Git revierte y no queda nada |
| Crear worktree: `post-checkout` | `post-checkout` | B (rs) | B (rs) | B (rs) | B | `worktree-add-postco` | ✅ Solo informa: el worktree queda creado |
| Borrar worktree | — | C | C | C | C | `worktree-remove` | ✅ No hay hook |
| `push --no-verify`, `rebase --no-verify` | — | C | C | C | C | `*-no-verify` | ✅ Salto voluntario declarado |
| `send-pack` (plumbing) | `pre-push` | C | C | C | C | `send-pack-force` | ✅ **Resuelto**: `send-pack` no ejecuta `pre-push` |
| Plumbing `commit-tree` + `update-ref` | `reference-transaction` | A | A | A | A | `commit-tree-plumb` | ✅ |

¹ Con reftable, la huella de refs es el contenido binario de `reftable/`, que incluye los reflogs y `refs/remotes/`. La "r" es ruido de la medición, no un cambio de una rama gobernada.
² A **efectiva**: en el momento del hook ya existen `.git/worktrees/<id>` y el directorio, pero al denegar Git los borra y la huella final es idéntica. Para la política, el hook ve una línea de `HEAD` y no la creación de un worktree (§ 3.2).

## 3. Force-push y borrado de la rama base (US-GRD-001): cerrado

**Método** (`suites/02-forcepush-basedelete.sh`): el prototipo `lib/guard-hook.sh` protege `main` como lo haría `raptor hook`. Implementa estas reglas:

- **Alias**: una ref que, plegada a minúsculas, coincide con la rama base sin ser idéntica a ella se deniega por ambigüedad (ADR-GRD-002 § 4).
- **`pre-push`**:
  - Deniega si la ref remota es la rama base y el objeto local es cero (borrado).
  - Deniega si el objeto remoto falta en local, si el clon es superficial o si `GIT_NO_REPLACE_OBJECTS=1 git -c core.commitGraph=false merge-base --is-ancestor remoto local` falla (forzado).
- **`reference-transaction`**, solo en `prepared`: deniega si una línea deja la rama base en cero, salvo el *prune* de `pack-refs` (§ 3.3), y cualquier línea sobre un alias de la base.

Cada escenario comprueba el código de salida y que la rama protegida sigue intacta en local y en el remoto. Resultado: **PASS** en las tres versiones de Git (y con reftable, salvo D10).

| ID | Escenario | Resultado | Nota |
|---|---|---|---|
| F01–F03 | `push -f`, `push +main`, `--force-with-lease` | Denegado, remoto intacto | — |
| F04, F05 | Push fast-forward de `main`; force-push de una rama propia | Permitido | Sin falsos positivos |
| F06 | Objeto remoto ausente en local | Denegado | Ausente = forzado (H-05) |
| F07 | Push **fast-forward** desde un clon superficial | Denegado | **Falso positivo asumido** por la regla "superficial = forzado". Se declara |
| F08 | `replace --graft` que hace parecer fast-forward un push que reescribe `main` | Denegado | F08b: **sin guard, Git acepta ese push sin `--force`** y reescribe el remoto. H-05 es real |
| F10 | Force-push desde un worktree enlazado | Denegado | — |
| F11 | `push -f origin main:refs/heads/Main` | Denegado por ambigüedad | Sin guard (F11b), el `receive-pack` remoto rechaza la creación ("reference already exists") porque en APFS `Main` es el archivo de `main`. **Sin la regla de ambigüedad, el guard lo habría permitido**: el remoto anuncia el objeto de `Main` como cero, es decir, como una creación |
| F12–F14 | `--mirror`, `'+refs/heads/*:refs/heads/*'`, `push -f` sin refspec con upstream | Denegado | `pre-push` recibe las refs ya resueltas: la normalización del argv (J9) no hace falta para **decidir**, solo para el token |
| F09, F15–F17 | `--no-verify`, `send-pack`, `-c core.hooksPath=/dev/null`, `GIT_CONFIG_COUNT…` | **Pasa** (salto) | Saltos voluntarios declarados (ADR-GRD-002 § 2) |
| D01, D02 | `branch -D main`, `update-ref -d refs/heads/main` | Denegado | — |
| D03, D04 | `push origin :main`, `push --delete main` | Denegado | — |
| D05 | `branch -D main` desde un worktree enlazado | Denegado | — |
| D06, D07 | `main` solo empaquetada, y suelta y empaquetada a la vez | Denegado | — |
| D08 | `branch -D Main` | Denegado por ambigüedad | D08b: **sin guard, en APFS con el backend de archivos `branch -D Main` borra `main`**. H-06 es real. Con reftable no aplica: `Main` es otra ref y no existe |
| D09 | `update-ref -d HEAD` con `HEAD` → `main` | Denegado | **2.38 entrega la línea como `HEAD`**; 2.50 y 2.56, como `refs/heads/main`. La normalización de `HEAD` es necesaria |
| D10 | `branch -m main otra` | Denegado (archivos) / **NO denegado (reftable)** | **Con reftable el renombrado no pasa por el hook** |
| D11 | `branch -M feat main` (renombrar sobre la base) | Denegado (archivos) / **NO denegado (reftable)** | Con archivos, Git primero borra `feat` en otra transacción. Al denegar, **`feat` queda borrada y `HEAD` apunta a una rama que no existe** ("No commits yet on feat"). El commit solo se recupera desde `.git/logs/HEAD` o con `git fsck --lost-found`. Efecto parcial (B) que afecta a NFR-01 |
| D12, D13 | `pack-refs --all`, `gc` | **Permitido** | Gracias a la excepción del § 3.3 |
| D14 | `pack-refs --all` con la regla ingenua | **Falla `pack-refs`** (exit 128) | Falso positivo grave sin la excepción |
| D15, D16 | `-c core.hooksPath`, `GIT_CONFIG_COUNT…` | Pasa (salto) | Declarado |
| D17 | Clave **relativa**, borrado desde un worktree enlazado | **Pasa** | La clave relativa no cubre los worktrees enlazados. D17b: desde el principal, sí deniega |
| D18 | `config.worktree` con otra clave en el worktree enlazado | **Pasa** | Caso "no se instala" de ADR-GRD-001 § 5 |
| D19 | Borrar `café` (NFC) escribiendo su forma NFD | Denegado | Con `core.precomposeUnicode=true` (el valor que pone `git init` en macOS), Git entrega NFC al hook |
| D19b | El mismo caso con `core.precomposeUnicode=false` | **No denegado: `café` se borra** | El hook recibe los bytes NFD y APFS resuelve el archivo NFC. El prototipo no normaliza a NFC: **confirma que la normalización NFC de ADR-GRD-002 § 4 es necesaria** (un repo clonado en Linux y abierto en macOS no tiene `precomposeUnicode`). Con reftable no aplica |
| D21 | `branch -f Main feat` (reescribir la base por su alias) | Denegado por ambigüedad | D21b: **sin guard, en APFS `main` queda apuntando a `feat`**. Con reftable, la regla deniega un `Main` que sí es otra ref: es un falso positivo asumido por la regla de ambigüedad |
| D20 | `rm .git/refs/heads/main` | Pasa (salto) | Escritura a mano en `refs/`: declarado |

### 3.1 Qué recibe `pre-push`

`pre-push` recibe las refs ya resueltas (`<ref local> <oid local> <ref remota> <oid remoto>`) para cualquier forma del argv: `-f`, `+`, `--mirror`, comodines, sin refspec y `push.default`. Para **decidir** basta con leer la entrada estándar. Lo que hay que normalizar del argv es solo el token de excepción (J9, ADR-GRD-007 § 3), y eso queda fuera de este spike.

### 3.2 Qué recibe `reference-transaction`

| Observación | Versiones | Consecuencia |
|---|---|---|
| Estado nuevo **`preparing`**, antes de tomar los locks. Un exit ≠ 0 también aborta. Los simbólicos no están resueltos | 2.54 en adelante (RelNotes 2.54.0; observado en 2.56.0) | Cada transacción lanza **tres** procesos en lugar de dos. El dispatcher debe salir en `sh` en cualquier estado que no sea `prepared`, y no solo en `committed` y `aborted` |
| Actualizaciones de **refs simbólicas** con el valor `ref:<destino>` (p. ej. `0000… ref:refs/heads/feat HEAD` al cambiar de rama) | 2.50.1 y 2.56.0; no en 2.38.5 | El parser debe aceptar `ref:` como valor. Si no, con la validación estricta de ADR-GRD-002 § 4, cada `switch` sería una "línea malformada" que va al daemon |
| Borrados con valor viejo **cero** (`0 0 refs/heads/x`), y dos invocaciones `prepared` por borrado (la transacción de `packed-refs` y la de la ref suelta) | Todas | La regla de borrado debe usar solo el valor nuevo |
| **`pack-refs`/`gc` emiten `<oid> 0000… refs/heads/main`** (*prune* de la ref suelta tras escribirla en `packed-refs`) | Todas con el backend de archivos | Es idéntico a un borrado. Sin excepción, Guardrails **rompe `gc`, `gc --auto` y `maintenance`** (D14) |
| Una ref con otra capitalización (`refs/heads/Main`) llega tal cual, y en una creación el valor viejo es cero aunque en APFS el archivo sea el de `main` | Todas (backend de archivos) | El plegado debe aplicarse a **todas** las líneas, no solo a los borrados (D21, F11) |
| Borrado a través de `HEAD`: la línea llega como `HEAD` en 2.38 y como `refs/heads/main` en 2.50+ | 2.38.5 | La normalización de `HEAD` (ADR-GRD-002 § 4) es imprescindible para la versión mínima |
| El `HEAD` del **worktree nuevo** llega como `HEAD` con el cwd del worktree **principal** (`git worktree add`, 2.50+) | 2.50.1 y 2.56.0 | Resolver `HEAD` leyendo el `HEAD` del cwd da la rama equivocada |
| Pseudo-refs frecuentes: `AUTO_MERGE` (en cada commit, reset y switch), `CHERRY_PICK_HEAD` y `REBASE_HEAD` (rebase), `ORIG_HEAD` y `HEAD` separado | Todas (`AUTO_MERGE` desde 2.4x) | `CHERRY_PICK_HEAD` y `REBASE_HEAD` **no están** en la lista de no gobernadas de ADR-GRD-002 § 4: un rebase de 3 commits enviaría 9 evaluaciones evitables al daemon (suite 05, `H06-rebase-*-prepared-refs`) |
| Hook con `GIT_DIR` cruzado: corre con el cwd del **otro** repo y `GIT_DIR` apuntando al protegido | Todas | Confirma M-02: el dispatcher no puede deducir su repo del cwd (`04`, X01–X03) |

### 3.3 La excepción del *prune* de `pack-refs`

Con el backend de archivos, `pack-refs` (y por tanto `gc`, `gc --auto` y `maintenance`) hace dos transacciones:

1. Escribe `packed-refs` (`0 → oid`).
2. Borra cada ref suelta (`oid → 0`).

La segunda es indistinguible de `git branch -D` por la línea. El criterio que usa el prototipo, y que pasa D01–D14, es este: una línea `viejo 0 ref` **no** es un borrado si `viejo ≠ 0`, el archivo suelto `<common>/<ref>` contiene `viejo` y `packed-refs` contiene exactamente `viejo ref`.

- **Por qué no deja pasar un borrado explícito**: `branch -D` y `update-ref -d` siempre emiten antes la transacción de `packed-refs` con `0 0 ref` (viejo cero), y esa sí se deniega.
- **Ámbito**: es específico del backend de archivos. Con reftable no hay *prune*.

**El código fijo del `sh` cuando falta el binario (ADR-GRD-001 § 3) tiene el mismo problema**: "una línea con nuevo igual a cero sobre `refs/heads/*` → exit 1" rompe `gc` mientras el binario no esté.

### 3.4 Reftable: borrar o reescribir la rama base renombrando

Con `--ref-format=reftable` (Git 2.45 en adelante), `git branch -m main otra` y `git branch -M feat main` **no ejecutan `reference-transaction`** en 2.56.0 (D10, D11; matriz `rename-branch`). No hay ningún otro hook antes. Hoy, con Git crudo, no se puede impedir borrar la rama base renombrándola en un repo reftable. Se puede elegir entre tres caminos:

- declararlo en la lista publicada para reftable;
- marcar los repos reftable como "no se instala";
- reportarlo aguas arriba a Git.

Es una decisión de producto y arquitectura (§ 9, E-02-4).

## 4. Saltos (ADR-GRD-002 § 2)

| Salto | Observado | Evidencia |
|---|---|---|
| `--no-verify` en commit, push y rebase | Salta el hook principal y **no** `reference-transaction` | `01`: `*-no-verify`; `02`: F09 |
| `-c core.hooksPath=…`, `GIT_CONFIG_COUNT/KEY/VALUE` | Saltan todos los hooks | `02`: F16, F17, D15, D16 |
| `send-pack` | No ejecuta `pre-push` | `01`: `send-pack-force`; `02`: F15 |
| `update-ref`, `update-ref -d`, `commit-tree` | **No es un salto**: pasan por `reference-transaction` | `01`: `update-ref-d*`, `commit-tree-plumb` |
| Escritura a mano en `refs/` | Salta todo | `02`: D20 |
| Clave relativa, `config.worktree`, `include` e `includeIf` (§ 5) | Salta todo en el worktree afectado | `02`: D17, D18; `04`: W03–W07, W09 |
| **Nuevo**: renombrar sobre la rama base (`branch -M x main`) | Con archivos se deniega, pero deja `x` borrada y `HEAD` colgando (B). Con reftable, pasa | `02`: D11 |
| **Nuevo**: renombrar la rama base con reftable | Pasa sin hook | `02`: D10 |

## 5. Clave de activación y worktrees (ADR-GRD-001 § 1, § 4, § 5)

### 5.1 Configuración byte a byte (`suites/04`, C01–C06)

| Caso | Tras instalar y desinstalar |
|---|---|
| Sin valor previo | Idéntico |
| Valor previo local escrito por `git config` (`.husky/_`) | Idéntico |
| Valor previo solo global | Idéntico |
| Segundo bloque `[core]` con la clave | Idéntico |
| **Valor previo escrito a mano** (`hookspath=.husky/_   # husky`) | **Difiere**: Git reescribe la línea como `hooksPath = .husky/_` y pierde el comentario |
| **`config` sin salto de línea final** | **Difiere**: Git añade el salto de línea |

Conclusión para NFR-GRD-01: la identidad byte a byte solo se puede prometer si se restaura el archivo, no la clave. Si no, el criterio tiene que ser **semántico** (mismo valor efectivo y mismo nivel). Recomendación en E-01-5.

### 5.2 Cobertura (`suites/04`, W01–W09)

| Caso | Valor efectivo en el worktree afectado | ¿Protegido? |
|---|---|---|
| Worktree creado **después** de instalar (W01), y desde un subdirectorio (W02) | La clave local absoluta | Sí |
| `extensions.worktreeConfig` + `core.hooksPath` en `config.worktree` del enlazado (W03) o del principal (W04) | El del `config.worktree` | **No** |
| `include.path` después de la clave (W05) | El del archivo incluido | **No** |
| `includeIf "onbranch:feat"` (W06), `includeIf "gitdir:…/worktrees/**"` (W07) | El incluido, solo en esa rama o en ese worktree | **No** |
| Clave solo global (W08) | La local, que gana | Sí |
| Clave local **relativa** (W09) | Relativa a cada worktree | **No**, en los enlazados |

`git config --show-scope --show-origin --get core.hooksPath`, ejecutado en cada worktree, basta para detectar todos estos casos al instalar y después, para detectar la pérdida (ADR-GRD-005).

## 6. Coexistencia con gestores (ADR-GRD-001 § 6; US-GRD-002)

**Versiones**: husky 9.1.7, lefthook 2.1.16 y pre-commit 4.6.2, instalados localmente (`setup-tools.sh`). **Método** (`suites/03`): con el gestor ya instalado, se instala el guard encadenando el valor previo de la clave (relativo al cwd, como Git) o `<common>/hooks`. Se hace commit en el worktree principal y en uno enlazado y se intenta `branch -D main`. Después se desinstala, o se reinstala el gestor encima.

| Gestor | Encadenado (M1) | Desinstalar (M4) | Reinstalar el gestor después (M3) | Hipótesis |
|---|---|---|---|---|
| Hooks propios en `.git/hooks` | Corren en el principal y en el enlazado; el guard deniega | Corren; `config` idéntico | — | ✅ |
| **husky** (`core.hooksPath=.husky/_`) | Corre en el principal. En el enlazado **no corre, ni sin Guardrails** (M2: `.husky/_` está ignorado y no existe allí). El guard deniega | Corre; `config` idéntico | `husky` (lo que hace `npm install` con el script `prepare`) **reescribe la clave en silencio**: la protección se pierde (`branch -D main` pasa) | ✅ Confirmada |
| **lefthook** (escribe en `.git/hooks`) | Corre en los dos; el guard deniega | Corre; `config` idéntico | `lefthook install`: **se niega** ("Custom hooks paths are not supported by default", exit 1), y la protección sigue intacta. `--force`: **escribe en la carpeta de Guardrails** y renombra nuestro `pre-commit` a `pre-commit.old` (el hash no cuadra). `--reset-hooks-path`: **borra la clave**, y la protección se pierde. La resincronización automática al cambiar `lefthook.yml` se salta ("Skipping hook sync") | ⚠️ **Corregida**: no escribe en la carpeta salvo con `--force` |
| **pre-commit** (framework, en `.git/hooks`) | Corre en los dos; el guard deniega | Corre; `config` idéntico | `pre-commit install` (también con `--overwrite`): "Cowardly refusing to install hooks with `core.hooksPath` set", y la protección sigue intacta | ✅ Confirmada |

**Encadenar sin alterarlos** funcionó en los cuatro casos. **Ningún gestor probado cae en "no se instala"** por el encadenado. Que el gestor no cubra los worktrees enlazados (husky) es su propio comportamiento y Guardrails lo preserva tal cual. Con un `GIT_DIR` cruzado, los hooks corren con el cwd del otro repo (X02). Un valor previo relativo se resolvería contra ese cwd, igual que en Git: es una inferencia, no se probó con un gestor.

## 7. Conjunto de dispatchers (`suites/05`)

| Hook | Efecto de que exista un dispatcher que sale con 0 | Evidencia |
|---|---|---|
| `push-to-checkout` | Con `receive.denyCurrentBranch=updateInstead`, Git **delega** en el hook: la ref se mueve, pero **el working tree del repo receptor no se actualiza** (queda `D new.txt`) | H01 frente a H02 |
| `proc-receive` | Solo se usa con `receive.procReceiveRefs`. Un dispatcher que no habla el protocolo hace fallar esos pushes ("fail to negotiate version"). Sin ese ajuste, no se invoca | H03, H04 |
| `post-index-change` | Se ejecuta **en cada `git status`**, también con el árbol limpio: un proceso más en cada consulta de estado de los IDE y de los agentes | H05 |
| Resto | Sin cambio de comportamiento observado | `01`, `05` |

**Confirmado**: `push-to-checkout`, `proc-receive` y `post-index-change` solo se instalan si el directorio previo ya los tenía (ADR-GRD-001 § 2).

**Procesos por comando** con el conjunto completo (sonda en todos los hooks; `05`, H06):

| Comando | 2.38.5 | 2.50.1 | 2.56.0 |
|---|---|---|---|
| `commit` | 7 | 10 | 12 |
| `switch -c` | 3 | 11 | 15 |
| `tag` | 2 | 2 | 3 |
| `stash` + `pop` | 18 | 28 | 33 |
| `rebase` de 3 commits | 52 | 72 | 91 |

## 8. Coste (`suites/06`)

**Método**: no hay daemon todavía, así que se mide lo que añade la capa de hooks por sí sola. Hay cuatro variantes:

- **V0**: sin hooks.
- **V1**: el diseño del ADR. Un dispatcher `sh` que sale en `sh` en todo estado que no sea `prepared` y hace `exec` de un binario nativo (`lib/hookstub.rs`, Rust `-O`, que lee y parte la entrada) en `prepared` y en el resto de hooks.
- **V2**: `sh` + binario siempre.
- **V3**: el guard entero en `sh`.

Las variantes se aplican a cada comando con `lib/bench.py` (p50 y p95 de N ejecuciones tras el calentamiento). La máquina es un portátil de desarrollo con la carga habitual y software corporativo de seguridad: **los valores absolutos son altos y ruidosos** (dos ejecuciones del mismo fetch dieron 17 s y 28 s), así que importan los órdenes de magnitud y las diferencias. TSV: `results/*/06-cost.tsv`.

### 8.1 Coste de lanzar un proceso en esta máquina (p50, ms)

| | 2.38.5 | 2.50.1 | 2.56.0 |
|---|---|---|---|
| Medición vacía (`:` dentro de `sh -c`) | 9,5 | 5,8 | 8,5 |
| `+ /bin/sh -c :` | +9,0 | +8,6 | +10,7 |
| `+` binario nativo | +3,1 | +3,6 | +1,2 |
| `+ /bin/sh` que hace `exec` del binario | +14,3 | +10,3 | +7,5 |

**Un dispatcher `sh` cuesta aquí unos 8–11 ms por invocación y un binario nativo, 1–4 ms**, antes de evaluar nada.

### 8.2 Coste por comando (p50 de V0 y Δp50 de V1 / V2 / V3, ms)

| Escenario | Procesos de hook (2.38 / 2.50 / 2.56) | 2.38.5 | 2.50.1 | 2.56.0 |
|---|---|---|---|---|
| `update-ref` (1 transacción) | 2 / 2 / 3 | 16 → +18 / +24 / +21 | 23 → +18 / +20 / +20 | 14 → +24 / +35 / +34 |
| `commit` con el conjunto completo de dispatchers | 6 / 10 / 11 | 22 → +73 / +80 / +74 | 31 → +83 / +94 / +96 | 23 → +103 / +124 / +117 |
| `status` con `post-index-change` | 1 / 1 / 1 | 17 → +10 / +12 / +12 | 21 → +12 / +11 / +11 | 17 → +11 / +13 / +11 |
| `push` fast-forward con la evaluación gobernada de `pre-push` (V3: `merge-base` real) | 3 / 3 / 4 | 99 → +38 / +46 / +98 | 101 → +33 / +43 / +88 | 70 → +32 / +46 / +79 |
| `fetch` de 1.000 refs nuevas | 2.004 / 2.006 / 3.016 | 989 → **+17.896** / +27.416 / +22.752 | 443 → **+16.816** / +20.811 / +21.368 | 1.103 → **+27.228** / +32.058 / +36.683 |
| `fetch --atomic` de 1.000 refs | 2 / 4 / 3.016 | 269 → +42 / +11 / +565 | 300 → +17 / +32 / +641 | 685 → **+27.095** / +38.739 / +35.923 |
| `fetch` de 1.000 refs sin maintenance automática (`-c maintenance.auto=false -c gc.auto=0`) | 2.002 / 2.004 / 6 | 272 → +24.336 / +31.981 / +30.852 | 300 → +27.751 / +34.234 / +31.816 | 257 → **+44** / +57 / +4.823 |
| `pack-refs --all` de 1.000 ramas sueltas (lo que hacen `gc --auto` y la maintenance) | ≈ 2.000 / ≈ 2.000 / ≈ 3.000 ³ | 159 → **+28.074** / +34.133 / +36.706 | 184 → **+27.277** / +34.939 / +37.753 | 159 → **+39.352** / +47.921 / +47.045 |

³ El TSV registra 6.006 y 9.009 porque cuenta la primera ejecución, con unas 3.000 refs sueltas. En las iteraciones medidas se podan 1.000 refs: una transacción por ref, con 2 procesos cada una (3 desde 2.54).

En los seis primeros escenarios, "procesos de hook" se contó con una sonda en **todos** los hooks, y la variante instala solo los nombrados en la suite. En los siguientes, con un contador mínimo en esos mismos hooks.

### 8.3 Conclusiones de coste

1. **Por invocación, el presupuesto se cumple**:
   - La vía rápida (V1, estado ≠ `prepared` o refs no gobernadas) cuesta un `sh`: unos 8–11 ms de p50 y menos de 20 ms de Δp95 (§ 8.1, TSV). Cabe en el objetivo de < 30 ms p95 en macOS (ADR-GRD-002 § 5).
   - La evaluación gobernada de `pre-push` en `sh` con `merge-base` (V3) añade unos 45–60 ms por encima de V1 en un repo pequeño. El daemon y la evaluación real no se midieron, porque no existen todavía.
2. **Por comando, no se cumple si se instala el conjunto completo**: un commit lanza de 6 a 12 procesos según la versión (§ 7) y paga **+73 a +103 ms** sin evaluar nada. Con `preparing` (2.54 en adelante), cada transacción lanza tres procesos. De ahí E-01-2 (instalar solo los dispatchers con política o con hook previo) y E-02-9 (objetivo por comando).
3. **Las transacciones de una sola ref escalan con el número de refs**:
   - En 2.38 y 2.50, `fetch` sin `--atomic` hace **una transacción por ref** (2.004 procesos para 1.000 refs: **+17 a +28 s**).
   - En 2.56 el `fetch` va en lote, pero el **`pack-refs` automático de la maintenance** que lo sigue poda cada ref suelta en una transacción propia (3.016 procesos, también con `--atomic`). Sin la maintenance automática, el fetch de 2.56 lanza 6 procesos y cuesta **+44 ms**.
   - **`pack-refs` de 1.000 ramas sueltas** (`gc --auto` y la maintenance, en cualquier versión): **+27 a +39 s**. Además, esas líneas de *prune* son de `refs/heads/*`, es decir, **gobernadas**: si la excepción del § 3.3 no se aplica antes de contactar con el daemon, cada una sería una evaluación gobernada.
   - La vía rápida por invocación no lo resuelve: el coste es el número de procesos. **No hay mitigación dentro de la capa de hooks**, porque Git lanza el hook por transacción. Mitigaciones posibles: un dispatcher nativo (divide el coste por invocación entre 2 y 3) y declararlo.
4. **El guard en `sh` no sirve con entradas grandes**: con 1.000 líneas en una sola transacción (`fetch --atomic`, 2.38/2.50), V3 añade +565/+641 ms frente a +17/+42 de V1. Confirma que la evaluación debe estar en el binario y que el `sh` solo debe decidir el estado.
5. **Windows sin medir**: el `sh` de Git for Windows es más caro que el de macOS. Con estos números, el dispatcher nativo de Windows (pendiente en ADR-GRD-001) deja de ser una optimización y pasa a ser **necesario** si se mantiene el conjunto completo.

## 9. Enmiendas recomendadas (aplicadas el 2026-10-04; resolución en el § 13)

### ADR-GRD-002

- **E-02-1 · Matriz § 1**:
  - "Crear worktree sin rama nueva" pasa a **impedible con reversión de Git**, con la reserva de que el hook solo ve una línea de `HEAD`. Si la política no la reconoce, se mantiene publicada como **no impedible** (decisión del Arquitecto).
  - Merge fast-forward: B.
  - `pull --rebase`: A por `pre-rebase` (se quita el "por confirmar").
  - `send-pack`: salto **declarado** (se quita el "por confirmar").
- **E-02-2 · § 4, estados**: evaluar **solo** en `prepared` y salir en `sh` en cualquier otro estado. Hoy son `preparing` (2.54 en adelante), `committed` y `aborted`, y cualquier estado futuro debe tratarse igual.
- **E-02-3 · § 4, entrada**:
  - Aceptar `ref:<destino>` como valor viejo o nuevo (refs simbólicas, 2.4x en adelante).
  - Detectar el borrado solo por el valor nuevo cero (el viejo puede ser cero).
  - No resolver `HEAD` desde el cwd: tomar el destino de la línea `ref:` si existe y, si no, el `HEAD` del `GIT_DIR` de la transacción. Una línea `HEAD` con valor nuevo distinto de cero emitida al crear un worktree no es una actualización del `HEAD` del principal.
- **E-02-4 · § 1 y § 3, reftable**: decidir entre declarar, marcar como "no se instala" o reportar aguas arriba que el renombrado (`branch -m/-M`) no ejecuta el hook. Se recomienda **detectar `extensions.refStorage=reftable` al instalar** y publicar la fila "borrar o reescribir la rama base renombrándola" como **no impedible en reftable**.
- **E-02-5 · § 4, *prune* de `pack-refs`**: añadir la excepción del § 3.3 a la regla de borrado, con su prueba (D12–D14) en INF-GRD-001, y **aplicarla en la vía rápida**, sin contactar con el daemon (un `gc --auto` con 1.000 ramas sueltas son 1.000 transacciones, § 8.3).
- **E-02-6 · § 4, pseudo-refs**: sustituir la lista cerrada de pseudo-refs no gobernadas por la regla de gitglossary: todo nombre de una sola componente en mayúsculas, fuera de `refs/`, salvo `HEAD`. Así entran `CHERRY_PICK_HEAD`, `REBASE_HEAD`, `MERGE_HEAD`, `REVERT_HEAD` y `BISECT_HEAD`.
- **E-02-7 · § 2, saltos nuevos**:
  - Renombrar sobre la rama base (`branch -M x main`): con archivos se deniega con un efecto parcial (B: la rama origen queda borrada y `HEAD` apunta a una rama que no existe; recuperable desde `logs/HEAD`). Conviene que la Time Machine lo cubra (snapshot previo) y que la lista publicada lo declare.
  - Renombrar con reftable: no pasa por el hook.
- **E-02-8 · § 1, fila force-push**: declarar el **falso positivo** del push fast-forward desde un clon superficial (F07), o aceptarlo, ya que la regla lo trata como forzado.
- **E-02-9 · § 5, presupuesto**: separar "coste por evaluación" de "coste por comando". Fijar un objetivo por comando (commit, switch, rebase, fetch) con el número de procesos (§ 8).
- **E-02-10 · § 4, alias (H-06)**: aplicar el plegado de mayúsculas y la NFC a **todas** las líneas de `pre-push` y de `reference-transaction` (creación y actualización, no solo borrados). `branch -f Main x` reescribe `main` en APFS (D21b) y un push a `refs/heads/Main` llega como creación (F11). Con reftable, el plegado produce falsos positivos asumidos, porque allí `Main` es otra ref. Confirmar que NFC es necesario: sin `precomposeUnicode`, Git borra la rama NFC con un nombre NFD (D19b).

### ADR-GRD-001

- **E-01-1 · § 3, código fijo del `sh`**: la regla "cualquier borrado de `refs/heads/*` → exit 1" debe incluir la excepción del *prune* de `pack-refs` (§ 3.3). Si no, `gc` falla mientras falte el binario.
- **E-01-2 · § 2, conjunto de dispatchers**:
  - Confirmado: `push-to-checkout`, `proc-receive` y `post-index-change` solo con hook previo.
  - **Recomendación nueva**: instalar **solo** los dispatchers de los hooks que tengan una política activa o un hook previo que encadenar (más los obligatorios de US-GRD-001: `pre-push`, `reference-transaction` y `pre-rebase`), y regenerar el conjunto cuando cambie cualquiera de las dos cosas. Con el coste medido (§ 8), cada dispatcher sin función cuesta un proceso `sh` en cada operación.
- **E-01-3 · § 2, `reference-transaction` sin hook previo**: la primera línea del dispatcher debe salir en todos los estados que no sean `prepared` (E-02-2).
- **E-01-4 · § 6, tabla de gestores**: corregir la fila de lefthook:
  - `lefthook install` **se niega** con la clave definida y la protección sigue intacta.
  - `--force` escribe en la carpeta y renombra el dispatcher a `.old`: el hash no cuadra y pasa a inactiva.
  - `--reset-hooks-path` borra la clave y pasa a inactiva.

  Añadir que **husky no cubre los worktrees enlazados ni sin Guardrails**: el encadenado relativo lo conserva y no debe tratarse como un fallo de cobertura.
- **E-01-5 · § 4, huella**: restaurar la clave con `git config` no garantiza la identidad byte a byte cuando el valor previo se escribió a mano o el `config` no termina en salto de línea (§ 5.1). Hay dos opciones:
  - **(a)** criterio **semántico** en NFR-GRD-01: mismo valor efectivo, mismo nivel y mismas demás claves;
  - **(b)** guardar en el diario la línea original exacta y restaurarla.

  Se recomienda **(a)**, con (b) solo si el valor previo era local.
- **E-01-6 · § 5**: añadir a "no se instala" **reftable** (si se elige así en E-02-4) y documentar que la comprobación de cobertura se hace con `--show-scope --show-origin` por worktree (§ 5.2).
- **E-01-7 · § 2 y § 8, binario nativo**: tener en cuenta el coste de `sh` (§ 8) en la decisión pendiente del dispatcher nativo de Windows, y también en macOS y Linux si se adopta E-01-2.

## 10. Casos que quedan como "no se instala"

Confirmados por este spike (ADR-GRD-001 § 5 y § 6):

1. `extensions.worktreeConfig` con `core.hooksPath` en el `config.worktree` de cualquier worktree (W03, W04, D18).
2. `include`/`includeIf` en el nivel local o de worktree que defina `core.hooksPath` (W05, W07), y cualquier `includeIf "onbranch:…"` (W06).
3. Un valor que no sea representable como constante (no probado en este spike: M-04, ver § 11).

Propuestos por este spike, a decidir:

4. **Repos reftable**, mientras el renombrado no pase por el hook (E-02-4).

Descartados para los gestores probados: **ninguno** de husky 9, lefthook 2 y pre-commit 4 obliga a "no se instala" por el encadenado (§ 6).

## 11. No verificado y procedimiento

| Pendiente | Por qué | Procedimiento |
|---|---|---|
| **Linux** (las tres versiones de Git) | El spike se ejecutó solo en este Mac | `./setup-tools.sh && SKIP_COST=1 ./run-all.sh` con cada `GIT_BIN_DIR`, y `suites/06-cost.sh` en reposo. Esperado: D08, D21 y F11 sin efecto, porque ext4 distingue mayúsculas |
| **Windows** (incluido el coste, que según `technical-stories.md` también bloquea el merge de US-GRD-001) | Sin máquina Windows | Mismas suites desde Git Bash. Comparar en `06` un dispatcher `sh` con un ejecutable nativo (si Git for Windows lo ejecuta como hook). D08, D21 y F11 deberían aplicar, porque NTFS no distingue mayúsculas por defecto |
| Criterio de humano y agente en Windows (M-07) | Requiere Windows | — |
| Dispatcher con constantes no ASCII, espacios o comillas en los tres SO (M-04) | Fuera del tiempo de esta entrega | Instalar con `SANDBOX` en una ruta con espacios y `José`, y comprobar los escapes octales |
| Actualización del binario (J4: `brew upgrade`, `winget`, `npm`) | Requiere el binario real | — |
| Pérdida externa (tiempo de detección del observador) | Requiere el observador del daemon | — |
| Grafts por archivo (`info/grafts`, obsoleto) y `commit-graph` con padres falsos | Solo se probó `replace --graft` | Añadir el caso a `02` |
| Clon superficial con `--unshallow` en curso | No se probó | — |

## 12. Reproducción

Ver [`spikes/hook-interceptability/README.md`](../../../../../spikes/hook-interceptability/README.md). Resumen:

```sh
cd spikes/hook-interceptability
./setup-tools.sh
SKIP_COST=1 ./run-all.sh                                   # git del PATH
GIT_BIN_DIR=<git-2.38.5>/bin SKIP_COST=1 ./run-all.sh
GIT_BIN_DIR=<git-2.56.0>/bin REF_FORMAT=reftable SKIP_COST=1 ./run-all.sh
BENCH_N=100 bash suites/06-cost.sh                          # en reposo
```

## 13. Resolución de las enmiendas (2026-10-04)

Cada fila es una **Decisión del orquestador (2026-10-04), validada por Arquitecto/PO**. Los subagentes Arquitecto y PO hicieron la validación y propusieron los ajustes de la tercera columna.

| Enmienda | Resolución | Ajuste del Arquitecto o el PO | Aplicada en |
|---|---|---|---|
| E-02-1 · Matriz | Aceptada. Crear worktree sin rama nueva: A efectiva, publicada como no impedible | Motivo nuevo `no-reconocible` (Arquitecto) | ADR-GRD-002 § 1–3 |
| E-02-2 · Solo `prepared` | Aceptada | — | ADR-GRD-002 § 4; ADR-GRD-001 § 2 |
| E-02-3 · Entrada (`ref:`, borrado, `HEAD`) | Aceptada | — | ADR-GRD-002 § 4 |
| E-02-4 · Reftable | Límite conocido, publicado en la lista del repo y en el permiso; mitigación después del hecho | Detector: no basta Q42, `-M feat main` reescribe la base; el backend se vuelve a comprobar; regresión por versión (Arquitecto). Aviso propio y recuperación guiada, después del MVP (PO) | ADR-GRD-002 (Enmienda); ADR-GRD-005; Q-GRD-28; BR-EDGE-001/003; US-GRD-001/004 |
| E-02-5 · *Prune* de `pack-refs` | Aceptada | En la vía rápida del binario, no en `sh` (Arquitecto) | ADR-GRD-002 § 4 |
| E-02-6 · Pseudo-refs | Aceptada | Formas `main-worktree/` y `worktrees/<id>/` (Arquitecto) | ADR-GRD-002 § 4 |
| E-02-7 · Saltos nuevos | Aceptada: efecto parcial (B) declarado | Motivo `renombrado-sobre-base` con el oid para recuperar (Arquitecto y PO) | ADR-GRD-002 § 2; ADR-GRD-003 § 3; BR-EDGE-003 |
| E-02-8 · Clon superficial | Falso positivo aceptado | Apartado "lo que se deniega de más", motivo `historia-superficial` (PO, Arquitecto) | ADR-GRD-002 § 1, § 3; Q-GRD-31 |
| E-02-9 · Presupuesto | Dos ejes y gate de procesos por comando | Se descarta +50 ms (Arquitecto); techo ≤ 150 ms p95 por commit o cambio de rama, ⚠️ por confirmar por Rene (PO) | ADR-GRD-002 § 5; NFR-GRD-04; Q-GRD-30 |
| E-02-10 · Alias en todas las líneas | Aceptada | — | ADR-GRD-002 § 4 |
| E-01-1 · `sh` sin binario | Aceptada | — | ADR-GRD-001 § 3 |
| E-01-2 · Conjunto mínimo | Aceptada | Orden dispatcher→política, regeneración archivo por archivo con lock, ventana de hooks previos declarada; husky ⚠️ (Arquitecto) | ADR-GRD-001 § 2; ADR-GRD-005 |
| E-01-3 · Salida fuera de `prepared` | Aceptada | — | ADR-GRD-001 § 2 |
| E-01-4 · lefthook 2 y husky | Aceptada | — | ADR-GRD-001 § 6 |
| E-01-5 · Huella | (a) criterio semántico | Redacción de BR-CONS-005 y del RNF (PO) | ADR-GRD-001 § 4; NFR-GRD-01; Q-GRD-29; US-GRD-003 |
| E-01-6 · No se instala | Reftable sí se instala; comprobación con `--show-scope --show-origin` | — | ADR-GRD-001 § 5 |
| E-01-7 · Binario nativo | Aceptada | Necesario para `reference-transaction` desde Git 2.54 en los tres SO (Arquitecto) | ADR-GRD-001 § 2 |
| Coste 17–39 s / 1.000 refs | Coste lineal declarado, pendiente ≤ una invocación de la vía mínima por transacción, vía rápida en el binario, dispatcher nativo; sin quitar `reference-transaction` | "Infrecuente" no se asume (Arquitecto); decirlo en la documentación y en el permiso (PO) | ADR-GRD-002 § 5; Q-GRD-30 |

**Pendiente en otros frentes**: ADR-GRP-010 (que el observador vigile `reftable/`) y ADR-TMC-004 § 2 (nivel b con eventos de reftable), en la tabla de [non-functional-guardrails.md](../../../../architecture/non-functional-guardrails.md). **Fuera de alcance**: reportar aguas arriba a Git el renombrado sin hook en reftable.

## 14. Coste en Windows (2026-10-05, US-GRD-001)

**Máquina**: Windows 10 22H2 (19045), Intel Core i5-7400, 8 GB, Git 2.56.0.windows.1, ejecutado por SSH con el `bash` de Git for Windows. **Suite**: [`suites/07-cost-native.sh`](../../../../../spikes/hook-interceptability/suites/07-cost-native.sh), nueva y portable: el cronómetro es [`lib/bench.rs`](../../../../../spikes/hook-interceptability/lib/bench.rs) (sin Python ni `sh` alrededor del comando medido) y añade la variante **VN**, el binario nativo como archivo del hook, sin `sh`. N = 100 (push 50; `fetch` y `pack-refs` 3). TSV: `results/msys_nt-10.0-19045-x86_64-git2.56.0.windows.1/07-cost-native.tsv`.

**Git for Windows ejecuta un PE nativo como hook**, tanto sin extensión (`hooks/reference-transaction`) como con `.exe` (`native-hook`: un binario que sale con 1 hace fallar `update-ref`).

| Escenario (procesos de hook) | V0 sin hooks | V1 `sh` + nativo en `prepared` | V2 `sh` + nativo siempre | VN nativo |
|---|---|---|---|---|
| Arranque: binario / `sh -c :` / `sh` + `exec` | — | 31,3 (`sh -c :`) | 46,8 (`sh` + `exec`) | 4,1 |
| `update-ref` (3) | 37,7 | 174,3 | 175,1 | 57,5 |
| commit, conjunto mínimo (7) | 65,9 | 368,6 | 411,0 | 106,2 |
| commit, conjunto completo (11) | 66,2 | 563,7 | 624,8 | 132,7 |
| `switch -c`, conjunto mínimo (14) | 44,5 | 644,0 | 722,7 | 118,7 |
| push fast-forward, conjunto mínimo (4) | 207,3 | 410,1 | 425,2 | 242,4 |
| `fetch` de 1.000 refs nuevas (3.018) | 2.557 | 132.304 | 142.085 | 15.714 |
| `pack-refs` de 1.000 ramas sueltas | 1.046 | 126.972 | 160.802 | 15.750 |

p50 en ms. Las p95 están en el TSV.

**Conclusiones**:

1. **Un dispatcher `sh` cuesta ≈ 43 ms por invocación en Windows** (commit con el conjunto mínimo: +303 ms), cuatro veces más que en macOS. Con `sh`, un commit o un cambio de rama habitual supera el techo ⚠️ ≤ 150 ms p95 de ADR-GRD-002 § 5 sin evaluar nada.
2. **El nativo cuesta ≈ 6 ms por invocación** (commit con el conjunto mínimo +40 ms; `switch -c` +74 ms con 14 invocaciones). El objetivo ⚠️ ≤ 5 ms p95 por invocación que no evalúa queda algo por encima en esta máquina, que no está en reposo ni es rápida.
3. **Las operaciones masivas** bajan de +130 s a +13 s (`fetch` de 1.000 refs) y de +126 s a +15 s (`pack-refs`). Siguen siendo lineales en el número de refs (ADR-GRD-002 § 5, declarado).
4. **Decisión que habilita** (US-GRD-001, D1; ADR-GRD-001, Enmienda 2026-10-05): el dispatcher nativo es necesario en Windows para todo el conjunto, no solo para `reference-transaction`, y se adopta en los tres SO.

**Lo que no cubre**: la evaluación con el daemon en Windows (no hay canal todavía, XP-01) y la matriz funcional de los §§ 2 a 7 en Windows.
