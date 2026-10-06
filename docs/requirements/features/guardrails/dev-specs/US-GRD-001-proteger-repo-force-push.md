---
id: DS-US-GRD-001
title: "Dev Spec — US-GRD-001: proteger un repo y denegar el force-push y el borrado de la rama base"
type: dev-spec
status: approved
feature: guardrails
domain: GRP
story: US-GRD-001
created: 2026-10-05
updated: 2026-10-05
related:
  stories: [US-GRD-001, US-GRP-001, US-GRP-012, INF-GRD-001, SPIKE-GRD-001, US-GRD-002, US-GRD-003, US-GRD-004, US-GRD-005, US-GRD-006]
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, ADR-GRD-007, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009]
  rules: [BR-AUTH-001, BR-AUTH-002, BR-EDGE-001, BR-EDGE-002, BR-CALC-001, BR-WF-002, BR-CONS-003]
  nfrs: [NFR-01, NFR-07, NFR-10, NFR-12, NFR-GRD-01, NFR-GRD-04]
tags: [guardrails, esqueleto-andante, hooks-git, minimo-seguro, force-push, dispatcher-nativo, instalacion, canal-autenticado, modo-degradado, coste-windows]
---

# Dev Spec — US-GRD-001: proteger un repo y denegar el force-push y el borrado de la rama base

Plano compacto (AADD ligero) de [US-GRD-001](../user-stories/US-GRD-001-proteger-repo-force-push-bloqueado.md). El contrato lo fijan [ADR-GRD-001](../../../../architecture/decisions/ADR-GRD-001-capa-hooks-instalacion.md) § 1 a § 4 y § 7, [ADR-GRD-002](../../../../architecture/decisions/ADR-GRD-002-operaciones-interceptables.md) § 1 a § 4, [ADR-GRD-003](../../../../architecture/decisions/ADR-GRD-003-motor-decision-contrato.md) § 1 a § 4 y [ADR-GRD-007](../../../../architecture/decisions/ADR-GRD-007-acciones-reservadas-excepcion.md) § 1, con las enmiendas de [SPIKE-GRD-001](../research/SPIKE-GRD-001-resultados.md) y la del 2026-10-05 que deja esta historia (§ 8). El arnés es el de [INF-GRD-001](./INF-GRD-001-arnes-hooks.md).

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-05), validada por el Arquitecto** (`nassa-architect:architect`) **y el PO** (`nassa-aadd:product-owner`). La columna de la derecha recoge los ajustes que pidieron, ya incorporados.

| # | Decisión | Ajuste de la validación |
|---|---|---|
| D1 | **Dispatcher nativo en los tres SO y para los tres hooks** (ADR-GRD-001 § 2, E-01-7). Un binario mínimo `raptor-hook` (solo `std`, junto a `raptor`) se **copia tal cual** a `<común>/gitraptor/hooks/<hook>`, sin extensión, y sus constantes van en `<común>/gitraptor/dispatch.conf`, que el stub localiza desde su propia ruta de ejecutable (la da el kernel, no el entorno). No se genera ningún `sh`. **Motivo medido** (§ 7): en Windows un dispatcher `sh` cuesta ≈ 43 ms por invocación y el nativo ≈ 6 ms; Git for Windows ejecuta un PE sin extensión como hook | Arquitecto: el stub abre `dispatch.conf` sin seguir enlaces, solo si es un archivo regular, acotado y con parseo estricto; comprueba que la constante `common` es la ruta canónica de su propia carpeta (si no, deniega: señal M-02); `template` versionado (actual y anterior); en release se firma (hardened runtime y Authenticode); excepción con nombre para su `Command::new` |
| D2 | **Respaldo sin `raptor`** (ADR-GRD-001 § 3) dentro del stub, en Rust: `pre-push` y `pre-rebase` fail-closed con el mensaje de recuperación; `reference-transaction` en `prepared` sale con 1 si una línea borra `refs/heads/*` o un `HEAD` (salvo el *prune* de `pack-refs`) o está malformada; si no, aviso "protección inactiva" y 0 | Arquitecto: el stub interpreta la salida de `raptor`: 0 permite, 1 deniega y cualquier otra cosa (señal, pánico, código inesperado) es un error interno con la tabla del § 3 |
| D3 | **Alcance de la instalación**: solo repos **sin hooks previos que encadenar**. Con un hook ejecutable con nombre de hook de Git en `<común>/hooks` (y `.exe` en Windows; nunca `.sample`) o un `core.hooksPath` en cualquier nivel, **no se instala** y se explica (`prior-hooks`, "lo cubre US-GRD-002"). Tampoco con los bloqueos de ADR-GRD-001 § 5, una constante no representable, una carpeta `gitraptor/` desconocida, un repo bare o un repo no observado | PO: el estado no cambia; el intento rechazado se guarda con su causa y se ve en `guard.status` (`last_refusal`); un rechazo **no** cuenta como denegar el permiso. Arquitecto: la lectura incluye los niveles global y de sistema y excluye `GIT_CONFIG_*`; ante la duda, cuenta como hook |
| D4 | **Métodos del canal** (protocolo 5): `guard.plan` y `guard.status` (lectura), `guard.install` y `guard.decline` (**reservados**, controles 1 a 3 de ADR-GRP-005 § 6; rechazados desde `raptor-mcp`) y `guard.evaluate` (cliente del hook; nunca para `raptor-mcp`). `RepoWrite::Guardrails`: solo `guard.install` puede declararla; se recupera con su propio diario, no con un snapshot | Arquitecto: `only_protected_paths_write` lo comprueba |
| D5 | **Permiso** (BR-AUTH-002): `raptor guard install <ruta>` explica y pregunta. "Sí" → `guard.install`; "no" → `guard.decline` (`permission = denied`, `offer = false`); sin respuesta (sin terminal, fin de entrada o línea vacía) → no se escribe nada y el comando sale con error y un mensaje claro. `--yes` responde "sí" pero **no** salta los controles del comando reservado. Volver a ejecutar el comando es "activarlo a mano" | PO: la explicación dice también qué pasa si no se autoriza, que push y rebase se bloquean si GitRaptor falta y que cubre todos los worktrees de ese repo y ningún otro; el revertir a mano son pasos copiables. **Condición para el Cockpit**: una superficie que ofrezca el permiso por iniciativa propia tratará "sin respuesta" como "no" y no insistirá |
| D6 | **Capa de escritura de Guardrails** en `crates/git/src/guard_write/` (ADR-GRD-001 § 7): `core.hooksPath` solo en el `config` común con `git config --no-includes --file` (perfil propio y cerrado en `invoke.rs`, sin `HOME`, `GIT_CONFIG_NOSYSTEM=1` y `GIT_CONFIG_GLOBAL` nulo), sus lecturas (`--show-scope --show-origin --get-all` y los `includeIf "onbranch:"`), y archivos solo dentro de `<común>/gitraptor/` (descriptor del directorio, `O_NOFOLLOW`, temporal aleatorio exclusivo, `fsync`, renombrado atómico con `NOREPLACE`, borrado solo de lo listado, `dev/inode` comprobado) | Arquitecto: perfil propio, sin relajar el de la Time Machine; comprobaciones estáticas ampliadas en `crates/git` y `crates/core` |
| D7 | **Diario en el almacén por repo** (`store_meta`): `guardrails.journal` (JSON: `installing` → `confirmed`, hashes SHA-256 de los archivos, `dev/inode` de la carpeta y del `config`, constantes), `guardrails.permission` y `guardrails.last_refusal`. **Recuperación al arrancar**: con la clave ajena se borra lo listado (también una carpeta sin identidad registrada si todos sus archivos tienen el hash del diario) y el repo queda idéntico; con la clave nuestra se verifica y se confirma | Arquitecto: el diario `confirmed`, la rama base y `permission = granted` en **una sola transacción**; al arrancar se vuelve a publicar el registro y la instantánea. El barrido de cortes del producto queda en US-GRD-003 (sin feature `test-cuts` en esta historia) |
| D8 | **Evaluación pura** en `crates/policy::guard`: `minimum.force-push` (cualquier ref remota gobernada; objeto remoto ausente, no ancestro o clon superficial = forzado) y `minimum.base-branch-delete` (local o remota, sobre la unión de ramas base; alias por NFC y plegado en todas las líneas). Causas `shallow-history` (`historia-superficial`) y `rename-onto-base` (`renombrado-sobre-base`, con la rama origen y su oid desde el reflog de `HEAD`). El contrato `Decision` completo (`decisionId`, `effect`, `appliedEffect`, `reasons[]`, `exception`, `configStatus`, `configRef`) vive en `crates/api` | — |
| D9 | **Vía rápida en el stub** (refs no gobernadas, pseudo-refs de gitglossary, valores `ref:` de `HEAD` y *prunes* de `pack-refs`), con la clasificación en un archivo solo-`std` (`crates/policy/src/guard/fastpath.rs`) que compilan la política y el stub. `raptor hook` valida estrictamente, resuelve `HEAD` desde la línea o el `HEAD` del `GIT_DIR` de la transacción (nunca el cwd), exige que el repo de la transacción sea el del dispatcher (M-02) y calcula los hechos de `pre-push` con gix sin objetos de reemplazo, sin `info/grafts` y sin commit-graph | Arquitecto: la vía rápida va en el stub, no en `raptor hook` (pendiente fija de ADR-GRD-002 § 5); lo malformado siempre va a `raptor`. La comprobación M-02 en el cliente es válida pero contradice el § 4 de ADR-GRD-003: enmienda (§ 8) |
| D10 | **Canal autenticado**: ruta fija del dispatcher; **antes de enviar nada** el cliente exige que el ejecutable del par (pid del kernel) sea el mismo archivo que el propio cliente (si no, deny `channel-not-authentic`) y, en el `hello`, el id de instancia del dispatcher (si no, modo degradado `instance-mismatch`). Daemon inalcanzable o de otro protocolo → modo degradado `daemon-unreachable`. **Sin arranque bajo demanda desde el hook** | Arquitecto: identidad de archivo en lugar de firma o huella, y sin arranque bajo demanda: enmienda (§ 8). Riesgo declarado: tras un `brew upgrade` con el daemon viejo vivo, el hook puede ver un ejecutable distinto (en Linux) y denegar hasta que el daemon se reemplace |
| D11 | **Modo degradado**: la misma función, mínimo forzado, rama base = unión {`main`, rama principal, instantánea `<estado>/guardrails/<repo_id>.json`}. Lecturas con gix aislado del entorno y sin objetos de reemplazo; el cliente nunca abre el SQLite; un aviso por stderr | Arquitecto: el spool y la ventana degradada son de US-GRD-005 |
| D12 | **Rama base al instalar** (Q-GRD-23; ADR-GRD-004 § 3.5): sin configuración del equipo en la copia de la rama principal, se confirma `main` (suelo `absent`); con ella, no se lee, queda `base-unconfirmed` y se protege {`main`, rama principal} | — |
| D13 | **Mensajes** (M-05): plantilla fija por código en los catálogos en/es; parámetros `«…»` saneados (Cc, Cf, Zl y Zp neutralizados; `Untrusted::sanitized` amplía ahora Zl, Zp y Cf) y acotados a 120 caracteres; nunca mencionan la excepción ni cómo desactivar la protección. Textos del PO | PO: textos aprobados (§ 6) |
| D14 | **Lista de lo no impedible** versionada en `crates/policy` según el backend de refs (reftable añade renombrar la rama base). Se muestra en la explicación y en `guard.status` | — |
| D15 | **Job de CI con Git 2.38.5 compilado** (Linux, en caché) dentro de este PR, como commit `ci:` propio, en vez de un PR previo | Ajuste respecto al Arquitecto (pedía un PR `ci:` previo): sin las suites de esta historia el job no tiene nada que ejecutar, y es el gate que bloquea su merge (INF-GRD-001 D11). Se anota en el PR |
| D16 | **Banco de latencia como informe, no como gate** (`latency_report`, ignorado); el gate determinista sigue siendo el número de procesos por comando de INF-GRD-001 | Arquitecto; el techo de 150 ms sigue como ⚠️ ASSUMPTION por confirmar por Rene |

## 2. Forma

| Pieza | Ubicación |
|---|---|
| Contrato: `Decision`, `GuardPlan`, `GuardStatus`, `EvaluateParams`, métodos y `GUARD_REJECTED` | `crates/api/src/{guard,methods,rpc}.rs` |
| Evaluación pura, entrada estricta, refs gobernadas, normalización, lista de lo no impedible | `crates/policy/src/guard/{mod,input,refs}.rs` |
| Vía rápida solo-`std` (compartida con el stub) | `crates/policy/src/guard/fastpath.rs` |
| Lecturas del hook: ancestro, backend de refs, `core.ignoreCase` | `crates/git/src/guard_read.rs` |
| Capa de escritura de Guardrails | `crates/git/src/guard_write/` y el perfil `GuardSubcommand`/`GuardRead` de `invoke.rs` |
| Instalación, diario, recuperación, registro, instantánea, evaluación en el daemon y cliente del hook | `crates/core/src/guardrails/` |
| Claves del almacén por repo | `crates/core/src/profile/guard_store.rs` |
| Métodos en el canal y en el bucle del daemon | `crates/core/src/channel/conn.rs`, `crates/core/src/daemon/` |
| `raptor guard install|status`, `raptor hook` y mensajes | `apps/cli/src/guard.rs`, `apps/cli/i18n/{en,es}.txt` |
| Dispatcher nativo | `apps/cli/src/bin/raptor-hook.rs` |

**`dispatch.conf`**: una línea `clave<TAB>valor` por constante (`template`, `raptor`, `repo`, `common`, `channel`, `instance`, `state`, `prior`), UTF-8, sin `\n`, `\r`, `\t` ni NUL. Una comilla simple o un `$(…)` son caracteres literales (no hay shell). El stub pasa a `raptor hook` las constantes como argumentos y `"$@"` tras `--`, con un entorno construido desde cero: `GIT_DIR` y `GIT_INDEX_FILE` si Git los fijó, `LC_ALL`, `LC_MESSAGES` y `LANG` (solo eligen el idioma de los mensajes) y, en Windows, `SystemRoot`.

## 3. Flujo

1. `raptor guard install <ruta>` → `guard.plan` → explicación → pregunta (D5). Con bloqueos, no se pregunta: `guard.install` los vuelve a comprobar, guarda el intento y no instala nada.
2. `guard.install` (reservado, auditado antes de leer la ruta) → el bucle del daemon repite las comprobaciones → diario `installing` → carpeta temporal con los tres dispatchers, `dispatch.conf` y `manifest.json` → renombrado atómico a `gitraptor/` → diario con la identidad de la carpeta → `core.hooksPath` absoluto (punto de commit) → diario con la identidad del `config` → verificación del valor efectivo en cada worktree → transacción final (diario `confirmed`, rama base, permiso) → registro e instantánea.
3. Git ejecuta el stub → salida inmediata fuera de `prepared` → vía rápida → `raptor hook` → `guard.evaluate` en el hilo de la conexión (o modo degradado) → mensaje y código de salida.

## 4. Pruebas

Repos, remotos, perfiles y daemons temporales (NFR-01); `raptor`, `raptor-hook` y Git reales; huella del testkit; sin esperas fijas (la salida de cada proceso, el pid del daemon y el texto de la pregunta en la terminal son las señales). La suite de punta a punta (`apps/cli/tests/guard_us_grd_001.rs`) se niega a correr sin *debug assertions*, porque sin ellas `raptor` ignoraría el perfil temporal.

| Escenario o criterio | Test |
|---|---|
| E1 · El desarrollador protege un repo con su permiso (explicación completa, respuesta "y", estado "Solo hooks", `main` confirmada, huella solo con la clave y la carpeta; solo los tres dispatchers, ADR-GRD-001 Val. 15) | `repo_intact::e1_the_developer_protects_a_repo_with_permission` |
| E2 · Force-push denegado por el mínimo (`-f`, `--force-with-lease`; regla nombrada en en y es; remoto intacto) | `repo_intact::e2_a_force_push_is_denied_by_the_safe_minimum` |
| E3 · Borrar la rama base, local y remota (`branch -D`, `update-ref -d`, `push :main`, `push --delete`) | `repo_intact::e3_deleting_the_base_branch_is_denied` |
| E4 · Commit permitido (y push fast-forward y borrar una rama de trabajo) | `repo_intact::e4_a_commit_outside_the_minimum_runs` |
| E5 · Sin permiso: sin respuesta (sin terminal y línea vacía) no escribe nada; "no" guarda la denegación y no se vuelve a ofrecer; después se activa a mano | `repo_intact::e5_without_permission_nothing_is_installed_nor_asked_again` |
| E6 · El permiso de un repo no alcanza a otro (huella de "otro" idéntica) | `repo_intact::e6_the_permission_of_one_repo_does_not_reach_another` |
| ADR-GRD-002 Val. 3 · `+ref`, `--force-with-lease`, comodines, `replace --graft` (H-05), objeto remoto ausente, clon superficial | `criteria::repo_intact_force_push_variants_are_denied`, `…_a_remote_tip_missing_locally_is_a_force_push`, `…_a_push_from_a_shallow_clone_is_denied_as_shallow` |
| ADR-GRD-002 Val. 4 y 5; ADR-GRD-001 Val. 4 · worktree creado tras instalar, `update-ref -d HEAD`, `GIT_DIR` cruzado, alias `Main` (APFS) | `criteria::repo_intact_base_branch_deletion_through_worktrees_head_and_aliases` |
| ADR-GRD-002 Val. 10; ADR-GRD-001 Val. 14 · `pack-refs` y `gc` pasan; la base empaquetada sigue protegida | `criteria::repo_intact_pack_refs_and_gc_pass_and_the_packed_base_stays_protected` |
| ADR-GRD-003 Val. 9 · modo degradado sin daemon | `criteria::repo_intact_degraded_mode_without_the_daemon` |
| ADR-GRD-003 Val. 6 · perfil recreado (`instance-mismatch`) y servidor impostor (`channel-not-authentic`, el cliente no envía nada) | `criteria::repo_intact_the_channel_is_authenticated` |
| ADR-GRD-001 Val. 7 · entorno hostil (`PATH`, `HOME`, `XDG_*`, `GITRAPTOR_PROFILE_DIR`; la inyección de bibliotecas rompería antes al propio Git y el stub limpia el entorno, comprobado por `guard_boundary`) | `criteria::repo_intact_a_hostile_environment_does_not_steer_the_hook` |
| ADR-GRD-001 Val. 5 y 14 · sin `raptor` | `criteria::repo_intact_without_raptor_risky_hooks_fail_closed` |
| ADR-GRD-001 Val. 6 · error interno | `criteria::repo_intact_an_internal_error_fails_closed_only_where_risky` |
| ADR-GRD-001 § 2, M-02 · constantes de otro repo | `criteria::repo_intact_moved_dispatchers_deny` |
| ADR-GRD-004 Val. 10 · configuración del equipo → `base-unconfirmed` y unión {`main`, `trunk`} | `criteria::repo_intact_a_team_configuration_leaves_the_base_unconfirmed` |
| D3, BR-EDGE-002 (certificación) · hooks previos: no se instala, huella intacta, rechazo guardado sin denegar el permiso | `criteria::repo_intact_prior_hooks_refuse_the_install_without_changes` |
| ADR-GRD-001 Val. 8 · `gitraptor/` como enlace: no se escribe a través | `criteria::repo_intact_a_linked_folder_is_not_written_through` |
| ADR-GRD-001 § 4 · recuperación al arrancar (deshacer y confirmar) | `recovery::repo_intact_an_unfinished_install_without_the_key_is_undone_at_startup`, `…_with_the_key_is_confirmed_at_startup` |
| ADR-GRD-003 Val. 1, 2 y 4 · pureza, todas las razones, mínimo | `crates/policy` `guard::tests::*` |
| Entrada estricta y vía rápida (ADR-GRD-002 Val. 6, 11) | `crates/policy` `guard::input::tests::*`, `guard::tests::fast_path::*` |
| ADR-GRD-003 Val. 10 · mensajes saneados, etiquetados y acotados, sin la excepción | `apps/cli` `guard::tests::*`; `crates/api` `untrusted::tests::line_separators_and_format_characters_are_neutralized` |
| Sin interbloqueo: `guard.evaluate` con el cerrojo del repo tomado; entrada inválida → deny | `crates/core/tests/guard_evaluate.rs` |
| ADR-GRD-001 Val. 12 · frontera de la capa de escritura y del cliente del hook | `crates/git/tests/static_check.rs` (`repo_intact_guard_write::*`), `crates/core/tests/guard_boundary.rs` |
| Latencia (informe) | `latency_report` (ignorado; § 7) |

## 5. Pendientes y fuera de alcance

| Pendiente | Dueño |
|---|---|
| Encadenar hooks previos y gestores (husky, lefthook, pre-commit, `core.hooksPath` previo); el módulo de invocación del encadenado | US-GRD-002 (la constante `prior` ya existe) |
| Desinstalar, adoptar o retirar una instalación huérfana, barrido de cortes del producto (feature `test-cuts` y su check en release), ADR-GRD-001 Val. 1 (mitad de desinstalar), 8 (`config` sustituido al desinstalar), 9 (`binario-no-valido`, firma o huella del binario) y el § 8 (actualización del binario) | US-GRD-003 |
| Detección de pérdida, `instalacion-huerfana`, integridad del dispatcher frente al diario en el estado (Val. 9, manifiesto frente a dispatcher), backend de refs re-leído en cada comprobación, estado completo | US-GRD-004 |
| Registro de decisiones, spool y ventana del modo degradado (ADR-GRD-003 Val. 9, segunda mitad) y arranque bajo demanda del daemon desde el hook | US-GRD-005 |
| Token de excepción (`raptor guard exec`) y su variable en la allowlist del stub | US-GRD-006 |
| Actor (señal S4) y "git más cercano" | US-GRD-005 / US-GRD-006 |
| Comprobación M-02 del lado del daemon en macOS (`process_cwd` sin wrapper seguro de libproc) | US-GRP-009 |
| Windows: canal (named pipe), instalación, DACL de la carpeta (M-07), ADR-GRD-001 Val. 11 y 13 y ADR-GRD-002 Val. 4 en Windows | Historia del named pipe de motor-local (dueño por asignar). **Pendiente: etapa de validación multiplataforma** |
| Linux: las suites corren en el runner de CI con Git 2.38.5 y la versión del runner; sin prueba manual en la máquina Linux | **Pendiente: etapa de validación multiplataforma** |
| Firmar `raptor-hook` en release (hardened runtime y Authenticode) junto a `raptor` | Se empaqueta en este PR; la firma sigue el mismo paso de `raptor` |
| Daemon que se cierra si cambia la identidad de su propio ejecutable (mitigación del riesgo de D10 tras un `brew upgrade`) | US-GRD-003 (§ 8) |

## 6. Mensajes (PO)

Los textos del PO están en `apps/cli/i18n/{en,es}.txt` (`guard.*`). Los del stub (binario ausente, error interno, constantes movidas) van fijos en el propio stub, en en y es según `LC_ALL`/`LC_MESSAGES`/`LANG`, porque el stub no carga catálogos.

## 7. Medición y verificación

**Coste en Windows** (SPIKE-GRD-001 § 11; 2026-10-05; Windows 10 19045, i5-7400, Git 2.56.0.windows.1; `suites/07-cost-native.sh`, N=100, p50 ms; detalle en el § 14 de los resultados del spike):

| Escenario | Sin hooks | `sh` + binario (V1) | Nativo (VN) |
|---|---|---|---|
| Arranque de un proceso | — | `sh -c :` 31,3 | binario 4,1 |
| `update-ref` (3 invocaciones) | 37,7 | 174,3 | 57,5 |
| commit, conjunto mínimo (7) | 65,9 | 368,6 | 106,2 |
| commit, conjunto completo (11) | 66,2 | 563,7 | 132,7 |
| `switch -c`, conjunto mínimo (14) | 44,5 | 644,0 | 118,7 |
| `fetch` de 1.000 refs (3.018) | 2.557 | 132.304 | 15.714 |

Un dispatcher `sh` cuesta ≈ 43 ms por invocación en Windows y el nativo ≈ 6 ms: el nativo es necesario en Windows para todo el conjunto (D1). El objetivo ⚠️ ≤ 5 ms p95 por invocación que no evalúa queda **un poco por encima en esta máquina** (≈ 6 ms p50); se declara.

**Latencia en macOS** (`latency_report`, release con *debug assertions*, Apple Git 2.50.1, portátil con software corporativo, no en reposo): commit +35,5 ms p50 / +40,7 ms p95; evaluación gobernada (`branch -D`, con el daemon) +25,6 / +30,9 ms; vía rápida (`tag`) +12,4 / +13,9 ms. Cumple < 100 ms por evaluación, < 30 ms la vía rápida y el techo ⚠️ ≤ 150 ms p95 por commit.

**Verificado (2026-10-05)**:

- **macOS** arm64, Apple Git 2.50.1: `cargo clippy --all-targets -- -D warnings` y `cargo test --workspace` en verde (697 tests).
- **Linux** (contenedor `xplat/run-linux.sh`, Ubuntu 24.04 arm64): las suites de esta historia pasan (22 de punta a punta con la terminal del desarrollador vía `script` de util-linux) con **Git 2.38.5** y con la de la distro (2.43.0). El único fallo del workspace es previo y ajeno: `cost_hook_processes_per_command_match_the_table` del testkit no tiene filas para esas versiones de Git (INF-GRD-001 § 7). En CI, el job `guardrails (Git 2.38.5, Linux)`.
- **Windows** (máquina real, Git 2.56.0.windows.1): `cargo fmt --check` y `cargo clippy --workspace --all-targets -D warnings` en verde; pasan los tests de `crates/policy` (evaluación y vía rápida), `crates/core` (`guardrails`, `guard_boundary`), `crates/git` (`static_check`) y de la CLI (`guard`). Fallan tests previos y ajenos (`settings::schema` por CRLF, `team_config` porque `git init` no lee la configuración del entorno de prueba, `catalog` y `mcp`). **Humo funcional del dispatcher nativo en modo degradado** (sin canal en Windows): commit permitido con el aviso, `branch -D main` denegado, borrar una rama de trabajo permitido, `pack-refs` y `gc` pasan, la base empaquetada sigue protegida y, sin `raptor`, el commit pasa con "protección inactiva" y `branch -D main` sale con 1. La instalación en Windows espera al canal (XP-01). **Pendiente: etapa de validación multiplataforma** para el resto.

**Revisión de código** (subagente, 2026-10-05): once hallazgos, corregidos en `fix(guard): close the gaps found in review` salvo dos que quedan anotados: el tiempo máximo de la llamada al daemon (10 s, ya acotado por el cliente) y que, sin configuración del equipo, la rama base confirmada sea `main` aunque el repo use `master` (es lo que fijan Q-GRD-23 y ADR-GRD-004 § 3.5; el modo degradado protege además la rama principal). **Observación de producto** para US-GRD-014: en un repo cuya rama principal es `master`, la explicación lo deja ver ("rama base confirmada: main") y el desarrollador puede no autorizar.
