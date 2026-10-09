---
id: INF-MCP-001
title: "Corpus de seguridad del MCP como suite de CI que bloquea el merge"
type: inf
status: partially-implemented
feature: mcp
domain: MCP
priority: high
complexity: medium
created: 2026-10-05
updated: 2026-10-09
related:
  adrs: [ADR-MCP-001, ADR-CKP-002, ADR-GRP-005, ADR-GRP-001]
  stories: [US-MCP-003, US-MCP-005, US-MCP-007, US-MCP-010, US-MCP-017, US-MCP-019, INF-GRP-001]
  specs: [DS-INF-MCP-001]
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

> Dev Spec: `dev-specs/INF-MCP-001-dev-spec.md`
>
> **Depende de**: US-MCP-003 (primer servidor que responde; el arnés nace con él) e INF-GRP-001 (huella del repo intacto y repos temporales). Cada historia de la tabla "Casos que aporta cada historia" **añade sus casos** al corpus en su propio PR; INF-MCP-001 aporta el arnés, el formato del caso, el gate de CI y los casos transversales. **ADRs**: ADR-MCP-001 § 1, § 2, § 5, § 6 y § 9; ADR-CKP-002 Validación 15, 19 y 27. **Seguridad**: SEC-MCP-01 a 11, SEC-02, SEC-05, SEC-12.

### Alcance Técnico

- **Arnés**: lanza el binario `raptor-mcp` como lo haría Claude Code (stdio, cwd elegido, entorno controlado), con un daemon de prueba en un perfil temporal (`GITRAPTOR_PROFILE_DIR`, solo en builds de test) y repos temporales. Nunca este repo ni el perfil real (NFR-01).
- **Formato del caso**: un archivo declarativo por caso (herramienta, parámetros, cwd, estado del repo, respuesta esperada: código estable, sin efectos, sin campos fuera de la allowlist). Un caso nuevo no exige código.
- **Comprobaciones comunes a todo caso**: el repo queda intacto (huella de INF-GRP-001) cuando el caso espera rechazo; la respuesta no pasa de los topes de ADR-MCP-001 § 6; ningún campo fuera de la allowlist; ningún secreto plantado: el arnés siembra en el repo, la config de git y el entorno canarios con forma de secreto real (tokens de GitHub, GitLab, Slack y Anthropic, clave de AWS, cabecera de clave privada, contraseña en la URL de un remoto). El caso falla si un canario, literal o escapado en JSON, o una forma de token conocida aparece en stdout o stderr. El fallo nombra el canario y nunca imprime el valor. *Decisión del orquestador (2026-10-09), validada por Arquitecto: sustituye a gitleaks, diferido para secretos no plantados (SEC-MCP-12).*
- **Casos transversales**: rutas maliciosas (traversal, absolutas, UNC, symlink hacia fuera, `:(glob)`), refs maliciosas (`--upload-pack=x`, `refs/heads/x`, hex de 40 y 64, `@{`, bidi y Tags), parámetros desconocidos y fuera de rango, `create_worktree` con `path`, cwd fuera de un worktree observado, repo fuera de la allowlist, repo de otro uid, entorno hostil (`HOME`, `PATH`, `GIT_DIR`), `acknowledge` con códigos de más y de menos, rate limit superado, respuesta de 3.000 archivos.
- **Casos de entrada alternativa y agotamiento**: cliente JSON-RPC directo que se declara `cli` bajo un agente simulado (SEC-MCP-01); muchas conexiones de un agente (SEC-MCP-03).
- **Gate**: la suite corre en CI en cada PR que toque `apps/mcp`, `crates/api` o el canal, y bloquea el merge. Informa el porcentaje del corpus rechazado (KPI de Q-MCP-18).
- **Huecos conocidos**: un caso puede llevar la marca `known_gap` con la historia que lo cierra cuando `raptor-mcp` aún no cumple un tope de ADR-MCP-001 § 6 (hoy, el de entrada: 1 MiB y profundidad 32; dueña US-MCP-005). El informe los cuenta aparte y fuera del KPI, y lo muestra junto a él ('100 % de N; M huecos conocidos'). Si un caso marcado pasa a rechazarse, la suite falla hasta quitar la marca. No relaja el contrato. Bloquea el corte de v0.1.0 (DEP-MCP-8) salvo excepción con ADR.
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
- Un canario que `raptor-mcp` devuelve rompe la suite; un `known_gap` que pasa a rechazarse la rompe hasta quitar la marca.

#### Verificación Manual / Sandbox

- Lanzar el arnés en macOS y revisar el informe del KPI.
- Linux y Windows (lectura del cwd de otro proceso, UNC): **Pendiente: etapa de validación multiplataforma**.

### Estado del corpus

| Grupo de casos | Historia dueña | Estado |
|---|---|---|
| Transversales de `status` y `snapshot` | INF-MCP-001 | Implementado |
| Ámbito (cwd fuera, subcarpeta, symlink, repo no habilitado) | US-MCP-003 | Implementado |
| Entorno hostil (`HOME`, `PATH`, `GIT_DIR`) | INF-MCP-001 | Implementado |
| JSON-RPC (forma, profundidad, basura con canario) | INF-MCP-001 | Implementado |
| Rate limit | US-MCP-005 | Implementado |
| PID reutilizado (`identity-unverified`) | US-MCP-003 | Pendiente |
| Cliente directo `cli` bajo un agente (S-01) | INF-MCP-001 | Pendiente |
| 50 conexiones de un agente (SEC-MCP-03) | US-MCP-009 | Pendiente |
| Repo de otro uid | INF-MCP-001 | Pendiente |
| Ruta UNC como cwd (XP-42) | INF-MCP-001 | Pendiente |
| Respuesta de 3.000 archivos | INF-MCP-001 | Pendiente |
| Tope de entrada, 1 MiB y 32 niveles (`known_gap`) | US-MCP-005 | Pendiente |
| Confused deputy | US-MCP-007 | Pendiente |
| Conexiones y cuotas | US-MCP-009 | Pendiente |
| Rutas de `safe_commit` | US-MCP-010 | Pendiente |
| `check_conflicts` | US-MCP-016 | Pendiente |
| Filtro y paginación de `explain_history` | US-MCP-017 | Pendiente |
| `undo` y `acknowledge` | US-MCP-018 | Pendiente |
| Ramas maliciosas y `path` de `create_worktree` | US-MCP-019 | Pendiente |

Implementado en: PR #224.

**Falta**: los casos de las herramientas futuras (filas pendientes de la tabla) y el tope de entrada, que `raptor-mcp` aún no aplica. Linux y Windows: *Pendiente: etapa de validación multiplataforma* (XP-42).
