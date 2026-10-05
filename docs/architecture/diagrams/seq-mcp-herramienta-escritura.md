---
id: SEQ-MCP-HERRAMIENTA-ESCRITURA
title: "Herramienta MCP de escritura: ámbito, allowlist, avisos y operación protegida"
type: diagram
status: expanded
domain: MCP
feature: mcp
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-MCP-001, ADR-CKP-002, ADR-TMC-004, ADR-TMC-005, ADR-GRD-003]
  stories: [US-MCP-003, US-MCP-009, US-MCP-011, US-MCP-018, TS-CKP-002, TS-CKP-003]
---

# Secuencia — Herramienta MCP de escritura (`safe_rebase` de una rama ya empujada)

> Covers: BR-14, BR-16 (BR-MCP-WF-001, CALC-001, VAL-005, EDGE-004, EDGE-007, TIME-001, TIME-002) · ADR-MCP-001 § 2 a § 7, ADR-CKP-002 § 2 a § 4 y § 9 · US-MCP-003, US-MCP-009, US-MCP-011, US-MCP-018.

Claude Code lanzó `raptor-mcp` con el cwd en su worktree. El agente pide `safe_rebase`. El daemon comprueba la identidad del par, lee su cwd, resuelve ámbito, allowlist y solicitante, y prepara el plan. El plan trae el aviso de upstream divergente: la primera llamada se rechaza sin efectos y la segunda, que lo reconoce, se ejecuta como operación protegida. La forma exacta de los métodos de preparar y ejecutar la fija TS-CKP-002; aquí se muestran como pasos.

```mermaid
sequenceDiagram
  participant A as Claude Code (agente)
  participant M as raptor-mcp (stdio)
  participant D as Daemon (canal y ejecutor)
  participant P as crates/policy
  participant T as Time Machine
  participant G as git (hijo directo)

  A->>M: tools/call safe_rebase {}
  M->>D: preparar rebase-onto-base (misma conexión)
  D->>D: identidad del par, cwd, identidad otra vez
  D->>D: worktree observado, allowlist, no disponible
  D->>D: solicitante por ascendencia (claude), capa mcp
  D->>D: precondiciones y plan con huella (worktree y marca incluidos)
  D->>P: vista previa (Rebase, capa mcp, actor)
  D-->>M: plan con aviso upstream-diverges
  M-->>A: isError warnings-not-acknowledged [upstream-diverges] y acción
  Note over A,M: Sin efectos y sin apunte en el oplog
  A->>M: tools/call safe_rebase {acknowledge: [upstream-diverges]}
  M->>D: preparar otra vez y ejecutar con los avisos reconocidos
  D->>D: cerrojo del repo, rehacer plan, comparar huella
  D->>P: decisión que cuenta (capa mcp)
  D->>T: intención en el oplog y snapshot previo garantizado
  D->>G: rebase modo atomic, hijo registrado tras la barrera
  G-->>D: termina sin conflicto
  D->>T: registro del resultado
  D-->>M: done, worktree, id de operación
  M-->>A: rebase hecho, aviso de upstream divergente
```

**Variantes**:

- Repo fuera de la allowlist: el daemon responde `repo-not-enabled` antes de leer nada del repo; la herramienta devuelve el rechazo con la acción del desarrollador.
- El rebase choca: el ejecutor lanza `git rebase --abort` dentro de la misma operación (hijo del mismo plan, sin reevaluar) y el resultado es `conflict-reverted` con las rutas.
- La operación pasa de 30 s: la herramienta devuelve `running` con el id; la operación sigue y el agente consulta el resultado con `explain_history`.
- El agente cancela la llamada o cierra la sesión: la operación termina igualmente y queda en el timeline.
