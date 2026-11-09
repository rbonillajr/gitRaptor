---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/td-grp-005-s3-sample-window.2-plan.md"
---

Lee este fichero y luego `td-grp-005-s3-sample-window.2-plan.md`; `td-grp-005-s3-sample-window.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-09T05:36:48.640Z`
- Sha de aprobación: `febef73b13a6586c4741ca96754c2633ffa31b045a118b559f30eb5470721749:aa2d59efeddf709980340ce579996aa1a4832e614022d3325056979019b17fdc:18ca74e19675da8502056f112d128ef93b14026db92ed8db2910dcedb5242dc0`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 0956023894cb>>>
```text
intent: Que un commit que un agente hace mientras su git sigue vivo salga atribuido al agente también con la máquina cargada, sin que S3 llegue nunca a atribuir a Claude Code el commit de un humano (ante la duda, sin atribuir).
endState: sample_loop vacía la cola sin quedarse con un aviso por repo; cada aviso en cola recibe, de una sola lectura de la tabla de procesos, un avistamiento con su propio t_recv y su propio started_before (los avisos con la misma máscara comparten un Sighting con t_recvs); evidence cuenta un avistamiento si alguno de sus t_recvs cae en la ventana del lote; la regla de ADR-GRP-012 no cambia; TD-GRP-005 queda implemented y ADR-GRP-012 lleva la aclaración.
nonGoals:
  - cambiar probed en watch/mod.rs (ya avisa por evento del SO)
  - ensanchar la ventana s3_lead o sellar con el aviso más reciente
  - el ámbito worktree de S3 y la redirección (TD-GRP-004)
  - el umbral de inactividad de las sesiones
  - cambiar los tests previos, salvo quitar esperas que dejen de hacer falta
  - añadir API pública al Detector
```
<<<FIN-DATOS-NO-CONFIABLES 0956023894cb>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES ecb26e530f32>>>
```text
[1] id: TD005-QUEUED-EVIDENCE
statement: T001: con los avisos A, B1 y B2 en el mismo lote en cola y el git escritor de la sesión empezado entre B1 y B2, evidence en el t_recv de B2 da Attributed a esa sesión (hoy da NoSighting)
verify: cargo test -p gitraptor-core --lib -- --exact detect::tests::queued_notices::a_later_queued_notice_is_evidence_of_its_commit
tests:
  - crates/core/src/detect/tests/queued_notices.rs

[2] id: TD005-NO-LEAK
statement: T001 (guarda): un aviso en cola nunca toma un git empezado después de él (NoSighting, gits_after_notice >= 1), y un git ajeno escritor en el worktree deja el evento de B2 en Ambiguous
verify: cargo test -p gitraptor-core --lib -- --exact detect::tests::queued_notices::a_queued_notice_never_takes_a_git_started_after_it detect::tests::queued_notices::a_queued_notice_with_a_foreign_git_still_makes_it_ambiguous
tests:
  - crates/core/src/detect/tests/queued_notices.rs

[3] id: TD005-BOUNDED
statement: T001 (guarda): una ráfaga de 300 avisos en cola no expulsa del ring el avistamiento anterior que vio un git ajeno; evidence en el t_recv de A sigue Ambiguous
verify: cargo test -p gitraptor-core --lib -- --exact detect::tests::queued_notices::a_burst_of_queued_notices_keeps_the_foreign_sighting
tests:
  - crates/core/src/detect/tests/queued_notices.rs
```
<<<FIN-DATOS-NO-CONFIABLES ecb26e530f32>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES a2ce5040ca43>>>
```text
- **Por qué no `sample_queued_now`.** Una entrada pública nueva no existe hoy, así que el test rojo no compilaría, y el commit de stub antes de la aprobación no está permitido. El test entra por `observer_hooks()` con un `ProcLister` con compuerta: compila con el código actual, pasa por el `sample_loop` real (donde está el defecto) y falla por aserción.
- La agrupación por máscara es exacta: dos avisos con la misma máscara dan los mismos `gits`, y `t_recvs` conserva cada marca, así que la ventana se evalúa aviso por aviso. Tiene dos efectos: los contadores de `s3_evidence` no se inflan con avisos idénticos y el ring (256) no se vacía en una ráfaga.
- `t_recvs` crece con la cola. Su tamaño lo limita el canal, que ya no tiene límite hoy: son 8 bytes por aviso, durante una muestra.
- Coste: `owner()` y `launched_from()` se calculan una vez por repo y muestra, no una vez por aviso. Es menos trabajo que hoy con un lote de varios repos.
- Plataformas: el cambio es lógica pura, igual en macOS, Linux y Windows. Linux usa `start_tolerance` de 1 s, que afecta igual a cada aviso. El test fija la tolerancia a 0 para ser determinista.
- Atribución de extremo a extremo bajo carga (10 de 10, TD-GRP-005 §Plan): se verifica en local con la suite de otro crate en paralelo. Si no se puede, se informa como SIN CERTIFICAR.
- Modo rápido: rojo de los tests del contrato, P0 abortado, verificación con la lista de T002.
```
<<<FIN-DATOS-NO-CONFIABLES a2ce5040ca43>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES c5c354d4e587>>>
```text
docs/dev-briefs/td-grp-005-s3-sample-window.2-plan.md
```
<<<FIN-DATOS-NO-CONFIABLES c5c354d4e587>>>

planSha: `febef73b13a6586c4741ca96754c2633ffa31b045a118b559f30eb5470721749` (base de la cadena de re-planes)

Léelo en `td-grp-005-s3-sample-window.2-plan.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 35b783b27f1b>>>
```text
docs/dev-briefs/td-grp-005-s3-sample-window.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 35b783b27f1b>>>

contractSha: `aa2d59efeddf709980340ce579996aa1a4832e614022d3325056979019b17fdc`

## Tests de criterio

testsSha: `18ca74e19675da8502056f112d128ef93b14026db92ed8db2910dcedb5242dc0`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 967d56cf44f4>>>
```text
a919acdcbc6e7158339ba710adba1bdc5cc9ddf7afceabc4402c893e35f6ef31  crates/core/src/detect/tests/queued_notices.rs
```
<<<FIN-DATOS-NO-CONFIABLES 967d56cf44f4>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 2 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `febef73b13a6586c4741ca96754c2633ffa31b045a118b559f30eb5470721749`
- contractSha: `aa2d59efeddf709980340ce579996aa1a4832e614022d3325056979019b17fdc`
- testsSha: `18ca74e19675da8502056f112d128ef93b14026db92ed8db2910dcedb5242dc0`
- approvalSha: `febef73b13a6586c4741ca96754c2633ffa31b045a118b559f30eb5470721749:aa2d59efeddf709980340ce579996aa1a4832e614022d3325056979019b17fdc:18ca74e19675da8502056f112d128ef93b14026db92ed8db2910dcedb5242dc0`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
