---
id: ADR-TMC-005
title: "ADR-TMC-005 — Solicitante, permisos y solape de undo, redo y restauración"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-005, ADR-GRP-007, ADR-GRP-012, ADR-GRP-013, ADR-TMC-002, ADR-TMC-003, ADR-CKP-002]
  stories: [US-TMC-002, US-TMC-003, US-TMC-009, US-TMC-010, US-TMC-011, US-TMC-012, US-TMC-013, US-TMC-021]
description: "El daemon atribuye al solicitante por la ascendencia del proceso llamante (agente X o sin atribuir), reutiliza la confirmación de los comandos reservados para tocar trabajo ajeno, Guardrails solo puede denegar y el solape se detecta por archivo y por ref"
tags: [adr, time-machine, permisos, solicitante, confirmacion-interactiva, solape, guardrails, mcp, br-tmc-auth-001, d-tmc-23]
published: true
---

# ADR-TMC-005 — Solicitante, permisos y solape de undo, redo y restauración

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-6 → (a) solape por archivo y ref; TQ-7 → (a) "sin atribuir" por MCP rechazado; TQ-8 → (a) el registro guarda la identidad del proceso; TQ-14 → (a) ascendencia + multiplexor + reto en el MVP, sin confirmación en Windows, y (b) presencia verificada por el SO en la Fase 2.

## Contexto

GitRaptor no puede probar que una petición viene del humano: un agente puede lanzar `raptor undo` desde su shell (Q34, R7). Por eso el **solicitante** se atribuye como un evento, "agente X" o "sin atribuir", nunca "humano" (D-TMC-23). Un agente solo deshace su propio trabajo; un solicitante "sin atribuir" necesita una confirmación interactiva, que un agente no pueda dar, para tocar trabajo de otro actor (BR-TMC-AUTH-001). Guardrails puede restringir, nunca ampliar (US-TMC-021). Un undo nunca sobrescribe cambios posteriores de otro actor en los mismos archivos o fragmentos (D-TMC-13, US-TMC-012). Motor-local ya resolvió en el daemon, para los comandos reservados, cómo distinguir un proceso de agente de uno que no lo es: identificador de proceso no reutilizable, ascendencia, terminal de control y líder de sesión (ADR-GRP-005 § 6, PQ-6, ADR-GRP-012 S3).

**Pregunta**: ¿cómo se identifica al solicitante por CLI y por MCP, cuándo se pide confirmación, cómo entra Guardrails y con qué granularidad se detecta el solape?

## Decisión

### 1. Identificación del solicitante (en el daemon, nunca en el cliente)

- El daemon obtiene el identificador no reutilizable del proceso cliente (pidfd, audit token o handle; ADR-GRP-005 § 6.1) y recorre su ascendencia con las reglas de SEC-TMC-03: un antecesor solo cuenta si arrancó antes que su hijo, y un multiplexor de terminal compartido con una sesión de agente convierte al cliente en agente.
- **Agente X**: si un antecesor es el proceso de una sesión de agente presente (S1 de ADR-GRP-012), el solicitante es el agente con atribución vigente de esa sesión en ese momento, con su origen. Se congela en el oplog (ADR-TMC-003 § 5).
- **"Sin atribuir"**: en cualquier otro caso. Nunca "humano"; el contrato de `crates/api` no tiene esa variante (ADR-GRP-013 § 6).
- **Canal**: se registra CLI, TUI, MCP o hook. `raptor-mcp` lo lanza el agente, así que su ascendencia resuelve a ese agente.
- **Agentes registrados** (Codex u otros, Q32): solo se reconocen como solicitantes si el registro guardó la identidad del proceso que se registró. Decidido (TQ-8 → a). **Depende de motor-local**: ADR-GRP-012/013 deben guardarla (nota aprobada, pendiente de integración); hasta entonces, una petición de un agente registrado que no se reconoce queda "sin atribuir".
- **Por MCP**, el repo y el worktree salen del cwd del llamante y el repo debe estar en la allowlist (NFR-02, SEC-TMC-15). **Solicitante "sin atribuir" por MCP** (TQ-7 → a): se rechaza de forma incondicional, antes de calcular el conjunto, porque el MCP es el canal de los agentes y un agente que no se puede identificar no puede probar que el trabajo es suyo.

### 2. Regla base de permisos

| Solicitante | Conjunto a deshacer o restaurar | Resultado |
|---|---|---|
| Agente X | Todo con atribución vigente = X | Permitido |
| Agente X | Incluye trabajo de otro agente o "sin atribuir" | Rechazado con el motivo |
| "Sin atribuir" (CLI/TUI) | Solo trabajo "sin atribuir" | Permitido |
| "Sin atribuir" (CLI/TUI) | Incluye trabajo atribuido a un agente | Requiere confirmación interactiva; sin ella, rechazado |
| "Sin atribuir" (MCP) | Cualquiera | Rechazado (TQ-7 → a) |

- **"Trabajo de otro actor"** para un solicitante sin atribuir = trabajo con atribución vigente a un agente. Lo "sin atribuir" es "Tú u otro" (D-TMC-12) y no exige confirmación, como piden US-TMC-002 y US-TMC-009.
- Los eventos de un hueco son "sin atribuir" y nunca entran en un undo por agente (BR-TMC-EDGE-002).
- **Operaciones del catálogo del Cockpit** (Enmienda 2026-10-04, Cockpit): la tabla se extiende a ellas; ver la sección final.

### 3. Confirmación interactiva

Se **reutiliza** el mecanismo de los comandos reservados de ADR-GRP-005 § 6, sin duplicarlo: el daemon comprueba por su cuenta que el cliente no desciende de un agente, que tiene terminal de control y que el líder de su sesión no desciende de un agente; la CLI/TUI pide la confirmación, que es UX y no un control. El MCP nunca la ofrece. A diferencia de un comando reservado, un undo no está prohibido a los agentes: solo la parte que toca trabajo ajeno exige la confirmación.

- **Reto ligado al plan** (SEC-TMC-03): el daemon emite un reto de un solo uso ligado a la conexión, al proceso y al hash del plan mostrado, que caduca a los 60 s y se invalida si el plan cambia.
- **Windows** (TQ-14 → a; cambia D-TMC-23 y BR-TMC-AUTH-001 en Windows, que actualiza el PO): sin confirmación de trabajo ajeno hasta tener una prueba de ascendencia y terminal equivalente; en Windows, un "sin atribuir" no puede tocar trabajo de un agente. Es una excepción declarada a NFR-TMC-13 (mismo comportamiento en los tres SO), decidida en TQ-14.
- **Inyección en la terminal**: el diagnóstico avisa si el SO permite escribir en la terminal de otro proceso (SEC-TMC-13).
- **Riesgo residual aceptado de forma explícita**: la confirmación protege frente a agentes confundidos, **no** frente a un proceso del mismo usuario que se desacopla de su árbol (doble fork + `setsid` + `script`, `launchctl`, `systemd-run`, `osascript`). Lo compensa el snapshot previo de cada operación, que permite deshacerla: un agente comprometido ya puede hacer `git reset --hard` sin GitRaptor. La presencia verificada por el SO queda para la Fase 2 (TQ-14 → b).

### 4. Guardrails solo puede denegar

Orden de evaluación: validación (BR-TMC-VAL-001), conjunto, regla base (§ 2), confirmación (§ 3), **política de Guardrails**, solape (§ 5), precondiciones de Git (ADR-TMC-002 § 3). La política la evalúa `crates/policy` (ADR-GRP-007) y devuelve `permitir` o `denegar(motivo)`; el resultado final es la **conjunción**: una política que "permite" algo que la regla base rechaza no tiene efecto (US-TMC-021, escenario 2). Toda petición, aceptada o rechazada, queda en el oplog con su solicitante y su motivo (auditoría, SEC-TMC-03).

### 5. Solape: por archivo y por ref en el MVP

- **Qué es**: deshacer o restaurar cambiaría una ruta o una ref que **después** del cambio a deshacer modificó otro actor. "Otro actor" = cualquier atribución vigente distinta del actor de lo que se deshace; "sin atribuir" cuenta como distinto de un agente.
- **Cómo se detecta**: por cada ruta del destino, se recorren los cambios de esa ruta entre capturas posteriores al cambio a deshacer (diferencias de árbol en el almacén) y su actor según los eventos del motor de ese intervalo. Por cada ref, sus movimientos posteriores. Si en un intervalo tocaron la ruta actores distintos o la atribución es mixta, cuenta como otro actor (conservador). Los cambios anteriores no cuentan (US-TMC-012, escenario 4).
- **Granularidad**: **archivo completo** en el MVP. Si otro actor tocó otra función del mismo archivo, también se detiene. Es más estricto que "fragmento" y nunca sobrescribe; la detección por fragmentos con fusión de tres vías queda para después (TQ-6 → a).
- **Resultado**: el undo se detiene sin cambiar el repo, se muestran los cambios en conflicto con su actor y su momento y la operación queda `rechazada` por solape. Qué opciones se ofrecen después lo decide el design-flow (US-TMC-012). El redo aplica la misma regla (S5).

## Alternativas consideradas

| Alternativa | En contra | Veredicto |
|---|---|---|
| **Identificar al solicitante en el cliente** (flag o variable de entorno) | Un agente declara lo que quiera; ADR-GRP-005 § 6 lo descarta | Descartada |
| **Tratar la terminal como humano** | Q34: nunca "humano"; un agente con pty la tendría | Descartada |
| **Confirmación por token o contraseña** | Un agente que lee la pantalla o el archivo la obtiene; añade fricción sin cerrar el riesgo | Descartada |
| **Guardrails con capacidad de ampliar** | Contradice US-TMC-021 | Descartada |
| **Solape por fragmento en el MVP** | Necesita fusión de tres vías fiable con bytes brutos; un error sobrescribe trabajo ajeno, que es lo que se quiere evitar | Diferida (TQ-6 → a) |

## Consecuencias

- ✅ BR-TMC-AUTH-001 sin variante "humano": las decisiones se toman en el daemon con los mismos controles que los comandos reservados.
- ✅ Un agente puede deshacer lo suyo por MCP o desde su shell, sin intervención.
- ✅ El solape nunca sobrescribe: la granularidad elegida solo puede detener de más, nunca de menos.
- ⚠️ Un agente no detectado ni reconocido que lanza la CLI desde una pty puede pasar por "sin atribuir" y confirmar. **Mitigación**: ascendencia de agentes registrados (TQ-8 → a, depende de motor-local), reto ligado al plan y snapshot previo de toda operación (se puede deshacer). La auditoría del oplog ayuda, pero el mismo usuario puede manipularla (SEC-TMC-09). Riesgo residual aceptado arriba.
- ⚠️ En worktrees compartidos casi todo queda "sin atribuir" (Q7, ADR-GRP-013 § 3), así que el solape detendrá muchos undos por agente. Es el comportamiento seguro.
- ⚠️ Depende de P17 para el undo por agente (D-TMC-22).

## Validación

1. **Por ascendencia**: un cliente lanzado desde el árbol de un Claude Code simulado queda como ese agente; por MCP, igual (US-TMC-013).
2. **Rechazos**: claude-1 sobre trabajo de claude-2 rechazado por MCP y por CLI; "sin atribuir" por MCP rechazado; repo sin cambios y rechazo en el oplog.
3. **Confirmación**: los casos de SEC-TMC-03 (cliente JSON-RPC directo, `setsid` sin pty, `tmux new-window` + `send-keys`, reto reutilizado o de otra conexión, plan cambiado) se rechazan; con terminal, fuera del árbol del agente y reto válido, se permite y se registra como "sin atribuir" con confirmación.
4. **Guardrails**: política que prohíbe, rechazo; política que "permite" lo prohibido, sigue rechazado (US-TMC-021).
5. **Solape**: los cuatro escenarios de US-TMC-012 con capturas reales; mismo archivo y otra función, también se detiene (granularidad declarada).

## Referencias

- **Reglas**: BR-TMC-AUTH-001, BR-TMC-CONS-005, BR-TMC-VAL-001, BR-TMC-WF-001, BR-TMC-WF-002, BR-TMC-EDGE-002; D-TMC-12, D-TMC-13, D-TMC-17, D-TMC-23. Q7, Q32, Q34, Q35, Q37.
- **ADRs**: ADR-GRP-005 § 6, ADR-GRP-007, ADR-GRP-012, ADR-GRP-013; ADR-TMC-002, ADR-TMC-003.
- **Enablers**: TS-TMC-004. **Features**: F-001-04 (políticas), F-001-05 (canal MCP). **Seguridad**: SEC-TMC-03, 07, 13, 15.

## Enmienda (2026-10-04, Cockpit)

Aplicada desde DEP-CKP-7 de [CTX-CKP-001](../../requirements/features/cockpit/context.md) (Q-CKP-16, BR-CKP-AUTH-001 a 003), con [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) § 3 (proposed). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia la identificación del solicitante, la regla base para undo, redo y restauración, ni el solape. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| La regla base del § 2 se extiende a las operaciones del catálogo de ADR-CKP-002, con su definición de "trabajo afectado" | § 2 | DEP-CKP-7; Q-CKP-16; ADR-CKP-002 § 3 |
| La confirmación interactiva del § 3 se reutiliza, ligada a la **huella del plan** de la operación | § 3 | ADR-CKP-002 § 2 y § 3 |
| Windows rechaza la confirmación de trabajo ajeno también para el catálogo | § 3 | TQ-14; BR-CKP-AUTH-003 |
| **Descendientes del ejecutor**: un proceso cuya ascendencia pasa por un hijo registrado del ejecutor se resuelve como el solicitante del plan de ese hijo, con su capa, nunca como "sin atribuir" (pasada de endurecimiento, 2026-10-04) | § 1 y § 3 | H-01 (revisión de seguridad de ADR-CKP-002); DEP-MCP-3 (CTX-MCP-001); ADR-CKP-002 § 3 |

- **Trabajo afectado** por operación (ADR-CKP-002 § 3): `merge-into-base`, los commits que entran en la base; `rebase-onto-base`, los commits reescritos; `discard-worktree`, los commits que no están en la base más lo sin commitear (si no hay nada, no afecta a nadie); `abort-in-progress`, lo que dejó la operación detenida. `create-worktree` no afecta trabajo de nadie y `open-in-editor` no escribe.
- **Atribución conservadora**: el actor de cada parte sale de los eventos del motor (ADR-GRP-013). Sin atribución clara o con atribución mixta, cuenta como otro actor, igual que el solape del § 5.
- **Misma tabla**: un agente X solo actúa sobre trabajo de X; sobre otro, rechazo. Un "sin atribuir" por CLI/TUI actúa sobre trabajo sin atribuir sin control, y sobre trabajo de un agente necesita la confirmación. Un "sin atribuir" por MCP, rechazo (TQ-7).
- **Reto**: de un solo uso, ligado a la conexión, al proceso y a la huella del plan; se invalida si el plan cambia o caduca. Un mismo ConfirmPrompt reúne todos los motivos del plan (⚡, sesión Activa, trabajo ajeno).
- **Guardrails solo puede denegar** (§ 4) también aquí: su decisión con la capa `cockpit` o `mcp` se toma antes de cualquier efecto (ADR-GRD-003, Enmienda (2026-10-04, Cockpit)).
- **Windows**: sin confirmación de trabajo ajeno; la operación se rechaza con su motivo. Pendiente: etapa de validación multiplataforma.
- **Validación añadida**: "sin atribuir" en macOS confirma el descarte de un worktree de claude-2 con el reto ligado; el reto reutilizado o el de un plan cambiado se rechazan; claude-1 sobre trabajo de claude-2, rechazo (ADR-CKP-002, Validación 13).

**Descendientes del ejecutor (§ 1; H-01, DEP-MCP-3)**. Decisión del orquestador (2026-10-04), validada por Arquitecto, PO y security-expert.

- **Regla nueva del § 1, antes que las demás**: si al recorrer la ascendencia del cliente aparece un **hijo registrado del ejecutor** del daemon (identidad verificada, ADR-CKP-002 § 4), el solicitante es **el del plan de ese hijo, con su capa**. Nunca "sin atribuir", aunque más arriba solo esté el daemon. Así un hook del usuario que abre el canal durante una operación de claude-1 actúa como claude-1, y la regla base del § 2 se le aplica como a claude-1.
- **Sin privilegios de humano**: ese proceso no puede pedir comandos reservados, la confirmación interactiva del § 3, la excepción consciente ni Cancelar, aunque el plan sea de un "sin atribuir" y aunque abra una pty. La capa del plan solo le sirve para la atribución y el registro. Por eso el § 3 añade una comprobación a los controles 1 a 3: el cliente no desciende de un hijo del ejecutor.
- **Canal**: se registra el canal real del proceso (CLI o hook), con una marca de "descendiente del ejecutor" y el id de la operación. El rechazo en el canal es **pendiente, dueño: worker del canal (TS-GRP-004)**.
- **Residuo**: un descendiente que se desacopla de su árbol (doble fork, `setsid`) deja de pasar por el hijo. Es el riesgo residual ya aceptado en el § 3, compensado por el snapshot previo.
- **Validación añadida**: un hook bajo el plan de claude-1 que se conecta al canal se resuelve como claude-1; si pide un comando reservado, una confirmación, la excepción o Cancelar, se rechaza, también con una pty abierta con `script` (ADR-CKP-002, Validación 17).
