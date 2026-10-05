---
id: US-MCP-010
title: "Un agente commitea solo los archivos que nombra y nunca fuera de su worktree"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-CKP-002
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-009
    - US-MCP-011
    - US-GRP-011
ado:
  id: null
  url: null
covers: [BR-MCP-VAL-001, BR-MCP-EDGE-010, BR-MCP-EDGE-003]
blocked_by: [ADR-MCP-001, ADR-CKP-002]
tags: [mcp, safe-commit, rutas, path-traversal, worktree-compartido, ola-3]
---

# US-MCP-010: Un agente commitea solo los archivos que nombra y nunca fuera de su worktree

## Descripción

**Como** desarrollador orquestador, **quiero** que `safe_commit` valide cada ruta como literal dentro del worktree, no añada archivos ignorados y, en un worktree compartido, exija rutas explícitas, **para** que un agente no commitee secretos, archivos de otro repo ni el trabajo de otro agente.

**Valor**: cierra las vulnerabilidades conocidas de los servidores MCP de Git (path traversal, inyección de argumentos; BR-16).

## Reglas cubiertas

BR-MCP-VAL-001 (rutas relativas, literales, con tope) · BR-MCP-EDGE-010 (sin seguimiento solo si se nombran; ignorados nunca) · BR-MCP-EDGE-003 (parte commit: en un worktree compartido, solo rutas explícitas) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-009, US-MCP-011 (va antes: las dos tocan los parámetros de `safe_commit`, D-11). De motor-local: US-GRP-011 (sesiones de un worktree compartido).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe) y ADR-CKP-002 (**propuesto**; la validación de pathspec literal llega vía ADR-CKP-002, DEP-MCP-2): bloqueos de arquitectura. Tope de rutas por llamada: S-MCP-1.
- **Transversal**: el corpus de seguridad (traversal, UNC, inyección de argumentos) verifica esta historia; el comportamiento con rutas UNC y enlaces en Windows y Linux queda para la etapa de validación multiplataforma.

## Criterios de Aceptación

**Escenario: Una ruta fuera del worktree rechaza toda la llamada**

Dado "claude-1" atribuido en "shop-feat-a" con "src/app.rs" modificado
Cuando pide `safe_commit` de "src/app.rs" y "../otro-repo/secreto"
Entonces la llamada entera se rechaza con el motivo "ruta fuera del worktree" y la ruta marcada como dato no confiable
  Y no se crea ningún commit

**Esquema del escenario: Las rutas no válidas no llegan a Git**

Dado "claude-1" atribuido en "shop-feat-a"
Cuando pide `safe_commit` con la ruta "<ruta>"
Entonces la llamada se rechaza con el motivo "<motivo>"
  Y no se crea ningún commit

Ejemplos:
| ruta | motivo |
| /etc/passwd | ruta no válida |
| \\servidor\compartido\a.rs | ruta no válida |
| una ruta con caracteres de control | ruta no válida |
| un enlace que apunta fuera del worktree | ruta fuera del worktree |
| más rutas que el tope por llamada | demasiadas rutas |

**Escenario: Una ruta con forma de patrón se trata como nombre literal**

Dado "shop-feat-a" sin ningún archivo llamado ":(glob)**"
Cuando "claude-1" pide `safe_commit` de ":(glob)**"
Entonces la llamada se rechaza con el motivo "ruta no encontrada"
  Y no se crea ningún commit

**Escenario: Un archivo nuevo entra solo si el agente lo nombra**

Dado "shop-feat-a" con "src/nuevo.rs" sin seguimiento y "src/a.rs" preparado
Cuando "claude-1" pide `safe_commit` de "todo lo preparado"
Entonces el commit contiene "src/a.rs" y no contiene "src/nuevo.rs"
Cuando después pide `safe_commit` de "src/nuevo.rs"
Entonces el commit contiene "src/nuevo.rs"

**Escenario: Un archivo ignorado nunca se commitea**

Dado ".env" ignorado por Git en "shop-feat-a"
Cuando "claude-1" pide `safe_commit` de ".env"
Entonces la llamada se rechaza con el motivo "la ruta .env está ignorada por Git; no se añade"
  Y ".env" no entra en ningún commit

**Escenario: En un worktree compartido el agente tiene que nombrar sus rutas**

Dado "claude-1" y "claude-3" con sesión presente en "shop-feat-a"
Cuando "claude-1" pide `safe_commit` de "todo lo preparado"
Entonces la petición se rechaza con la acción "indica tus rutas: el worktree es compartido"
Cuando "claude-1" pide `safe_commit` de "src/a.rs"
Entonces el commit contiene solo "src/a.rs"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y ADR-CKP-002).
