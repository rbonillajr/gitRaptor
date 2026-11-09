---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/cockpit/dev-specs/TS-CKP-011-base-confirmada-replanifica.md"
---

Lee este fichero y luego `TS-CKP-011-base-confirmada-replanifica.md`; `TS-CKP-011-base-confirmada-replanifica.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico agotó sus rondas en la ronda 2 sin un plan aprobable; ver «Diferidos»
- Generada: `2026-11-09T14:08:57.087Z`
- Sha de aprobación: `20c812c3b4c4ae63d77bc81148553289b345d60525775e5bf3ce2bdcacfec26e:3dafaf1ae34a413d966d193ef73a03dc0ce2a9ca5d818c2b3f4e0862bc02dfa5:654486ea10ce428e287997f8af08d98859847aaa5a4823d9eeb6c6cea24bdb8f`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 8340479d8200>>>
```text
intent: Que confirmar la rama base con el daemon en marcha (raptor guard confirm o raptor guard install) refresque la base publicada y haga que el predictor replanifique los pares contra la base, sin reiniciar el daemon (TS-CKP-011, cierra G-B de US-CKP-006).
endState: crates/core/src/daemon/base.rs con Daemon::refresh_base (publica divergence.base y RepoView.base solo si cambió, recuenta adelanto y retraso y llama a Modules::base_changed); guard() lo llama tras GuardRequest::Install y GuardRequest::Confirm; DaemonModule::base_changed con implementación vacía; Predict lo atiende con Msg::BaseChanged (salvo repo dormido). El escenario 3 de US-CKP-006 sin #[ignore] pasa. Sin métodos, eventos, capacidades ni mensajes nuevos en el canal; API del predictor sin cambios.
nonGoals:
  - Un evento del canal que lleve la base nueva
  - Desconfirmar la base
  - Cambiar publish_observed_views
  - Cambiar la API del predictor o los mensajes que consume la app de escritorio (US-DSK-004)
  - Linux y Windows para el e2e de la TUI (etapa de validación multiplataforma)
```
<<<FIN-DATOS-NO-CONFIABLES 8340479d8200>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 8d8ebf0e62c2>>>
```text
[1] id: C1
statement: Con el daemon en marcha y el repo sin base confirmada, tras guard.confirm (Accept sin ventana) el par (wt-a, base) pasa de pending a calculating y a current con level y conflict_files, todo por prediction.changed
verify: cargo test -p gitraptor-core --test ts_ckp_011_base_refresh confirm_turns_the_base_pair_from_pending_to_current -- --exact
tests:
  - crates/core/tests/ts_ckp_011_base_refresh.rs

[2] id: C2
statement: Lo mismo cuando la base se confirma con guard.install (journal.confirms_base): el par contra la base sale current
verify: cargo test -p gitraptor-core --test ts_ckp_011_base_refresh install_confirming_the_base_replans -- --exact
tests:
  - crates/core/tests/ts_ckp_011_base_refresh.rs

[3] id: C3
statement: Tras la primera confirmación el par (wt-a, base) llega a current; después, confirmar otra vez la misma base no publica ningún prediction.changed de ese par
verify: cargo test -p gitraptor-core --test ts_ckp_011_base_refresh confirming_the_same_base_twice_publishes_once -- --exact
tests:
  - crates/core/tests/ts_ckp_011_base_refresh.rs

[4] id: C4
statement: Confirmar otra rama (develop) como base deja el par calculado contra develop; ningún lote posterior trae el nivel calculado contra la base anterior
verify: cargo test -p gitraptor-core --test ts_ckp_011_base_refresh a_new_base_replans_against_it_and_drops_late_results -- --exact
tests:
  - crates/core/tests/ts_ckp_011_base_refresh.rs

[5] id: C5
statement: Con el repo dormido antes de confirmar (repo_dormant en el log), la confirmación lo despierta y el par contra la base sale current con conflicto contra la base confirmada; mientras está dormido, base_changed no planifica (rama Msg::BaseChanged dormida, sin observador posible desde fuera porque suscribirse despierta el repo)
verify: cargo test -p gitraptor-core --test ts_ckp_011_base_refresh a_dormant_repo_replans_on_waking -- --exact
tests:
  - crates/core/tests/ts_ckp_011_base_refresh.rs

[6] id: C6
statement: Escenario 3 de US-CKP-006 sin #[ignore]: tras raptor guard install con la TUI, la TUI muestra ⚡ wt-pagos ↔ main con src/api.rs y el rango 10–12 ↔ 10–12
verify: cargo test -p gitraptor-cli --test tui_conflicts a_worktree_against_the_confirmed_base_shows_the_conflict -- --exact
tests:
  - apps/cli/tests/tui_conflicts.rs
```
<<<FIN-DATOS-NO-CONFIABLES 8d8ebf0e62c2>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 7d6b32f466f5>>>
```text
docs/requirements/features/cockpit/dev-specs/TS-CKP-011-base-confirmada-replanifica.md
```
<<<FIN-DATOS-NO-CONFIABLES 7d6b32f466f5>>>

planSha: `20c812c3b4c4ae63d77bc81148553289b345d60525775e5bf3ce2bdcacfec26e` (base de la cadena de re-planes)

Léelo en `TS-CKP-011-base-confirmada-replanifica.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 686ca8ec6c2b>>>
```text
docs/dev-briefs/ts-ckp-011-base-confirmed-replans.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 686ca8ec6c2b>>>

contractSha: `3dafaf1ae34a413d966d193ef73a03dc0ce2a9ca5d818c2b3f4e0862bc02dfa5`

## Tests de criterio

testsSha: `654486ea10ce428e287997f8af08d98859847aaa5a4823d9eeb6c6cea24bdb8f`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 0d1405eceb43>>>
```text
ea6502744054777c2d40fd58301fb9f2dbf0b55a84c027bfe70ca7ad1eeafb7c  apps/cli/tests/tui_conflicts.rs
dbcfce4ebf4b235d21a53513929d7055f3385924a394efef4241c8353ddad04e  crates/core/tests/ts_ckp_011_base_refresh.rs
```
<<<FIN-DATOS-NO-CONFIABLES 0d1405eceb43>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
  - ronda 2: AJUSTES · 1 bloqueante(s) · siguiente: excepcion
- Último veredicto: AJUSTES
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

1 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES 260be02bed67>>>
```text
id: PC-3
kind: contradiccion
toHuman: false
evidence: docs/requirements/features/cockpit/dev-specs/TS-CKP-011-base-confirmada-replanifica.md:220
ask: Reescribe la fila de a_dormant_repo_replans_on_waking en la tabla de pruebas de T001 para que diga lo mismo que el nuevo C5 (la confirmación despierta el repo; Daemon::guard llama a wake_for_common_dir antes de refresh_base; el par sale current con conflicto contra la base confirmada).
```
<<<FIN-DATOS-NO-CONFIABLES 260be02bed67>>>

## Firmas

- planSha (contexto, no firmado): `20c812c3b4c4ae63d77bc81148553289b345d60525775e5bf3ce2bdcacfec26e`
- contractSha: `3dafaf1ae34a413d966d193ef73a03dc0ce2a9ca5d818c2b3f4e0862bc02dfa5`
- testsSha: `654486ea10ce428e287997f8af08d98859847aaa5a4823d9eeb6c6cea24bdb8f`
- approvalSha: `20c812c3b4c4ae63d77bc81148553289b345d60525775e5bf3ce2bdcacfec26e:3dafaf1ae34a413d966d193ef73a03dc0ce2a9ca5d818c2b3f4e0862bc02dfa5:654486ea10ce428e287997f8af08d98859847aaa5a4823d9eeb6c6cea24bdb8f`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
