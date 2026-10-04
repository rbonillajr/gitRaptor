# Secuencia — Operación de GitRaptor con snapshot previo garantizado

> Covers: US-TMC-001, US-TMC-020 · ADR-TMC-004 § 1, ADR-TMC-001, ADR-TMC-003 § 3. Una superficie de GitRaptor (CLI, TUI o MCP) pide una operación que modifica el repo; sin snapshot previo válido no se ejecuta.

```mermaid
sequenceDiagram
  autonumber
  participant C as Cliente (CLI, TUI o MCP)
  participant G as Operación protegida (daemon)
  participant S as Solicitante (daemon)
  participant O as Oplog
  participant K as Captura y almacén
  participant E as Ejecutor de operaciones (daemon, F-001-02/05)
  participant R as Repo del usuario

  C->>G: operación(tipo, ámbito)
  G->>S: resolver solicitante por ascendencia del proceso
  S-->>G: agente X o "sin atribuir"
  G->>O: intención (solicitante congelado, ámbito)
  G->>K: snapshot previo del ámbito
  alt nada cambió desde la última captura
    K-->>G: reutiliza el árbol (camino rápido)
  else hay cambios
    K->>R: lectura en bruto de rutas cambiadas, índice y refs (solo lectura)
    K->>K: blobs, árboles con índice temporal, commit del almacén
  end
  alt snapshot completo
    K->>K: crea la ref del almacén
    K->>O: snapshot completo (nivel previo_garantizado)
    G->>E: ejecutar la operación de usuario
    E->>R: Git CLI con argv fijo, config y hooks del usuario (NFR-07)
    E-->>G: resultado
    G->>O: operación terminada
    G-->>C: resultado y avisos
  else falla (disco lleno, tiempo máximo, almacén no disponible)
    K->>O: snapshot descartado
    G->>O: operación abortada (motivo)
    G-->>C: no se ejecutó + motivo
    Note over R: el repo no cambia
  end
```
