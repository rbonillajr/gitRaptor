---
mode: bulk
status: expanded
generated: 2026-10-04
updated: 2026-10-04
generator: architect
domain: GRP
feature: guardrails
total_artifacts: 3
expanded: 3
approved: 0
related:
  context: [CTX-GRD-001]
  rules: [BR-GRD-001]
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007]
---

# Technical Stories — INDEX: Guardrails

> Índice. Cada historia vive en su archivo, dentro de [`technical-stories/`](./technical-stories/), con `status: draft` en el frontmatter. La columna Status de esta tabla indica el siguiente paso: `Dev Spec Pending` para los INF y los TS (la Dev Spec se genera con `/aadd-devspec <id>`) y `Research Pending` para el SPIKE, que lleva un Research Brief y no una Dev Spec.
>
> **Criterio de inclusión (Enabler Decision Gate)**: solo entra el trabajo técnico **sin historia de usuario dueña y sin resultado observable** por el usuario. Hay 3 enablers para 17 historias. El resto del trabajo técnico (instalador, motor de decisión, estado de protección, registro, token de excepción) tiene una historia dueña y va en su Dev Spec, siguiendo ADR-GRD-001..007.

## Índice

| ID | Tipo | Título | Valor (1 línea) | ADR | US que habilita | Depende de | Complejidad | Status |
|----|------|--------|-----------------|-----|-----------------|-----------|-------------|--------|
| [SPIKE-GRD-001](./technical-stories/SPIKE-GRD-001-interceptabilidad-hooks.md) | SPIKE | Interceptabilidad, coexistencia y coste de la capa de hooks en los tres SO | Confirmar la matriz de operaciones, el encadenado con gestores, la cobertura de worktrees y el presupuesto < 100 ms antes de convertirlos en contrato | ADR-GRD-001, ADR-GRD-002 (valida) | US-GRD-001, 002, 003, 004, 007 | — (prototipo aislado) | Medium | Research Pending |
| [INF-GRD-001](./technical-stories/INF-GRD-001-arnes-hooks.md) | INF | Arnés de la capa de hooks: repos con hooks previos, huella, interrupción y matriz de CI | Gate de CI de NFR-01 y NFR-12 de la capa de hooks y de la fidelidad de la lista publicada | ADR-GRD-001, ADR-GRD-002, ADR-GRD-005 | US-GRD-001..006 (suites); regresión para US-GRD-007 en adelante | Núcleo de INF-GRP-001; SPIKE-GRD-001 | Medium | Dev Spec Pending |
| [TS-GRD-001](./technical-stories/TS-GRD-001-configuracion-commiteada.md) | TS | Lectura commiteada de la configuración del equipo y de la rama principal | Un solo cargador y un solo criterio para Guardrails y el motor, con el suelo en la rama principal (Q-GRD-17 refinada por D6, Q-GRD-18, R-GRD-8) | ADR-GRD-004 | US-GRD-007, 011, 014; US-GRP-016 | US-GRP-013 (reutiliza su cargador de tres niveles). Desbloqueada el 2026-10-04: ADR-GRP-007 `accepted` con sus enmiendas (PQ-9, `permissions`/`policies`, estado por fuente) | High | Dev Spec Pending |

## Encaje con el DAG de historias

| Historia | Espera a (enablers) | Por qué |
|----------|---------------------|---------|
| US-GRD-001 | SPIKE-GRD-001 (parte de force-push, borrado de la rama base y coste en Windows) antes del merge; núcleo de INF-GRD-001 y su suite de instalación | El esqueleto andante fija el contrato de decisión (ADR-GRD-003) y el instalador (ADR-GRD-001); el gate de huella tiene que existir antes de tocar repos |
| US-GRD-002 | SPIKE-GRD-001 completo (coexistencia) antes de cerrar su Dev Spec; suite de encadenado de INF-GRD-001 | El comportamiento real de husky, lefthook y pre-commit decide qué casos son "no se instala" |
| US-GRD-003 | Suite de interrupción de INF-GRD-001 | NFR-12 solo se verifica con cortes en cada paso de la transacción |
| US-GRD-004 | SPIKE-GRD-001 completo (matriz) antes de cerrar su Dev Spec; suite de pérdida externa y de la lista de INF-GRD-001 | La lista publicada es el dato que confirma el SPIKE y que la regresión vigila |
| US-GRD-005 | Suite de registro de INF-GRD-001 (huella sin cambios) | "El registro no está en el repo" se comprueba con la huella |
| US-GRD-006 | Suite de token de INF-GRD-001 | Higiene del token al encadenar hooks previos |
| US-GRD-007, 011, 014 | TS-GRD-001 (y US-GRP-013, como ya marca el índice de historias; P8 quedó cerrada por ADR-GRP-007 el 2026-10-04) | Leen el suelo y el commit del worktree, o la rama base desde la rama principal. US-GRD-007 y US-GRD-014 aportan además los comandos de confirmación de D6 (ADR-GRD-007) |
| US-GRD-013, 015 | ADR del factor fuera de banda del SO (todavía sin crear; gate duro de D5) | Relajar la configuración y aprobar en la cola no salen sin ese factor; si no está disponible, fail-closed |
| US-GRP-016 (motor-local) | TS-GRD-001 (desbloqueada el 2026-10-04 por decisión del coordinador, pendiente de confirmar por Rene) | Lee la rama base confirmada de la copia de la rama principal con el mismo cargador |

**Ruta crítica ejecutable hoy**: SPIKE-GRD-001 en paralelo con US-GRP-001 → núcleo de INF-GRD-001 → US-GRD-001 → US-GRD-005 → US-GRD-006.
