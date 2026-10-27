---
id: US-CKP-007
title: "Una predicción vieja nunca se presenta como actual"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-27
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-006
  specs:
    - DS-US-CKP-007
tags:
  - cockpit
  - prediccion-conflictos
  - frescura
  - must
---

# US-CKP-007: Una predicción vieja nunca se presenta como actual

## Descripción

**Como** desarrollador orquestador, **quiero** ver cuándo se calculó cada resultado de la predicción y si se está recalculando, **para** no integrar confiado en un "sin conflictos" que ya no es verdad.

**Valor**: BR-06 (Must). Una predicción vieja presentada como actual engaña más que no tenerla.

## Reglas cubiertas

BR-CKP-CALC-003 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-006.
- **Técnicas**: TS-CKP-001. La cifra de 5 s p95 es el supuesto S-CKP-1 y queda provisional hasta los resultados de SPIKE-CKP-001.

## Criterios de Aceptación

**Escenario: "Calculando" en el cálculo inicial**

Dado el daemon recién arrancado con 10 worktrees en "shop"
Cuando el desarrollador abre la TUI antes de que termine el primer cálculo
Entonces cada par sin resultado dice "calculando" y ninguno dice "sin conflictos"

**Escenario: Tras un commit se recalculan solo los pares de ese worktree**

Dado el par "claude-1 ↔ claude-2" calculado hace 40 s y el par "claude-3 ↔ claude-4" calculado hace 40 s
Cuando "claude-2" commitea
Entonces el par "claude-1 ↔ claude-2" se marca "desactualizado, recalculando" con su antigüedad
  Y el par "claude-3 ↔ claude-4" no se recalcula
  Y el resultado nuevo del par de "claude-2" llega en 5 s p95 o menos con 10 worktrees

**Escenario: Mover la base pone en "calculando" los pares contra la base**

Dado resultados vigentes para todos los pares de "shop"
Cuando "main" avanza por un merge
Entonces los pares contra "main" pasan a "calculando" hasta tener resultado nuevo

**Escenario: La antigüedad siempre se ve**

Dado un resultado calculado hace 3 minutos sin cambios posteriores
Cuando el desarrollador mira el par
Entonces el par muestra "hace 3 min" junto al resultado

## Requisitos Técnicos

Resumen de la [Dev Spec DS-US-CKP-007](../dev-specs/US-CKP-007-frescura-prediccion.md):

- La TUI deriva la antigüedad de `computed_at_utc_ms` en el `Msg::Tick` de 1 Hz, sin pedir nada al daemon; si sale negativa vale 0. Formatos "hace N s/min/h/d" con cadenas enteras en el catálogo propio `freshness` (en/es).
- Cada `state` del contrato ([DS-TS-CKP-001 § Contrato de API](../dev-specs/TS-CKP-001-predictor-conflictos.md#contrato-de-api)) se mapea al `Prediction` de `ConflictAlert`, que gana la variante `Pending`.
- `recalculating` dice "desactualizado · hace N · recalculando", también sin ⚡ ni ⚠. `calculating` se agrega en "Calculando N de M pares". `pending` y `not-computable` se agrupan por motivo y nunca dicen "sin conflictos".
- "Sin conflictos previstos · hace N" solo con todos los pares `current` y sin ⚡ ni ⚠.
- El motor garantiza qué pares se recalculan, los 5 s p95 (S-CKP-1) y la base movida (TS-CKP-001: T003, T012 y T009). La DS añade las pruebas de la TUI con eventos `prediction.changed` simulados.
- Bloqueada por el PR-B de TS-CKP-001 (T009) y por DS-US-CKP-006.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** [DS-US-CKP-007](../dev-specs/US-CKP-007-frescura-prediccion.md).
