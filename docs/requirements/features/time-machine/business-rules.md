---
id: BR-TMC-001
title: "Reglas de Negocio — Time Machine"
type: business-rules
status: draft
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  stories:
    - US-TMC-001
    - US-TMC-002
    - US-TMC-003
    - US-TMC-004
    - US-TMC-005
    - US-TMC-006
    - US-TMC-007
    - US-TMC-008
    - US-TMC-009
    - US-TMC-010
    - US-TMC-011
    - US-TMC-012
    - US-TMC-013
    - US-TMC-014
    - US-TMC-015
    - US-TMC-016
    - US-TMC-017
    - US-TMC-018
    - US-TMC-019
    - US-TMC-020
    - US-TMC-021
tags:
  - time-machine
  - snapshots
  - undo
  - timeline
---

# Reglas de Negocio: Time Machine

> **Propósito**: reglas que gobiernan los snapshots, el undo/redo, la restauración y el timeline. Los códigos llevan el prefijo `TMC` para no colisionar con los del Motor local (BR-CONS-001, BR-EDGE-004, etc.).

---

## Contexto

**Feature**: Time Machine (F-001-03)
**Enlace a contexto**: [context.md](./context.md) (CTX-TMC-001)
**Última actualización**: 2026-10-03

---

## Categorías de Reglas

| Categoría | Cantidad | Críticas |
|-----------|----------|----------|
| Validaciones de Datos | 1 | 0 |
| Workflows y Estados | 3 | 3 |
| Permisos y Autorizaciones | 1 | 1 |
| Consistencia de Datos | 5 | 5 |
| Tiempo y Expiración | 1 | 0 |
| Edge Cases | 4 | 3 |

> Cálculos de Negocio y Elegibilidad: no aplican a esta feature.

---

## 1. Validaciones de Datos

### BR-TMC-VAL-001: Un undo que no puede resolverse no cambia nada

**Descripción**: si el agente indicado no existe en el timeline, el periodo no es válido o no hay nada que deshacer en el alcance pedido, el repo queda igual y el usuario recibe el motivo.

**Criticidad**: Media

**Regla formal**:
```
IF agente desconocido OR periodo inválido OR conjunto a deshacer vacío
THEN no se modifica el repo + se informa el motivo
```

**Ejemplo**:
- `raptor undo --agent claude-9 --since 20m` sin ese agente en el timeline → "no hay operaciones de claude-9 en los últimos 20 minutos"; nada cambia.
- `raptor undo --since veinte` → periodo no válido; nada cambia.

---

## 2. Cálculos de Negocio

No aplica.

## 3. Reglas de Elegibilidad

No aplica.

---

## 4. Workflows y Estados

### BR-TMC-WF-001: Undo y redo

**Descripción**: `undo` devuelve el ámbito afectado al estado previo a la última operación. `redo` revierte el último undo. Ambos son operaciones de la Time Machine y llevan su propio snapshot previo (BR-TMC-CONS-001), así que nada se pierde al encadenarlos.

**Ámbito por defecto** (D-TMC-19, D-TMC-20, D-TMC-23): `raptor undo` sin flags deshace la última operación del **worktree desde el que se invoca**. Si esa operación es de un actor distinto del solicitante, aplican BR-TMC-AUTH-001 y la regla de solape (BR-TMC-CONS-005). Nunca actúa sobre otros worktrees sin pedirlo.

**Undos seguidos** (TQ-9, D-TMC-25): cada worktree tiene su pila. Un segundo `undo` deshace la operación anterior aún no deshecha, y así sucesivamente. `redo` rehace el último undo; una operación nueva en el worktree invalida el redo pendiente.

**Criticidad**: Alta

**Regla formal**:
```
ámbito = worktree desde el que se invoca                       (D-TMC-19, D-TMC-23)
IF actor de la última operación ≠ solicitante → BR-TMC-AUTH-001 + BR-TMC-CONS-005
undo:  snapshot previo → restaurar estado anterior a la última operación del ámbito aún no deshecha → registrar el undo
       (undos seguidos = pila por worktree, TQ-9)
redo:  disponible si el último evento del ámbito es un undo
       → snapshot previo → restaurar el estado anterior a ese undo
IF hubo una operación nueva en el ámbito después del undo THEN redo no disponible + se informa   (TQ-9)
IF desde el undo hubo cambios de otro actor en los mismos archivos
THEN aplica BR-TMC-CONS-005 (solape): se detiene                  (S5, aceptado 2026-10-03)
```

**Ejemplo**: un agente hace `reset --hard` y borra 3 archivos sin commitear → `raptor undo` los recupera → `raptor redo` vuelve al estado tras el reset; el contenido de los 3 archivos sigue recuperable en el timeline.

**Ejemplo (pila, TQ-9)**: en `feat-pagos` hubo un commit A y luego un checkout B → `raptor undo` deshace B → otro `raptor undo` deshace A. Si después se hace un commit C, `raptor redo` ya no está disponible.

### BR-TMC-WF-002: Undo por agente y por periodo

**Descripción**: `raptor undo --agent <id> [--since <duración>]` deshace solo las operaciones con atribución vigente al agente `<id>` en el periodo. Lo "sin atribuir" y lo de otros agentes nunca entra.

**Criticidad**: Alta

**Regla formal**:
```
conjunto = operaciones con atribución vigente = <id> AND momento ≥ ahora − duración
EXCLUDE "sin atribuir", otros agentes, eventos en huecos
IF deshacer el conjunto toca cambios posteriores de otro actor → BR-TMC-CONS-005
ELSE snapshot previo → deshacer el conjunto → registrar el undo
```

**Ejemplo**: `raptor undo --agent claude-1 --since 20m` revierte dos commits y una edición de claude-1; un cambio "sin atribuir" en otro archivo queda intacto.

### BR-TMC-WF-003: Restaurar a un punto del timeline

**Descripción**: el desarrollador elige un snapshot del timeline y el repo vuelve a ese estado. Solo se puede restaurar a un punto que tenga snapshot completo. La restauración lleva snapshot previo, así que se puede deshacer.

**Criticidad**: Alta

**Regla formal**:
```
REQUIRE punto con snapshot completo
alcance = worktree donde se pide + ramas y worktrees cambiados después del punto   (D-TMC-20)
snapshot previo → restaurar → registrar la restauración
```

---

## 5. Permisos y Autorizaciones

### BR-TMC-AUTH-001: Quién puede deshacer qué

**Descripción** (D-TMC-17, D-TMC-23): GitRaptor no puede probar que una petición viene del humano: un agente puede lanzar `raptor undo` desde su propia shell. Por eso el **solicitante** de un undo, redo o restauración se atribuye igual que los eventos: "agente X" o "sin atribuir", nunca "humano" (Q34). Si el solicitante es un agente, solo deshace operaciones con atribución vigente a él. Si es "sin atribuir", deshacer trabajo de otro actor exige una confirmación interactiva del desarrollador en ese momento, que un agente no puede dar.

**Excepciones del MVP** (D-TMC-23 actualizada): en **Windows** no se ofrece esa confirmación hasta que exista una forma fiable de probar que no la da un agente; la petición "sin atribuir" que toca trabajo de otro actor se rechaza con su motivo (TQ-14). En macOS y Linux sigue la confirmación. En la Fase 2 se planea la presencia verificada por el sistema operativo (Touch ID, Windows Hello, polkit). Por **MCP**, una petición "sin atribuir" se rechaza siempre, antes de evaluar qué toca (TQ-7): es más estricto que la regla general, no la contradice. Guardrails puede restringirlo más, nunca ampliarlo. Cómo se identifica al solicitante (CLI frente a MCP) lo decide el Arquitecto.

**Criticidad**: Alta

**Regla formal**:
```
solicitante ∈ {agente X, "sin atribuir"}                       (nunca "humano", Q34)
IF solicitante = agente X
  THEN permitido solo si todo el conjunto tiene atribución vigente = X; si no, rechazo
IF solicitante = "sin atribuir" AND la petición llega por MCP
  THEN rechazo, siempre                                             (TQ-7)
IF solicitante = "sin atribuir" AND el conjunto incluye trabajo de otro actor
  IF sistema = Windows THEN rechazo, sin confirmación               (TQ-14, MVP)
  ELSE pedir confirmación interactiva en ese momento; sin ella, rechazo
IF una política de Guardrails lo prohíbe THEN rechazo
rechazo → el repo no cambia + se informa el motivo
```

**Ejemplo**:
- claude-1 pide vía MCP deshacer un commit de claude-2 → rechazado.
- Claude Code ejecuta `raptor undo` desde su shell y se le atribuye la petición → solo puede deshacer lo suyo.
- Una petición "sin atribuir" en la terminal de macOS o Linux toca un commit de claude-1 → se pide confirmación interactiva; sin ella, nada cambia.
- La misma petición en Windows → rechazada con el motivo ("en Windows no se puede confirmar trabajo de otro actor todavía"); nada cambia.
- Una petición "sin atribuir" llega por MCP, aunque solo toque trabajo "sin atribuir" → rechazada; nada cambia.

---

## 6. Reglas de Consistencia de Datos

### BR-TMC-CONS-001: Sin snapshot previo no hay operación

**Descripción**: toda operación lanzada por GitRaptor (CLI, TUI/Cockpit, MCP) que modifica el repo, y todo undo, redo o restauración, va precedida de un snapshot. Si el snapshot no se completa, la operación no se ejecuta y se informa del motivo. Deriva de NFR-01.

**Criticidad**: Alta

**Regla formal**:
```
FOR EACH operación lanzada por GitRaptor que modifica el repo:
  snapshot previo completo → ejecutar
  IF falla el snapshot THEN no ejecutar + informar
```

**Ejemplo**: el Cockpit descarta el worktree `feat-pagos` y su rama (BR-07) → antes queda un snapshot con su trabajo sin commitear; si el disco está lleno y el snapshot falla, el worktree no se borra.

### BR-TMC-CONS-002: Qué contiene un snapshot

**Descripción**: un snapshot incluye el working tree sin commitear, lo preparado para el próximo commit, los archivos nuevos sin seguimiento y el estado de ramas y worktrees. No incluye (D-TMC-16, actualizada por TQ-16; D-TMC-25):
- los archivos ignorados por `.gitignore`;
- por defecto, la lista cerrada de archivos de credenciales sin seguimiento: `.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`, `.npmrc`, `.pypirc`, `.netrc`, `*.tfstate*`, `credentials*.json`. El perfil permite incluirlos;
- los repos anidados sin seguimiento (un directorio con su propio repo dentro del worktree) (TQ-15).

Las credenciales excluidas y los repos anidados se **declaran** en el snapshot como exclusión. Al restaurar, nada de lo excluido se escribe ni se borra.

**Criticidad**: Alta

**Regla formal**:
```
contenido = working tree + preparado + sin seguimiento + estado de ramas y worktrees
EXCLUDE ignorados por .gitignore
EXCLUDE lista cerrada de credenciales sin seguimiento   UNLESS el perfil los incluye   (TQ-16)
EXCLUDE repos anidados sin seguimiento                                                  (TQ-15)
credenciales excluidas y repos anidados → declarados como exclusión en el snapshot
restaurar → lo excluido no se escribe ni se borra
```

**Ejemplo**: un agente crea `nuevo.rs` sin añadirlo y edita `lib.rs` → ambos se recuperan. `node_modules/` y `.env`, ignorados, no se copian ni se tocan al restaurar.

**Ejemplo (credenciales, TQ-16)**: un agente crea `deploy.pem` y `.env.local` sin seguimiento y sin ignorar, junto a `nuevo.rs` → el snapshot guarda `nuevo.rs` y declara `deploy.pem` y `.env.local` como "excluidos por credenciales". Al restaurar ese punto, `nuevo.rs` vuelve y los dos archivos de credenciales quedan como estén en el disco. Si el desarrollador los incluye en su perfil, el siguiente snapshot sí los guarda.

### BR-TMC-CONS-003: Cobertura declarada en dos niveles

**Descripción** (D-TMC-9, D-TMC-10; refina BR-08 y NFR-01 del BRD): (a) **Garantizada**: snapshot previo antes de toda operación lanzada por GitRaptor y de cada undo o restauración. (b) **Por observación**: el trabajo hecho con Git crudo o en el editor se captura a medida que el motor observa cambios, y con snapshot previo cuando existan los hooks de Guardrails, sin depender de ellos. Cada punto del timeline indica su nivel. La Time Machine no promete un snapshot previo donde no lo hay.

**Límites de disco** (TQ-5, D-TMC-25; cifras propuestas 50 MB, 20 GB y máx(5 GB, 5 %), ajustadas por el spike): en la captura por observación, un archivo que supera el tope de tamaño no se copia y la captura queda **parcial**, con la lista de lo omitido. Si se alcanza la cuota del repo o el espacio libre mínimo, la captura por observación se detiene y el timeline muestra un **hueco "sin espacio"** (BR-TMC-EDGE-002). El snapshot previo garantizado (nivel a) no tiene tope por archivo.

**Criticidad**: Alta

**Ejemplo**: un agente ejecuta `git reset --hard` con Git crudo sin hooks de Guardrails → el timeline muestra el último estado capturado antes del reset como "capturado por observación", no como "snapshot previo". Lo editado entre esa captura y el reset puede perderse (riesgo R2 del contexto).

### BR-TMC-CONS-004: Las escrituras de la Time Machine son suyas, explícitas y recuperables

**Descripción**: la Time Machine solo escribe para guardar snapshots y para ejecutar un undo, redo o restauración pedido por un actor. Los snapshots no aparecen como cambios del usuario, no se empujan al remoto por accidente, no los borra un `git gc` y no los altera un agente que trabaja en el working tree (D-TMC-11). Ninguna escritura se atribuye al Motor local (Q21).

**Excepción explícita** (ADR-TMC-003 § 6.4, D-TMC-25): al arrancar tras una interrupción, la Time Machine libera el bloqueo de Git que ella misma dejó, sin que nadie lo pida, para que Git no quede bloqueado para el usuario y sus agentes. No toca contenido, y un bloqueo que no es suyo nunca se libera.

**Criticidad**: Alta

**Regla formal**:
```
escrituras permitidas = {guardar snapshot, undo, redo, restauración pedida}
                      + {liberar al arrancar un bloqueo de Git propio}       (excepción, no toca contenido)
bloqueo ajeno → nunca se libera
Constraint: guardar un snapshot no cambia el estado observable del repo
Constraint: push / gc / trabajo de un agente no exponen ni alteran snapshots
```

### BR-TMC-CONS-005: Atribución vigente, presentación y solape

**Descripción**: el timeline y el undo por agente usan la atribución vigente (Q37). "Sin atribuir" se presenta como "Tú u otro (sin atribuir)" y nunca como "humano" (Q34, D-TMC-12). Si deshacer lo de un actor tocaría cambios posteriores de otro actor en los mismos archivos o fragmentos, la Time Machine no sobrescribe: se detiene, muestra el solape y deja decidir al desarrollador (D-TMC-13). El registro de un undo (el solicitante, atribuido según BR-TMC-AUTH-001, y sobre qué actuó) no se reescribe si después cambia la atribución (D-TMC-18).

**Criticidad**: Alta

**Regla formal**:
```
actor mostrado ∈ {"agente X (detectado|registrado)", "Tú u otro (sin atribuir)"}
IF solape con otro actor THEN detener + mostrar solape; el repo no cambia
registro de undo = inmutable; atribución mostrada = vigente
```

**Ejemplo**: claude-1 editó `api.rs` a las 10:00 y un cambio "sin atribuir" tocó la misma función a las 10:05 → `undo --agent claude-1` se detiene y muestra ambos cambios.

---

## 7. Reglas de Tiempo y Expiración

### BR-TMC-TIME-001: Retención de snapshots

**Descripción** (D-TMC-15): los snapshots se conservan un tiempo configurable; por defecto 30 días. El valor lo admiten el perfil y la configuración local personal, no la del equipo (Q24). Nunca se purga el snapshot previo a la última operación destructiva. Se avisa antes de purgar: la purga solo ocurre si el aviso se mostró al menos una vez en la CLI o la TUI **y** pasaron 24 horas desde que se mostró por primera vez; si nadie lo vio, no se purga (TQ-11). Alcanzar una cuota de disco no adelanta la purga (TQ-5). Borrar contenido concreto de los snapshots (`raptor tm forget`) queda fuera del MVP (D-TMC-24, TQ-17).

**Criticidad**: Media

**Regla formal**:
```
retención = local personal ?? perfil ?? 30 días        (equipo: no admitido)
purgable = snapshot más antiguo que la retención AND NOT previo a la última operación destructiva
purga solo si aviso mostrado en CLI/TUI AND ≥ 24 h desde la primera vez que se mostró   (TQ-11)
cuota de disco alcanzada → no purga antes de tiempo                   (TQ-5)
```

**Ejemplo (TQ-11)**: un snapshot cumple 30 días el lunes, pero el desarrollador no abre la CLI ni la TUI hasta el miércoles → el miércoles ve el aviso y la purga ocurre a partir del jueves a la misma hora.

---

## 8. Reglas Excepcionales (Edge Cases)

### BR-TMC-EDGE-001: Lo deshecho ya está en el remoto

**Descripción**: el undo es solo local. Si lo deshecho ya se empujó, se avisa de que sigue en el remoto. La Time Machine nunca hace push ni force-push por su cuenta (D-TMC-14).

**Criticidad**: Alta

### BR-TMC-EDGE-002: Huecos de observación en el timeline

**Descripción**: el timeline muestra cada hueco de forma explícita. Lo ocurrido en un hueco es "sin atribuir" (BR-EDGE-005 de motor-local), nunca entra en un undo por agente y no ofrece puntos de restauración que no tengan snapshot. Un hueco puede deberse a falta de espacio: la captura por observación se detuvo por la cuota o el espacio libre mínimo, y el hueco se declara como "sin espacio" (TQ-5, BR-TMC-CONS-003).

**Criticidad**: Alta

### BR-TMC-EDGE-003: Interrupción a mitad de un snapshot o de un undo

**Descripción**: si el proceso muere durante un snapshot, ese snapshot no cuenta como punto válido y el repo no cambió. Si muere durante un undo o restauración, el repo queda recuperable al estado previo gracias al snapshot que lo precede, y al volver se informa de lo ocurrido (NFR-12).

**Fallo detectado a mitad** (TQ-10, D-TMC-25): si el undo o la restauración detecta un fallo antes de terminar (disco lleno, un archivo bloqueado por otro programa), no hay vuelta atrás automática: la operación se detiene, queda marcada como **interrumpida** en el timeline, igual que tras una muerte del proceso, y se informa de que `raptor undo` devuelve el repo al snapshot previo. Hay un único camino de recuperación.

**Criticidad**: Alta

### BR-TMC-EDGE-004: Operación de Git en curso

**Descripción**: (S6, aceptado por Rene Bonilla el 2026-10-03). Con un rebase o un merge a medias, el undo y la restauración se detienen y piden terminar o abortar esa operación antes; el repo no cambia.

**Criticidad**: Media

---

## Matriz de Priorización

| Regla | Criticidad | Prioridad |
|-------|------------|-----------|
| BR-TMC-CONS-001, CONS-002, CONS-004 | Alta | 🔴 P0 |
| BR-TMC-WF-001, EDGE-003 | Alta | 🔴 P0 |
| BR-TMC-CONS-003, CONS-005, WF-002, AUTH-001, EDGE-001, EDGE-002 | Alta | 🟡 P1 |
| BR-TMC-WF-003, VAL-001, TIME-001, EDGE-004 | Media/Alta | 🟡 P1 |

---

## Trazabilidad

### Reglas → User Stories

Pendiente: las historias se generan tras la aprobación humana del requerimiento (ADR-018).

### Reglas → Criterios de Aceptación

Cada regla tendrá al menos un escenario Gherkin, incluido uno negativo, en su historia.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 0.1 | 2026-10-03 | PO (AADD) | Versión inicial en revisión. |
| 0.2 | 2026-10-03 | PO (AADD) | RESERVAS del Artifact Judge: AUTH-001 reformulada como supuesto con solicitante atribuido (P8, P14); ámbito por defecto del undo en WF-001 (S8); ejemplo de CONS-003 precisado. |
| 0.3 | 2026-10-03 | PO (AADD) | Rene Bonilla cierra P1-P14 (D-TMC-10 a D-TMC-23): sin marcas de supuesto en AUTH-001, WF-001 (ámbito), WF-003, CONS-002, CONS-003 y TIME-001. Siguen como supuesto S5 (redo con solape, WF-001) y S6 (EDGE-004). |
| 0.4 | 2026-10-03 | PO (AADD) | Rene Bonilla acepta S5 y S6: sin marcas de supuesto en WF-001 (redo) y EDGE-004. |
| 0.5 | 2026-10-03 | PO (AADD) | Rene Bonilla acepta TQ-1 a TQ-17 del Arquitecto. CONS-002: lista cerrada de credenciales excluida por defecto, con opción en el perfil, y repos anidados excluidos; ambos declarados y no tocados al restaurar (TQ-16, TQ-15; D-TMC-16 actualizada). AUTH-001: en Windows, rechazo sin confirmación en el MVP (TQ-14; D-TMC-23 actualizada) y rechazo siempre por MCP (TQ-7). WF-001: pila de undo por worktree (TQ-9). CONS-003 y EDGE-002: tope por archivo, cuotas y hueco "sin espacio" (TQ-5). CONS-004: excepción de liberar el bloqueo propio al arrancar. TIME-001: aviso visto + 24 h (TQ-11); `forget` fuera del MVP (TQ-17, D-TMC-24). EDGE-003: fallo a mitad sin rollback automático (TQ-10). |
