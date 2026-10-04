---
id: C4-GRP-L2
title: "Contenedores del motor local"
type: diagram
status: expanded
domain: GRP
feature: motor-local
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-008, ADR-GRP-009, ADR-GRP-010, ADR-GRP-013]
---

# C4 · Nivel 2 — Contenedores del motor local

```mermaid
C4Container
  title Motor local — contenedores (ADR-GRP-002, ADR-GRP-005)

  Person(dev, "Desarrollador")
  System_Ext(claude, "Claude Code")

  System_Boundary(bin, "Binario raptor (apps/cli)") {
    Container(daemon, "raptor daemon", "Rust: crates/core + crates/git + crates/policy", "Proceso único por usuario: observador, detección, modelo de eventos, único escritor del perfil")
    Container(cli, "raptor (CLI/TUI)", "Rust: clap + ratatui", "Cliente; comandos reservados con confirmación en terminal")
  }
  Container(mcp, "raptor-mcp (apps/mcp)", "Rust: rmcp", "Cliente; sin comandos reservados")

  System_Boundary(perfil, "Perfil (carpetas estándar por SO, ADR-GRP-006)") {
    ContainerDb(store, "Almacén", "SQLite por repo + índice global", "Datos")
    ContainerDb(conf, "Configuración", "settings.json + repos/<id>/settings.local.json", "Solo lectura para el motor")
    Container(state, "Estado y ejecución", "Bloqueo, logs, socket", "")
  }

  SystemDb_Ext(repo, "Repo observado", "Working trees, .git y .gitraptor/settings.json del worktree principal")
  System_Ext(git, "Git ≥ 2.38")

  Rel(dev, cli, "Usa")
  Rel(claude, mcp, "MCP")
  Rel(cli, daemon, "JSON-RPC", "socket Unix / named pipe")
  Rel(mcp, daemon, "JSON-RPC", "socket Unix / named pipe")
  Rel(daemon, store, "Lee y escribe")
  Rel(daemon, conf, "Lee y vigila")
  Rel(daemon, state, "Bloqueo, logs, escucha")
  Rel(daemon, repo, "Lee y vigila", "gitoxide + notify")
  Rel(daemon, git, "Allowlist de lecturas", "argv fijo")
```

- `crates/api` define el contrato que comparten el daemon y los dos clientes. Los clientes nunca abren el perfil.
