---
id: TD-GRD-001
title: "Dispatcher plantilla 3: evaluar el pre-push de toda ref empujada (tags, notes y demás refs no gobernadas)"
type: td
status: draft
feature: guardrails
domain: GRP
priority: high
complexity: high
created: 2026-10-08
updated: 2026-10-08
related:
  adrs: [ADR-GRD-001, ADR-GRD-002]
  stories: [US-GRD-008]
  specs: [DS-US-GRD-008, DS-US-GRD-018]
ado:
  id: null
  url: null
tags: [guardrails, deuda-tecnica, dispatcher, plantilla-3, pre-push, rutas-prohibidas, policy-reach, nfr-01]
---

## TD-GRD-001: Dispatcher plantilla 3: evaluar el pre-push de toda ref empujada (tags, notes y demás refs no gobernadas)

**Valor**: una ruta prohibida no se cuela por un push a una ref que Guardrails no gobierna, y el residuo `policy-reach` de la política de rutas prohibidas desaparece de la lista pública.

### Descripción

**Como** responsable de Guardrails
**Quiero** que el `pre-push` evalúe todas las refs que se empujan, también las no gobernadas (tags, notes y otras refs)
**Para** cerrar el residuo declarado de DS-US-GRD-008: `git push origin <commit>:refs/tags/x` sube un commit que toca una ruta prohibida (política `forbiddenPaths`) sin evaluación

> Brief de implementación: [td-grd-001-dispatcher-template-3.md](../../../../dev-briefs/td-grd-001-dispatcher-template-3.md) (hace de Dev Spec)

**Origen.** Nota bloqueante del Arquitecto en la validación de DS-US-GRD-008 (2026-10-08, B1). El coordinador la resolvió como **residuo declarado** en `policy-reach`, solo para ese PR, porque el cierre cambia la instalación y queda fuera de la rama de US-GRD-008. Esta ficha es el pendiente con dueño que el Arquitecto exigió para aceptarlo.

**Hoy.** La vía rápida del dispatcher nativo (`apps/cli/src/bin/raptor-hook.rs`, Enmienda 2026-10-05 de ADR-GRD-001) de `pre-push` sale sin llamar al cliente cuando todas las refs remotas son no gobernadas (`skippable_push` y `is_governed`, en `crates/policy/src/guard/fastpath.rs`), y el cliente (`push()`, en `crates/core/src/guardrails/hook.rs`) descarta esas líneas. Un push solo a tags u otras refs no gobernadas no se evalúa nunca.

**Por qué no es un cambio local.** La vía rápida vive en el dispatcher instalado en cada repo. Cambiarla exige una plantilla 3 del dispatcher (`TEMPLATE_VERSION`, ADR-GRD-001 § 2) y su ruta de actualización en sitio (S8 de DS-US-GRD-018 § 11, transaccional por NFR-12). El cliente además debe saber cuándo enviar las líneas no gobernadas: solo cuando el daemon concede la capacidad `guard.policies`, para que un daemon sin ella no cambie de comportamiento.

### Alcance Técnico

- **Crear** la plantilla 3 del dispatcher de `pre-push`, que ya no sale en la vía rápida cuando todas las refs remotas son no gobernadas y llama al cliente con todas las líneas.
- **Extender** la ruta de actualización en sitio para que una instalación con la plantilla 1 o 2 pase a la 3 con `raptor guard install`, con las mismas garantías transaccionales y de diario que la subida a la plantilla 2.
- **Mantener** la aceptación de las plantillas 1 y 2 por el cliente del hook, de modo que una instalación sin actualizar siga funcionando y la actualización se complete al reinstalar.
- **Enviar** desde el cliente las líneas no gobernadas al daemon solo cuando este conceda la capacidad `guard.policies`; sin ella, mantener el comportamiento actual.
- **Aplicar** la regla de rutas prohibidas a toda ref empujada, y la regla de rama protegida solo a las ramas.
- **Retirar** de la lista pública de residuos de `policy-reach` el push solo a tags u otras refs no gobernadas, y actualizar los límites de lo que el hook no ve.
- **Corregir** el defecto latente de la subida 1→2 que daba una alerta falsa `dispatcher-altered`: la actualización en el sitio no republicaba la referencia de integridad que usa el monitor (M1 del brief).
- **Corregir** la subida interrumpida que se leía como protección inactiva, porque el diario guardaba los hashes nuevos antes de escribir los archivos: ahora la actualización queda como pendiente y la siguiente instalación la completa (M2 del brief).
- **Limpiar** los temporales que deja una escritura matada entre escribir el temporal y hacer el `rename` (`<archivo>.gitraptor.tmp-<hex>`), solo los de archivos listados en el diario y dentro de la carpeta de Guardrails, al actualizar, reparar y desinstalar (M4 del brief).
- **Fuera de alcance**: otros residuos de `policy-reach` (objetos sueltos, `stash`, `update-ref` a mano de refs remotas, cambios en el servidor, `reftable`, `send-pack` directo), nuevas reglas de política y la evaluación de refs no gobernadas para reglas distintas de rutas prohibidas.

### Plan de Verificación

#### Pruebas Automatizadas

- Un push de un commit que toca una ruta prohibida a un tag, a una ref de notes y a otra ref no gobernada queda denegado; el mismo push de un commit limpio pasa.
- El push de una rama protegida sigue evaluándose como antes, y la regla de rama protegida no se aplica a tags ni a notes.
- Con un daemon sin la capacidad `guard.policies`, el push a un tag se comporta como hoy.
- Una instalación con la plantilla 1 o 2 se actualiza a la 3 con `raptor guard install`; una actualización interrumpida en cada paso deja un repo con un dispatcher que funciona y la siguiente reinstalación la completa (suite de interrupción de INF-GRD-001).
- La huella del repo no cambia salvo en los archivos del dispatcher y del diario (suite de huella de INF-GRD-001).
- El presupuesto de latencia del hook se mantiene para un push de pocas refs (NFR-GRD-04).

#### Verificación Manual / Sandbox

- En un repo temporal con la plantilla 2 instalada, reproducir `git push origin <commit>:refs/tags/x` con un commit que toque una ruta prohibida: sube sin evaluar. Reinstalar y repetirlo: queda denegado.
