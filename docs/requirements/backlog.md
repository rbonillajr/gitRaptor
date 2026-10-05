# Product Backlog

> Índice maestro de GitRaptor. Agrupa las Épicas y, dentro de ellas, las Features. Cada Feature enlaza a su `context.md` en `docs/requirements/features/<feature>/`.
> Fuente del alcance: [BRD-GRP-001](../business/gitraptor-documento-de-negocio.md) § 6.

---

## Epic: E-001 - MVP Fase 1: Cockpit + Time Machine + Guardrails (CLI/TUI + MCP)
**Status**: In Progress | **Value**: High
> Hipótesis: una persona que orquesta varios agentes de IA (en el MVP, Claude Code con soporte completo y el resto como "otro agente"; Q32, BRD v0.5) en paralelo sobre el mismo repo necesita **ver** qué hace cada agente, **deshacer** cualquier operación sin perder datos y **acotar** lo que los agentes pueden hacer con Git. Si el MVP lo resuelve desde la terminal y vía MCP, el usuario adopta GitRaptor en su día a día (ver KPIs en BRD § 9).

### Features

*   **F-001-01**: Motor local (BR-01, BR-02, BR-03)
    -   **Contexto**: [context.md](features/motor-local/context.md)
    -   **Historias**: [user-stories.md](features/motor-local/user-stories.md) (16 historias expandidas en `features/motor-local/user-stories/`, 2026-10-03; ninguna bloqueada desde el 2026-10-04: US-GRP-013 y US-GRP-016 se desbloquearon al aceptarse ADR-GRP-007, que cierra P8)
    -   **Status**: Priorizada

*   **F-001-02**: Cockpit (BR-04, BR-05, BR-06, BR-07)
    -   **Contexto**: [context.md](features/cockpit/context.md) (reglas en [business-rules.md](features/cockpit/business-rules.md), 2026-10-04; decisiones Q-CKP-1 a Q-CKP-30 tomadas por el orquestador y validadas por PO y Arquitecto. Depende de 14 huecos del motor y de otras features, anotados como DEP-CKP-1 a DEP-CKP-14 sin aplicar; entre ellos SPIKE-CKP-001/ADR-CKP-001 (predicción de conflictos) y ADR-CKP-002 (catálogo y ejecutor de operaciones))
    -   **Historias**: Pendiente (se escriben tras aprobar el requerimiento)
    -   **Status**: Propuesta

*   **F-001-03**: Time Machine (BR-08, BR-09, BR-10)
    -   **Contexto**: [context.md](features/time-machine/context.md)
    -   **Historias**: [user-stories.md](features/time-machine/user-stories.md) (21 historias expandidas en `features/time-machine/user-stories/`, 2026-10-03; desde el 2026-10-04, 2 bloqueadas (US-TMC-011 por P17, US-TMC-020 por SPIKE-TMC-001) y 1 fuera del MVP (US-TMC-021, Fase 2), por decisiones de Rene Bonilla)
    -   **Status**: Propuesta

*   **F-001-04**: Guardrails (BR-11, BR-12, BR-13)
    -   **Contexto**: [context.md](features/guardrails/context.md) (reglas en [business-rules.md](features/guardrails/business-rules.md), 2026-10-03)
    -   **Historias**: [user-stories.md](features/guardrails/user-stories.md) (17 historias en `features/guardrails/user-stories/`, 2026-10-04; 16 listas y 1 bloqueada, US-GRD-016, por el MCP. US-GRD-013 y US-GRD-015 se desbloquearon el 2026-10-04 al aceptarse ADR-GRD-008 (factor del SO de Q-GRD-19); su Dev Spec espera a SPIKE-GRD-002. ADR-GRP-005 a 013 y ADR-GRD-001 a 008 aceptados el 2026-10-04. Pendiente del PO: historia de la relajación personal pendiente (Q-GRD-32) y de la adopción del factor por D8, desinstalar y la excepción (OQ-GRD-008-3))
    -   **Status**: Priorizada

*   **F-001-05**: Servidor MCP (BR-14, BR-15, BR-16)
    -   **Contexto**: [context.md](features/mcp/context.md) (reglas en [business-rules.md](features/mcp/business-rules.md), 2026-10-04; decisiones Q-MCP-1 a Q-MCP-31 tomadas por el orquestador y validadas por PO y Arquitecto. Cubre BR-14, BR-16 y NFR-02; BR-15 parcial por D2 (solo Claude Code). El MCP es cliente del daemon y comparte el catálogo de operaciones del Cockpit (DEP-CKP-7). Depende de 9 huecos anotados como DEP-MCP-1 a DEP-MCP-9 sin aplicar; entre ellos ADR-MCP-001 (contrato del MCP), ADR-CKP-002 (catálogo y ejecutor) y la enmienda de comandos reservados por el "confused deputy" de los hooks. Desbloquea US-GRD-016 cuando exista ADR-MCP-001)
    -   **Historias**: [user-stories.md](features/mcp/user-stories.md) (19 historias expandidas en `features/mcp/user-stories/`, 2026-10-04, en 5 olas según Q-MCP-19; requerimiento aprobado por Rene Bonilla el 2026-10-05. 18 esperan ADR-MCP-001 (bloqueo de arquitectura, DEP-MCP-1; US-MCP-002 solo espera DEP-MCP-3 y DEP-MCP-4); las escrituras esperan además ADR-CKP-002 (propuesto). Bloqueo de producto: US-MCP-014 por la cola de confirmación US-GRD-015. Desbloquean US-GRD-016)
    -   **Status**: Propuesta
