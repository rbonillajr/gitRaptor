---
mode: draft
status: draft
generated: 2026-10-03
generator: architect
domain: GRP
feature: motor-local
total_artifacts: 8
expanded: 0
approved: 0
related:
  context: [CTX-GRP-001]
  rules: [BR-GRP-001]
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013]
---

# Technical Stories — INDEX: Motor local

> Outline generado en modo Draft. Expande con `/aadd-expand <ID>`. Cada expansión escribe `technical-stories/<id>-<slug>.md` (TS e INF con las 3 secciones canónicas; SPIKE con forma de investigación), y su Dev Spec se genera después con `/aadd-devspec <id>`. Los SPIKE no tienen Dev Spec: llevan un Research Brief.
>
> **Criterio de inclusión (Enabler Decision Gate)**: solo trabajo técnico **sin historia de usuario dueña y sin resultado observable** por el usuario. Lo observable ya está en US-GRP-001..016. Hay 8 enablers para 16 historias.
>
> **Trabajo técnico de una sola historia**: va en la Dev Spec de esa historia, no aquí. Ver "Trabajo técnico que vive en la US" en el [architecture overview](../../../architecture/architecture-overview.md).

## Outline

| ID | Tipo | Título | Resumen (1 línea) | ADR | US que habilita | Depende de | Complejidad | Status |
|----|------|--------|-------------------|-----|-----------------|-----------|-------------|--------|
| TS-GRP-001 | TS | Almacén de datos del motor en el perfil | Construir la ubicación por SO, la separación por repo y la persistencia transaccional que habilitan todas las US con datos | ADR-GRP-006, ADR-GRP-013 | US-GRP-001, 004, 005, 006, 007, 009, 010, 011, 015 | — | Medium | draft |
| TS-GRP-002 | TS | Capa de lectura de Git sin escrituras | Implementar las lecturas de repos y worktrees y la resolución del Git del sistema, sin efectos en el repo | ADR-GRP-009 | US-GRP-001, 002, 003, 012, 014 | — | Medium | draft |
| TS-GRP-003 | TS | Proceso del motor en segundo plano por usuario | Construir el proceso único por usuario, su ciclo de vida y el arranque bajo demanda que habilitan la continuidad | ADR-GRP-005 | US-GRP-001, 002, 004, 005, 014, 015 | TS-GRP-001 | High | draft |
| TS-GRP-004 | TS | Canal local de clientes y contrato de mensajes | Exponer consultas, comandos y el stream de eventos a la CLI y al MCP, restringido al usuario | ADR-GRP-005 | US-GRP-001..016 (verificación por estado expuesto) | TS-GRP-003 | Medium | draft |
| INF-GRP-001 | INF | Arnés de verificación "repo intacto" en los tres SO | Provisionar el arnés que compara el repo y la máquina antes y después de observar, como gate de CI transversal | ADR-GRP-009, ADR-GRP-006 | Todas, de forma transversal (BR-CONS-001) | TS-GRP-002 | Medium | draft |
| INF-GRP-002 | INF | Banco de medición de frescura y escala | Provisionar el banco de latencia por etapa y de escala (10+ worktrees, 100K commits) como gate de CI | ADR-GRP-011, ADR-GRP-010 | US-GRP-001, 002, 012 | TS-GRP-004, US-GRP-002 | Medium | draft |
| SPIKE-GRP-001 | SPIKE | Precisión de la detección de Claude Code en dogfooding | Medir si las señales de ADR-GRP-012 alcanzan el 90% sin atribuir trabajo humano a Claude Code | ADR-GRP-012 (valida) | US-GRP-007, 008 | — (prototipo aislado); respuesta a PQ-2 para la variante con transcripts | Medium | draft |
| SPIKE-GRP-002 | SPIKE | Viabilidad del observador de cambios a escala en los tres SO | Medir latencia, límites de vigilancia, bloqueos de borrado en Windows y huecos por suspensión | ADR-GRP-010, ADR-GRP-011 (valida) | US-GRP-002, 003, 004, 005 | — (prototipo aislado) | Medium | draft |

## Ruta de ejecución sugerida

- **Antes del esqueleto andante**: TS-GRP-001 y TS-GRP-002 en paralelo; luego TS-GRP-003 y después TS-GRP-004. INF-GRP-001 sale tras TS-GRP-002 y bloquea el merge de cualquier historia (BR-CONS-001 es transversal).
- **En paralelo desde el día uno**: SPIKE-GRP-001 y SPIKE-GRP-002, que no dependen de código del motor. Sus resultados pueden cambiar ADR-GRP-012 y ADR-GRP-010 antes de que US-GRP-002 y US-GRP-007 entren en desarrollo.
- **Después de US-GRP-002**: INF-GRP-002.
- ⚠️ **ASSUMPTION**: la base de CI del monorepo con runners de Windows, macOS y Linux viene del spike del stack (ADR-GRP-001) o de un bootstrap común. Si no existe, INF-GRP-001 la incorpora.

## Ficha de outline — SPIKE-GRP-001

> Resumen para dimensionar el spike. El Research Brief completo se escribe al expandir.

- **Qué se mide**:
  - **Precisión de sesiones (Q9)**: sesiones de Claude Code detectadas correctamente frente al total de sesiones reales. Cuenta como fallo cada sesión que el desarrollador tuvo que registrar o corregir a mano.
  - **Errores humano → Claude Code**: cambios del humano atribuidos a Claude Code (BR-EDGE-004).
  - **Precisión por señal**: S1, S2 (solo mtime o con contenido), S3 y S4, con y sin las señales de hooks de Guardrails (R7).
  - **Transiciones de estado**: si llegan dentro del umbral.
- **Verdad de referencia**:
  - **Suite guionizada** con etiquetas exactas:
    - Claude Code y edición humana en el mismo worktree y en otro.
    - Claude Code lanzado en un directorio que modifica otro (R3).
    - Dos sesiones en un worktree.
    - Cierre forzado y suspensión.
  - **Dogfooding real**, con una revisión diaria en la que Rene marca cada sesión detectada como correcta o incorrecta y añade las que faltaron.
- **Duración**: time-box de 2 semanas de dogfooding más 2 días de suite guionizada (BRD § 13, spike c). ⚠️ **ASSUMPTION**: al menos 50 sesiones reales para que el porcentaje sea significativo.
- **Éxito**: precisión ≥ 90% en dogfooding (macOS) y en la suite guionizada en los tres SO, y **0** cambios humanos atribuidos a Claude Code en la suite.
- **Fracaso**: precisión < 90%, o cualquier atribución humano → Claude Code que la regla "ante la duda, sin atribuir" no evite. En ese caso:
  - Se replantea ADR-GRP-012: S2 de contenido u opt-in de S5.
  - Se refuerza el registro explícito.
  - Se escala a Rene el posible replanteo de US-GRP-007 y 008.
- **Valida**: ADR-GRP-012. También informa ADR-GRP-013 (qué evidencia se guarda por evento).
