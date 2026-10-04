---
id: SEQ-GRD-INSTALACION
title: "Instalación transaccional de la protección de hooks y recuperación"
type: diagram
status: expanded
domain: GRP
feature: guardrails
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-GRD-001, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007]
  stories: [US-GRD-001, US-GRD-002, US-GRD-003]
---

# Secuencia — Instalación transaccional y recuperación (US-GRD-001, US-GRD-002, US-GRD-003)

Muestra el comando reservado, el diario en el perfil, la carpeta escrita con renombrado atómico, el único punto de commit (la clave local) y la verificación por worktree. Al final, cómo recupera el daemon una instalación interrumpida.

```mermaid
sequenceDiagram
  autonumber
  actor DEV as Desarrollador
  participant CLI as raptor guard install
  participant DM as Daemon · módulo guardrails
  participant J as Diario de instalación (perfil)
  participant W as Capa de escritura de Guardrails (crates/git)
  participant R as Repo (.git común)

  DEV->>CLI: proteger "demo"
  CLI->>DM: plan de instalación (qué, dónde, por qué, cómo se revierte, hooks previos)
  DM-->>CLI: plan
  CLI->>DEV: confirmación (UX, no es un control de seguridad)
  DEV-->>CLI: concede
  CLI->>DM: instalar (comando reservado)
  DM->>DM: controles de ADR-GRP-005 § 6: ascendencia, terminal de control, líder de sesión
  DM->>DM: comprobaciones previas: observado, worktrees, encadenado, binario válido
  DM->>J: inicio (valor previo de la clave y su nivel, manifiesto)
  DM->>W: escribir la carpeta en un temporal aleatorio y exclusivo y renombrarla a gitraptor/ (sin seguir enlaces)
  W->>R: gitraptor/hooks/* (solo constantes) + manifest.json (no autoritativo)
  DM->>J: carpeta lista
  DM->>W: escribir core.hooksPath (argv fijo, --file)
  W->>R: config (lock y renombrado de Git): PUNTO DE COMMIT
  DM->>J: clave escrita
  DM->>W: leer el valor efectivo en cada worktree
  alt todos apuntan a gitraptor/hooks
    DM->>J: confirmada + referencia de integridad (hashes, firma o huella del binario, dev/inode)
    DM->>DM: estado → Solo hooks, registro y auditoría
  else alguno no
    DM->>W: restaurar o borrar la clave y después borrar la carpeta
    DM->>J: revertida
  end

  Note over DM,J: Al arrancar el daemon, por cada entrada sin estado final:<br/>clave ajena → borrar carpeta y temporal (idéntico al anterior)<br/>clave propia → verificar → confirmada o revertida
```
