---
mode: bulk
generated: 2026-11-10T00:00Z
updated: 2026-11-10
generator: product-owner
total_artifacts: 13
expanded: 13
approved: 0
blocked:
  - US-WTL-003
---

# User Stories — INDEX: Ciclo de vida del worktree

> Modo lean: cada historia es un contrato compacto para un agente (descripción con valor, Gherkin con los valores de las reglas, dependencias y bloqueos). Cada historia vive en su archivo de `user-stories/`.

---

## Contexto del Feature

**Feature**: Ciclo de vida del worktree · **Epic**: E-001 — MVP Fase 1
**Prioridad**: Must en M3 (US-WTL-001 a 005 y 013), Should en M4 / v0.1.0 (US-WTL-006 a 009), Could después de v0.1.0 (US-WTL-010 a 012).
**Estado**: requerimiento aprobado por Rene Bonilla desde el menú el 2026-11-10. 13 historias escritas (US-WTL-001 se partió en 001 y 013 el 2026-11-10); 7 `ready`, 6 `draft`.

**Contexto**: [`context.md`](./context.md) (CTX-WTL-001), reglas BR-WTL-001 a BR-WTL-016 en su sección «Reglas de negocio» · **Enablers**: los define el Arquitecto.

## Análisis de división

- **Flujo**: actor = desarrollador orquestador (también el novato en la app). Resultado = sumar un agente a la flota con una orden, saber en qué estado está y cerrar su worktree sin perder nada.
- **Corte**: una historia por capacidad de CTX-WTL-001. Esqueleto andante: **US-WTL-001** (`raptor new`); el resto la reutiliza o reutiliza US-CKP-017.
- **Criterio de bloqueo**: una historia figura en `blocked` si depende de una enmienda decidida y sin aplicar. Las del BRD § 6.4 y las de hooks de worktree de ADR-GRP-012 y de ADR-GRD-007 se aplicaron el 2026-11-10; solo queda la de los estados nuevos del agente de ADR-GRP-012 (US-WTL-003). No arranca hasta que se aplique. `blocked_by` lista solo historias.
- **Draft**: además de la bloqueada, las que tienen una pregunta abierta que cambia un escenario (US-WTL-006, 009) y las Could (US-WTL-010 a 012).

> **Transversales (no son historias)**: dónde vive la semilla (Pregunta 9), copy-on-write sin reflink (Pregunta 11) y la medición del disco ganado los define el Arquitecto. Linux y Windows: Pendiente: etapa de validación multiplataforma.

---

## Índice de Historias

| ID | Título | Prioridad | Hito | blocked_by | Bloqueos (enmiendas y preguntas) | Status |
|----|--------|-----------|------|------------|----------------------------------|--------|
| [US-WTL-001](./user-stories/US-WTL-001-worktree-con-copia-desde-semilla.md) | El desarrollador crea un worktree con rama con nombre y artefactos copiados desde una semilla quieta | Must | M3 | US-CKP-018 | — | ready |
| [US-WTL-013](./user-stories/US-WTL-013-lanzar-agente-e-instalar-skill.md) | El desarrollador lanza un agente de Claude Code en un worktree nuevo, con la skill de GitRaptor instalada, con una sola orden | Must | M3 | US-WTL-001, US-MCP-020 | — | ready |
| [US-WTL-002](./user-stories/US-WTL-002-descarte-reversible-disco.md) | El desarrollador descarta el worktree de un agente terminado, sabe cuánto disco ganó y lo puede restaurar | Must | M3 | US-CKP-017, US-WTL-003 | — | ready |
| [US-WTL-003](./user-stories/US-WTL-003-estado-real-del-agente.md) | El desarrollador ve qué agentes esperan su decisión, cuáles esperan el CI y cuáles se colgaron | Must | M3 | — | Enmienda de ADR-GRP-012 sin aplicar (estados nuevos) | draft |
| [US-WTL-004](./user-stories/US-WTL-004-worktrees-de-claude-code.md) | Los worktrees que Claude Code crea y borra por su cuenta quedan bajo la red de seguridad de GitRaptor | Must | M3 | US-WTL-001, US-WTL-002 | — | ready |
| [US-WTL-005](./user-stories/US-WTL-005-convivencia-worktrees-ajenos.md) | El desarrollador ve los worktrees de wt, Codex, GitKraken y Orca con su origen sin que GitRaptor los toque | Must | M3 | — | — | ready |
| [US-WTL-006](./user-stories/US-WTL-006-semilla-mantenida.md) | El desarrollador crea worktrees con artefactos al día sin tener que reconstruir la semilla | Should | M4 | US-WTL-001 | Pregunta 6 («momento tranquilo», `sin dato`) | draft |
| [US-WTL-007](./user-stories/US-WTL-007-traer-y-devolver.md) | El desarrollador prueba en su copia el trabajo de un agente y se lo devuelve sin perder nada de ninguno de los dos | Should | M4 | US-WTL-001 | — | ready |
| [US-WTL-008](./user-stories/US-WTL-008-limpieza-por-politica.md) | El disco no se llena de artefactos viejos porque GitRaptor los limpia según la política del repo | Should | M4 | US-WTL-005 | — | ready |
| [US-WTL-009](./user-stories/US-WTL-009-hooks-de-setup-aprobados.md) | El desarrollador prepara cada worktree nuevo con su propio setup sin que un agente pueda ejecutar nada por él | Should | M4 | US-WTL-001 | Pregunta 17 (reaprobar al cambiar el hook) | draft |
| [US-WTL-010](./user-stories/US-WTL-010-integrar-en-local.md) | El desarrollador integra en local el trabajo de un agente y cierra su worktree en un paso que puede deshacer | Could | post-v0.1.0 | US-CKP-014, US-WTL-002 | Estrategias de integración sin decidir | draft |
| [US-WTL-011](./user-stories/US-WTL-011-resumen-ci-pr-por-rama.md) | El desarrollador sabe en qué va el CI y el PR de la rama de cada agente sin salir del cockpit | Could | post-v0.1.0 | US-WTL-003 | Proveedores y datos del PR sin decidir | draft |
| [US-WTL-012](./user-stories/US-WTL-012-puerto-por-worktree.md) | El desarrollador levanta a la vez el servidor de varios agentes sin que choquen sus puertos | Could | post-v0.1.0 | US-WTL-001 | Sin regla de negocio en CTX-WTL-001 | draft |

## Preguntas nuevas surgidas al escribir las historias

Pasadas a [CTX-WTL-001](./context.md) (2026-11-10) como Preguntas 18 a 20, con una propuesta de respuesta cada una; las confirma Rene y no bloquean (todas de M4).

| Historia | Pregunta |
|----------|----------|
| US-WTL-007 | Pregunta 18: ¿Se puede traer o devolver el trabajo de un agente que sigue vivo, o aplica la misma protección que al descartar? |
| US-WTL-009 | Pregunta 19: Si el hook de setup falla, ¿se lanza igual el agente? |
| US-WTL-006 | Pregunta 20: Mientras la semilla se reconstruye, ¿`raptor new` espera o crea el worktree sin copia? |
