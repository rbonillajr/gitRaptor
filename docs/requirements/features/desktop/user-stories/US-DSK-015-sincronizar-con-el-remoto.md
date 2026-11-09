---
id: US-DSK-015
title: "El usuario sube y trae los cambios del remoto desde la app con la protección del repo"
type: us
status: ready
priority: high
created: 2026-10-28
updated: 2026-11-08
feature: desktop
source: inline
decision: "Decisión de Rene del 2026-11-08 (aceptación de las recomendaciones, Q-DSK-7 a Q-DSK-20), ver CTX-DSK-001 § 11"
related:
  context:
    - CTX-DSK-001
  stories:
    - US-GRD-001
    - US-GRD-008
    - US-TMC-014
    - US-DSK-012
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - remoto
  - must
  - pendiente-de-rene
---

# US-DSK-015: El usuario sube y trae los cambios del remoto desde la app con la protección del repo

## Descripción

**Como** desarrollador novato, **quiero** traer los cambios del remoto y subir los míos desde la app, sabiendo antes cuántos commits van y vienen, **para** usar la app como mi cliente Git diario sin volver a otra herramienta.

**Valor**: BR-27 (UI-2, Must). Nivel **UI-2**. Imprescindible. Decisión de Rene (2026-11-08, Q-DSK-14): la app puede hacer push, pull y fetch como acción explícita del humano. BR-CKP-CONS-002 ("nunca push ni fetch") se enmienda para la app; lo escribe el Arquitecto y la historia no pasa a Dev Spec antes. Entra antes de v0.1.0 (Q-DSK-10, decisión de Rene del 2026-11-08).

## Reglas

- Solo el humano, como acción explícita en la app; nunca un agente ni el MCP (decisión de Rene del 2026-11-08, Q-DSK-14 opción b).
- Un push pasa por Guardrails como cualquier otro: un force-push a una rama protegida se deniega (US-GRD-001, US-GRD-008).
- Traer cambios que modifican la copia de trabajo tiene punto de recuperación previo (NFR-01).
- Las credenciales son las del usuario, en su configuración de Git (NFR-07); la app no las guarda.

## Dependencias

- **Reutiliza**: US-GRD-001, US-GRD-008 (la capa de hooks ya frena el push), US-TMC-014 (aviso de lo que sigue en el remoto), US-GRP-012 (ahead/behind).
- **Historias**: US-DSK-012 (ramas).
- **Qué falta en el motor**: DEP-DSK-4, operaciones de red en el catálogo (hoy prohibidas por BR-CKP-CONS-002 y la Validación 16 de ADR-CKP-002). transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Traer y subir cambios**

Dado "main" con 2 commits locales sin subir y 1 commit nuevo en el remoto
Cuando el usuario trae los cambios y después sube los suyos
Entonces "main" local y la del remoto tienen los mismos 3 commits

**Escenario: Force-push a una rama protegida**

Dado "main" protegida y reescrita en local
Cuando el usuario intenta subirla forzando
Entonces el push se deniega
  Y el usuario conoce la regla y cómo hacer una excepción consciente

**Escenario: Falla la autenticación con el remoto**

Dado un remoto que rechaza las credenciales del usuario
Cuando el usuario intenta subir "main"
Entonces nada cambia en el remoto ni en local
  Y el usuario sabe que el problema son sus credenciales de Git

## Requisitos Técnicos

Completado por el Arquitecto (2026-11-08). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: las operaciones de red tienen una sola historia dueña, esta, así que van aquí y no en una TS; ADR-CKP-002 (Enmienda 2026-11-08) ya asigna su entorno de red a esta historia. BR-CKP-CONS-002 ya está enmendada. **Bloqueada por**: TS-CKP-007 (rama de seguimiento y ahead/behind), TS-CKP-003, SPIKE-GRD-002 y el módulo del factor del SO de ADR-GRD-008 § 6 (sin factor no hay red desde la app), y la revisión del security-expert del entorno de red.

- Tres operaciones de red del catálogo, separadas de las locales (ADR-CKP-002, Enmienda 2026-11-08; versión menor): `fetch`, `pull` y `push`, servidas por `operation.prepare` y `operation.run`. Marcas: Cockpit sí, MCP **no**; sin herramienta MCP.
- Quién: solo el humano desde la app (clase de cliente `desktop`). La TUI o la CLI con capa `cockpit` reciben `not-available-for-layer`; un agente solicitante, `raptor-mcp` o un descendiente del ejecutor, rechazo (Q-DSK-14).
- Factor: **siempre factor por acción** de ADR-GRD-008, uno por plan y ligado a su huella; la presencia de sesión no lo cubre (ADR-GRP-017 § 4). Sin factor disponible, la app muestra el comando de terminal equivalente (ADR-GRP-017 § 5).
- Parámetros: nombre de un remoto ya configurado (nunca una URL) y ramas con las reglas L-02. Sin `--force`, `--force-with-lease`, `--mirror`, `--tags` ni borrar ramas remotas.
- `pull`: `fetch` de la rama de seguimiento más poner la rama local al día sobre ella; avance rápido si se puede y, si divergen, rebase en modo `stop` (ADR-CKP-002 § 9) para que el historial quede lineal (escenario 1). Gobernado como Rebase cuando hay commits locales. Punto previo: snapshot `previo_garantizado` antes de tocar refs o el worktree; "Deshacer" lo devuelve al previo.
- `fetch`: punto previo cuando mueve refs de seguimiento que la Time Machine guarda. No toca el worktree.
- `push`: gobernado como "push" con la decisión de Guardrails registrada una vez (US-GRD-001, US-GRD-008); un intento de reescribir una rama protegida se deniega con la regla (escenario 2). No tiene vuelta atrás en el remoto: el plan lo dice; el registro local guarda lo enviado (US-TMC-014).
- Entorno de red (lo cierra el plan de esta historia con el security-expert): credenciales del usuario por su `credential.helper`, `GIT_TERMINAL_PROMPT=0`, sin `GIT_ASKPASS` ni `SSH_ASKPASS`, protocolos limitados a los de la URL del remoto, `submodule.recurse=false` y hooks del usuario respetados (`pre-push`). Un fallo de credenciales deja todo igual y lo dice (escenario 3).
- Pruebas: contra un remoto local (`file://` o loopback), sin red real en CI (Validación 16 enmendada); captura de red de las operaciones locales: 0 conexiones; matriz V4 de ADR-GRP-017 con "push con sesión vigente pide el factor".

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente; espera la enmienda de BR-CKP-CONS-002 que escribe el Arquitecto.
