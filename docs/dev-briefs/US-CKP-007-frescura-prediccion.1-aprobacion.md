---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/cockpit/dev-specs/US-CKP-007-frescura-prediccion.md"
---

Lee este fichero y luego `US-CKP-007-frescura-prediccion.md`; `US-CKP-007-frescura-prediccion.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico agotó sus rondas en la ronda 2 sin un plan aprobable; ver «Diferidos»
- Generada: `2026-11-09T11:38:56.106Z`
- Sha de aprobación: `7e70ab4134be28138abe5afc8258560a5eb40164ebfac569efb041d3d937421f:f224a6fe9a49218420c3505cc50f0c0d73da62ae65d06e3d34a2389801c8c475:7450ffd58ec0d2e47136f6cc1f8d305e8599b5d678d842cf7990d538e1832f3d`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 131cad2b7f1d>>>
```text
intent: Que ninguna predicción vieja se presente como actual en la TUI: cada par muestra su antigüedad (que avanza sola cada segundo), un par recalculado dice 'desactualizado · recalculando', los pares sin resultado se agregan como 'Calculando N de M pares', los pending/not-computable se agrupan por motivo y 'Sin conflictos previstos' solo sale con todo vigente y limpio (BR-CKP-CALC-003, US-CKP-007).
endState: apps/cli tiene present/freshness_text.rs (FreshnessText en/es, cadenas enteras), tui/freshness.rs (funciones puras de mapeo y líneas agregadas), Prediction::Pending, fn age pub(crate), el Tick de update.rs que repinta mientras haya resultados, y el panel de conflictos pinta filas y líneas agregadas (ConflictPanelModel.summary) con snapshots en en/es y ASCII; la TUI no importa motor ni Git; el contrato de API y el motor no cambian.
nonGoals:
  - Alerta de conflicto nuevo (US-CKP-008)
  - PolicyBanner y acción ante pending/not-computable (US-CKP-009)
  - Cambios en el motor, la API o la regla de base movida (ya en main por #314)
  - G-B: confirmar la base en runtime no replanifica (TS-CKP-011)
  - KPI de US-CKP-010 y age_s del perfil mcp
  - Linux y Windows (etapa de validación multiplataforma)
```
<<<FIN-DATOS-NO-CONFIABLES 131cad2b7f1d>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 0b181e8a6653>>>
```text
[1] id: S1
statement: Cuando la TUI abre un repo de 10 worktrees (10 pares) con todos los pares calculating, el panel dice 'Calculando 10 de 10 pares' (es) / 'Calculating 10 of 10 pairs' (en), ningún par dice 'Sin conflictos' y no hay línea 'Sin conflictos previstos'
verify: cargo test -p gitraptor-cli --test tui_freshness s1_initial_calculation_says_calculating_and_never_no_conflicts
tests:
  - apps/cli/tests/tui_freshness.rs

[2] id: S2
statement: Cuando un prediction.changed lleva el par claude-1 ↔ claude-2 a recalculating con resultado de hace 40 s, su fila dice 'desactualizado · hace 40 s · recalculando' (también sin ⚡ ni ⚠), el par claude-3 ↔ claude-4 sigue current 'hace 40 s', y un segundo evento devuelve el primero a current 'hace 0 s'. La fila de un par recalculating sin ⚡ ni ⚠ no lleva ⚡ ni ⚠ ni línea de archivos
verify: cargo test -p gitraptor-cli --test tui_freshness s2_a_commit_marks_only_that_pair_stale_with_its_age
tests:
  - apps/cli/tests/tui_freshness.rs

[3] id: S3
statement: Cuando un prediction.changed pasa los 4 pares contra la base a calculating sin level, el panel muestra 'Calculando 4 de 10 pares', esos pares pierden su fila y no aparece 'Sin conflictos previstos'
verify: cargo test -p gitraptor-cli --test tui_freshness s3_a_moved_base_shows_the_base_pairs_calculating
tests:
  - apps/cli/tests/tui_freshness.rs

[4] id: S4
statement: Un resultado calculado hace 3 min muestra 'hace 3 min' junto al par (es) / '3 min ago' o su cadena entera en en, y un Msg::Tick de 1 s lo mueve de 'hace 59 s' a 'hace 1 min'; una antigüedad negativa (reloj por detrás) se ve 'hace 0 s'
verify: cargo test -p gitraptor-cli --test tui_freshness s4_the_age_is_always_shown_and_moves_with_the_tick
tests:
  - apps/cli/tests/tui_freshness.rs

[5] id: S5
statement: Los pares pending y not-computable se agrupan por motivo con singular y plural ('Pendiente: confirma la rama base (4 pares)', 'No calculable: falta la rama base (1 par)') y ninguno dice 'Sin conflictos' ni habilita 'Sin conflictos previstos'
verify: cargo test -p gitraptor-cli --test tui_freshness s5_pending_and_not_computable_group_by_reason_and_never_say_no_conflicts
tests:
  - apps/cli/tests/tui_freshness.rs

[6] id: S6
statement: 'Sin conflictos previstos · hace N' (N del resultado más antiguo) solo aparece con al menos un par y todos current sin ⚡ ni ⚠; con 0 pares, o con un solo par recalculating, calculating, pending o con ⚠, no aparece
verify: cargo test -p gitraptor-cli --test tui_freshness s6_no_conflicts_line_only_when_every_pair_is_current_and_clean
tests:
  - apps/cli/tests/tui_freshness.rs

[7] id: S7
statement: Con NO_COLOR/ASCII la frescura se ve igual: 'hace N', 'desactualizado' y 'Calculando' salen sin color y el '·' se pliega a '|' según Glyphs::fold (tui/style.rs), sin otros caracteres Unicode, en 80x24, en en y es
verify: cargo test -p gitraptor-cli --test tui_freshness s7_freshness_is_legible_without_color_and_in_ascii
tests:
  - apps/cli/tests/tui_freshness.rs
```
<<<FIN-DATOS-NO-CONFIABLES 0b181e8a6653>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 6d8a067720b6>>>
```text
docs/requirements/features/cockpit/dev-specs/US-CKP-007-frescura-prediccion.md
```
<<<FIN-DATOS-NO-CONFIABLES 6d8a067720b6>>>

planSha: `7e70ab4134be28138abe5afc8258560a5eb40164ebfac569efb041d3d937421f` (base de la cadena de re-planes)

Léelo en `US-CKP-007-frescura-prediccion.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 8dc9e5145820>>>
```text
docs/dev-briefs/us-ckp-007-prediction-freshness.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 8dc9e5145820>>>

contractSha: `f224a6fe9a49218420c3505cc50f0c0d73da62ae65d06e3d34a2389801c8c475`

## Tests de criterio

testsSha: `7450ffd58ec0d2e47136f6cc1f8d305e8599b5d678d842cf7990d538e1832f3d`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES a36de5812e41>>>
```text
73f969197b2908bacd5ce8061fc210dd3725d747f151064713a16f6abd0e2b3b  apps/cli/tests/tui_freshness.rs
```
<<<FIN-DATOS-NO-CONFIABLES a36de5812e41>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 3 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: AJUSTES · 1 bloqueante(s) · siguiente: excepcion
- Último veredicto: AJUSTES
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

1 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES 5749011649ab>>>
```text
id: PC-5
kind: contradiccion
toHuman: false
evidence: apps/cli/tests/tui_freshness.rs:461
ask: S7 esperaba '-' en vez de '|' (corregido tras la ronda).
```
<<<FIN-DATOS-NO-CONFIABLES 5749011649ab>>>

## Firmas

- planSha (contexto, no firmado): `7e70ab4134be28138abe5afc8258560a5eb40164ebfac569efb041d3d937421f`
- contractSha: `f224a6fe9a49218420c3505cc50f0c0d73da62ae65d06e3d34a2389801c8c475`
- testsSha: `7450ffd58ec0d2e47136f6cc1f8d305e8599b5d678d842cf7990d538e1832f3d`
- approvalSha: `7e70ab4134be28138abe5afc8258560a5eb40164ebfac569efb041d3d937421f:f224a6fe9a49218420c3505cc50f0c0d73da62ae65d06e3d34a2389801c8c475:7450ffd58ec0d2e47136f6cc1f8d305e8599b5d678d842cf7990d538e1832f3d`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
