---
id: TS-CKP-003
title: "Capa cockpit en la decisión de Guardrails, ligada al git del ejecutor y registrada una sola vez"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: high
complexity: medium
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-002, ADR-GRD-002, ADR-GRD-003, ADR-GRD-006, ADR-GRD-007]
  stories: [TS-CKP-002, US-GRD-001, US-GRD-005, US-GRD-016, INF-GRD-001]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, guardrails, capa-cockpit, decision, registro, hooks-git, br-07, dep-ckp-10, f-001-05]
---

## TS-CKP-003: Capa cockpit en la decisión de Guardrails

**Valor**: las operaciones gobernadas del catálogo (merge, rebase, commit, crear y borrar worktree), lanzadas desde la TUI o el MCP, reciben una sola decisión de Guardrails, tomada antes de cualquier efecto, que los hooks del `git` del ejecutor heredan sin volver a preguntar ni duplicar el registro.

### Descripción

**Como** Arquitecto
**Quiero** que el daemon evalúe cada plan del ejecutor con la capa de la petición y ligue esa decisión al `git` que lanza
**Para** que Guardrails gobierne también lo que los hooks no impiden, como el merge y el borrado de un worktree (ADR-GRD-002), con una entrada de registro por operación (DEP-CKP-10, ADR-CKP-002 § 4)

> Dev Spec: `dev-specs/TS-CKP-003-capa-cockpit-guardrails.md` | Pendiente
>
> **Depende de**: US-GRD-001 (función de decisión, cliente del hook y canal autenticado de ADR-GRD-003), US-GRD-005 (registro de ADR-GRD-006), INF-GRD-001 (fixtures de hooks) y TS-CKP-002 (plan, huella, capa fijada por el daemon y registro de hijos con su barrera de arranque). Las enmiendas de ADR-GRD-003 § 1 y § 4 a § 6 y de ADR-GRD-006 § 1 y § 6 están aplicadas (2026-10-04, Cockpit, incluido el endurecimiento H-01, M-03 y DEP-MCP-5). Coordina con US-GRD-016 la capa `mcp`. **ADRs**: ADR-CKP-002 § 3 y § 4, ADR-GRD-003 § 1 y § 4 a § 6, ADR-GRD-006 § 1 y § 6. **Seguridad**: H-01 y M-03 de la revisión de ADR-CKP-002; SEC-03. **Habilita**: las operaciones gobernadas de BR-07 (BR-CKP-AUTH-001, WF-002) y las herramientas de escritura de F-001-05 (DEP-MCP-3, DEP-MCP-5).

### Alcance Técnico

- **Admitir** la capa `cockpit` en el contexto de la decisión, junto a `hooks` y `mcp`, sin cambiar la forma de la función de decisión.
- **Evaluar** cada plan con la operación normalizada, el actor resuelto y la capa que fija el daemon: vista previa al preparar y decisión que cuenta al ejecutar, antes del snapshot.
- **Aplicar** "pedir confirmación" como denegar mientras no exista la cola de confirmación (S-GRD-9, DEP-CKP-8).
- **Consultar** en la evaluación de un hook el registro de hijos del ejecutor (TS-CKP-002): si el `git` más cercano es un hijo registrado con el daemon como padre directo y la transición coincide, devolver la decisión ya tomada.
- **Evaluar** de nuevo, con el actor del plan, cualquier transición no registrada y cualquier `git` nieto lanzado por un hook del usuario (H-01).
- **Reutilizar** la decisión del plan para los hijos adicionales del mismo plan, como el abort del rebase atómico, sin reevaluarlos ni registrar otra entrada (DEP-MCP-5).
- **Omitir** el snapshot previo por hook para los hijos del ejecutor, que ya llevan el previo garantizado.
- **Separar** la evaluación de un hook del cerrojo de escritura del repo, para que un hook bajo el ejecutor nunca espere a su propio padre.
- **Escribir** como mucho una entrada de registro por plan, con la capa y el tipo que corresponden al desenlace, al cerrarse el plan.
- **Devolver** la denegación con su regla y su nivel como código tipado, para que el cliente pinte el PolicyBanner y ofrezca la excepción consciente.
- **Fuera de alcance**: el registro de hijos y la barrera de arranque (TS-CKP-002); la excepción consciente desde el Cockpit (anuncio, ventana cancelable, auditoría y, en el rebase, la ligadura a ref, punta y base con su comprobación posterior), que va en la historia dueña de Q-CKP-15 sobre ADR-GRD-007; la cola de confirmación (US-GRD-015, DEP-CKP-8); la capa `mcp` de las herramientas que no pasan por el ejecutor (US-GRD-016); la presentación.

### Plan de Verificación

#### Pruebas Automatizadas

- **Registro único**: un merge permitido sin regla no deja entrada; un borrado de rama denegado deja una sola denegación con capa `cockpit`; una excepción aplicada deja una sola entrada aunque los hooks pregunten varias veces.
- **Antes de efectos**: con una regla que deniega, el repo no cambia y no se toma snapshot (huella de INF-GRP-001).
- **Lo no impedible**: un merge a una base protegida por regla desde el ejecutor se deniega, aunque ningún hook lo impediría (ADR-GRD-002).
- **Ligadura**: un `git` lanzado por un hook del usuario durante la operación no hereda la decisión y se evalúa con el actor del plan; una transición distinta de la registrada se evalúa de nuevo.
- **Abort del rebase atómico**: un `safe_rebase` que choca deja una sola evaluación y ninguna entrada propia del abort.
- **Sin interbloqueo**: un hook de Guardrails bajo el ejecutor recibe su decisión mientras el cerrojo del repo está tomado, dentro del presupuesto de evaluación de NFR-GRD-04.
- **Capa fijada por el daemon**: un plan pedido por un agente, también desde la TUI, se evalúa con capa `mcp` y deja su entrada con esa capa.
- **Regresión de hooks**: las suites de encadenado de INF-GRD-001 pasan con el ejecutor como padre del `git`.

#### Verificación Manual / Sandbox

- En un repo temporal con Guardrails instalado, lanzar desde un cliente de prueba un merge permitido y un borrado denegado, y revisar el registro de decisiones por el canal.
- Identidad del hijo en Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
