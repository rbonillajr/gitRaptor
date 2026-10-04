---
mode: bulk
generated: 2026-10-04T00:00Z
updated: 2026-10-04
generator: product-owner
total_artifacts: 17
expanded: 17
approved: 0
blocked:
  - US-GRD-007
  - US-GRD-008
  - US-GRD-009
  - US-GRD-010
  - US-GRD-011
  - US-GRD-012
  - US-GRD-013
  - US-GRD-014
  - US-GRD-015
  - US-GRD-016
  - US-GRD-017
---

# User Stories — INDEX: Guardrails

> Generado en modo Bulk (modelo plano). Cada historia vive en su archivo de `user-stories/`. Esta tabla solo enlaza y marca el Status.

---

## Contexto del Feature

**Feature**: Guardrails (F-001-04)
**Epic**: E-001 — MVP Fase 1: Cockpit + Time Machine + Guardrails (CLI/TUI + MCP)
**Prioridad**: Alta (BR-11 y BR-12 Must; BR-13 Should)
**Estado**: Listo para Desarrollo: US-GRD-001 a US-GRD-006. El resto está bloqueado (ver `blocked`).

**Enlace a contexto completo**: [`context.md`](./context.md) (CTX-GRD-001)
**Reglas de negocio**: [`business-rules.md`](./business-rules.md) (BR-GRD-001, 23 reglas) · **Diseño**: no aplica (sin superficie propia; la cola y el estado de protección los presenta el Cockpit, F-001-02, y la CLI)

---

## Análisis de División (Sistema de 4 Pasos)

- **Paso 1 — Flujo**: actor principal = desarrollador orquestador. Resultado buscado = que ningún agente destruya trabajo ni reescriba la rama base. Camino mínimo = proteger un repo con permiso → un force-push queda denegado con su motivo.
- **Paso 2 — Puntos de resultado**: cada historia deja una decisión, un estado o una entrada de registro que se comprueba desde fuera, intentando la operación y consultando el resultado.
- **Paso 3 — Patrón aplicado**: camino feliz primero. El esqueleto andante es US-GRD-001: capa de hooks con el mínimo seguro, sin configuración. No espera al ADR P8 (motor-local) ni al MCP (F-001-05) y ya demuestra el force-push bloqueado del BRD § 13. Después va por capacidad acumulable: instalación recuperable → registro y excepción → reglas configuradas (orden de Q-GRD-9) → precedencia → fail-safe → comando → cola. El corte por capa (MCP, US-GRD-016) se justifica porque es la otra vía del agente y su resultado es observable: la misma decisión.
- **Paso 4 — Filtro**: todas pasan (usuario real · resultado observable · cabe en una rama corta de un agente · comportamiento, no código).

> **Criterio de bloqueo**: una historia está bloqueada si depende del ADR de formato P8 (motor-local), de una historia de motor-local bloqueada (US-GRP-013) o de una feature sin historias todavía: Cockpit (F-001-02), Time Machine (F-001-03) o Servidor MCP (F-001-05). Las demás historias aprobadas de motor-local son dependencias de historia, no bloqueos.
>
> **Lectura de la configuración en tres niveles**: tiene un solo dueño, US-GRP-013 (motor-local). Las historias de Guardrails que leen la configuración (desde US-GRD-007) la reutilizan; ninguna la vuelve a definir.
>
> **Transversales (no son historias)**: el mismo comportamiento en Windows, macOS y Linux (BR-03, NFR-06) y nada modificado fuera del repo ni hooks previos perdidos (NFR-01) se exigen en todos los escenarios. Cómo se distingue al humano en las acciones reservadas es transversal (lo define el Arquitecto; R-GRD-3).
>
> **Sin snapshot previo hasta US-GRD-017**: mientras no exista la Time Machine (F-001-03) y US-GRD-017, las operaciones destructivas que US-GRD-001 a US-GRD-006 permiten se ejecutan sin snapshot previo. Es un límite conocido de esas historias, no un incumplimiento de NFR-01 por su parte.
>
> **US-GRD-001 se puede partir**: el Arquitecto puede dividirla en "instalar la protección con permiso" y "decidir con el mínimo seguro" si no cabe en una rama corta. Las dos mitades conservan sus escenarios tal como están.

---

## Índice de Historias

| ID | Título | Descripción (1 línea) | Status |
|----|--------|-----------------------|--------|
| [US-GRD-001](./user-stories/US-GRD-001-proteger-repo-force-push-bloqueado.md) | Un agente que intenta hacer force-push en un repo protegido queda bloqueado | Desarrollador quiere proteger un repo con permiso y que el mínimo seguro deniegue force-push y borrar la rama base | expanded |
| [US-GRD-002](./user-stories/US-GRD-002-hooks-previos-respetados.md) | Los hooks que el repo ya tenía siguen funcionando al protegerlo | Desarrollador quiere encadenar con sus hooks previos o no instalar nada si no se puede | expanded |
| [US-GRD-003](./user-stories/US-GRD-003-retirar-proteccion-sin-rastro.md) | El desarrollador retira la protección y el repo queda exactamente como estaba | Desarrollador quiere desinstalar sin rastro y que una instalación interrumpida no quede a medias | expanded |
| [US-GRD-004](./user-stories/US-GRD-004-aviso-proteccion-inactiva.md) | El desarrollador se entera de que la protección de un repo dejó de estar activa | Desarrollador quiere un aviso si los hooks desaparecen, ver el mínimo seguro y saber qué no se puede impedir | expanded |
| [US-GRD-005](./user-stories/US-GRD-005-registro-de-bloqueos.md) | El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo | Desarrollador quiere un registro por repo, en el perfil, de 90 días, para el KPI | expanded |
| [US-GRD-006](./user-stories/US-GRD-006-excepcion-consciente.md) | El desarrollador hace a conciencia una operación prohibida y queda constancia | Desarrollador quiere reglas para cualquier actor y una excepción consciente registrada | expanded |
| [US-GRD-007](./user-stories/US-GRD-007-permisos-por-operacion.md) | El equipo decide qué operaciones de Git se permiten, se deniegan o piden confirmación | Desarrollador quiere fijar el permiso de cada operación en la configuración del equipo (bloqueada: P8, US-GRP-013) | expanded |
| [US-GRD-008](./user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md) | Ningún agente cambia una rama protegida ni toca una ruta prohibida | Desarrollador quiere ramas protegidas y rutas prohibidas (bloqueada: P8) | expanded |
| [US-GRD-009](./user-stories/US-GRD-009-tamano-diff-formato-commit.md) | Los agentes entregan commits pequeños y con el formato que exige el equipo | Desarrollador quiere límite de diff y formato de commit (bloqueada: P8) | expanded |
| [US-GRD-010](./user-stories/US-GRD-010-endurecer-sin-relajar.md) | El desarrollador endurece las reglas en su máquina sin poder relajar las del equipo | Desarrollador quiere la precedencia de Q-GRD-14 entre equipo, perfil y local (bloqueada: P8) | expanded |
| [US-GRD-011](./user-stories/US-GRD-011-configuracion-ilegible.md) | Una configuración rota no deja pasar las operaciones peligrosas | Desarrollador quiere que una configuración ilegible mantenga el mínimo seguro (bloqueada: P8) | expanded |
| [US-GRD-012](./user-stories/US-GRD-012-agente-no-relaja-configuracion.md) | Un agente no puede relajar las reglas del equipo cambiando su configuración | Desarrollador quiere la configuración como ruta prohibida para agentes (bloqueada: P8) | expanded |
| [US-GRD-013](./user-stories/US-GRD-013-comando-edicion-configuracion.md) | El desarrollador cambia la configuración con un comando sin perder lo que editó a mano | Desarrollador quiere un comando seguro que respete niveles y cambios a mano (bloqueada: P8) | expanded |
| [US-GRD-014](./user-stories/US-GRD-014-rama-base-del-equipo.md) | El equipo fija la rama base del repo y Guardrails la protege | Desarrollador quiere que Guardrails proteja la rama base que fija el equipo (bloqueada: P8) | expanded |
| [US-GRD-015](./user-stories/US-GRD-015-cola-de-confirmacion.md) | El desarrollador aprueba o rechaza las acciones de riesgo que un agente deja en espera | Desarrollador quiere la cola de confirmación con caducidad de 5 minutos (bloqueada: Cockpit y P8) | expanded |
| [US-GRD-016](./user-stories/US-GRD-016-misma-decision-por-mcp.md) | Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo | Desarrollador quiere la misma decisión por MCP para todo el catálogo y los estados según la allowlist (bloqueada: F-001-05 y P8) | expanded |
| [US-GRD-017](./user-stories/US-GRD-017-sin-snapshot-no-se-ejecuta.md) | Una operación destructiva permitida no se ejecuta sin un punto de recuperación | Desarrollador quiere que sin snapshot previo la operación se deniegue (bloqueada: F-001-03) | expanded |

---

## Mapa para la flota

> Un agente por historia y por worktree (D3). "Depende de" = historias que deben estar integradas antes de empezar. Las historias de motor-local se nombran como US-GRP-NNN.

| ID | Reglas cubiertas | Depende de | Externas | Prioridad | Valor en una línea |
|----|------------------|------------|----------|-----------|--------------------|
| US-GRD-001 | BR-AUTH-002, BR-EDGE-001, BR-CALC-001, BR-WF-002 | US-GRP-001, US-GRP-012 | — | Must | La demo del force-push bloqueado funciona desde el primer día |
| US-GRD-002 | BR-EDGE-002, BR-CONS-005, BR-AUTH-002 | US-GRD-001 | — | Must | Proteger un repo no rompe lo que ya tenía |
| US-GRD-003 | BR-CONS-005, BR-WF-002 | US-GRD-001, US-GRD-002 | — | Must | Probar Guardrails es reversible del todo |
| US-GRD-004 | BR-WF-002, BR-EDGE-003, BR-EDGE-001 (visible) | US-GRD-001, US-GRP-006 | — | Should | El estado de protección nunca miente y dice qué reglas aplican |
| US-GRD-005 | BR-CONS-004, BR-TIME-002 | US-GRD-001, US-GRP-009 | — | Must | El KPI "acciones peligrosas bloqueadas" se mide |
| US-GRD-006 | BR-AUTH-003, BR-AUTH-001, BR-CONS-004 | US-GRD-001, US-GRD-005, US-GRP-007, US-GRP-009 | Arquitecto: distinguir al humano (R-GRD-3) | Must | Fail-safe sin dejar atrapado al humano |
| US-GRD-007 | BR-VAL-002, BR-CALC-001, BR-VAL-001, BR-EDGE-001 | US-GRD-001, US-GRD-004, US-GRP-013 | **Bloqueada**: P8 (motor-local) y US-GRP-013; un escenario depende de P-GRD-17 | Must | Las reglas del repo viajan con él |
| US-GRD-008 | BR-VAL-003 (ramas protegidas, rutas prohibidas), BR-CALC-001 | US-GRD-007 | **Bloqueada**: P8 | Must | Que nadie rompa main |
| US-GRD-009 | BR-VAL-003 (tamaño de diff, formato de commit) | US-GRD-008 | **Bloqueada**: P8 | Should | Diffs revisables y un historial legible |
| US-GRD-010 | BR-CONS-001, BR-VAL-001 | US-GRD-007, US-GRD-008 | **Bloqueada**: P8 | Must | Las reglas del equipo son un suelo |
| US-GRD-011 | BR-EDGE-004 | US-GRD-010 | **Bloqueada**: P8; dos escenarios dependen de P-GRD-17; alinear con BR-CONS-007 (motor-local), Arquitecto | Must | Nunca "todo permitido" |
| US-GRD-012 | BR-AUTH-004, BR-AUTH-001 | US-GRD-008, US-GRD-010 | **Bloqueada**: P8; un escenario depende de P-GRD-17; R-GRD-3 | Must | Las reglas no las cambia quien está sujeto a ellas |
| US-GRD-013 | BR-CONS-006, BR-VAL-001, BR-CONS-001, BR-AUTH-001 | US-GRD-010, US-GRD-014 | **Bloqueada**: P8; un escenario depende de P-GRD-17; R-GRD-3 | Should | Editar la configuración es seguro |
| US-GRD-014 | BR-CONS-003, BR-EDGE-001 | US-GRD-007 | **Bloqueada**: P8 | Should | La rama de integración del equipo queda protegida |
| US-GRD-015 | BR-WF-001, BR-TIME-001, BR-AUTH-001 | US-GRD-004, US-GRD-005, US-GRD-007 | **Bloqueada**: Cockpit F-001-02 y P8 | Should | El humano tiene la última palabra en lo arriesgado |
| US-GRD-016 | BR-CONS-002, BR-WF-002, BR-AUTH-004 | US-GRD-001, US-GRD-004, US-GRD-007 | **Bloqueada**: Servidor MCP F-001-05 y P8 | Must | Dos capas, una sola decisión |
| US-GRD-017 | BR-EDGE-005 | US-GRD-001 | **Bloqueada**: Time Machine F-001-03 | Must | Permitir nunca significa perder trabajo |

### Orden y paralelismo (DAG)

```
US-GRP-001, US-GRP-012 ─► GRD-001 ─┬─► GRD-002 ─► GRD-003
                                   ├─► GRD-004 (+ US-GRP-006)
                                   └─► GRD-005 (+ US-GRP-009) ─► GRD-006 (+ US-GRP-007)

[P8 ─► US-GRP-013]
GRD-001, GRD-004, US-GRP-013 ─► GRD-007 ─┬─► GRD-008 ─┬─► GRD-009
                                         │            └─► GRD-010 (+ 007) ─┬─► GRD-011
                                         │                                 ├─► GRD-012 (+ 008)
                                         │                                 └─► GRD-013 (+ 014)
                                         ├─► GRD-014
                                         ├─► GRD-015 (+ 004, 005) [Cockpit]
                                         └─► GRD-016 (+ 001, 004) [F-001-05]
[F-001-03] GRD-001 ─► GRD-017
```

- **Esqueleto andante**: US-GRD-001.
- **Ola 1** (en paralelo): US-GRD-002, US-GRD-004 y US-GRD-005.
- **Ola 2**: US-GRD-003 (tras 002) y US-GRD-006 (tras 005).
- **Tras el ADR P8 y US-GRP-013 (motor-local)**: US-GRD-007; luego US-GRD-008 y US-GRD-014 en paralelo; luego US-GRD-009 y US-GRD-010; por último US-GRD-011, US-GRD-012 y US-GRD-013.
- **Tras otras features**: US-GRD-015 (Cockpit, además de P8), US-GRD-016 (MCP, además de P8) y US-GRD-017 (Time Machine).

> **Secuencias por contrato compartido**: 001 → 002 → 003 (instalación de hooks), 001 → 007 (contrato de decisión), 004 → 007 (lista de operaciones interceptables) y 004 → 016 (estados de protección). Van en serie. En cada caso el contrato lo fija la Dev Spec de la historia que va primero. La lectura de los tres niveles la fija US-GRP-013 (motor-local).
>
> **Ruta crítica ejecutable hoy**: US-GRP-001 → GRD-001 → GRD-005 → GRD-006. **Ruta crítica total**: P8 → US-GRP-013 → GRD-007 → GRD-008 → GRD-010 → GRD-013.

### Relación con US-GRP-016 (motor-local)

US-GRD-014 **no** desbloquea US-GRP-016. Según el contexto aprobado, US-GRP-016 depende solo del valor de rama base del nivel de equipo y de la regla de lectura, que ya define el requerimiento (BR-CONS-003), y del ADR P8 (motor-local). Las dos historias son independientes.

**Prueba de integración posterior** (cuando estén integradas US-GRD-014 y US-GRP-016): con la rama base "develop" en la configuración del equipo de "demo", la rama base efectiva que protege Guardrails y la que usa el motor para el ahead/behind son las dos "develop". No es un criterio de ninguna de las dos historias.

---

## Preguntas abiertas

| # | Pregunta | Recomendación del PO | Historias afectadas | Estado |
|---|----------|----------------------|---------------------|--------|
| P-GRD-17 | Un agente puede relajar sus reglas editando la configuración del equipo en su worktree sin hacer commit, y no está definido qué copia rige si hay varios worktrees. ¿Qué versión de la configuración del equipo es la efectiva? | La configuración del equipo efectiva es la última versión commiteada en el worktree donde ocurre la operación, nunca las ediciones sin commitear. Junto con Q-GRD-7, que impide a los agentes commitear cambios en la configuración, un agente no puede relajarla. | US-GRD-007, US-GRD-011, US-GRD-012, US-GRD-013 (escenarios marcados "Depende de P-GRD-17") | Abierta (Rene Bonilla). Se llevará al requerimiento cuando se decida |

---

## Cobertura de reglas (regla → historias)

| Regla | Historias | Regla | Historias |
|-------|-----------|-------|-----------|
| BR-VAL-001 | US-GRD-007, 010, 013 | BR-CONS-001 | US-GRD-010, 013 |
| BR-VAL-002 | US-GRD-007, 016 | BR-CONS-002 | US-GRD-016 |
| BR-VAL-003 | US-GRD-008, 009 | BR-CONS-003 | US-GRD-014 |
| BR-CALC-001 | US-GRD-001, 007, 008 | BR-CONS-004 | US-GRD-005, 006 |
| BR-WF-001 | US-GRD-015 | BR-CONS-005 | US-GRD-002, 003 |
| BR-WF-002 | US-GRD-001, 003, 004, 016 | BR-CONS-006 | US-GRD-013 |
| BR-AUTH-001 | US-GRD-006, 012, 013, 015 | BR-TIME-001 | US-GRD-015 |
| BR-AUTH-002 | US-GRD-001, 002 | BR-TIME-002 | US-GRD-005 |
| BR-AUTH-003 | US-GRD-006 | BR-EDGE-001 | US-GRD-001, 004, 007, 014 |
| BR-AUTH-004 | US-GRD-012, 016 | BR-EDGE-002 | US-GRD-002 |
| | | BR-EDGE-003 | US-GRD-004 |
| | | BR-EDGE-004 | US-GRD-011 |
| | | BR-EDGE-005 | US-GRD-017 |

**Resultado**: las 23 reglas tienen al menos una historia. Solo la cubren historias bloqueadas: BR-VAL-001, BR-VAL-002, BR-VAL-003, BR-CONS-001, BR-CONS-003, BR-CONS-006, BR-EDGE-004 y BR-AUTH-004 (P8); BR-WF-001 y BR-TIME-001 (Cockpit y P8); BR-CONS-002 (MCP y P8); BR-EDGE-005 (Time Machine). El catálogo completo de BR-VAL-002, incluido `reset --hard`, se verifica por la capa MCP en US-GRD-016; por Git directo, US-GRD-007 cubre las operaciones que la lista de US-GRD-004 declara interceptables.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-04 | PO (AADD) para Rene Bonilla | Versión inicial en modo Bulk (modelo plano): 17 historias, mapa para la flota, DAG y cobertura de las 23 reglas. 11 historias bloqueadas (P8 de motor-local, Cockpit, MCP y Time Machine) |
| 1.1 | 2026-10-04 | PO (AADD) para Rene Bonilla | Artifact Judge (RESERVAS): US-GRD-014 deja de "desbloquear" US-GRP-016 y pierde el escenario de coherencia con el motor, que pasa a prueba de integración posterior; la lectura de los tres niveles tiene un solo dueño, US-GRP-013 (motor-local), del que depende US-GRD-007; catálogo completo de BR-VAL-002 por MCP en US-GRD-016 (incluido `reset --hard`) y por Git directo en US-GRD-007 (operaciones interceptables, con dependencia de US-GRD-004); US-GRD-015 indica la capa en cada escenario (push con Git directo); US-GRD-004 añade la visibilidad del mínimo seguro (BR-EDGE-001); notas sobre el snapshot previo y la posible división de US-GRD-001; nueva pregunta abierta P-GRD-17 con los escenarios afectados marcados en US-GRD-007, 011, 012 y 013 |
