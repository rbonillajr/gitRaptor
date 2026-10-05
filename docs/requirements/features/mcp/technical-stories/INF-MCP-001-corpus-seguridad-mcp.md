---
id: INF-MCP-001
title: "Corpus de seguridad del MCP como suite de CI que bloquea el merge"
type: inf
status: draft
feature: mcp
domain: MCP
priority: high
complexity: medium
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-MCP-001, ADR-CKP-002, ADR-GRP-005, ADR-GRP-001]
  stories: [US-MCP-003, US-MCP-005, US-MCP-007, US-MCP-010, US-MCP-017, US-MCP-019, INF-GRP-001]
  specs: []
ado:
  id: null
  url: null
tags: [mcp, seguridad, corpus, ci, owasp-mcp-top-10, nfr-02, br-16, dep-mcp-8, sec-mcp]
---

## INF-MCP-001: Corpus de seguridad del MCP como suite de CI

**Valor**: el KPI "100 % del corpus de seguridad rechazado" (Q-MCP-18) se mide en cada PR, y una regresión en la validación de entradas, en el ámbito o en la frontera de dependencias de `raptor-mcp` rompe el CI antes del merge.

### Descripción

**Como** Arquitecto
**Quiero** un arnés que lance `raptor-mcp` por stdio contra un daemon de prueba y repos temporales y ejecute un corpus de ataques versionado
**Para** que BR-16, NFR-02 y SEC-MCP-01 a 11 se verifiquen en CI y no dependan de la revisión manual por release (DEP-MCP-8, ADR-MCP-001 § 9)

> Dev Spec: `dev-specs/INF-MCP-001-corpus-seguridad-mcp.md` | Pendiente
>
> **Depende de**: US-MCP-003 (primer servidor que responde; el arnés nace con él) e INF-GRP-001 (huella del repo intacto y repos temporales). Cada historia de la tabla "Casos que aporta cada historia" **añade sus casos** al corpus en su propio PR; INF-MCP-001 aporta el arnés, el formato del caso, el gate de CI y los casos transversales. **ADRs**: ADR-MCP-001 § 1, § 2, § 5, § 6 y § 9; ADR-CKP-002 Validación 15, 19 y 27. **Seguridad**: SEC-MCP-01 a 11, SEC-02, SEC-05, SEC-12.

### Alcance Técnico

- **Arnés**: lanza el binario `raptor-mcp` como lo haría Claude Code (stdio, cwd elegido, entorno controlado), con un daemon de prueba en un perfil temporal (`GITRAPTOR_PROFILE_DIR`, solo en builds de test) y repos temporales. Nunca este repo ni el perfil real (NFR-01).
- **Formato del caso**: un archivo declarativo por caso (herramienta, parámetros, cwd, estado del repo, respuesta esperada: código estable, sin efectos, sin campos fuera de la allowlist). Un caso nuevo no exige código.
- **Comprobaciones comunes a todo caso**: el repo queda intacto (huella de INF-GRP-001) cuando el caso espera rechazo; la respuesta no pasa de los topes de ADR-MCP-001 § 6; ningún campo fuera de la allowlist; ningún secreto plantado (gitleaks sobre las respuestas y sobre stderr).
- **Casos transversales**: rutas maliciosas (traversal, absolutas, UNC, symlink hacia fuera, `:(glob)`), refs maliciosas (`--upload-pack=x`, `refs/heads/x`, hex de 40 y 64, `@{`, bidi y Tags), parámetros desconocidos y fuera de rango, `create_worktree` con `path`, cwd fuera de un worktree observado, repo fuera de la allowlist, repo de otro uid, entorno hostil (`HOME`, `PATH`, `GIT_DIR`), `acknowledge` con códigos de más y de menos, rate limit superado, respuesta de 3.000 archivos.
- **Casos de entrada alternativa y agotamiento**: cliente JSON-RPC directo que se declara `cli` bajo un agente simulado (SEC-MCP-01); muchas conexiones de un agente (SEC-MCP-03).
- **Gate**: la suite corre en CI en cada PR que toque `apps/mcp`, `crates/api` o el canal, y bloquea el merge. Informa el porcentaje del corpus rechazado (KPI de Q-MCP-18).
- **Fuera de alcance** (ajuste del Arquitecto): la instantánea de `initialize` y `tools/list` (dueña US-MCP-005); la comprobación estática de la frontera de dependencias de `apps/mcp` (dueña US-MCP-003, ampliando la Validación 5 de ADR-GRP-009); la revisión manual OWASP / MCP Top 10 por release (SEC-MCP-08, security-expert); los casos funcionales de cada herramienta (van en su historia); fuzzing del decodificador del canal (SEC-02, ya en motor-local).

### Casos que aporta cada historia

| Historia | Casos |
|---|---|
| US-MCP-003 | Ámbito: cwd fuera, subcarpeta, `cd` del agente; repo no habilitado; PID reutilizado |
| US-MCP-005 | Texto no confiable (OSC, bidi, Tags), topes de respuesta, rate limit, catálogo fijo |
| US-MCP-007 | Confused deputy: hook que pide un reservado, Cancelar o una operación del catálogo |
| US-MCP-010 | Rutas de `safe_commit`: traversal, ignorados, `:(glob)`, symlink |
| US-MCP-017 | Filtro y paginación de `explain_history` sin mensajes ni contenido |
| US-MCP-019 | Ramas maliciosas y `path` rechazado por esquema; padre con enlace simbólico |

### Plan de Verificación

#### Pruebas Automatizadas

- Un caso de traversal que, por un fallo inyectado, no se rechaza hace fallar la suite y el CI.
- Un cliente directo que se declara `cli` bajo un agente simulado y obtiene datos de un repo no habilitado rompe el CI.
- Una respuesta con un campo nuevo no declarado rompe la comprobación de allowlist.

#### Verificación Manual / Sandbox

- Lanzar el arnés en macOS y revisar el informe del KPI.
- Linux y Windows (lectura del cwd de otro proceso, UNC): **Pendiente: etapa de validación multiplataforma**.
