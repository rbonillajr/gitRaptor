---
id: DS-US-GRD-002
title: "Dev Spec — US-GRD-002: los hooks que el repo ya tenía siguen funcionando al protegerlo"
type: dev-spec
status: implemented
feature: guardrails
domain: GRP
story: US-GRD-002
created: 2026-10-08
updated: 2026-10-08
related:
  stories: [US-GRD-002, US-GRD-001, US-GRD-003, US-GRD-004, US-GRD-006, INF-GRD-001, SPIKE-GRD-001]
  adrs: [ADR-GRD-001, ADR-GRD-005, ADR-GRD-007, ADR-GRP-016]
  rules: [BR-EDGE-002, BR-CONS-005, BR-AUTH-002]
  nfrs: [NFR-01, NFR-07, NFR-GRD-01]
tags: [guardrails, hooks-git, hooks-previos, encadenado, husky, lefthook, pre-commit, nfr-01]
---

# Dev Spec — US-GRD-002: los hooks que el repo ya tenía siguen funcionando al protegerlo

Plano compacto (AADD ligero) de [US-GRD-002](../user-stories/US-GRD-002-hooks-previos-respetados.md), construido en el mismo PR que [US-GRD-003](./US-GRD-003-retirar-proteccion-sin-rastro.md) porque las dos tocan el mismo instalador. El contrato lo fijan [ADR-GRD-001](../../../../architecture/decisions/ADR-GRD-001-capa-hooks-instalacion.md) § 2, § 5, § 6 y § 7, con la Enmienda del 2026-10-08 que deja esta historia, y [SPIKE-GRD-001](../research/SPIKE-GRD-001-resultados.md) § 5 y § 6.

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-08), validada por el Arquitecto** (`nassa-architect:architect`). La columna de la derecha recoge los ajustes que pidió, ya incorporados. El coordinador aprobó el plan con tres notas (entrada, salida y entorno idénticos para el hook previo; el orden "GitRaptor decide primero"; barrido de cortes de la desinstalación), cubiertas por tests.

| # | Decisión | Ajuste de la validación |
|---|---|---|
| D1 | **El stub nativo `raptor-hook` encadena el hook previo**, no `raptor hook`. Tras el "permite" de `raptor`, tras la vía rápida o en el respaldo que deja pasar, el stub ejecuta `<prior>/<hook>` sin shell, con los mismos argumentos, la misma entrada (la que ya leyó, reenviada por una tubería; si no la leyó, heredada), el **entorno original de Git** y el cwd de Git; su salida pasa tal cual y su código de salida es el resultado. Si `raptor` deniega, no lo ejecuta. Motivo: `raptor hook` corre con `env_clear` y no tiene el entorno que el hook previo necesita; el respaldo sin `raptor` ya exigía encadenar en el stub | El stub tiene exactamente tres `Command::new`: `raptor`, el hook previo y `/bin/sh` para un script sin `#!` (ENOEXEC; lo hace igual que Git, con el script como argumento, nunca `-c`). La ruta del previo sale solo de la constante `prior` más un nombre fijo. Lo comprueba `guard_boundary`. `reference-transaction` fuera de `prepared` encadena si hay hook previo. Enmienda de ADR-GRD-001 § 2 y § 7 en este PR |
| D2 | **Directorio previo** = el valor efectivo de `core.hooksPath` antes de instalar (la última entrada de `--show-scope --get-all` desde el worktree principal, con su nivel), o `<común>/hooks` si no hay. Relativo se queda relativo (Git lo resuelve contra la raíz de cada worktree). Con un valor previo, los hooks de `.git/hooks` no se encadenan, porque Git tampoco los ejecutaba | `~/` solo se expande si el `HOME` del daemon coincide con el home de la cuenta (`getpwuid`); si no, `chain-impossible`. Un cambio posterior de `HOME` no se sigue (declarado) |
| D3 | **Conjunto de dispatchers** = los 5 de la plantilla 2 ∪ un dispatcher de **solo encadenado** por cada hook previo con nombre de githooks(5) (unión de todos los worktrees si el valor es relativo). `journal.chained` guarda la lista y la actualización en el sitio la conserva. Sin cambio de plantilla: las instalaciones de US-GRD-001 no tenían hooks previos | La lista de nombres de solo encadenado es fija en el stub; un nombre fuera de ella no encadena |
| D4 | **`chain-impossible`** (`InstallBlocker::ChainImpossible`) sustituye a `prior-hooks` como bloqueo: valor previo con tabulador, salto de línea o control, vacío o que empieza por `-`, `~usuario`, `%(prefix)` o `~/` con otro home, que resuelve dentro de `gitraptor/` (en cualquier worktree), **definido más de una vez a nivel local**, definido por un `include`, o una lectura de la configuración que falla. No se escribe nada y el intento queda en `last_refusal` | El valor relativo se comprueba contra cada worktree; la causa "varias entradas locales" la añadió el Arquitecto (la desinstalación no podría recrearlas) |
| D5 | **El permiso enumera los hooks previos** (`GuardPlan.prior`: valor previo, nivel, directorio y nombres, saneados como texto no confiable) y dice que se conservan y que solo corren si GitRaptor permite. El "revertir a mano" restaura el valor local previo. Campo y bloqueo nuevos = cambio de forma: capacidad `guard.prior-hooks`; sin ella, el canal responde `prior-hooks` y omite el campo | SEC-GRD-06 |

## 2. Forma

| Pieza | Ubicación |
|---|---|
| Detección y causas de `chain-impossible` (función pura `from_entries`) | `crates/core/src/guardrails/prior.rs` |
| Plan, instalación y actualización con el conjunto nuevo (`Folder::build` común) | `crates/core/src/guardrails/install.rs` |
| `journal.prior.dir` y `journal.chained` (con `serde(default)`) | `crates/core/src/guardrails/journal.rs` |
| Encadenado, `Hook::Chain` y respaldo que devuelve "pasa/no pasa" | `apps/cli/src/bin/raptor-hook.rs` |
| `InstallBlocker::ChainImpossible`, `PriorHooks`, `HooksPathLevel`, capacidad | `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` |
| Forma por capacidad en el canal | `crates/core/src/channel/conn.rs` (`guard_shape_plan`, `guard_blockers`) |
| Explicación y mensajes en/es | `apps/cli/src/guard.rs` (`explain`), `apps/cli/i18n/{en,es}/guard.txt` |

## 3. Pruebas

Repos, remotos, perfiles y daemons temporales (NFR-01); `raptor`, `raptor-hook` y Git reales; sin esperas fijas.

| Escenario o criterio | Test |
|---|---|
| E1 · El permiso informa de los hooks previos | `guard_us_grd_002::repo_intact::e1_the_permission_names_the_prior_hooks` |
| E2 y E3 · Linter propio y Guardrails actúan los dos; el hook conserva su contenido (huella con las excepciones de instalar) | `…::e2_e3_an_own_hook_and_guardrails_both_act` |
| E2 · `core.hooksPath` local previo (husky): corren `pre-commit` y `post-commit` (solo encadenado) con el entorno original de Git | `…::e2_a_prior_hooks_path_keeps_running_with_the_original_environment` |
| Misma entrada (`reference-transaction` en `prepared` y `committed`) y resultado respetado (`pre-push` previo que rechaza) | `…::a_chained_hook_gets_the_same_input_and_its_result_is_respected` |
| Argumentos, entrada y cwd de `pre-push`; la edición del mensaje por `commit-msg` se conserva (nota 1 del coordinador) | `…::a_chained_hook_gets_argv_stdin_cwd_and_its_edits_survive` |
| GitRaptor decide primero: si deniega, el hook previo no corre y se ve el motivo (nota 2) | `…::guardrails_decides_first_and_a_deny_never_runs_the_prior_hook` |
| Un hook previo sin `#!` corre con `sh` (como Git); uno con un intérprete que no existe hace fallar la operación | `…::a_prior_hook_without_shebang_runs_and_a_broken_one_fails_the_operation` |
| E4 · Si no se puede encadenar, no se instala nada (huella intacta, `chain-impossible`) | `…::e4_a_prior_hook_that_cannot_be_chained_installs_nothing`; también `guard_us_grd_001::criteria::repo_intact_prior_hooks_refuse_the_install_without_changes` |
| Causas de `chain-impossible`, unión de worktrees, nivel global, `~/` | `crates/core` `guardrails::prior::tests::*` |
| Frontera del stub (tres `Command::new`, sin `-c`) | `crates/core/tests/guard_boundary.rs` |

## 4. Pendientes y fuera de alcance

| Pendiente | Dueño |
|---|---|
| Un `core.hooksPath` **global** previo de punta a punta: el daemon toma `HOME` de la base de usuarios, no del home temporal del test, así que solo lo cubren los tests de `prior` | Declarado |
| Diagnóstico `hook-previo-no-encadenado` y regeneración cuando aparece un hook previo después de instalar (ventana declarada en ADR-GRD-001 § 2) | US-GRD-004 |
| Quitar la variable del token de excepción del entorno del hook previo (aún no hay token) | US-GRD-006 |
| Gestores reales (husky 9, lefthook 2, pre-commit 4) de punta a punta: se prueban sus formas (`core.hooksPath` relativo, scripts en `.git/hooks`), como en SPIKE-GRD-001 | Etapa de validación multiplataforma / dogfooding |
| Windows: un hook previo que es un script y no un PE; Linux en máquina real | **Pendiente: etapa de validación multiplataforma** |

## Estado de la implementación (2026-10-08)

Implementado en: PR #193. Verificado en macOS arm64 (Apple Git 2.50.1); en CI, también con Git 2.38.5 en Linux.
