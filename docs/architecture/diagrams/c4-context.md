---
id: C4-GRP-L1
title: "Contexto del motor local"
type: diagram
status: expanded
domain: GRP
feature: motor-local
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-009, ADR-GRP-012]
---

# C4 · Nivel 1 — Contexto del motor local

```mermaid
C4Context
  title Motor local de GitRaptor (F-001-01) — contexto

  Person(dev, "Desarrollador", "Añade repos, corrige atribuciones y consulta el estado")
  System_Ext(claude, "Claude Code", "Agente de IA que trabaja en worktrees")

  System(motor, "Motor local (raptor daemon)", "Observa repos, worktrees, eventos de Git y sesiones; solo escribe su perfil")

  System_Ext(cockpit, "Cockpit / CLI / TUI", "F-001-02, cliente del motor")
  System_Ext(mcp, "raptor-mcp", "F-001-05, cliente del motor para agentes")
  System_Ext(guard, "Guardrails", "F-001-04: políticas y edición de configuración")
  System_Ext(git, "Git del sistema ≥ 2.38", "Lecturas por allowlist (ADR-GRP-009)")
  System_Ext(os, "SO", "Watcher nativo, procesos, launchd / systemd --user / HKCU Run")
  SystemDb_Ext(repos, "Repos observados", "Working trees y directorio Git común: solo lectura")
  SystemDb_Ext(claudedir, "~/.claude/projects", "Transcripts: solo metadatos (PQ-2)")

  Rel(dev, cockpit, "Usa")
  Rel(claude, mcp, "Consulta y se registra", "MCP")
  Rel(claude, repos, "Edita y hace commits")
  Rel(dev, repos, "Edita en su editor")
  Rel(cockpit, motor, "Consultas y stream", "IPC local")
  Rel(mcp, motor, "Consultas y registro", "IPC local")
  Rel(motor, repos, "Lee", "gitoxide")
  Rel(motor, git, "Invoca lecturas", "argv fijo")
  Rel(motor, os, "Eventos de archivos y procesos")
  Rel(motor, claudedir, "Lee metadatos")
  Rel(guard, motor, "Lee el modelo de atribución")
```

- El motor no escribe en los repos, en `~/.claude` ni en la configuración de Git (ADR-GRP-009, ADR-GRP-012). La única escritura fuera del perfil es el registro del autoarranque, que hace el instalador o `raptor daemon enable` (PQ-1, ADR-GRP-005).
