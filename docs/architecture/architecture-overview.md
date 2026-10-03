---
mode: draft
status: draft
generated: 2026-10-03
generator: architect
domain: GRP
feature: motor-local
total_artifacts: 26
expanded: 0
approved: 0
related:
  context: [CTX-GRP-001]
  rules: [BR-GRP-001]
  adrs: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-008, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016]
---

# Architecture Overview — Motor local (F-001-01)

> Outline generado en modo Draft. Expande con `/aadd-expand <ID>`, `--range` o `--all`. Es un documento de navegación: enlaza los artefactos y no duplica su contenido.
>
> **Restricciones activas**: no hay `architecture-constitution.md` en la cascada. Rigen ADR-GRP-001 (stack) y ADR-GRP-002 (monorepo y crates). ⚠️ **ASSUMPTION**: se tratan como constitución hasta que se cree una formal con `/aadd-architect --init-constitution`.

## 1. Propósito y alcance

El motor local es la única fuente de verdad de GitRaptor (BRD § 11). Observa los repos que añade el desarrollador, sus worktrees, los eventos de Git y las sesiones de agentes, y lo expone a la CLI/TUI y al MCP.

Garantías que fijan el diseño:

- Solo observa. No escribe nada en el repo observado (Q21) y fuera del repo solo escribe su perfil (Q17).
- Observa sin huecos aunque no haya ninguna superficie abierta (BR-CONS-005).
- Funciona igual en Windows, macOS y Linux.

## 2. Vista de componentes (mapeada a ADR-GRP-002)

| Componente del motor | Ubicación en el monorepo | Responsabilidad | ADR |
|----------------------|--------------------------|-----------------|-----|
| Proceso del motor (daemon por usuario) | `apps/cli`: modo daemon del binario `raptor` (pendiente de PQ-5) | Ciclo de vida, instancia única, arranque bajo demanda y autoarranque (PQ-1) | ADR-GRP-005 |
| Registro de repos y orquestación | `crates/core` | Repos observados, estados del motor ("Esperando Git", "Sin repos", observando) | ADR-GRP-005, ADR-GRP-009 |
| Observador de cambios | `crates/core` | Watcher, debounce, sondeo de respaldo, reconciliación tras huecos | ADR-GRP-010 |
| Cálculo de estado por worktree | `crates/core` sobre `crates/git` | Rama, cambios, archivos, estados especiales, ahead/behind | ADR-GRP-009, ADR-GRP-010 |
| Detección de agentes (adaptador Claude Code) | `crates/core` (módulo de adaptadores por agente) | Sesiones, estados y evidencia de atribución | ADR-GRP-012 |
| Modelo de eventos, sesiones y atribución | `crates/core` | Eventos, registros, correcciones y confirmaciones | ADR-GRP-013 |
| Almacén del perfil | `crates/core` (módulo de almacenamiento) | Ubicación por SO, un archivo por repo, migraciones | ADR-GRP-006 |
| Capa de lectura de Git | `crates/git` | gitoxide en solo lectura, allowlist de lecturas del Git CLI, resolución y versión de Git | ADR-GRP-009 |
| Lector de configuración en tres niveles | `crates/policy` (compartido con Guardrails) | Lectura, validación con esquema y niveles admitidos por valor | ADR-GRP-007, ADR-GRP-008 |
| Contrato y transporte de clientes | `crates/api` | Métodos JSON-RPC, stream de eventos, versión del protocolo, cliente y servidor IPC | ADR-GRP-005 |
| Clientes | `apps/cli` (CLI/TUI) y `apps/mcp` (`raptor-mcp`) | Consumen el contrato; la presentación es de F-001-02 y el MCP de F-001-05 | — |
| — | `crates/theme` | No lo usa el motor | — |

⚠️ **ASSUMPTION**: el almacén y los adaptadores viven como módulos de `crates/core` para no crear crates fuera de ADR-GRP-002. Separarlos después exigiría enmendar ese ADR.

## 3. Skeleton Index

| ID | Tipo | Título | Resumen (1 línea) | Path objetivo (al expandir) | Status |
|----|------|--------|-------------------|-----------------------------|--------|
| ADR-GRP-005 | ADR | Forma del motor: proceso por usuario y canal local | Decisión: daemon por usuario con IPC local, para observar sin huecos | architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md | draft |
| ADR-GRP-006 | ADR | Perfil: ubicación, clave de repo y almacenamiento | Decisión: dirs estándar por SO y una base embebida por repo (P9) | architecture/decisions/ADR-GRP-006-perfil-ubicacion-almacenamiento.md | draft |
| ADR-GRP-007 | ADR | Configuración en tres niveles: formato y precedencia | Decisión: JSON con `$schema` y niveles admitidos por valor (P8) | architecture/decisions/ADR-GRP-007-configuracion-tres-niveles-formato.md | draft |
| ADR-GRP-008 | ADR | Configuración local personal sin versionar | Decisión: dónde vive el nivel local para que no se versione nunca (P10) | architecture/decisions/ADR-GRP-008-configuracion-local-no-versionada.md | draft |
| ADR-GRP-009 | ADR | Frontera de solo lectura e invocación de Git | Decisión: lecturas sin bloqueos opcionales y Git 2.38 resuelto sin efectos | architecture/decisions/ADR-GRP-009-frontera-solo-lectura-git.md | draft |
| ADR-GRP-010 | ADR | Observación de cambios en worktrees | Decisión: watcher nativo, debounce y reconciliación, para frescura y cero huecos | architecture/decisions/ADR-GRP-010-observacion-cambios-worktrees.md | draft |
| ADR-GRP-011 | ADR | Reparto del presupuesto de frescura | Decisión: motor ≤ 300 ms, Cockpit ≤ 100 ms y 100 ms de margen, medidos | architecture/decisions/ADR-GRP-011-presupuesto-frescura.md | draft |
| ADR-GRP-012 | ADR | Detección de sesiones de Claude Code | Decisión: proceso y cwd más evidencia positiva; ante la duda, "sin atribuir" | architecture/decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md | draft |
| ADR-GRP-013 | ADR | Modelo de eventos, sesiones y atribución | Decisión: los eventos apuntan a la sesión y se reatribuyen desde ella (Q37) | architecture/decisions/ADR-GRP-013-modelo-eventos-atribucion.md | draft |
| TS-GRP-001 | TS | Almacén de datos del motor en el perfil | Construir la persistencia por repo que habilita US-GRP-001, 004-007, 009-011, 015 | requirements/features/motor-local/technical-stories/TS-GRP-001-almacen-perfil.md | draft |
| TS-GRP-002 | TS | Capa de lectura de Git sin escrituras | Implementar las lecturas sin efectos que habilitan US-GRP-001, 002, 003, 012, 014 | requirements/features/motor-local/technical-stories/TS-GRP-002-lectura-git.md | draft |
| TS-GRP-003 | TS | Proceso del motor en segundo plano | Construir el proceso único por usuario que habilita US-GRP-004 y las demás | requirements/features/motor-local/technical-stories/TS-GRP-003-proceso-motor.md | draft |
| TS-GRP-004 | TS | Canal local de clientes y contrato | Exponer consultas, comandos y eventos que habilitan US-GRP-001..016 | requirements/features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md | draft |
| INF-GRP-001 | INF | Arnés "repo intacto" en los tres SO | Provisionar el gate transversal de BR-CONS-001 para todas las US | requirements/features/motor-local/technical-stories/INF-GRP-001-arnes-repo-intacto.md | draft |
| INF-GRP-002 | INF | Banco de frescura y escala | Provisionar los gates de NFR-04 y NFR-05 para US-GRP-001, 002, 012 | requirements/features/motor-local/technical-stories/INF-GRP-002-banco-frescura-escala.md | draft |
| SPIKE-GRP-001 | SPIKE | Precisión de detección de Claude Code | Medir el 90% en dogfooding sin atribuir trabajo humano; valida ADR-GRP-012 | requirements/features/motor-local/technical-stories/SPIKE-GRP-001-precision-deteccion.md | draft |
| SPIKE-GRP-002 | SPIKE | Viabilidad del observador a escala | Medir latencia, límites y bloqueos por SO; valida ADR-GRP-010 y 011 | requirements/features/motor-local/technical-stories/SPIKE-GRP-002-viabilidad-observador.md | draft |
| NFR-MOTOR | NFR | Requisitos no funcionales del motor | Requisito: NFR-01, 03-08 y 11 aplicados al motor, más Security NFRs | architecture/non-functional.md | draft |
| C4-GRP-L1 | C4-L1 | Contexto del motor local | Vista de contexto: desarrollador, Claude Code, Git, SO, Guardrails y clientes | architecture/diagrams/c4-context.md | draft |
| C4-GRP-L2 | C4-L2 | Contenedores del motor local | Vista de contenedores: daemon, CLI/TUI, MCP, perfil y repos | architecture/diagrams/c4-container.md | draft |
| SEQ-GRP-CAMBIO | Secuencia | Cambio en worktree → Cockpit | Cubre el flujo en vivo (US-GRP-002) con 5 actores y los timestamps de ADR-GRP-011 | architecture/diagrams/seq-cambio-worktree-cockpit.md | draft |
| SEQ-GRP-DETECCION | Secuencia | Detección de sesión de Claude Code | Cubre la detección y los estados (US-GRP-007, 008) con 5 actores | architecture/diagrams/seq-deteccion-sesion-claude-code.md | draft |
| SEQ-GRP-HUECO | Secuencia | Reconciliación tras un hueco (opcional) | Cubre el arranque o la vuelta de suspensión (US-GRP-004, 005) con 4 actores | architecture/diagrams/seq-reconciliacion-hueco.md | draft |
| API-GRP-IPC | API Spec | Contrato del canal local | Contrato JSON-RPC con stream de eventos (formato OpenRPC, no OpenAPI) | architecture/design/api-contract-ipc.md | draft |
| DATA-GRP-PERFIL | Data Model | Modelo de datos del perfil | Repos, sesiones, registros, eventos, correcciones y huecos | architecture/design/data-model-perfil.md | draft |
| THREAT-GRP-MOTOR | Threat Model | Modelo de amenazas del motor | STRIDE de la IPC local, archivos de terceros y perfil (delegar en `security-expert`) | architecture/security/threat-model-motor.md | draft |

## 4. Decisiones clave

Ver [decisions/index.md](./decisions/index.md): resumen de opciones, recomendación, impacto y decisión de producto de cada ADR.

## 5. Flujos críticos

Filas SEQ-GRP-CAMBIO, SEQ-GRP-DETECCION y SEQ-GRP-HUECO del Skeleton Index. Todavía no están dibujados.

## 6. Atributos de calidad

Ver [non-functional.md](./non-functional.md). Los tres que más pesan en el diseño:

- **Cero escrituras en el repo**: NFR-01 y BR-CONS-001.
- **Continuidad sin huecos**: BR-CONS-005.
- **Frescura < 500 ms**: NFR-04.

## 7. Enablers técnicos

Ver [technical-stories.md](../requirements/features/motor-local/technical-stories.md): 4 TS, 2 INF y 2 SPIKE.

### Trabajo técnico que vive en la US (no es TS)

Lo que tiene una sola historia dueña va en la Dev Spec de esa historia, según la regla del dueño:

| Trabajo técnico | Historia dueña | ADR |
|-----------------|----------------|-----|
| Watcher y recomputo incremental | US-GRP-002 | ADR-GRP-010 |
| Detección de estados especiales y worktree no disponible | US-GRP-003 | ADR-GRP-010 |
| Registro del autoarranque por SO (bloqueado por PQ-1) | US-GRP-004 | ADR-GRP-005 |
| Reconciliación tras un hueco | US-GRP-005 | ADR-GRP-010, ADR-GRP-013 |
| Adaptador de detección de Claude Code y modelo de sesión | US-GRP-007 | ADR-GRP-012, ADR-GRP-013 |
| Contrato de "quién hizo un evento" | US-GRP-009 | ADR-GRP-013 |
| Ahead/behind sin tocar el remoto | US-GRP-012 | ADR-GRP-009 |
| Lector de configuración en tres niveles (coordinado con F-001-04) | US-GRP-013 | ADR-GRP-007, ADR-GRP-008 |
| Resolución de Git, versión y estado "Esperando Git" | US-GRP-014 | ADR-GRP-009 |

## 8. Índice de Dev Specs

Pendiente. Se generan con `/aadd-devspec <id>` cuando las historias estén al menos en `expanded`: una por TS e INF y una por cada US de la tabla anterior. Los SPIKE llevan un Research Brief, no una Dev Spec.

## 9. Preguntas abiertas y riesgos

- **Preguntas de producto**: PQ-1 a PQ-9, detalladas en la entrega de esta fase y referenciadas en [decisions/index.md](./decisions/index.md). Las que bloquean la expansión:
  - PQ-1: autoarranque frente a Q17.
  - PQ-2: transcripts de Claude Code frente a NFR-08.
  - PQ-3: ubicación de `settings.local.json`.
- **Abiertas del PO que afectan a ADR-GRP-013**: P16 (origen de una sesión confirmada) y P17 (eventos reatribuidos al retirar una corrección).
- **Ruta crítica**: los cuatro TS van antes del esqueleto andante (US-GRP-001). La ruta pasa a TS-GRP-001 y TS-GRP-002 → TS-GRP-003 → TS-GRP-004 → US-GRP-001 → US-GRP-002. El Scrum Master debe recalcular las olas.
- **Riesgo de atribución**: sin evidencia positiva por cambio (PQ-2), los cambios sin commitear en un worktree con editor abierto quedan casi siempre "sin atribuir". Puede que la meta del 90% se cumpla para sesiones y no para cambios (R2, R7).
- **Riesgo de Windows**: los handles del watcher podrían impedir borrar o mover worktrees. Lo verifica SPIKE-GRP-002 (R4).
