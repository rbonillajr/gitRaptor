---
id: TS-CKP-011
title: "Confirmar la base en runtime refresca la base publicada y replanifica los pares contra la base"
type: ts
status: implemented
feature: cockpit
domain: GRP
priority: high
complexity: medium
milestone: M2
created: 2026-11-08
updated: 2026-11-09
blocked_by: []
related:
  adrs: [ADR-CKP-001, ADR-GRP-009, ADR-GRP-011, ADR-GRP-013, ADR-GRP-016, ADR-GRD-004, ADR-GRD-005]
  stories: [US-CKP-006, TS-CKP-001, US-GRD-014, US-GRP-012, US-GRP-016]
  specs: [DS-TS-CKP-011]
ado:
  id: null
  url: null
tags: [cockpit, prediccion-conflictos, rama-base, guardrails, daemon, modulos-daemon, g-b, br-06]
---

## TS-CKP-011: Confirmar la base en runtime refresca la base publicada y replanifica los pares contra la base

**Valor**: tras confirmar la rama base con el daemon en marcha, el par de cada worktree contra la base deja de quedarse "pendiente" y la TUI muestra su ⚡ o su ⚠ sin reiniciar el daemon.

### Descripción

**Como** Arquitecto
**Quiero** que, cuando Guardrails escribe la confirmación de la base en el almacén del repo, el motor publique la base nueva y avise al predictor para que vuelva a planificar ese repo
**Para** cerrar el hueco G-B de [DS-US-CKP-006](../dev-specs/US-CKP-006-conflictos-previstos.md) (nota N4) y que pase el escenario 3 de [US-CKP-006](../user-stories/US-CKP-006-conflictos-previstos.md), con la predicción de [TS-CKP-001](./TS-CKP-001-predictor-conflictos.md) sin cambios de contrato

**Estado actual (verificado en el código el 2026-11-08).** Hay dos escrituras de la confirmación con el daemon en marcha: `guard.install` (`crates/core/src/guardrails/install.rs:1178`, vía `set_guard_keys`) y `guard.confirm` (`crates/core/src/guardrails/confirm.rs:147`, llamada desde `crates/core/src/daemon/guard.rs:279` y `:349`). Ninguna toca lo publicado: `Predict::base()` (`crates/core/src/daemon/modules/predict.rs:480`) lee `divergence[repo].base` de la instantánea, y la primera fase de cada cambio observado conserva esa base (`crates/core/src/daemon/repos.rs:712`). El almacén solo se vuelve a leer al reconciliar el repo entero (`repo_base`, `crates/core/src/daemon/mod.rs:1356`), así que el par contra la base sigue `pending(base_unconfirmed)` (`crates/core/src/predict/pairs.rs:79`) hasta el próximo arranque o despertar.

**Decisión del orquestador (2026-11-08), validada por Arquitecto**: avisa el bucle del daemon y no Guardrails. Al terminar una petición `guard.install` o `guard.confirm`, el bucle compara la base del almacén con la publicada; si difieren, publica la base nueva (primera fase, sin leer Git), recuenta el adelanto y retraso (segunda fase, ADR-GRP-011 § 2) y después avisa a los módulos con un punto nuevo del trait de módulos, `base_changed(repo_id)` (ADR-GRP-016 § 5). El predictor lo traduce a un mensaje de su hilo que marca el repo para recalcular. No hay evento nuevo en el canal ni lógica de Git nueva: la replanificación usa `read_plan` tal como está.

### Alcance Técnico

- **Comparar**, al final de cada petición `guard.install` o `guard.confirm` del bucle del daemon, la base que guarda el almacén del repo con la base publicada; si son iguales no se publica nada (idempotencia).
- **Publicar** la base nueva en la instantánea (`divergence[repo].base` y la vista `RepoView.base`) en la misma sección crítica, y recontar el adelanto y retraso de sus worktrees como segunda fase.
- **Añadir** al trait de módulos del daemon el punto `base_changed(repo_id)`, con implementación vacía por defecto, llamado desde un solo sitio del daemon y siempre después de publicar la base.
- **Replanificar** en el módulo `predict` el repo avisado: el pase lee la base ya publicada; un repo dormido no hace nada (al despertar se replanifica con la base nueva); en ahorro de energía queda diferido como cualquier otro cambio.
- **Descartar** los resultados del pool calculados con la base anterior: la comprobación de triple de `RepoPairs::complete` ya lo hace; se cubre con una prueba.
- **Des-ignorar** la prueba de punta a punta del escenario 3 de US-CKP-006 (`apps/cli/tests/tui_conflicts.rs`, `a_worktree_against_the_confirmed_base_shows_the_conflict`).
- **Fuera de alcance**: un evento del canal que lleve la base (los clientes la ven en la siguiente instantánea); desconfirmar la base, que hoy no tiene camino de escritura (el refresco cubre cualquier cambio de base y sirve el día que exista); la frescura visible del par (US-CKP-007).

### Plan de Verificación

#### Pruebas Automatizadas

- **Confirmar replanifica**: con el daemon en marcha y la base sin confirmar, el par de un worktree contra la base pasa de `pending` a `calculating` y luego a `current` con su nivel, tras `guard.confirm` y tras `guard.install`.
- **Idempotencia**: confirmar la misma base dos veces publica una sola vez y avisa una sola vez al predictor.
- **Base que vuelve a cambiar**: confirmar otra rama como base replanifica otra vez contra la rama nueva, y el resultado tardío del pool con la base anterior no se publica.
- **Repo dormido**: el aviso no despierta el predictor; al pasar a activo, el par sale calculado contra la base confirmada.
- **Orden**: los módulos oyen `base_changed` en el orden de registro y después de la publicación de la base.
- **Punta a punta**: el escenario 3 de US-CKP-006 pasa en macOS con el daemon real.

#### Verificación Manual / Sandbox

- En un repo de demo con un worktree que cambia las mismas líneas que `main`: abrir `raptor`, ver el par contra la base "pendiente", ejecutar `raptor guard confirm` desde otra terminal y ver el ⚡ contra `main` sin reiniciar el daemon.

> Dev Spec: [`dev-specs/TS-CKP-011-base-confirmada-replanifica.md`](../dev-specs/TS-CKP-011-base-confirmada-replanifica.md) — `DS-TS-CKP-011` (status: implemented).

Implementado en: PR #343. El escenario 3 de US-CKP-006 pasa sin `#[ignore]`; el repo dormido se prueba por lo observable (la confirmación lo despierta antes de refrescar la base) y la rama dormida de `Msg::BaseChanged` con una unitaria (nota N5 de la Dev Spec).
