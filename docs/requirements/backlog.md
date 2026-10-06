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
    -   **Historias**: [user-stories.md](features/cockpit/user-stories.md) (24 historias en `features/cockpit/user-stories/`, 2026-10-04, tras aprobar Rene Bonilla el requerimiento (marca del 2026-10-05); 23 listas para Dev Spec y 1 bloqueada: US-CKP-023, la cola de confirmación, por US-GRD-015 y el factor fuera de banda de Q-GRD-19. DAG en olas con esqueletos US-CKP-001 (lectura) y US-CKP-014 (escritura))
    -   **Status**: Priorizada

*   **F-001-03**: Time Machine (BR-08, BR-09, BR-10)
    -   **Contexto**: [context.md](features/time-machine/context.md)
    -   **Historias**: [user-stories.md](features/time-machine/user-stories.md) (21 historias expandidas en `features/time-machine/user-stories/`, 2026-10-03; desde el 2026-10-04, 2 bloqueadas (US-TMC-011 por P17, US-TMC-020 por SPIKE-TMC-001) y 1 fuera del MVP (US-TMC-021, Fase 2), por decisiones de Rene Bonilla)
    -   **Status**: Propuesta

*   **F-001-04**: Guardrails (BR-11, BR-12, BR-13, BR-26)
    -   **Contexto**: [context.md](features/guardrails/context.md) (reglas en [business-rules.md](features/guardrails/business-rules.md), 2026-10-03)
    -   **Historias**: [user-stories.md](features/guardrails/user-stories.md) (19 historias en `features/guardrails/user-stories/`, 2026-10-04 y 2026-10-06; 18 listas y 1 bloqueada, US-GRD-016, por el MCP. US-GRD-018 (política de autoría de los commits) y US-GRD-019 (quién ejecutó frente a a nombre de quién entra) se añadieron el 2026-10-06 por BR-26 y D6 del BRD (decisión de Rene Bonilla); entran en el MVP, fuera de M1 (decisión del orquestador, 2026-10-06, validada por el PO). US-GRD-013 y US-GRD-015 se desbloquearon el 2026-10-04 al aceptarse ADR-GRD-008 (factor del SO de Q-GRD-19); su Dev Spec espera a SPIKE-GRD-002. ADR-GRP-005 a 013 y ADR-GRD-001 a 008 aceptados el 2026-10-04. Pendiente del PO: historia de la relajación personal pendiente (Q-GRD-32) y de la adopción del factor por D8, desinstalar y la excepción (OQ-GRD-008-3))
    -   **Status**: Priorizada

*   **F-001-05**: Servidor MCP (BR-14, BR-15, BR-16)
    -   **Contexto**: [context.md](features/mcp/context.md) (reglas en [business-rules.md](features/mcp/business-rules.md), 2026-10-04; decisiones Q-MCP-1 a Q-MCP-31 tomadas por el orquestador y validadas por PO y Arquitecto. Cubre BR-14, BR-16 y NFR-02; BR-15 parcial por D2 (solo Claude Code). El MCP es cliente del daemon y comparte el catálogo de operaciones del Cockpit (DEP-CKP-7). Depende de 9 huecos anotados como DEP-MCP-1 a DEP-MCP-9 sin aplicar; entre ellos ADR-MCP-001 (contrato del MCP), ADR-CKP-002 (catálogo y ejecutor) y la enmienda de comandos reservados por el "confused deputy" de los hooks. Desbloquea US-GRD-016 cuando exista ADR-MCP-001)
    -   **Historias**: [user-stories.md](features/mcp/user-stories.md) (19 historias expandidas en `features/mcp/user-stories/`, 2026-10-04, en 5 olas según Q-MCP-19; requerimiento aprobado por Rene Bonilla el 2026-10-05. 18 esperan ADR-MCP-001 (bloqueo de arquitectura, DEP-MCP-1; US-MCP-002 solo espera DEP-MCP-3 y DEP-MCP-4); las escrituras esperan además ADR-CKP-002 (propuesto). Bloqueo de producto: US-MCP-014 por la cola de confirmación US-GRD-015. Desbloquean US-GRD-016)
    -   **Status**: Propuesta

---

## Hito M1 — Dogfooding

**Status**: Planificado (2026-10-05) | **Valor**: High
> **Decisión de Rene Bonilla (2026-10-05)**: las próximas tandas se orientan a M1, el mínimo para usar GitRaptor a diario mientras se desarrolla GitRaptor con agentes. Le preocupa también que GitRaptor consuma los recursos del equipo del usuario. El alcance, el criterio de salida y el DAG son **decisión del orquestador (2026-10-05), validada por el PO y el Arquitecto** (ajustes al final). Las NFRs de recursos están en [non-functional.md](../architecture/non-functional.md) § Consumo de recursos y en [ADR-GRP-015](../architecture/decisions/ADR-GRP-015-consumo-recursos.md).

### Objetivo

Rene desarrolla GitRaptor a diario con varias sesiones de Claude Code en paralelo y, con GitRaptor, **ve** la flota en vivo, **deshace** lo que un agente rompa (también con Git crudo) y **no sufre** un force-push de un agente, sin notar que GitRaptor gasta recursos de su máquina.

### Criterio de salida

M1 termina cuando se cumplen los seis puntos, verificados en **macOS** (Linux y Windows: *Pendiente: etapa de validación multiplataforma*):

1. **Uso real**: 10 días laborables seguidos en el repo de GitRaptor, con al menos 3 sesiones de Claude Code en paralelo y el daemon encendido. Los tests siguen en repos temporales (NFR-01); el dogfooding es uso real, no un test.
2. **Detección**: SPIKE-GRP-001 cerrado con su Research Brief: al menos el 90 % de las sesiones detectadas y 0 trabajo humano atribuido a Claude Code.
3. **Cero pérdida de datos**: el gate "repo intacto" (INF-GRP-001) está en verde y se recuperó con `raptor undo` al menos una operación destructiva real de un agente (por ejemplo, un `reset --hard` con trabajo sin commitear).
4. **Protección**: un force-push de un agente a un repo protegido queda bloqueado (US-GRD-001).
5. **Recursos**: los gates RES-01 y RES-02 de INF-GRP-002 están en verde en CI, y `raptor status --resources` (US-GRP-017) muestra a diario la CPU en reposo y la memoria del daemon dentro de su objetivo (CPU < 1 % y RSS < 150 MB con 10 worktrees). ⚠️ **ASSUMPTION**: Rene confirma esas cifras antes de cerrar M1; la cifra exacta del gate la fija la Dev Spec de INF-GRP-002.
6. **Frescura**: la TUI refleja un cambio en menos de 500 ms p95 (NFR-04, gate de INF-CKP-001 e INF-GRP-002).

### Alcance

| Bloque | Item | Prioridad M1 | Estado al 2026-10-05 | Por qué está |
|---|---|---|---|---|
| Ver | US-GRP-001 | Must | In review (Dev Spec) | Base de todo: repo observado |
| Ver | US-GRP-002 | Must | Implementada | Cambios y eventos en vivo |
| Ver | US-GRP-007 | Must | En curso | Sesiones de Claude Code, el único agente del MVP |
| Ver | US-GRP-009 | Must | Expanded | La pide US-GRP-004 (registros que sobreviven al reinicio) |
| Ver | US-GRP-004 | Must | Expanded | Observación continua; la pide la captura continua (US-TMC-004) |
| Ver | TS-GRP-004: N1 a N7 de ADR-CKP-003 § 4 | Must | Pendiente (dueño: worker del canal) | Lo que necesita INF-CKP-001; N8 a N11 son Should |
| Ver | INF-CKP-001 | Must | Dev Spec Pending | Esqueleto de la TUI y gate de 100 ms |
| Ver | US-CKP-001 | Must | Expanded | La flota en vivo en la TUI |
| Ver | SPIKE-GRP-001 | Must | Research Pending | Se valida durante el dogfooding (criterio 2) |
| Deshacer | TS-TMC-004 | Must | Dev Spec In Review | Operación protegida y solicitante |
| Deshacer | US-TMC-001 | Must | En curso | Snapshot previo de las operaciones de GitRaptor |
| Deshacer | US-TMC-002 | Must | Expanded | `raptor undo` |
| Deshacer | US-TMC-004 | Must | Expanded | **Añadida por el PO**: Claude Code usa Git crudo y `operation.run` aún no existe; sin captura continua el undo no protege nada en el dogfooding |
| Proteger | US-GRP-012 | Must | In review (Dev Spec) | Dependencia de US-GRD-001 (rama base `main`) |
| Proteger | INF-GRD-001 | Must | En curso | Arnés de la capa de hooks |
| Proteger | US-GRD-001 | Must | Expanded | Force-push de un agente bloqueado |
| Gates | INF-GRP-001 | Must | Núcleo implementado | Repo intacto: se usa sobre un repo real y propio |
| Gates | INF-GRP-002 | Must | En curso | Frescura y huella (RES-01, RES-02) como gate |
| Recursos | US-GRP-017 | Must | Expanded | Mide el criterio 5 cada día |
| Recursos | TS-GRP-005 | Should | Dev Spec Pending | Prioridad de segundo plano del SO (RES-06, RES-07) |
| Deshacer | US-TMC-005 | Should | Expanded | Previo por los hooks que M1 ya instala; poco coste extra |
| MCP mínimo | US-MCP-001, US-MCP-002, US-MCP-003 | Should | US-MCP-001 en curso; ADR-MCP-001 aceptado | No bloquea la salida: la detección funciona por observación y US-MCP-002 arrastra US-GRP-006 y US-GRD-004. Si no entra, es lo primero de M2 |
| Ver | TS-GRP-004: N8 a N11 | Should | Pendiente | Solo si una historia de M1 los usa |

**Fuera de M1** (siguen en el MVP): US-TMC-022 (tope de disco; sube a M1 si US-GRP-017 mide un almacén de más de 2 GiB), US-GRP-018 (`raptor doctor`), US-GRP-019 (ahorro de energía; el predictor no está en M1), US-GRP-014, US-GRP-015 e INF-GRP-004 (Rene ya tiene Git y compila desde el código), el predictor de conflictos (SPIKE-CKP-001, TS-CKP-001) y la política de autoría de los commits (US-GRD-018 y US-GRD-019, BR-26): el criterio de salida no la necesita y Claude Code ya añade el trailer `Co-Authored-By` por defecto (decisión del orquestador, 2026-10-06, validada por el PO).

### DAG del hito

Las aristas son "requiere terminada". En verde, lo implementado; en gris, los Should, que no bloquean la salida.

```mermaid
flowchart LR
  classDef done fill:#d1fae5,stroke:#047857,color:#064e3b
  classDef should fill:#f3f4f6,stroke:#9ca3af,color:#374151,stroke-dasharray: 4 3
  classDef exit fill:#fef3c7,stroke:#b45309,color:#78350f

  G1["US-GRP-001<br/>estado de worktrees"]
  G2["US-GRP-002<br/>eventos en vivo"]:::done
  G7["US-GRP-007<br/>sesiones Claude Code"]
  G9["US-GRP-009<br/>registro explícito"]
  G4["US-GRP-004<br/>observación continua"]
  G12["US-GRP-012<br/>rama base main"]
  G17["US-GRP-017<br/>status --resources"]
  CH["TS-GRP-004<br/>canal N1 a N7"]
  IG1["INF-GRP-001<br/>repo intacto"]
  IG2["INF-GRP-002<br/>frescura y huella"]
  IC1["INF-CKP-001<br/>esqueleto TUI"]
  C1["US-CKP-001<br/>flota en vivo"]
  T4["TS-TMC-004<br/>operación protegida"]
  M1T["US-TMC-001<br/>snapshot previo"]
  M2T["US-TMC-002<br/>undo"]
  M4T["US-TMC-004<br/>captura continua"]
  ID1["INF-GRD-001<br/>arnés de hooks"]
  D1["US-GRD-001<br/>force-push bloqueado"]
  SP["SPIKE-GRP-001<br/>precisión de detección"]
  TS5["TS-GRP-005<br/>prioridad del SO"]:::should
  M5T["US-TMC-005<br/>previo por hooks"]:::should
  MCP["US-MCP-001 a 003<br/>MCP mínimo"]:::should
  EXIT(["Salida de M1<br/>10 días de dogfooding"]):::exit

  G1 --> G2 --> G7 --> G9 --> G4
  G1 --> G12 --> D1
  G2 --> IG2
  G1 --> G17
  G2 --> G17
  CH --> IC1
  IG2 --> IC1 --> C1
  T4 --> M1T
  G1 --> M1T --> M2T
  M1T --> M4T
  G4 --> M4T
  IG1 --> ID1 --> D1
  IG2 --> TS5
  M4T --> M5T
  D1 --> M5T
  G7 --> MCP
  T4 --> MCP
  C1 --> EXIT
  M2T --> EXIT
  M4T --> EXIT
  D1 --> EXIT
  G17 --> EXIT
  IG1 --> EXIT
  SP --> EXIT
```

**Ruta crítica**: US-GRP-001 → 002 → 007 → 009 → 004 → US-TMC-004 → salida. En paralelo desde hoy: TS-TMC-004 → US-TMC-001 → 002; INF-GRP-001 → INF-GRD-001 → US-GRD-001; INF-GRP-002 y los N1 a N7 del canal → INF-CKP-001 → US-CKP-001.

### Riesgos

1. **¿Alcanza `raptor undo` lo que capturó US-TMC-004?** Si la pila de undo de US-TMC-002 no llega a las operaciones de Git crudo capturadas, el criterio 3 necesita US-TMC-009 (restaurar un punto), que arrastra el timeline (US-TMC-006). Lo confirma la Dev Spec de US-TMC-002 antes de empezarla. Si no llega, US-TMC-009 entra en M1.
2. **N1 a N11 del canal tienen un dueño externo** (TS-GRP-004) y pueden retrasar US-CKP-001.
3. **Sin canal de instalación**, el binario compilado puede desfasarse del daemon en marcha; ADR-GRP-005 § 4 ("versión incompatible") lo detecta.
4. **US-GRD-001 tiene el merge bloqueado** por SPIKE-GRD-001 en Linux y Windows. Para M1 se acepta en macOS. *Pendiente: etapa de validación multiplataforma*.
5. **Cifras de huella sin decidir**: RES-01 y RES-02 son ⚠️ **ASSUMPTION** hasta que Rene las confirme (criterio 5).

### Decisiones registradas

| Decisión | Quién |
|---|---|
| Objetivo, criterio de salida y alcance de M1 | Decisión del orquestador (2026-10-05), validada por el PO |
| Añadir US-GRP-001, US-GRP-009, US-GRP-004, US-GRP-012, US-TMC-004, INF-GRP-001 e INF-GRP-002 a la propuesta inicial | Propuesta del PO, aceptada por el orquestador |
| De los cambios del canal, solo N1 a N7 son Must | Propuesta del PO, aceptada por el orquestador |
| **MCP mínimo como Should, no como Must ni fuera de M1** | El PO propuso sacarlo de M1. El orquestador lo deja como Should porque US-MCP-001 ya está en curso, pero no bloquea la salida |
| US-GRP-017 Must; TS-GRP-005 Should; US-TMC-022, US-GRP-018 y US-GRP-019 fuera de M1 | Decisión del orquestador (2026-10-05), validada por el PO y el Arquitecto |
