---
id: US-TMC-004
title: "El trabajo hecho fuera de GitRaptor queda capturado como punto recuperable"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-06
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-001, US-GRP-002, US-GRP-004]
covers: [BR-TMC-CONS-003, BR-TMC-CONS-002, D-TMC-9, D-TMC-10, D-TMC-16, D-TMC-25]
blocked_by: []
tags: [time-machine, captura-continua, git-crudo]
---

# US-TMC-004: El trabajo hecho fuera de GitRaptor queda capturado como punto recuperable

## Descripción

**Como** desarrollador orquestador
**Quiero** que lo que mis agentes o yo hacemos con Git crudo o en el editor se capture a medida que ocurre
**Para** recuperar trabajo aunque la operación no haya pasado por GitRaptor

**Valor**: la red de seguridad cubre el caso más frecuente, el agente que usa Git directamente, sin prometer más de lo que cumple.

## Reglas cubiertas

BR-TMC-CONS-003 (cobertura por observación, nivel b; tope por archivo, TQ-5) · BR-TMC-CONS-002 (ignorados y credenciales) · D-TMC-9, D-TMC-10, D-TMC-16, D-TMC-25 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Una edición fuera de GitRaptor queda capturada**

Dado un repo observado sin hooks de Guardrails
Cuando se modifica "api.rs" y se crea "util.rs" sin seguimiento en "feat-login" desde el editor
Entonces la Time Machine guarda un punto recuperable con ambos archivos
  Y ese punto figura como "capturado por observación"

**Escenario: Un reset destructivo con Git crudo deja recuperable el último estado capturado**

Dado que el último estado capturado de "feat-login" incluye "api.rs" modificado
Cuando un agente ejecuta un reset destructivo con Git crudo en "feat-login"
Entonces el último estado capturado antes del reset sigue disponible para restaurar
  Y figura como "capturado por observación", no como "snapshot previo"

**Escenario: Los archivos ignorados y las credenciales no se capturan**

Dado que en "feat-login" se modifica ".env", ignorado por el repo, y se crea "deploy.pem" sin seguimiento y sin ignorar
  Y un perfil sin opción para incluir credenciales
Cuando la Time Machine captura los cambios de "feat-login"
Entonces el punto no contiene ".env" ni "deploy.pem"
  Y el punto declara "deploy.pem" como excluido por credenciales

**Escenario: Un archivo por encima del tope deja la captura parcial**

Dado que en "feat-login" se modifica "api.rs" y se crea "dump.bin" con un tamaño por encima del tope de la captura por observación
Cuando la Time Machine captura los cambios de "feat-login"
Entonces el punto contiene "api.rs" y no contiene "dump.bin"
  Y el punto figura como captura parcial con "dump.bin" en la lista de lo omitido

**Escenario: Una captura que falla no se presenta como protegida**

Dado que guardar una captura de "feat-login" falla
Cuando se consulta el historial de la Time Machine
Entonces ese cambio figura sin punto recuperable
  Y ningún punto se presenta como protegido para ese cambio

## Requisitos Técnicos

- La captura se alimenta de los eventos ya publicados por el motor, fuera de su presupuesto de 300 ms (ADR-TMC-004 § 2; ADR-GRP-011).
- Disparadores por worktree: quietud de 1 s o cada 5 s con actividad continua, e inmediata tras un evento de Git, con anclaje de commits (⚠️ ASSUMPTION, ajusta SPIKE-TMC-001).
- Coalescencia (una captura en curso por worktree), captura incremental y descarte si llega un evento de Git durante la lectura.
- Archivos grandes en esta captura: exclusión declarada de los de más de 50 MB (ADR-TMC-001 § 2) y hueco "sin espacio" al llegar a la cuota (SEC-TMC-12); una captura fallida no crea punto (nivel declarado, ADR-TMC-004 § 4).
- Verificación: el gate de INF-GRP-002 con la Time Machine activa mantiene el p95 del motor ≤ 300 ms.
- Diferido (Decisión del orquestador, 2026-10-06, validada por el Arquitecto; ver la Dev Spec): la captura incremental con las rutas del motor (escalón 2) espera a TS-GRP-002/003 y cada captura hace detección completa; la cuota y el hueco "sin espacio" son de US-TMC-022, y mientras tanto la captura se omite por debajo del suelo de espacio libre de SEC-TMC-12. La consistencia se garantiza por estado (reflog de `HEAD`, `HEAD`, índice), no por la marca al empezar.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-004](../dev-specs/US-TMC-004-captura-continua-git-crudo.md) (validada por el Arquitecto, 2026-10-06).

## Dependencias

- **Historias**: US-TMC-001; US-GRP-002 (eventos en vivo) y US-GRP-004 (observación continua) de motor-local.
- **Externas**: ninguna. Riesgo residual R2: lo editado entre la última captura y una operación destructiva de Git crudo puede perderse.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
