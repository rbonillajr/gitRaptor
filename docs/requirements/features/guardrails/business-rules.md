---
id: BR-GRD-001
title: "Reglas de Negocio — Guardrails"
type: business-rules
status: draft
created: 2026-10-03
updated: 2026-10-04
domain: GRP
epic: E-001
feature: guardrails
related:
  context:
    - CTX-GRD-001
  stories:
    - US-GRD-001
    - US-GRD-002
    - US-GRD-003
    - US-GRD-004
    - US-GRD-005
    - US-GRD-006
    - US-GRD-007
    - US-GRD-008
    - US-GRD-009
    - US-GRD-010
    - US-GRD-011
    - US-GRD-012
    - US-GRD-013
    - US-GRD-014
    - US-GRD-015
    - US-GRD-016
    - US-GRD-017
tags:
  - guardrails
  - politicas
  - permisos
  - configuracion-tres-niveles
  - pedir-confirmacion
  - hooks-git
  - registro-decisiones
---

# Reglas de Negocio: Guardrails

> **Propósito**: Documentar las reglas que deciden si una operación de Git de un agente se permite, se deniega o espera la confirmación de un humano; cómo se combinan los tres niveles de la configuración; quién puede cambiar las reglas y decidir en la cola; y cómo se instala la protección de hooks sin perder nada del usuario.
>
> **Nota de nomenclatura**: `BR-11`, `BR-12` y `BR-13` del BRD son **capacidades**. Las reglas de este documento usan la forma `BR-<CAT>-NNN` y son internas a esta feature. Cualquier ID del Motor local se califica siempre con "(motor-local)", p. ej. "BR-AUTH-002 (motor-local)" o "P8 (motor-local)"; sin esa marca, un ID `BR-<CAT>-NNN` es de esta feature. Las decisiones heredadas se citan como "Q21 de motor-local". Las preguntas, decisiones, supuestos y riesgos propios llevan el prefijo de la feature (P-GRD-n, Q-GRD-n, S-GRD-n, R-GRD-n) y están en el [contexto](./context.md). Las 16 preguntas P-GRD-1 a P-GRD-16 están resueltas por las decisiones Q-GRD-1 a Q-GRD-16 (2026-10-03).
>
> **Historias**: la cobertura regla → historias está en el [índice de historias](./user-stories.md#cobertura-de-reglas-regla--historias).

---

## Contexto

**Feature**: Guardrails (F-001-04)
**Enlace a contexto**: [`context.md`](./context.md) (CTX-GRD-001)
**Última actualización**: 2026-10-04 (Q-GRD-17 a Q-GRD-27 y sus aplicaciones derivadas: Q-GRD-12 en la versión del worktree, confirmación inicial y configuración antes de confirmar, unión de la rama base por historia; ver Changelog). Antes, 2026-10-03 (versión inicial)

---

## Categorías de Reglas

| Categoría | Cantidad | Críticas |
|-----------|----------|----------|
| Validaciones de Datos | 3 | 3 |
| Cálculos de Negocio | 1 | 1 |
| Reglas de Elegibilidad | 0 | 0 |
| Workflows y Estados | 2 | 1 |
| Permisos y Autorizaciones | 4 | 4 |
| Reglas de Consistencia de Datos | 6 | 4 |
| Reglas de Tiempo y Expiración | 2 | 0 |
| Reglas Excepcionales (Edge Cases) | 5 | 5 |
| **Total** | **23** | **18** |

"Críticas" = reglas con criticidad Alta.

---

## 1. Validaciones de Datos

### BR-VAL-001: Cada valor de la configuración declara qué niveles lo admiten

**Descripción**: La configuración tiene tres niveles, de menor a mayor especificidad: **perfil del usuario**, **configuración del equipo** (versionada con el repo, BRD BR-11) y **configuración local personal del repo** (no versionada) (Q23 de motor-local). Cada valor declara en qué niveles se puede definir (Q24 de motor-local). Un valor escrito en un nivel que no lo admite no se tiene en cuenta. El comando de edición (Q27 de motor-local) **rechaza** escribirlo y explica qué niveles lo admiten.

> **Decisión** (Q-GRD-17, Rene Bonilla, 2026-10-04): la configuración del equipo que rige una operación es la **última versión commiteada en el worktree donde ocurre esa operación**; las ediciones sin commitear nunca cuentan. **Excepción**: la rama base se lee de la rama principal del repo (Q-GRD-18, BR-CONS-003). Los niveles personales (perfil y configuración local personal, que no se versionan) no cambian: rige su contenido actual.

> **Decisión** (Q-GRD-20, Rene Bonilla, 2026-10-04; refina Q-GRD-17): lo que **relaja** (desactivar el conjunto mínimo, un "permitir" explícito, cualquier valor menos restrictivo que el valor por defecto) y la rama base se leen **solo** de la configuración del equipo commiteada en la **rama principal**: la copia que el repo ya conoce del remoto, sin consultarlo; si no hay remoto, la rama local (BR-CONS-003). La versión commiteada en el worktree de la operación **solo puede endurecer**: se combina con la de la rama principal como un nivel personal (BR-CONS-001). Un endurecimiento que ya está en la rama principal rige en todos los worktrees, aunque no lo hayan integrado.
>
> **Decisión** (Q-GRD-21, Rene Bonilla, 2026-10-04): una relajación que llega por un cambio de la configuración del equipo en la rama principal **no se aplica hasta que el humano la confirma en esa máquina** (BR-AUTH-001). Mientras tanto rige la combinación más restrictiva de la configuración confirmada y la nueva, con aviso. Un endurecimiento se aplica al momento.

> **Aplicación de Q-GRD-21 y Q-GRD-23** (Rene Bonilla, 2026-10-04): mientras no hay confirmación inicial, la configuración del equipo de la rama principal **solo endurece**, igual que la versión de un worktree, y el **conjunto mínimo sigue aplicando** aunque esa configuración lo desactive. Ejemplo: un repo cuya configuración del equipo desactiva el mínimo y deniega push se protege sin confirmar esa configuración → push denegado (endurece) y force-push denegado por el mínimo, hasta que el desarrollador la confirma de forma explícita.

> **Decisión** (Q-GRD-26, Rene Bonilla, 2026-10-04; D12): una **clave desconocida** dentro de los permisos o de las políticas de un nivel (por ejemplo, una errata) no se ignora en silencio: ese nivel queda **parcial** y se trata como indica BR-EDGE-004. Una errata nunca relaja.

**Aplicabilidad**: Al leer la configuración y al editarla con el comando.

**Criticidad**: Alta

**Valores y niveles admitidos**:

| Valor | Niveles que lo admiten | Si nadie lo define | Cómo se combinan | Dueño |
|-------|------------------------|--------------------|------------------|-------|
| Permiso de cada operación gobernada (BR-VAL-002) | Perfil, equipo, local personal (Q-GRD-14) | Permitir, salvo el conjunto mínimo (BR-EDGE-001) | Los personales solo endurecen (BR-CONS-001, Q-GRD-14) | Guardrails |
| Ramas protegidas | Perfil, equipo, local personal (Q-GRD-14) | Ninguna, salvo el conjunto mínimo | Los personales solo añaden ramas | Guardrails |
| Límite de tamaño de diff | Perfil, equipo, local personal (Q-GRD-14) | Sin límite | Los personales solo lo bajan | Guardrails |
| Formato de commit | Perfil, equipo, local personal (Q-GRD-14) | Sin formato exigido | Si el equipo lo fija, un personal no lo cambia (S-GRD-2) | Guardrails |
| Rutas prohibidas | Perfil, equipo, local personal (Q-GRD-14) | Las de la configuración de Guardrails (BR-AUTH-004) | Los personales solo añaden rutas | Guardrails |
| Plazo de respuesta de la cola | Perfil, equipo, local personal (Q-GRD-14) | 5 minutos (Q-GRD-6) | Los personales solo lo acortan | Guardrails |
| Rama base | **Solo equipo** | `main` | Sin combinación; se lee de la versión commiteada en la rama principal del repo (Q-GRD-18, Q-GRD-20) y rige la confirmada por el humano (Q-GRD-21) | Motor local lee; Guardrails define (BR-CONS-003) |
| Umbral de inactividad | **Solo perfil y local personal** | 5 minutos | Gana el más específico | Motor local (BR-TIME-001 (motor-local)) |

Un valor nuevo tiene que declarar sus niveles al incorporarse a esta tabla. Los niveles de la rama base y del umbral los fijó el Motor local (Q24 de motor-local) y Guardrails los respeta.

**Regla formal**:
```
IF un nivel define un valor que ese nivel no admite
THEN al leer: el valor no se tiene en cuenta
     al editar con el comando: se rechaza e informa de los niveles admitidos
```

**Ejemplos de Q-GRD-17, Q-GRD-20 y Q-GRD-21**:
- En el worktree `feat-x` alguien edita la configuración del equipo para permitir force-push y no lo commitea → en `feat-x` rige la versión commiteada, que lo deniega.
- En el worktree `feat-x` hay un commit cuya configuración del equipo permite force-push o desactiva el conjunto mínimo; la rama principal lo deniega → en `feat-x` se sigue denegando: el worktree no relaja (Q-GRD-20).
- En el worktree `feat-x` hay un commit cuya configuración del equipo deniega push → en `feat-x` se deniega push: el worktree sí endurece (Q-GRD-20).
- Llega a la rama principal un cambio que deniega push; el worktree `feat-y` sigue en un commit anterior → en `feat-y` también se deniega push desde ese momento (Q-GRD-20, Q-GRD-21).
- Llega a la rama principal un cambio que permite force-push → el force-push se sigue denegando, con aviso de relajación pendiente, hasta que el humano la confirma en esa máquina (Q-GRD-21).

**Ejemplos**:
- El desarrollador intenta fijar la rama base `develop` en su perfil → el comando lo rechaza: "la rama base solo se define en la configuración del equipo".
- El desarrollador intenta fijar un umbral de inactividad de 30 minutos en la configuración del equipo → el comando lo rechaza: "el umbral solo se define en el perfil o en la configuración local personal".
- Alguien edita a mano la configuración del equipo y pone un umbral de inactividad → no se tiene en cuenta.
- La configuración del equipo escribe mal el nombre de un permiso (`forse-push: permitir`) → el nivel queda parcial: aviso, y el conjunto mínimo sigue aplicando (Q-GRD-26, BR-EDGE-004).

**Cómo se verifica**: por cada valor de la tabla, escribirlo en un nivel no admitido (a mano y con el comando) y comprobar que no cambia el valor efectivo y que el comando lo rechaza con los niveles admitidos.

**Referencias**: BRD BR-11; Q23, Q24, Q27 de motor-local; BR-CONS-007 (motor-local); Q-GRD-6, Q-GRD-14, Q-GRD-17, Q-GRD-18, Q-GRD-20, Q-GRD-21, Q-GRD-26; S-GRD-2.

---

### BR-VAL-002: Operaciones gobernadas y su permiso

**Descripción**: Guardrails evalúa estas operaciones de Git antes de que ocurran: **commit, push, force-push, `reset --hard`, borrar rama, rebase, merge, crear worktree y borrar worktree**. Cada una tiene un permiso: **permitir**, **pedir confirmación** o **denegar**. Las operaciones fuera de esta lista no las gobierna Guardrails en el MVP.

**Aplicabilidad**: Toda operación gobernada en un repo con Guardrails, por cualquiera de las dos capas (BR-CONS-002).

**Criticidad**: Alta

**Regla formal**:
```
operación ∈ {commit, push, force-push, reset --hard, borrar rama, rebase, merge, crear worktree, borrar worktree}
permiso(operación) ∈ {permitir, pedir confirmación, denegar}
orden de restricción: denegar > pedir confirmación > permitir
```

> **Supuesto confirmado** (S-GRD-9, Rene Bonilla, 2026-10-04): mientras el modo "pedir confirmación" (BRD BR-13, Should) no esté disponible, una operación con "pedir confirmación" se trata como **denegar** (fail-safe).

**Ejemplos**:
- Configuración del equipo: force-push = denegar → un agente intenta un force-push → denegado.
- Configuración del equipo: borrar worktree = pedir confirmación → un agente intenta borrar un worktree → la operación queda en la cola (BR-WF-001).
- Configuración del equipo: commit = permitir y sin políticas → un agente hace commit → permitido.

**Cómo se verifica**: para cada operación del catálogo y cada uno de los tres permisos, lanzar la operación como agente y comprobar el resultado (ejecutada, en cola o rechazada) en las dos capas.

**Referencias**: BRD BR-11, BR-12, BR-13; contexto § 2.

---

### BR-VAL-003: Políticas del repo

**Descripción**: Además del permiso por operación, el repo puede tener las políticas de BRD BR-11. Una política se incumple según la tabla y su efecto es **denegar**, salvo que la configuración la marque como "pedir confirmación".

| Política | Qué se incumple | Ejemplo |
|----------|-----------------|---------|
| **Rama protegida** | Un commit, push, force-push, `reset --hard` o borrado sobre esa rama. La rama solo cambia por una acción consciente del humano (Q-GRD-1) | Un agente hace commit directo en `main` protegida → denegado |
| **Prohibir force-push** | Cualquier force-push. Equivale a fijar "denegar" en el permiso de force-push | Un agente hace force-push sobre su propia rama → denegado |
| **Prohibir `reset --hard`** | Cualquier `reset --hard`. Equivale a fijar "denegar" en su permiso. Límite de la capa de hooks: BR-EDGE-003 | Un agente pide `reset --hard` por MCP → denegado |
| **Límite de tamaño de diff** | Un commit cuyo diff supera el límite, en líneas cambiadas (S-GRD-7) | Límite 400; un commit de 1.200 líneas → denegado, con el tamaño y el límite |
| **Formato de commit** | Un commit cuyo mensaje no cumple el formato fijado. El MVP ofrece al menos Conventional Commits (confirmado por Rene Bonilla, 2026-10-04) | "arreglos varios" con Conventional Commits exigido → denegado, con un ejemplo válido |
| **Ruta prohibida** | Un commit que modifica, crea o borra una ruta prohibida | Ruta prohibida `secrets/`; un commit que toca `secrets/api.txt` → denegado |

**Aplicabilidad**: Toda operación gobernada a la que la política se refiere.

**Criticidad**: Alta

**Regla formal**:
```
IF la operación incumple una política activa
THEN la política aporta su efecto (denegar, o pedir confirmación si así está marcada)
     a la decisión efectiva (BR-CALC-001)
```

**Prioridad dentro del MVP** (Q-GRD-9): primero ramas protegidas, force-push, `reset --hard` y rutas prohibidas; después tamaño de diff y formato de commit.

**Cómo se verifica**: para cada política, una operación que la cumple (permitida) y otra que la incumple (denegada, con la política nombrada en el motivo), en las dos capas.

**Referencias**: BRD BR-11, § 4 ("que nadie rompa main"), § 13 (demo de force-push); S-GRD-7, Q-GRD-1, Q-GRD-9.

---

## 2. Cálculos de Negocio

### BR-CALC-001: La decisión efectiva es la más restrictiva de las reglas que aplican

**Descripción**: Para una operación pueden aplicar varias reglas a la vez: su permiso y una o más políticas. La **decisión efectiva** es la más restrictiva de todas: denegar > pedir confirmación > permitir. La decisión siempre dice **qué regla la causó y de qué nivel viene**. Si varias reglas deniegan, se nombran todas.

**Aplicabilidad**: Cada vez que se evalúa una operación gobernada.

**Criticidad**: Alta

**Fórmula**:
```
reglas = { permiso(operación) } ∪ { efecto de cada política incumplida }
decisión = máximo(reglas) según denegar > pedir confirmación > permitir
motivo = las reglas que producen ese máximo, con su nivel de origen
```

**Ejemplos**:
- Push a `main`: permiso de push = permitir; `main` protegida → **denegar**, motivo "rama protegida `main` (configuración del equipo)".
- Commit en la rama del agente: permiso = permitir; diff dentro del límite; formato correcto → **permitir**.
- Borrar worktree: permiso = pedir confirmación; ninguna política aplica → **pedir confirmación**.
- Commit que toca una ruta prohibida y supera el límite de diff → **denegar**, motivo con las dos políticas.

**Cómo se verifica**: combinaciones de permiso y políticas con resultado esperado; el motivo nombra la regla y el nivel.

**Referencias**: BRD BR-11; contexto § 6 (explicabilidad).

---

## 3. Reglas de Elegibilidad

No aplica: Guardrails no decide quién puede usar el producto. Quién puede hacer cada acción está en § 5.

---

## 4. Workflows y Estados

### BR-WF-001: Ciclo de vida de una petición de confirmación

**Descripción**: Cuando la decisión efectiva es "pedir confirmación", la operación no se ejecuta: se crea una **petición** en la cola del repo. La petición lleva qué operación es, en qué repo, worktree y rama, qué actor la pidió ("agente X" o "sin atribuir"), qué regla la causó y cuándo se pidió. **Solo el humano decide**; un agente nunca aprueba ni rechaza, ni la suya ni la de otro (BR-AUTH-001). Presentar la cola es del Cockpit (F-001-02).

**Criticidad**: Alta

**Estados posibles**:
1. Pendiente
2. Aprobada
3. Rechazada
4. Caducada

**Transiciones permitidas**:
```
(nueva) → Pendiente
  Condición: decisión efectiva = pedir confirmación
  Actor: Guardrails (automático)
  Acción: la operación espera; se anota en el registro (BR-CONS-004)

Pendiente → Aprobada
  Condición: el humano la aprueba dentro del plazo
  Actor: desarrollador (nunca un agente)
  Acción: la operación concreta se ejecuta una vez (S-GRD-8), con el snapshot previo si es destructiva (BR-EDGE-005)

Pendiente → Rechazada
  Condición: el humano la rechaza
  Actor: desarrollador (nunca un agente)
  Acción: la operación no se ejecuta; el agente recibe el motivo

Pendiente → Caducada
  Condición: se cumple el plazo sin decisión (BR-TIME-001)
  Actor: Guardrails (automático)
  Acción: la operación no se ejecuta (fail-safe); el agente recibe el motivo
```

**Diagrama de estados**:
```
[Pendiente] → [Aprobada]
     ├──────→ [Rechazada]
     └──────→ [Caducada]
```

**Reglas adicionales**:
- Aprobada, Rechazada y Caducada son finales. Una petición no se reabre: si el agente lo vuelve a intentar, es una petición nueva.
- Una aprobación vale para esa operación concreta, una sola vez; no cambia la configuración ni crea un permiso permanente (S-GRD-8).
- Toda transición queda en el registro de decisiones (BR-CONS-004).

**Ejemplos**:
- Claude Code pide borrar el worktree `feat-x` → Pendiente → el humano aprueba a los 40 s → Aprobada → el worktree se borra.
- Claude Code pide un rebase de `feat-y` → Pendiente → el humano rechaza → Rechazada → nada cambia; Claude Code recibe "rechazada por el desarrollador".
- Nadie responde en 5 minutos → Caducada → nada cambia.

**Cómo se verifica**: una petición por cada transición; un agente que intenta aprobar su propia petición (rechazado); reintento tras rechazo crea una petición nueva.

**Referencias**: BRD BR-13 (Should); S-GRD-8, Q-GRD-6, Q-GRD-13; dependencia con F-001-02.

---

### BR-WF-002: Estado de protección de cada repo

**Descripción**: El desarrollador siempre puede saber qué capas aplican las reglas en cada repo. Guardrails expone uno de cuatro estados, y el Cockpit y la CLI los presentan. El estado depende **solo de las capas activas**, no de que haya configuración: con una capa activa y sin configuración, aplica el conjunto mínimo por defecto (BR-EDGE-001). Un repo sin configuración nunca queda "sin protección" por ese motivo.

**Alcance de Guardrails** (cuándo un repo entra):

> **Decisión** (Q-GRD-15, Rene Bonilla, 2026-10-03): un repo entra en el alcance de Guardrails cuando el desarrollador lo añade a la observación del motor (BR-AUTH-001 (motor-local)), para que cada operación tenga actor atribuido. La allowlist del MCP (NFR-02) es un subconjunto de los repos observados, a coordinar con F-001-05. Instalar la protección de hooks solo se ofrece en un repo observado. Retirar un repo de la observación no desinstala sus hooks: siguen aplicando las reglas con actor "sin atribuir" y Guardrails avisa de ello.

**Las dos vías de un agente**: por MCP y por Git crudo. La vía MCP está **cubierta** si el repo está en la allowlist (las herramientas aplican la decisión) y **cerrada** si no lo está (las herramientas no operan sobre él, NFR-02). La vía Git crudo solo está cubierta si los hooks de Guardrails están activos.

**Estados posibles**:
1. **Sin protección**: ninguna capa activa. Ni allowlist del MCP ni hooks de Guardrails. Guardrails no evalúa nada en el repo. Es el estado de un repo recién observado.
2. **Solo MCP**: el repo está en la allowlist del MCP y no tiene hooks activos. Git crudo no está cubierto.
3. **Solo hooks**: el repo tiene hooks de Guardrails activos y no está en la allowlist del MCP. Git crudo está cubierto; la vía MCP está cerrada para ese repo.
4. **Completa**: allowlist del MCP y hooks activos.

**Transiciones permitidas**:
```
Sin protección → Solo MCP        el repo se añade a la allowlist del MCP (F-001-05)
Sin protección → Solo hooks      el desarrollador instala la protección de hooks (BR-AUTH-002)
Solo MCP → Completa              el desarrollador instala la protección de hooks (BR-AUTH-002)
Solo hooks → Completa            el repo se añade a la allowlist del MCP
Completa → Solo MCP              el desarrollador desinstala los hooks, o dejan de estar activos por otra causa
Completa → Solo hooks            el repo sale de la allowlist del MCP
Solo MCP → Sin protección        el repo sale de la allowlist del MCP
Solo hooks → Sin protección      el desarrollador desinstala los hooks, o dejan de estar activos por otra causa

Acción en toda transición: se anota en el registro (BR-CONS-004)
Acción si los hooks dejan de estar activos sin que el desarrollador los desinstale desde Guardrails
  (borrados, reemplazados, desactivados): se avisa al desarrollador
```

**Diagnósticos visibles** (Q-GRD-25, Rene Bonilla, 2026-10-04; D11): los cuatro estados no cambian. Además, el estado de protección muestra estos diagnósticos, cada uno con la acción para confirmar:
- **Relajación pendiente de confirmar**: llegó a la rama principal un cambio de la configuración del equipo que relaja algo y el desarrollador aún no lo confirmó en esa máquina (Q-GRD-21). Mientras tanto rige la combinación más restrictiva.
- **Rama base no confirmada o pendiente**: el repo aún no tiene confirmación inicial de su rama base (Q-GRD-23), o llegó a la rama principal un cambio de rama base sin confirmar (Q-GRD-21).

Un diagnóstico no es un estado: no cambia qué capas están activas. Desaparece cuando el desarrollador confirma.

**Criticidad**: Media

**Ejemplos**:
- Repo recién observado, sin allowlist ni hooks → Sin protección.
- Se añade a la allowlist del MCP y no tiene configuración → Solo MCP, con el conjunto mínimo aplicado (BR-EDGE-001).
- Repo con hooks instalados que no está en la allowlist → Solo hooks: un agente con Git crudo recibe la decisión; las herramientas MCP no operan sobre ese repo.
- Otro gestor de hooks reemplaza los de Guardrails en un repo Completa → Solo MCP, con aviso.
- Se retira el repo de la observación con hooks activos → los hooks siguen aplicando las reglas, con actor "sin atribuir", y se avisa (Q-GRD-15).
- Repo Solo hooks con la rama base confirmada al instalar; llega a la rama principal un cambio que desactiva el conjunto mínimo → sigue en Solo hooks, con el diagnóstico "relajación pendiente de confirmar" y la acción para confirmarla (Q-GRD-25).
- Repo Solo MCP sin confirmación inicial de su rama base → sigue en Solo MCP, con el diagnóstico "rama base no confirmada" y la acción para confirmarla (Q-GRD-25).

**Cómo se verifica**: un repo en cada uno de los cuatro estados; cada transición de la lista; retirar los hooks por fuera de Guardrails y comprobar el cambio de estado y el aviso; un repo sin configuración en Solo MCP aplica el conjunto mínimo; cada diagnóstico aparece con su acción y desaparece al confirmar, sin cambiar el estado.

**Referencias**: BRD BR-12; NFR-02; BR-AUTH-001 (motor-local); BR-EDGE-001; Q-GRD-4, Q-GRD-15, Q-GRD-21, Q-GRD-23, Q-GRD-25; dependencia con F-001-02 y F-001-05.

---

## 5. Permisos y Autorizaciones

### BR-AUTH-001: Acciones reservadas al humano

**Descripción**: Algunas acciones solo las puede hacer el desarrollador, nunca un agente. Como un agente puede usar la terminal, estas acciones exigen una confirmación que un agente no pueda dar desde su canal (MCP o la terminal que usa). Cómo se logra lo decide el Arquitecto (riesgo R-GRD-3 del contexto).

> **Decisión** (Q-GRD-19, Rene Bonilla, 2026-10-04; D5): en el MVP se **acepta un riesgo residual** para desinstalar la protección y para la excepción consciente: cada uso se anuncia, abre una ventana en la que se puede cancelar y queda auditado, con la aceptación del riesgo en cada acción. **Relajar con el comando de edición y aprobar una petición de la cola exigen un factor de autenticación del sistema operativo**, fuera del canal del agente; sin él, esas acciones no se ofrecen (gate antes de US-GRD-013 y US-GRD-015).
>
> **Decisión** (Q-GRD-21 y Q-GRD-22, Rene Bonilla, 2026-10-04; D7 y D8): **confirmar en una máquina un cambio de la configuración del equipo que relaja** (incluidos desactivar el conjunto mínimo y cambiar la rama base) es una acción reservada nueva. También lo es la confirmación inicial de la rama base y de la configuración del equipo, que se hace **al instalar la protección o con una confirmación explícita**, nunca al añadir el repo a la observación (Q-GRD-23). Si esa configuración inicial trae relajaciones (por ejemplo, desactivar el conjunto mínimo), la confirmación pasa por el anuncio y la ventana para cancelar de Q-GRD-19. Usa el mismo mecanismo que las demás acciones reservadas del MVP y muestra qué cambia y de dónde viene. El factor de autenticación del sistema operativo se aplicará también aquí cuando exista, pero no la bloquea.

**Aplicabilidad**: Siempre.

**Criticidad**: Alta

**Tabla de permisos**:

| Acción | Desarrollador | Agente (Claude Code u otro) | Condiciones |
|--------|---------------|-----------------------------|-------------|
| Editar la configuración para endurecer | ✅ | ❌ por defecto (BR-AUTH-004) | — |
| Editar la configuración para relajar | ✅ | ❌ | Confirmación que un agente no pueda dar, con el factor de autenticación del sistema operativo (Q-GRD-19) |
| Aprobar o rechazar una petición de la cola | ✅ | ❌ | Ni la suya ni la de otro agente. Aprobar exige el factor de autenticación del sistema operativo (Q-GRD-19) |
| Instalar o desinstalar la protección de hooks | ✅ | ❌ | Con permiso explícito (BR-AUTH-002). Desinstalar: anuncio, ventana para cancelar y auditoría (Q-GRD-19) |
| Usar una excepción consciente para saltarse una regla (Q-GRD-1), incluida la aprobación explícita en una superficie de GitRaptor (Q-GRD-24) | ✅ | ❌ | Una operación concreta; queda registrada. Anuncio, ventana para cancelar y auditoría (Q-GRD-19, Q-GRD-24) |
| Confirmar en su máquina una relajación de la configuración del equipo o un cambio de rama base, y la confirmación inicial de ambas al instalar la protección o de forma explícita (Q-GRD-21, Q-GRD-23) | ✅ | ❌ | Por máquina. Mismo mecanismo que las demás acciones reservadas del MVP, con qué cambia y de dónde viene a la vista (Q-GRD-22) |
| Realizar operaciones gobernadas | ✅ (ver BR-AUTH-003) | ✅ | Sujetas a la decisión (BR-CALC-001) |

**Ejemplos**:
- Claude Code intenta aprobar su petición de rebase por MCP → no existe esa herramienta; por la CLI → rechazado.
- Claude Code ejecuta el comando de edición para cambiar force-push a "permitir" → rechazado.
- Claude Code intenta confirmar el cambio de rama base de `main` a `develop` que llegó a la rama principal → rechazado; el cambio sigue pendiente.
- El desarrollador confirma ese cambio → se anuncia, puede cancelarlo durante la ventana y, si no lo cancela, se aplica en su máquina y queda auditado.

**Cómo se verifica**: cada acción reservada intentada desde el canal del agente (MCP y terminal del agente) queda rechazada y registrada.

**Referencias**: BRD BR-13 (las acciones de riesgo de un agente quedan para que un humano las apruebe); Q27 de motor-local (el comando de edición es de Guardrails); riesgos R-GRD-3 y R-GRD-10 y decisiones Q-GRD-7, Q-GRD-19, Q-GRD-21, Q-GRD-22, Q-GRD-23 y Q-GRD-24 de esta feature.

---

### BR-AUTH-002: Instalar la protección de hooks requiere el permiso explícito del desarrollador

**Descripción**: Instalar o desinstalar la protección de hooks es una **modificación operativa** del repo. Guardrails es la única feature que la hace (Q22 de motor-local). Sigue el modelo de permiso explícito de BR-AUTH-002 (motor-local):

1. La petición explica en lenguaje claro **qué** se instala, **dónde** (repo), **por qué** (qué cubre) y **cómo se revierte**, y qué pasa si no se autoriza (el repo sigue sin la capa de hooks (Solo MCP o Sin protección, según la allowlist)).
2. **Solo el humano concede**. Un agente no puede conceder, pedir en nombre del desarrollador ni dar por concedido. Sin respuesta, no se instala nada.
3. **Sin repreguntar tras una denegación**, hasta que el desarrollador lo active a mano.
4. **Un permiso, un repo**: no se extiende a otros repos. Cambiar lo instalado requiere un permiso nuevo.

> **Decisión** (Q-GRD-3, Rene Bonilla, 2026-10-03): la instalación es explícita; nunca automática al observar un repo.

**Aplicabilidad**: Al instalar, actualizar o desinstalar la protección de hooks en un repo.

**Criticidad**: Alta

**Ejemplos**:
- El desarrollador pide proteger `gitRaptor` → ve qué, dónde, por qué y cómo se revierte → concede → estado Completa.
- Deniega → nada cambia, el repo sigue sin la capa de hooks (Solo MCP o Sin protección, según la allowlist), y no se le vuelve a preguntar.
- Concede en `gitRaptor` → `otro-repo` sigue sin hooks.

**Cómo se verifica**: instalación concedida, denegada y sin respuesta; tras denegar, ninguna petición nueva; repo vecino intacto.

**Referencias**: BRD BR-12; Q22 de motor-local; BR-AUTH-002 (motor-local) (Q11, Q14, Q15); Q-GRD-3.

---

### BR-AUTH-003: A quién se aplican las reglas

**Descripción**: El motor solo dice "agente X" o "sin atribuir"; nunca "humano" (Q34, Q35 de motor-local). Un agente sin registrar es indistinguible del humano.

> **Decisión** (Q-GRD-1, Rene Bonilla, 2026-10-03): las reglas se aplican a **toda** operación gobernada, sea cual sea el actor (fail-safe). El humano tiene una **excepción consciente** para saltarse una regla en una operación concreta; la excepción queda registrada (BR-CONS-004). Una acción que el humano confirma de forma explícita en una superficie de GitRaptor (p. ej. aprobar un merge a la rama base en el Cockpit) cuenta como esa excepción.
>
> **Decisión** (Q-GRD-24, Rene Bonilla, 2026-10-04; D10; refina Q-GRD-1): **toda** excepción consciente, incluida la aprobación explícita en el Cockpit u otra superficie de GitRaptor, pasa por el mismo **anuncio, ventana para cancelar y auditoría** de Q-GRD-19 (BR-AUTH-001). Cancelada dentro de la ventana, la operación no se ejecuta y la cancelación queda registrada.
>
> **Decisión** (Q-GRD-2, Rene Bonilla, 2026-10-03): en el MVP todos los agentes comparten la misma configuración; permisos por agente en una fase posterior.

**Aplicabilidad**: Cada evaluación de una operación gobernada.

**Criticidad**: Alta

**Regla de autorización**:
```
Actor: agente X (detectado o registrado) | sin atribuir
Se evalúa: siempre, con la misma configuración (BR-CALC-001)
Salvo: excepción consciente del humano, para una operación concreta, registrada
```

**Ejemplos**:
- Claude Code (detectado) hace force-push con force-push denegado → denegado.
- Un agente sin registrar ("sin atribuir") hace force-push → denegado: no se distingue del humano.
- El desarrollador necesita hacer ese force-push → usa la excepción consciente → se anuncia, deja pasar la ventana sin cancelar → se ejecuta y queda registrado (Q-GRD-24).
- El desarrollador aprueba en el Cockpit un merge a la rama base → la misma ventana; si la cancela, el merge no se ejecuta (Q-GRD-24).

**Cómo se verifica**: la misma operación como "agente X" y como "sin atribuir" recibe la misma decisión; la excepción consciente solo funciona fuera del canal del agente; toda excepción, también la aprobada en el Cockpit, se anuncia, se puede cancelar y queda auditada.

**Referencias**: Q34, Q35 de motor-local; Q-GRD-1, Q-GRD-2, Q-GRD-19, Q-GRD-24; riesgo R-GRD-2.

---

### BR-AUTH-004: La configuración de Guardrails está protegida frente a los agentes

**Descripción**: Un agente podría relajar las reglas editando la configuración en el working tree o en su máquina.

> **Decisión** (Q-GRD-17, Rene Bonilla, 2026-10-04): una edición sin commitear de la configuración del equipo nunca cuenta; rige la última versión commiteada en el worktree de la operación. Junto con Q-GRD-7, que impide a los agentes commitear cambios en la configuración, un agente no puede relajar la configuración del equipo. La configuración personal, que no se versiona, sigue siendo el riesgo R-GRD-4.

> **Decisión** (Q-GRD-20, Rene Bonilla, 2026-10-04; refina Q-GRD-17): un agente podía relajar sus reglas con un commit fabricado en su worktree, sin pasar por la protección. Por eso las relajaciones salen **solo** de la configuración del equipo en la rama principal, y la versión commiteada en el worktree de la operación solo endurece. Una relajación que llega a la rama principal espera además la confirmación del humano (Q-GRD-21).

> **Decisión** (Q-GRD-7, Rene Bonilla, 2026-10-03): las rutas de la configuración de Guardrails (los tres niveles) son **rutas prohibidas para los agentes por defecto**. Los cambios a la configuración del equipo entran por **commit revisado** (S-GRD-4). Ninguna herramienta MCP edita la configuración ni decide en la cola.

**Aplicabilidad**: Siempre, en todo repo con Guardrails.

**Criticidad**: Alta

**Regla de autorización**:
```
Actor: agente
No puede: modificar ninguna ruta de la configuración de Guardrails, ni por commit ni con el comando
Actor: desarrollador
Puede: editarla a mano o con el comando (relajar requiere BR-AUTH-001)
```

**Ejemplos**:
- Claude Code hace commit de un cambio en la configuración del equipo que permite force-push → denegado (ruta prohibida).
- El desarrollador edita la configuración del equipo y la commitea en su rama → entra por revisión.
- Un agente deja su worktree en un commit, creado sin pasar por la protección, cuya configuración del equipo permite force-push → el force-push sigue denegado por la configuración de la rama principal (Q-GRD-20).

**Cómo se verifica**: commit de un agente sobre la configuración (denegado); worktree en un commit con una configuración más laxa que la de la rama principal (no relaja nada); ausencia de herramientas MCP que editen la configuración o decidan en la cola.

**Referencias**: Q27 de motor-local; NFR-02; Q-GRD-7, Q-GRD-17, Q-GRD-20, Q-GRD-21, S-GRD-4 (confirmado por Q-GRD-7); riesgo R-GRD-4.

---

## 6. Reglas de Consistencia de Datos

### BR-CONS-001: Un nivel personal puede endurecer una regla del equipo, nunca relajarla

**Descripción**: Para los permisos y las políticas, un nivel personal (perfil o configuración local personal) **puede endurecer cualquier regla del equipo y nunca relajarla**. Un endurecimiento del perfil prevalece también sobre un "permitir" del equipo.

> **Decisión** (Q-GRD-14, Rene Bonilla, 2026-10-03; confirma S-GRD-1): **refina Q23 de motor-local**. Q23 dejaba que el equipo ganara al perfil, con la única excepción de que un nivel personal no relaja una prohibición del equipo. Q-GRD-14 amplía esa excepción: cualquier nivel personal endurece y ninguno relaja. Motivo: cada persona puede ser más estricta con sus agentes en su máquina sin afectar al equipo.

> **Decisión** (Q-GRD-20, Rene Bonilla, 2026-10-04): la "configuración del equipo" de esta regla es la de la rama principal (BR-VAL-001). La versión commiteada en el worktree de la operación se combina con ella igual que un nivel personal: endurece y nunca relaja.

**Aplicabilidad**: Permisos por operación, ramas protegidas, límite de diff, formato de commit, rutas prohibidas y plazo de la cola (BR-VAL-001). No aplica a la rama base ni al umbral de inactividad.

**Criticidad**: Alta

**Regla de consistencia**:
```
personal = el del nivel personal más específico que lo defina (local > perfil)
efectivo = el más restrictivo entre (equipo, personal)
           para listas (ramas protegidas, rutas prohibidas): unión de equipo y personales
           para el límite de diff y el plazo de la cola: el menor
           para el formato de commit: el del equipo si lo define; si no, el personal (S-GRD-2)
Constraint: efectivo nunca es menos restrictivo que el del equipo
```

**Comportamiento en conflicto**:
- **Si un nivel personal intenta relajar**: no se tiene en cuenta en esa parte; la regla del equipo se mantiene.
- **Mensaje al usuario** (al editar con el comando): "la configuración del equipo deniega force-push; un nivel personal no puede permitirlo".

**Ejemplos**:
| Equipo | Perfil | Local personal | Efectivo | Por qué |
|--------|--------|----------------|----------|---------|
| force-push: denegar | — | permitir | denegar | Un nivel personal no relaja (Q23) |
| force-push: permitir | denegar | — | denegar | El perfil endurece sobre el equipo (Q-GRD-14) |
| force-push: permitir | denegar | permitir | permitir | Entre personales gana el local; no baja del equipo |
| rebase: pedir confirmación | — | permitir | pedir confirmación | Un nivel personal no relaja (Q23) |
| límite de diff 400 | — | 200 | 200 | El local endurece (Q-GRD-14) |
| límite de diff 400 | — | 1.000 | 400 | Un nivel personal no relaja (Q23) |
| ramas protegidas: `main` | `release` | — | `main` y `release` | El perfil endurece sobre el equipo (Q-GRD-14) |

**Cómo se verifica**: la tabla de ejemplos como esquema de escenarios; el comando rechaza relajar con el mensaje explicado.

**Referencias**: Q23, Q24 de motor-local; Q-GRD-14, Q-GRD-20; S-GRD-1 (confirmado), S-GRD-2.

---

### BR-CONS-002: La misma operación recibe la misma decisión en las dos capas

**Descripción**: La capa MCP y la capa de hooks aplican la **misma decisión** para la misma operación en el mismo repo, con la misma configuración y el mismo actor. Guardrails define la decisión; las herramientas MCP (F-001-05) y los hooks solo la aplican.

**Aplicabilidad**: Siempre que ambas capas cubren la operación.

**Criticidad**: Alta

**Regla de consistencia**:
```
Constraint: decisión(operación, repo, configuración, actor) es independiente de la capa
```

**Comportamiento en conflicto**: no debe darse. Si una operación no se puede evaluar en una capa (BR-EDGE-003), esa limitación se declara; no se sustituye por otra decisión.

**Ejemplos**:
- Force-push a `main` por la herramienta MCP → denegado. El mismo force-push con Git crudo y hooks instalados → denegado con el mismo motivo.

**Cómo se verifica**: cada escenario de BR-VAL-002 y BR-VAL-003 se ejecuta por las dos capas con el mismo resultado y motivo.

**Referencias**: BRD BR-12; dependencia con F-001-05.

---

### BR-CONS-003: La rama base es un valor de la configuración del equipo que el Motor local lee

**Descripción**: Guardrails define la **rama base** como valor de la configuración del equipo, el único nivel que la admite, con `main` por defecto (Q24 de motor-local). El Motor local la lee para calcular ahead/behind (BR-CONS-006 (motor-local)). Un valor de rama base en el perfil o en la configuración local personal no se tiene en cuenta y el comando lo rechaza (BR-VAL-001). Con esta regla y el ADR de formato (P8 (motor-local)), US-GRP-016 deja de estar bloqueada (Q36 de motor-local).

> **Decisión** (Q-GRD-18, Rene Bonilla, 2026-10-04): la rama base es una **excepción a Q-GRD-17**. Se lee de la configuración del equipo **commiteada en la rama principal del repo**: la que el remoto marca como principal, o `main` si no hay ninguna. Así hay un solo valor por repo, sin circularidad y coherente con BR-CONS-006 (motor-local) y Q24 de motor-local. Ni la versión commiteada en el worktree de la operación ni las ediciones sin commitear la cambian. De la rama principal se usa solo lo que el repo ya conoce: Guardrails no consulta el remoto por su cuenta (coherente con Q12 de motor-local).

> **Decisión** (Q-GRD-20, Rene Bonilla, 2026-10-04; refina Q-GRD-18): de la rama principal se usa la **copia que el repo ya conoce del remoto**, sin consultarlo; si no hay remoto, la rama local; si tampoco existe, la rama base es `main`. Nunca el archivo en disco de ningún worktree, tampoco el del worktree principal.
>
> **Decisión** (Q-GRD-21 y Q-GRD-22, Rene Bonilla, 2026-10-04): hay **un solo valor de rama base por repo y máquina: la confirmada por el humano**. Guardrails la protege y el motor calcula contra ella el ahead/behind. La confirmación inicial la hace siempre el humano, **al instalar la protección o con una confirmación explícita**, nunca al añadir el repo a la observación (Q-GRD-23); no hay vía automática. Un cambio de rama base que llega a la rama principal **no se aplica hasta que el humano lo confirma** en esa máquina (acción reservada, BR-AUTH-001): mientras tanto Guardrails protege la confirmada y la nueva, el motor sigue con la confirmada y muestra la nueva como "pendiente de confirmar", y GitRaptor avisa. Mientras no hay confirmación inicial, Guardrails protege `main`, la rama principal y la rama base leída, y el motor calcula contra la rama base leída y la marca como "no confirmada".

> **Entrega por historias**: la rama base leída de la configuración del equipo entra en la unión protegida a partir de US-GRD-014 y TS-GRD-001. Antes de eso, US-GRD-001 protege {`main`, rama principal}.

**Aplicabilidad**: Al editar la configuración y cuando el motor lee la rama base.

**Criticidad**: Media

**Regla de consistencia**:
```
rama principal = la que el remoto marca como principal, ELSE main
copia de la rama principal = la que el repo ya conoce del remoto, ELSE la rama local, ELSE ninguna
rama base leída = la de la configuración del equipo commiteada en esa copia, si la define
                  ELSE main
rama base confirmada = la última que el humano confirmó en esta máquina (Q-GRD-21)

IF no hay rama base confirmada
THEN Guardrails protege {main, rama principal, rama base leída}
     el motor calcula contra la rama base leída, marcada como "no confirmada"
ELSE IF rama base leída = rama base confirmada
THEN Guardrails y el motor usan la confirmada
ELSE Guardrails protege {confirmada, leída}; el motor calcula contra la confirmada
     la leída queda "pendiente de confirmar", con aviso, hasta que el humano la confirma
Constraint: perfil y configuración local personal no intervienen
Constraint: la versión de la configuración en cada worktree no interviene (Q-GRD-18, Q-GRD-20)
Constraint: la rama base que usa el motor es siempre la misma que usa Guardrails como rama base
```

**Ejemplos**:
- La configuración del equipo fija `develop` y el humano la confirmó → el motor calcula ahead/behind contra `develop` y Guardrails protege `develop`.
- El perfil fija `release` y el equipo no define nada → `main`.
- Dos worktrees en commits distintos: en `feat-a` la configuración del equipo dice `develop` y en `feat-b` dice `release`; en la rama principal `main` dice `develop` → la rama base es `develop` para los dos worktrees, para Guardrails y para el motor (Q-GRD-18).
- Un worktree commitea en su rama un cambio de la rama base a `release` → la rama base sigue siendo la confirmada hasta que ese cambio llegue a la rama principal y el humano lo confirme.
- El remoto marca `trunk` como rama principal y la configuración commiteada en `trunk` no define rama base → `main` (valor por defecto del producto).
- Llega a la copia conocida de la rama principal un cambio de `main` a `develop` → hasta la confirmación, Guardrails deniega borrar `main` y borrar `develop`, el motor sigue calculando contra `main` y muestra `develop` como pendiente de confirmar; tras la confirmación, los dos usan solo `develop` (Q-GRD-21).
- El desarrollador commitea en el worktree principal un cambio de rama base que aún no está en la copia conocida del remoto → no cuenta: la rama base no cambia ni queda pendiente (Q-GRD-20).
- Repo observado sin confirmación inicial, con `develop` en la configuración del equipo → Guardrails deniega borrar `main` y `develop`; el motor calcula contra `develop`, marcado como "no confirmado" (Q-GRD-21).
- Se pierde el perfil de GitRaptor, o el desarrollador adopta una protección huérfana → adoptar **no** confirma la rama base ni la configuración del equipo (Q-GRD-23 solo confirma al instalar o de forma explícita): la rama base queda "no confirmada", con la unión protegida, hasta que el desarrollador la confirma (Q-GRD-21, Q-GRD-23).
- En una máquina nueva, el repo clonado trae `develop` en la configuración del equipo; el desarrollador lo añade y lo protege, y el motor calcula contra `develop` marcado como "no confirmado" (BR-EDGE-007 (motor-local)). Instalar no confirma una configuración del equipo existente (Q-GRD-23): `develop` queda confirmada cuando el desarrollador la confirma después de forma explícita (US-GRD-014).
- El desarrollador fija con el comando la rama base `release`, que no existe en el repo → el comando avisa y pide confirmación; si confirma, se guarda y el motor indica que no puede calcular ahead/behind (Q-GRD-16, Q42 de motor-local). Ese valor solo aplica cuando el cambio está commiteado en la rama principal del repo (Q-GRD-18) y el humano confirma el cambio de rama base en su máquina (Q-GRD-21).

**Cómo se verifica**: los escenarios de US-GRP-016 (motor-local) pasan con la configuración que define esta feature; con un cambio pendiente, Guardrails y el motor obtienen la misma rama base confirmada (prueba de integración de US-GRD-014 y US-GRP-016).

**Referencias**: BRD BR-11; Q5, Q12, Q24, Q36 de motor-local; Q-GRD-18, Q-GRD-20, Q-GRD-21, Q-GRD-22; BR-AUTH-001; Q-GRD-23; BR-CONS-006 (motor-local) y BR-CONS-007 (motor-local); Q42 de motor-local y Q-GRD-16 (rama base que no existe al fijarla con el comando).

---

### BR-CONS-004: Registro de decisiones

**Descripción**: Guardrails anota cada operación **denegada**, cada **petición** de confirmación con su resultado y cada **excepción consciente**. Cada entrada lleva: cuándo, repo, worktree, rama, actor ("agente X" o "sin atribuir"), operación, decisión, regla y nivel que la causaron, y capa (MCP o hooks). Las operaciones permitidas sin regla de por medio no se anotan una a una. Es la fuente del KPI "acciones peligrosas bloqueadas" (BRD § 9).

> **Decisión** (Q-GRD-10, Rene Bonilla, 2026-10-03; confirma S-GRD-5): el registro vive en el **perfil de GitRaptor**, separado por repo, nunca en el repo (coherente con Q21 de motor-local).

> **Decisión** (Q-GRD-27, Rene Bonilla, 2026-10-04; revisión de arquitectura de Guardrails): el KPI "acciones peligrosas bloqueadas" cuenta las operaciones **denegadas, rechazadas y caducadas verificadas por GitRaptor**. Las entradas anotadas en **modo degradado**, cuando GitRaptor no pudo verificarlas, se muestran **aparte** y **no entran en el KPI por defecto**; el desarrollador puede incluirlas de forma explícita.

**Aplicabilidad**: Cada decisión de denegar o pedir confirmación, y cada excepción consciente.

**Criticidad**: Media

**Regla de consistencia**:
```
Constraint: toda denegación, petición (con su estado final) y excepción consciente tiene una entrada
Constraint: el registro no se escribe en el repo ni sale de la máquina (NFR-03)
KPI acciones peligrosas bloqueadas = denegadas + rechazadas + caducadas, solo las verificadas
Constraint: las entradas anotadas en modo degradado se muestran aparte y solo cuentan si el desarrollador las incluye
```

**Ejemplos**:
- Claude Code intenta force-push a `main` → entrada: denegado, regla "rama protegida `main`", nivel equipo, capa MCP.
- Petición de borrar worktree que caduca → entrada con estado final "caducada".
- Contar las acciones bloqueadas de la semana en `gitRaptor` → suma de denegadas, rechazadas y caducadas verificadas.
- Esta semana `gitRaptor` tiene 3 denegaciones verificadas y 2 anotadas en modo degradado → el KPI es 3; las 2 aparecen aparte; si el desarrollador las incluye, 5.

**Cómo se verifica**: tras cada escenario de denegación, petición y excepción, existe su entrada con todos los campos; el repo no contiene el registro; el recuento por defecto excluye las entradas anotadas en modo degradado y las muestra aparte.

**Referencias**: BRD § 9, BR-24 (exportar: Fase 3); Q21 de motor-local; Q-GRD-10; S-GRD-5 (confirmado por Q-GRD-10); Q-GRD-27 (KPI verificado).

---

### BR-CONS-005: Instalar y desinstalar la protección de hooks es recuperable (NFR-01)

**Descripción**: La instalación de la protección de hooks:
- **Conserva** los hooks previos del usuario: siguen funcionando igual (BR-EDGE-002).
- Se hace **solo dentro del repo**: nunca en la configuración global de Git, en plantillas del usuario ni en otros repos (Q17 de motor-local).
- Se puede **desinstalar** dejando las rutas operativas del repo **exactamente** como estaban antes de instalar.
- Queda registrada: qué, dónde, cuándo y quién la autorizó.

> **Supuesto confirmado** (S-GRD-6, Rene Bonilla, 2026-10-04): instalarla en un repo cubre todos sus worktrees, actuales y futuros. La viabilidad técnica sigue siendo comprobación del Arquitecto.

**Aplicabilidad**: Al instalar, actualizar o desinstalar la protección de hooks.

**Criticidad**: Alta

**Regla de consistencia**:
```
Constraint: hooks previos del usuario: mismo contenido y mismo efecto tras instalar
Constraint: estado tras desinstalar = estado antes de instalar
Constraint: nada cambia fuera del repo
```

**Comportamiento en conflicto**:
- **Si no se puede instalar sin alterar un hook previo**: no se instala nada, se informa y el repo sigue sin la capa de hooks (Solo MCP o Sin protección, según la allowlist) (BR-EDGE-002).

**Ejemplos**:
- Repo con un hook propio que pasa el linter → instalar → el linter sigue corriendo y Guardrails también evalúa → desinstalar → el hook propio queda idéntico.
- Instalación interrumpida a mitad (p. ej. se cierra el proceso) → el repo queda como antes o con la instalación completa, nunca a medias.
- **Instalación huérfana**: se perdió el perfil de GitRaptor y el repo sigue con la protección instalada, sin registro que la respalde → el desarrollador la retira y el repo queda como estaba antes de instalarla, o la adopta y el repo vuelve a "Solo hooks". Retirarla relaja: pasa por el anuncio, la ventana para cancelar y la auditoría (Q-GRD-19). Adoptarla no confirma la rama base ni la configuración del equipo (Q-GRD-23). **Detecta US-GRD-004; adopta o retira US-GRD-003.**

**Cómo se verifica**: comparación antes/después de las rutas operativas del repo tras instalar y desinstalar; configuración global de Git y repos vecinos sin cambios; prueba de interrupción (NFR-12).

**Referencias**: NFR-01, NFR-07, NFR-12; Q17, Q22 de motor-local; S-GRD-6.

---

### BR-CONS-006: El comando de edición no pisa cambios del usuario y escribe de forma recuperable (NFR-01)

**Descripción**: El comando para editar la configuración (Q27 de motor-local) escribe en el working tree (configuración del equipo y configuración local personal) y en el perfil. Esas escrituras cumplen NFR-01:
- **Nunca pisa cambios hechos a mano** que el usuario no haya commiteado en la configuración. Si el archivo cambió desde la última vez que el comando lo leyó, o tiene cambios sin commitear que el comando no hizo, el comando no escribe: avisa y deja que el desarrollador decida.
- **La escritura es atómica**: o queda el cambio completo o queda la configuración anterior, nunca a medias. Una escritura interrumpida (proceso cerrado, máquina apagada) no deja la configuración corrupta ni perdida.
- **Es recuperable**: la configuración anterior se puede recuperar después del cambio.
- **No hace commit** (S-GRD-3): un cambio en la configuración del equipo queda en el working tree para que el desarrollador lo revise y lo commitee. Ese cambio **no se aplica hasta que se commitea** en el worktree de la operación (Q-GRD-17), y allí solo cuenta si endurece. Si relaja, solo se aplica cuando llega a la rama principal y el humano lo confirma en su máquina (Q-GRD-20, Q-GRD-21). Los cambios del comando en los niveles personales se aplican al escribirse.

**Aplicabilidad**: Toda escritura del comando, en cualquiera de los tres niveles.

**Criticidad**: Alta

**Regla de consistencia**:
```
IF la configuración tiene cambios que el comando no hizo
THEN el comando no escribe; avisa y el desarrollador decide
Constraint: tras cualquier escritura, interrumpida o no: configuración anterior completa OR nueva completa
Constraint: la configuración anterior a cada cambio se puede recuperar
```

**Comportamiento en conflicto**:
- **Si hay cambios a mano**: no se escribe nada.
- **Mensaje al usuario**: "la configuración del equipo tiene cambios sin commitear que no hizo este comando; revísalos antes de continuar".

**Ejemplos**:
- El desarrollador añadió a mano una ruta prohibida y no la commiteó; después usa el comando para cambiar el permiso de rebase → el comando avisa y no escribe; la ruta añadida sigue ahí.
- Se cierra el proceso a mitad de una escritura → la configuración es la anterior o la nueva, completa.
- El desarrollador quiere volver al valor anterior a un cambio del comando → lo recupera.

**Cómo se verifica**: edición con cambios a mano sin commitear (no se pierden); prueba de interrupción a mitad de escritura (NFR-12); recuperación del valor anterior; el comando no crea commits.

**Referencias**: NFR-01, NFR-12; Q27 de motor-local; S-GRD-3; Q-GRD-17, Q-GRD-20, Q-GRD-21; BR-AUTH-001.

---

## 7. Reglas de Tiempo y Expiración

### BR-TIME-001: Plazo de respuesta de una petición de confirmación

**Descripción**: Una petición pendiente espera la decisión del humano hasta un plazo. Al cumplirse sin decisión, pasa a **Caducada** y la operación no se ejecuta (fail-safe).

> **Decisión** (Q-GRD-6, Rene Bonilla, 2026-10-03): en la capa de hooks (Git crudo) la operación **espera** la decisión hasta el plazo, en lugar de rechazarse para reintentarla. Plazo por defecto: **5 minutos**. Los niveles personales solo pueden acortarlo (BR-CONS-001).

**Aplicabilidad**: BR-WF-001.

**Criticidad**: Media

**Regla temporal**:
```
Entidad: petición de confirmación
Plazo: el efectivo según BR-CONS-001; por defecto 5 minutos desde que se pidió
Acción al vencer: Pendiente → Caducada; la operación no se ejecuta
```

**Ejemplo**:
- Pedida: 10:00:00. Plazo: 5 minutos. Sin decisión a las 10:05:00 → Caducada. Una aprobación a las 10:05:30 ya no ejecuta nada.

**Comportamiento al expirar**: el agente recibe "caducada sin respuesta"; si vuelve a intentarlo, se crea una petición nueva.

**Cómo se verifica**: petición sin respuesta que caduca en el plazo; aprobación tardía sin efecto.

**Referencias**: BRD BR-13; Q-GRD-6.

---

### BR-TIME-002: Conservación del registro de decisiones

**Descripción**: Las entradas del registro de decisiones se conservan durante un tiempo y luego se descartan.

> **Decisión** (Q-GRD-10, Rene Bonilla, 2026-10-03): **90 días** por repo. Retirar un repo de la observación no borra su registro (coherente con Q25 de motor-local).

**Aplicabilidad**: BR-CONS-004.

**Criticidad**: Baja

**Regla temporal**:
```
Entidad: entrada del registro de decisiones
TTL: 90 días desde la decisión
Acción al expirar: se descarta
```

**Ejemplo**: una denegación del 2026-10-03 deja de estar en el registro el 2027-01-01.

**Cómo se verifica**: una entrada de más de 90 días ya no aparece; una de 89 días sí.

**Referencias**: BRD § 9, BR-24; Q25 de motor-local; Q-GRD-10.

---

## 8. Reglas Excepcionales (Edge Cases)

### BR-EDGE-001: Repo sin configuración de Guardrails

**Descripción**: Un repo sin configuración del equipo ni niveles personales no queda sin protección.

> **Decisión** (Q-GRD-5, Rene Bonilla, 2026-10-03): aplica un **conjunto mínimo seguro**: denegar force-push y denegar el borrado de la rama base. Es visible para el desarrollador (BR-WF-002) y el equipo lo puede desactivar en su configuración.

> **Rama base protegida** (Q-GRD-18, 2026-10-04): la rama base que protege el mínimo seguro es la de BR-CONS-003, leída de la configuración del equipo commiteada en la rama principal del repo. Es la misma en todos los worktrees, sea cual sea su commit. Por Q-GRD-21 es la rama base **confirmada**; con un cambio pendiente o sin confirmación inicial, el mínimo protege también las demás ramas que indica BR-CONS-003.

> **Decisión** (Q-GRD-20 y Q-GRD-21, Rene Bonilla, 2026-10-04; refinan Q-GRD-5): el conjunto mínimo **solo lo desactiva la configuración del equipo commiteada en la rama principal**, nunca la versión de un worktree. Además, desactivarlo es una relajación: **no se aplica hasta que el humano la confirma en cada máquina** (BR-AUTH-001). Mientras no la confirma, el mínimo sigue aplicando y GitRaptor avisa de la relajación pendiente.

> **Aplicación de Q-GRD-21 y Q-GRD-23** (Rene Bonilla, 2026-10-04): mientras no hay confirmación inicial, la configuración del equipo solo endurece y el mínimo sigue aplicando aunque esa configuración lo desactive (BR-VAL-001).

**Frecuencia esperada**: alta al empezar (todo repo nuevo).

**Criticidad**: Alta

**Ejemplos**:
- Repo sin configuración: un agente hace force-push → denegado, motivo "conjunto mínimo por defecto".
- El equipo desactiva el conjunto mínimo en la configuración de la rama principal y el desarrollador confirma ese cambio en su máquina → el force-push se rige por la configuración del equipo.
- El mismo cambio llega a la rama principal y nadie lo confirma en esa máquina → el force-push sigue denegado por el mínimo, con aviso de relajación pendiente.
- Un worktree está en un commit cuya configuración del equipo desactiva el mínimo → no cuenta: el mínimo sigue aplicando (Q-GRD-20).
- El desarrollador protege un repo cuya configuración del equipo ya desactiva el mínimo y aún no la confirma → el force-push sigue denegado por el mínimo; tras confirmarla de forma explícita, se rige por la configuración del equipo (Q-GRD-21, Q-GRD-23).

**Cómo se verifica**: repo sin configuración con las dos operaciones del conjunto mínimo (denegadas) y una operación fuera de él (permitida); mínimo desactivado en la rama principal antes y después de la confirmación humana; mínimo desactivado solo en un worktree.

**Referencias**: Q-GRD-5, Q-GRD-18, Q-GRD-20, Q-GRD-21; BR-AUTH-001; riesgo R-GRD-2.

---

### BR-EDGE-002: Repo con hooks propios o con otro gestor de hooks

**Descripción**: Guardrails nunca reemplaza hooks que ya existan.

> **Decisión** (Q-GRD-4, Rene Bonilla, 2026-10-03): Guardrails detecta los hooks previos, informa al desarrollador y los **encadena** solo con su permiso (BR-AUTH-002). Si no se pueden encadenar sin alterarlos, no instala nada, avisa y el repo sigue sin la capa de hooks (Solo MCP o Sin protección, según la allowlist) (BR-WF-002).

**Frecuencia esperada**: media (repos con linters, gestores de hooks del equipo).

**Criticidad**: Alta

**Ejemplos**:
- Repo con un gestor de hooks del equipo → Guardrails informa y pide permiso para encadenarse → concedido → Completa, y el gestor sigue funcionando.
- Encadenar no es posible → el repo sigue sin la capa de hooks (Solo MCP o Sin protección, según la allowlist), con el motivo explicado.

**Cómo se verifica**: repo con hooks previos antes y después de instalar: siguen haciendo lo mismo; caso no encadenable deja el repo intacto.

**Referencias**: NFR-01, NFR-07; Q-GRD-4; riesgo R-GRD-5.

---

### BR-EDGE-003: Límites de la capa de hooks

**Descripción**: La capa de hooks no cubre todo. Hay operaciones destructivas que Git no deja interceptar (p. ej. `reset --hard` sobre el working tree con Git crudo) y Git permite saltarse los hooks a propósito (p. ej. con `--no-verify`). Guardrails **declara** qué operaciones del catálogo no puede impedir en Git crudo y lo muestra en el estado de protección. La mitigación es la Time Machine (F-001-03); integrar con los permisos nativos del agente queda fuera del MVP.

> **Decisión** (Q-GRD-8, Rene Bonilla, 2026-10-03): se acepta como límite del MVP y se registra como riesgo.

**Frecuencia esperada**: baja, pero con impacto alto.

**Criticidad**: Alta

**Ejemplos**:
- Un agente ejecuta `reset --hard` con Git crudo → la capa de hooks no lo impide; la Time Machine permite recuperar.
- La misma operación por la herramienta MCP → denegada (BR-CONS-002).

**Cómo se verifica**: la lista de operaciones no cubiertas está publicada y coincide con lo que se observa en pruebas con Git crudo.

**Referencias**: BRD BR-12, § 10 (riesgo "los agentes usan Git crudo"); Q-GRD-8; riesgo R-GRD-1.

---

### BR-EDGE-004: Configuración ilegible o inválida

**Descripción**: Si un nivel de la configuración no se puede leer o tiene valores inválidos, Guardrails **avisa** y nunca cae en "todo permitido".

> **Decisión** (Q-GRD-12, Rene Bonilla, 2026-10-03): para permisos y políticas se aplica el conjunto mínimo por defecto (BR-EDGE-001) más lo legible de los demás niveles. Si el nivel ilegible es personal, solo se pierden sus endurecimientos.
>
> **Aplicación de Q-GRD-12 a Q-GRD-20** (Rene Bonilla, 2026-10-04): **cualquier** versión ilegible de la configuración del equipo fuerza el conjunto mínimo y aplica lo legible: la de la rama principal y **también la versión commiteada en el worktree de la operación**, aunque esta solo pueda endurecer.

> **Decisión** (Q-GRD-26, Rene Bonilla, 2026-10-04; D12): una **clave desconocida** dentro de los permisos o de las políticas (por ejemplo, una errata) deja ese nivel como **parcial**: se aplica lo legible, se **fuerza el conjunto mínimo aunque el equipo lo hubiera desactivado** y se avisa. **Una errata nunca relaja.**

**Dependencia abierta con el Motor local**: BR-CONS-007 (motor-local) tiene como supuesto ignorar un nivel ilegible para los valores del motor (rama base, umbral de inactividad). Para esos valores no hay riesgo de relajar nada, pero la misma configuración se trata distinto según quién la lea. Queda para el Arquitecto, o para una revisión de motor-local, alinear los dos comportamientos. Este requerimiento no cambia motor-local.

**Frecuencia esperada**: baja (un nivel personal mal editado, o una configuración del equipo commiteada con errores o con marcas de conflicto). Por Q-GRD-17, una edición o un conflicto sin commitear en la configuración del equipo no la vuelven ilegible: rige la última versión commiteada.

**Criticidad**: Alta

**Ejemplos**:
- Se commitea la configuración del equipo con marcas de conflicto de un merge → aviso; force-push sigue denegado por el conjunto mínimo.
- Un merge deja la configuración del equipo en conflicto en el working tree, sin commitear → no hay aviso de ilegible: rige la última versión commiteada (Q-GRD-17).
- La configuración del equipo, con el conjunto mínimo desactivado y confirmado, añade una política con un nombre mal escrito → el nivel queda parcial: aviso, el resto de lo legible aplica y force-push vuelve a estar denegado por el mínimo hasta que se corrija la errata (Q-GRD-26).

**Cómo se verifica**: cada nivel ilegible por separado, incluida la versión commiteada en el worktree de la operación: hay aviso y ninguna operación del conjunto mínimo pasa; un nivel con una clave desconocida en permisos o políticas queda parcial, avisa, aplica lo legible y fuerza el mínimo aunque estuviera desactivado.

**Referencias**: contexto § 6 (fail-safe); BR-CONS-007 (motor-local); Q-GRD-12, Q-GRD-17, Q-GRD-26; riesgo R-GRD-8.

---

### BR-EDGE-005: Operación destructiva permitida sin snapshot previo

**Descripción**: Toda operación destructiva que Guardrails permite o que el humano aprueba debe ir precedida de un snapshot de la Time Machine (NFR-01). Guardrails no toma el snapshot.

> **Decisión** (Q-GRD-11, Rene Bonilla, 2026-10-03): si la Time Machine no puede tomar el snapshot, la operación se **deniega** con el motivo explicado; el humano puede usar una excepción consciente.

**Frecuencia esperada**: muy baja.

**Criticidad**: Alta

**Ejemplos**:
- `reset --hard` permitido por la configuración, snapshot imposible → denegado, motivo "no se pudo guardar un punto de recuperación".

**Cómo se verifica**: simular el fallo del snapshot y comprobar que la operación no se ejecuta.

**Referencias**: NFR-01; dependencia con F-001-03; Q-GRD-11; riesgo R-GRD-9.

---

## Matriz de Priorización

| Regla | Criticidad | Complejidad | Prioridad de Implementación |
|-------|------------|-------------|------------------------------|
| BR-VAL-002 | Alta | Baja | 🔴 P0 |
| BR-VAL-003 | Alta | Media | 🔴 P0 (Q-GRD-9: primero ramas protegidas, force-push, `reset --hard`, rutas prohibidas) |
| BR-CALC-001 | Alta | Baja | 🔴 P0 |
| BR-VAL-001 | Alta | Baja | 🔴 P0 |
| BR-CONS-001 | Alta | Media | 🔴 P0 |
| BR-CONS-002 | Alta | Media | 🔴 P0 |
| BR-AUTH-001 | Alta | Alta | 🔴 P0 |
| BR-AUTH-003 | Alta | Baja | 🔴 P0 |
| BR-AUTH-004 | Alta | Baja | 🔴 P0 |
| BR-AUTH-002 | Alta | Media | 🔴 P0 |
| BR-CONS-005 | Alta | Alta | 🔴 P0 |
| BR-CONS-006 | Alta | Media | 🔴 P0 |
| BR-EDGE-001 | Alta | Baja | 🔴 P0 |
| BR-EDGE-002 | Alta | Alta | 🔴 P0 |
| BR-EDGE-003 | Alta | Baja | 🔴 P0 |
| BR-EDGE-004 | Alta | Media | 🔴 P0 |
| BR-EDGE-005 | Alta | Baja | 🔴 P0 |
| BR-CONS-003 | Media | Baja | 🟡 P1 (desbloquea US-GRP-016) |
| BR-CONS-004 | Media | Baja | 🟡 P1 |
| BR-WF-002 | Media | Media | 🟡 P1 |
| BR-WF-001 | Alta | Media | 🟡 P1 (BR-13 es Should) |
| BR-TIME-001 | Media | Baja | 🟡 P1 (BR-13 es Should) |
| BR-TIME-002 | Baja | Baja | 🟢 P2 |

**Leyenda**:
- 🔴 **P0**: Crítico. Sin esto el feature no funciona.
- 🟡 **P1**: Importante. Necesario para el MVP.
- 🟢 **P2**: Deseable. Puede ir en iteraciones posteriores.

---

## Trazabilidad

### Reglas → User Stories

Las 23 reglas tienen al menos una historia. La tabla vive en el [índice de historias](./user-stories.md#cobertura-de-reglas-regla--historias), sección "Cobertura de reglas".

### Reglas → Criterios de Aceptación

Cada regla debe reflejarse en al menos un escenario Gherkin de su historia. Cada regla indica en "Cómo se verifica" los escenarios mínimos. BR-CONS-002 obliga a que los escenarios de BR-VAL-002 y BR-VAL-003 se ejecuten por las dos capas. BR-CONS-005 se verifica con una comparación antes/después de las rutas operativas del repo y una prueba de interrupción. Las decisiones Q-GRD-1 a Q-GRD-27 ya están incorporadas a las reglas, junto con sus aplicaciones derivadas sin ID propio: Q-GRD-12 aplicada a la versión del worktree de la operación (BR-EDGE-004), la configuración del equipo antes de la confirmación inicial (BR-VAL-001, BR-EDGE-001), la rama base tras perder el perfil o adoptar una huérfana y la unión protegida por historia (BR-CONS-003), y el reparto de las huérfanas entre US-GRD-004 y US-GRD-003 (BR-CONS-005). Los supuestos que quedaban (S-GRD-6, S-GRD-9 y el formato Conventional Commits) los confirmó Rene Bonilla el 2026-10-04; de S-GRD-6 queda pendiente solo la comprobación técnica del Arquitecto.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-03 | PO (AADD) para Rene Bonilla | Versión inicial: 22 reglas (17 críticas). Supuestos S1-S9 y preguntas P1-P13 en el contexto (numeración original). |
| 1.1 | 2026-10-03 | PO (AADD) para Rene Bonilla | Artifact Judge (RESERVAS): convención de IDs (P-GRD-n, S-GRD-n, R-GRD-n; IDs del Motor local calificados con "(motor-local)"); BR-CONS-001 separa lo decidido por Q23 de la ampliación pendiente (P-GRD-14) y marca las filas dependientes; BR-AUTH-001 deja de atribuir a Q27 una frase que no está en la fuente; nueva BR-CONS-006 (el comando no pisa cambios a mano y escribe de forma atómica y recuperable, NFR-01); BR-WF-002 con cuatro estados, todas las transiciones, alcance (P-GRD-15) y alineada con BR-EDGE-001. 23 reglas (18 críticas). |
| 1.2 | 2026-10-03 | PO (AADD) para Rene Bonilla | Decisiones Q-GRD-1 a Q-GRD-16 de Rene Bonilla (aceptan las recomendaciones de P-GRD-1 a P-GRD-16): los bloques de supuesto pasan a "Decisión"; BR-CONS-001 aplica Q-GRD-14 (un nivel personal endurece cualquier regla del equipo y nunca la relaja; refina Q23 de motor-local) y su tabla deja de tener filas dependientes; BR-TIME-001 fija el plazo en 5 minutos y BR-TIME-002 la retención en 90 días; BR-WF-002 aplica el alcance de Q-GRD-15; BR-EDGE-001 a BR-EDGE-005 aplican Q-GRD-5, 4, 8, 12 y 11; BR-EDGE-004 anota la dependencia con BR-CONS-007 (motor-local) para el Arquitecto o una revisión de motor-local. S-GRD-1, S-GRD-4 y S-GRD-5 confirmados por Q-GRD-14, Q-GRD-7 y Q-GRD-10. Sin reglas nuevas: 23 reglas (18 críticas). |
| 1.3 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisión Q-GRD-17 de Rene Bonilla, posterior a la aprobación del requerimiento: la configuración del equipo que rige una operación es la última versión commiteada en el worktree de esa operación; las ediciones sin commitear nunca cuentan. BR-VAL-001 (decisión y ejemplos), BR-AUTH-004 (con Q-GRD-7, un agente no puede relajarla), BR-CONS-006 (el cambio del comando en el nivel de equipo se aplica al commitearlo) y BR-EDGE-004 (un conflicto sin commitear no vuelve ilegible la configuración; ejemplos reformulados). Sin reglas nuevas. |
| 1.4 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisión Q-GRD-18 de Rene Bonilla, posterior a la aprobación del requerimiento: la rama base es una excepción a Q-GRD-17 y se lee de la configuración del equipo commiteada en la rama principal del repo (la que marca el remoto, o `main`). BR-CONS-003 (decisión, regla formal y ejemplos con dos worktrees en commits distintos), BR-EDGE-001 (la rama base protegida por el mínimo seguro), BR-VAL-001 (tabla de valores y excepción en el bloque de Q-GRD-17). Sin reglas nuevas. |
| 1.5 | 2026-10-04 | PO (AADD) para Rene Bonilla | Supuestos confirmados por Rene Bonilla: S-GRD-9 (BR-VAL-002), S-GRD-6 (BR-CONS-005; la viabilidad técnica sigue siendo del Arquitecto) y Conventional Commits como formato mínimo (BR-VAL-003). Sin marcas ASSUMPTION ni [POR VERIFICAR] pendientes de validación de negocio. Sin reglas nuevas. |
| 1.6 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisiones Q-GRD-19 a Q-GRD-22 (D5 a D8 de Rene Bonilla en la revisión de arquitectura y seguridad), posteriores a la aprobación del requerimiento, y KPI verificado. Q-GRD-20 refina Q-GRD-17 y Q-GRD-18: las relajaciones y la rama base salen solo de la configuración del equipo en la copia conocida de la rama principal; el worktree solo endurece (BR-VAL-001 con ejemplos reformulados, BR-AUTH-004, BR-CONS-001, BR-CONS-006). Q-GRD-21: toda relajación que llega a la rama principal, incluidos desactivar el mínimo y cambiar la rama base, espera la confirmación humana en cada máquina; rama base confirmada como valor único para Guardrails y el motor (BR-VAL-001, BR-CONS-003 con regla formal y ejemplos nuevos, BR-EDGE-001). Q-GRD-19 y Q-GRD-22: BR-AUTH-001 añade la acción reservada "confirmar un cambio de la configuración del equipo o de la rama base", el riesgo aceptado del MVP y el factor de autenticación del sistema operativo antes de relajar con el comando y aprobar en la cola. BR-CONS-004: el KPI cuenta solo lo verificado; lo anotado en modo degradado va aparte. Sin reglas nuevas: 23 reglas (18 críticas). |
| 1.7 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisiones Q-GRD-23 a Q-GRD-27 de Rene Bonilla (D9 a D12 y KPI), posteriores a la aprobación del requerimiento. Q-GRD-23: la confirmación inicial de la rama base y de la configuración del equipo se hace al instalar la protección o de forma explícita, nunca al añadir el repo; si trae relajaciones, pasa por el anuncio y la ventana (BR-AUTH-001, BR-CONS-003; se quita "al añadir el repo"). Q-GRD-24: toda excepción consciente, incluida la aprobación en el Cockpit, pasa por anuncio, ventana y auditoría (BR-AUTH-001, BR-AUTH-003). Q-GRD-25: BR-WF-002 añade los diagnósticos "relajación pendiente de confirmar" y "rama base no confirmada o pendiente", sin estados nuevos. Q-GRD-26: una clave desconocida en permisos o políticas deja el nivel parcial y fuerza el mínimo (BR-VAL-001, BR-EDGE-004). Q-GRD-27: la decisión del KPI verificado de la versión 1.6 recibe ID (BR-CONS-004). BR-CONS-005: instalación huérfana (retirar o adoptar), entregada por US-GRD-003. Sin reglas nuevas: 23 reglas (18 críticas). |
| 1.8 | 2026-10-04 | PO (AADD) para Rene Bonilla | Aplicación de decisiones existentes, sin IDs nuevos. BR-EDGE-004: por Q-GRD-12, también la versión ilegible commiteada en el worktree de la operación fuerza el mínimo. BR-CONS-003: tras perder el perfil o adoptar una protección huérfana, la rama base queda "no confirmada" con la unión protegida hasta la confirmación (Q-GRD-21, Q-GRD-23). Sin reglas nuevas. |
| 1.9 | 2026-10-04 | PO (AADD) para Rene Bonilla | Artifact Judge (FAIL). Q-GRD-23 tal cual: instalar no confirma una configuración del equipo existente; el ejemplo de máquina nueva de BR-CONS-003 confirma después de forma explícita. BR-VAL-001 y BR-EDGE-001: sin confirmación inicial la configuración del equipo solo endurece y el mínimo sigue aplicando (aplicación de Q-GRD-21 y Q-GRD-23, con ejemplo). BR-CONS-005: detecta US-GRD-004; adopta o retira US-GRD-003. BR-CONS-003: la rama base leída entra en la unión con US-GRD-014 y TS-GRD-001; antes, US-GRD-001 protege {`main`, rama principal}. Cabeceras y Trazabilidad hasta Q-GRD-27 con las derivadas; se quita "Sin historias todavía". Sin reglas nuevas. |
