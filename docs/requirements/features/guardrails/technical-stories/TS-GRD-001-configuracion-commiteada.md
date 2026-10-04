---
id: TS-GRD-001
title: "Lectura commiteada de la configuración del equipo y de la rama principal"
type: ts
status: draft
feature: guardrails
domain: GRP
priority: high
complexity: high
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-GRD-004, ADR-GRD-003, ADR-GRD-005]
  stories: [US-GRD-007, US-GRD-011, US-GRD-014, US-GRP-016, US-GRP-013]
  specs: []
ado:
  id: null
  url: null
tags: [guardrails, configuracion, q-grd-17, q-grd-18, rama-base, rama-principal, br-cons-007, br-edge-004]
---

## TS-GRD-001: Lectura commiteada de la configuración del equipo y de la rama principal

**Valor**: Guardrails y el motor leen la misma configuración del equipo, sin que una edición sin commitear la cambie y con una sola rama base por repo.

### Descripción

**Como** Arquitecto
**Quiero** que el cargador único de configuración lea el nivel de equipo de objetos commiteados, con la copia de la rama principal como suelo (la única fuente de relajaciones y de la rama base) y el commit del worktree de la operación solo para endurecer, y que devuelva el estado de cada fuente
**Para** cumplir Q-GRD-17 (refinada por D6), Q-GRD-18 y la decisión 2 con un solo criterio de validez para el motor y Guardrails, que cierra el roce con BR-CONS-007 (motor-local) y el riesgo R-GRD-8

> Dev Spec: `dev-specs/TS-GRD-001-configuracion-commiteada.md` | Pendiente
>
> **Por qué es un enabler y no parte de una historia** (regla del dueño): es una base compartida **entre dos features**, sin una historia dueña única. La usan US-GRP-016 (motor-local, rama base para ahead/behind), US-GRD-007 (permisos con suelo y worktree), US-GRD-014 (rama base protegida) y US-GRD-011 (suelo ilegible). Ninguna de ellas produce por sí sola la lectura que necesitan las demás, y la lectura no tiene un resultado observable propio. Ponerla en la Dev Spec de una sola historia obligaría a la otra feature a esperar a esa historia o a duplicar el cargador, que es justo el riesgo R-GRD-8.
>
> **Desbloqueada** el 2026-10-04: Rene Bonilla aceptó ADR-GRP-007 (motor-local) con sus enmiendas, que era su único bloqueo: PQ-9 (decisión 1 de Rene Bonilla), `permissions`/`policies`, la clave del mínimo, el estado por fuente y la lectura sin objetos de reemplazo (tabla de enmiendas de `docs/architecture/non-functional-guardrails.md`). **Reutiliza** el cargador de tres niveles de US-GRP-013 (motor-local). **ADR**: ADR-GRD-004. **Complejidad alta** por D6: suelo confirmado, rama base confirmada y unión mientras hay un cambio pendiente.

### Alcance Técnico

- **Implementar** en el cargador dos fuentes de nivel de equipo leídas de objetos commiteados, sin tocar el working tree: el suelo (copia de la rama principal) y el commit actual del worktree de la operación.
- **Implementar** la resolución de la rama principal y de su copia conocida, con el orden de la decisión 2 y sin consultar el remoto.
- **Implementar** la combinación de D6: el suelo aporta relajaciones y rama base; el worktree solo endurece y nunca desactiva el mínimo.
- **Guardar** por repo el suelo y la rama base confirmados, y detectar los cambios que relajan o que cambian la rama base, para que la combinación más restrictiva rija hasta la confirmación.
- **Exponer** el estado de cada fuente (ausente, legible, ignorado, parcial o pendiente de confirmación) con diagnósticos sin contenido del archivo.
- **Aplicar** la validación de la entrada commiteada: solo un archivo regular, con límites de tamaño, profundidad, claves y cadenas, y sin objetos de reemplazo.
- **Implementar** una caché acotada de documentos ya leídos, indexada por la identidad del objeto.
- **Exponer** una única función de rama base, que devuelve la **confirmada**. La consumen el motor (ahead/behind) y Guardrails. Una resuelta distinta solo aparece como diagnóstico de cambio pendiente. Sin confirmación inicial del humano, devuelve la resuelta marcada como no confirmada; nunca se confirma sola.
- **Exponer** a Guardrails, aparte, el conjunto de ramas base que debe proteger mientras un cambio está pendiente (la confirmada y la resuelta). El motor no lo usa.
- **Recalcular** el suelo y la rama base ante los cambios de refs que ya publica el observador del motor.
- **Fuera de alcance**: la semántica de permisos y políticas (US-GRD-007 en adelante); la reacción de Guardrails ante un suelo ilegible (US-GRD-011); los comandos reservados de confirmación (US-GRD-007, US-GRD-014; ADR-GRD-007); el comando de edición (US-GRD-013); las enmiendas de ADR-GRP-007, que hace el frente motor-local.

### Plan de Verificación

#### Pruebas Automatizadas

- **Sin commitear**: una edición sin commitear en el worktree no cambia el nivel de equipo leído. Un conflicto sin commitear no lo vuelve ilegible.
- **Por worktree (D6)**: dos worktrees en commits distintos aportan cada uno sus endurecimientos. Una relajación commiteada en la rama de un worktree, un checkout antiguo o una rama huérfana no relajan el suelo.
- **Suelo forjado**: mover en local la copia de la rama principal a un commit con configuración más laxa deja la combinación más restrictiva y el cambio pendiente de confirmación.
- **Cambio de rama base**: un suelo que cambia la rama base deja la confirmada como valor para el motor y para Guardrails. La nueva aparece como diagnóstico pendiente, y Guardrails protege además la nueva hasta la confirmación.
- **Objetos de reemplazo**: un reemplazo del blob de configuración no cambia el documento leído.
- **Rama principal**: con la copia remota marcada como principal se usa su versión. Sin remoto, la rama local. Sin nada, el valor por defecto. Con varios remotos y ninguno principal, la rama local.
- **Commit local sin push**: no cambia la rama base mientras exista la copia de seguimiento.
- **Dos consumidores**: el motor y Guardrails obtienen la misma rama base **confirmada** y el mismo estado de fuente para el mismo objeto, también con un cambio de rama base pendiente. Solo Guardrails recibe además el conjunto ampliado que protege.
- **Entradas hostiles**: un enlace, un submódulo o un objeto de tamaño excesivo en la ruta de configuración dan nivel ignorado y un diagnóstico sin contenido.
- **Sin escrituras ni red**: el arnés de INF-GRP-001 no ve cambios en el repo ni tráfico de red.
- **Presupuesto**: la lectura con caché cabe en el presupuesto de NFR-GRD-04.

#### Verificación Manual / Sandbox

- En un repo real con dos worktrees y una rama principal distinta de `main`, comprobar desde la CLI que la rama base mostrada por el motor y la que protege Guardrails coinciden.
