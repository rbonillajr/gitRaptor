# C4 L3 — Componentes de la Time Machine dentro del daemon

> Componentes de la Time Machine (F-001-03) en el proceso `raptor daemon`, junto al motor de solo lectura y al ejecutor de operaciones de usuario, y los almacenes del perfil. Cubre ADR-TMC-001..005 y ADR-TMC-007. El motor, el canal y el perfil son de motor-local (ADR-GRP-005, 006, 009, 010, 013); el ejecutor es de F-001-02 y F-001-05.

```mermaid
flowchart LR
  subgraph Clientes["Clientes (procesos no confiables)"]
    CLI["raptor (CLI/TUI)"]
    MCP["raptor-mcp (lo lanza el agente)"]
    HOOK["Hooks de Guardrails (F-001-04, opcionales)"]
  end

  subgraph Daemon["raptor daemon (un proceso por usuario)"]
    CANAL["Canal JSON-RPC (crates/api)<br/>identifica al proceso llamante"]
    subgraph Motor["Motor (solo lectura, ADR-GRP-009)"]
      OBS["Observador y reconciliación"]
      ATR["Sesiones y atribución vigente"]
    end
    subgraph TM["Time Machine (crates/core::timemachine)"]
      GUARD["Operación protegida<br/>(intención, snapshot previo, registro)"]
      SOL["Solicitante, permisos<br/>y confirmación"]
      PLAN["Planificador:<br/>destino, solape, ya empujado"]
      CAP["Captura y snapshot<br/>(niveles a, b, hook)"]
      APL["Aplicador:<br/>locks, refs, archivos, índice"]
      OPL["Oplog y diario"]
      REC["Recuperación al arrancar"]
      PURGA["Retención y purga"]
    end
    EXEC["Ejecutor de operaciones de usuario<br/>(F-001-02 / F-001-05)<br/>respeta config y hooks (NFR-07)"]
    GREAD["crates/git: capa de lectura"]
    GWRITE["crates/git: capa de escritura<br/>de la Time Machine<br/>(sin hooks ni filtros, SEC-TMC-02)"]
    POL["crates/policy:<br/>configuración y políticas"]
  end

  subgraph Perfil["Perfil del usuario (0700)"]
    MDB[("Almacén del motor<br/>SQLite por repo")]
    STORE[("Almacén de snapshots<br/>repo Git bare por repo")]
    ODB[("Oplog SQLite por repo")]
  end

  REPO[("Repo del usuario<br/>worktrees y .git")]

  CLI --> CANAL
  MCP --> CANAL
  HOOK -->|"snapshot previo vía hook"| CANAL
  CANAL --> GUARD
  CANAL --> SOL
  OBS -->|"eventos publicados"| CAP
  ATR -->|"atribución vigente"| SOL
  ATR --> PLAN
  GUARD --> CAP
  GUARD --> SOL
  GUARD -->|"solo tras snapshot válido"| EXEC
  SOL --> POL
  SOL --> PLAN
  PLAN --> APL
  CAP --> GREAD
  CAP --> GWRITE
  APL --> GWRITE
  PURGA --> GWRITE
  PURGA --> POL
  GREAD -->|"solo lectura"| REPO
  GWRITE -->|"objetos y refs del almacén"| STORE
  GWRITE -->|"solo undo, redo y restauración"| REPO
  EXEC -->|"merge, rebase, commit, descartar<br/>(Git CLI, argv fijo)"| REPO
  OBS --> MDB
  OPL --> ODB
  GUARD --> OPL
  APL --> OPL
  REC --> OPL
  REC --> STORE
```

**Lectura del diagrama**: la Time Machine solo escribe en el repo con su capa de escritura y por el aplicador (undo, redo, restauración; BR-TMC-CONS-004). Las operaciones de usuario del Cockpit y del MCP las ejecuta el ejecutor del daemon, fuera de esa capa y con los hooks del usuario, después de que la operación protegida tenga un snapshot válido. Capturar y purgar escriben solo en el almacén del perfil. El motor no tiene ninguna flecha hacia una capa de escritura.
