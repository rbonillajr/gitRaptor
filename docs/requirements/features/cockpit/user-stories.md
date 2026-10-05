---
mode: bulk
generated: 2026-10-04T00:00Z
updated: 2026-10-04
generator: product-owner
total_artifacts: 23
expanded: 23
approved: 0
blocked:
  - US-CKP-023
---

# User Stories — INDEX: Cockpit

> Modo lean: cada historia es un contrato compacto para un agente (valor en una línea, reglas cubiertas, dependencias y Gherkin). Sin story points ni narrativas largas. Cada historia vive en su archivo de `user-stories/`.

---

## Contexto del Feature

**Feature**: Cockpit (F-001-02) · **Epic**: E-001 — MVP Fase 1
**Prioridad**: Alta (BR-04 a BR-07 Must; abrir en el editor y la CLI de solo lectura, Should)
**Estado**: 22 historias listas para Dev Spec; 1 bloqueada (US-CKP-023). Requerimiento aprobado por Rene Bonilla el 2026-10-05.

**Contexto**: [`context.md`](./context.md) (CTX-CKP-001) · **Reglas**: [`business-rules.md`](./business-rules.md) (BR-CKP-001) · **Enablers**: [`technical-stories.md`](./technical-stories.md) (ADR-CKP-001 a 003, TS-CKP-001 a 004, INF-CKP-001, SPIKE-CKP-001)

---

## Análisis de división

- **Flujo**: actor = desarrollador orquestador. Resultado = ver la flota, saber quién choca y actuar sobre el trabajo de cada agente sin perder nada.
- **Patrón**: camino feliz primero, en el orden de entrega Q-CKP-24 (BR-04 → BR-06 → BR-07 → BR-05). Esqueleto andante de lectura: **US-CKP-001**. Esqueleto de escritura: **US-CKP-014** (integrar), que fija el flujo de toda acción protegida; rebase, descartar y crear lo reutilizan.
- **Filtro**: todas tienen usuario real, resultado observable desde fuera y caben en una rama corta de un agente.

> **Criterio de bloqueo**: una historia está bloqueada si depende de una pieza que **no existe** todavía: la cola publicada por Guardrails (US-GRD-015, bloqueada a su vez) o el factor de autenticación fuera de banda del SO (Q-GRD-19). Las piezas que **existen** pero aún no están construidas (TS-CKP-n, SPIKE-CKP-001, historias de otras features, enmiendas DEP-CKP-n) son dependencias, no bloqueos.
>
> **Transversales (no son historias)**: el saneado de texto no confiable (SEC-12), el gate de 100 ms y la fuente única (el motor) los fija INF-CKP-001 y los verifica US-CKP-001; el resto de historias los heredan. i18n en/es y accesibilidad se verifican en US-CKP-005 y aplican a todas. Linux y Windows: Pendiente: etapa de validación multiplataforma (S-CKP-4).

---

## Índice de Historias

| ID | Título | Ola | Prioridad | Status |
|----|--------|-----|-----------|--------|
| [US-CKP-001](./user-stories/US-CKP-001-flota-en-vivo.md) | El desarrollador ve en vivo qué agente trabaja en cada worktree de un repo | 0 | Must | expanded |
| [US-CKP-002](./user-stories/US-CKP-002-orden-atencion-terminadas.md) | La lista pone primero lo que pide atención y no se llena de sesiones viejas | 1 | Must | expanded |
| [US-CKP-003](./user-stories/US-CKP-003-estados-motor-conexion.md) | La TUI dice en qué estado está el motor y qué hacer en cada caso | 1 | Must | expanded |
| [US-CKP-004](./user-stories/US-CKP-004-selector-repo-preferencias.md) | El desarrollador cambia de repo y la TUI recuerda cómo la dejó | 2 | Should | expanded |
| [US-CKP-005](./user-stories/US-CKP-005-terminal-pequena-sin-color.md) | La TUI se puede usar en una terminal pequeña, sin color o en ASCII | 1 | Must | expanded |
| [US-CKP-006](./user-stories/US-CKP-006-conflictos-previstos.md) | El desarrollador ve qué agentes van a chocar antes de hacer merge | 2 | Must | expanded |
| [US-CKP-007](./user-stories/US-CKP-007-frescura-prediccion.md) | Una predicción vieja nunca se presenta como actual | 3 | Must | expanded |
| [US-CKP-008](./user-stories/US-CKP-008-alerta-conflicto-nuevo.md) | El desarrollador se entera en la TUI cuando aparece un conflicto previsto nuevo | 3 | Must | expanded |
| [US-CKP-009](./user-stories/US-CKP-009-base-pendiente.md) | Con la rama base sin confirmar, la predicción contra la base queda pendiente | 3 | Must | expanded |
| [US-CKP-010](./user-stories/US-CKP-010-kpi-deteccion.md) | El desarrollador sabe qué parte de los conflictos reales se vio antes de ocurrir | 3 | Must | expanded |
| [US-CKP-011](./user-stories/US-CKP-011-cli-solo-lectura.md) | El desarrollador consulta la flota y los conflictos desde la línea de comandos | 3 | Should | expanded |
| [US-CKP-012](./user-stories/US-CKP-012-ver-diff.md) | El desarrollador revisa lo que un agente integraría antes de hacer merge | 1 | Must | expanded |
| [US-CKP-013](./user-stories/US-CKP-013-abrir-en-editor.md) | El desarrollador abre el worktree de un agente en su editor | 2 | Should | expanded |
| [US-CKP-014](./user-stories/US-CKP-014-integrar-rama-agente.md) | El desarrollador integra la rama de un agente con una tecla y puede deshacerlo | 2 | Must | expanded |
| [US-CKP-015](./user-stories/US-CKP-015-rebasar-rama-agente.md) | El desarrollador pone al día la rama de un agente sobre la base | 3 | Must | expanded |
| [US-CKP-016](./user-stories/US-CKP-016-operacion-detenida.md) | Un merge o rebase que choca queda detenido y el desarrollador decide cómo salir | 4 | Must | expanded |
| [US-CKP-017](./user-stories/US-CKP-017-descartar-worktree.md) | El desarrollador descarta el trabajo de un agente sin miedo a perderlo | 3 | Must | expanded |
| [US-CKP-018](./user-stories/US-CKP-018-crear-worktree.md) | El desarrollador prepara un worktree para un agente nuevo | 3 | Must | expanded |
| [US-CKP-019](./user-stories/US-CKP-019-denegada-excepcion-consciente.md) | El desarrollador entiende por qué Guardrails frena una acción y puede hacer una excepción consciente | 3 | Must | expanded |
| [US-CKP-020](./user-stories/US-CKP-020-trabajo-de-otro-actor.md) | Tocar el trabajo de otro actor exige confirmar el plan concreto | 4 | Must | expanded |
| [US-CKP-021](./user-stories/US-CKP-021-historial-aviso-purga.md) | El desarrollador ve en la TUI el historial de operaciones y el aviso de purga | 2 | Must | expanded |
| [US-CKP-022](./user-stories/US-CKP-022-grafo-carriles.md) | El desarrollador ve crecer la rama de cada agente sobre la base | 2 | Must | expanded |
| [US-CKP-023](./user-stories/US-CKP-023-cola-confirmacion.md) | El desarrollador aprueba o rechaza desde la TUI las acciones que un agente deja en espera | — | Should | blocked |

---

## Orden y paralelismo (DAG)

```
INF-CKP-001, TS-CKP-004, US-GRP-001/002/007/008 ─► CKP-001 ─┬─► CKP-002 ─► CKP-004
                                                            ├─► CKP-003
                                                            ├─► CKP-005 ─► CKP-022  [DEP-CKP-2]
                                                            ├─► CKP-012              [DEP-CKP-3]
                                                            ├─► CKP-013              [DEP-CKP-12, 13]
                                                            └─► CKP-021 (+ US-TMC-006, 012, 016) [DEP-CKP-5]

SPIKE-CKP-001 ─► TS-CKP-001 ─► CKP-006 (+ 001, US-GRD-014) ─┬─► CKP-007
                                                            ├─► CKP-008 (+ 002)
                                                            ├─► CKP-009
                                                            ├─► CKP-010  [DEP-CKP-11, 14]
                                                            └─► CKP-011

TS-TMC-004 ─► TS-CKP-002 ─► TS-CKP-003 ─► CKP-014 (+ 001, US-TMC-001, 002) ─┬─► CKP-015 ─┐
                                                                             ├─► CKP-017 ─┼─► CKP-020 (+ US-TMC-013)
                                                                             ├─► CKP-018  │
                                                                             ├─► CKP-019 (+ US-GRD-006, 008)
                                                                             └────────────┴─► CKP-016 (+ 015, US-TMC-015) [DEP-CKP-14]

US-GRD-015 + factor OS (Q-GRD-19) ─► CKP-023   ⛔ bloqueada
```

Las olas cuentan desde US-CKP-001 y suponen integrados sus enablers. Las tres ramas del DAG (lectura, predicción, escritura) avanzan en paralelo en cuanto lo permiten sus enablers.

- **Ola 0 — esqueleto andante**: US-CKP-001.
- **Ola 1** (en paralelo): US-CKP-002, 003, 005 y 012.
- **Ola 2** (en paralelo): US-CKP-004, 013, 021 y 022 (lectura); US-CKP-006 (cuando esté TS-CKP-001); US-CKP-014 (cuando estén TS-CKP-002 y TS-CKP-003).
- **Ola 3** (en paralelo): US-CKP-007, 008, 009, 010 y 011 (tras 006); US-CKP-015, 017, 018 y 019 (tras 014).
- **Ola 4**: US-CKP-016 (tras 014 y 015) y US-CKP-020 (tras 014, 015 y 017).
- **Bloqueada**: US-CKP-023 (cola de US-GRD-015 y factor fuera de banda de Q-GRD-19).

> **Orden de entrega (Q-CKP-24)**: con varias historias listas a la vez, la flota toma primero las de BR-04 (001, 002, 003, 005), luego BR-06 (006 a 011), luego BR-07 (012 a 021) y por último BR-05 (022). US-CKP-022 está lista en la ola 2, pero se toma al final.
>
> **Ruta crítica**: SPIKE-CKP-001 → TS-CKP-001 → US-CKP-006 → US-CKP-007 (la predicción, que sostiene la demo del BRD § 13). En escritura: TS-TMC-004 → TS-CKP-002 → TS-CKP-003 → US-CKP-014 → US-CKP-015 → US-CKP-016.
>
> **Dependencias blandas**: US-CKP-002 sube filas por ⚡ y ⛔ cuando existan US-CKP-006 y US-CKP-019; US-CKP-010 cuenta los conflictos de merges del Cockpit cuando exista US-CKP-016; US-CKP-014 verifica el aviso por ⚡ cuando exista US-CKP-006. Esos escenarios se verifican al integrarse la otra historia; no frenan el arranque.

---

## Cobertura de reglas (regla → historias)

Las 46 reglas BR-CKP quedan cubiertas por al menos una historia. Los IDs omiten el prefijo `BR-CKP-`.

| Regla | Historias | Regla | Historias |
|-------|-----------|-------|-----------|
| VAL-001 | US-CKP-018 | WF-005 | US-CKP-009, 014, 015, 018 |
| VAL-002 | US-CKP-001 | WF-006 | US-CKP-023 ⛔ |
| VAL-003 | US-CKP-013 | WF-007 | US-CKP-008 |
| CALC-001 | US-CKP-001 | WF-008 | US-CKP-016 |
| CALC-002 | US-CKP-006 | AUTH-001 | US-CKP-019 |
| CALC-003 | US-CKP-007 | AUTH-002 | US-CKP-019 |
| CALC-004 | US-CKP-022 | AUTH-003 | US-CKP-020 |
| CALC-005 | US-CKP-012 | AUTH-004 | US-CKP-023 ⛔ |
| ELIG-001 | US-CKP-014, 015, 017, 018 | CONS-001 | US-CKP-001 |
| ELIG-002 | US-CKP-014 | CONS-002 | US-CKP-014 |
| ELIG-003 | US-CKP-015 | CONS-003 | US-CKP-001, 021 |
| ELIG-004 | US-CKP-017 | CONS-004 | US-CKP-014 |
| ELIG-005 | US-CKP-018 | CONS-005 | US-CKP-010 |
| ELIG-006 | US-CKP-012, 013 | CONS-006 | US-CKP-004 |
| WF-001 | US-CKP-001, 002, 008 | CONS-007 | US-CKP-011 |
| WF-002 | US-CKP-014, 021 | TIME-001 | US-CKP-001 |
| WF-003 | US-CKP-016 | TIME-002 | US-CKP-002 |
| WF-004 | US-CKP-003 | TIME-003 | US-CKP-023 ⛔ |
| EDGE-001 | US-CKP-003, 009, 022 | TIME-004 | US-CKP-021 |
| EDGE-002 | US-CKP-016, 017 | EDGE-006 | US-CKP-002, 005 |
| EDGE-003 | US-CKP-003 | EDGE-007 | US-CKP-013 |
| EDGE-004 | US-CKP-002, 012, 015 | EDGE-008 | US-CKP-017 |
| EDGE-005 | US-CKP-005, 022 | EDGE-009 | US-CKP-014 |

⛔ = cubierta por una historia bloqueada: WF-006, AUTH-004 y TIME-003 son las mismas reglas que business-rules.md ya marca "Bloqueadas por DEP-CKP-8".

### Cobertura de capacidades del BRD

| Capacidad | Historias |
|-----------|-----------|
| BR-04 Lista en vivo | US-CKP-001 a 005, 011 |
| BR-05 Grafo en vivo | US-CKP-022 |
| BR-06 Predicción de conflictos | US-CKP-006 a 011 |
| BR-07 Acciones por agente | US-CKP-012 a 021, 023 ⛔ |

---

## Decisiones

Ninguna decisión de este índice cambia una decisión del requerimiento.

| # | Decisión | Validación |
|---|----------|------------|
| D-US-CKP-1 | Dos esqueletos andantes: US-CKP-001 (lectura) y US-CKP-014 (escritura, integrar). El flujo de escritura lo fija la primera historia que lo usa y las demás lo reutilizan. | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO |
| D-US-CKP-2 | La predicción se parte por resultado observable: niveles y pares (006), frescura (007), alerta (008), base pendiente (009), KPI (010), CLI (011). La demo del BRD § 13 es el primer escenario de US-CKP-006 (Q-CKP-25). | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO |
| D-US-CKP-3 | SPIKE-CKP-001 y las enmiendas DEP-CKP-n son dependencias, no bloqueos: el artefacto existe. Solo US-CKP-023 se bloquea, por piezas que no existen (cola de US-GRD-015 y factor de Q-GRD-19). | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO |
| D-US-CKP-4 | Las historias ya cubren BR-CKP-WF-008 (Cancelar) y los ajustes de ELIG-004 (worktree bloqueado), EDGE-008 (repos anidados), AUTH-003 (rebase) y CONS-005 (equivalencia del mismo par) que introducen ADR-CKP-001 y ADR-CKP-002. | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO |
| D-US-CKP-5 | El historial y el aviso de purga (US-CKP-021) son una historia propia: es la única superficie de la Time Machine en la TUI y soporta el Deshacer "después de 5 s" de BR-CKP-WF-002. | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO |
