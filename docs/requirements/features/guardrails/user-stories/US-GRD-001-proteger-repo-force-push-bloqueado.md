---
id: US-GRD-001
title: "Un agente que intenta hacer force-push en un repo protegido queda bloqueado"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRP-001
    - US-GRP-012
tags:
  - guardrails
  - esqueleto-andante
  - hooks-git
  - minimo-seguro
  - force-push
---

# US-GRD-001: Un agente que intenta hacer force-push en un repo protegido queda bloqueado

## Descripción

**Como** desarrollador orquestador, **quiero** proteger un repo con mi permiso explícito y que, aunque el equipo no haya configurado nada, un force-push o el borrado de la rama base queden denegados con su motivo, **para** que ningún agente reescriba ni borre `main` aunque use Git directo.

**Valor**: la demo del BRD (§ 13): un agente intenta un force-push y queda bloqueado, con la regla explicada.

## Reglas cubiertas

BR-AUTH-002 (instalar solo con permiso explícito; sin repreguntar tras denegar) · BR-EDGE-001 (conjunto mínimo seguro sin configuración, Q-GRD-5) · BR-CALC-001 (la decisión dice qué regla y de qué nivel) · BR-WF-002 (Sin protección → Solo hooks) · BR-CONS-003 (la rama base `main` se confirma al instalar la protección, Q-GRD-23) · BR-AUTH-001 (instalar y confirmar la rama base inicial están reservados al humano) — ver [business-rules.md](../business-rules.md)

## Alcance

Esta historia cubre **repos sin configuración del equipo**. Al instalar se confirma la rama base `main` por defecto (Q-GRD-23). Si el repo ya tiene configuración del equipo, la instalación no la lee en esta historia: la rama base queda "no confirmada", con la unión {`main`, rama principal} protegida, hasta US-GRD-014 (Q-GRD-21, Q-GRD-23).

## Dependencias

- **Historias**: US-GRP-001 (motor-local: repo observado; Q-GRD-15) y US-GRP-012 (motor-local: rama base `main`).
- **Externas**: ninguna bloqueante. No necesita el ADR P8 (motor-local): sin configuración aplica el mínimo seguro.
- **Transversal**: los escenarios se cumplen igual en Windows, macOS y Linux; instalar no toca nada fuera del repo (NFR-01, Q17 de motor-local). Cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: El desarrollador protege un repo con su permiso**

Dado el repo "demo" observado, sin configuración de Guardrails, sin hooks y en estado "Sin protección"
Cuando el desarrollador pide proteger "demo"
Entonces GitRaptor le explica qué se instala, en qué repo, por qué y cómo se revierte, y qué rama base va a quedar confirmada ("main")
  Y al conceder el permiso el estado de protección de "demo" pasa a "Solo hooks", con la rama base "main" confirmada

**Escenario: Un force-push queda denegado por el mínimo seguro**

Dado el repo "demo" en estado "Solo hooks" y sin configuración de Guardrails
Cuando un proceso hace force-push de la rama "feat-x" con Git directo
Entonces la operación no se ejecuta
  Y el motivo nombra la regla "prohibir force-push" del conjunto mínimo por defecto
  Y la rama "feat-x" del remoto no cambia

**Escenario: Borrar la rama base queda denegado**

Dado el repo "demo" en estado "Solo hooks" con rama base "main"
Cuando un proceso intenta borrar la rama "main" con Git directo
Entonces la operación no se ejecuta y el motivo nombra la protección de la rama base
  Y la rama "main" sigue existiendo

**Escenario: Una operación fuera del mínimo seguro se permite**

Dado el repo "demo" en estado "Solo hooks" y sin configuración de Guardrails
Cuando un proceso hace commit en la rama "feat-x"
Entonces el commit se ejecuta con normalidad

**Escenario: Sin permiso no se instala nada ni se vuelve a preguntar**

Dado el repo "demo" observado y en estado "Sin protección"
Cuando el desarrollador deniega el permiso, o no responde
Entonces las rutas operativas de "demo" quedan idénticas a como estaban
  Y "demo" sigue en "Sin protección"
  Y GitRaptor no vuelve a pedir ese permiso hasta que el desarrollador lo active a mano

**Escenario: El permiso de un repo no alcanza a otro**

Dado los repos observados "demo" y "otro", ninguno protegido
Cuando el desarrollador concede el permiso para "demo"
Entonces "otro" sigue en "Sin protección" y sin ningún cambio en sus rutas operativas

## Requisitos Técnicos

- **Gobierno**: ADR-GRD-001 § 1 a § 4 y § 7 (dispatchers, instalación transaccional y capa de escritura), ADR-GRD-003 § 1 a § 4 (evaluación pura, mínimo seguro, contrato de decisión y canal autenticado) y ADR-GRD-007 § 1 (instalar y registrar la denegación del permiso como comandos reservados; permiso por repo).
- **Interceptación**: el force-push se decide en `pre-push`; el borrado de la rama base, en `reference-transaction` (estado `prepared`) en local y en `pre-push` en remoto (ADR-GRD-002 § 1 y § 4). Esta historia no lee la configuración del equipo (ADR-GRD-004 § 5).
- **Rama base confirmada al instalar** (Q-GRD-23): la historia cubre repos sin configuración del equipo. Al conceder el permiso, el comando reservado de instalación confirma la rama base `main`, el estado pasa a `hooks-only` y el permiso queda `granted` o `denied` sin volver a ofrecerse (ADR-GRD-007 § 1; ADR-GRD-004 § 3, punto 5; ADR-GRD-005 § 3).
- **Rama principal y repo con configuración del equipo**: esta historia resuelve el nombre de la rama principal (`refs/remotes/<remoto>/HEAD` conocida; si no, la rama local; si no, `main`) y detecta si hay configuración del equipo en su copia, sin interpretarla (ADR-GRD-004 § 3, puntos 1 a 3). Si la hay, la rama base queda "no confirmada" y el mínimo protege la unión {`main`, rama principal} hasta US-GRD-014 y TS-GRD-001 (ADR-GRD-004 § 3, punto 5).
- **Crates**: `crates/policy` (evaluación y mínimo seguro), `crates/core` módulo `guardrails` (instalación, rama base confirmada y evaluación en el daemon), `crates/git` capa de lectura (rama principal y presencia de la configuración) y capa de escritura de Guardrails, `crates/api` (contrato de decisión y comandos reservados) y `apps/cli` (`raptor hook` y la instalación con permiso).
- **Enablers**: SPIKE-GRD-001 **bloquea el merge** (force-push, borrado de la rama base y coste en Windows). El núcleo de INF-GRD-001 con su suite de instalación y mínimo es el gate de CI que **bloquea el merge**. TS-GRD-001 no bloquea: se queda con la lectura e interpretación de los blobs de configuración commiteados.
- **NFR, SEC y verificación**: NFR-GRD-01 a 08 y 15; SEC-GRD-01, 02, 03, 06, 07, 10, 14, 16, 17, 18 y 19. ADR-GRD-001 Validación 1, 5 a 9, 12 y 13; ADR-GRD-002 Validación 3 a 6; ADR-GRD-003 Validación 1 a 4 y 6 a 10; ADR-GRD-004 Validación 10; ADR-GRD-007 Validación 11. Siempre en repos temporales.
- **Enmiendas**: en motor-local, ADR-GRP-005 § 1 y § 6 con SEC-03, ADR-GRP-006 § 4 (diario, rama base confirmada e id de instancia), ADR-GRP-009, ADR-GRP-013 § 1 e INF-GRP-001. Q-GRD-23 está en ADR-GRD-004 § 3 (punto 5).

## Diseño y Dev Spec

- **Diseño:** no aplica (sin superficie propia; la presentación es del Cockpit y la CLI).
- **Dev Spec:** pendiente (`/aadd-devspec US-GRD-001`).
