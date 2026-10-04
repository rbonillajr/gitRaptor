---
id: ADR-GRP-013
title: Modelo persistido de eventos, sesiones y atribución
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
deciders: [Rene Bonilla]
related: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-012, CTX-GRP-001, BR-GRP-001]
tags: [motor-local, eventos, sesiones, atribucion, correccion, huecos, append-only, modelo-de-datos]
---

# ADR-GRP-013 — Modelo persistido de eventos, sesiones y atribución

## Contexto

El motor guarda en el perfil (ADR-GRP-006) los eventos de Git, las sesiones de agente, los registros explícitos y las correcciones. Las reglas de negocio fijan cómo se comportan:

- El motor solo emite "agente X" con su origen ("detectado" o "registrado") o "sin atribuir". **Nunca "humano"** (Q34) ni "no identificada" (Q35), y ante la duda no atribuye (BR-EDGE-004).
- **Corregir** reemplaza la atribución detectada de una sesión, no añade sesiones (Q33) y alcanza todos los eventos de esa sesión **desde su inicio**, sin tocar los de otras sesiones (Q37). Solo se corrige donde hay una atribución detectada (Q38). Una detección posterior no deshace una corrección vigente (BR-CONS-002).
- **Registrar otro agente** añade una sesión y el worktree pasa a compartido; registrar al mismo agente ya detectado **confirma** esa sesión (Q39, BR-CONS-004).
- Una sesión terminada no se reactiva (Q41).
- Lo que el motor no vio ocurrir queda "sin atribuir", aunque hubiera un agente registrado antes del hueco (BR-EDGE-005).
- Todo sobrevive a reinicios (Q6, BR-CONS-005).
- Abiertas para el PO: **P16** (¿una sesión confirmada por registro pasa a origen "registrado"?) y **P17** (¿al retirar una corrección, sus eventos vuelven a la atribución detectada?).

## Decisión

**Cada evento apunta a una sesión o a ninguna ("sin atribuir"). La atribución se resuelve desde la sesión, a partir de su atribución inicial y de una cadena append-only de registros de atribución. Los eventos nunca se reescriben ni se borran.**

### 1. Entidades (por repo, en el almacén del repo de ADR-GRP-006)

| Entidad | Qué guarda |
|---------|------------|
| **Worktree** | Ruta canónica, nombre administrativo en Git, primera vez visto y, si desapareció, cuándo. |
| **Sesión** | Worktree, agente (Claude Code u "otro agente" con su nombre declarado), atribución inicial y su origen (detectada o registrada), clave de detección de ADR-GRP-012 si se detectó, inicio, fin y causa del fin. |
| **Registro de atribución** | Append-only. Sesión, tipo (confirmación, corrección, retiro de corrección), agente, autor (desarrollador o agente, como exige la tabla de BR-CONS-001), momento y secuencia en la que entra en vigor. |
| **Evento** | Secuencia por repo, worktree, tipo, metadatos (refs, ids de commit, rutas afectadas), hora de observación en UTC con el desfase de zona horaria local, sesión (opcional), evidencia de atribución (qué señales de ADR-GRP-012 la sustentan) y hueco (opcional). |
| **Hueco** | Intervalo de inicio y fin y su causa: máquina apagada o suspendida, daemon caído, repo retirado, Git ausente o insuficiente (BR-WF-002), perfil perdido o almacén corrupto. |
| **Último estado conocido** | Por worktree: HEAD, puntas de refs, operación en curso y huella de los cambios sin commitear. Es la base de la reconciliación (ADR-GRP-010). |
| **Marca "observado hasta"** | Momento hasta el que el repo estuvo observado, persistido con cada lote y de forma periódica. |

Los cambios de estado de las sesiones (inicio, activo, inactivo, terminado) se guardan como eventos del mismo historial, así que su estado sobrevive a reinicios (BR-WF-001).

### 2. Resolución de la atribución

- **Actor de un evento**: si no tiene sesión, "sin atribuir". Si la tiene, la atribución efectiva de esa sesión.
- **Atribución efectiva de una sesión**: se parte de la atribución inicial y se aplican en orden los registros vigentes:
  - **Confirmación** (registro del mismo agente ya detectado, Q39): misma sesión y mismo agente. ⚠️ **ASSUMPTION** dependiente de **P16** (supuesto: sí): el origen pasa a "registrado".
  - **Corrección**: agente corregido con origen "registrado". Como todos los eventos de la sesión apuntan a ella, la corrección alcanza **desde el inicio de la sesión** (Q37) y no toca los eventos de otras sesiones. No crea sesiones ni marca el worktree como compartido (Q33).
  - **Retiro de corrección**: deja sin efecto la corrección retirada y vuelve la atribución anterior. ⚠️ **ASSUMPTION** dependiente de **P17** (supuesto: sí): los eventos que la corrección había reatribuido vuelven también, porque se resuelven desde la sesión.
- **Precondiciones de la corrección** (las aplica el daemon): la pide el desarrollador (BR-AUTH-001, controles de ADR-GRP-005) y la sesión tiene atribución inicial detectada; si en el worktree no hay ninguna, se rechaza e indica que se use el registro (Q38).
- **La detección no escribe registros**: actualizar la presencia o la actividad de una sesión detectada no cambia su atribución, así que una detección posterior no deshace una corrección vigente (BR-CONS-002).
- **Si el PO responde "no" a P16 o P17**, el modelo no cambia de forma: la confirmación deja el origen como está (P16), y el retiro de una corrección solo vale para los eventos con secuencia posterior al retiro, gracias a la secuencia de entrada en vigor de cada registro (P17).

### 3. Asignación de sesión a un evento

- Un evento solo apunta a una sesión con **evidencia positiva** de ADR-GRP-012 (proceso, ascendencia, contenido de sesión o hooks existentes). La co-ubicación sola no basta. Ante la duda, sin sesión (BR-EDGE-004).
- En un **worktree compartido** solo se asigna una sesión si la evidencia identifica una sola. En el MVP no hay atribución por archivo dentro de un worktree compartido (Q7, BR-CONS-004).
- Que un worktree sea compartido se deriva de cuántas sesiones presentes tiene; no se guarda como dato propio.
- Una sesión terminada no se reabre: si el agente vuelve, es una sesión nueva (Q41).

### 4. Orden, tiempo e inmutabilidad

- **Secuencia monotónica por repo**, asignada por el único escritor (ADR-GRP-005). El orden lo da la secuencia, no el reloj, para resistir saltos de hora.
- La hora de observación se guarda en UTC con el desfase local. Las fechas propias de Git (autor y commit) se guardan como metadatos, sin usarlas para ordenar.
- **Los eventos nunca se borran ni se reescriben**, tampoco al retirar un repo (Q25). Las correcciones y sus retiros quedan como rastro en los registros de atribución.
- Solo se registran como eventos los cambios ocurridos desde que el repo se observa: la historia previa de Git no se importa como eventos.

### 5. Huecos y reconciliación

- Al arrancar, al volver de suspensión, al volver a añadir un repo y al salir de "Esperando Git", el motor abre un hueco desde la marca "observado hasta" hasta ese momento, con su causa.
- La reconciliación (ADR-GRP-010) compara el estado actual con el último estado conocido y genera **eventos de reconciliación sin sesión y enlazados al hueco**. Nunca se atribuyen a un agente, aunque hubiera uno registrado antes (BR-EDGE-005).
- Sin último estado conocido (perfil perdido, almacén corrupto o máquina nueva), el repo expone "observado desde" su fecha de alta y todo lo anterior queda "sin atribuir" (Q26, Q31).
- Una sesión cuyo proceso desapareció durante un hueco se cierra con causa "terminada durante un hueco" y hora de fin desconocida dentro del intervalo.

### 6. Lo que expone el contrato (`crates/api`)

- El actor expuesto solo tiene dos variantes: **agente** (tipo, nombre y origen) o **sin atribuir**. El tipo no tiene variante "humano", así que el motor no puede emitirla.
- Al aplicar una confirmación, una corrección o un retiro, el stream de eventos publica que la atribución de esa sesión cambió, con el rango de secuencias afectado, para que la Time Machine (F-001-03) y Guardrails (F-001-04) refresquen.
- Consultas por worktree, por sesión, por rango de secuencia y por actor efectivo.

## Alternativas consideradas

| Alternativa | Por qué no |
|-------------|------------|
| **Atribución copiada en cada evento; una corrección reescribe los eventos afectados** | Mutación masiva en cada corrección, sin rastro de la corrección, y retirar una corrección obliga a recordar qué valía antes cada evento. |
| **Event sourcing completo con proyecciones** | Más potente, pero desproporcionado para el MVP: reconstruir proyecciones a escala de NFR-05 añade complejidad sin un caso de uso que lo pida. Se puede llegar a él más tarde, porque los registros ya son append-only. |
| **Atribución por cambio sin sesión** (cada evento con su agente y su origen) | Q37 obliga a encontrar todos los eventos de una sesión; sin sesión como entidad, una corrección no sabe hasta dónde llegar. |

## Consecuencias

- ✅ Q37 se cumple por construcción: una corrección es un registro, no una reescritura, y alcanza toda la sesión.
- ✅ Retirar una corrección es inmediato y deja rastro; el modelo admite las dos respuestas posibles a P16 y P17.
- ✅ "Humano" no se puede emitir porque el contrato no tiene esa variante (Q34).
- ✅ Los huecos quedan señalados y sus cambios nunca se atribuyen a un agente (BR-EDGE-005); la Time Machine sabe qué periodos no tienen atribución.
- ✅ La evidencia guardada por evento permite medir la precisión de la detección (SPIKE-GRP-001).
- ⚠️ **Depende de P16 y P17**, abiertas para el PO. **Mitigación**: el modelo cubre ambas respuestas cambiando solo la regla de resolución, no los datos.
- ⚠️ BR-CONS-002 no dice qué pasa al retirar una corrección **si la sesión ya terminó** ("si la sesión sigue presente, vuelve la atribución detectada"). ⚠️ **ASSUMPTION**: se resuelve igual que con la sesión presente. **Pendiente para el PO**.
- ⚠️ En un worktree compartido, los cambios de archivos quedan casi siempre "sin atribuir" en el MVP, porque no hay atribución por archivo (Q7). **Mitigación**: los eventos de Git con evidencia de una sola sesión (por ejemplo, un commit por ascendencia de procesos) sí se atribuyen.
- ⚠️ Resolver el actor exige un cruce entre evento y sesión. **Mitigación**: índices por sesión y por worktree, y la atribución efectiva de cada sesión en memoria del daemon; se mide en INF-GRP-002 dentro del presupuesto de ADR-GRP-011.
- ⚠️ El historial crece sin límite (los eventos no se borran). La retención queda fuera del MVP (ver ADR-GRP-006).

## Validación

Pruebas con repos y perfiles temporales; las sesiones se simulan con la interfaz de detección de ADR-GRP-012.

1. **Alcance de la corrección** (US-GRP-010): una sesión detectada con commits `c1` y `c2` y una sesión anterior terminada con `c0`; tras corregir, `c1` y `c2` resuelven al agente corregido con origen "registrado" y `c0` no cambia; el worktree sigue con una sola sesión.
2. **Retiro** (P17): retirar la corrección devuelve `c1` y `c2` a la atribución detectada.
3. **Rechazos**: corregir en un worktree sin atribución detectada se rechaza (Q38); una corrección pedida por un agente se rechaza (BR-AUTH-001).
4. **Confirmación** (US-GRP-009): registrar a Claude Code donde ya se detectaba no crea una segunda sesión ni marca compartido; el origen pasa a "registrado" (P16).
5. **Compartido** (US-GRP-011): registrar otro agente añade una sesión y el worktree se reporta compartido; un cambio de archivo sin evidencia de una sola sesión queda "sin atribuir".
6. **Huecos** (US-GRP-005): con un agente registrado, matar el daemon, hacer dos commits y relanzarlo; los dos aparecen como eventos de reconciliación "sin atribuir" enlazados a un hueco con su intervalo.
7. **Persistencia** (US-GRP-004): tras reiniciar el daemon, correcciones, sesiones, estados y eventos siguen iguales.
8. **Sin "humano"**: el esquema del contrato no contiene esa variante; una prueba de propiedades sobre secuencias aleatorias de registros comprueba que todo actor resuelto es un agente con origen o "sin atribuir".
9. **Orden**: con la hora del sistema retrasada a mitad de prueba, el orden de los eventos sigue la secuencia.
10. **Repo intacto**: todo lo anterior pasa por el arnés de INF-GRP-001.

## Referencias

- **Reglas**: BR-CONS-002, BR-CONS-003, BR-CONS-004, BR-CONS-005, BR-WF-001, BR-AUTH-001, BR-EDGE-003, BR-EDGE-004, BR-EDGE-005.
- **Historias**: US-GRP-002, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011.
- **Decisiones del context**: Q6, Q7, Q25, Q26, Q31, Q33, Q34, Q35, Q36, Q37, Q38, Q39, Q41; preguntas abiertas P16 y P17.
- **ADRs**: ADR-GRP-005 (único escritor, controles del canal), ADR-GRP-006 (almacén por repo), ADR-GRP-010 (reconciliación), ADR-GRP-012 (señales y evidencia).
- **Enablers**: TS-GRP-001, SPIKE-GRP-001, INF-GRP-001, INF-GRP-002.
- **Features consumidoras**: F-001-03 Time Machine y F-001-04 Guardrails.
