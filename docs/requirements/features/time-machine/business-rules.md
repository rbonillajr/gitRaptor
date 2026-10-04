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
  stories: []
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

**Criticidad**: Alta

**Regla formal**:
```
ámbito = worktree desde el que se invoca                       (D-TMC-19, D-TMC-23)
IF actor de la última operación ≠ solicitante → BR-TMC-AUTH-001 + BR-TMC-CONS-005
undo:  snapshot previo → restaurar estado anterior a la última operación del ámbito → registrar el undo
redo:  disponible si el último evento del ámbito es un undo
       → snapshot previo → restaurar el estado anterior a ese undo
IF desde el undo hubo cambios de otro actor en los mismos archivos
THEN aplica BR-TMC-CONS-005 (solape): se detiene                  ⚠️ ASSUMPTION (S5)
```

**Ejemplo**: un agente hace `reset --hard` y borra 3 archivos sin commitear → `raptor undo` los recupera → `raptor redo` vuelve al estado tras el reset; el contenido de los 3 archivos sigue recuperable en el timeline.

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

**Descripción** (D-TMC-17, D-TMC-23): GitRaptor no puede probar que una petición viene del humano: un agente puede lanzar `raptor undo` desde su propia shell. Por eso el **solicitante** de un undo, redo o restauración se atribuye igual que los eventos: "agente X" o "sin atribuir", nunca "humano" (Q34). Si el solicitante es un agente, solo deshace operaciones con atribución vigente a él. Si es "sin atribuir", deshacer trabajo de otro actor exige una confirmación interactiva del desarrollador en ese momento, que un agente no puede dar. Guardrails puede restringirlo más, nunca ampliarlo. Cómo se identifica al solicitante (CLI frente a MCP) lo decide el Arquitecto.

**Criticidad**: Alta

**Regla formal**:
```
solicitante ∈ {agente X, "sin atribuir"}                       (nunca "humano", Q34)
IF solicitante = agente X
  THEN permitido solo si todo el conjunto tiene atribución vigente = X; si no, rechazo
IF solicitante = "sin atribuir" AND el conjunto incluye trabajo de otro actor
  THEN pedir confirmación interactiva en ese momento; sin ella, rechazo
IF una política de Guardrails lo prohíbe THEN rechazo
rechazo → el repo no cambia + se informa el motivo
```

**Ejemplo**:
- claude-1 pide vía MCP deshacer un commit de claude-2 → rechazado.
- Claude Code ejecuta `raptor undo` desde su shell y se le atribuye la petición → solo puede deshacer lo suyo.
- Una petición "sin atribuir" en la terminal toca un commit de claude-1 → se pide confirmación interactiva; sin ella, nada cambia.

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

**Descripción**: un snapshot incluye el working tree sin commitear, lo preparado para el próximo commit, los archivos nuevos sin seguimiento y el estado de ramas y worktrees. No incluye los archivos ignorados por `.gitignore` (D-TMC-16).

**Criticidad**: Alta

**Ejemplo**: un agente crea `nuevo.rs` sin añadirlo y edita `lib.rs` → ambos se recuperan. `node_modules/` y `.env`, ignorados, no se copian ni se tocan al restaurar.

### BR-TMC-CONS-003: Cobertura declarada en dos niveles

**Descripción** (D-TMC-9, D-TMC-10; refina BR-08 y NFR-01 del BRD): (a) **Garantizada**: snapshot previo antes de toda operación lanzada por GitRaptor y de cada undo o restauración. (b) **Por observación**: el trabajo hecho con Git crudo o en el editor se captura a medida que el motor observa cambios, y con snapshot previo cuando existan los hooks de Guardrails, sin depender de ellos. Cada punto del timeline indica su nivel. La Time Machine no promete un snapshot previo donde no lo hay.

**Criticidad**: Alta

**Ejemplo**: un agente ejecuta `git reset --hard` con Git crudo sin hooks de Guardrails → el timeline muestra el último estado capturado antes del reset como "capturado por observación", no como "snapshot previo". Lo editado entre esa captura y el reset puede perderse (riesgo R2 del contexto).

### BR-TMC-CONS-004: Las escrituras de la Time Machine son suyas, explícitas y recuperables

**Descripción**: la Time Machine solo escribe para guardar snapshots y para ejecutar un undo, redo o restauración pedido por un actor. Los snapshots no aparecen como cambios del usuario, no se empujan al remoto por accidente, no los borra un `git gc` y no los altera un agente que trabaja en el working tree (D-TMC-11). Ninguna escritura se atribuye al Motor local (Q21).

**Criticidad**: Alta

**Regla formal**:
```
escrituras permitidas = {guardar snapshot, undo, redo, restauración pedida}
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

**Descripción** (D-TMC-15): los snapshots se conservan un tiempo configurable; por defecto 30 días. El valor lo admiten el perfil y la configuración local personal, no la del equipo (Q24). Nunca se purga el snapshot previo a la última operación destructiva. Se avisa antes de purgar.

**Criticidad**: Media

**Regla formal**:
```
retención = local personal ?? perfil ?? 30 días        (equipo: no admitido)
purgable = snapshot más antiguo que la retención AND NOT previo a la última operación destructiva
aviso antes de purgar
```

---

## 8. Reglas Excepcionales (Edge Cases)

### BR-TMC-EDGE-001: Lo deshecho ya está en el remoto

**Descripción**: el undo es solo local. Si lo deshecho ya se empujó, se avisa de que sigue en el remoto. La Time Machine nunca hace push ni force-push por su cuenta (D-TMC-14).

**Criticidad**: Alta

### BR-TMC-EDGE-002: Huecos de observación en el timeline

**Descripción**: el timeline muestra cada hueco de forma explícita. Lo ocurrido en un hueco es "sin atribuir" (BR-EDGE-005 de motor-local), nunca entra en un undo por agente y no ofrece puntos de restauración que no tengan snapshot.

**Criticidad**: Alta

### BR-TMC-EDGE-003: Interrupción a mitad de un snapshot o de un undo

**Descripción**: si el proceso muere durante un snapshot, ese snapshot no cuenta como punto válido y el repo no cambió. Si muere durante un undo o restauración, el repo queda recuperable al estado previo gracias al snapshot que lo precede, y al volver se informa de lo ocurrido (NFR-12).

**Criticidad**: Alta

### BR-TMC-EDGE-004: Operación de Git en curso

**Descripción**: ⚠️ **ASSUMPTION** (S6). Con un rebase o un merge a medias, el undo y la restauración se detienen y piden terminar o abortar esa operación antes; el repo no cambia.

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
