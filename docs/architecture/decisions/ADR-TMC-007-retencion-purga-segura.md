---
id: ADR-TMC-007
title: "ADR-TMC-007 — Retención y purga segura de snapshots"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-007, ADR-GRP-008, ADR-TMC-001, ADR-TMC-003]
  stories: [US-TMC-016, US-TMC-017]
description: "Retención timeMachine.retentionDays (perfil y local, 30 días), protección del previo a la última operación destructiva por worktree, purga en dos fases con aviso y liberación de objetos solo en el almacén"
tags: [adr, time-machine, retencion, purga, configuracion, gc, br-tmc-time-001, d-tmc-15]
published: true
---

# ADR-TMC-007 — Retención y purga segura de snapshots

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-5 → (b) cuotas sin purga anticipada; TQ-11 → (a) aviso mostrado en CLI/TUI + 24 h desde que se mostró por primera vez (BR-TMC-TIME-001, D-TMC-25); TQ-17 → (a) `forget` aplazado a una US futura.

## Contexto

BR-TMC-TIME-001 y D-TMC-15: los snapshots se conservan un tiempo configurable, 30 días por defecto, en los niveles perfil y local personal, **no** en el de equipo (Q24). Nunca se purga el snapshot previo a la última operación destructiva y se avisa antes de purgar. Una purga interrumpida no deja puntos a medias (US-TMC-016, escenario 4). La configuración tiene tres niveles, que el motor solo lee, con niveles admitidos por clave (ADR-GRP-007, aún propuesto; local en el perfil por ADR-GRP-008). Los snapshots viven en el almacén privado del perfil (ADR-TMC-001).

**Pregunta**: ¿qué clave se lee, qué es una operación destructiva, qué se protege, cómo se avisa y cómo se liberan los objetos sin riesgo?

## Decisión

### 1. Clave de configuración

- Nueva sección `timeMachine` en el documento de ADR-GRP-007, con tipos en `crates/policy`:

| Clave | Tipo | Niveles admitidos | Por defecto |
|---|---|---|---|
| `timeMachine.retentionDays` | entero, de 1 a 3650 | Perfil y local | 30 |

- Precedencia: local ?? perfil ?? 30. En el nivel de equipo, la clave se ignora con diagnóstico (ADR-GRP-007, "clave en un nivel que no la admite"). Valor de tipo o rango inválido: se aplica la regla de ADR-GRP-007 (PQ-8) y el resultado es el valor por defecto con aviso (US-TMC-017, escenario 4).
- La Time Machine solo lee; el comando para editar es de Guardrails (Q27).

### 2. Operación destructiva

Una operación es **destructiva** si puede dejar contenido fuera del alcance de las refs locales y del working tree:

- Descarta cambios sin commitear de archivos con seguimiento: `reset --hard`, `--merge` o `--keep`; `checkout` o `switch` forzados; restaurar rutas del working tree.
- Borra archivos sin seguimiento: `clean`; quitar un worktree.
- Quita o reescribe refs de forma que un commit queda inalcanzable: borrar una rama, `reset` a algo que no desciende de la punta, `rebase`, `commit --amend`, borrar una ref.
- `stash drop` y `stash clear`.
- Todo undo, redo y restauración.

Para las operaciones protegidas, el tipo de operación declara si es destructiva. Para el Git crudo, se clasifica por el efecto observado (HEAD o rama movida a algo que no desciende, archivos que desaparecen) y por el mensaje del reflog. **Ante la duda, cuenta como destructiva**: protege más.

### 3. Qué se protege

- Por cada worktree y por el estado de refs del repo, el snapshot que precede a la **última** operación destructiva, sea cual sea su nivel. Proteger por worktree es más que lo que exige la regla ("del repo"), y nunca menos.
- Los snapshots previos de operaciones `interrumpida` (ADR-TMC-003 § 6).
- Un snapshot protegido no se purga aunque supere la retención (US-TMC-016, escenario 2).

### 4. Purga en dos fases, solo en el almacén

1. **Anuncio**: el trabajo de purga (al arrancar el daemon y una vez al día) calcula los candidatos: más antiguos que la retención y no protegidos. Si hay alguno, registra un **aviso pendiente** con el número, el periodo y el tamaño, que la CLI y la TUI muestran.
2. **Gracia** (TQ-11 → a): la purga solo se ejecuta cuando el aviso se mostró al menos una vez en la CLI o la TUI **y** pasaron 24 horas desde que se mostró **por primera vez** (BR-TMC-TIME-001, D-TMC-25). El oplog guarda el momento de esa primera entrega. Sin ningún cliente, no se purga (el disco crece antes que perder un punto sin avisar).
3. **Ejecución**: se recalcula la elegibilidad (un candidato que pasó a protegido se salta), se anota la intención en el diario, se borran sus refs del almacén en **una sola transacción** y se anota que se purgaron. La fila del snapshot se conserva como "punto purgado" en el timeline.
4. **Liberación de objetos**: el mantenimiento del almacén (compactar y borrar objetos sin referencias) corre en reposo, con un periodo de gracia para objetos sueltos (⚠️ **ASSUMPTION**: 1 hora, valor de diseño que fija la Dev Spec de TS-TMC-001) que protege las capturas en curso. **El repo del usuario no se toca nunca durante la purga**.
5. **Interrupción** (US-TMC-016, escenario 4): al recuperar, un snapshot con intención de purga y ref presente vuelve a estar disponible; uno con la ref ya borrada pasa a purgado. Como la transacción de refs es atómica, no hay estados intermedios.

### 5. Borrado inmediato y cuotas

- **`forget` no forma parte de este diseño**: aplazado a una US futura del PO (TQ-17 → a). Tal como se propuso, chocaría con D-TMC-15 y BR-TMC-TIME-001 (borra sin aviso y sin respetar el snapshot protegido), ampliaría BR-TMC-CONS-004, se saltaría el periodo de gracia de § 4.4 y, con una ruta, borraría snapshots enteros (NFR-01). Cuando se especifique, su semántica debe reescribir los snapshots sin esa ruta, conservar el resto, respetar las capturas en curso y salir de una US del PO. Mientras tanto, el contenido sensible lo cubre la lista de exclusión de SEC-TMC-06 (TQ-16 → a).
- **Cuotas de disco** (SEC-TMC-12, TQ-5 → b; ⚠️ **ASSUMPTION**: cifras que ajusta SPIKE-TMC-001): al alcanzarlas no se purga antes de tiempo; se detiene la captura por observación con un hueco "sin espacio" (ADR-TMC-004 § 2).

## Alternativas consideradas

| Alternativa | En contra | Veredicto |
|---|---|---|
| **Retención por tamaño** (cuota de disco) | No es lo que decidió D-TMC-15; podría purgar puntos recientes | Descartada en el MVP |
| **Proteger solo el previo a la última destructiva del repo** | Con varios worktrees, una operación en otro worktree dejaría sin proteger el último punto de este | Descartada: se protege por worktree |
| **Purga inmediata con aviso simultáneo** | El aviso llega tarde: no es "antes de purgar" | Descartada (TQ-11 → a) |
| **`gc --prune=now` del almacén** | Puede borrar objetos de una captura en curso sin referencia todavía | Descartada: periodo de gracia |
| **Borrar también las filas del oplog** | El timeline perdería que hubo un punto y cuándo | Descartada |

## Consecuencias

- ✅ La purga solo borra refs del almacén: no puede dañar el repo del usuario ni otros snapshots.
- ✅ El último punto antes de algo destructivo sobrevive siempre, en cada worktree.
- ✅ La retención usa la misma configuración y diagnósticos que el resto del producto.
- ⚠️ Sin clientes abiertos el almacén crece sin límite. Se acepta; el tamaño se expone en diagnóstico.
- ⚠️ Los commits anclados se liberan solo cuando ningún snapshot vivo los referencia; el historial del repo sigue ocupando sitio en el almacén mientras exista algún snapshot (ADR-TMC-001).
- ⚠️ Depende de que ADR-GRP-007 incorpore la sección `timeMachine` (pendiente de integración) y de su paso a `accepted` con Guardrails.

## Validación

1. **Retención**: puntos de 40 y 5 días con 30 de retención; tras aviso y gracia, se purgan los de 40 (US-TMC-016).
2. **Protección**: última destructiva hace 45 días, su previo se conserva; con dos worktrees, cada uno conserva el suyo.
3. **Niveles**: local 7 gana a perfil 14; equipo 2 se ignora con diagnóstico; "mucho" en el perfil da 30 y aviso (US-TMC-017).
4. **Interrupción**: muerte del daemon antes, durante y después de la transacción de refs; todo punto no purgado se restaura bit a bit (INF-TMC-001).
5. **Repo intacto**: huella del repo idéntica antes y después de una purga y del mantenimiento del almacén.

## Referencias

- **Reglas**: BR-TMC-TIME-001; D-TMC-15. Q24, Q27.
- **ADRs**: ADR-GRP-007, ADR-GRP-008; ADR-TMC-001, ADR-TMC-003.
- **Historias**: US-TMC-016 (dueña de la purga), US-TMC-017 (dueña de la configuración). **Seguridad**: SEC-TMC-06, 12.
