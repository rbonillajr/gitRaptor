---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/td-grp-007-daemon-inert.2-plan.md"
---

Lee este fichero y luego `td-grp-007-daemon-inert.2-plan.md`; `td-grp-007-daemon-inert.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico paró en la ronda 2 con una decisión que solo puede tomar una persona (BLOQUEO); ver «Diferidos»
- Generada: `2026-11-10T02:36:18.550Z`
- Sha de aprobación: `0193aeec9bf9a268c5730f055284e46c5d1c24d85053717e18b76efb11539582:197ad65fda70c04d46396d726f5bea4b3081a8308e5db9983f0a8bd0da4bb243:c3c1ca7db356f468150f404fd0f30c89f4a0d2cb883f1fe78293d86783bf5df1`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 8b101b196864>>>
```text
intent: Que la atribución de los commits de un agente no dependa de la suerte con la máquina cargada, sin abrir el riesgo residual de S3: un git del propio daemon que no puede producir un evento deja de volver ambiguo el commit del agente, y ante la duda el evento sigue ambiguo (NFR-01, ADR-GRP-012 Enmienda 2026-11-09).
endState: Un git hijo directo del daemon, que ejecuta el Git resuelto por el motor, con su cwd legible en el repo y un argv que encaja exactamente en una plantilla inerte de las capas de lectura o de escritura de la Time Machine (nunca update-ref), se clasifica Ignored y suma daemon_inert; todo evento cuya ventana S3 se solapa con una ventana de escritura del daemon (aplicador de la Time Machine o ejecutor de operaciones de usuario, guarda RAII) es ajeno y suma daemon_write_window; todo lo demás del daemon sigue siendo ajeno; los dos tests de us_tmc_003 ignorados en macOS por el PR #347 vuelven a correr allí.
nonGoals:
  - distinguir ESRCH de EPERM/EACCES en la lectura del cwd
  - cambiar la regla del git ajeno o la lista de inofensivos
  - aceptar como inerte un nieto del daemon
  - relajar el tope de 3 s del aviso de lectura o tocar la latencia de FSEvents (TD-GRP-006)
  - sacar s3_cwd_unreadable de la línea s3_evidence
  - lector de argv en Windows
```
<<<FIN-DATOS-NO-CONFIABLES 8b101b196864>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES d0e85ee1716b>>>
```text
[1] id: GRP007-ARGV
statement: is_daemon_inert_argv da true para el argv real de update-index (las dos formas), pack-objects, index-pack, repack y prune de la capa de escritura (con y sin --work-tree) y para version, rev-parse, for-each-ref, rev-list, merge-base y log de la capa de lectura con argumentos; da false para update-ref, worktree list, config, --exec-path, un -c de más, una opción de WRITE_OPTIONS que falta o cambia de orden, un --git-dir relativo, un argumento detrás de la plantilla de escritura, --output o --output=x en la lectura, un argv sin opciones fijas (git update-index) y un argv vacío
verify: cargo test -p gitraptor-git --lib invoke::inert_tests::argv_
tests:
  - crates/git/src/invoke_inert_tests.rs

[2] id: GRP007-FIT
statement: Fitness: para cada entrada de daemon_git_argvs(), is_daemon_inert_argv es true si y solo si su subcomando no es update-ref, worktree, config ni --exec-path
verify: cargo test -p gitraptor-git --lib invoke::inert_tests::fitness_
tests:
  - crates/git/src/invoke_inert_tests.rs

[3] id: GRP007-CALL
statement: git_call da inert para el argv real de un git inerte del daemon y no para git update-index sin las opciones fijas, git status, git log suelto ni el argv de update-ref del daemon
verify: cargo test -p gitraptor-core --lib detect::procs::inert_tests
tests:
  - crates/core/src/detect/procs_inert_tests.rs

[4] id: GRP007-CLASSIFY
statement: Con la tabla sintética y engine_git fijado: un git hijo directo del daemon con el Git del motor, una plantilla inerte y cwd en el repo, junto a un git de la sesión que escribe, da Attributed con daemon_inert=1 y foreign_daemon=0; dan Ambiguous con foreign_daemon un exe distinto del Git del motor, engine_git sin fijar, un argv sin plantilla, (Daemon, None) con argv inerte, un nieto (daemon → commit → git inerte), un git por encima del tope de lecturas y un inerte junto a un update-ref del daemon en la misma muestra; un git ajeno que escribe en el worktree del evento, junto a un inerte del daemon y un git de la sesión, da Ambiguous (NFR-01)
verify: cargo test -p gitraptor-core --lib detect::tests::daemon_inert
tests:
  - crates/core/src/detect/tests/daemon_inert.rs

[5] id: GRP007-WINDOW
statement: Con un git de la sesión que escribe y sin git del daemon: una ventana de escritura abierta sobre el repo durante la ventana S3, o cerrada menos de WINDOW_TAIL (3 s) antes de su comienzo, da Ambiguous con daemon_write_window=1; una ventana sobre otro repo, o cerrada más de WINDOW_TAIL antes del comienzo de la ventana S3, no cuenta (Attributed); soltar la guarda cierra la ventana y un pánico con la guarda viva también la cierra
verify: cargo test -p gitraptor-core --lib detect::tests::daemon_write_window
tests:
  - crates/core/src/detect/tests/daemon_write_window.rs

[6] id: GRP007-WIRE
statement: Integración en repos temporales: un undo simple del desarrollador sobre una operación (como el escenario 1 de us_tmc_003) deja al menos una ventana de escritura del daemon sobre su repo en DaemonWrites::global(), y una operación de usuario del ejecutor que lanza git (run_git_with) deja otra sobre el suyo
verify: cargo test -p gitraptor-core --test td_grp_007
tests:
  - crates/core/tests/td_grp_007.rs

[7] id: GRP007-LOG
statement: SEC-04: s3_evidence lleva daemon_inert y daemon_write_window como enteros con el valor contado, y ningún valor de la línea es una ruta, un nombre, un pid ni un argv
verify: cargo test -p gitraptor-core --lib daemon::sessions::s3_
tests:
  - crates/core/src/daemon/sessions_s3_inert_tests.rs
  - crates/core/src/daemon/sessions_s3_tests.rs

[8] id: GRP007-ENGINE
statement: Con el daemon real, el detector que arranca detector_hooks tiene como engine_git la ruta canónica del SystemGit que resolvió el daemon, y None cuando no hay Git resuelto
verify: cargo test -p gitraptor-core --lib daemon::sessions::s3_inert_tests::engine_
tests:
  - crates/core/src/daemon/sessions_s3_inert_tests.rs

[9] id: GRP007-MACOS
statement: Los dos tests de us_tmc_003 ignorados en macOS por el PR #347 (a_new_commit_invalidates_the_redo y a_redo_right_after_an_undo_is_not_invalidated_by_its_echo) ya no llevan cfg_attr ignore y pasan
verify: cargo test -p gitraptor-core --test us_tmc_003 -- --exact a_new_commit_invalidates_the_redo a_redo_right_after_an_undo_is_not_invalidated_by_its_echo
tests:
  - crates/core/tests/us_tmc_003.rs
```
<<<FIN-DATOS-NO-CONFIABLES d0e85ee1716b>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES c466459548bf>>>
```text
- **Tests existentes que cambian a propósito**:
  - `daemon/sessions_s3_tests.rs::the_s3_log_line_carries_only_counters` pasa de 10 a 12 contadores. El literal de `S3Counts` ya está en T000. N2.
  - `detect/tests/s3_scope.rs::…counts_the_daemons_git_anywhere_in_the_repo` sigue valiendo, porque su argv por defecto es `commit`.
- **NFR-01 con un `git` ajeno concurrente** (Plan de Verificación de la ficha): GRP007-CLASSIFY añade el caso de un `git` ajeno que escribe en el worktree, junto a un inerte del daemon y un `git` de la sesión, que da `Ambiguous`. Los casos de la Enmienda 2026-10-28 siguen en `detect/tests/s3_scope.rs`.
- **Margen de la ventana**: 3 s después de cerrarse, igual que el tope del aviso de lectura. Un aviso que llega más tarde se descarta y el evento queda `no-sighting` (TD-GRP-006): sin atribuir y nunca al agente. El precio es que un commit del agente hasta 3 s después de un restore o de una operación del ejecutor en el mismo repo queda ambiguo, igual que hoy. N2.
- **Un nieto no es inerte**: el Arquitecto había propuesto aceptar `repack` → `pack-objects`. Solo cuenta el hijo directo: `repack` y `prune` corren en el almacén, fuera del repo, y el `exe` de un hijo de Git es `libexec/git-core/git`, que no es el Git del motor. Es más estricto. N2.
- **Ventana del ejecutor**: abarca cada `git` de usuario del ejecutor, no la operación entera. Entre dos `git` de una misma operación queda un hueco de milisegundos, y es la carrera de S3 que ya se acepta. N2.
- **Retención de 120 s**: más que `s3_lead` y que el tope de 3 s del aviso de lectura. Un lote evaluado más tarde ya no ve la ventana, y con un `git` vivo del daemon sigue siendo ajeno si no es inerte. N2.
- **El test de `env_clear()`** (condición 6) ya existe: `invoke.rs::tests::child_env_is_an_allowlist`. No se añade otro.
- **Windows**: sin lector de argv, ningún `git` del daemon es inerte. La ventana sí se aplica. Pendiente de la validación multiplataforma.
- **Linux**: misma lectura de `/proc`. Lo verifica el CI de ubuntu, no esta Mac.
- **Modo 2 (TD-GRP-006)**: los dos tests reactivados aún pueden fallar por FSEvents con la máquina muy cargada (2 de 14 en local con carga 18+). La condición del coordinador es el CI de macOS en verde con los dos tests activos. Si falla por `no-sighting`, lo informo y pido más corridas. El Arquitecto prefería 5 verdes seguidos en el CI antes de reactivarlos. Lo decide el coordinador. N3.
- **Modo rápido** (Rene, 2026-11-08): sin línea base de la suite completa. Los tests del contrato se prueban en rojo de forma dirigida y la entrega queda **SIN CERTIFICAR**. Excepción NFR-01, porque toca `timemachine/apply`: antes del PR se corren completos `cargo test -p gitraptor-core` y `-p gitraptor-git` y los e2e de undo de cli.
```
<<<FIN-DATOS-NO-CONFIABLES c466459548bf>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES c31ed1b68abf>>>
```text
docs/dev-briefs/td-grp-007-daemon-inert.2-plan.md
```
<<<FIN-DATOS-NO-CONFIABLES c31ed1b68abf>>>

planSha: `0193aeec9bf9a268c5730f055284e46c5d1c24d85053717e18b76efb11539582` (base de la cadena de re-planes)

Léelo en `td-grp-007-daemon-inert.2-plan.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES bd57a9031daa>>>
```text
docs/dev-briefs/td-grp-007-daemon-inert.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES bd57a9031daa>>>

contractSha: `197ad65fda70c04d46396d726f5bea4b3081a8308e5db9983f0a8bd0da4bb243`

## Tests de criterio

testsSha: `c3c1ca7db356f468150f404fd0f30c89f4a0d2cb883f1fe78293d86783bf5df1`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES e39eda2b3d1c>>>
```text
5fd4cc27d2063905ec5cdbe207a725be61bfda27843b919125e32412064b3883  crates/core/src/daemon/sessions_s3_inert_tests.rs
785e1b2dc30915725510694fa86b2010fad0d62b4d3a05f2afc7f441dadbda3d  crates/core/src/daemon/sessions_s3_tests.rs
800f8e9d087507e2a84fa5a882742e76cbfe921c387da8f628f921ec8741989c  crates/core/src/detect/procs_inert_tests.rs
f0043af7ee1bdd5e140e2ae2acb71c4438df6fc63000f79e6e558c6508c253e9  crates/core/src/detect/tests/daemon_inert.rs
76fa098ef97081ee0ccf95740e562560823f96dd557275b6298707b727d12155  crates/core/src/detect/tests/daemon_write_window.rs
ee431fd56626d483dc7bfdde94ac3c1ee9afb6086d550611a2d090e7cbf7aba4  crates/core/tests/td_grp_007.rs
4ea507e85b718a216784b2a2c9373b54140aea0217c065c89627cfe66d72a10f  crates/core/tests/us_tmc_003.rs
f28ee0f43fb387e3f419662771f19396c50df315380692596c2fd3bb672be9fe  crates/git/src/invoke_inert_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES e39eda2b3d1c>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: BLOQUEO · 6 bloqueante(s) · siguiente: excepcion
  - ronda 2: BLOQUEO · 1 bloqueante(s) · siguiente: excepcion
- Último veredicto: BLOQUEO
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

1 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES 84af25e4bd85>>>
```text
id: PC-1
kind: indeciso
toHuman: true
evidence: docs/dev-briefs/td-grp-007-daemon-inert.2-plan.md:129
ask: Aprobador: elige D1. La opción A cambia el Plan de Verificación de la ficha TD-GRP-007 (línea 74, «10 de 10 con carga» en us_tmc_003, us_tmc_003_agents y us_tmc_011): el CI de macOS en verde más las cifras de carga en el PR, y un fallo no-sighting cuenta contra TD-GRP-006. La opción B pide 5 corridas verdes seguidas del CI de macOS antes de reactivar los dos tests. El contrato y el árbol ya dan por hecha la A: GRP007-MACOS (contract.json:96) exige que no haya cfg_attr ignore, y el tester ya los quitó de us_tmc_003.rs. Si eliges B, el arquitecto debe quitar o reescribir GRP007-MACOS y T006, y hay que restaurar los cfg_attr. Si eliges A, la ficha debe recoger la enmienda de su Plan de Verificación en el mismo PR.
```
<<<FIN-DATOS-NO-CONFIABLES 84af25e4bd85>>>

## Firmas

- planSha (contexto, no firmado): `0193aeec9bf9a268c5730f055284e46c5d1c24d85053717e18b76efb11539582`
- contractSha: `197ad65fda70c04d46396d726f5bea4b3081a8308e5db9983f0a8bd0da4bb243`
- testsSha: `c3c1ca7db356f468150f404fd0f30c89f4a0d2cb883f1fe78293d86783bf5df1`
- approvalSha: `0193aeec9bf9a268c5730f055284e46c5d1c24d85053717e18b76efb11539582:197ad65fda70c04d46396d726f5bea4b3081a8308e5db9983f0a8bd0da4bb243:c3c1ca7db356f468150f404fd0f30c89f4a0d2cb883f1fe78293d86783bf5df1`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
