---
id: BR-GRP-001
title: "Reglas de Negocio — Motor local"
type: business-rules
status: draft
created: 2026-10-01
updated: 2026-10-07
domain: GRP
epic: E-001
feature: motor-local
related:
  context:
    - CTX-GRP-001
  stories:
    - US-GRP-001
    - US-GRP-002
    - US-GRP-003
    - US-GRP-004
    - US-GRP-005
    - US-GRP-006
    - US-GRP-007
    - US-GRP-008
    - US-GRP-009
    - US-GRP-010
    - US-GRP-011
    - US-GRP-012
    - US-GRP-013
    - US-GRP-014
    - US-GRP-015
    - US-GRP-016
    - US-GRP-020
    - US-GRP-021
    - US-GRP-022
    - US-CKP-025
tags:
  - motor-local
  - atribucion
  - estados-sesion
  - deteccion-agentes
  - solo-observacion
  - configuracion-tres-niveles
  - primer-uso
  - requisitos-entorno
  - repos-descubiertos
---

# Reglas de Negocio: Motor local

> **Propósito**: Documentar las reglas que gobiernan qué observa el motor, cómo atribuye un worktree a un agente, en qué estado está cada sesión de agente, qué garantiza el motor sobre los repos que observa (no escribe nada en ellos), dónde guarda sus datos, cómo lee su configuración y qué hace cuando la máquina todavía no tiene lo que necesita (Git, agentes, repos).
>
> **Nota de nomenclatura**: los ids `BR-01`, `BR-02` y `BR-03` del BRD son **capacidades** de alto nivel. Las reglas de este documento usan la forma `BR-<CAT>-NNN` (p. ej. `BR-WF-001`) y son internas a esta feature.

---

## Contexto

**Feature**: Motor local (F-001-01)
**Enlace a contexto**: [`context.md`](./context.md) (CTX-GRP-001)
**Última actualización**: 2026-10-04 (decisiones heredadas de Guardrails Q-GRD-20 y Q-GRD-21: BR-CONS-006 calcula contra la rama base confirmada, leída de la copia conocida de la rama principal; BR-EDGE-007 marca la rama base como "no confirmada" en una máquina nueva hasta que el humano la confirma al instalar la protección de Guardrails o de forma explícita, Q-GRD-23). Antes, 2026-10-03 (Q33-Q36: corregir reemplaza y registrar otro agente añade sesión; el motor nunca emite "humano"; "sin atribuir" es el único valor para lo no identificado; la rama base del equipo se difiere a US-GRP-016). También el 2026-10-03 (Q32: soporte completo solo para Claude Code en el MVP; Codex y Cursor como "otro agente" hasta su integración; el editor del humano nunca se atribuye a un agente). Antes, 2026-10-02 (decisiones Q1-Q31 de Rene Bonilla; Q28-Q31 fijan el primer uso: Git ausente o antiguo, agente instalado después, sin repos y máquina nueva; Q10 y Q11 reemplazan a Q2; Q21 reemplaza a Q18 y Q19; Q22 acota BR-AUTH-002 a un principio de frontera; Q23 añade BR-CONS-007; Q24 fija qué niveles admite cada valor; Q25 y Q26 confirman los supuestos S16 y S17; Q27 asigna a Guardrails el comando de edición de la configuración)

---

## Categorías de Reglas

| Categoría | Cantidad | Críticas |
|-----------|----------|----------|
| Validaciones de Datos | 3 | 2 |
| Cálculos de Negocio | 0 | 0 |
| Reglas de Elegibilidad | 0 | 0 |
| Workflows y Estados | 2 | 2 |
| Permisos y Autorizaciones | 3 | 3 |
| Reglas de Consistencia de Datos | 7 | 4 |
| Reglas de Tiempo y Expiración | 1 | 0 |
| Reglas Excepcionales (Edge Cases) | 7 | 3 |
| **Total** | **23** | **14** |

"Críticas" = reglas con criticidad Alta.

---

## 1. Validaciones de Datos

### BR-VAL-001: Todo agente registrado se acepta; el nivel de soporte depende de su tipo

**Descripción**: Los agentes se integran en GitRaptor de uno en uno, empezando por los más usados. En el MVP, **solo Claude Code** tiene **soporte completo** (detección automática y registro explícito, D2 revisada, Q32). Cualquier otro agente (Codex, Cursor, Copilot u otro) **se acepta** al registrarse de forma explícita como **"otro agente"**, con el nombre que declare: el motor lo observa y le atribuye su actividad igual que a los demás, pero sin detección automática ni funciones específicas de ese agente. El orden de integración es: primero Codex, luego Cursor y, más adelante, Copilot (BRD v0.5, Q32).

> **Cambio Q32 (2026-10-03)**: antes decía "Claude Code y Cursor tienen soporte completo; los siguientes son Codex y Copilot". Cursor pasa a "otro agente" como agente; como editor del humano, ver BR-EDGE-004.

**Por qué se acepta en lugar de rechazarse**: si el registro se rechazara, la actividad de ese agente quedaría "sin atribuir", indistinguible del trabajo del desarrollador (Q34, Q35), y un deshacer de la Time Machine podría alcanzar trabajo que no corresponde.

**Aplicabilidad**: Al registrar un agente en un worktree, lo haga el desarrollador o el propio agente.

**Criticidad**: Alta

**Regla formal**:
```
IF tipo de agente = Claude Code        (Q32; antes: ∈ {Claude Code, Cursor})
THEN se acepta el registro con soporte completo
ELSE se acepta el registro como "otro agente" con el nombre declarado
     (observación y atribución sí; detección automática y funciones específicas no)
```

**Ejemplo**:
- **Soporte completo**: registrar "Claude Code" en el worktree `feat-login` → sesión de Claude Code, origen "registrado".
- **Otro agente**: registrar "Codex" en el worktree `feat-login` → sesión de "otro agente: Codex", origen "registrado"; su actividad en `feat-login` se le atribuye a él y deja de quedar "sin atribuir".
- **Cursor registrado como agente** (Q32): registrar "Cursor" en `feat-pagos` → sesión de "otro agente: Cursor", origen "registrado". Sin ese registro, lo que se hace en Cursor queda "sin atribuir" (BR-EDGE-004).
- **Mensaje informativo**: "Codex se registró como otro agente: se observa y se le atribuye su actividad, sin funciones específicas."

**Referencias**:
- User Story: US-GRP-009
- BRD: BR-02, D2 (revisada en v0.5), § 11
- Contexto: decisiones Q4 y Q32

---

### BR-VAL-002: El registro apunta a un worktree existente de un repo observado

**Descripción**: Un agente solo se puede registrar en un worktree que existe y que pertenece a un repo que el desarrollador está observando.

**Aplicabilidad**: Al registrar un agente.

**Criticidad**: Alta

**Regla formal**:
```
IF el worktree existe
   AND pertenece a un repo observado
THEN se acepta el registro
ELSE se rechaza indicando el motivo (worktree inexistente / repo no observado)
```

**Ejemplo**:
- **Input válido**: worktree `../repo-wt/feat-login` de un repo observado.
- **Input inválido**: un directorio que no es un worktree, o un worktree de un repo no añadido.
- **Mensaje de error**: "Ese directorio no es un worktree de ningún repo observado."

**Referencias**:
- User Story: US-GRP-009, US-GRP-010
- Relacionada: BR-AUTH-001

---

### BR-VAL-003: Git del sistema ausente o insuficiente: el motor avisa y espera

**Descripción**: El motor necesita el Git del sistema en la versión mínima que fija NFR-07 del BRD: **2.38 o superior**. GitRaptor se instala igualmente aunque la máquina no tenga Git, o tenga uno anterior, porque en una computadora nueva es normal instalar GitRaptor antes que el resto de herramientas de desarrollo (Q28). Si Git **no está instalado**, o su versión es **inferior a 2.38**, el motor informa con claridad qué falta, qué versión necesita y cómo resolverlo, y **no observa nada** hasta que el requisito se cumpla: no hay observación parcial con las capacidades que no exigen esa versión. Cuando Git aparece o se actualiza, el motor lo detecta por sí solo y empieza a observar, sin reinstalar ni reconfigurar GitRaptor (BR-WF-002).

**El motor nunca instala ni actualiza Git** (Q28): hacerlo sería modificar la máquina del usuario fuera del perfil de GitRaptor, algo que el motor no hace ni siquiera con permiso (Q17, Q21, BR-CONS-001). El motor dice cómo resolverlo; quien instala o actualiza Git es el desarrollador.

**Aplicabilidad**: Al arrancar el motor y mientras espera a que el requisito se cumpla, en Windows, macOS y Linux.

**Criticidad**: Media

**Regla formal**:
```
IF no hay Git del sistema
THEN no se observa nada; se informa que falta Git, la versión mínima (2.38) y cómo instalarlo
ELSE IF la versión de Git < 2.38
THEN no se observa nada; se informa la versión encontrada, la mínima y cómo actualizarlo
ELSE se observa con normalidad
Constraint: el motor no instala ni actualiza Git en ningún caso
Constraint: cuando el requisito pasa a cumplirse, la observación empieza sola (BR-WF-002)
```

**Ejemplo**:
- **Válido**: Git 2.45 → se observa con normalidad.
- **Git ausente**: máquina nueva sin Git → "GitRaptor necesita Git 2.38 o superior y no lo encuentra en este equipo. Instálalo con el instalador oficial o el gestor de paquetes de tu sistema; GitRaptor empezará a observar en cuanto lo detecte, sin reinstalar nada."
- **Git antiguo**: Git 2.34 → "GitRaptor necesita Git 2.38 o superior; se encontró 2.34. Actualiza Git y GitRaptor empezará a observar en cuanto detecte la versión nueva."
- **Sin observación parcial**: con Git 2.34, ningún repo se observa, aunque alguna capacidad funcionara con esa versión.

**Git que deja de cumplir mientras se observa** (supuesto S19, aceptado por Rene Bonilla el 2026-10-02; cierra P13): si Git desaparece o pasa a una versión inferior a 2.38 mientras el motor ya observaba (p. ej. el desarrollador lo desinstala o lo cambia por otro), el motor vuelve a esperar con el mismo aviso, y lo ocurrido mientras esperaba se trata como un hueco de observación ("sin atribuir", BR-EDGE-005).

**Referencias**:
- User Story: US-GRP-014
- BRD: NFR-07
- Contexto: decisiones Q17, Q21 y Q28 (cierra P4)
- Relacionada: BR-WF-002, BR-CONS-001, BR-EDGE-005

---

## 2. Cálculos de Negocio

No aplica en esta feature: el motor no calcula importes, totales ni fórmulas de negocio. Ahead/behind es información que Git ya reporta, y la meta de precisión de la detección (90%) es un KPI que se mide en dogfooding, no un cálculo que el sistema haga (ver § 3 del contexto).

---

## 3. Reglas de Elegibilidad

No aplica en esta feature: no hay condiciones que determinen si un usuario o una entidad califica para una funcionalidad. Qué se puede observar y quién puede registrar lo cubren BR-AUTH-001 y BR-VAL-001 / BR-VAL-002.

---

## 4. Workflows y Estados

### BR-WF-001: Estados de una sesión de agente

**Descripción**: Cada sesión de agente asociada a un worktree está en uno de tres estados, que el Cockpit muestra (BR-04 del BRD). Aplica igual a Claude Code y a "otro agente" (Q32: antes, "a Claude Code, Cursor y otro agente").

**Criticidad**: Alta

**Estados posibles**:
1. **Activo**: el agente está presente y hubo actividad en su worktree dentro del umbral de inactividad (BR-TIME-001).
2. **Inactivo**: el agente sigue presente pero no hubo actividad en su worktree durante el umbral de inactividad.
3. **Terminado**: el agente ya no está presente, o su registro explícito se retiró.

"Presente" significa que el motor detecta la sesión del agente, o que el agente está registrado de forma explícita y su registro no se ha retirado. "Actividad" significa cualquier cambio en los archivos del worktree o cualquier evento de Git en él.

**Transiciones permitidas**:

```
(ninguno) → Activo
  Condición: el motor detecta una sesión nueva o alguien registra el agente
  Actor: Sistema (detección) o desarrollador / agente (registro)
  Acción: la sesión aparece asociada a su worktree con su origen (detectado o registrado)

Activo → Inactivo
  Condición: pasa el umbral de inactividad sin actividad en el worktree
  Actor: Sistema (automático)

Inactivo → Activo
  Condición: vuelve a haber actividad en el worktree
  Actor: Sistema (automático)

Activo / Inactivo → Terminado
  Condición: el agente deja de estar presente, o se retira su registro explícito
  Actor: Sistema (detección) o desarrollador / agente (retiro del registro)
```

**Diagrama de estados**:
```
(ninguno) → [Activo] ⇄ [Inactivo]
                ↓           ↓
             [Terminado] ←──┘
```

**Reglas adicionales**:
- Una sesión "Terminado" no vuelve a "Activo": si el mismo agente reaparece en el mismo worktree, es una **sesión nueva** (Q41, confirma el supuesto S8).
- Un agente registrado de forma explícita cuya presencia el motor no puede comprobar (el caso normal de "otro agente") se considera presente hasta que se retire el registro (Q41, confirma el supuesto S8).
- El estado de cada sesión sobrevive a reinicios (BR-CONS-005).
- Cuánto tiempo sigue visible una sesión "Terminado" lo decide el Cockpit, no el motor.

**Referencias**:
- User Story: US-GRP-007, US-GRP-009
- BRD: BR-02, BR-04

---

### BR-WF-002: Disponibilidad del motor según el entorno: esperando Git, sin repos u observando

**Descripción**: Recién instalado en una máquina nueva, el motor puede encontrarse sin Git, sin repos que observar o sin agentes. Cada situación tiene un estado propio que el motor expone, para que el desarrollador sepa siempre qué le falta y las superficies (Cockpit, CLI, MCP) lo presenten. El motor pasa de un estado a otro **por sí solo**, sin reinstalar ni reconfigurar GitRaptor (Q28, Q30). La ausencia de agentes no es un estado del motor: sin agentes, el motor observa igual (BR-EDGE-006).

**Criticidad**: Alta

**Estados posibles**:
1. **Esperando Git**: no hay Git del sistema o su versión es inferior a 2.38 (BR-VAL-003). No se observa nada. El motor expone qué falta, qué versión se necesita y cómo resolverlo.
2. **Sin repos**: Git cumple el requisito, pero el desarrollador todavía no ha añadido ningún repo, o los ha retirado todos. El motor expone que no hay repos observados y cómo añadir el primero, para que la superficie ofrezca un estado vacío guiado (Q30).
3. **Observando**: Git cumple el requisito y hay al menos un repo observado. Es el funcionamiento normal del resto de reglas.

El estado "Esperando Git" tiene prioridad sobre "Sin repos": mientras falte Git, el aviso es el de Git.

**Transiciones permitidas**:

```
(arranque) → Esperando Git | Sin repos | Observando
  Condición: según el Git encontrado y los repos anotados en el perfil
  Actor: Sistema

Esperando Git → Sin repos | Observando
  Condición: el motor detecta que Git ya está instalado o actualizado a 2.38 o superior
  Actor: Sistema (automático, sin reinstalar ni reconfigurar)
  Acción: empieza a observar los repos anotados en el perfil, si los hay

Sin repos → Observando
  Condición: el desarrollador añade su primer repo
  Actor: Desarrollador

Observando → Sin repos
  Condición: el desarrollador retira el último repo observado
  Actor: Desarrollador

Observando | Sin repos → Esperando Git
  Condición: Git desaparece o pasa a una versión inferior a 2.38
  Actor: Sistema (supuesto S19, aceptado)
```

**Diagrama de estados**:
```
[Esperando Git] ──Git listo──→ [Sin repos] ──primer repo──→ [Observando]
       ↑                            ↑  ←──último repo retirado──┘
       └──────Git ya no cumple (S19)─┴──────────────────────────┘
```

**Reglas adicionales**:
- El motor nunca instala ni actualiza Git para salir de "Esperando Git" (BR-VAL-003).
- Ningún estado requiere tener un agente instalado (BR-EDGE-006).
- Cómo se presenta cada estado (texto, diseño del estado vacío) lo deciden el Cockpit y la CLI; el motor expone el estado y lo que falta.
- Mientras el motor está en "Esperando Git" no se pueden añadir repos, porque sin Git el motor no puede comprobar que un directorio es un repo (BR-VAL-002); el desarrollador los añade cuando Git ya cumple el requisito (supuesto S18, aceptado por Rene Bonilla el 2026-10-02; cierra P13).
- **Enmienda (2026-10-07)**: un repo descubierto (BR-AUTH-003) no cuenta como observado. Con repos descubiertos y ninguno aceptado, el estado sigue siendo "Sin repos"; la guía para añadir el primero puede ofrecer los descubiertos.

#### Cómo se verifica (escenarios de verificación)

| Escenario | Situación de partida | Resultado esperado |
|-----------|----------------------|--------------------|
| Máquina nueva con GitRaptor instalado antes que Git | Computadora recién estrenada, sin Git; se instala GitRaptor | La instalación termina bien. El motor queda en "Esperando Git", informa que falta Git, la versión mínima y cómo instalarlo, y no observa nada. No instala Git ni modifica nada fuera del perfil |
| Git instalado después | El motor está en "Esperando Git" por falta de Git; el desarrollador instala Git 2.45 | El motor lo detecta por sí solo y pasa a "Sin repos" (o a "Observando" si ya había repos en el perfil), sin reinstalar ni reconfigurar GitRaptor |
| Git antiguo actualizado | El motor está en "Esperando Git" con Git 2.34; el desarrollador lo actualiza a 2.45 | El motor detecta la versión nueva por sí solo y empieza a observar los repos anotados en el perfil, sin reinstalar ni reconfigurar. Mientras tuvo 2.34 no observó ningún repo |
| Claude Code instalado después | Git cumple el requisito, hay un repo observado y no hay ningún agente instalado; el desarrollador instala Claude Code y lo lanza en un worktree | Antes de instalarlo, el motor observaba el repo y sus worktrees sin sesiones de agente. Después, detecta la sesión de Claude Code en ese worktree sin reinstalar ni reconfigurar GitRaptor (BR-EDGE-006) |
| Sin repos | Git cumple el requisito y el perfil no tiene ningún repo (p. ej. recién instalado) | El motor queda en "Sin repos" y expone que no hay repos observados y cómo añadir el primero. Al añadirlo, pasa a "Observando" |

**Referencias**:
- User Story: US-GRP-014, US-GRP-015
- BRD: NFR-06, NFR-07
- Contexto: decisiones Q28, Q29 y Q30; supuestos S18 y S19 (aceptados, P13 cerrada)
- Relacionada: BR-VAL-003, BR-AUTH-001, BR-EDGE-006, BR-EDGE-007

---

## 5. Permisos y Autorizaciones

### BR-AUTH-001: Solo se observan los repos que el desarrollador añadió

**Descripción**: El motor observa únicamente los repos que el desarrollador añadió de forma explícita. Solo descubre repos dentro de las carpetas de código que el desarrollador declaró, y descubrir nunca es observar (BR-AUTH-003) *(enmienda 2026-10-07: antes, "No descubre repos por su cuenta")*. Un agente no puede ampliar ese conjunto. Añadir un repo no crea nada en él: el repo queda anotado en el perfil de GitRaptor (Q21, BR-CONS-001). Retirarlo deja de observarlo.

**Retirar no borra** (Q25, confirma el supuesto S16): retirar un repo de la observación no borra sus datos del perfil (registro de agentes, atribuciones, historial de eventos, estado de sesiones). Si el desarrollador vuelve a añadir ese repo, esos datos vuelven a estar disponibles. Lo ocurrido en el repo mientras estuvo retirado no se observó y se trata como un hueco de observación: queda "sin atribuir" (BR-EDGE-005).

**Aplicabilidad**: Añadir o retirar repos observados; registrar agentes.

**Criticidad**: Alta

**Regla de autorización**:
```
Actor: Desarrollador orquestador
Puede: Añadir y retirar repos observados; editar la configuración en cualquiera de sus tres niveles
Sobre: Cualquier repo Git de su máquina
Cuando: Siempre

Actor: Agente (Claude Code u otro agente, incluidos Codex y Cursor; Q32)
Puede: Registrarse en un worktree de un repo ya observado
No puede: Añadir ni retirar repos observados (Q40); corregir una atribución (BR-CONS-002)
```

**Tabla de permisos**:

| Rol | Acción | Permitido | Condiciones |
|-----|--------|-----------|-------------|
| **Desarrollador** | Añadir / retirar repo observado | ✅ | Queda anotado en el perfil; no se crea nada en el repo (Q21). Retirarlo no borra sus datos del perfil (Q25) |
| **Desarrollador** | Registrar / retirar agente en un worktree | ✅ | BR-VAL-001, BR-VAL-002 |
| **Desarrollador** | Declarar / retirar una carpeta de código (raíz) | ✅ | Con `raptor repo roots add` / `remove` (comando reservado), nunca como configuración; raíces válidas según BR-AUTH-003 (enmienda 2026-10-07) |
| **Desarrollador** | Aceptar / descartar un repo descubierto | ✅ | Aceptar es añadir el repo; solo desde la TUI o la CLI en su propia terminal, nunca por MCP (BR-AUTH-003) |
| **Desarrollador** | Corregir una atribución automática | ✅ | Reemplaza la atribución detectada; no añade sesión (BR-CONS-002, Q33) |
| **Desarrollador** | Ajustar el umbral de inactividad de un repo | ✅ | Lo edita él en la configuración local personal del repo; el motor solo la lee (BR-TIME-001, BR-CONS-007) |
| **Agente** (cualquier tipo) | Registrarse en su worktree | ✅ | BR-VAL-001, BR-VAL-002 |
| **Agente** (cualquier tipo) | Añadir / retirar repo observado | ❌ | Solo el desarrollador cambia los repos observados (Q40) |
| **Agente** (cualquier tipo) | Corregir una atribución | ❌ | Solo el desarrollador corrige; el agente se registra (BR-CONS-002) |
| **Agente** (cualquier tipo) | Declarar / retirar una raíz; aceptar / descartar un repo descubierto | ❌ | Ni por MCP ni desde un proceso lanzado por el agente (BR-AUTH-003, enmienda 2026-10-07) |

**Excepciones**:
- Ninguna en el MVP.

**Un agente no cambia los repos observados** (Q40, confirma el supuesto S7; coherente con la allowlist de NFR-02): solo el desarrollador añade o retira repos. Si un agente lo pide, el motor lo rechaza y la lista de repos observados no cambia.

#### Enmienda (2026-10-07): descubrir dentro de las raíces declaradas

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO, sobre las propuestas A1 a A3 aceptadas por Rene Bonilla (2026-10-06). Decisiones Q43 a Q48 del contexto.

La frase "no descubre repos por su cuenta" se sustituye por: **el motor solo descubre repos dentro de las carpetas de código que el desarrollador declaró, y descubrir nunca es observar**. Lo que no cambia: solo el humano decide qué se observa, y un agente no amplía el conjunto. Las condiciones del descubrimiento (raíces, primer nivel, confirmación humana, descarte) están en BR-AUTH-003.

**Referencias**:
- User Story: US-GRP-001, US-GRP-006, US-GRP-010 (el agente no corrige); US-GRP-020, US-GRP-022 y US-CKP-025 (enmienda 2026-10-07)
- BRD: NFR-02, NFR-03
- Contexto: decisiones Q16, Q21, Q25, Q40 y Q43 a Q48
- Relacionada: BR-EDGE-005, BR-AUTH-003

---

### BR-AUTH-002: El motor no hace modificaciones operativas; una capacidad futura que las necesitara exige cambio de requerimiento y permiso explícito

**Descripción**: En el MVP el motor **no hace ninguna modificación operativa** en los repos que observa (Q22): no instala hooks de Git, no toca la configuración de Git del repo, no cambia metadatos de worktrees ni ejecuta ninguna otra operación que modifique una ruta operativa (ver la definición en BR-CONS-001). Los hooks de Git son de Guardrails (F-001-04, BR-12). El motor puede **leer como señal adicional** la información de hooks que ya existan en el repo (p. ej. los de Guardrails), pero ninguna capacidad del motor puede depender de ellos: la detección de agentes funciona sin hooks propios y sin hooks ajenos.

**Principio de frontera**: si una capacidad futura del motor necesitara modificar una ruta operativa, haría falta un **cambio explícito de este requerimiento** y aplicar el **modelo de permiso explícito del desarrollador** que se describe abajo. Ese modelo se conserva como decisión del producto (Q11, Q14, Q15): sirve como principio para el motor futuro y como referencia para otras features. En el MVP no hay escenarios que lo ejerzan.

**Aplicabilidad**: Siempre, en todo repo observado y en Windows, macOS y Linux. No aplica a los efectos internos y temporales que Git produce por sí mismo al leer (ver BR-CONS-001, "Lo que no cuenta como modificación").

**Criticidad**: Alta

**Regla de autorización (MVP)**:
```
Actor: Motor local
No puede: Modificar ninguna ruta operativa del repo observado, con o sin permiso
Puede: Leer la información de hooks que ya existan, como señal opcional

Actor: Desarrollador orquestador
Puede: Instalar o cambiar sus hooks y su configuración por su cuenta, como siempre (fuera del motor)
```

#### Modelo de permiso explícito (principio del producto, sin uso en el MVP)

Si un cambio de requerimiento habilitara una modificación operativa del motor, esa modificación seguiría estas condiciones:

1. **La petición se hace cuando hace falta** y explica en lenguaje claro qué se modifica, dónde (repo y ruta exacta), por qué (qué capacidad mejora), cómo se revierte y qué pasa si no se autoriza.
2. **Solo el humano concede**: un agente no puede conceder el permiso, ni pedirlo en nombre del desarrollador, ni dar por concedido un permiso pendiente. Sin respuesta, la petición queda pendiente y nunca se asume.
3. **Sin re-preguntar tras una denegación**: el motor no repite la petición hasta que el desarrollador la active a mano (Q14).
4. **Un permiso, una modificación, un repo**: no se extiende a otros repos ni a otras modificaciones; cambiar lo ya instalado requiere un permiso nuevo (Q15).
5. **Recuperable, registrada y respetuosa**: se puede revertir dejando la ruta como estaba, queda registrada (qué, dónde, cuándo, quién la autorizó) y no sustituye ni rompe lo que el usuario o Guardrails ya tenían (NFR-01, NFR-07).
6. **Nada básico depende de ella**: la capacidad sigue funcionando, aunque con menos precisión, si el permiso no se concede.

Lo que está fuera del repo observado (configuración global de Git, otros repos, archivos del usuario) no es ruta operativa y no se modifica ni siquiera con permiso (Q17).

**Ejemplo**:
- **Detección sin hooks**: el repo `gitRaptor` no tiene hooks de Guardrails → el motor detecta a Claude Code en `feat-login` con lo observable, sin instalar nada.
- **Señal adicional**: el repo `otro-repo` tiene hooks de Guardrails que dejan constancia de cada commit → el motor puede usar esa información para atribuir mejor, sin modificar los hooks.
- **Hooks retirados**: el desarrollador desactiva los hooks de Guardrails en `otro-repo` → la detección sigue funcionando, con la precisión que dé lo observable.
- **Sin modificación operativa**: en ninguna situación del MVP el motor cambia los hooks ni la configuración de Git de un repo observado.

**Referencias**:
- User Story: US-GRP-001, US-GRP-007
- BRD: BR-02, BR-12, NFR-01, NFR-07
- Contexto: decisiones Q11, Q14, Q15, Q17 y Q22; riesgos R7, R8 y R9
- Relacionada: BR-CONS-001

---

### BR-AUTH-003: Descubrir repos dentro de las carpetas de código declaradas nunca es observar

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO, sobre las propuestas A1 a A3 aceptadas por Rene Bonilla (2026-10-06). Enmienda BR-AUTH-001 (decisiones Q43 a Q48 del contexto).

**Descripción**: El desarrollador puede declarar una o varias **carpetas de código** (raíces), por ejemplo `~/Documents/code`. El motor vigila **solo el primer nivel** de cada raíz y, cuando encuentra en él un repo que no observa, lo marca como **descubierto** y lo propone: "¿Observar *x*?". Un repo descubierto **no se observa**: el motor no registra sus eventos ni sus sesiones, no aparece en la vista de la flota y no entra en la lista de repos que puede usar el MCP. Pasa a observado solo cuando el humano lo **acepta**, y aceptar es lo mismo que añadirlo (BR-AUTH-001).

**Criticidad**: Alta

**Condiciones**:

1. **Las raíces son personales y de la máquina, y no son configuración.** Se guardan en el perfil, junto a la lista de repos observados, y solo se gestionan con comandos reservados al humano: `raptor repo roots` (listar), `raptor repo roots add <ruta>` y `raptor repo roots remove <ruta>` *(ajuste 2026-10-07: antes, "se declaran en el perfil" como valor de configuración)*. Una raíz escrita en cualquier archivo de configuración (del equipo, del perfil o local del repo) no se tiene en cuenta y el motor avisa. Motivo: un repo clonado no puede decidir qué carpetas de la máquina se vigilan, y un agente puede escribir un archivo de configuración pero no ejecutar un comando reservado.
2. **Raíces válidas** *(enmienda 2026-10-07, **Decisión de Rene (2026-10-07)**: "el usuario da el path de la ruta que quiere monitorear")*. Se acepta la carpeta que el humano dé con el comando reservado, **incluida su carpeta personal**. Se rechazan, con su motivo y la alternativa: la raíz del sistema de archivos o de la unidad del sistema, una carpeta que contiene carpetas personales (`/Users`, `C:\Users`), una ruta que no existe o que no es una carpeta, una carpeta que ya es un repo (se añade con `raptor repo add`; una carpeta personal con `~/.git` también es un repo), una carpeta dentro de un repo, el propio perfil de GitRaptor y una ruta de red. Como mucho 16 raíces (SEC-15). Declarar dos veces la misma raíz no cambia nada.
   - **Raíz amplia: aviso y confirmación.** La carpeta personal, la raíz de otro volumen (`/Volumes/X`, `D:\`) o una carpeta con más de 512 entradas en su primer nivel es una raíz amplia. Antes de declararla, la CLI avisa del coste y pide confirmación explícita al humano (`[s/N]`; por defecto, no). Sin terminal interactiva no se declara. Como en la condición 8, un agente no puede declararla.
   - **Exclusiones fijas.** En cualquier raíz no se miran nunca las entradas que no son carpetas, las ocultas (`.*`) y los enlaces simbólicos. Cuando la raíz es la carpeta personal tampoco se miran las carpetas de datos de otras aplicaciones ni las carpetas protegidas del sistema: `Library`, `Desktop`, `Documents`, `Downloads`, `Pictures`, `Movies` y `Music` en macOS; `snap` en Linux; `AppData` y `OneDrive` en Windows. Motivo: privacidad (ahí no hay repos del desarrollador) y, en macOS, que el sistema no pida permiso de acceso. Un repo que esté en `~/Documents/code` se descubre declarando esa carpeta como raíz.
3. **Solo el primer nivel.** Un repo anidado más abajo (`~/code/clientes/acme`) no se descubre. Un worktree de un repo ya observado no se propone como repo nuevo: ya se observa como worktree.
4. **Avisos.** Al declarar una raíz, los repos que ya contiene quedan descubiertos con **un único aviso** que da el número. Después, cada repo que aparece genera un aviso. Un repo descubierto sigue en la lista de descubiertos hasta que el humano decida. En el MVP el aviso se ve en la TUI y en la lista de la CLI (`raptor repo discovered`); la notificación nativa del sistema operativo queda fuera del MVP (Q46; **Decisión de Rene (2026-10-07)**).
5. **Observar exige confirmación humana.** Aceptar un repo descubierto, responder a la pregunta de `raptor clone` o a la de la TUI abierta en un repo no observado son acciones del humano: desde la TUI o la CLI que él lanza en su propia terminal. Nunca por MCP y nunca desde un proceso lanzado por un agente (el motor resuelve quién pide la acción por la ascendencia del proceso, como en BR-CKP-AUTH-002). Sin terminal interactiva no se pregunta y no se observa. **La respuesta por defecto es no observar** (`[s/N]`).
6. **Descartar es persistente** (`raptor repo dismiss <ruta>`; aceptar es `raptor repo add <ruta>`). Un repo descartado no se vuelve a proponer, tampoco tras reiniciar el motor (BR-CONS-005). Se puede añadir a mano en cualquier momento con `raptor repo add`. Responder "N" a una pregunta no es descartar: solo no observa ahora.
7. **Retirar una raíz** quita las propuestas pendientes de esa raíz y no cambia nada de lo observado.
8. **Un agente no decide.** Un agente no puede declarar ni retirar raíces, ni aceptar ni descartar repos descubiertos (coherente con Q40). Sí puede clonar (con `git clone` o con `raptor clone`), pero sin que eso observe nada.
9. **Descubrir no escribe nada** en el repo descubierto ni en la raíz (BR-CONS-001): la lista de raíces, de descubiertos y de descartados vive en el perfil.

> ✅ **Decisión de Rene (2026-10-07)**: la carpeta personal **puede ser una raíz** ("debemos darle la opción al usuario a que monitoree el folder que desea"). Cierra el punto que el PR #142 dejó "pendiente de confirmar con Rene (2026-10-07)" y sustituye el supuesto del PO que la rechazaba. **Decisión del orquestador (2026-10-07), validada por el Arquitecto**: aviso de coste y confirmación para una raíz amplia (umbral de 512 entradas), exclusiones fijas por sistema operativo, y `/`, la unidad del sistema y los ancestros de las carpetas personales siguen rechazados (contienen carpetas de otros usuarios; SEC-11). Descubrir sigue siendo solo de primer nivel y nunca es observar.
>
> ✅ **Supuesto ratificado**: un repo descartado se identifica por su ruta; si en la misma ruta aparece otro repo, sigue descartado. Retirar un repo observado que vive en una raíz cuenta como descartarlo: no se vuelve a proponer. **Decisión de Rene (2026-10-07)**: ratifica el supuesto; un repo descartado se recuerda por su ruta y no se vuelve a preguntar.

**Regla formal**:
```
raíces ⊆ índice del perfil (solo por comando reservado);  raíz válida ⇔ carpeta existente ∧ ¬raíz del sistema o de la unidad del sistema ∧ ¬ancestro de carpetas personales ∧ ¬repo ∧ ¬dentro de un repo
raíz amplia(r) ⇔ r = carpeta personal ∨ r = raíz de otro volumen ∨ entradas_primer_nivel(r) > 512  →  declarar exige confirmación humana explícita
mirado(e) ⇔ e carpeta ∧ ¬oculta ∧ ¬enlace ∧ e ∉ exclusiones del SO (si la raíz es la carpeta personal)
descubierto(r) ⇔ r es repo en el primer nivel de una raíz ∧ r ∉ observados ∧ r ∉ descartados ∧ r ∉ worktrees de observados
observado(r) ⇐ añadir(r) | aceptar(r)   con solicitante = humano y canal ∈ {TUI, CLI en su terminal}
Constraint: solicitante = agente ∨ canal = MCP → rechazo; la lista no cambia
Constraint: pregunta sin respuesta, sin terminal interactiva o con Intro → no observar
```

**Ejemplo**:
- Rene declara `~/Documents/code` con 104 repos → un aviso: "104 repos descubiertos"; ninguno se observa. Acepta `gitRaptor` y descarta el resto que no usa.
- Clona `billing` desde GitKraken en `~/Documents/code` → la TUI propone "¿Observar billing?"; mientras no responda, `billing` no se observa.
- Claude Code pide por MCP observar `~/Documents/code/otro` → no hay herramienta para hacerlo.
- La configuración de equipo de `shop` declara `~/proyectos` como raíz → no se vigila; el motor avisa.
- Rene declara su carpeta personal → la CLI avisa de que es una raíz amplia y pregunta `[s/N]`; con "s" se descubren los repos del primer nivel de `~`, sin mirar `Library`, `Documents` ni las carpetas ocultas.

**Referencias**:
- User Story: US-GRP-020 (raíces y descubrimiento), US-GRP-022 (aceptar y descartar), US-GRP-021 (`raptor clone`), US-CKP-025 (la TUI)
- BRD: NFR-02, NFR-03, NFR-01
- Contexto: decisiones Q43 a Q48
- Relacionada: BR-AUTH-001, BR-CONS-001, BR-CONS-005, BR-WF-002 · NFR: SEC-15, RES-11

---

## 6. Reglas de Consistencia de Datos

### BR-CONS-001: El motor no escribe nada en el repo observado (derivada de NFR-01)

**Descripción**: El motor **solo observa** (Q21). Después de que el motor observe un repo, durante el tiempo que sea y haga lo que haga (añadirlo, seguir sus worktrees, detectar o registrar agentes, reiniciarse, leer su configuración), el repo queda exactamente como estaba: ni su **código fuente** ni sus **rutas operativas** cambian, y el motor no crea en él ninguna carpeta ni archivo propio. Esta regla **se deriva** de NFR-01 (cero pérdida de datos): el trabajo sin commitear no existe en ningún otro sitio, así que la forma más fuerte de protegerlo es no tocar el repo. Todo lo que el motor genera lo guarda en el **perfil de GitRaptor**, separado por repo; fuera del repo, el perfil es lo único que escribe (Q17, Q21).

**Por qué no se escribe en el repo** (Q21): escribir datos del motor en el working tree obligaba a una excepción a la definición de código fuente; los agentes podían hacer commit de esos datos por accidente o alterar los datos de atribución; con varios worktrees los datos se fragmentaban; y se mezclaban con la configuración versionada. Se renuncia a que los datos viajen con el repo, que no es objetivo del MVP.

**Aplicabilidad**: Siempre, en todo repo observado y en todos sus worktrees, en Windows, macOS y Linux.

**Criticidad**: Alta

#### Qué es "código fuente" (lo que el motor nunca modifica)

"Código fuente" es todo lo que el usuario (o sus agentes) ha escrito, preparado o commiteado en el repo, es decir, su trabajo. Esta definición está confirmada por el dueño del producto (decisión Q13 del contexto) y, desde Q21, no tiene excepciones. En ningún repo observado ni en ninguno de sus worktrees, el motor:

1. **Archivos de trabajo**: no crea, modifica, mueve ni borra archivos o carpetas del working tree, sean versionados, no rastreados o ignorados por Git. Tampoco deja archivos propios entre ellos ni hace que aparezca nada nuevo como cambio pendiente.
2. **Lo preparado para el próximo commit**: no añade ni quita cambios del área de preparación.
3. **La historia y lo que la señala**: no crea commits, no reescribe la historia y no crea, mueve ni borra ramas, tags, HEAD, el stash ni las ramas remotas que el repo conoce.
4. **Operaciones en curso**: no completa, continúa ni aborta rebases, merges u otras operaciones a medias, ni deja ninguna operación nueva a medias.
5. **Worktrees como espacio de trabajo**: no crea, borra ni mueve worktrees (borrar o mover un worktree se lleva los archivos de trabajo del usuario).
6. **Configuración del repo**: no escribe la configuración del repo compartida con el equipo (la de Guardrails, BR-11) ni la configuración local personal del repo. Las dos son archivos del usuario y el motor solo las lee (BR-CONS-007, Q23).
7. **Credenciales**: no las cambia ni las usa para nada que no sea leer.

En consecuencia, el motor no trae novedades del remoto por su cuenta: el ahead/behind se calcula con lo que el repo ya conoce del remoto (decisión Q12 del contexto).

#### Qué es "ruta operativa" (en el MVP tampoco se modifica)

Una "ruta operativa" es una parte **del repo observado** que hace funcionar a Git pero no contiene el trabajo del usuario. Solo hay rutas operativas dentro del repo observado. Son rutas operativas, entre otras:

- Los **hooks de Git** del repo.
- La **configuración de Git del repo**.
- Los **metadatos de los worktrees** (p. ej. marcar un worktree como bloqueado o limpiar los datos de un worktree que ya no existe).
- El resto del contenido interno del directorio `.git` que no sea el trabajo del usuario descrito arriba (lo preparado, la historia, las ramas, los tags y el stash son código fuente aunque Git los guarde dentro de `.git`).

En el MVP el motor no modifica ninguna ruta operativa (BR-AUTH-002, Q22). Puede leer la información que dejen los hooks que ya existan, como señal adicional.

#### Lo que está fuera del repo observado (nada del usuario se modifica nunca)

La configuración global de Git del usuario, otros repos y cualquier otro archivo del usuario fuera del repo observado **no son rutas operativas** y el motor no los modifica, ni siquiera con permiso del desarrollador (decisión Q17 del contexto). Lo único que el motor escribe fuera del repo son sus datos en el **perfil de GitRaptor**, un espacio exclusivo de la herramienta, fuera de cualquier repo. La configuración de nivel perfil del usuario también vive en el perfil, pero el motor solo la lee (BR-CONS-007).

#### Lo que no cuenta como modificación

Al leer un repo, Git produce por sí mismo efectos internos y temporales (por ejemplo, bloqueos momentáneos o refrescos de información interna en `.git`). Esos efectos **no son una modificación** a efectos de esta regla ni de BR-AUTH-002, siempre que no cambien nada del estado observable del repo (la lista de "Cómo se verifica"). Lo que el usuario ve del repo, con Git o sin él, tiene que ser lo mismo antes y después.

> 🏗️ **Para el Arquitecto**: la frontera técnica entre "efecto interno de una lectura" y "modificación" la define y documenta el Arquitecto en Fase 2, justificando por qué cada efecto interno no cambia el estado observable. Ante la duda, se trata como modificación y el motor no la hace.

#### Qué datos propios genera el motor y dónde viven

Todos los datos propios del motor viven en el **perfil de GitRaptor**, separados por repo (Q21). La configuración no es un dato propio: el motor la lee y no la escribe (Q23).

| Dato | Qué contiene | Dónde vive | Acceso del motor | Regla relacionada |
|------|--------------|------------|------------------|-------------------|
| Lista de repos observados | Qué repos añadió el desarrollador | Perfil | Lectura y escritura | BR-AUTH-001 |
| Registro de agentes | Qué agente se registró en qué worktree, cuándo y quién lo registró | Perfil, separado por repo | Lectura y escritura | BR-VAL-001, BR-CONS-002 |
| Atribuciones | Qué agente corresponde a cada worktree, cambio o evento, con su origen, o "sin atribuir" si no hay ninguno. "Humano" no es un valor (Q34) | Perfil, separado por repo | Lectura y escritura | BR-CONS-003, BR-EDGE-004 |
| Historial de eventos de Git | Qué evento ocurrió, cuándo y a quién se atribuye | Perfil, separado por repo | Lectura y escritura | BR-CONS-005 |
| Estado de las sesiones | Activo, inactivo o terminado por sesión | Perfil, separado por repo | Lectura y escritura | BR-WF-001 |
| Configuración de nivel perfil | Valores por defecto globales del usuario, entre ellos el umbral de inactividad por defecto (5 minutos) | Perfil | Solo lectura | BR-CONS-007, BR-TIME-001 |
| Configuración del repo (equipo) | Valores compartidos con el equipo, entre ellos la rama base | Repo (de Guardrails, BR-11) | Solo lectura | BR-CONS-007, BR-CONS-006 |
| Configuración local personal del repo | Ajustes personales de ese repo, entre ellos el umbral de inactividad | Repo, no versionada | Solo lectura | BR-CONS-007, BR-TIME-001 |
| ~~Permisos y registro de modificaciones operativas~~ | ~~Permisos concedidos, denegados o pendientes y lo que se modificó~~ | Obsoleto (Q22): en el MVP no hay modificaciones operativas | — | — |

Cada dato propio tiene un único sitio, el perfil, y ningún dato propio vive en el repo. Cómo se organiza el perfil, su formato y su ubicación concreta los decide el Arquitecto.

**Si se pierde o se borra el perfil** (Q26, confirma el supuesto S17): el motor sigue funcionando. El desarrollador vuelve a añadir los repos que quiera observar y el motor los observa desde ese momento. Lo ocurrido mientras no hubo datos queda "sin atribuir", igual que en un hueco de observación (BR-EDGE-005), y nunca se atribuye a un agente. Ni siquiera en ese caso el motor escribe nada en el repo para reconstruir lo perdido.

#### Quién sí puede modificar el repo

| Quién | Código fuente | Rutas operativas |
|-------|---------------|------------------|
| **El desarrollador y sus agentes** | Sí: es su trabajo. El motor observa esas escrituras, no las provoca | Sí, por su cuenta, como siempre |
| **Motor local (esta feature)** | **Nunca** | **Nunca en el MVP** (BR-AUTH-002) |
| F-001-02 Cockpit | Acciones por agente: aprobar, merge, descartar, crear worktree (BR-07), con su propia garantía NFR-01 | Según su feature |
| F-001-03 Time Machine | Snapshots, undo y redo (BR-08, BR-09), con su propia garantía NFR-01 | Según su feature |
| F-001-04 Guardrails | Según su feature | Aplicación de políticas en hooks de Git (BR-12). El motor puede leer la información de esos hooks y no los modifica |
| F-001-05 Servidor MCP | Operaciones seguras pedidas por los agentes (BR-14), con su propia garantía NFR-01 | Según su feature |

#### Si una capacidad futura del motor necesitara modificar algo

- **Si necesitara modificar una ruta operativa** (p. ej. instalar un hook para mejorar la detección de agentes): requiere un **cambio explícito de este requerimiento** y el modelo de permiso explícito del desarrollador de BR-AUTH-002, y la capacidad tiene que seguir funcionando, aunque con menos precisión, cuando el permiso no se concede.
- **Si necesitara escribir datos en el repo o modificar código fuente**: requiere un **cambio explícito de este requerimiento** aprobado por el dueño del producto, con su garantía de recuperabilidad, o resolverse desde otra feature que ya escriba en el repo (Cockpit, Time Machine, MCP).

#### Cómo se verifica (aceptación)

La regla se cumple si, para un repo de prueba:

1. **Nada cambia en el repo** entre antes y después de que el motor lo observe, comparando como mínimo:
   - El estado del working tree que reporta Git (archivos modificados, preparados, no rastreados e ignorados) y el contenido de esos archivos.
   - La lista de ramas, tags, HEAD, stash y ramas remotas conocidas, y a qué apunta cada una.
   - La lista de worktrees y sus archivos de trabajo.
   - Las operaciones en curso (rebase o merge a medias) siguen igual.
   - La configuración del repo de nivel equipo y la configuración local personal, idénticas.
   - Los hooks de Git, la configuración de Git del repo y los metadatos de worktrees, idénticos.
   - La ausencia de archivos o carpetas nuevos en cualquier lugar del repo.
2. **Fuera del repo observado, lo único que cambia son los datos del motor en el perfil de GitRaptor**: la configuración global de Git, los otros repos, los archivos del usuario y la configuración de nivel perfil son idénticos antes y después.

La comparación se repite en los escenarios que más riesgo tienen: añadir y retirar el repo; observarlo mientras un agente trabaja; registrar y retirar un agente; corregir una atribución automática; cambiar la configuración en cada uno de sus tres niveles; reiniciar GitRaptor; reiniciar la máquina; observar un worktree en un estado especial de Git (rebase o merge en curso); observar un repo con hooks de Guardrails instalados; y preparar todos los cambios del repo de una vez (el equivalente a "añadir todo") después de observarlo, sin que entre nada del motor. Se verifica en Windows, macOS y Linux. Los cambios que hagan el desarrollador o los agentes durante la prueba no cuentan: lo que se comprueba es que el motor no añada ninguno.

**Regla de consistencia**:
```
Constraint: repo observado antes de observar = repo observado después de observar
            (código fuente + rutas operativas + ausencia de archivos nuevos)
Constraint: escritura del motor ⇒ es en sus datos del perfil de GitRaptor
Constraint: configuración (cualquier nivel) ⇒ el motor solo la lee
Scope: todo repo observado, todos sus worktrees, todo el tiempo que esté en observación
Datos propios del motor: solo en el perfil, separados por repo
```

**Comportamiento en conflicto**:
- **Si el motor modifica código fuente**: es un defecto crítico (NFR-01, KPI "0 incidentes de pérdida de datos"). Bloquea la entrega de la feature.
- **Si el motor modifica una ruta operativa**: también es un defecto crítico (BR-AUTH-002). Bloquea la entrega de la feature.
- **Si el motor crea cualquier archivo o carpeta en el repo, o escribe en cualquier nivel de la configuración**: defecto crítico (Q21, Q23). Bloquea la entrega de la feature.

**Referencias**:
- User Story: US-GRP-001, US-GRP-012 (y transversal en todas)
- BRD: NFR-01, NFR-07, BR-11
- Contexto: decisiones Q10, Q12, Q13, Q17, Q21, Q22, Q23 y Q26 (Q2, Q18 y Q19 reemplazadas), supuestos S1 y S2, riesgo R13
- Relacionada: BR-AUTH-002, BR-CONS-007

---

### BR-CONS-002: Corregir una atribución reemplaza la detectada

**Descripción**: Cuando la detección automática se equivoca, el desarrollador **corrige** la atribución de ese worktree indicando el agente correcto. Corregir es una acción explícita y distinta de registrar otro agente (Q33): **reemplaza** la atribución detectada, porque la detección estaba mal. Después de corregir, el worktree no tiene una sesión más: la sesión que el motor había detectado pasa a atribuirse al agente corregido, con origen "registrado", y el worktree no pasa a compartido. **La corrección alcanza toda la sesión** (Q37): los eventos ya atribuidos a la sesión mal detectada se reatribuyen al agente corregido **desde el inicio de esa sesión**; los de otras sesiones (p. ej. una anterior ya terminada) no cambian. Si no fuera así, la Time Machine ofrecería deshacer "lo que hizo Claude" con eventos que no eran suyos. Solo se corrige donde hay una atribución detectada (Q38): en un worktree sin ninguna, el motor rechaza la corrección e indica que se use el registro explícito (BR-VAL-001). Mientras la corrección esté vigente, una detección posterior de esa misma sesión no la deshace. Si el desarrollador retira la corrección y la sesión sigue presente, vuelve la atribución detectada.

Registrar otro agente donde ya hay una sesión es otra cosa: **añade** una sesión y el worktree pasa a compartido (BR-CONS-004).

> **Cambio Q33 (2026-10-03)**: antes decía "corregir una atribución automática equivale a registrar la atribución correcta" y "el registro explícito prevalece sobre la detección". Esa frase hacía que registrar otro agente y corregir se solaparan; deja de valer.

**Aplicabilidad**: Cuando el desarrollador corrige la atribución de un worktree con una atribución detectada. Solo el desarrollador corrige; un agente se registra, no corrige (BR-AUTH-001).

**Criticidad**: Alta

**Regla de consistencia**:
```
corregir(worktree, agente correcto):
  REQUIRE quien corrige = desarrollador                        (un agente no corrige, BR-AUTH-001)
  REQUIRE existe una atribución detectada en el worktree      (Q38; si no: rechazo + "usa el registro")
  la sesión detectada pasa a atribuirse a "agente correcto", origen = registrado
  los eventos de esa sesión, desde su inicio, pasan a "agente correcto", origen = registrado   (Q37)
  los eventos de otras sesiones del worktree: no cambian
  número de sesiones del worktree: no cambia (no se marca compartido por la corrección)
  mientras la corrección esté vigente: la detección de esa sesión no la reemplaza
  la corrección persiste aunque el motor deje de ejecutarse y vuelva a arrancar (BR-CONS-005)
retirar corrección:
  IF la sesión sigue presente THEN vuelve la atribución detectada
Constraint: corregir ≠ registrar otro agente (BR-CONS-004)
```

**Ejemplo**:
- **Corregir**: el motor detecta "Claude Code" en `feat-login`, pero quien trabaja allí es Codex; el desarrollador corrige la atribución a "Codex" → `feat-login` tiene una sola sesión, "otro agente: Codex", con origen "registrado"; no es compartido. *(Q32: antes, el ejemplo partía de una detección de Cursor.)*
- **Registrar otro agente (no es corregir)**: el motor detecta "Claude Code" en `feat-login` y el desarrollador registra además "Codex" → dos sesiones y worktree compartido (BR-CONS-004).
- **Retirar la corrección**: con la corrección a "Codex" vigente y la sesión aún presente, el desarrollador la retira → `feat-login` vuelve a "Claude Code" con origen "detectado".
- **Alcance hacia atrás** (Q37): la sesión detectada como "Claude Code" en `feat-login` ya tenía atribuidos los commits `c1` y `c2`; antes hubo `c0`, de otra sesión de Claude Code ya terminada. Al corregir a "Codex" → `c1` y `c2` pasan a "otro agente: Codex" (registrado); `c0` sigue como "Claude Code" (detectado).
- **Sin detección** (Q38): `feat-pagos` no tiene ninguna atribución detectada; el desarrollador intenta corregirla a "Codex" → rechazo: no hay nada que corregir; para atribuir trabajo a Codex, regístralo.
- **Un agente intenta corregir**: Claude Code pide corregir la atribución de `feat-login` → rechazo: solo el desarrollador corrige (BR-AUTH-001).

> ⚠️ **ASSUMPTION** `[POR VERIFICAR]` (P17): al retirar una corrección, los eventos que esta había reatribuido vuelven también a la atribución detectada. Las historias solo exigen que vuelva la atribución de la sesión.

**Referencias**:
- User Story: US-GRP-010
- BRD: BR-02
- Contexto: decisiones Q33, Q37 (cierra P14) y Q38 (cierra P15); pregunta P17
- Relacionada: BR-CONS-004, BR-VAL-002

---

### BR-CONS-003: El origen de cada atribución es siempre visible

**Descripción**: Toda atribución de un worktree o de un evento a un agente indica su origen: "detectado" o "registrado". Las features que dependen de la atribución (Time Machine, Guardrails) necesitan saber cuánto confiar en ella. Lo que no se atribuye a ningún agente se presenta como "sin atribuir".

**Valores que emite el motor** (Q34, Q35): solo dos. "Agente X" (con su origen) o "sin atribuir". El motor **nunca emite "humano"**: no puede probar que un cambio lo hizo una persona, y un agente sin registrar que no se detecta es indistinguible del trabajo humano. "No identificada" tampoco es un valor: todo lo que no tiene agente seguro es "sin atribuir". Presentar "sin atribuir" como "tú u otro" es decisión de la Time Machine y el Cockpit.

**Aplicabilidad**: Toda atribución que el motor ofrece.

**Criticidad**: Alta

**Regla de consistencia**:
```
Constraint: actor ∈ {agente X, "sin atribuir"}          (nunca "humano", Q34; nunca "no identificada", Q35)
Constraint: toda atribución a un agente lleva origen ∈ {detectado, registrado}
Constraint: un cambio o evento sin agente atribuido se marca "sin atribuir"
```

**Referencias**:
- User Story: US-GRP-002, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010

---

### BR-CONS-004: Un worktree puede tener varias sesiones de agente

**Descripción**: Si el motor identifica más de un agente trabajando en el mismo worktree, los reporta todos y marca el worktree como **compartido**. Registrar otro agente en un worktree donde ya hay una sesión (detectada o registrada) **añade** una sesión: no reemplaza la que había (Q33). Si el agente que se registra **es el mismo que ya se detectó** en ese worktree (p. ej. Claude Code se registra a sí mismo vía MCP donde ya se le detectaba), el registro **confirma esa misma sesión**: no se duplica y el worktree no pasa a compartido (Q39). Reemplazar una atribución detectada es corregirla (BR-CONS-002) y no vuelve compartido el worktree. El worktree deja de ser compartido cuando vuelve a quedar una sola sesión presente. En el MVP no se atribuye cada archivo a uno u otro agente dentro de un worktree compartido; eso queda para una fase posterior.

**Aplicabilidad**: Detección y registro (no corrección).

**Criticidad**: Media

**Regla de consistencia**:
```
registrar(worktree, agente) con sesiones presentes ≥ 1:
  IF ya hay una sesión detectada de ese mismo agente en el worktree
  THEN se confirma esa sesión; el número de sesiones no cambia        (Q39)
  ELSE se añade una sesión; las existentes se conservan
IF el número de sesiones presentes en un worktree > 1
THEN se reportan todas y el worktree se marca como compartido
     (sin atribución por archivo en el MVP)
Constraint: una corrección (BR-CONS-002) no cambia el número de sesiones
```

**Ejemplo**:
- Claude Code (detectado) en `feat-login`; el desarrollador registra Codex → dos sesiones, "Claude Code" (detectado) y "otro agente: Codex" (registrado); worktree compartido. *(Q32: antes, "Claude Code y Cursor".)*
- Mismo punto de partida, pero el desarrollador **corrige** la atribución a Codex → una sola sesión, no compartido (BR-CONS-002).
- Claude Code (detectado) en `feat-login` se registra a sí mismo en `feat-login` → la misma sesión, confirmada; una sola sesión, no compartido (Q39).

> ✅ **Supuesto ratificado** (P16): una sesión confirmada por registro pasa a mostrar origen "registrado". Las historias solo exigen que sea la misma sesión y que el worktree no pase a compartido.
>
> **Decisión del orquestador (2026-10-05), validada por el PO (agente)**: US-GRP-009 adopta el supuesto "sí" (D6 de su [Dev Spec](./dev-specs/US-GRP-009-dev-spec.md)). Queda pendiente de ratificar por Rene Bonilla; si la respuesta fuera "no", solo cambia el origen mostrado (ADR-GRP-013 § 2).
>
> **Decisión de Rene (2026-10-07)**: P16 = sí. Ratifica el supuesto; el origen de una sesión confirmada por registro es "registrado".
- El desarrollador edita en Cursor dentro de `feat-login`, donde trabaja Claude Code → una sola sesión (Claude Code); el editor del humano no cuenta como sesión de agente (BR-EDGE-004).

**Referencias**:
- User Story: US-GRP-011, US-GRP-009 (registro que confirma una sesión detectada)
- Contexto: decisiones Q7, Q33 y Q39; pregunta P16
- Relacionada: BR-CONS-002

---

### BR-CONS-005: La observación es continua y lo observado persiste

**Descripción**: La Time Machine no puede tener huecos. Por eso el motor captura la actividad de los agentes en los repos observados **aunque el desarrollador no tenga abierta ninguna superficie de GitRaptor** (TUI, CLI o MCP). La atribución, el historial de eventos y el estado de las sesiones **sobreviven a reinicios** de GitRaptor y de la máquina: al volver, todo lo observado antes sigue disponible. Cómo se logra (proceso en segundo plano u otro) lo decide el Arquitecto.

**Aplicabilidad**: Todo repo observado, todo el tiempo que la máquina esté encendida.

**Criticidad**: Alta

**Regla de consistencia**:
```
Constraint: mientras la máquina está encendida y el repo está en observación,
            la actividad se captura haya o no una superficie abierta
Constraint: atribución + historial de eventos + estado de sesiones antes del reinicio
            = lo disponible después del reinicio
```

**Ejemplo**:
- El desarrollador cierra la TUI y la terminal; Claude Code sigue trabajando en `feat-login` y hace dos commits. Al abrir la TUI una hora después, los dos commits aparecen atribuidos a Claude Code.
- La máquina se reinicia; al volver, el historial de eventos de ayer y sus atribuciones siguen disponibles.

**Referencias**:
- User Story: US-GRP-004 (también la persistencia de una corrección en US-GRP-010 y de un worktree compartido en US-GRP-011)
- Contexto: decisiones Q1 y Q6
- Relacionada: BR-EDGE-005 (qué pasa si aun así hay un hueco)

#### Enmienda (2026-10-07): un repo en reposo sigue observado

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO (condición Q49) y por el Arquitecto (TS-GRP-006, RES-12).

Un repo observado puede pasar a **dormido** cuando lleva un tiempo configurable sin actividad, sin sesiones presentes y sin ninguna superficie abierta sobre él. **Dormir no lo saca de la observación**: lo que ocurre en él se sigue capturando y la protección no se reduce (Q49). Lo que cambia es el retraso: el primer cambio lo despierta y queda publicado en ≤ 2 s p95 (RES-12), frente al presupuesto en vivo de un repo activo (NFR-04). También lo despiertan la aparición de una sesión de agente, abrir la TUI en él o la petición de cualquier cliente. Un repo con algún worktree que el motor no puede vigilar en vivo no duerme.

**Ejemplo**: `docs-site` lleva 8 horas sin actividad y duerme. Claude Code empieza una sesión en él → el repo despierta y la sesión y sus commits se atribuyen como en un repo activo.

---

### BR-CONS-006: La rama base de cada repo la define solo la configuración del repo del equipo; por defecto `main`

**Descripción**: La rama base (contra la que se calcula ahead/behind) es `main`, salvo que la configuración del repo compartida con el equipo defina otra (es la configuración de Guardrails, BR-11 del BRD). Es un valor **compartido**: no cambia por persona. Por eso solo lo admite el nivel de equipo (Q24, BR-CONS-007): un valor de rama base en el perfil del usuario o en la configuración local personal del repo **no la cambia**. Quien define ese valor es el desarrollador, a través de Guardrails (F-001-04); el motor solo lo lee.

> **Decisión heredada de Guardrails** (Q-GRD-18, Q-GRD-20 y Q-GRD-21, Rene Bonilla, 2026-10-04; posterior a la aprobación de este requerimiento): la rama base se lee de la configuración del equipo **commiteada en la copia que el repo ya conoce del remoto para la rama principal**, sin consultarlo (Q12); si no hay remoto, de la rama local; si tampoco existe, `main`. Nunca del archivo en disco del worktree principal ni de la versión de otro worktree. El motor calcula el ahead/behind contra la rama base **confirmada** por el humano en esa máquina, el mismo valor que protege Guardrails (BR-CONS-003 de Guardrails). Un cambio de rama base en la rama principal queda como diagnóstico **"pendiente de confirmar"** hasta que el humano lo confirma. Mientras no hay confirmación inicial (que el humano hace al instalar la protección de Guardrails o de forma explícita, nunca al añadir el repo, Q-GRD-23), el motor calcula contra la rama base leída y la marca como **"no confirmada"**.

**Aplicabilidad**: Al calcular ahead/behind de cada worktree.

**Criticidad**: Media

**Regla de consistencia**:
```
rama base leída = la que define la configuración del repo (nivel equipo) commiteada en la copia
                  conocida de la rama principal, ELSE main
IF no hay rama base confirmada
THEN ahead/behind contra la rama base leída, marcada como "no confirmada"
ELSE ahead/behind contra la rama base confirmada
     IF rama base leída ≠ confirmada THEN la leída se muestra como "pendiente de confirmar"
Constraint: el perfil y la configuración local personal no intervienen en la rama base
Constraint: el archivo en disco de cualquier worktree, incluido el principal, no interviene
Constraint: la rama base del motor es la misma que la de Guardrails
```

**Ejemplo**:
- Repo cuya configuración no define rama base → ahead/behind contra `main`.
- Repo cuya configuración de equipo define `develop`, confirmada por el desarrollador → ahead/behind contra `develop`.
- Repo cuya configuración de equipo define `develop` y cuya configuración local personal pone `release` → sigue siendo `develop`: el ajuste local no la cambia.
- Repo sin rama base en la configuración de equipo y con `develop` en el perfil → `main`: el perfil no la cambia.
- Rama base confirmada `develop`; llega a la copia conocida de la rama principal un cambio a `release` → el ahead/behind sigue contra `develop` y `release` aparece como "pendiente de confirmar"; tras la confirmación, contra `release`.
- El desarrollador cambia la rama base en el worktree principal, sin que el cambio llegue a la copia conocida del remoto → no cuenta: ni cambia la rama base ni queda nada pendiente.
- Repo observado sin confirmación inicial, con `develop` en la configuración del equipo → ahead/behind contra `develop`, marcado como "no confirmado".

**Rama base inexistente** (Q42): si la rama base (la definida o `main`) no existe en el repo, el motor indica que no puede calcular ahead/behind; nunca elige otra rama por su cuenta.

**Entrega en dos pasos** (Q36): esta regla describe el comportamiento final. En el MVP la rama base es `main` en todos los repos, de forma provisional (US-GRP-012). Leerla de la configuración del equipo es US-GRP-016, bloqueada por Guardrails (F-001-04) y el ADR de formato de la configuración (P8).

**Referencias**:
- User Story: US-GRP-012 (`main` provisional), US-GRP-016 (configuración del equipo; bloqueada)
- BRD: BR-11
- Contexto: decisiones Q5, Q12, Q23, Q24, Q36 y Q42; decisiones heredadas de Guardrails Q-GRD-18, Q-GRD-20, Q-GRD-21 y Q-GRD-23; dependencia con F-001-04
- Relacionada: BR-CONS-007; BR-CONS-003 y BR-AUTH-001 de Guardrails

---

### BR-CONS-007: El motor lee su configuración en tres niveles y nunca la escribe

**Descripción**: El motor lee su configuración en tres niveles, de menor a mayor prioridad (Q23):

1. **Perfil del usuario**: valores por defecto globales, para todos los repos.
2. **Configuración del repo**, versionada y compartida con el equipo: la de Guardrails (BR-11 del BRD).
3. **Configuración local personal del repo**, no versionada: los ajustes del desarrollador para ese repo en su máquina.

**Niveles admitidos por valor** (Q24): no todos los valores se pueden definir en todos los niveles. Cada valor dice en qué niveles se puede definir y, **dentro de los niveles que admite, gana el más específico**. Un valor escrito en un nivel que ese valor no admite no se tiene en cuenta.

| Valor | Niveles que lo admiten | Si ningún nivel admitido lo define | Por qué | Regla |
|-------|------------------------|------------------------------------|---------|-------|
| Rama base | Solo la configuración del repo (equipo) | `main` | Es compartida: no cambia por persona | BR-CONS-006 |
| Umbral de inactividad | Perfil del usuario (valor por defecto personal global) y configuración local personal del repo (ajuste por repo). **Nunca** la del equipo | 5 minutos | Es una preferencia personal (Q20) | BR-TIME-001 |

Un valor nuevo que afecte al motor tiene que declarar sus niveles admitidos al incorporarse a esta tabla. La excepción de las prohibiciones la pone Guardrails: un nivel personal no puede relajar una prohibición del equipo (esa regla es de F-001-04 y el motor la respeta como dependencia). El motor **nunca escribe** ninguno de los tres niveles: los edita el desarrollador, a mano o con el comando de edición que ofrece Guardrails (F-001-04, Q27). El formato y la estructura los decide un ADR pendiente y los define el context de Guardrails (pregunta P8 del contexto).

**Aplicabilidad**: Siempre que el motor necesite un valor configurable (la rama base, BR-CONS-006, y el umbral de inactividad, BR-TIME-001).

**Criticidad**: Media

**Regla de consistencia**:
```
niveles admitidos(valor) ⊆ {perfil, repo (equipo), local personal del repo}
valor efectivo = el del nivel más específico, entre los niveles admitidos para ese valor, que lo defina
                 ELSE el valor por defecto del producto para ese valor
Constraint: un valor escrito en un nivel no admitido no se tiene en cuenta
salvo: una prohibición del equipo no la relaja un nivel personal (regla de Guardrails)
Constraint: el motor no escribe ningún nivel de la configuración
```

**Ejemplo**:
- El perfil fija el umbral de inactividad en 5 minutos y la configuración local de `gitRaptor` lo fija en 15 → en `gitRaptor` vale 15; en el resto de repos, 5.
- La configuración de equipo de `otro-repo` fija un umbral de inactividad de 30 minutos → no se tiene en cuenta: el umbral no lo admite el nivel de equipo; vale el del perfil o la configuración local.
- La configuración de equipo de `otro-repo` fija la rama base `develop` y la configuración local personal pone `release` → `develop`: la rama base solo la admite el nivel de equipo.
- El desarrollador cambia un valor en su configuración local → el motor aplica el nuevo valor sin que el motor escriba nada.

> ⚠️ **ASSUMPTION**: si un nivel de la configuración no se puede leer (p. ej. está mal formado), el motor avisa e ignora ese nivel, aplicando el siguiente, en lugar de dejar de observar `[POR VERIFICAR]`.

**Referencias**:
- User Story: US-GRP-013 (umbral), US-GRP-016 (rama base); las dos bloqueadas por P8
- BRD: BR-11
- Contexto: decisiones Q23, Q24, Q27 y Q36; preguntas P8 y P10; riesgo R12; dependencia con F-001-04
- Relacionada: BR-CONS-001, BR-CONS-006, BR-TIME-001

---

## 7. Reglas de Tiempo y Expiración

### BR-TIME-001: Umbral de inactividad de una sesión

**Descripción**: Una sesión pasa de "Activo" a "Inactivo" cuando su worktree lleva un tiempo sin actividad. El umbral es de **5 minutos por defecto** y el desarrollador lo puede **ajustar por repo**.

**Dónde vive** (Q20, refinada por Q23 y Q24): el umbral es una **preferencia personal** del desarrollador. Solo lo admiten dos niveles (BR-CONS-007): el perfil del usuario (valor por defecto personal global) y la configuración local personal de cada repo (ajuste por repo). La configuración del repo del equipo **nunca** lo define: si trae un umbral, no se tiene en cuenta. Si ninguno de los dos niveles admitidos lo define, vale 5 minutos. No se comparte con el equipo: otro clon del mismo repo usa su propio valor. Lo edita el desarrollador y el motor solo lo lee.

**Aplicabilidad**: BR-WF-001.

**Criticidad**: Media

**Regla temporal**:
```
Entidad: sesión de agente
Umbral: el de la configuración local personal del repo;
        si no lo define, el del perfil;
        si tampoco, 5 minutos sin actividad en su worktree
        (la configuración del repo del equipo no interviene)
Acción al cumplirse: la sesión pasa a Inactivo
```

**Ejemplo**:
- Repo con el umbral por defecto: última actividad 10:00:00; a las 10:05:00 sin actividad → Inactivo; cambio de archivo a las 10:07:00 → Activo.
- Repo con umbral ajustado a 15 minutos: última actividad 10:00:00; a las 10:05:00 sigue Activo; a las 10:15:00 sin actividad → Inactivo.
- Personal: el desarrollador ajusta `gitRaptor` a 15 minutos en su configuración local; en el clon de otra persona el mismo repo sigue con su propio valor.
- Equipo ignorado: la configuración del repo del equipo de `otro-repo` trae un umbral de 30 minutos y el desarrollador no lo ajusta en ningún nivel personal → 5 minutos.

**Referencias**:
- User Story: US-GRP-007, US-GRP-013
- Contexto: decisiones Q3, Q20, Q23 y Q24
- Relacionada: BR-CONS-007, BR-CONS-001 (tabla "Qué datos propios genera el motor y dónde viven")

---

## 8. Reglas Excepcionales (Edge Cases)

### BR-EDGE-001: Un repo o worktree deja de estar disponible

**Descripción**: Si un worktree se borra o un repo observado se mueve, se borra o queda inaccesible, el motor lo reporta como **no disponible** y sigue observando el resto con normalidad.

**Criticidad**: Alta

**Frecuencia esperada**: frecuente (los agentes crean y borran worktrees constantemente).

**Regla**: la caída de uno no interrumpe a los demás. Un worktree borrado termina sus sesiones (BR-WF-001).

**Ejemplo**: se borra `feat-login` mientras se observa; los otros 9 worktrees siguen reflejándose; las sesiones de `feat-login` pasan a "Terminado".

**Referencias**:
- User Story: US-GRP-003

---

### BR-EDGE-002: Estados especiales de Git

**Descripción**: Si un worktree está en un estado intermedio (rebase o merge en curso, HEAD separado, conflictos sin resolver), el motor lo reporta tal cual en lugar de presentarlo como un estado normal, y no intenta completarlo ni deshacerlo (BR-CONS-001).

**Criticidad**: Media

**Frecuencia esperada**: ocasional.

**Regla**: el estado especial forma parte del estado del worktree.

**Ejemplo**: un agente deja un rebase a medias en `feat-pagos` → el worktree se reporta "rebase en curso".

**Referencias**:
- User Story: US-GRP-003

---

### BR-EDGE-003: Actividad sin agente identificable: "sin atribuir"

**Descripción**: Si el motor ve actividad en un worktree pero no puede identificar a qué agente pertenece, la reporta como **"sin atribuir"** y no la atribuye a ningún agente. Pasa cuando una sesión deja de reconocerse (p. ej. porque Claude Code cambió su forma de ejecutarse) y cuando un agente sin soporte completo (Codex, Cursor u otro) trabaja sin haberse registrado: ese trabajo es indistinguible del del desarrollador (Q35). El desarrollador puede resolverlo registrando el agente.

> **Cambio Q35 (2026-10-03)**: antes esta actividad se reportaba como "no identificada", un valor distinto de "sin atribuir". Ahora hay un solo valor.

**Criticidad**: Media

**Frecuencia esperada**: rara, pero esperable tras actualizaciones de Claude Code (riesgo R1 del contexto; Q32: antes, "de Claude Code o Cursor"). Frecuente con agentes sin soporte completo que no se registran.

**Regla**: mejor "sin atribuir" que un agente equivocado. Nunca se presenta como de Claude Code.

**Referencias**:
- User Story: US-GRP-008

---

### BR-EDGE-004: En caso de duda entre humano y agente, no se atribuye al agente

**Descripción**: Cuando el motor no puede distinguir si un cambio lo hizo el humano o un agente, no lo atribuye al agente y lo marca "sin atribuir". Caso típico (Q32): el desarrollador edita en su editor (Cursor, VS Code u otro) en un worktree donde trabaja Claude Code, o en uno sin agentes; esa actividad nunca se atribuye a Claude Code ni a ningún agente. Que el editor sea Cursor no lo convierte en agente: Cursor solo cuenta como agente si se registra de forma explícita, y entonces es "otro agente" (BR-VAL-001). Un "deshacer lo que hizo el agente X" nunca debe alcanzar trabajo humano por una atribución dudosa.

> **Cambio Q32 (2026-10-03)**: antes el caso típico era "Cursor es a la vez el editor del humano y un agente". Con solo Claude Code soportado, el caso que importa es que el trabajo del humano en su editor no se atribuya a Claude Code.

**Criticidad**: Alta

**Frecuencia esperada**: frecuente cuando el desarrollador edita en el mismo worktree en el que trabaja Claude Code ⚠️ **ASSUMPTION** `[POR VERIFICAR]` en el spike (c) del BRD. *(Q32: antes, "frecuente con Cursor".)*

**Regla**: sin evidencia suficiente de que fue un agente, el cambio queda "sin atribuir". El editor del humano no es una sesión de agente. El motor no marca el cambio como "humano" (Q34): solo dice que no tiene agente; presentarlo como "tú u otro" es de la Time Machine y el Cockpit.

**Ejemplo**:
- El desarrollador modifica `README.md` en Cursor dentro de `feat-login` mientras Claude Code está activo allí, y el motor no tiene evidencia de que lo hiciera Claude Code → el cambio queda "sin atribuir".
- El desarrollador abre Cursor sobre `feat-pagos`, donde no hay ningún agente, y edita un archivo → no aparece ninguna sesión de agente y el cambio queda "sin atribuir".
- El desarrollador registra "Cursor" como agente en `feat-pagos` → desde ese momento es "otro agente: Cursor" y su actividad se le atribuye (BR-VAL-001).

**Referencias**:
- User Story: US-GRP-008
- Contexto: riesgo R2, decisión Q32

---

### BR-EDGE-005: Cambios ocurridos durante un hueco de observación

**Descripción**: Con BR-CONS-005 no debería haber huecos, pero pueden darse: por ejemplo, la máquina está apagada mientras otro proceso (otra máquina, un disco compartido) toca el repo. Al volver a observar, el motor **reconcilia el estado actual** del repo (ramas, worktrees, cambios, commits) y presenta los cambios ocurridos durante el hueco como **"sin atribuir"**, nunca atribuidos a un agente, aunque hubiera un agente registrado en ese worktree antes del hueco.

**Criticidad**: Alta

**Frecuencia esperada**: rara.

**Regla**: lo que el motor no vio ocurrir no se atribuye a ningún agente. El hueco queda señalado para que la Time Machine sepa que en ese periodo no hay atribución.

**Enmienda (2026-10-07), causa `dormant`**: lo que el motor encuentra en un repo dormido **sin que su vigilancia lo señalara**, solo en una comprobación periódica de respaldo, es un hueco de causa `dormant` ("observación en reposo"): queda "sin atribuir" como cualquier otro hueco. Lo que la vigilancia sí señaló al despertar el repo **no es hueco** y se atribuye con normalidad (BR-CONS-005). El motor lleva la cuenta de los huecos `dormant` como diagnóstico. Si esa cuenta deja de ser excepcional en el dogfooding, el PO revisa Q49.

**Otros casos que se tratan como hueco**: el tiempo en que un repo estuvo retirado de la observación y se vuelve a añadir (Q25), y la pérdida o el borrado del perfil de GitRaptor: el motor sigue funcionando, los repos se vuelven a añadir y lo ocurrido mientras no hubo datos queda "sin atribuir" (Q26). Una computadora nueva se comporta igual que un perfil perdido (BR-EDGE-007, Q31).

**Ejemplo**:
- Claude Code está registrado en `feat-login`; la máquina se apaga; mientras tanto, desde otra máquina se hacen dos commits en esa rama sobre el disco compartido. Al encender, el motor muestra el estado actual de `feat-login` y los dos commits como "sin atribuir".
- El perfil de GitRaptor se borra. El desarrollador vuelve a añadir `gitRaptor`; el motor muestra su estado actual, observa desde ese momento y presenta como "sin atribuir" todo lo anterior que ya no tiene datos.
- *(Enmienda 2026-10-07)* `docs-site` duerme. Una edición en él lo despierta y se publica sin hueco. Si la vigilancia hubiera fallado y el commit lo encontrara la comprobación de respaldo, ese commit quedaría "sin atribuir" en un hueco `dormant`.

**Referencias**:
- User Story: US-GRP-005, US-GRP-006
- Contexto: decisiones Q6, Q25 y Q26, supuesto S4, riesgo R13
- Relacionada: BR-EDGE-007 (máquina nueva: lo anterior a añadir el repo en ella tampoco tiene datos)

---

### BR-EDGE-006: Agente instalado después que GitRaptor

**Descripción**: El motor funciona sin ningún agente instalado: observa los repos y sus worktrees (estado, ramas, eventos de Git) y simplemente no hay sesiones de agente que mostrar; la actividad de esos worktrees queda "sin atribuir" (BR-EDGE-004; Q34: antes, "se atribuye al humano o queda sin atribuir"). Cuando el desarrollador instala Claude Code más tarde, el motor detecta sus sesiones en cuanto aparecen, **sin reinstalar ni reconfigurar GitRaptor** (Q29). *(Q32: antes, "Claude Code o Cursor"; un agente sin soporte completo instalado después se registra como "otro agente".)*

**Criticidad**: Media

**Frecuencia esperada**: frecuente en una máquina nueva y al incorporar un agente nuevo.

**Regla**: tener agentes instalados no es un requisito del motor. Que un agente aparezca después no exige ninguna acción sobre GitRaptor para que se detecten sus sesiones.

**Fuera de esta regla**: conectar el agente a GitRaptor vía MCP (BR-15 del BRD, p. ej. `raptor mcp install`) pertenece al Servidor MCP (F-001-05). Es una dependencia: la detección automática de las sesiones no la necesita; el registro explícito desde el propio agente vía MCP sí.

**Ejemplo**:
- El desarrollador observa `gitRaptor` sin agentes instalados: el motor reporta sus worktrees, ramas y commits, sin sesiones. Instala Claude Code, lo lanza en `feat-login` y el motor detecta la sesión de Claude Code en `feat-login` sin que haya que tocar GitRaptor. *(Q32: antes, el ejemplo usaba Cursor.)*
- Un "otro agente" (p. ej. Codex o Cursor) instalado después no se detecta automáticamente en el MVP: se registra de forma explícita, como siempre (BR-VAL-001).

**Referencias**:
- User Story: US-GRP-007
- BRD: BR-02, BR-15, D2
- Contexto: decisiones Q29 y Q32; dependencia con F-001-05
- Relacionada: BR-WF-001, BR-WF-002

---

### BR-EDGE-007: Computadora nueva: el motor empieza de cero

**Descripción**: Los datos del motor son de cada máquina (Q21, Q31): viven en el perfil de GitRaptor de esa máquina y no viajan con el repo. En una computadora nueva el perfil empieza vacío: el historial de eventos, la atribución, el registro de agentes, la lista de repos observados y la configuración personal (configuración de nivel perfil y configuración local personal de cada repo) de la máquina anterior **no se traen**. La **configuración del repo compartida con el equipo sí aplica desde el primer momento**, porque viaja versionada con el repo: al clonarlo y añadirlo, el motor lee de ella la rama base, igual que en la máquina anterior. Mientras el humano no la confirma (al instalar la protección de Guardrails o de forma explícita, nunca al añadir el repo; decisión heredada Q-GRD-23), el motor la marca como "no confirmada".

**Conexión con la pérdida del perfil** (S17, Q26): una máquina nueva se comporta como un perfil perdido. El motor funciona, los repos se añaden de nuevo y lo anterior a añadirlos en esta máquina no tiene datos del motor: el estado actual del repo y su historia de Git se ven, pero los cambios de antes quedan "sin atribuir" (BR-EDGE-005), nunca atribuidos a un agente.

**Criticidad**: Media

**Frecuencia esperada**: ocasional (cambio o reinstalación de equipo, incorporación de una persona de los equipos piloto).

**Regla**: perfil vacío en máquina nueva; la configuración del equipo sí aplica; la configuración personal y los datos del motor de otra máquina no. Exportar o importar el perfil entre máquinas queda **fuera del MVP** (fase posterior).

**Ejemplo**:
- El desarrollador estrena portátil, instala GitRaptor y Git, clona `gitRaptor` y lo añade. El motor observa desde ese momento; los commits que ya existían se ven en la historia, sin atribución a ningún agente. La configuración del equipo de `gitRaptor` define `develop` como rama base → el ahead/behind se calcula contra `develop` desde el primer momento, marcado como "no confirmado" hasta que el desarrollador confirme la rama base.
- En la máquina anterior tenía el umbral de inactividad de `gitRaptor` en 15 minutos en su configuración local personal; en la nueva no está → vale el del perfil y, como el perfil es nuevo, 5 minutos (BR-TIME-001).

**Referencias**:
- User Story: US-GRP-015 (perfil vacío, lo anterior "sin atribuir"), US-GRP-016 (la rama base del equipo aplica desde el primer momento; bloqueada)
- Contexto: decisiones Q21, Q26, Q31 y Q36; decisiones heredadas de Guardrails Q-GRD-21 y Q-GRD-23; supuesto S17
- Relacionada: BR-CONS-001, BR-CONS-006, BR-CONS-007, BR-TIME-001, BR-EDGE-005

---

## Matriz de Priorización

| Regla | Criticidad | Complejidad | Prioridad de Implementación |
|-------|------------|-------------|------------------------------|
| BR-CONS-001 | Alta | Media | 🔴 P0 |
| BR-AUTH-002 | Alta | Baja | 🔴 P0 |
| BR-CONS-005 | Alta | Alta | 🔴 P0 |
| BR-AUTH-001 | Alta | Baja | 🔴 P0 |
| BR-WF-001 | Alta | Alta | 🔴 P0 |
| BR-WF-002 | Alta | Media | 🔴 P0 |
| BR-CONS-002 | Alta | Baja | 🔴 P0 |
| BR-CONS-003 | Alta | Baja | 🔴 P0 |
| BR-EDGE-001 | Alta | Media | 🔴 P0 |
| BR-EDGE-004 | Alta | Alta | 🔴 P0 |
| BR-EDGE-005 | Alta | Media | 🔴 P0 |
| BR-VAL-001 | Alta | Baja | 🟡 P1 |
| BR-VAL-002 | Alta | Baja | 🟡 P1 |
| BR-EDGE-003 | Media | Media | 🟡 P1 |
| BR-EDGE-002 | Media | Baja | 🟡 P1 |
| BR-CONS-004 | Media | Media | 🟡 P1 |
| BR-CONS-006 | Media | Baja | 🟡 P1 |
| BR-CONS-007 | Media | Baja | 🟡 P1 |
| BR-TIME-001 | Media | Baja | 🟡 P1 |
| BR-VAL-003 | Media | Baja | 🟡 P1 |
| BR-EDGE-006 | Media | Baja | 🟡 P1 |
| BR-EDGE-007 | Media | Baja | 🟡 P1 |
| BR-AUTH-003 | Alta | Media | 🟡 P1 (sus historias son Should: el MVP funciona añadiendo repos a mano) |

**Leyenda**:
- 🔴 **P0**: Crítico. Sin esto el feature no funciona.
- 🟡 **P1**: Importante. Necesario para el MVP.
- 🟢 **P2**: Deseable. Puede ir en iteraciones posteriores.

---

## Trazabilidad

### Reglas → User Stories

Cada regla indica sus historias en su apartado "Referencias". La matriz completa está en el índice [`user-stories.md`](./user-stories.md) (sección "Cobertura de reglas"). Las 23 reglas tienen al menos una historia. BR-CONS-007 solo la cubren historias bloqueadas por P8 (US-GRP-013 y US-GRP-016), y la parte "configuración del equipo" de BR-CONS-006 y BR-EDGE-007 está en US-GRP-016 (Q36). BR-CONS-002 (corregir, US-GRP-010) y BR-CONS-004 (registrar otro agente, US-GRP-011) tienen escenarios que se contrastan entre sí (Q33); US-GRP-009 añade el registro que confirma una sesión ya detectada (Q39). BR-AUTH-001 exige que un agente no cambie los repos observados ya en una historia Must (US-GRP-001) y que no corrija atribuciones (US-GRP-010). BR-CONS-005 se comprueba también en la persistencia de una corrección (US-GRP-010) y de un worktree compartido (US-GRP-011). BR-CONS-001 (cero escrituras en el repo) y el mismo comportamiento en Windows, macOS y Linux se exigen además en todas las historias. **Enmienda 2026-10-07**: BR-AUTH-003 la cubren US-GRP-020 (raíces, primer nivel, raíz inválida, raíz en la configuración de un repo, agente que declara), US-GRP-022 (aceptar, descartar, agente que acepta), US-GRP-021 (`raptor clone`, no por defecto) y US-CKP-025 (la TUI pregunta con N por defecto; una TUI de agente no pregunta); la enmienda de BR-AUTH-001 se verifica en esas mismas historias.

### Reglas → Criterios de Aceptación

Cada regla debe estar reflejada en al menos un **escenario Gherkin** (criterio de aceptación del PO) de la user story correspondiente. BR-CONS-001 se verifica además con la comparación "antes y después" descrita en la propia regla (nada cambia en el repo; fuera de él solo cambian los datos del motor en el perfil), e incluye un escenario en el que preparar todos los cambios del repo después de observarlo no recoge nada del motor. BR-AUTH-002 necesita en el MVP un escenario negativo (el motor no modifica hooks, configuración de Git del repo ni metadatos de worktrees en ninguna situación) y uno de detección sin hooks (la detección funciona sin hooks propios y con los de Guardrails ausentes o desactivados); el modelo de permiso explícito no tiene escenarios en el MVP. BR-CONS-007 necesita escenarios de precedencia por valor (Q24): para el umbral de inactividad, la configuración local personal gana al perfil y un umbral en la configuración del equipo no se tiene en cuenta; para la rama base, solo cuenta la configuración del equipo y un valor en el perfil o en la configuración local personal no la cambia. Necesita además uno que compruebe que el motor no escribe ningún nivel. BR-EDGE-005 necesita, además del hueco por máquina apagada, un escenario de repo retirado y vuelto a añadir (sus datos anteriores siguen disponibles, Q25) y uno de perfil perdido (el motor sigue funcionando y lo no observado queda "sin atribuir", Q26). El primer uso (Q28-Q31) necesita los cinco escenarios de verificación de BR-WF-002: máquina nueva con GitRaptor instalado antes que Git, Git instalado después, Git antiguo actualizado, Claude Code instalado después y sin repos. BR-VAL-003 necesita además un escenario negativo (con Git ausente o antiguo no se observa ningún repo y el motor no instala ni actualiza Git) y BR-EDGE-007 uno de máquina nueva (perfil vacío, lo anterior "sin atribuir" y la rama base del equipo aplicada desde el primer momento). BR-AUTH-003 necesita escenarios negativos de agente (por MCP y desde su terminal) para declarar raíces y para aceptar, uno de raíz inválida, uno de primer nivel, uno de descarte que sobrevive al reinicio y uno de respuesta por defecto "no observar".

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-01 | PO (AADD) para Rene Bonilla | Versión inicial |
| 1.1 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisiones Q1-Q9: BR-CONS-001 reescrita y detallada como regla de solo lectura (derivada de NFR-01); BR-VAL-001 acepta "otro agente"; BR-TIME-001 configurable por repo; BR-CONS-004 sin atribución por archivo; nuevas BR-CONS-005, BR-CONS-006 y BR-EDGE-005; secciones 2 y 3 restauradas ("No aplica"); criticidad en todas las BR-EDGE; conteos y matriz actualizados |
| 1.2 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisiones Q10-Q12 (reemplazan a Q2): BR-CONS-001 pasa a "el motor nunca modifica el código fuente", con definiciones de código fuente y ruta operativa, excepción de efectos internos de lectura, tabla de quién puede modificar qué y verificación en dos partes; nueva BR-AUTH-002 (rutas operativas solo con permiso explícito del desarrollador, degradación sin bloqueo, petición pendiente si nadie responde, coordinación de hooks con Guardrails); BR-AUTH-001 añade el permiso operativo a la tabla; BR-VAL-003 con 2.38 fijada; escenarios de verificación con reinicio de la máquina y corrección de atribución; conteos (18 / 12) y matriz actualizados |
| 1.3 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisiones Q13-Q17: definición de código fuente confirmada (Q13); BR-AUTH-002 incorpora como reglas firmes la no repetición tras una denegación (Q14) y el alcance "un permiso, una modificación, un repo" (Q15), con ejemplos; retirar un repo no revierte ni ofrece revertir (Q16, fuera del MVP); ruta operativa limitada al repo observado y configuración global de Git intocable incluso con permiso (Q17), con un tercer punto de verificación. Se retiran los supuestos S9-S12. Q12 queda confirmada por Rene Bonilla. Sin reglas nuevas: conteos (18 / 12) y matriz sin cambios |
| 1.4 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisión Q18 (modelo híbrido perfil de GitRaptor + carpeta `.gitraptor/`) y tercera pasada del Artifact Judge: BR-CONS-001 añade la excepción acotada de los datos locales en `.gitraptor/` al punto 1, mantiene la configuración versionada del repo como código fuente, sustituye el "directorio propio" por `.gitraptor/`, reescribe la tabla de datos propios con la columna "Dónde vive", la verificación (dentro del repo solo `.gitraptor/` o rutas con permiso; fuera solo el perfil) y las restricciones, que ya no dicen "siempre fuera del repo"; BR-AUTH-002 añade el primer permiso de cada repo (una vez, al añadirlo; degradación y registro en el perfil si se deniega), la revocación disponible en repos retirados sin borrado de `.gitraptor/` (Q16) y la carpeta compartida con Guardrails; BR-WF-001 declara `Criticidad: Alta`; frontmatter con `related.context`. Sin reglas nuevas: conteos (18 / 12) y matriz sin cambios |
| 1.5 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisiones Q19 y Q20. Q19 refina Q18: crear `.gitraptor/` y escribir sus datos locales no requiere permiso (forma parte de añadir el repo); la carpeta es obligatoria y el motor informa de lo que creó. BR-AUTH-002 sustituye "El primer permiso de cada repo" por "La frontera: lo que no necesita permiso", amplía la aplicabilidad (hooks, configuración del repo, metadatos de worktrees, otras operaciones) y marca obsoleto el ejemplo "carpeta denegada"; añade los ejemplos "añadir un repo", "configuración del repo" y "carpeta borrada" (recrear e informar) y el supuesto S14. BR-CONS-001 saca `.gitraptor/` de las rutas operativas a una sección propia, marca obsoleta la fila "Primer permiso de cada repo" y el modo degradado, y cambia la verificación (carpeta creada con aviso; escenario de carpeta borrada en lugar de denegada). BR-AUTH-001 vincula añadir el repo con crear `.gitraptor/` (supuesto S15). Q20: BR-TIME-001 fija dónde vive el umbral (por defecto en el perfil, ajuste personal en `.gitraptor/`) y se retira la marca de supuesto de BR-CONS-001. S13 y P6 quedan resueltos y obsoletos. Sin reglas nuevas: conteos (18 / 12) y matriz sin cambios |
| 1.6 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisiones Q21-Q23: el motor solo observa. Q21 (reemplaza a Q18 y Q19): BR-CONS-001 pasa a "el motor no escribe nada en el repo observado", sin la excepción de la carpeta propia; todos los datos propios viven en el perfil, separados por repo; la tabla "Dónde vive" añade la columna de acceso (la configuración, solo lectura) y marca obsoleta la fila de permisos; la verificación queda en "nada cambia en el repo; fuera, solo el perfil"; se eliminan los casos de carpeta creada, borrada o denegada. Q22: BR-AUTH-002 se convierte en principio de frontera (sin modificaciones operativas en el MVP; señales opcionales de los hooks de Guardrails; el modelo de permiso explícito se conserva como principio, sin escenarios del MVP). BR-AUTH-001: añadir un repo no crea nada en él; se quitan las filas de permiso operativo y el supuesto de carpeta no creable; nuevo supuesto S16. Q23: nueva BR-CONS-007 (configuración en tres niveles, gana el más específico, el motor nunca la escribe); BR-CONS-006 lee la rama base del nivel de equipo; BR-TIME-001 lee el ajuste por repo de la configuración local personal. Se quitan las menciones al archivo de política. Conteos (19 / 12) y matriz actualizados |
| 1.7 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisiones Q24-Q27. Q24 resuelve la contradicción de precedencia que detectó el Artifact Judge: BR-CONS-007 añade la tabla "Niveles admitidos por valor" y la regla pasa a "gana el más específico entre los niveles que admite cada valor"; un valor en un nivel no admitido no se tiene en cuenta. BR-CONS-006: la rama base solo la admite la configuración del repo del equipo (`main` por defecto); un ajuste local o del perfil no la cambia. BR-TIME-001: el umbral solo lo admiten el perfil y la configuración local personal, nunca la del equipo (5 minutos por defecto). Ejemplos alineados. Q25 (supuesto S16 confirmado): BR-AUTH-001 fija como regla que retirar un repo no borra sus datos del perfil. Q26 (supuesto S17 confirmado): BR-CONS-001 y BR-EDGE-005 fijan el comportamiento ante la pérdida del perfil. Q27: el comando de edición de la configuración es de Guardrails (BR-CONS-007). Trazabilidad con escenarios de precedencia por valor, retiro y pérdida del perfil. Sin reglas nuevas: conteos (19 / 12) y matriz sin cambios |
| 1.8 | 2026-10-02 | PO (AADD) para Rene Bonilla | Decisiones Q28-Q31 (primer uso en una máquina nueva). Q28 (cierra P4): BR-VAL-003 pasa a "Git ausente o insuficiente: el motor avisa y espera"; sin observación parcial; el motor detecta solo cuando Git aparece o se actualiza y nunca instala ni actualiza Git; nuevo supuesto S19 (Git que deja de cumplir mientras se observa). Nueva BR-WF-002 (estados "Esperando Git", "Sin repos" y "Observando", con transiciones automáticas y los cinco escenarios de verificación del primer uso; supuesto S18). Q29: nueva BR-EDGE-006 (agente instalado después, sin reinstalar ni reconfigurar; conexión MCP en F-001-05). Q30: el estado "Sin repos" con guía para añadir el primero, dentro de BR-WF-002. Q31: nueva BR-EDGE-007 (máquina nueva: perfil vacío, la configuración del equipo sí aplica, exportar/importar el perfil fuera del MVP), conectada con S17/Q26 y BR-EDGE-005. Trazabilidad ampliada. Conteos (22 / 13) y matriz actualizados |
| 1.9 | 2026-10-03 | PO (AADD) para Rene Bonilla | Decisión Q32 (BRD v0.5, D2 revisada; cambio de alcance sobre el requerimiento ya aprobado): soporte completo solo para Claude Code; Codex, luego Cursor y más adelante Copilot se integran uno por uno y, mientras tanto, son "otro agente". BR-VAL-001 (regla formal y ejemplo de Cursor registrado), BR-WF-001, BR-AUTH-001, BR-CONS-002 y BR-CONS-004 (ejemplos sin detección de Cursor), BR-EDGE-003 (agente sin registrar: no identificado), BR-EDGE-004 reformulada (la actividad del humano en su editor, Cursor u otro, nunca se atribuye a Claude Code ni a ningún agente; ejemplos nuevos) y BR-EDGE-006 (detección al instalar después, solo Claude Code). Las frases sustituidas quedan marcadas con "Q32: antes, …". Trazabilidad Reglas → User Stories completada (US-GRP-001 a 015). Sin reglas nuevas: conteos (22 / 13) y matriz sin cambios |
| 1.10 | 2026-10-03 | PO (AADD) para Rene Bonilla | Decisiones Q33-Q36 tras el Artifact Judge de las historias. Q33: BR-CONS-002 pasa a "corregir una atribución reemplaza la detectada" (ya no "equivale a registrar"), con regla formal, ejemplos contrastados y los supuestos P14 (alcance retroactivo) y P15 (corregir sin detección); BR-CONS-004 aclara que registrar otro agente añade sesión y que una corrección no vuelve compartido el worktree; BR-AUTH-001 ajusta la fila "corregir". Q34: BR-CONS-003 fija los dos valores que emite el motor ("agente X" o "sin atribuir"; nunca "humano"); BR-VAL-001, la tabla de datos de BR-CONS-001, BR-EDGE-004 y BR-EDGE-006 dejan de hablar de atribuir al humano. Q35: BR-EDGE-003 reescrita ("sin atribuir" en lugar de "no identificada"). Q36: BR-CONS-006 añade la entrega en dos pasos (`main` provisional en US-GRP-012; configuración del equipo en US-GRP-016, bloqueada) y deja de citar US-GRP-001; BR-CONS-007 y BR-EDGE-007 referencian US-GRP-016. Trazabilidad actualizada. Sin reglas nuevas: conteos (22 / 13) y matriz sin cambios |
| 1.11 | 2026-10-03 | PO (AADD) para Rene Bonilla | Decisiones Q37-Q42 (segunda pasada del Artifact Judge, RESERVAS). Q37 (cierra P14): BR-CONS-002 reatribuye los eventos de la sesión mal detectada desde su inicio; los de otras sesiones no cambian. Q38 (cierra P15): sin atribución detectada no se corrige; el motor indica que se use el registro. BR-CONS-002 añade que solo corrige el desarrollador y que la corrección persiste al reiniciar el motor, con ejemplos; nuevo supuesto P17 (retirar la corrección y los eventos reatribuidos). Q39: BR-CONS-004 distingue registrar al mismo agente ya detectado (confirma la sesión, no duplica, no comparte) de registrar otro; nuevo supuesto P16 (origen de la sesión confirmada). Q40 (confirma S7): BR-AUTH-001 sin marca de supuesto y con la fila "un agente no corrige". Q41 (confirma S8): BR-WF-001 sin marcas de supuesto. Q42: BR-CONS-006, rama base inexistente sin marca de supuesto. S18 y S19 figuran como aceptados en BR-VAL-003 y BR-WF-002 (cierra P13). Referencias y trazabilidad actualizadas. Sin reglas nuevas: conteos (22 / 13) y matriz sin cambios |
| 1.12 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisiones heredadas de Guardrails Q-GRD-18, Q-GRD-20 y Q-GRD-21 (Rene Bonilla, revisión de arquitectura de Guardrails), posteriores a la aprobación del requerimiento. BR-CONS-006: la rama base se lee de la configuración del equipo commiteada en la copia conocida de la rama principal (no del archivo en disco del worktree principal); el ahead/behind se calcula contra la rama base confirmada, la misma que protege Guardrails; un cambio queda "pendiente de confirmar" y, sin confirmación inicial, se calcula contra la leída marcada como "no confirmada". BR-EDGE-007: la confirmación inicial al añadir el repo en una máquina nueva. Sin reglas nuevas. |
| 1.13 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisión heredada de Guardrails Q-GRD-23 (Rene Bonilla): la confirmación inicial de la rama base se hace al instalar la protección de Guardrails o de forma explícita, nunca al añadir el repo. BR-EDGE-007 vuelve a su sentido original (la rama base del equipo aplica desde el primer momento) con la marca "no confirmada" hasta la confirmación; BR-CONS-006 quita "al añadir el repo" como vía de confirmación. Sin reglas nuevas. |
| 1.14 | 2026-10-07 | PO (AADD) para Rene Bonilla | Decisión del orquestador (2026-10-07), validada por el PO, sobre las propuestas A1 a A3 aceptadas por Rene Bonilla (2026-10-06); decisiones Q43 a Q48 del contexto. Enmienda de BR-AUTH-001: el motor solo descubre dentro de las carpetas de código declaradas y descubrir nunca es observar (frase anterior marcada); nuevas filas de permisos. Nueva BR-AUTH-003 (raíces solo en el perfil, raíces válidas, primer nivel, avisos, confirmación humana con "no" por defecto, descarte persistente, el agente no decide; dos supuestos para Rene). BR-CONS-007 añade el valor "carpetas de código" (solo perfil). BR-WF-002: un repo descubierto no cuenta como observado. Conteos (23 / 14), matriz y trazabilidad actualizados |
| 1.15 | 2026-10-07 | PO (AADD) para Rene Bonilla | Ajustes tras el Arquitecto (TS-GRP-006, SEC-15, RES-11/12). BR-CONS-005: enmienda "un repo en reposo sigue observado" (despierta en ≤ 2 s p95; un repo sin vigilancia en vivo no duerme). BR-EDGE-005: causa de hueco `dormant` (solo lo que encuentra la comprobación de respaldo sin aviso de la vigilancia) con ejemplo. BR-AUTH-003: las raíces no son configuración; viven en el perfil y se gestionan con comandos reservados (`raptor repo roots`, `roots add`, `roots remove`, `repo dismiss`, `repo add`); raíces inválidas alineadas con SEC-15 (ancestros de la carpeta personal, perfil, rutas de red, tope de 16). Se quita de BR-CONS-007 la fila de las raíces añadida en la 1.14 |
| 1.16 | 2026-10-07 | PO (AADD) para Rene Bonilla | **Decisión de Rene (2026-10-07)**: la carpeta personal puede ser una raíz (cierra el pendiente del PR #142). BR-AUTH-003, condición 2: raíz amplia con aviso y confirmación (> 512 entradas, carpeta personal u otro volumen), exclusiones fijas por SO; siguen rechazados `/`, la unidad del sistema y los ancestros de las carpetas personales. Decisión del orquestador (2026-10-07), validada por el Arquitecto |
