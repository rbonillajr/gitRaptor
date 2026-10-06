---
mode: bulk
generated: 2026-10-04T00:00Z
updated: 2026-10-04
generator: product-owner
total_artifacts: 24
expanded: 24
approved: 0
blocked:
  - US-CKP-023
---

# User Stories — INDEX: Cockpit

> Modo lean: cada historia es un contrato compacto para un agente (valor en una línea, reglas cubiertas, dependencias y Gherkin). Sin story points ni narrativas largas. Cada historia vive en su archivo de `user-stories/`.

---

## Contexto del Feature

**Feature**: Cockpit (F-001-02) · **Epic**: E-001 — MVP Fase 1
**Prioridad**: Alta (BR-04 a BR-07 Must; abrir en el editor, selector de repo y CLI de solo lectura, Should)
**Estado**: 23 historias listas para Dev Spec; 1 bloqueada (US-CKP-023). Requerimiento aprobado por Rene Bonilla (marca de aprobación del 2026-10-05).

**Contexto**: [`context.md`](./context.md) (CTX-CKP-001) · **Reglas**: [`business-rules.md`](./business-rules.md) (BR-CKP-001) · **Enablers**: [`technical-stories.md`](./technical-stories.md) (ADR-CKP-001 a 003, TS-CKP-001 a 005, INF-CKP-001, SPIKE-CKP-001)

---

## Análisis de división

- **Flujo**: actor = desarrollador orquestador. Resultado = ver la flota, saber quién choca y actuar sobre el trabajo de cada agente sin perder nada.
- **Patrón**: camino feliz primero, en el orden de entrega Q-CKP-24 (BR-04 → BR-06 → BR-07 → BR-05). Esqueleto andante de lectura: **US-CKP-001**. Esqueleto de escritura: **US-CKP-014** (integrar), que fija el flujo preparar → confirmar el plan → ejecutar → Deshacer; rebase, descartar, crear y los casos límite (US-CKP-024) lo reutilizan.
- **Filtro**: todas tienen usuario real, resultado observable desde fuera y caben en una rama corta de un agente.

> **Criterio de bloqueo**: una historia está bloqueada si depende de una pieza **sin decisión ni dueño**: la cola publicada por Guardrails (US-GRD-015, bloqueada a su vez) y el factor de autenticación fuera de banda del SO (Q-GRD-19, decisión en curso en otra rama). Las piezas con decisión y dueño (TS-CKP-n, SPIKE-CKP-001, historias de otras features, enmiendas DEP-CKP-n) son dependencias. Si una enmienda DEP-CKP-n no está implementada, es **condición de arranque** de la historia (columna "Arranque"): la historia no empieza sin ella, pero no está bloqueada.
>
> **Transversales (no son historias)**: el saneado de texto no confiable (SEC-12), el gate de 100 ms y la fuente única (el motor) los fija INF-CKP-001 y los verifica US-CKP-001; el resto los hereda. i18n en/es y accesibilidad se verifican en US-CKP-005 y aplican a todas. Linux y Windows: Pendiente: etapa de validación multiplataforma (S-CKP-4).

---

## Índice de Historias

| ID | Título | Ola | Prioridad | Arranque (enablers y enmiendas) | Status |
|----|--------|-----|-----------|----------------------------------|--------|
| [US-CKP-001](./user-stories/US-CKP-001-flota-en-vivo.md) | El desarrollador ve en vivo qué agente trabaja en cada worktree de un repo | 0 | Must | INF-CKP-001, TS-CKP-004, TS-GRP-004 (DEP-CKP-6), TS-CKP-005 | expanded |
| [US-CKP-002](./user-stories/US-CKP-002-orden-atencion-terminadas.md) | La lista pone primero lo que pide atención y no se llena de sesiones viejas | 1 | Must | DEP-CKP-4, TS-CKP-005 | expanded |
| [US-CKP-003](./user-stories/US-CKP-003-estados-motor-conexion.md) | La TUI dice en qué estado está el motor y qué hacer en cada caso | 1 | Must | DEP-CKP-12 (solo el autoarranque), TS-CKP-005 | expanded |
| [US-CKP-004](./user-stories/US-CKP-004-selector-repo-preferencias.md) | El desarrollador cambia de repo y la TUI recuerda cómo la dejó | 2 | Should | DEP-CKP-11, TS-CKP-005 | expanded |
| [US-CKP-005](./user-stories/US-CKP-005-terminal-pequena-sin-color.md) | La TUI se puede usar en una terminal pequeña, sin color o en ASCII | 1 | Must | TS-CKP-004, TS-CKP-005 | expanded |
| [US-CKP-006](./user-stories/US-CKP-006-conflictos-previstos.md) | El desarrollador ve qué agentes van a chocar antes de hacer merge | 2 | Must | SPIKE-CKP-001 → TS-CKP-001, TS-CKP-005 | expanded |
| [US-CKP-007](./user-stories/US-CKP-007-frescura-prediccion.md) | Una predicción vieja nunca se presenta como actual | 3 | Must | TS-CKP-001, TS-CKP-005 | expanded |
| [US-CKP-008](./user-stories/US-CKP-008-alerta-conflicto-nuevo.md) | El desarrollador se entera en la TUI cuando aparece un conflicto previsto nuevo | 3 | Must | TS-CKP-001, TS-CKP-005 | expanded |
| [US-CKP-009](./user-stories/US-CKP-009-base-pendiente.md) | Con la rama base sin confirmar, la predicción contra la base queda pendiente | 3 | Must | TS-CKP-001, TS-CKP-005 | expanded |
| [US-CKP-010](./user-stories/US-CKP-010-kpi-deteccion.md) | El desarrollador sabe qué parte de los conflictos reales se vio antes de ocurrir | 3 | Must | DEP-CKP-11, DEP-CKP-14 | expanded |
| [US-CKP-011](./user-stories/US-CKP-011-cli-solo-lectura.md) | El desarrollador consulta la flota y los conflictos desde la línea de comandos | 3 | Should | TS-CKP-001 | expanded |
| [US-CKP-012](./user-stories/US-CKP-012-ver-diff.md) | El desarrollador revisa lo que un agente integraría antes de hacer merge | 1 | Must | DEP-CKP-3, TS-CKP-005 | expanded |
| [US-CKP-013](./user-stories/US-CKP-013-abrir-en-editor.md) | El desarrollador abre el worktree de un agente en su editor | 3 | Should | DEP-CKP-11, 12, 13, TS-CKP-005 | expanded |
| [US-CKP-014](./user-stories/US-CKP-014-integrar-rama-agente.md) | El desarrollador integra la rama de un agente con una tecla y puede deshacerlo | 2 | Must | TS-TMC-004 → TS-CKP-002 → TS-CKP-003, TS-CKP-005 | expanded |
| [US-CKP-015](./user-stories/US-CKP-015-rebasar-rama-agente.md) | El desarrollador pone al día la rama de un agente sobre la base | 3 | Must | TS-CKP-002, TS-CKP-003, TS-CKP-005 | expanded |
| [US-CKP-016](./user-stories/US-CKP-016-operacion-detenida.md) | Un merge o rebase que choca queda detenido y el desarrollador decide cómo salir | 4 | Must | TS-CKP-002, DEP-CKP-14, TS-CKP-005 | expanded |
| [US-CKP-017](./user-stories/US-CKP-017-descartar-worktree.md) | El desarrollador descarta el trabajo de un agente sin miedo a perderlo | 3 | Must | TS-CKP-002, TS-CKP-003, TS-CKP-005 | expanded |
| [US-CKP-018](./user-stories/US-CKP-018-crear-worktree.md) | El desarrollador prepara un worktree para un agente nuevo | 3 | Must | TS-CKP-002, TS-CKP-003, TS-CKP-005 | expanded |
| [US-CKP-019](./user-stories/US-CKP-019-denegada-excepcion-consciente.md) | El desarrollador entiende por qué Guardrails frena una acción y puede hacer una excepción consciente | 3 | Must | TS-CKP-003 (DEP-CKP-10), TS-CKP-005 | expanded |
| [US-CKP-020](./user-stories/US-CKP-020-trabajo-de-otro-actor.md) | Tocar el trabajo de otro actor exige confirmar el plan concreto | 3 | Must | TS-TMC-004, TS-CKP-002, TS-CKP-005 | expanded |
| [US-CKP-021](./user-stories/US-CKP-021-historial-aviso-purga.md) | El desarrollador ve en la TUI el historial de operaciones y el aviso de purga | 3 | Must | DEP-CKP-5, TS-CKP-005 | expanded |
| [US-CKP-022](./user-stories/US-CKP-022-grafo-carriles.md) | El desarrollador ve crecer la rama de cada agente sobre la base | 2 | Must | DEP-CKP-2, TS-CKP-005 | expanded |
| [US-CKP-023](./user-stories/US-CKP-023-cola-confirmacion.md) | El desarrollador aprueba o rechaza desde la TUI las acciones que un agente deja en espera | — | Should | ⛔ US-GRD-015 + factor de Q-GRD-19, TS-CKP-005 | blocked |
| [US-CKP-024](./user-stories/US-CKP-024-integrar-casos-limite.md) | Integrar sigue siendo seguro cuando el estado cambia, choca o la base no está sacada | 3 | Must | TS-CKP-002, TS-CKP-005 | expanded |

---

## Orden y paralelismo (DAG)

```
INF-CKP-001, TS-CKP-004, US-GRP-001/002/007/008 ─► CKP-001 ─┬─► CKP-002 ─► CKP-004 ─► CKP-013
                                                            ├─► CKP-003
                                                            ├─► CKP-005 ─┐
                                                            └─► CKP-012 ─┴─► CKP-022
                                                                 └────────────────► CKP-021 (+ 014, US-TMC-006, 012, 016)

SPIKE-CKP-001 ─► TS-CKP-001 ─► CKP-006 (+ 001, US-GRD-014, US-GRP-016) ─┬─► CKP-007
                                                                        ├─► CKP-008 (+ 002)
                                                                        ├─► CKP-009
                                                                        ├─► CKP-010 (+ 004)
                                                                        └─► CKP-011

TS-TMC-004 ─► TS-CKP-002 ─► TS-CKP-003 ─► CKP-014 (+ 001, US-TMC-001, 002) ─┬─► CKP-015 ─► CKP-016 (+ US-TMC-015)
                                                                             ├─► CKP-017
                                                                             ├─► CKP-018
                                                                             ├─► CKP-019 (+ 002, US-GRD-006, 007, 008)
                                                                             ├─► CKP-020 (+ US-TMC-013)
                                                                             └─► CKP-024 (+ 006)

US-GRD-015 + factor OS (Q-GRD-19) ─► CKP-023   ⛔ bloqueada
```

Las olas cuentan desde US-CKP-001 y suponen integrados sus enablers. Las tres ramas del DAG (lectura, predicción, escritura) avanzan en paralelo en cuanto lo permiten sus enablers.

- **Ola 0 — esqueleto andante**: US-CKP-001.
- **Ola 1** (en paralelo): US-CKP-002, 003, 005 y 012.
- **Ola 2** (en paralelo): US-CKP-004 y 022 (lectura); US-CKP-006 (cuando esté TS-CKP-001); US-CKP-014 (cuando estén TS-CKP-002 y TS-CKP-003).
- **Ola 3** (en paralelo): US-CKP-013 y 021 (lectura); US-CKP-007, 008, 009, 010 y 011 (tras 006); US-CKP-015, 017, 018, 019, 020 y 024 (tras 014).
- **Ola 4**: US-CKP-016 (tras 014 y 015).
- **Bloqueada**: US-CKP-023.

> **Orden de entrega (Q-CKP-24)**: con varias historias listas a la vez, la flota toma primero las de BR-04 (001 a 005), luego BR-06 (006 a 011), luego BR-07 (012 a 021 y 024) y por último BR-05 (022). US-CKP-022 está lista en la ola 2, pero se toma al final.
>
> **Ruta crítica**: SPIKE-CKP-001 → TS-CKP-001 → US-CKP-006 → US-CKP-007 (la predicción, que sostiene la demo del BRD § 13). En escritura: TS-TMC-004 → TS-CKP-002 → TS-CKP-003 → US-CKP-014 → US-CKP-015 → US-CKP-016.

### Contratos compartidos (quién los fija)

Dos historias de la misma ola no implementan el mismo contrato: lo fija la primera y las demás lo reutilizan.

| Contrato | Lo fija | Lo reutilizan |
|----------|---------|---------------|
| Consultas bajo demanda (N8 de ADR-CKP-003) | US-CKP-012 | US-CKP-021, US-CKP-022 |
| Preferencias de la TUI (N9 de ADR-CKP-003) | US-CKP-004 | US-CKP-010, US-CKP-013 |
| Flujo de escritura, confirmación del plan y Deshacer desde la TUI | US-CKP-014 | US-CKP-015, 017, 018, 020, 021, 024 |
| Comando reservado desde la TUI (excepción consciente) | US-CKP-019 | confirmar la base desde la TUI (US-CKP-009 usa la CLI mientras tanto) |

> **Dependencias blandas**: US-CKP-002 sube filas por ⚡ cuando exista US-CKP-006 (por ⛔ lo verifica US-CKP-019); US-CKP-010 cuenta los conflictos de merges del Cockpit cuando exista US-CKP-016. Esos escenarios se verifican al integrarse la otra historia; no frenan el arranque.

---

## Cobertura de reglas (regla → historias)

Las 46 reglas BR-CKP (las 45 de `main` más BR-CKP-WF-008, que añade ADR-CKP-002) quedan cubiertas por al menos una historia. Los IDs omiten el prefijo `BR-CKP-`.

| Regla | Historias | Regla | Historias |
|-------|-----------|-------|-----------|
| VAL-001 | US-CKP-018 | WF-005 | US-CKP-009, 014, 015, 018 |
| VAL-002 | US-CKP-001 | WF-006 | US-CKP-023 ⛔ |
| VAL-003 | US-CKP-013 | WF-007 | US-CKP-008 |
| CALC-001 | US-CKP-001 | WF-008 | US-CKP-016 |
| CALC-002 | US-CKP-006 | AUTH-001 | US-CKP-019 |
| CALC-003 | US-CKP-007 | AUTH-002 | US-CKP-019 |
| CALC-004 | US-CKP-022 | AUTH-003 | US-CKP-014, 015, 017, 020 |
| CALC-005 | US-CKP-012 | AUTH-004 | US-CKP-023 ⛔ |
| ELIG-001 | US-CKP-014, 015, 017, 018, 024 | CONS-001 | US-CKP-001 |
| ELIG-002 | US-CKP-014, 024 | CONS-002 | US-CKP-014 |
| ELIG-003 | US-CKP-015 | CONS-003 | US-CKP-001, 021 |
| ELIG-004 | US-CKP-017 | CONS-004 | US-CKP-024 |
| ELIG-005 | US-CKP-018 | CONS-005 | US-CKP-010 |
| ELIG-006 | US-CKP-012, 013 | CONS-006 | US-CKP-004 |
| WF-001 | US-CKP-001, 002, 008, 019 | CONS-007 | US-CKP-011 |
| WF-002 | US-CKP-014, 021 | TIME-001 | US-CKP-001 |
| WF-003 | US-CKP-016 | TIME-002 | US-CKP-002 |
| WF-004 | US-CKP-003 | TIME-003 | US-CKP-023 ⛔ |
| EDGE-001 | US-CKP-003, 009, 022 | TIME-004 | US-CKP-021 |
| EDGE-002 | US-CKP-016, 017 | EDGE-006 | US-CKP-002, 005 |
| EDGE-003 | US-CKP-003 | EDGE-007 | US-CKP-013 |
| EDGE-004 | US-CKP-002, 012, 015 | EDGE-008 | US-CKP-017 |
| EDGE-005 | US-CKP-005, 022 | EDGE-009 | US-CKP-024 |

⛔ = cubierta por una historia bloqueada: WF-006, AUTH-004 y TIME-003 son las reglas que business-rules.md ya marca "Bloqueadas por DEP-CKP-8".

### Cobertura de capacidades del BRD

| Capacidad | Historias |
|-----------|-----------|
| BR-04 Lista en vivo | US-CKP-001 a 005, 011 |
| BR-05 Grafo en vivo | US-CKP-022 |
| BR-06 Predicción de conflictos | US-CKP-006 a 011 |
| BR-07 Acciones por agente | US-CKP-012 a 021, 024, 023 ⛔ |

---

## Decisiones

Ninguna decisión de este índice cambia una decisión del requerimiento. Todas: **Decisión del orquestador (2026-10-04), validada por Arquitecto/PO**.

| # | Decisión | Origen del ajuste |
|---|----------|-------------------|
| D-US-CKP-1 | Dos esqueletos andantes: US-CKP-001 (lectura) y US-CKP-014 (escritura). Los casos límite del merge (⚡, dos TUIs, commit entre preparar y ejecutar, destino con sesión, avance rápido sin worktree) van en US-CKP-024 para que el esqueleto quepa en una rama corta. | Partición pedida por el PO |
| D-US-CKP-2 | La confirmación ligada al plan forma parte del camino feliz de integrar, rebasar y descartar trabajo de un agente (ADR-CKP-002 § 3: la rama de un agente es trabajo de otro actor). US-CKP-020 queda con los casos límite de AUTH-003. | Ajuste del Arquitecto |
| D-US-CKP-3 | La predicción se parte por resultado observable: niveles y pares (006), frescura (007), alerta (008), base pendiente (009), KPI (010), CLI (011). La demo del BRD § 13 es el primer escenario de US-CKP-006 (Q-CKP-25). | Validado por ambos |
| D-US-CKP-4 | Solo US-CKP-023 se bloquea. SPIKE-CKP-001 y las enmiendas DEP-CKP-n tienen dueño: son dependencias o condiciones de arranque, no bloqueos. | Criterio redefinido por el Arquitecto |
| D-US-CKP-5 | Cada contrato compartido tiene una historia que lo fija (tabla de contratos) para que la flota no lo implemente dos veces en paralelo. | Ajuste del Arquitecto |
| D-US-CKP-6 | Confirmar la rama base es una acción reservada: US-CKP-009 la verifica con la CLI; ofrecerla desde la TUI reutiliza el comando reservado de US-CKP-019. | Ajuste del Arquitecto |
| D-US-CKP-7 | Las historias cubren ya BR-CKP-WF-008 (Cancelar, solo desde la TUI) y los ajustes de ELIG-004, EDGE-008, AUTH-003 y CONS-005 que introducen ADR-CKP-001 y ADR-CKP-002. Que WF-008 permita cancelar desde la CLI choca con CONS-007 (CLI de solo lectura): queda para el Arquitecto antes de la Dev Spec de US-CKP-016. | Detectado por el PO |
