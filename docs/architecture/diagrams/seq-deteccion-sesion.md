---
id: SEQ-GRP-DETECCION
title: "Detección de una sesión de Claude Code"
type: diagram
status: expanded
domain: GRP
feature: motor-local
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-012, ADR-GRP-013, ADR-GRP-007]
  stories: [US-GRP-007, US-GRP-008]
---

# Secuencia — Detección de una sesión de Claude Code (US-GRP-007, US-GRP-008)

```mermaid
sequenceDiagram
  autonumber
  participant CC as Claude Code
  participant OS as SO (procesos y archivos)
  participant D as raptor daemon
  participant T as ~/.claude/projects (metadatos)
  participant P as Perfil

  D->>OS: Escaneo periódico de procesos (S1)
  OS-->>D: Proceso de Claude Code con cwd en un worktree observado
  D->>P: Sesión nueva (pid, hora de inicio), origen "detectado", activa
  CC->>OS: Escribe un archivo del worktree
  OS->>D: Evento de archivo
  D->>T: Lee herramienta, ruta, hora, id de sesión y cwd (S2b)
  alt Evidencia positiva de esa sesión (S2b, S3 o S4)
    D->>P: Evento apuntando a la sesión
  else Solo co-ubicación, o dos sesiones sin desempate
    D->>P: Evento "sin atribuir"
  end
  Note over D: Sin actividad durante el umbral (5 min por defecto) → inactiva
  CC-->>OS: El proceso termina
  OS-->>D: (pid, hora de inicio) ya no existe
  D->>P: Sesión terminada; no se reactiva (Q41)
```

- S2a (mtime) solo correlaciona proceso y transcript; nunca atribuye. Si el formato del transcript no se reconoce, S2b se desactiva y lo que habría atribuido queda "sin atribuir" (ADR-GRP-012).
