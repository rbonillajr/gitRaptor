---
id: ADR-GRD-006
title: Registro de decisiones en el perfil con retención de 90 días
type: adr
status: proposed
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRD-001, ADR-GRD-003, ADR-GRD-005, ADR-GRD-007, CTX-GRD-001, BR-GRD-001]
tags: [guardrails, registro-decisiones, br-cons-004, br-time-002, retencion, kpi, perfil, sqlite, spool, nfr-03]
---

# ADR-GRD-006 — Registro de decisiones en el perfil con retención de 90 días

## Contexto

BR-CONS-004 obliga a anotar:

- **Qué**: cada operación denegada, cada petición de confirmación con su resultado y cada excepción consciente.
- **Con qué campos**: momento, repo, worktree, rama, actor, operación, decisión, regla, nivel y capa.
- **Qué no**: las operaciones permitidas sin regla no se anotan una a una.

El registro es la fuente del KPI "acciones peligrosas bloqueadas", que BR-CONS-004 define como la suma de **denegadas, rechazadas y caducadas** (BRD § 9). Q-GRD-10 lo ubica en el **perfil**, separado por repo y nunca en el repo, y BR-TIME-002 lo conserva **90 días**. BR-WF-002 pide anotar las transiciones del estado de protección, y US-GRD-003 que cada instalación quede con qué, dónde, cuándo y quién la autorizó.

ADR-GRP-006 (propuesto, en `main`) fija un SQLite por repo en el perfil con un único escritor, el daemon. ADR-GRP-013 (propuesto) dice que los eventos son inmutables y tiene una auditoría append-only de comandos reservados. Todo este contenido es confidencial y no sale de la máquina (NFR-03).

## Decisión

**El registro es una tabla propia de Guardrails en el almacén por repo del perfil, separada de los eventos inmutables. El daemon la escribe, con agregación y límite de inserciones, la purga a los 90 días y la consulta por el canal. Las instalaciones, las desinstalaciones, las adopciones y los intentos de comandos reservados van además a la auditoría permanente de ADR-GRP-013.**

### 1. Ubicación y esquema

- **Ubicación**: una tabla `guardrails_decisions` en el almacén SQLite del repo (ADR-GRP-006 § 2), con migraciones versionadas en el binario.
- **El repo sale de la clave del almacén** (J8): no se guarda como campo, porque cada almacén es de un solo repo, identificado por la clave del directorio común.
- **Campos**:

| Campo | Contenido |
|---|---|
| `id`, `at` | Identificador y momento (UTC con desfase local) de la primera ocurrencia |
| `lastAt`, `count` | Agregación de ocurrencias idénticas (L-01, § 2) |
| `worktree`, `branch` | Ruta canónica y rama normalizada (ADR-GRD-002 § 4). Texto no confiable |
| `actor` | Agente (tipo, nombre y origen) o "sin atribuir" (ADR-GRP-013 § 6) |
| `operation` | **Operación normalizada** con sus transiciones y el remoto sin `userinfo`. **Nunca argv** (M-06) |
| `kind` | `denial` \| `request` \| `exception` \| `exception-rejected` \| `exception-cancelled` \| `protection-state` |
| `effect`, `appliedEffect` | Los de ADR-GRD-003 § 3 |
| `reasons` | Lista `{rule, level}`, con parámetros acotados, sin mensajes de commit ni contenido |
| `layer` | `hooks` \| `mcp` \| `guardrails` |
| `requestState` | Para `request`: `pending` → `approved` \| `rejected` \| `expired` (US-GRD-015) |
| `decisionId` | Correlación (ADR-GRD-003 § 6) |
| `origin` | `daemon` \| `spool-unverified` |

- **Qué no se anota**: los permitidos sin regla. Un permitido por excepción sí (`exception`).
- **`exception-cancelled`**: una excepción consciente (`raptor guard exec` o aprobación en el Cockpit) que el humano cancela dentro de la ventana de D5 / D10 (ADR-GRD-007 § 3). Lleva los campos de BR-CONS-004 y **no cuenta en el KPI** (§ 6).

### 2. Agregación y límite de inserciones (L-01)

- **Agregación**: dos ocurrencias son **idénticas** si coinciden en worktree, rama, actor, operación normalizada, `kind` y reglas, dentro de una ventana (⚠️ **ASSUMPTION**: 60 s). En ese caso incrementan `count` y `lastAt` en la misma fila.
- **Conteo**: el KPI cuenta ocurrencias, es decir, suma `count`.
- **Límite de inserciones por repo**: ⚠️ **ASSUMPTION**: 100 filas nuevas por minuto. **Nunca afecta a la decisión**, solo a su registro.
  - **El exceso no se pierde para el KPI** (Judge ronda 2, hallazgo 5): las ocurrencias que superan el límite se acumulan en **contadores agregados por `(kind, operation, rule)`** dentro de una fila `rate-limited` por ventana, con `count`, `at` y `lastAt`. Se pierden el detalle de actor, worktree y rama de esas ocurrencias, pero no su conteo.

### 3. Retención (BR-TIME-002)

- **Plazo**: 90 días desde `lastAt`.
- **Purga**: la hace el daemon al arrancar y cada 24 h (⚠️ **ASSUMPTION**). Las consultas filtran por fecha, así que una entrada vencida no aparece aunque la purga aún no haya pasado.
- **Retirar un repo de la observación** no borra su registro (Q25 de motor-local).
- **No contradice ADR-GRP-013**: estas filas no son eventos ni atribuyen cambios.

### 4. Auditoría permanente (ADR-GRP-013 § 1)

- **Qué entra**:
  - la instalación, la desinstalación, la adopción y el refresco de integridad;
  - cada intento de comando reservado de Guardrails, aceptado o rechazado;
  - cada uso de la excepción.
- **Con qué datos**: la cadena completa de ascendencia, la terminal de control, el líder de sesión y la aceptación de riesgo por acción (D5, ADR-GRD-007).
- **Cómo se guarda**: append-only y sin caducidad. Siempre con la operación normalizada, nunca con argv (M-06).

### 5. Spool del modo degradado (L-02; M-06; H-03)

- **Cuándo**: con el daemon inalcanzable o el canal no auténtico, el cliente del hook escribe la entrada en un spool append-only del directorio de estado del perfil (ADR-GRD-003 § 4).
- **Escritura**:
  - un archivo por entrada, creado en exclusiva y sin seguir enlaces;
  - 0600 dentro de una carpeta 0700;
  - en Windows, sin ACE de escritura de otros;
  - tamaño máximo por entrada y número máximo de archivos;
  - solo campos tipados.
- **Ingesta** (daemon, al arrancar y luego de forma periódica):
  1. Comprueba propietario y modo de la carpeta en cada ingesta.
  2. Abre cada archivo sin seguir enlaces y sin bloquear. Descarta FIFOs y cualquier cosa que no sea un archivo regular.
  3. Valida la entrada y la inserta con `origin = spool-unverified`.
  4. Borra el archivo.
- **Entradas descartadas**: dejan un diagnóstico sin contenido.
- **Ventana degradada** (H-03): el daemon, al volver, registra una entrada `protection-state` con la causa (`daemon-unreachable` o `channel-not-authentic`) y la ventana. Esa ventana la deduce de su propia caída y de las entradas del spool, no solo del spool.
- **Garantías**: el spool tiene la misma retención que el registro y entra en el escaneo de secretos (M-06). No crea excepciones ni peticiones.
- **Contradicción a anotar**: el spool es una escritura de un cliente en el perfil, frente a ADR-GRP-005 § 1 y ADR-GRP-006 § 4. El daemon sigue siendo el único escritor del **almacén**. La excepción está **aplicada (2026-10-04)** en ADR-GRP-005 § 1 y ADR-GRP-006 § 4.

### 6. Consulta (canal)

- **Por repo y periodo**: lista paginada, filtrable por `kind`, `operation`, `actor` y `layer`.
- **KPI "acciones bloqueadas"** (J8; BR-CONS-004): **denegadas** (`denial`), más **rechazadas** y **caducadas** (`request` con `requestState` igual a `rejected` o `expired`), sumando `count`. Incluye las ocurrencias agregadas en las filas `rate-limited`.
  - **Por defecto excluye `origin = spool-unverified`**: un proceso del mismo usuario puede escribir esas entradas, así que no son verificables. Se muestran aparte y se pueden incluir con un filtro explícito. El KPI oficial es el verificado y lo del spool es complementario (Q-GRD-27).
  - `exception-rejected` se informa **aparte**, como intentos de excepción rechazados: es una métrica de seguridad, no del KPI.
  - El uso de `exception` también se informa aparte, como señal de exceso de bloqueo (contexto § 3).
- **Qué se devuelve**: códigos y texto no confiable marcado. Por el MCP, solo el repo del llamante y sin parámetros sensibles.
- **Exportación**: no hay; es BR-24, de la Fase 3.

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| Guardar las decisiones como eventos de ADR-GRP-013 | Los eventos no se borran nunca |
| Un archivo en el repo | Contradice Q-GRD-10 y Q21 de motor-local |
| Un almacén propio de Guardrails | Una segunda base de datos por repo sin ninguna ventaja |
| Descartar la entrada si el daemon no responde | Rompe BR-CONS-004 |
| El cliente del hook escribe directamente en el SQLite | Varios escritores, contradice ADR-GRP-006 |
| Una fila por ocurrencia, sin agregación | Un agente en bucle llena el almacén (L-01) |

## Consecuencias

- ✅ El KPI sale de una consulta local, alineado con BR-CONS-004.
- ✅ La retención y la inmutabilidad conviven.
- ✅ Ni un bucle de denegaciones ni un spool envenenado degradan el almacén.
- ⚠️ **Contradicción** con "el daemon es el único escritor del perfil" por el spool, acotada como excepción y reconocida por motor-local (aplicada, 2026-10-04).
- ⚠️ **Un agente del mismo usuario puede escribir entradas falsas en el spool o borrarlas.** **Mitigación**: validación, topes y `origin` visible; la decisión real nunca depende del spool. Riesgo residual aceptado (SEC-GRD-09).
- ⚠️ La agregación pierde el momento exacto de las ocurrencias intermedias. Conserva el conteo, la primera y la última.

## Validación

1. **Campos**: una denegación de force-push de "codex" en `feat-x` tiene todos los campos de BR-CONS-004, y el repo sale de la clave del almacén (US-GRD-005, escenario 1).
2. **Conteo**: 3 denegaciones esta semana y 1 la anterior → 3. Las peticiones rechazadas y caducadas suman. `exception-rejected` no suma y aparece aparte (US-GRD-005, escenario 2; J8).
3. **Permitidos**: 10 commits permitidos no crean entradas (escenario 3).
4. **No en el repo** (escenario 4; INF-GRP-001).
5. **Retención**: 91 días no aparece y 89 sí (escenario 5).
6. **Agregación** (L-01): 1.000 force-push idénticos en un minuto dan una fila con `count = 1000` y el KPI cuenta 1.000. 500 denegaciones **distintas** en un minuto superan el límite: las que quedan fuera van a contadores `rate-limited` por operación y regla, el KPI cuenta las 500 y las decisiones no cambian.
7. **KPI sin spool** (Judge ronda 2): con 2 denegaciones del daemon y 3 del spool, el KPI por defecto devuelve 2, el desglose muestra 3 `spool-unverified` y el filtro explícito devuelve 5.
8. **Excepción**: `exception` y `exception-rejected` quedan en el registro y en la auditoría (US-GRD-006).
9. **Instalación**: `protection-state` y una entrada de auditoría con la cadena de ascendencia (US-GRD-003, escenario 5).
10. **Spool** (L-02): un enlace, una FIFO, un archivo enorme, una carpeta con otro propietario o en modo 0755 → no se ingiere, no se bloquea y hay diagnóstico. Con el daemon parado, la denegación aparece al volver con `spool-unverified` y con la ventana degradada registrada.
11. **Privacidad** (M-06): un push a una URL con token no deja el token en el registro, la auditoría ni el spool. El escáner de secretos devuelve 0 hallazgos.

## Referencias

- **Reglas**: BR-CONS-004, BR-TIME-002, BR-WF-001, BR-WF-002, BR-CONS-005; Q-GRD-10; NFR-03.
- **Historias**: US-GRD-003, US-GRD-005, US-GRD-006, US-GRD-015.
- **ADRs de otros frentes**: ADR-GRP-005, ADR-GRP-006, ADR-GRP-013 (motor-local, en `main`).
- **Seguridad**: SEC-GRD-09, SEC-GRD-11; SEC-05, SEC-06 y SEC-12 de motor-local.

## Revisión de seguridad (2026-10-04)

| Hallazgo | Cómo se cubre |
|---|---|
| J8 · KPI | § 6: denegadas, rechazadas y caducadas; `exception-rejected` aparte; sin "relajaciones"; § 1: el repo sale de la clave del almacén; Validación 2 |
| L-01 · Inundación del registro | § 2: agregación de ocurrencias idénticas y límite de inserciones por repo; Validación 6 |
| L-02 · Spool | § 5: sin seguir enlaces, sin bloquear, solo archivos regulares, tope de archivos, propietario y modo comprobados en cada ingesta; Validación 9 |
| M-06 · URL y argv | § 1 y § 4: operación normalizada, nunca argv, remoto sin `userinfo`; § 5: el spool con la misma retención y en el escaneo de secretos; Validación 10 |
| H-03 · Ventana degradada | § 5: el daemon registra la ventana, no solo el spool |
| Judge ronda 2, hallazgo 5 · KPI | § 6: excluye `spool-unverified` por defecto (Q-GRD-27); § 2: las ocurrencias por encima del límite cuentan en contadores por `(kind, operation, rule)`; Validación 6 y 7 |
| J13 · Referencias rotas en el frontmatter | `related` solo con IDs existentes |

## Cambios (2026-10-04, coherencia con motor-local)

- § 5 y Consecuencias: la excepción del spool pasa a "aplicada (2026-10-04)" en ADR-GRP-005 § 1 y ADR-GRP-006 § 4; la tabla `guardrails_decisions` y la auditoría de comandos reservados están recogidas en ADR-GRP-006 § 4 y ADR-GRP-013 § 1.
- § 1: nuevo `kind` `exception-cancelled` para una excepción consciente cancelada dentro de la ventana de D5 / D10; lleva los campos de BR-CONS-004 y no cuenta en el KPI.
- Corrección tras el Judge: el KPI verificado, sin ⚠️, queda registrado como Q-GRD-27.
