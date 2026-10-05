---
id: DS-INF-GRD-001
title: "Dev Spec — Arnés de la capa de hooks (núcleo)"
type: dev-spec
status: approved
feature: guardrails
domain: GRP
story: INF-GRD-001
created: 2026-10-05
updated: 2026-10-05
related:
  stories: [INF-GRD-001, INF-GRP-001, SPIKE-GRD-001, US-GRD-001, US-GRD-002, US-GRD-003, US-GRD-004, US-GRD-005, US-GRD-006]
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-005, ADR-GRP-009]
  nfrs: [NFR-01, NFR-12, NFR-GRD-01, NFR-GRD-04]
tags: [guardrails, arnes, testkit, hooks-git, huella, interrupcion, cortes, matriz, coste-por-comando, nfr-01, nfr-12]
---

# Dev Spec — INF-GRD-001: arnés de la capa de hooks (núcleo)

Plano de ejecución compacto (AADD ligero) del **núcleo** de [INF-GRD-001](../technical-stories/INF-GRD-001-arnes-hooks.md). Amplía `crates/testkit` (INF-GRP-001) sin romper a sus consumidores. Sigue la Validación de [ADR-GRD-001](../../../../architecture/decisions/ADR-GRD-001-capa-hooks-instalacion.md) y [ADR-GRD-002](../../../../architecture/decisions/ADR-GRD-002-operaciones-interceptables.md), con sus enmiendas de SPIKE-GRD-001. La capa de hooks del producto no existe todavía: llega con US-GRD-001, que es quien conecta este arnés a su código.

## 1. Decisiones

Todas son **decisiones del orquestador (2026-10-04)**, validadas por el Arquitecto (`nassa-architect:architect`) y por el PO (`nassa-aadd:product-owner`). Se incluyen los ajustes que pidieron.

| # | Decisión | Validada por |
|---|---|---|
| D1 | **Hooks previos simulados**, sin npm, Go ni Python: los archivos y el `core.hooksPath` que deja cada gestor, tal como los observó SPIKE-GRD-001 (husky 9.1.7, lefthook 2.1.16, pre-commit 4.6.2). Cada hook previo es el mismo "linter", que rechaza un commit con un marcador, así que se puede comprobar que rechaza lo mismo antes y después. Hay una fixture más con `core.hooksPath` **global** (en el home temporal), que sirve para probar que solo se restaura el valor local | Arquitecto (ajuste: fixture global) |
| D2 | **Criterio semántico del `config`** (Q-GRD-29; ADR-GRD-001 § 4, Enmienda): el archivo se compara **como lo lee Git** (`git config --file <tmp> --list -z`, entorno aislado, sin seguir includes). No cuentan el formato, los comentarios, las comillas ni las secciones vacías. **El orden sí cuenta**, porque una clave posterior a un `include.path` lo sobrescribe. El parser propio de `without_keys` se sustituye; hasta ahora solo lo usaban los tests del testkit | Arquitecto (ajuste: parser de Git y orden) |
| D3 | **Valor efectivo y nivel** de `core.hooksPath` por worktree con `git config --show-scope --show-origin --get`, sin `GIT_CONFIG_COUNT` ni `GIT_CONFIG_PARAMETERS`. El exit 1 se lee como "clave ausente" | Arquitecto |
| D4 | **Protocolo de cortes por variables de entorno**: `GITRAPTOR_TEST_CUT=<paso>:<before\|during\|after>` y `GITRAPTOR_TEST_CUT_TRACE=<archivo>`. El producto las lee **solo con la cargo feature `test-cuts`**, no con `cfg(test)`, que no llega al binario hijo. En el punto de corte muere con exit 86, sin limpiar. `during` deja el estado en disco de un Git matado a mitad de escritura (un `config.lock` huérfano). US-GRD-001 añade un check de CI que falla si `GITRAPTOR_TEST_CUT` aparece en el binario de release | Arquitecto |
| D5 | **Cobertura de cortes**: antes del barrido, una pasada sin cortes y con traza. El arnés falla si un punto declarado no se alcanza, si se alcanza uno sin declarar o si la lista está vacía. **La recuperación corre en otro proceso**, y el perfil (donde vive el diario) queda fuera de la huella del repo | Arquitecto |
| D6 | **Matriz con dos columnas**: momento observado (A, B o C) y forma de publicación. Crear un worktree sin rama nueva se observa como A, pero se publica como `no-reconocible`. Cada caso deniega su hook principal, y la segunda línea (`reference-transaction`) tiene casos propios. **La tabla de referencia va por rangos de versión de Git**, y una versión que no cubre ninguna fila falla por ese motivo concreto. El informe imprime la versión de Git del runner | Arquitecto |
| D7 | **Una sola fuente de la lista**: el ejecutor vive en el testkit. Cuando `crates/policy` publique su lista versionada (US-GRD-004), la comparación pasa a sus tests (con el testkit como dev-dependency) y la tabla `REFERENCE` del testkit se borra | Arquitecto |
| D8 | **El contador de procesos de hook por comando entra en el núcleo**: sale casi gratis de las sondas y es el gate determinista de ADR-GRD-002 § 5. **`post-index-change` queda fuera del gate**, porque su número depende de si Git encuentra el índice "racy" (con carga, el mismo `rebase` dio 9 y 10). Guardrails solo lo instala si el usuario ya lo tenía (ADR-GRD-001 § 2) | Arquitecto |
| D9 | **Instalador de referencia, solo en los tests**: como el producto no existe, el arnés se prueba con un instalador que sigue los pasos de ADR-GRD-001 § 4 con archivos y el Git CLI, en un proceso hijo (el propio binario de test relanzado). Sus variantes rotas son los tests negativos. No es código de producto ni un diseño para US-GRD-001 | Arquitecto |
| D10 | **La INF queda en curso** (núcleo cerrado). Cada suite entra con su historia dueña (§ 6). Las fixtures del Alcance Técnico están todas en el núcleo: sin hooks, con hooks propios, husky, lefthook y pre-commit, **varios worktrees** (`Fixture::add_worktree`), **configuración por worktree** (`CoverageBlocker::WorktreeConfig`) e **inclusiones** (`Include` e `IncludeIfOnbranch`) | PO (ajuste: nombrarlas) |
| D11 | **Diferidos**: el banco de latencia p95 necesita el dispatcher real y un runner en reposo, así que **entra con US-GRD-001**, que entrega el dispatcher y su forma nativa (ADR-GRD-001 § 2). El job de CI con **Git 2.38 compilado** es un **predecesor que bloquea el merge de US-GRD-001** (ADR-GRD-001 Validación 13), no un diferido abierto | Arquitecto, PO (ajuste: ancla de la latencia) |
| D12 | **Regresión de SPIKE-GRD-001**: entra en el núcleo lo que se observa con Git crudo, sin el clasificador del producto: el renombrado con reftable por versión (D10, D11) y la matriz. Los casos que dependen de la decisión de la política (*prune* de `pack-refs` y `gc` D12–D14, borrado vía `HEAD` D09, alias de mayúsculas D08, D21 y F11, NFC D19b, `preparing`, valores `ref:`) entran con US-GRD-001 | PO |

## 2. Estructura

| Archivo | Qué hace |
|---|---|
| `crates/testkit/src/hooks.rs` | `PriorHooks` (`None`, `Own`, `Husky`, `Lefthook`, `PreCommit`, `Global`), `CoverageBlocker` (`WorktreeConfig`, `Include`, `IncludeIfOnbranch`), `Fixture::with_prior_hooks`, `add_coverage_blocker`, `commit_with_marker_rejected`, `common_dir`, `worktrees` y `hooks_path_state` |
| `crates/testkit/src/exceptions.rs` | `without_keys` hace la comparación con el parser de Git; `Exceptions::and`; la excepción `ConfigKeys` aplica el criterio semántico |
| `crates/testkit/src/cut.rs` | `CutPoint`, `points`, `trip`, `trip_during`, `Exit`, `Sweep`, `EndState`, `Baseline` y `SweepReport` |
| `crates/testkit/src/interceptability.rs` | `Lab` (repo, remoto bare y sondas), `catalog`, `observe`, `derive`, `compare`, `REFERENCE`, `cost_commands`, `count_hooks`, `COST_TABLE` y `restrict` |
| `crates/testkit/tests/hooks_harness.rs` | Instalador de referencia (hijo) y tests de huella, encadenado, cortes, sensibilidad y fixtures |
| `crates/testkit/tests/interceptability.rs` | Matriz, reftable, cobertura de la tabla, reglas de derivación y coste por comando |

Todos los tests llevan el prefijo `repo_intact::`, así que ya forman parte del gate de CI `repo-intact` en los tres SO (`.github/workflows/repo-intact.yml`). No hace falta ningún workflow nuevo.

## 3. Huella (Validación 1) y encadenado (Validación 2)

- **Tras instalar** se admiten solo `Exceptions::guardrails_install` (la clave `core.hooksPath` en el `config` común, la carpeta `gitraptor/` y los tiempos del directorio común) más el perfil.
- **Tras desinstalar**, comparado con el estado antes de instalar, se admite solo `Exceptions::guardrails_uninstalled`: el `config` puede tener otro inodo y otros tiempos, pero tiene que decir lo mismo según el criterio de D2. El valor efectivo y el nivel de cada worktree son los de antes.
- **Encadenado**: con cada tipo de hook previo, el linter rechaza lo mismo antes de instalar, después y tras desinstalar, en el worktree principal y en uno enlazado. husky no corre en los worktrees enlazados, con Guardrails o sin él (ADR-GRD-001 § 6).

## 4. Cortes (NFR-12; Validación 3)

`Sweep` recibe la fixture, una preparación opcional, la transacción y la recuperación (las dos en otro proceso), los puntos y dos estados finales:

| Barrido | "Sin hacer" | "Hecho" |
|---|---|---|
| Instalación | Frente al repo prístino, con `guardrails_uninstalled` y el perfil; ningún worktree apunta a la carpeta | Frente al repo prístino, con `guardrails_install` y el perfil; cada worktree ve la clave local de Guardrails y los tres dispatchers obligatorios |
| Desinstalación | Frente al inicio (instalado), con el perfil y los tiempos del directorio común; sigue instalada | Frente al repo prístino, con `guardrails_uninstalled` y el perfil |

Los puntos de la instalación de referencia son `precheck`, `journal`, `folder-temp`, `folder-rename`, `key` (también con `during`), `verify` y `close`. Los de la desinstalación son `journal`, `key` (también con `during`), `folder` y `close`. US-GRD-001 y US-GRD-003 declaran los suyos, y el arnés comprueba que su código los recorre todos.

Si un barrido falla, el informe nombra el escenario, el punto de corte, la ruta y el tipo de cambio frente a cada estado final, más el fallo de la sonda semántica (Verificación manual de la INF).

## 5. Matriz (ADR-GRD-002 Validación 1, 2 y 12) y coste por comando (Validación 13)

- **Ejecutor**: por cada caso, `Lab` crea un repo con `main`, `feat` y un remoto bare. Prepara el caso sin hooks e instala sondas `sh` en todos los hooks de cliente, con `core.hooksPath` local en el repo temporal. Después ejecuta la operación con Git crudo, denegando el hook principal. La sonda solo usa constantes, builtins de `sh` y `cat`.
- **Efectos residuales**, leídos sin ejecutar hooks: `r` refs/heads y `HEAD` (local y remoto), `i` índice, `w` working tree, `s` operación en curso y lista de worktrees.
- **Momento**: el hook no corrió → C. Corrió, denegó y no dejó nada → A. Corrió y dejó efectos → B. Corrió y la operación pasó → no impedida (anomalía).
- **Catálogo** (23 casos con el backend de archivos y 2 con reftable): commit; commit con la segunda línea; los saltos `--no-verify`, `-c core.hooksPath` y `GIT_CONFIG_COUNT`; push; push `--no-verify`; `send-pack`; force-push; borrar rama remota y local; `update-ref -d`; renombrar la base; renombrar sobre la base; `reset --hard`; rebase; rebase `--no-verify`; `pull --rebase`; merge; merge fast-forward; crear worktree con rama nueva y sin ella; borrar worktree.
- **Coste por comando**: commit, `switch` (rama nueva y existente), `rebase` de 3 commits, `fetch` de una ref, `stash` y `stash pop`. Se mide con todas las sondas de `COST_HOOKS` y con las del conjunto mínimo (`pre-push`, `pre-rebase` y `reference-transaction`). Se exige que el mínimo coincida con el recuento completo restringido a esos hooks.

**Hallazgo (2026-10-05)**: con **reftable**, `git branch -M feat main` **sí** ejecuta `reference-transaction` en `prepared` con Git 2.50.1 (macOS, en local) y con Git 2.55.0 (runners de CI de Linux y macOS). La denegación deja `feat` borrada (B, igual que con el backend de archivos). SPIKE-GRD-001 midió C (ningún hook) con 2.56.0, con una preparación algo distinta (un commit en `feat` antes del renombrado). En todas las versiones la operación se publica como no impedible y solo cambia el motivo (B o C). La tabla lo recoge con dos filas por rango de versión: 2.50.1–2.55.0 y 2.56.0 en adelante. Ninguna de esas versiones es de la matriz (2.38 y la última estable): son **evidencia local y de CI**, no un gate de la matriz. Una versión fuera de los dos rangos falla con el motivo "no está en la lista de referencia". **Pendiente**: que el Arquitecto propague el hallazgo a ADR-GRD-002 (§ 1, § 3 y la Enmienda) y confirme la fila de 2.56.0 con la preparación del ejecutor. Este PR no toca el ADR.

## 6. Plan de pruebas (criterio de la INF → test)

| Criterio de INF-GRD-001 | Test | Estado |
|---|---|---|
| Sensibilidad: un instalador que cambia un byte de un hook previo hace fallar el arnés | `sensitivity_install_changing_a_prior_hook_byte_is_detected` | Núcleo, verde en macOS |
| Sensibilidad: un instalador que escribe en la config global hace fallar el arnés | `sensitivity_install_writing_global_config_is_detected` | Núcleo, verde en macOS |
| Huella: una desinstalación que deja rastro se detecta | `sensitivity_uninstall_leaving_a_trace_is_detected` | Núcleo, verde en macOS |
| Huella tras instalar y desinstalar, con cada gestor (Validación 1) | `footprint_install_uninstall_every_prior_hooks`, `footprint_prior_hooks_local_value_is_restored_at_its_level` | Núcleo; la suite del producto es de US-GRD-001 y US-GRD-003 |
| Criterio semántico del `config` | `config_criterion_is_semantic_but_ordered` | Núcleo |
| Interrupción: un corte en cada punto deja el repo completo o idéntico | `cuts_install_is_complete_or_identical_at_every_point`, `cuts_uninstall_is_complete_or_identical_at_every_point`, `sensitivity_install_without_recovery_fails_the_sweep`, `sensitivity_undeclared_or_unreached_cut_points_fail_the_sweep`, `cut_point_protocol_round_trips` | Núcleo; la suite del producto es de US-GRD-003 |
| Encadenado: con cada gestor, el hook previo rechaza lo mismo | `chaining_prior_hook_rejects_the_same_before_and_after` | Núcleo; la suite del producto es de US-GRD-002 |
| Fixtures de cobertura de worktrees y de gestores | `fixtures_coverage_blockers_are_visible_per_worktree`, `fixtures_managers_leave_what_the_spike_observed` | Núcleo |
| Matriz: la lista publicada coincide con Git crudo | `matrix_matches_the_published_list`, `matrix_reftable_rename_rows_still_hold`, `matrix_catalog_covers_the_reference_list`, `matrix_derivation_rules` | Núcleo; la comparación con la lista del binario es de US-GRD-004 |
| Coste por comando (gate determinista) | `cost_hook_processes_per_command_match_the_table`, `cost_version_parsing_and_ranges` | Núcleo; la tabla tiene Git 2.50.1 (macOS) y se amplía con cada versión medida |
| Latencia p95 (NFR-GRD-04) | — | Diferido: entra con US-GRD-001 (D11) |
| Pérdida externa, integridad, actualización, carpeta hostil, registro | — | Suites de US-GRD-004, US-GRD-001, US-GRD-003 y US-GRD-005 |
| Gate: una historia anterior no queda bloqueada por una suite que aún no existe | El gate `repo_intact::` solo ejecuta los tests que existen; cada suite se registra por su prefijo al entrar | Núcleo |

## 7. Verificación realizada

- macOS arm64, Apple Git 2.50.1: `cargo clippy --all-targets -- -D warnings` y `cargo test --workspace` en verde. El test del coste se repitió 4 veces seguidas, con los otros tests en paralelo, sin variaciones tras sacar `post-index-change` del gate.
- CI (`repo-intact` y `lint and test`), runners de Linux y macOS con Git 2.55.0. La primera ejecución falló **por el motivo diseñado**: 2.55.0 no estaba en `COST_TABLE` ni en la fila de reftable, y el test imprimió los recuentos medidos. Los dos runners dieron valores idénticos, y con ellos se añadieron las filas de 2.55.0 (con el estado `preparing`, que ya existe en 2.55). El resto de la matriz y las 15 pruebas del arnés pasaron a la primera en los dos SO.
- **Pendiente: etapa de validación multiplataforma.** Linux solo se verificó en el runner de CI. En Windows las suites de este arnés **no llegaron a ejecutarse**: `cargo test` se detiene antes, en tres tests de otro crate que ya fallan en `main`. En Windows el gate `repo-intact` sigue siendo `continue-on-error`; fmt y clippy son bloqueantes. Si la versión de Git de un runner no está en `COST_TABLE`, el test falla diciéndolo e imprime los recuentos medidos para añadir la fila.

## 8. Fuera de alcance y pendientes

- La capa de hooks del producto: US-GRD-001.
- El check de CI que busca `GITRAPTOR_TEST_CUT` en el binario de release: US-GRD-001, cuando exista la feature `test-cuts`.
- El job de CI con Git 2.38 compilado, y la última estable si el runner no la trae: predecesor del merge de US-GRD-001.
- El banco de latencia p95 por evaluación gobernada y de la vía rápida: con US-GRD-001.
- Los casos de regresión de SPIKE-GRD-001 que dependen de la decisión de la política: con US-GRD-001 (D12).
- Propagar a ADR-GRD-002 el hallazgo de reftable con Git 2.50.1 (§ 5).
