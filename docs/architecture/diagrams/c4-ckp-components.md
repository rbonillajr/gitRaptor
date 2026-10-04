# C4 L3 — Componentes del Cockpit: TUI y CLI de solo lectura, y su relación con el daemon

> Componentes de la TUI `raptor` y de la CLI de solo lectura (`raptor status`, `raptor conflicts`) en `apps/cli`, y las piezas del Cockpit (F-001-02) dentro del proceso `raptor daemon`. Cubre ADR-CKP-003 (TUI), ADR-CKP-001 (publicador de la predicción) y ADR-CKP-002 (ejecutor de operaciones). El canal y el motor son de motor-local (ADR-GRP-005, 011, 013); la operación protegida es de la Time Machine (ADR-TMC-004); la decisión es de Guardrails (ADR-GRD-003).

```mermaid
flowchart LR
  USER(["Desarrollador<br/>(terminal)"])
  EDITOR["Editor del usuario<br/>(argv fijo, sin shell)"]

  subgraph CLIP["Proceso raptor: TUI y CLI (apps/cli, cliente no confiable)"]
    subgraph TUI["tui"]
      TERM["term<br/>init/restore, suspensión"]
      INPUT["Entrada y keymap<br/>(tabla única acción ↔ teclas)"]
      LOOP["app: bucle<br/>colas de entrada y del motor,<br/>drenado y render coalescido"]
      UPD["update<br/>Msg → Model + Cmd (puro)"]
      FX["effects<br/>ejecuta Cmd fuera de update"]
      VIEW["view y widgets<br/>layout 80×24 por prioridad,<br/>AgentList, GraphLanes, ConflictAlert,<br/>PolicyBanner, ConfirmPrompt, toast, ayuda"]
      PLAIN["plain<br/>renderer --plain"]
      EDL["editor<br/>lanzador autorizado (DEP-CKP-12)"]
      MET["metrics<br/>t_client_recv, t_render,<br/>histograma local"]
    end
    STORE[("Model (store de vista)<br/>réplica del motor + estado de UI<br/>+ conexión + reloj de vista")]
    subgraph PRES["present"]
      ING["ingest<br/>contrato → modelo de vista"]
      SAN["sanitize<br/>único punto SEC-12 → SafeText"]
      I18N["i18n<br/>catálogo tipado en/es"]
      JSON["json<br/>esquema propio versionado,<br/>controles escapados"]
    end
    CLIENT["client<br/>conexión, estados, secuencia N/N+1,<br/>resync, reconexión, cambio de repo"]
    RO["cli: status, conflicts<br/>(solo lectura)"]
    DMOD["daemon<br/>único módulo que importa crates/core"]
  end

  THEME["crates/theme<br/>tokens: color en 3 profundidades,<br/>símbolos con fallback ASCII y anchura"]
  APILIB["crates/api<br/>contrato JSON-RPC, biblioteca cliente,<br/>arranque bajo demanda, reloj monótono,<br/>validación del editor"]

  subgraph DAEMON["raptor daemon (un proceso por usuario)"]
    CANAL["Canal (crates/api)<br/>handshake, instantánea + stream,<br/>consultas, peticiones; solicitante"]
    MOTOR["Motor (crates/core)<br/>estado, sesiones, atribución,<br/>secuencia por repo"]
    PRED["Publicador de la predicción<br/>solape y conflicto previsto<br/>(ADR-CKP-001, sin escribir en el repo)"]
    EXEC["Ejecutor de operaciones<br/>catálogo, serializa por repo, revalida<br/>(ADR-CKP-002)"]
    GUARD["Operación protegida<br/>(Time Machine, ADR-TMC-004)"]
    POL["Decisión de Guardrails<br/>(crates/policy, ADR-GRD-003)"]
    PREFS["Preferencias de la TUI<br/>(escritas solo por el daemon, DEP-CKP-11)"]
  end

  PERFIL[("Perfil del usuario")]
  REPO[("Repo del usuario")]

  USER -->|"teclas"| INPUT
  INPUT -->|"Msg"| LOOP
  CLIENT -->|"Msg del motor"| LOOP
  LOOP --> UPD
  UPD --> STORE
  UPD -->|"Cmd"| FX
  LOOP -->|"draw coalescido"| VIEW
  LOOP --> PLAIN
  VIEW -->|"lee"| STORE
  VIEW --> THEME
  VIEW --> I18N
  VIEW --> TERM
  TERM -->|"pinta"| USER
  LOOP --> MET
  FX -->|"consultas, operaciones,<br/>preferencias, resolver editor"| CLIENT
  FX -->|"suspende la TUI"| TERM
  FX --> EDL
  EDL -->|"lanza"| EDITOR
  CLIENT -->|"DTO del contrato"| ING
  ING --> SAN
  ING -->|"solo SafeText e ids"| STORE
  RO --> CLIENT
  RO --> ING
  RO --> JSON
  RO --> I18N
  CLIENT --> APILIB
  APILIB -->|"socket Unix / named pipe"| CANAL
  APILIB -.->|"arranque bajo demanda<br/>(gestor de servicios o entorno limpio)"| DAEMON
  DMOD -.->|"punto de entrada del proceso"| MOTOR

  CANAL --> MOTOR
  CANAL --> PRED
  CANAL --> EXEC
  CANAL --> PREFS
  MOTOR -->|"eventos publicados"| PRED
  EXEC -->|"solo tras decidir"| POL
  EXEC -->|"intención, snapshot previo, registro"| GUARD
  EXEC -->|"merge, rebase, worktree add/remove<br/>(Git CLI, argv fijo, nunca push)"| REPO
  MOTOR -->|"solo lectura"| REPO
  PRED -->|"solo lectura"| REPO
  MOTOR --> PERFIL
  PREFS --> PERFIL
```

**Lectura del diagrama**: el proceso de la TUI no tiene ninguna flecha hacia el repo ni hacia el perfil. Todo lo que pinta llega por `client`, pasa por `ingest` y su saneador, y queda en el `Model`. `view` solo lee el `Model`, el tema y el catálogo. El único proceso que lanza la TUI es el editor, desde su módulo autorizado. El daemon lo arranca la biblioteca de `crates/api`, nunca la TUI incrustando el motor: el módulo `daemon` de `apps/cli` solo es el punto de entrada de `raptor daemon` en su propio proceso. Las escrituras salen como peticiones al ejecutor del daemon, que pide la decisión a Guardrails y ejecuta como operación protegida. La predicción la calcula y la publica el daemon para todos los clientes, sin escribir en el repo. La CLI de solo lectura reutiliza `client`, `ingest` e `i18n`, y solo tiene una salida propia: `json`.
