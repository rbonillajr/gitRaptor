---
mode: bulk
generated: 2026-10-03T00:00Z
updated: 2026-10-03
generator: product-owner
total_artifacts: 21
expanded: 21
approved: 0
blocked:
  - US-TMC-005
  - US-TMC-011
  - US-TMC-017
  - US-TMC-020
  - US-TMC-021
---

# User Stories — INDEX: Time Machine

> Índice de historias de F-001-03. Cada historia vive en `user-stories/US-TMC-NNN-<slug>.md` con frontmatter real, modelo plano y un Gherkin declarativo verificable en repos temporales. Reglas en [business-rules.md](./business-rules.md) (BR-TMC-001); contexto en [context.md](./context.md) (CTX-TMC-001).

---

## Contexto del Feature

**Feature**: Time Machine (F-001-03)
**Epic**: E-001 — MVP Fase 1
**Prioridad**: Alta
**Estado**: En Análisis

**Enlace a contexto completo**: [`context.md`](./context.md)
**Reglas de negocio**: [`business-rules.md`](./business-rules.md) · **Diseño**: Pendiente de diseño

---

## Análisis de División (Sistema de 4 Pasos)

- **Paso 1 — Flujo**: actor principal = desarrollador orquestador; resultado = recuperar cualquier trabajo sin perder el de otros; camino mínimo = snapshot previo (001) → undo (002).
- **Paso 2 — Puntos de resultado**: proteger (001, 004, 005, 018), deshacer (002, 003, 010, 011), restaurar (009), entender (006-008), gobernar (012-015, 021), sostener (016, 017, 019, 020).
- **Paso 3 — Patrón aplicado**: camino feliz primero y capacidad acumulable; el undo por agente se apoya en solape (012) y permisos (013).
- **Paso 4 — Filtro**: todas las HU tienen un resultado observable y verificable desde fuera, con 6 escenarios o menos.

---

## Índice de Historias

> Columna **Status** = estado de expansión del índice (`draft → expanded`), como en motor-local; el `status` del frontmatter de cada archivo es su ciclo de vida (`draft` hasta que se refine), también como en motor-local.
>
> Columna **Depende de** = aristas del DAG de ejecución (requiere terminada). Las dependencias de motor-local (US-GRP-*) y de otras features (F-001-0x) son externas a esta feature. **Bloqueo** = no se puede empezar hasta que se cierre lo indicado.

| ID | Título | Descripción (1 línea) | Depende de | Bloqueo | Status |
|----|--------|-----------------------|------------|---------|--------|
| [US-TMC-001](./user-stories/US-TMC-001-snapshot-previo-operaciones-gitraptor.md) | El desarrollador recupera su trabajo sin commitear tras cualquier operación lanzada por GitRaptor | Desarrollador quiere un punto previo garantizado y que la operación no se ejecute si falla | US-GRP-001 | — | expanded |
| [US-TMC-002](./user-stories/US-TMC-002-undo-ultima-operacion.md) | El desarrollador deshace con un comando la última operación de su worktree | Desarrollador quiere `raptor undo` en el worktree actual, protegido por su propio punto previo | 001 | — | expanded |
| [US-TMC-003](./user-stories/US-TMC-003-redo.md) | El desarrollador rehace lo que deshizo por error | Desarrollador quiere `raptor redo` del último undo, deteniéndose ante un solape | 002, 012, 013 | — | expanded |
| [US-TMC-004](./user-stories/US-TMC-004-captura-continua-git-crudo.md) | El trabajo hecho fuera de GitRaptor queda capturado como punto recuperable | Desarrollador quiere captura continua del Git crudo y del editor, sin ignorados | 001, US-GRP-002, US-GRP-004 | — | expanded |
| [US-TMC-005](./user-stories/US-TMC-005-snapshot-previo-hooks-guardrails.md) | Las operaciones de Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails | Desarrollador quiere snapshot previo vía hooks de Guardrails sin depender de ellos | 004 | F-001-04 (hooks) | expanded |
| [US-TMC-006](./user-stories/US-TMC-006-timeline-que-cuando-quien.md) | El desarrollador sabe qué cambió en su repo, cuándo y quién lo hizo | Desarrollador quiere el timeline con actor, origen, "Tú u otro (sin atribuir)" y nivel de cobertura | 001, 004, US-GRP-002, US-GRP-007, US-GRP-009 | — | expanded |
| [US-TMC-007](./user-stories/US-TMC-007-timeline-filtros-huecos.md) | El desarrollador filtra el timeline por worktree, agente o periodo y ve lo que no se observó | Desarrollador quiere filtros y huecos explícitos en el timeline | 006, US-GRP-005 | — | expanded |
| [US-TMC-008](./user-stories/US-TMC-008-timeline-atribucion-vigente.md) | El timeline refleja las correcciones de atribución sin reescribir quién deshizo qué | Desarrollador quiere la atribución vigente y el registro de cada undo intacto | 002, 006, US-GRP-010 | — | expanded |
| [US-TMC-009](./user-stories/US-TMC-009-restaurar-punto-timeline.md) | El desarrollador devuelve su worktree a cualquier punto del timeline | Desarrollador quiere restaurar un punto con el alcance de D-TMC-20, deshaciendo la restauración si quiere | 001, 002, 006, 013 | — | expanded |
| [US-TMC-010](./user-stories/US-TMC-010-undo-since.md) | El desarrollador deshace todo lo ocurrido en su worktree en los últimos minutos | Desarrollador quiere `raptor undo --since` en el worktree actual | 002, 012, 013 | — | expanded |
| [US-TMC-011](./user-stories/US-TMC-011-undo-por-agente.md) | El desarrollador deshace solo lo que hizo un agente en un periodo | Desarrollador quiere `raptor undo --agent --since` con atribución vigente, sin huecos ni trabajo ajeno | 002, 007, 012, 013, US-GRP-007, US-GRP-009, US-GRP-010 | P17 de motor-local (D-TMC-22) | expanded |
| [US-TMC-012](./user-stories/US-TMC-012-solape-otro-actor.md) | Un undo nunca sobrescribe trabajo posterior de otro actor | Desarrollador quiere que el undo se detenga y muestre el solape | 002, 006 | — | expanded |
| [US-TMC-013](./user-stories/US-TMC-013-permisos-solicitante.md) | Un agente no puede deshacer trabajo ajeno, aunque lance la CLI desde su propia shell | Desarrollador quiere solicitante atribuido y confirmación interactiva para tocar trabajo ajeno | 002, US-GRP-007, US-GRP-009 | — (F-001-05 para el canal MCP) | expanded |
| [US-TMC-014](./user-stories/US-TMC-014-undo-ya-empujado.md) | El desarrollador sabe cuándo lo que deshizo sigue en el remoto | Desarrollador quiere undo solo local con aviso si ya se empujó | 002 | — | expanded |
| [US-TMC-015](./user-stories/US-TMC-015-operacion-git-en-curso.md) | El desarrollador no rompe un rebase o un merge a medias al deshacer o restaurar | Desarrollador quiere que undo y restauración se detengan con una operación de Git en curso | 002, 009, US-GRP-003 | — | expanded |
| [US-TMC-016](./user-stories/US-TMC-016-retencion-por-defecto.md) | Los snapshots no llenan el disco y nunca se pierde el último punto antes de una operación destructiva | Desarrollador quiere purga a 30 días con aviso y protección del último punto destructivo | 001 | — | expanded |
| [US-TMC-017](./user-stories/US-TMC-017-retencion-configurable.md) | El desarrollador ajusta para sí cuánto tiempo se conservan los snapshots | Desarrollador quiere retención en perfil y local personal, nunca en el nivel de equipo | 016 | ADR de formato de configuración (P8 motor-local), F-001-04 | expanded |
| [US-TMC-018](./user-stories/US-TMC-018-garantias-snapshots.md) | Los snapshots no se publican, no se pierden con el mantenimiento de Git y ningún agente los altera | Desarrollador quiere las tres garantías de D-TMC-11 verificadas | 001 | — | expanded |
| [US-TMC-019](./user-stories/US-TMC-019-robustez-interrupcion.md) | El repo sigue recuperable aunque GitRaptor muera a mitad de un snapshot o de un undo | Desarrollador quiere resistir pruebas de caos (NFR-12) | 001, 002, 009 | — | expanded |
| [US-TMC-020](./user-stories/US-TMC-020-overhead-snapshot.md) | El desarrollador y sus agentes no notan el coste de los snapshots | Desarrollador quiere overhead < 200 ms por snapshot (NFR-04) | 001, 004 | Spike (a): repo mediano (D-TMC-21) | expanded |
| [US-TMC-021](./user-stories/US-TMC-021-politica-guardrails-undo.md) | Las políticas del repo pueden restringir quién deshace, nunca ampliarlo | Desarrollador quiere que Guardrails endurezca los permisos del undo sin poder relajarlos | 013 | F-001-04 (políticas) | expanded |

### Orden de ejecución sugerido (capas del DAG, solo dependencias internas)

1. **Capa 0**: 001
2. **Capa 1**: 002, 004, 016, 018
3. **Capa 2**: 006, 013, 014, 005 (bloqueada), 017 (bloqueada), 020 (bloqueada)
4. **Capa 3**: 007, 008, 009, 012, 021 (bloqueada)
5. **Capa 4**: 003, 010, 015, 019, 011 (bloqueada por P17)

---

## Cobertura de reglas y decisiones

| Regla / decisión | Historias |
|------------------|-----------|
| BR-TMC-CONS-001 | 001, 002, 009 |
| BR-TMC-CONS-002 | 001, 004 |
| BR-TMC-CONS-003 | 004, 005, 006 |
| BR-TMC-CONS-004 | 018 |
| BR-TMC-CONS-005 | 003, 006, 008, 012 |
| BR-TMC-WF-001 | 002, 003, 010 |
| BR-TMC-WF-002 | 011 |
| BR-TMC-WF-003 | 009 |
| BR-TMC-VAL-001 | 002, 003, 010, 011 |
| BR-TMC-AUTH-001 | 003, 009, 010, 011, 013, 021 |
| BR-TMC-TIME-001 | 016, 017 |
| BR-TMC-EDGE-001 | 014 |
| BR-TMC-EDGE-002 | 007, 009, 011 |
| BR-TMC-EDGE-003 | 019 |
| BR-TMC-EDGE-004 | 015 |
| D-TMC-1 a D-TMC-23 | 1: 006 · 2: 008, 011 · 3: 001, 018 · 4: 005 · 5: 017 · 6: 013 · 7: 007 · 8: 014 · 9: 001, 004, 005 · 10: 001, 004, 005 · 11: 018 · 12: 006 · 13: 003, 012 · 14: 014 · 15: 016, 017 · 16: 001, 004 · 17: 013, 021 · 18: 008 · 19: 002, 006, 007, 010 · 20: 009 · 21: 020 · 22: 011 · 23: 002, 003, 009, 010, 011, 013 |

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-03 | PO (AADD) | Versión inicial en modo bulk: 20 historias expandidas; 4 bloqueadas (005, 011, 017, 020). |
| 1.1 | 2026-10-03 | PO (AADD) | RESERVAS del Artifact Judge: solicitante y confirmación interactiva en 003, 009, 010 y 011 (AUTH-001, D-TMC-23); escenario de Guardrails de 013 extraído a US-TMC-021 (bloqueada por F-001-04); cobertura alineada con `covers`; escenarios de validación o borde en 005, 012, 014, 016 y 019. |
| 1.2 | 2026-10-03 | PO (AADD) | Segunda pasada del judge: 013 como dependencia de 003 y 009; escenario 6 de 009 retitulado; 010 separa confirmación y rechazo; el solicitante de undo, redo y restauración figura como "solicitante sin atribuir", nunca como el desarrollador (Q34). |
