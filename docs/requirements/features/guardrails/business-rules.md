---
id: BR-GRD-001
title: "Reglas de Negocio — Guardrails"
type: business-rules
status: draft
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: guardrails
related:
  context:
    - CTX-GRD-001
  stories: []
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
> **Sin historias todavía**: las referencias a historias se añadirán cuando existan.

---

## Contexto

**Feature**: Guardrails (F-001-04)
**Enlace a contexto**: [`context.md`](./context.md) (CTX-GRD-001)
**Última actualización**: 2026-10-03 (versión inicial)

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
| Rama base | **Solo equipo** | `main` | Sin combinación | Motor local lee; Guardrails define (BR-CONS-003) |
| Umbral de inactividad | **Solo perfil y local personal** | 5 minutos | Gana el más específico | Motor local (BR-TIME-001 (motor-local)) |

Un valor nuevo tiene que declarar sus niveles al incorporarse a esta tabla. Los niveles de la rama base y del umbral los fijó el Motor local (Q24 de motor-local) y Guardrails los respeta.

**Regla formal**:
```
IF un nivel define un valor que ese nivel no admite
THEN al leer: el valor no se tiene en cuenta
     al editar con el comando: se rechaza e informa de los niveles admitidos
```

**Ejemplos**:
- El desarrollador intenta fijar la rama base `develop` en su perfil → el comando lo rechaza: "la rama base solo se define en la configuración del equipo".
- El desarrollador intenta fijar un umbral de inactividad de 30 minutos en la configuración del equipo → el comando lo rechaza: "el umbral solo se define en el perfil o en la configuración local personal".
- Alguien edita a mano la configuración del equipo y pone un umbral de inactividad → no se tiene en cuenta.

**Cómo se verifica**: por cada valor de la tabla, escribirlo en un nivel no admitido (a mano y con el comando) y comprobar que no cambia el valor efectivo y que el comando lo rechaza con los niveles admitidos.

**Referencias**: BRD BR-11; Q23, Q24, Q27 de motor-local; BR-CONS-007 (motor-local); Q-GRD-6, Q-GRD-14; S-GRD-2.

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

> ⚠️ **ASSUMPTION** (S-GRD-9): mientras el modo "pedir confirmación" (BRD BR-13, Should) no esté disponible, una operación con "pedir confirmación" se trata como **denegar** (fail-safe) `[POR VERIFICAR]`.

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
| **Formato de commit** | Un commit cuyo mensaje no cumple el formato fijado. ⚠️ **ASSUMPTION**: el MVP ofrece al menos Conventional Commits `[POR VERIFICAR]` | "arreglos varios" con Conventional Commits exigido → denegado, con un ejemplo válido |
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

**Criticidad**: Media

**Ejemplos**:
- Repo recién observado, sin allowlist ni hooks → Sin protección.
- Se añade a la allowlist del MCP y no tiene configuración → Solo MCP, con el conjunto mínimo aplicado (BR-EDGE-001).
- Repo con hooks instalados que no está en la allowlist → Solo hooks: un agente con Git crudo recibe la decisión; las herramientas MCP no operan sobre ese repo.
- Otro gestor de hooks reemplaza los de Guardrails en un repo Completa → Solo MCP, con aviso.
- Se retira el repo de la observación con hooks activos → los hooks siguen aplicando las reglas, con actor "sin atribuir", y se avisa (Q-GRD-15).

**Cómo se verifica**: un repo en cada uno de los cuatro estados; cada transición de la lista; retirar los hooks por fuera de Guardrails y comprobar el cambio de estado y el aviso; un repo sin configuración en Solo MCP aplica el conjunto mínimo.

**Referencias**: BRD BR-12; NFR-02; BR-AUTH-001 (motor-local); BR-EDGE-001; Q-GRD-4, Q-GRD-15; dependencia con F-001-02 y F-001-05.

---

## 5. Permisos y Autorizaciones

### BR-AUTH-001: Acciones reservadas al humano

**Descripción**: Algunas acciones solo las puede hacer el desarrollador, nunca un agente. Como un agente puede usar la terminal, estas acciones exigen una confirmación que un agente no pueda dar desde su canal (MCP o la terminal que usa). Cómo se logra lo decide el Arquitecto (riesgo R-GRD-3 del contexto).

**Aplicabilidad**: Siempre.

**Criticidad**: Alta

**Tabla de permisos**:

| Acción | Desarrollador | Agente (Claude Code u otro) | Condiciones |
|--------|---------------|-----------------------------|-------------|
| Editar la configuración para endurecer | ✅ | ❌ por defecto (BR-AUTH-004) | — |
| Editar la configuración para relajar | ✅ | ❌ | Confirmación que un agente no pueda dar |
| Aprobar o rechazar una petición de la cola | ✅ | ❌ | Ni la suya ni la de otro agente |
| Instalar o desinstalar la protección de hooks | ✅ | ❌ | Con permiso explícito (BR-AUTH-002) |
| Usar una excepción consciente para saltarse una regla (Q-GRD-1) | ✅ | ❌ | Una operación concreta; queda registrada |
| Realizar operaciones gobernadas | ✅ (ver BR-AUTH-003) | ✅ | Sujetas a la decisión (BR-CALC-001) |

**Ejemplos**:
- Claude Code intenta aprobar su petición de rebase por MCP → no existe esa herramienta; por la CLI → rechazado.
- Claude Code ejecuta el comando de edición para cambiar force-push a "permitir" → rechazado.

**Cómo se verifica**: cada acción reservada intentada desde el canal del agente (MCP y terminal del agente) queda rechazada y registrada.

**Referencias**: BRD BR-13 (las acciones de riesgo de un agente quedan para que un humano las apruebe); Q27 de motor-local (el comando de edición es de Guardrails); riesgo R-GRD-3 y decisión Q-GRD-7 de esta feature.

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
- El desarrollador necesita hacer ese force-push → usa la excepción consciente → se ejecuta y queda registrado.

**Cómo se verifica**: la misma operación como "agente X" y como "sin atribuir" recibe la misma decisión; la excepción consciente solo funciona fuera del canal del agente.

**Referencias**: Q34, Q35 de motor-local; Q-GRD-1, Q-GRD-2; riesgo R-GRD-2.

---

### BR-AUTH-004: La configuración de Guardrails está protegida frente a los agentes

**Descripción**: Un agente podría relajar las reglas editando la configuración en el working tree o en su máquina.

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

**Cómo se verifica**: commit de un agente sobre la configuración (denegado); ausencia de herramientas MCP que editen la configuración o decidan en la cola.

**Referencias**: Q27 de motor-local; NFR-02; Q-GRD-7, S-GRD-4 (confirmado por Q-GRD-7); riesgo R-GRD-4.

---

## 6. Reglas de Consistencia de Datos

### BR-CONS-001: Un nivel personal puede endurecer una regla del equipo, nunca relajarla

**Descripción**: Para los permisos y las políticas, un nivel personal (perfil o configuración local personal) **puede endurecer cualquier regla del equipo y nunca relajarla**. Un endurecimiento del perfil prevalece también sobre un "permitir" del equipo.

> **Decisión** (Q-GRD-14, Rene Bonilla, 2026-10-03; confirma S-GRD-1): **refina Q23 de motor-local**. Q23 dejaba que el equipo ganara al perfil, con la única excepción de que un nivel personal no relaja una prohibición del equipo. Q-GRD-14 amplía esa excepción: cualquier nivel personal endurece y ninguno relaja. Motivo: cada persona puede ser más estricta con sus agentes en su máquina sin afectar al equipo.

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

**Referencias**: Q23, Q24 de motor-local; Q-GRD-14; S-GRD-1 (confirmado), S-GRD-2.

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

**Aplicabilidad**: Al editar la configuración y cuando el motor lee la rama base.

**Criticidad**: Media

**Regla de consistencia**:
```
rama base = la de la configuración del equipo, si la define
            ELSE main
Constraint: perfil y configuración local personal no intervienen
```

**Ejemplos**:
- La configuración del equipo fija `develop` → el motor calcula ahead/behind contra `develop`.
- El perfil fija `release` y el equipo no define nada → `main`.
- En una máquina nueva, el repo clonado trae `develop` en la configuración del equipo → aplica desde el primer momento (BR-EDGE-007 (motor-local)).
- El desarrollador fija con el comando la rama base `release`, que no existe en el repo → el comando avisa y pide confirmación; si confirma, se guarda y el motor indica que no puede calcular ahead/behind (Q-GRD-16, Q42 de motor-local).

**Cómo se verifica**: los escenarios de US-GRP-016 (motor-local) pasan con la configuración que define esta feature.

**Referencias**: BRD BR-11; Q5, Q24, Q36 de motor-local; BR-CONS-006 (motor-local) y BR-CONS-007 (motor-local); Q42 de motor-local y Q-GRD-16 (rama base que no existe al fijarla con el comando).

---

### BR-CONS-004: Registro de decisiones

**Descripción**: Guardrails anota cada operación **denegada**, cada **petición** de confirmación con su resultado y cada **excepción consciente**. Cada entrada lleva: cuándo, repo, worktree, rama, actor ("agente X" o "sin atribuir"), operación, decisión, regla y nivel que la causaron, y capa (MCP o hooks). Las operaciones permitidas sin regla de por medio no se anotan una a una. Es la fuente del KPI "acciones peligrosas bloqueadas" (BRD § 9).

> **Decisión** (Q-GRD-10, Rene Bonilla, 2026-10-03; confirma S-GRD-5): el registro vive en el **perfil de GitRaptor**, separado por repo, nunca en el repo (coherente con Q21 de motor-local).

**Aplicabilidad**: Cada decisión de denegar o pedir confirmación, y cada excepción consciente.

**Criticidad**: Media

**Regla de consistencia**:
```
Constraint: toda denegación, petición (con su estado final) y excepción consciente tiene una entrada
Constraint: el registro no se escribe en el repo ni sale de la máquina (NFR-03)
```

**Ejemplos**:
- Claude Code intenta force-push a `main` → entrada: denegado, regla "rama protegida `main`", nivel equipo, capa MCP.
- Petición de borrar worktree que caduca → entrada con estado final "caducada".
- Contar las acciones bloqueadas de la semana en `gitRaptor` → suma de denegadas, rechazadas y caducadas.

**Cómo se verifica**: tras cada escenario de denegación, petición y excepción, existe su entrada con todos los campos; el repo no contiene el registro.

**Referencias**: BRD § 9, BR-24 (exportar: Fase 3); Q21 de motor-local; Q-GRD-10; S-GRD-5 (confirmado por Q-GRD-10).

---

### BR-CONS-005: Instalar y desinstalar la protección de hooks es recuperable (NFR-01)

**Descripción**: La instalación de la protección de hooks:
- **Conserva** los hooks previos del usuario: siguen funcionando igual (BR-EDGE-002).
- Se hace **solo dentro del repo**: nunca en la configuración global de Git, en plantillas del usuario ni en otros repos (Q17 de motor-local).
- Se puede **desinstalar** dejando las rutas operativas del repo **exactamente** como estaban antes de instalar.
- Queda registrada: qué, dónde, cuándo y quién la autorizó.

> ⚠️ **ASSUMPTION** (S-GRD-6): instalarla en un repo cubre todos sus worktrees, actuales y futuros `[POR VERIFICAR]` viabilidad con el Arquitecto.

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

**Cómo se verifica**: comparación antes/después de las rutas operativas del repo tras instalar y desinstalar; configuración global de Git y repos vecinos sin cambios; prueba de interrupción (NFR-12).

**Referencias**: NFR-01, NFR-07, NFR-12; Q17, Q22 de motor-local; S-GRD-6.

---

### BR-CONS-006: El comando de edición no pisa cambios del usuario y escribe de forma recuperable (NFR-01)

**Descripción**: El comando para editar la configuración (Q27 de motor-local) escribe en el working tree (configuración del equipo y configuración local personal) y en el perfil. Esas escrituras cumplen NFR-01:
- **Nunca pisa cambios hechos a mano** que el usuario no haya commiteado en la configuración. Si el archivo cambió desde la última vez que el comando lo leyó, o tiene cambios sin commitear que el comando no hizo, el comando no escribe: avisa y deja que el desarrollador decida.
- **La escritura es atómica**: o queda el cambio completo o queda la configuración anterior, nunca a medias. Una escritura interrumpida (proceso cerrado, máquina apagada) no deja la configuración corrupta ni perdida.
- **Es recuperable**: la configuración anterior se puede recuperar después del cambio.
- **No hace commit** (S-GRD-3): un cambio en la configuración del equipo queda en el working tree para que el desarrollador lo revise y lo commitee.

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

**Referencias**: NFR-01, NFR-12; Q27 de motor-local; S-GRD-3; BR-AUTH-001.

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

**Frecuencia esperada**: alta al empezar (todo repo nuevo).

**Criticidad**: Alta

**Ejemplos**:
- Repo sin configuración: un agente hace force-push → denegado, motivo "conjunto mínimo por defecto".
- El equipo desactiva el conjunto mínimo en su configuración → el force-push se rige por la configuración del equipo.

**Cómo se verifica**: repo sin configuración con las dos operaciones del conjunto mínimo (denegadas) y una operación fuera de él (permitida).

**Referencias**: Q-GRD-5; riesgo R-GRD-2.

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

**Dependencia abierta con el Motor local**: BR-CONS-007 (motor-local) tiene como supuesto ignorar un nivel ilegible para los valores del motor (rama base, umbral de inactividad). Para esos valores no hay riesgo de relajar nada, pero la misma configuración se trata distinto según quién la lea. Queda para el Arquitecto, o para una revisión de motor-local, alinear los dos comportamientos. Este requerimiento no cambia motor-local.

**Frecuencia esperada**: baja (edición a mano con errores, conflicto de merge en la configuración del equipo).

**Criticidad**: Alta

**Ejemplos**:
- La configuración del equipo queda con un conflicto de merge sin resolver → aviso; force-push sigue denegado por el conjunto mínimo.

**Cómo se verifica**: cada nivel ilegible por separado: hay aviso y ninguna operación del conjunto mínimo pasa.

**Referencias**: contexto § 6 (fail-safe); BR-CONS-007 (motor-local); Q-GRD-12; riesgo R-GRD-8.

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

Sin historias todavía. Cada regla deberá tener al menos una historia cuando se generen.

### Reglas → Criterios de Aceptación

Cada regla debe reflejarse en al menos un escenario Gherkin de su historia. Cada regla indica en "Cómo se verifica" los escenarios mínimos. BR-CONS-002 obliga a que los escenarios de BR-VAL-002 y BR-VAL-003 se ejecuten por las dos capas. BR-CONS-005 se verifica con una comparación antes/después de las rutas operativas del repo y una prueba de interrupción. Las decisiones Q-GRD-1 a Q-GRD-16 ya están incorporadas a las reglas. Las partes que siguen marcadas ⚠️ **ASSUMPTION** (S-GRD-6, S-GRD-9 y el formato Conventional Commits) se validan antes de cerrar las historias que las usen.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-03 | PO (AADD) para Rene Bonilla | Versión inicial: 22 reglas (17 críticas). Supuestos S1-S9 y preguntas P1-P13 en el contexto (numeración original). |
| 1.1 | 2026-10-03 | PO (AADD) para Rene Bonilla | Artifact Judge (RESERVAS): convención de IDs (P-GRD-n, S-GRD-n, R-GRD-n; IDs del Motor local calificados con "(motor-local)"); BR-CONS-001 separa lo decidido por Q23 de la ampliación pendiente (P-GRD-14) y marca las filas dependientes; BR-AUTH-001 deja de atribuir a Q27 una frase que no está en la fuente; nueva BR-CONS-006 (el comando no pisa cambios a mano y escribe de forma atómica y recuperable, NFR-01); BR-WF-002 con cuatro estados, todas las transiciones, alcance (P-GRD-15) y alineada con BR-EDGE-001. 23 reglas (18 críticas). |
| 1.2 | 2026-10-03 | PO (AADD) para Rene Bonilla | Decisiones Q-GRD-1 a Q-GRD-16 de Rene Bonilla (aceptan las recomendaciones de P-GRD-1 a P-GRD-16): los bloques de supuesto pasan a "Decisión"; BR-CONS-001 aplica Q-GRD-14 (un nivel personal endurece cualquier regla del equipo y nunca la relaja; refina Q23 de motor-local) y su tabla deja de tener filas dependientes; BR-TIME-001 fija el plazo en 5 minutos y BR-TIME-002 la retención en 90 días; BR-WF-002 aplica el alcance de Q-GRD-15; BR-EDGE-001 a BR-EDGE-005 aplican Q-GRD-5, 4, 8, 12 y 11; BR-EDGE-004 anota la dependencia con BR-CONS-007 (motor-local) para el Arquitecto o una revisión de motor-local. S-GRD-1, S-GRD-4 y S-GRD-5 confirmados por Q-GRD-14, Q-GRD-7 y Q-GRD-10. Sin reglas nuevas: 23 reglas (18 críticas). |
