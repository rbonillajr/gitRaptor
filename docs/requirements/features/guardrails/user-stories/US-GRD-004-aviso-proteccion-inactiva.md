---
id: US-GRD-004
title: "El desarrollador se entera de que la protección de un repo dejó de estar activa"
type: us
status: partially-implemented
priority: medium
created: 2026-10-04
updated: 2026-10-08
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-001
    - US-GRP-006
tags:
  - guardrails
  - estado-proteccion
  - hooks-git
  - limites
---

# US-GRD-004: El desarrollador se entera de que la protección de un repo dejó de estar activa

## Descripción

**Como** desarrollador orquestador, **quiero** saber cuándo la protección de hooks de un repo deja de funcionar por una causa ajena a Guardrails y qué operaciones no puede impedir, **para** no creerme protegido cuando no lo estoy.

**Valor**: el estado de protección nunca miente (BR-WF-002, Q-GRD-8).

## Reglas cubiertas

BR-WF-002 (hooks inactivos por otra causa → aviso; repo retirado de la observación, Q-GRD-15) · BR-EDGE-003 (lista publicada de operaciones que la capa de hooks no puede impedir, por formato de refs, Q-GRD-28; y de lo que se deniega de más, Q-GRD-31) · BR-WF-002 (diagnósticos visibles de lo pendiente de confirmar, Q-GRD-25) · BR-EDGE-001 (el mínimo seguro es visible para el desarrollador; solo lo desactiva la configuración del equipo en la rama principal, con su confirmación, Q-GRD-21) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (estado "Solo hooks"); US-GRP-006 (motor-local: retirar un repo de la observación).
- **Externas**: ninguna. Los estados que dependen de la allowlist del MCP están en US-GRD-016.
- **Transversal**: Windows, macOS y Linux. La presentación del aviso es del Cockpit y la CLI.

## Criterios de Aceptación

**Escenario: Otro gestor reemplaza la protección**

Dado el repo "demo" en estado "Solo hooks"
Cuando otra herramienta reemplaza los hooks de Guardrails en "demo"
Entonces el estado de protección de "demo" pasa a "Sin protección"
  Y GitRaptor avisa al desarrollador de que la protección dejó de estar activa
  Y el cambio de estado queda en el registro

**Escenario: Retirar la protección desde Guardrails no genera aviso**

Dado el repo "demo" en estado "Solo hooks"
Cuando el desarrollador retira la protección desde Guardrails
Entonces el estado pasa a "Sin protección" sin aviso de pérdida

**Escenario: Retirar el repo de la observación no desactiva sus hooks**

Dado el repo "demo" en estado "Solo hooks"
Cuando el desarrollador retira "demo" de la observación del motor
Entonces los hooks de Guardrails siguen denegando el force-push en "demo", con actor "sin atribuir"
  Y GitRaptor avisa de que "demo" sigue protegido aunque ya no se observa

**Escenario: El desarrollador ve qué reglas aplican y qué espera su confirmación**

Dado el repo "demo" en estado "Solo hooks" y sin configuración de Guardrails
Cuando el desarrollador consulta el estado de protección de "demo"
Entonces obtiene que aplica el conjunto mínimo por defecto: force-push denegado y borrado de la rama base denegado
  Y que solo lo desactiva la configuración del equipo en la rama principal, una vez que el desarrollador confirma ese cambio en su máquina
  Y obtiene los diagnósticos pendientes de confirmar (relajación pendiente, rama base no confirmada o pendiente), cada uno con la acción para confirmarlo, o que no hay ninguno

**Escenario: El desarrollador consulta qué no se puede impedir**

Dado el repo "demo" en estado "Solo hooks"
Cuando el desarrollador consulta el estado de protección de "demo"
Entonces obtiene la lista de operaciones del catálogo que la capa de hooks no puede impedir con Git directo
  Y la lista coincide con lo que se observa al probar esas operaciones con Git directo

**Escenario: En un repo reftable, la lista incluye renombrar la rama base**

Dado el repo "demo" en estado "Solo hooks", con sus ramas guardadas en formato reftable
Cuando el desarrollador consulta qué no se puede impedir en "demo"
Entonces la lista incluye renombrar la rama base y renombrar otra rama sobre ella
  Y en un repo sin ese formato esas operaciones no aparecen en la lista

## Requisitos Técnicos

- **Gobierno**: ADR-GRD-005 § 1 (la capa está activa solo si se cumplen H1 a H4 contra el diario del perfil), § 3 (estado expuesto), § 4 (detección de pérdida) y § 5 (transición esperada, registro y aviso con rebote); ADR-GRD-002 § 2 y § 3 (lista publicada, versionada con el catálogo y expuesta como códigos; el cliente pone el texto).
- **Mínimo visible**: en esta historia `minimumSet` es siempre `active`. El valor `disabled-by-team` solo llega con TS-GRD-001 y la confirmación de D7 (Q-GRD-21) y D8 (Q-GRD-22) que aporta US-GRD-007 (ADR-GRD-003 § 2; ADR-GRD-004 § 5; ADR-GRD-007 § 1).
- **Diagnósticos pendientes de confirmar** (D11 (Q-GRD-25)): `diagnostics[]` expone "relajación pendiente de confirmar" (`floor-relax-pending`) y "rama base no confirmada o pendiente" (`base-unconfirmed` y `base-change-pending`), cada uno con el código de la acción para confirmar, o una lista vacía (ADR-GRD-005 § 1 y § 3). Aquí solo se exponen: los casos de relajación y de cambio pendiente llegan con US-GRD-007 y US-GRD-014, que aportan también la confirmación. "No confirmada" ya puede aparecer tras US-GRD-001 en un repo con configuración del equipo y tras adoptar una huérfana (US-GRD-003).
- **Instalación huérfana**: esta historia detecta `instalacion-huerfana` (perfil borrado con la clave y el manifiesto aún en el repo) y la muestra con aviso, nunca como "Sin protección"; mientras tanto los hooks aplican el modo degradado con `instance-mismatch`. Adoptarla o retirarla es de US-GRD-003 (ADR-GRD-005 § 1; ADR-GRD-003 § 4).
- **Repo no observado**: comprobación periódica y aviso `protected-but-unobserved`; los hooks siguen evaluando con actor "sin atribuir" (ADR-GRD-005 § 4 y § 5; ADR-GRD-003 § 4).
- **Crates**: `crates/core` módulo `guardrails` (comprobación y detección), `crates/policy` (lista publicada), `crates/api` (estado y evento `protection-lost`) y `apps/cli` (consulta del estado).
- **Lista publicada tras SPIKE-GRD-001** (Enmienda 2026-10-04 de ADR-GRD-002 § 3 y ADR-GRD-005): `notPreventable[]` depende del SO, la versión de Git y el **backend de refs**, que se vuelve a comprobar en cada comprobación de estado (`git refs migrate`). Motivos: C, B, salto declarado y `no-reconocible` (crear worktree sin rama nueva). Apartado "se deniega de más" (Q-GRD-31). `hook-previo-no-encadenado` cubre el hook previo añadido después de instalar. El aviso propio de la rama base que desaparece o se reescribe en reftable es posterior al MVP (Q-GRD-28).
- **Enablers**: SPIKE-GRD-001 completo (matriz) **bloquea el cierre de la Dev Spec**: hecho en macOS; faltan Linux y Windows. La suite de pérdida externa y de la lista de INF-GRD-001 **bloquea el merge**. TS-GRD-001 no bloquea.
- **NFR, SEC y verificación**: NFR-GRD-10 y 13; SEC-GRD-01, 02, 05 y 08. ADR-GRD-005 Validación 1 a 7 y 9 a 11; ADR-GRD-002 Validación 9.
- **Enmiendas en motor-local**: ADR-GRP-010 (vigilar el `config` común, cada `config.worktree` y la carpeta de Guardrails en los repos protegidos) y ADR-GRP-005 § 6 (comandos reservados; adoptar y retirar una huérfana es de US-GRD-003). Depende además de US-GRP-006.

## Diseño y Dev Spec

- **Diseño:** no aplica (presentación del Cockpit y la CLI).
- **Dev Spec:** [DS-US-GRD-004](../dev-specs/US-GRD-004-aviso-proteccion-inactiva.md).

## Estado de la implementación (2026-10-08)

Implementado en: PR #201 (parcial). **Cumple** el escenario 1 (otro gestor reemplaza la protección: estado, causa, registro y aviso), el escenario 2 (una retirada propia no avisa), el 4 en lo ya expuesto (el mínimo seguro y el estado) y el 5 en lo que ya existía (`notPreventable` por backend de refs). **Falta**, con su dueño en la Dev Spec: el escenario 3 (repo retirado de la observación, con US-GRP-006), el vigilante de `config` y de `gitraptor/` para el ≤ 5 s de los repos observados (hoy 60 s o el siguiente evento de Git), H2 por worktree (`config.worktree`), la firma del binario y Linux y Windows. No cuenta como `implemented` para el criterio 1 de M2 mientras falte el escenario 3. Dev Spec: [DS-US-GRD-004](../dev-specs/US-GRD-004-aviso-proteccion-inactiva.md).
