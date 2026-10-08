---
mode: bulk
generated: 2026-10-04T00:00Z
updated: 2026-10-06
generator: product-owner
total_artifacts: 19
expanded: 19
approved: 0
blocked:
  - US-GRD-016
---

# User Stories — INDEX: Guardrails

> Generado en modo Bulk (modelo plano). Cada historia vive en su archivo de `user-stories/`. Esta tabla solo enlaza y marca el Status.

---

## Contexto del Feature

**Feature**: Guardrails (F-001-04)
**Epic**: E-001 — MVP Fase 1: Cockpit + Time Machine + Guardrails (CLI/TUI + MCP)
**Prioridad**: Alta (BR-11 y BR-12 Must; BR-13 y BR-26 Should)
**Estado**: Listo para Desarrollo: US-GRD-001 a US-GRD-015, US-GRD-017, US-GRD-018 y US-GRD-019 (18 listas; la Dev Spec de US-GRD-013 y la de US-GRD-015 esperan a SPIKE-GRD-002). Bloqueada: US-GRD-016 (1; ver `blocked`). US-GRD-013 y US-GRD-015 se desbloquearon el 2026-10-04 al aceptarse ADR-GRD-008. US-GRD-007 a 012 y 014 se desbloquearon el 2026-10-04 al aceptarse ADR-GRP-007, que cierra P8.

**Enlace a contexto completo**: [`context.md`](./context.md) (CTX-GRD-001)
**Reglas de negocio**: [`business-rules.md`](./business-rules.md) (BR-GRD-001, 24 reglas) · **Diseño**: no aplica (sin superficie propia; la cola y el estado de protección los presenta el Cockpit, F-001-02, y la CLI)

---

## Análisis de División (Sistema de 4 Pasos)

- **Paso 1 — Flujo**: actor principal = desarrollador orquestador. Resultado buscado = que ningún agente destruya trabajo ni reescriba la rama base. Camino mínimo = proteger un repo con permiso → un force-push queda denegado con su motivo.
- **Paso 2 — Puntos de resultado**: cada historia deja una decisión, un estado o una entrada de registro que se comprueba desde fuera, intentando la operación y consultando el resultado.
- **Paso 3 — Patrón aplicado**: camino feliz primero. El esqueleto andante es US-GRD-001: capa de hooks con el mínimo seguro, sin configuración. No necesitaba el ADR P8 (motor-local), ya cerrado por ADR-GRP-007, ni espera al MCP (F-001-05) y ya demuestra el force-push bloqueado del BRD § 13. Después va por capacidad acumulable: instalación recuperable → registro y excepción → reglas configuradas (orden de Q-GRD-9) → precedencia → fail-safe → comando → cola. El corte por capa (MCP, US-GRD-016) se justifica porque es la otra vía del agente y su resultado es observable: la misma decisión.
- **Paso 4 — Filtro**: todas pasan (usuario real · resultado observable · cabe en una rama corta de un agente · comportamiento, no código).

> **Criterio de bloqueo**: una historia está bloqueada si depende de una feature sin historias todavía —Cockpit (F-001-02) o Servidor MCP (F-001-05)— o del gate de Q-GRD-19 (factor de autenticación fuera de banda del SO, D5; su ADR aún no existe). El ADR de formato P8 dejó de ser un bloqueo el 2026-10-04 (ADR-GRP-007 aceptado), igual que US-GRP-013, desbloqueada ese día. Las demás historias aprobadas de motor-local son dependencias de historia, no bloqueos.
>
> **Lectura de la configuración en tres niveles**: tiene un solo dueño, US-GRP-013 (motor-local). Las historias de Guardrails que leen la configuración (desde US-GRD-007) la reutilizan; ninguna la vuelve a definir.
>
> **Transversales (no son historias)**: el mismo comportamiento en Windows, macOS y Linux (BR-03, NFR-06) y nada modificado fuera del repo ni hooks previos perdidos (NFR-01) se exigen en todos los escenarios. Cómo se distingue al humano en las acciones reservadas es transversal (lo define el Arquitecto; R-GRD-3).
>
> **Sin snapshot previo hasta US-GRD-017**: mientras no estén integradas US-TMC-005 (Time Machine) y US-GRD-017, las operaciones destructivas que US-GRD-001 a US-GRD-006 permiten se ejecutan sin snapshot previo. Es un límite conocido de esas historias, no un incumplimiento de NFR-01 por su parte.
>
> **US-GRD-001 se puede partir**: el Arquitecto puede dividirla en "instalar la protección con permiso" y "decidir con el mínimo seguro" si no cabe en una rama corta. Las dos mitades conservan sus escenarios tal como están.

---

## Índice de Historias

| ID | Título | Descripción (1 línea) | Status |
|----|--------|-----------------------|--------|
| [US-GRD-001](./user-stories/US-GRD-001-proteger-repo-force-push-bloqueado.md) | Un agente que intenta hacer force-push en un repo protegido queda bloqueado | Desarrollador quiere proteger un repo con permiso y que el mínimo seguro deniegue force-push y borrar la rama base | implemented (PR #116, #121) |
| [US-GRD-002](./user-stories/US-GRD-002-hooks-previos-respetados.md) | Los hooks que el repo ya tenía siguen funcionando al protegerlo | Desarrollador quiere encadenar con sus hooks previos o no instalar nada si no se puede | implemented (PR #193) |
| [US-GRD-003](./user-stories/US-GRD-003-retirar-proteccion-sin-rastro.md) | El desarrollador retira la protección y el repo queda exactamente como estaba | Desarrollador quiere desinstalar sin rastro y que una instalación interrumpida no quede a medias | partially-implemented (PR #193; falta la instalación huérfana y el registro protection-state) |
| [US-GRD-004](./user-stories/US-GRD-004-aviso-proteccion-inactiva.md) | El desarrollador se entera de que la protección de un repo dejó de estar activa | Desarrollador quiere un aviso si los hooks desaparecen, ver el mínimo seguro y saber qué no se puede impedir | expanded |
| [US-GRD-005](./user-stories/US-GRD-005-registro-de-bloqueos.md) | El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo | Desarrollador quiere un registro por repo, en el perfil, de 90 días, para el KPI | partially-implemented (PR #154 (ajuste en #168)) |
| [US-GRD-006](./user-stories/US-GRD-006-excepcion-consciente.md) | El desarrollador hace a conciencia una operación prohibida y queda constancia | Desarrollador quiere reglas para cualquier actor y una excepción consciente registrada | expanded |
| [US-GRD-007](./user-stories/US-GRD-007-permisos-por-operacion.md) | El equipo decide qué operaciones de Git se permiten, se deniegan o piden confirmación | Desarrollador quiere fijar el permiso de cada operación en la configuración del equipo | expanded |
| [US-GRD-008](./user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md) | Ningún agente cambia una rama protegida ni toca una ruta prohibida | Desarrollador quiere ramas protegidas y rutas prohibidas | expanded |
| [US-GRD-009](./user-stories/US-GRD-009-tamano-diff-formato-commit.md) | Los agentes entregan commits pequeños y con el formato que exige el equipo | Desarrollador quiere límite de diff y formato de commit | expanded |
| [US-GRD-010](./user-stories/US-GRD-010-endurecer-sin-relajar.md) | El desarrollador endurece las reglas en su máquina sin poder relajar las del equipo | Desarrollador quiere la precedencia de Q-GRD-14 entre equipo, perfil y local | expanded |
| [US-GRD-011](./user-stories/US-GRD-011-configuracion-ilegible.md) | Una configuración rota no deja pasar las operaciones peligrosas | Desarrollador quiere que una configuración ilegible mantenga el mínimo seguro | expanded |
| [US-GRD-012](./user-stories/US-GRD-012-agente-no-relaja-configuracion.md) | Un agente no puede relajar las reglas del equipo cambiando su configuración | Desarrollador quiere la configuración como ruta prohibida para agentes | expanded |
| [US-GRD-013](./user-stories/US-GRD-013-comando-edicion-configuracion.md) | El desarrollador cambia la configuración con un comando sin perder lo que editó a mano | Desarrollador quiere un comando seguro que respete niveles y cambios a mano (Dev Spec tras SPIKE-GRD-002 y Q-GRD-32; ADR-GRD-008) | expanded |
| [US-GRD-014](./user-stories/US-GRD-014-rama-base-del-equipo.md) | El equipo fija la rama base del repo y Guardrails la protege | Desarrollador quiere que Guardrails proteja la rama base que fija el equipo | expanded |
| [US-GRD-015](./user-stories/US-GRD-015-cola-de-confirmacion.md) | El desarrollador aprueba o rechaza las acciones de riesgo que un agente deja en espera | Desarrollador quiere la cola de confirmación con caducidad de 5 minutos (Dev Spec tras SPIKE-GRD-002; ADR-GRD-008; el Cockpit la presenta) | expanded |
| [US-GRD-016](./user-stories/US-GRD-016-misma-decision-por-mcp.md) | Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo | Desarrollador quiere la misma decisión por MCP para todo el catálogo y los estados según la allowlist (bloqueada: F-001-05) | expanded |
| [US-GRD-017](./user-stories/US-GRD-017-sin-snapshot-no-se-ejecuta.md) | Una operación destructiva permitida no se ejecuta sin un punto de recuperación | Desarrollador quiere que sin snapshot previo la operación se deniegue | expanded |
| [US-GRD-018](./user-stories/US-GRD-018-autoria-commits-persona-y-agente.md) | Cada commit entra a nombre de la persona y deja constancia del agente que lo hizo, con la exigencia que fija el equipo | Desarrollador quiere la política de autoría por repo: `agents-commit` por defecto, `human-author` o `flexible` (BR-26, D6) | partially-implemented (PR #137, #141, #150 (ajustes en #152 y #153); D12 en el registro con #154) |
| [US-GRD-019](./user-stories/US-GRD-019-quien-ejecuto-y-a-nombre-de-quien.md) | El desarrollador ve quién ejecutó cada commit y a nombre de quién entró cuando no coinciden | Desarrollador quiere ver en `raptor events` agente, persona y worktree, y la pista inferida contrastada con el trailer | implemented (PR #144, #147, #151, #175) |

---

## Mapa para la flota

> Un agente por historia y por worktree (D3). "Depende de" = historias que deben estar integradas antes de empezar. Las historias de motor-local se nombran como US-GRP-NNN.

| ID | Reglas cubiertas | Depende de | Externas | Prioridad | Valor en una línea |
|----|------------------|------------|----------|-----------|--------------------|
| US-GRD-001 | BR-AUTH-002, BR-EDGE-001, BR-CALC-001, BR-WF-002, BR-CONS-003 (`main`), BR-AUTH-001 | US-GRP-001, US-GRP-012 | — | Must | La demo del force-push bloqueado funciona desde el primer día |
| US-GRD-002 | BR-EDGE-002, BR-CONS-005, BR-AUTH-002 | US-GRD-001 | — | Must | Proteger un repo no rompe lo que ya tenía |
| US-GRD-003 | BR-CONS-005 (incluidas las huérfanas), BR-WF-002, BR-AUTH-001 | US-GRD-001, US-GRD-002, US-GRD-004 | — | Must | Probar Guardrails es reversible del todo |
| US-GRD-004 | BR-WF-002, BR-EDGE-003, BR-EDGE-001 (visible) | US-GRD-001, US-GRP-006 | — | Should | El estado de protección nunca miente y dice qué reglas aplican |
| US-GRD-005 | BR-CONS-004, BR-TIME-002 | US-GRD-001, US-GRP-009 | — | Must | El KPI "acciones peligrosas bloqueadas" se mide |
| US-GRD-006 | BR-AUTH-003, BR-AUTH-001, BR-CONS-004 | US-GRD-001, US-GRD-005, US-GRP-007, US-GRP-009 | Arquitecto: distinguir al humano (R-GRD-3) | Must | Fail-safe sin dejar atrapado al humano |
| US-GRD-007 | BR-VAL-002, BR-CALC-001, BR-VAL-001, BR-EDGE-001 | US-GRD-001, US-GRD-004, US-GRP-013, TS-GRD-001 | — (P8 cerrada por ADR-GRP-007, 2026-10-04; Q-GRD-17 resuelta) | Must | Las reglas del repo viajan con él |
| US-GRD-008 | BR-VAL-003 (ramas protegidas, rutas prohibidas), BR-CALC-001 | US-GRD-007 | — | Must | Que nadie rompa main |
| US-GRD-009 | BR-VAL-003 (tamaño de diff, formato de commit) | US-GRD-008 | — | Should | Diffs revisables y un historial legible |
| US-GRD-010 | BR-CONS-001, BR-VAL-001 | US-GRD-007, US-GRD-008 | — | Must | Las reglas del equipo son un suelo |
| US-GRD-011 | BR-EDGE-004 | US-GRD-010, TS-GRD-001 | — (Q-GRD-17 resuelta; la alineación con BR-CONS-007 la cierran ADR-GRP-007 y TS-GRD-001) | Must | Nunca "todo permitido" |
| US-GRD-012 | BR-AUTH-004, BR-AUTH-001 | US-GRD-008, US-GRD-010 | — (Q-GRD-17 resuelta; R-GRD-3 en ADR-GRD-007) | Must | Las reglas no las cambia quien está sujeto a ellas |
| US-GRD-013 | BR-CONS-006, BR-VAL-001, BR-CONS-001, BR-AUTH-001 | US-GRD-010, US-GRD-014 | Factor del SO de ADR-GRD-008 (aceptado); Dev Spec tras SPIKE-GRD-002 y el trinquete de Q-GRD-32 | Should | Editar la configuración es seguro |
| US-GRD-014 | BR-CONS-003, BR-EDGE-001 | US-GRD-007, TS-GRD-001 | — | Should | La rama de integración del equipo queda protegida |
| US-GRD-015 | BR-WF-001, BR-TIME-001, BR-AUTH-001 | US-GRD-004, US-GRD-005, US-GRD-007 | Factor del SO de ADR-GRD-008 (aceptado); Dev Spec tras SPIKE-GRD-002. El Cockpit la consume (DEP-CKP-8), no la bloquea | Should | El humano tiene la última palabra en lo arriesgado |
| US-GRD-016 | BR-CONS-002, BR-WF-002, BR-AUTH-004 | US-GRD-001, US-GRD-004, US-GRD-007 | **Bloqueada**: Servidor MCP F-001-05 | Must | Dos capas, una sola decisión |
| US-GRD-017 | BR-EDGE-005 | US-GRD-001, US-TMC-005 | — (Time Machine con historias; contrato en ADR-GRD-003) | Must | Permitir nunca significa perder trabajo |
| US-GRD-018 | BR-AUTH-005, BR-CONS-001, BR-CONS-004, BR-CALC-001 | US-GRD-005, US-GRD-007, US-GRD-009, US-GRD-010, US-GRP-007, US-GRP-009 | Arquitecto: saber en el momento del commit qué proceso lo ejecuta (carrera S3 de SPIKE-GRP-001) | Should | El historial dice quién responde y qué agente participó |
| US-GRD-019 | BR-AUTH-005 (presentación, pista `inferred`) | US-GRD-018, US-GRD-005, US-GRP-002, US-GRP-007, US-GRP-009 | La presentación en el Cockpit va en una historia de Cockpit, todavía sin escribir | Should | Ver qué agente hay detrás de un commit firmado por una persona |

### Orden y paralelismo (DAG)

```
US-GRP-001, US-GRP-012 ─► GRD-001 ─┬─► GRD-002 ─► GRD-003 (+ 004)
                                   ├─► GRD-004 (+ US-GRP-006)
                                   └─► GRD-005 (+ US-GRP-009) ─► GRD-006 (+ US-GRP-007)

[US-GRP-007 ─► US-GRP-013 ─► TS-GRD-001]   (P8 cerrada por ADR-GRP-007, 2026-10-04)
GRD-001, GRD-004, TS-GRD-001 ─► GRD-007 ─┬─► GRD-008 ─┬─► GRD-009
                                         │            └─► GRD-010 (+ 007) ─┬─► GRD-011
                                         │                                 ├─► GRD-012 (+ 008)
                                         │                                 └─► GRD-013 (+ 014) [Q-GRD-19]
                                         ├─► GRD-014
                                         ├─► GRD-015 (+ 004, 005) [Cockpit, Q-GRD-19]
                                         └─► GRD-016 (+ 001, 004) [F-001-05]
GRD-001, US-TMC-005 (Time Machine) ─► GRD-017
GRD-005, GRD-009, GRD-010, US-GRP-007, US-GRP-009 ─► GRD-018 ─► GRD-019 (+ US-GRP-002)
```

- **Esqueleto andante**: US-GRD-001.
- **Ola 1** (en paralelo): US-GRD-002, US-GRD-004 y US-GRD-005.
- **Ola 2**: US-GRD-003 (tras 002 y 004; de US-GRD-004, una Should, solo depende la parte de instalaciones huérfanas: el resto de US-GRD-003 se puede entregar sin ella) y US-GRD-006 (tras 005).
- **Tras US-GRP-013 (motor-local) y TS-GRD-001** (desbloqueadas el 2026-10-04 al aceptarse ADR-GRP-007, que cierra P8): US-GRD-007; luego US-GRD-008 y US-GRD-014 en paralelo; luego US-GRD-009 y US-GRD-010; por último US-GRD-011 y US-GRD-012.
- **Tras SPIKE-GRD-002** (desbloqueadas el 2026-10-04 al aceptarse ADR-GRD-008): US-GRD-013 (además tras 010 y 014, y con el trinquete de Q-GRD-32) y US-GRD-015 (tras 004, 005 y 007).
- **Sigue bloqueada**: US-GRD-016 (Servidor MCP F-001-05).
- **Política de autoría (BR-26, fuera de M1)**: US-GRD-018 tras 005, 009 y 010 (y US-GRP-007 y 009 de motor-local); después US-GRD-019 (además tras US-GRP-002).
- **Tras US-TMC-005 (Time Machine)**: US-GRD-017 (desbloqueada el 2026-10-04; antes se bloqueaba en cruz con US-TMC-005).

> **Secuencias por contrato compartido**: 001 → 002 → 003 (instalación de hooks), 004 → 003 (detección de la instalación huérfana), 001 → 007 (contrato de decisión), 004 → 007 (lista de operaciones interceptables) y 004 → 016 (estados de protección). Van en serie. En cada caso el contrato lo fija la Dev Spec de la historia que va primero. La lectura de los tres niveles la fija US-GRP-013 (motor-local).
>
> **Ruta crítica de la protección base**: US-GRP-001 → GRD-001 → GRD-005 → GRD-006. **Ruta crítica de las reglas configuradas** (ejecutable desde el 2026-10-04, tras cerrarse P8): US-GRP-013 → TS-GRD-001 → GRD-007 → GRD-008 → GRD-010 → GRD-011 / GRD-012. **Ruta crítica total**: la anterior → GRD-013, tras SPIKE-GRD-002 (factor de ADR-GRD-008, Q-GRD-19).

### Relación con US-GRP-016 (motor-local)

US-GRD-014 **no** desbloquea US-GRP-016. Según el contexto aprobado, US-GRP-016 depende solo del valor de rama base del nivel de equipo y de la regla de lectura, que ya define el requerimiento (BR-CONS-003), y del ADR P8 (motor-local), ya cerrado por ADR-GRP-007 (aceptado el 2026-10-04). Desde ese día US-GRP-016 está desbloqueada y depende de US-GRP-013 y TS-GRD-001, no de US-GRD-014. Las dos historias son independientes.

**Prueba de integración posterior** (cuando estén integradas US-GRD-014 y US-GRP-016): con la rama base "develop" en la configuración del equipo de "demo" commiteada en su rama principal, y dos worktrees en commits con versiones distintas de esa configuración, la rama base efectiva que protege Guardrails y la que usa el motor para el ahead/behind son "develop" en los dos worktrees (Q-GRD-18). Además, con un cambio de rama base pendiente de confirmar, los dos usan la misma rama base **confirmada**; la protección adicional de la rama pendiente solo aparece en las decisiones de Guardrails (Q-GRD-21). No es un criterio de ninguna de las dos historias.

---

## Preguntas abiertas (todas resueltas)

| # | Pregunta | Recomendación del PO | Historias afectadas | Estado |
|---|----------|----------------------|---------------------|--------|
| P-GRD-17 | Un agente puede relajar sus reglas editando la configuración del equipo en su worktree sin hacer commit, y no está definido qué copia rige si hay varios worktrees. ¿Qué versión de la configuración del equipo es la efectiva? | La configuración del equipo efectiva es la última versión commiteada en el worktree donde ocurre la operación, nunca las ediciones sin commitear. Junto con Q-GRD-7, que impide a los agentes commitear cambios en la configuración, un agente no puede relajarla. | US-GRD-007, US-GRD-011, US-GRD-012, US-GRD-013 | **Resuelta (Q-GRD-17)**, Rene Bonilla, 2026-10-04. Registrada en el requerimiento (context.md y business-rules.md) |

---

## Cobertura de reglas (regla → historias)

| Regla | Historias | Regla | Historias |
|-------|-----------|-------|-----------|
| BR-VAL-001 | US-GRD-007, 010, 012, 013 | BR-CONS-001 | US-GRD-010, 013, 018 |
| BR-VAL-002 | US-GRD-007, 016 | BR-CONS-002 | US-GRD-016 |
| BR-VAL-003 | US-GRD-008, 009 | BR-CONS-003 | US-GRD-001, 014 |
| BR-CALC-001 | US-GRD-001, 007, 008, 018 | BR-CONS-004 | US-GRD-005, 006, 018 |
| BR-WF-001 | US-GRD-015 | BR-CONS-005 | US-GRD-002, 003 |
| BR-WF-002 | US-GRD-001, 003, 004, 007, 014, 016 | BR-CONS-006 | US-GRD-013 |
| BR-AUTH-001 | US-GRD-001, 003, 006, 007, 012, 013, 014, 015 | BR-TIME-001 | US-GRD-015 |
| BR-AUTH-002 | US-GRD-001, 002 | BR-TIME-002 | US-GRD-005 |
| BR-AUTH-003 | US-GRD-006 | BR-EDGE-001 | US-GRD-001, 004, 007, 014 |
| BR-AUTH-004 | US-GRD-012, 016 | BR-EDGE-002 | US-GRD-002 |
| BR-AUTH-005 | US-GRD-018, 019 | | |
| | | BR-EDGE-003 | US-GRD-004 |
| | | BR-EDGE-004 | US-GRD-011 |
| | | BR-EDGE-005 | US-GRD-017 |

**Resultado**: las 24 reglas tienen al menos una historia. Solo la cubre una historia bloqueada: BR-CONS-002 (US-GRD-016, MCP). BR-CONS-006 (US-GRD-013), BR-WF-001 y BR-TIME-001 (US-GRD-015) dependen de historias cuya Dev Spec espera a SPIKE-GRD-002. Desde el 2026-10-04 (ADR-GRP-007 aceptado) ya no esperan a P8 BR-VAL-001, BR-VAL-002, BR-VAL-003, BR-CONS-001, BR-CONS-003, BR-EDGE-004 ni BR-AUTH-004. El catálogo completo de BR-VAL-002, incluido `reset --hard`, se verifica por la capa MCP en US-GRD-016; por Git directo, US-GRD-007 cubre las operaciones que la lista de US-GRD-004 declara interceptables.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-04 | PO (AADD) para Rene Bonilla | Versión inicial en modo Bulk (modelo plano): 17 historias, mapa para la flota, DAG y cobertura de las 23 reglas. 11 historias bloqueadas (P8 de motor-local, Cockpit, MCP y Time Machine) |
| 1.1 | 2026-10-04 | PO (AADD) para Rene Bonilla | Artifact Judge (RESERVAS): US-GRD-014 deja de "desbloquear" US-GRP-016 y pierde el escenario de coherencia con el motor, que pasa a prueba de integración posterior; la lectura de los tres niveles tiene un solo dueño, US-GRP-013 (motor-local), del que depende US-GRD-007; catálogo completo de BR-VAL-002 por MCP en US-GRD-016 (incluido `reset --hard`) y por Git directo en US-GRD-007 (operaciones interceptables, con dependencia de US-GRD-004); US-GRD-015 indica la capa en cada escenario (push con Git directo); US-GRD-004 añade la visibilidad del mínimo seguro (BR-EDGE-001); notas sobre el snapshot previo y la posible división de US-GRD-001; nueva pregunta abierta P-GRD-17 con los escenarios afectados marcados en US-GRD-007, 011, 012 y 013 |
| 1.2 | 2026-10-04 | PO (AADD) para Rene Bonilla | P-GRD-17 resuelta por Q-GRD-17 (rige la última versión commiteada de la configuración del equipo en el worktree de la operación): US-GRD-007 y 013 aplican el cambio al commitearlo; US-GRD-011 reformula los escenarios de conflicto de merge (commiteado frente a sin commitear); US-GRD-012 deja explícito que editar sin commitear no relaja nada. Sin marcas "Depende de P-GRD-17" |
| 1.3 | 2026-10-04 | PO (AADD) para Rene Bonilla | Q-GRD-18 (la rama base se lee de la configuración del equipo commiteada en la rama principal del repo): US-GRD-014 añade el escenario de dos worktrees con versiones distintas de la configuración que comparten la rama base de la rama principal; la prueba de integración con US-GRP-016 se ajusta a dos worktrees |
| 1.4 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisiones Q-GRD-19 a Q-GRD-22 (D5 a D8 de la revisión de arquitectura y seguridad) y KPI verificado, sin historias nuevas. US-GRD-004: el mínimo seguro solo lo desactiva la configuración del equipo en la rama principal, con la confirmación del desarrollador (Q-GRD-21). US-GRD-005: nuevo escenario, las entradas anotadas en modo degradado se muestran aparte y no entran en el recuento por defecto (6 escenarios). US-GRD-007: desactivar el mínimo exige la confirmación del desarrollador y nuevo escenario de relajación pendiente (Q-GRD-21, Q-GRD-22; 6 escenarios); el escenario de Q-GRD-17 pasa a hablar de endurecimiento. US-GRD-012: nuevo escenario, un commit laxo solo en el worktree de un agente no relaja nada (Q-GRD-20; 6 escenarios). US-GRD-014: protección antes de la confirmación inicial, confirmación al añadir el repo y nuevo escenario de cambio de rama base pendiente (Q-GRD-20 a Q-GRD-22; 6 escenarios). US-GRD-013 y US-GRD-015: gate del factor de autenticación del sistema operativo en Dependencias (Q-GRD-19). Prueba de integración con US-GRP-016: misma rama base confirmada con un cambio pendiente |
| 1.5 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisiones Q-GRD-23 a Q-GRD-27 (D9 a D12 y KPI), sin historias nuevas. US-GRD-001: al instalar se confirma la rama base "main" (Q-GRD-23). US-GRD-003: nuevo escenario de instalación huérfana, retirarla o adoptarla (6 escenarios). US-GRD-004: el escenario del mínimo muestra también los diagnósticos pendientes de confirmar con su acción (Q-GRD-25); la mención de huérfanas en sus Requisitos Técnicos apunta a US-GRD-003. US-GRD-006: la excepción se anuncia y abre una ventana cancelable, más un escenario de cancelación (Q-GRD-24; 5 escenarios). US-GRD-007: la confirmación inicial de un mínimo desactivado se hace al instalar, con anuncio y ventana (Q-GRD-23); la relajación pendiente se ve como diagnóstico (Q-GRD-25). US-GRD-011: nuevo escenario de errata que fuerza el mínimo (Q-GRD-26; 6 escenarios). US-GRD-014: confirmación al instalar en lugar de al añadir el repo (Q-GRD-23) y diagnóstico de rama base pendiente (Q-GRD-25) |
| 1.6 | 2026-10-04 | PO (AADD) para Rene Bonilla | Aplicación de decisiones existentes, sin IDs nuevos. US-GRD-011: el escenario de marcas de conflicto en el worktree cita Q-GRD-12 (cualquier versión ilegible fuerza el mínimo). US-GRD-014: el primer escenario pasa a perfil perdido o protección adoptada, con la rama base no confirmada y la unión protegida (Q-GRD-21, Q-GRD-23). US-GRD-001: alcance acotado a repos sin configuración del equipo, sin escenario nuevo |
| 1.7 | 2026-10-04 | PO (AADD) para Rene Bonilla | Artifact Judge (FAIL). US-GRD-007 y US-GRD-014: instalar no confirma una configuración del equipo existente; se instala y después se confirma de forma explícita (Q-GRD-23). US-GRD-003: el desinstalar se anuncia y abre una ventana cancelable (Q-GRD-19); reglas cubiertas fusionadas y BR-AUTH-001; la huérfana pasa a esquema retirar/adoptar, con la rama base no confirmada al adoptar; depende de US-GRD-004 (detección). US-GRD-001: BR-AUTH-001 en reglas cubiertas y alcance en párrafo propio. Matriz, mapa y DAG actualizados (BR-AUTH-001 con 001 y 003; 004 → 003). US-GRP-016 (motor-local) no depende de US-GRD-001 ni de US-GRD-014: la coherencia se comprueba en la prueba de integración posterior |
| 1.8 | 2026-10-04 | Agente de documentación para Rene Bonilla | Aceptación de ADR-GRP-005 a 013 y ADR-GRD-001 a 007 (Rene Bonilla, 2026-10-04). ADR-GRP-007 cierra P8: se desbloquean US-GRD-007, 008, 009, 010, 011, 012 y 014 (y TS-GRD-001 en el índice de enablers). Siguen bloqueadas US-GRD-013 (Q-GRD-19), US-GRD-015 (Cockpit y Q-GRD-19), US-GRD-016 (MCP) y US-GRD-017 (Time Machine), con el motivo actualizado. 13 listas y 4 bloqueadas; DAG, olas, rutas críticas y cobertura actualizados |
| 1.9 | 2026-10-04 | Agente de documentación para Rene Bonilla | Decisión de Rene Bonilla, 2026-10-04: US-GRD-017 se desbloquea y depende de US-GRD-001 y US-TMC-005 (Time Machine), que también se desbloquea; se rompe el bloqueo cruzado entre las dos. El criterio de bloqueo ya no cita a la Time Machine, que tiene historias. 14 listas y 3 bloqueadas |
| 1.10 | 2026-10-04 | Orquestador para Rene Bonilla | Aceptación de ADR-GRD-008 (decisión del orquestador validada por Arquitecto y PO). US-GRD-013 y US-GRD-015 se desbloquean; su Dev Spec espera a SPIKE-GRD-002, y la de US-GRD-013 al trinquete de Q-GRD-32. US-GRD-015 ya no depende del Cockpit, que la consume (DEP-CKP-8). Pendiente del PO: historia de la relajación personal pendiente (Q-GRD-32) y de la adopción del factor por D8, desinstalar y excepción (OQ-GRD-008-3) |
| 1.11 | 2026-10-06 | PO (AADD); decisión del orquestador (2026-10-06), validada por el PO | D6 del BRD (decisión de Rene Bonilla, 2026-10-06; Q-GRD-34): BR-26 y nueva regla BR-AUTH-005. Dos historias nuevas, las dos en el MVP y fuera de M1: **US-GRD-018** (política de autoría: `agents-commit` por defecto, `human-author` con bloquear o avisar, `flexible`; 5 escenarios) y **US-GRD-019** (quién ejecutó frente a a nombre de quién entra en `raptor events`, y la pista `inferred` contrastada con el trailer y oculta con `human-author`; 5 escenarios). 19 historias, 18 listas y 1 bloqueada; mapa, DAG y cobertura de las 24 reglas actualizados |
