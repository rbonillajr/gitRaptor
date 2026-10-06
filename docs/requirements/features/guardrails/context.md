---
id: CTX-GRD-001
title: "Contexto — Guardrails"
type: context
status: draft
created: 2026-10-03
updated: 2026-10-06
domain: GRP
epic: E-001
feature: guardrails
scope: feature
stakeholders:
  - rene-bonilla
related:
  rules:
    - BR-GRD-001
tags:
  - guardrails
  - politicas
  - permisos
  - configuracion-tres-niveles
  - niveles-por-valor
  - pedir-confirmacion
  - hooks-git
  - rama-base
  - registro-decisiones
  - mvp
---

# Contexto del Feature: Guardrails

> **Fundamento**: Este documento sigue un enfoque híbrido BRD-PRD adaptado para arquitectura pre-desarrollo. Combina objetivos de negocio (BRD) con requisitos de producto (PRD) para informar decisiones arquitectónicas en el framework AADD.
>
> **Origen**: [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.5) § 6.1, capacidades **BR-11, BR-12 y BR-13**, y los NFR-01 y NFR-02. Épica **E-001**, feature **F-001-04** del [backlog](../../backlog.md). Reglas de negocio en [business-rules.md](./business-rules.md) (BR-GRD-001).
>
> **Decisiones heredadas**: este requerimiento parte de decisiones ya tomadas en el [contexto del Motor local](../motor-local/context.md) (CTX-GRP-001). Se citan como "Q21 de motor-local". Donde una decisión de esta feature refina una decisión heredada, se anota en la tabla [Decisiones heredadas](#decisiones-heredadas-de-motor-local). Hoy hay dos: Q-GRD-14 refina Q23 de motor-local (un nivel personal endurece cualquier regla del equipo y nunca la relaja), y Q-GRD-20 y Q-GRD-21 refinan Q5 y Q12 de motor-local (de qué copia de la rama principal se lee la rama base y que el motor usa la rama base confirmada por el humano). Las decisiones propias de esta feature están en [Decisiones tomadas](#decisiones-tomadas).
>
> **Convención de IDs**: las preguntas, decisiones, supuestos y riesgos propios de esta feature llevan el prefijo de la feature: **P-GRD-n**, **Q-GRD-n**, **S-GRD-n** y **R-GRD-n**. Así no chocan con los P, Q, S y R del Motor local. Las reglas propias usan `BR-<CAT>-NNN` (catálogo BR-GRD-001). Cualquier ID del Motor local se califica siempre con "(motor-local)", p. ej. "BR-AUTH-002 (motor-local)" o "P8 (motor-local)". Las decisiones del Motor local se citan como "Q23 de motor-local".
>
> **Dependencia externa**: el formato y la estructura de la configuración los decide un ADR pendiente (**P8 (motor-local)**). Este documento solo fija qué valores existen, qué niveles admite cada uno y cómo se combinan.

---

## 1. Visión General (BRD)

Guardrails es el pilar de GitRaptor que **acota lo que los agentes pueden hacer con Git**. El equipo define, por repo, qué operaciones se permiten, cuáles se prohíben y cuáles necesitan que un humano las confirme. GitRaptor aplica esas reglas antes de que la operación ocurra, tanto si el agente usa las herramientas MCP de GitRaptor como si usa Git directamente.

La pieza central es la **configuración del repo**. Es versionable y viaja con el repo (BRD BR-11). Junto a ella hay dos niveles personales: el perfil del usuario y la configuración local personal del repo, que no se versiona (Q23 de motor-local). Guardrails es la **dueña** de la configuración del equipo y de la definición de negocio de los tres niveles. El Motor local solo la lee (Q23 de motor-local). Guardrails ofrece además el comando con el que el desarrollador la edita (Q27 de motor-local).

Guardrails es la única feature que **instala hooks de Git** (BRD BR-12; Q22 de motor-local). Esa instalación toca el repo del usuario, así que debe ser recuperable y no puede perder hooks que el usuario ya tenía (NFR-01).

Guardrails también desbloquea la historia **US-GRP-016** del Motor local: la rama base pasa a ser un valor de la configuración del equipo que Guardrails define y que el motor lee (Q36 de motor-local; ver § 2, "Qué entrega Guardrails al Motor local").

### Problema de Negocio (BRD)

Un agente de IA puede hacer en segundos un `reset --hard`, un force-push o borrar una rama. Puede hacerlo por error, por una mala instrucción o por *prompt injection* (BRD P3). El trabajo sin commitear que destruye no existe en ningún otro sitio. Hoy la única defensa es mirar lo que hace cada agente, y con 3 a 10 agentes en paralelo eso no escala (BRD § 1).

Las herramientas que existen no cubren el caso (BRD § 3.3): los proxies de políticas genéricos no entienden Git, los servidores MCP de Git dan Git crudo con límites mínimos (`mcp-server-git` tuvo CVEs en enero de 2026) y las funciones nativas de cada agente solo protegen a ese agente. Además, los agentes ignoran a menudo el MCP y usan Git crudo (BRD § 10, riesgo de probabilidad alta).

Impacto de no resolverlo:
- Pérdida de trabajo por una acción destructiva de un agente, que es el miedo principal del usuario objetivo (BRD P3, persona "Dev junior o semi-senior").
- La rama base (normalmente `main`) puede quedar rota o reescrita por un agente, y lo descubre el humano tarde.
- No hay forma de demostrar cuántas acciones peligrosas se evitaron, que es el argumento de venta del BRD (§ 9).
- La propuesta de valor "que nadie rompa main" (BRD § 4) queda sin cumplir.

### Valor Esperado (BRD)

- **ROI estimado**: no se cuantifica en dinero (herramienta interna, D1). El retorno es evitar incidentes de pérdida de trabajo y de rama base rota. No hay línea base de cuántos incidentes así ocurren hoy por semana en dogfooding; el valor se juzga por los bloqueos medidos (Confirmado por Rene Bonilla (2026-10-04)).
- **KPIs de éxito**: ver § 3. Los principales son medir las acciones peligrosas bloqueadas (KPI del BRD), cero operaciones prohibidas ejecutadas por la capa MCP, cero hooks del usuario perdidos al instalar o desinstalar y la demo "un agente intenta hacer force-push y queda bloqueado" (BRD § 13).
- **Beneficiarios**: el desarrollador orquestador (directo); los equipos internos piloto, que comparten las reglas del repo; el Motor local (lee la rama base del equipo); el Servidor MCP (consulta la decisión antes de ejecutar).

---

## 2. Dominio Específico (PRD)

- **Tipo de funcionalidad**: motor de decisión de políticas por repo, con su configuración en tres niveles, una cola de confirmación humana y dos capas de aplicación.
- **Usuarios principales**: desarrollador orquestador (humano, define reglas, decide en la cola, instala la protección); agente Claude Code (soporte completo en el MVP); cualquier otro agente registrado como "otro agente" (Q32 de motor-local); el equipo del repo, que comparte la configuración versionada.
- **Casos de uso principales**:
  - **Definir las reglas del repo para el equipo**: el desarrollador fija en la configuración del equipo qué operaciones se permiten, se prohíben o piden confirmación, y qué políticas aplican (ramas protegidas, tamaño de diff, formato de commit, rutas prohibidas). La configuración viaja con el repo y entra por commit.
  - **Endurecer las reglas para sí mismo**: el desarrollador añade restricciones en su perfil o en la configuración local personal del repo. Nunca puede relajar una prohibición del equipo (BR-CONS-001).
  - **Bloquear una operación peligrosa de un agente**: un agente intenta un force-push sobre `main`. GitRaptor la rechaza antes de que ocurra, le dice al agente qué regla lo impide y deja constancia (BR-CALC-001, BR-CONS-004).
  - **Pedir confirmación al humano**: un agente intenta una operación marcada como "pedir confirmación". La operación queda en la cola hasta que el humano la aprueba o la rechaza, o hasta que caduca (BR-WF-001). Solo el humano decide.
  - **Proteger también a quien usa Git crudo**: el desarrollador instala la protección de hooks en un repo, con su permiso explícito, sin perder los hooks que ya tenía (BR-AUTH-002, BR-CONS-005).
  - **Retirar la protección de hooks**: el desarrollador la desinstala y el repo vuelve a su estado anterior (BR-CONS-005): la entrada que tocó Guardrails recupera su valor efectivo y su nivel, y las demás no cambian; las únicas diferencias posibles son de formato, las introduce Git y se declaran (Q-GRD-29).
  - **Saber si un repo está protegido**: el desarrollador ve qué capas protegen cada repo: completa, solo MCP, solo hooks o ninguna (BR-WF-002). Ve también lo que espera su confirmación (relajación pendiente, rama base no confirmada o pendiente), con la acción para confirmarlo (Q-GRD-25).
  - **Editar la configuración con un comando**: el desarrollador edita cualquiera de los tres niveles; el comando rechaza un valor en un nivel que no lo admite (BR-VAL-001, Q27 de motor-local).
  - **Fijar la rama base del equipo**: el desarrollador define la rama base en la configuración del equipo y el Motor local la usa para calcular ahead/behind (BR-CONS-003; desbloquea US-GRP-016). En cada máquina, el humano confirma la rama base inicial (al instalar la protección o de forma explícita, Q-GRD-23) y cada cambio posterior antes de que se aplique (Q-GRD-21).
  - **Confirmar una relajación del equipo**: cuando llega a la rama principal un cambio de la configuración del equipo que relaja una regla (incluidos desactivar el conjunto mínimo y cambiar la rama base), el desarrollador lo confirma en su máquina. Hasta entonces rige la combinación más restrictiva y GitRaptor avisa (BR-AUTH-001, Q-GRD-21, Q-GRD-22).
  - **Consultar qué se bloqueó**: el desarrollador consulta el registro de operaciones denegadas, pedidas y saltadas a conciencia, por repo (BR-CONS-004).
- **Alcance**:
  - **IN scope**:
    - Definición de negocio de la configuración en tres niveles: qué valores existen, qué niveles admite cada uno y cómo se combinan (BR-VAL-001, BR-CONS-001). Guardrails es dueña de la configuración del equipo (Q23 de motor-local).
    - Respetar los niveles admitidos que fijó el Motor local para sus valores: rama base solo en el nivel de equipo, `main` por defecto; umbral de inactividad solo en el perfil y en la configuración local personal, 5 minutos por defecto (Q24 de motor-local).
    - Catálogo de operaciones gobernadas de los agentes: commit, push, force-push, `reset --hard`, borrar rama, rebase, merge, crear worktree y borrar worktree, cada una con permiso **permitir / pedir confirmación / denegar** (BR-VAL-002).
    - Las políticas de BR-11: ramas protegidas, prohibir force-push, prohibir `reset --hard`, límite de tamaño de diff, formato de commit y rutas prohibidas (BR-VAL-003). Prioridad entre ellas: Q-GRD-9.
    - Decisión única por operación: la más restrictiva de todas las reglas que aplican (BR-CALC-001).
    - Un nivel personal puede endurecer, nunca relajar, una regla del equipo (BR-CONS-001).
    - Modo "pedir confirmación" (BR-13, **Should**): la cola de peticiones, su ciclo de vida (pendiente, aprobada, rechazada, caducada) y quién decide: solo el humano (BR-WF-001, BR-TIME-001).
    - La misma decisión en las dos capas de aplicación: herramientas MCP y hooks de Git (BR-CONS-002). Guardrails define la decisión; las herramientas MCP son de F-001-05.
    - Instalación y desinstalación de la protección de hooks en un repo: explícita, con permiso del desarrollador, recuperable, sin perder hooks previos y solo dentro del repo (BR-AUTH-002, BR-CONS-005, BR-EDGE-002).
    - Estado de protección de cada repo, visible para el desarrollador, y qué repos entran en el alcance de Guardrails (BR-WF-002, Q-GRD-15).
    - Comando para editar la configuración en sus tres niveles (Q27 de motor-local), que respeta los niveles admitidos y que un agente no puede usar para relajar una regla (BR-VAL-001, BR-AUTH-004). Nunca pisa cambios hechos a mano y su escritura es recuperable (BR-CONS-006 (comando, Guardrails), NFR-01).
    - Protección de la propia configuración frente a los agentes (BR-AUTH-004).
    - Registro de decisiones (denegadas, pedidas y su resultado, excepciones conscientes) en el perfil de GitRaptor, separado por repo (BR-CONS-004, S-GRD-5).
    - Comportamiento sin configuración, con configuración ilegible y con límites de la capa de hooks (BR-EDGE-001, BR-EDGE-003, BR-EDGE-004).
  - **OUT of scope**:
    - **Formato y estructura de la configuración** (tipo de archivo, nombres, ubicación exacta): los decide el ADR pendiente P8 (motor-local).
    - **Presentar la cola de confirmación** en la TUI (pantallas, atajos, avisos): es del Cockpit (F-001-02). Guardrails define la cola y sus reglas.
    - **Las herramientas MCP en sí** (`safe_commit`, `safe_rebase`, etc.), su instalación y su endurecimiento (allowlist, validación de entradas): son del Servidor MCP (F-001-05, BR-14 a BR-16). Guardrails da la decisión que esas herramientas consultan.
    - **Tomar snapshots** antes de una operación destructiva permitida: es de la Time Machine (F-001-03, BR-08). Guardrails solo depende de ello (BR-EDGE-005).
    - **Atribuir la operación a un actor**: es del Motor local (F-001-01). Guardrails usa "agente X" o "sin atribuir" tal como el motor los emite (Q34 y Q35 de motor-local).
    - **Políticas centralizadas de organización** y herencia desde un repo central (BR-23, Fase 3).
    - **Auditoría exportable** del registro hacia herramientas externas (BR-24, Fase 3).
    - **Soporte completo de agentes distintos de Claude Code**: Codex y luego Cursor llegan después; mientras tanto son "otro agente" (Q32 de motor-local).
    - **Integración con los permisos nativos de cada agente** (p. ej. la configuración de permisos propia de Claude Code): fase posterior (ver Q-GRD-8).
    - **Permisos distintos por agente**: fase posterior (Q-GRD-2).
    - **Proteger operaciones que Git no deja interceptar** mediante hooks (p. ej. `reset --hard` hecho con Git crudo, o renombrar la rama base en un repo reftable, Q-GRD-28): límite declarado del MVP; lo mitiga la Time Machine (BR-EDGE-003, Q-GRD-8).
    - **Aviso propio de Guardrails y recuperación guiada** cuando la rama base desaparece o se reescribe sin pasar por un hook (repos reftable): backlog posterior al MVP (Could, Q-GRD-28).
    - **Instalar hooks fuera del repo** (configuración global de Git, plantillas del usuario, otros repos): nunca (Q17 de motor-local).
    - **Hacer commit de la configuración del equipo por el desarrollador**: el comando de edición deja el cambio en el working tree; el commit lo hace el desarrollador (S-GRD-3).

### Dependencias con otras features

| Feature | Relación con Guardrails | Dirección |
|---------|-------------------------|-----------|
| F-001-01 Motor local | Guardrails necesita saber qué actor intenta cada operación: "agente X" (detectado o registrado) o "sin atribuir" (Q34, Q35 de motor-local). El motor lee la configuración en tres niveles que define Guardrails, incluida la rama base del equipo (Q23, Q24, Q36 de motor-local). El motor puede leer como señal opcional la información de los hooks de Guardrails, sin depender de ella (Q22 de motor-local). | Bidireccional |
| F-001-02 Cockpit | Presenta la cola de confirmación y permite al humano aprobar o rechazar (BR-13 dice "en la TUI"). Presenta el estado de protección de cada repo. Si una acción del Cockpit (p. ej. aprobar un merge a la rama base) choca con una regla, ver Q-GRD-1. | Cockpit depende de Guardrails |
| F-001-03 Time Machine | Toma el snapshot antes de toda operación destructiva que Guardrails permite o que el humano aprueba (NFR-01). Es la mitigación de las operaciones que la capa de hooks no puede interceptar (BR-EDGE-003). Si no puede tomar el snapshot, ver Q-GRD-11. | Guardrails depende de la Time Machine |
| F-001-05 Servidor MCP | Sus herramientas consultan la decisión de Guardrails antes de ejecutar y la respetan: denegar, pedir confirmación o permitir. Solo opera sobre repos de la allowlist (NFR-02). Ninguna herramienta MCP permite editar la configuración ni decidir en la cola (BR-AUTH-004). | MCP depende de Guardrails |

### Qué entrega Guardrails al Motor local (desbloquea US-GRP-016)

US-GRP-016 está bloqueada por Guardrails y por el ADR de formato P8 (motor-local) (Q36 de motor-local). **Solo tres cosas la desbloquean**:

1. **La rama base como valor del nivel de equipo**, el único nivel que la admite, con `main` por defecto (BR-CONS-003, Q24 de motor-local). Se lee de la configuración del equipo **commiteada en la rama principal del repo** (la que el remoto marca como principal, o `main` si no hay ninguna), no de la versión de cada worktree ni de ediciones sin commitear (Q-GRD-18). De la rama principal se usa la copia que el repo ya conoce del remoto, sin consultarlo; si no hay remoto, la rama local (Q-GRD-20). La define este requerimiento.
2. **La regla de lectura** que aplica el motor: la rama base efectiva sale solo del nivel de equipo, en esa copia de la rama principal; un valor en otro nivel no se tiene en cuenta (BR-CONS-003). El motor y Guardrails usan el mismo valor: la rama base **confirmada** por el humano en esa máquina. Un cambio en la rama principal queda pendiente de confirmar, con aviso, y mientras no hay confirmación inicial el motor calcula contra la rama base leída y la marca como "no confirmada" (Q-GRD-21). La define este requerimiento.
3. **El ADR de formato P8 (motor-local)**, que fija dónde y cómo se escribe ese valor. Es externo a esta feature.

**El comando de edición no es un prerrequisito** de US-GRP-016: el desarrollador puede escribir la rama base a mano en la configuración del equipo. El comando es una comodidad que Guardrails entrega aparte (BR-VAL-001, BR-CONS-006 (comando, Guardrails)). Qué hace el comando si se fija una rama base que no existe en el repo queda en Q-GRD-16 (relacionada con Q42 de motor-local).

US-GRP-013 (umbral de inactividad) solo depende de P8 (motor-local); tampoco necesita el comando, que le sirve para editar el umbral en el perfil y en la configuración local personal.

---

## 3. Objetivos de Negocio (BRD)

| Objetivo | Métrica de Éxito | Prioridad |
|----------|------------------|-----------|
| Evitar acciones peligrosas de los agentes | Número de operaciones denegadas, rechazadas en la cola o caducadas, por repo y por semana, **verificadas por GitRaptor**. Las anotadas en modo degradado, que GitRaptor no pudo verificar, se muestran aparte y no entran en el recuento por defecto (BR-CONS-004, Q-GRD-27). Se mide desde el primer día (KPI del BRD § 9, sin meta numérica) | Alta |
| Que la capa MCP no deje pasar nada prohibido | 0 operaciones denegadas por la configuración que se ejecutan a través de las herramientas MCP de GitRaptor | Alta |
| Proteger también a quien usa Git crudo | Con la protección de hooks instalada, el 100% de las operaciones gobernadas que Git permite interceptar se evalúan antes de ejecutarse. Las que no se pueden interceptar están listadas (BR-EDGE-003) | Alta |
| Instalar la protección sin riesgo | 0 hooks del usuario perdidos o alterados al instalar o desinstalar; tras desinstalar, el repo queda idéntico a su estado anterior (NFR-01) | Alta |
| Demostrar el valor | La demo del BRD § 13 funciona: un agente intenta un force-push sobre la rama base y queda bloqueado, con la regla explicada | Alta |
| Adopción interna | ≥ 5 repos internos con políticas configuradas a 6 meses del MVP (BRD § 9) | Media |
| No estorbar al trabajo legítimo | Se mide cuántas veces por semana el humano usa una excepción consciente o relaja una regla (señal de exceso de bloqueo). Sin meta hasta tener línea base en dogfooding (Confirmado por Rene Bonilla (2026-10-04)) | Media |

---

## 4. Stakeholders y Actores (BRD + PRD)

### Stakeholders de Negocio (BRD)

| Stakeholder | Interés | Expectativa |
|-------------|---------|-------------|
| Rene Bonilla (producto, revisión e integración) | Único humano del proyecto (D3) y primer usuario. Construye GitRaptor con agentes en paralelo y necesita que no rompan `main`. | Que un agente no pueda destruir trabajo ni reescribir la rama base, y que la protección no le estorbe en su propio trabajo. |
| Equipos internos piloto (2, BRD § 9) | Comparten las reglas del repo a través de la configuración versionada. | Reglas del equipo que nadie relaje en su máquina. `[POR VERIFICAR]` quiénes son. |

### Actores del Sistema (PRD)

| Actor | Descripción | Permisos/Capacidades |
|-------|-------------|----------------------|
| **Desarrollador orquestador** | Persona que lanza y supervisa los agentes (D3). | Edita los tres niveles de la configuración; aprueba o rechaza peticiones de la cola; instala y desinstala la protección de hooks; usa una excepción consciente para saltarse una regla puntual (Q-GRD-1); confirma en su máquina la rama base inicial y cada relajación que llegue a la configuración del equipo en la rama principal (Q-GRD-21). Es el único que concede un permiso operativo (BR-AUTH-002). |
| **Agente Claude Code** | Agente con soporte completo en el MVP (Q32 de motor-local). | Sus operaciones gobernadas pasan por la decisión de Guardrails. No puede editar la configuración para relajar una regla, decidir en la cola, instalar ni desinstalar hooks, usar una excepción ni confirmar un cambio de la configuración del equipo o de la rama base (BR-AUTH-001). |
| **Otro agente** (Codex, Cursor, Copilot u otro) | Agente registrado de forma explícita (Q32 de motor-local). | Igual que Claude Code. |
| **Actor "sin atribuir"** | Operación que el motor no asigna a ningún agente: el humano o un agente sin registrar que no se detecta. Son indistinguibles (Q35 de motor-local). | Las reglas se le aplican como a cualquier actor (Q-GRD-1, fail-safe). El humano dispone de la excepción consciente. |
| **Equipo del repo** | Personas que comparten el repo y su configuración versionada. | Cambian la configuración del equipo por commit revisado (S-GRD-4). |
| **Git del sistema** | Ejecuta las operaciones. | Respeta los hooks del usuario (NFR-07). Guardrails no lo reemplaza. |

---

## 5. Restricciones y Limitaciones (BRD + Arquitectura)

### Regulatorias (BRD + Arquitectura)
- No hay restricciones regulatorias específicas: herramienta interna y 100% local (NFR-03). Ver [domain-context.md](../../../domain-context.md). `[POR VERIFICAR]` la política interna de ASSA (P1 (motor-local)).
- El registro de decisiones contiene nombres de ramas, rutas y mensajes de commit, que son confidenciales: no sale de la máquina (NFR-03).

### Técnicas (Arquitectura)
- **Cero pérdida de datos (NFR-01)**: instalar o desinstalar la protección de hooks es una modificación operativa. Debe ser recuperable, conservar los hooks previos del usuario y volver a su estado anterior con el criterio semántico de Q-GRD-29 (BR-CONS-005).
- **Seguridad del MCP (NFR-02)**: la capa MCP solo opera sobre repos de la allowlist. Ninguna herramienta MCP edita la configuración ni decide en la cola (BR-AUTH-004).
- **Respeto a Git y al usuario (NFR-07)**: Guardrails respeta la configuración, los hooks y las credenciales del usuario.
- **Hooks solo dentro del repo**: nunca en la configuración global de Git ni en otros repos (Q17 de motor-local).
- **El Motor local no escribe** la configuración ni instala hooks (Q21, Q22, Q23 de motor-local). Toda escritura de esta feature es de Guardrails y lleva su propia garantía NFR-01.
- **Límite de la capa de hooks**: hay operaciones destructivas que Git no deja interceptar y Git permite saltarse los hooks a propósito (p. ej. con `--no-verify`). La capa de hooks reduce el riesgo; no lo elimina (BR-EDGE-003, Q-GRD-8).
- **Formato de la configuración**: lo decide el ADR P8 (motor-local). Este requerimiento no lo fija.
- **Distinguir al humano**: las acciones reservadas al humano (decidir en la cola, relajar una regla, excepción consciente, confirmar un cambio de la configuración del equipo o de la rama base) exigen una confirmación que un agente no pueda dar desde su canal. Cómo se logra lo decide el Arquitecto (BR-AUTH-001, riesgo R-GRD-3). En el MVP se acepta un riesgo residual con anuncio, ventana para cancelar y auditoría; relajar con el comando de edición y aprobar en la cola esperan a un factor de autenticación del sistema operativo (Q-GRD-19, Q-GRD-22).

### De Negocio (BRD)
- Una persona orquestando agentes (D3): historias pequeñas y verificables.
- Soporte completo solo para Claude Code en el MVP (D2, Q32 de motor-local).
- BR-11 y BR-12 son **Must**; BR-13 ("pedir confirmación") es **Should**.
- Sin fecha objetivo para el MVP (BRD § 12.2).

---

## 6. Requisitos No Funcionales Destacados (PRD + Arquitectura)

| RNF | Valor Objetivo | Crítico | Justificación |
|-----|----------------|---------|---------------|
| **Cero pérdida de datos** | Instalar y desinstalar la protección de hooks no pierde ni altera nada del usuario. Desinstalar deja las rutas operativas **como estaban**: la entrada que tocó Guardrails vuelve a su valor efectivo y a su nivel, y las demás entradas no cambian. Las únicas diferencias posibles son de formato y las introduce Git (un comentario en la línea de esa entrada, el salto de línea final); se declaran (NFR-01, Q-GRD-29) | Sí | Un bug aquí destruye hooks que el usuario no puede recuperar. |
| **Fail-safe** | Ante una duda (configuración ilegible, sin respuesta en la cola, actor desconocido) la operación de riesgo no se ejecuta sin decisión humana (BR-EDGE-004, BR-TIME-001, Q-GRD-1) | Sí | Un fallo de Guardrails no puede dejar pasar lo que debía bloquear. |
| **Decisión inmediata** | Evaluar una operación añade < 100 ms a la operación en repos medianos (Confirmado por Rene Bonilla (2026-10-04); el BRD no fija un valor). **Por comando** (Q-GRD-30): un comando habitual añade sus evaluaciones (normalmente una) más un coste fijo por cada proceso de hook que no evalúa (⚠️ **ASSUMPTION**: ≤ 5 ms p95); un commit o un cambio de rama habitual añade ⚠️ **ASSUMPTION** ≤ 150 ms p95 en total, por confirmar por Rene Bonilla. Las operaciones masivas (muchas refs en una sola operación) tienen un coste proporcional al número de refs, declarado en la documentación y en la explicación del permiso. Si Windows multiplica estas cifras, vuelve al PO | No | Los agentes hacen muchas operaciones; una espera visible frena el trabajo. |
| **Seguridad del MCP** | Allowlist, sin shell, entradas validadas (NFR-02) | Sí | El MCP es un vector de ataque (BRD § 10). |
| **Privacidad** | El registro de decisiones vive en la máquina (NFR-03) | Sí | Contiene datos confidenciales del repo. |
| **Portabilidad** | El mismo comportamiento en Windows, macOS y Linux (BR-03, NFR-06) | Sí | Hueco de mercado principal (BRD P7). |
| **i18n** | Los motivos de bloqueo y los mensajes de la cola en inglés y español (NFR-10) | No | Convención del producto. |
| **Explicabilidad** | Toda denegación dice qué regla la causó, de qué nivel viene y qué puede hacer el humano (BR-CALC-001) | Sí | Un bloqueo sin motivo hace que el usuario desactive la protección. |

---

## 7. Integraciones Externas (PRD + Arquitectura)

| Sistema/API | Propósito | Tipo de Integración | Criticidad |
|-------------|-----------|---------------------|------------|
| Git del sistema (≥ 2.38, NFR-07) | Interceptar las operaciones de Git crudo mediante hooks | Hooks de Git instalados por Guardrails, solo dentro del repo. Mecanismo: lo define el Arquitecto | Alta |
| Hooks previos del usuario o de otro gestor de hooks | Convivir con ellos sin perderlos | Detectar, informar y encadenar solo con permiso (Q-GRD-4). Mecanismo: lo define el Arquitecto | Alta |
| Motor local (F-001-01) | Actor de cada operación; lectura de la configuración | Interna. Dos valores de actor: "agente X" o "sin atribuir" | Alta |
| Servidor MCP (F-001-05) | Consultar la decisión antes de ejecutar una herramienta | Interna. Mecanismo: lo define el Arquitecto | Alta |
| Time Machine (F-001-03) | Snapshot antes de una operación destructiva permitida | Interna (NFR-01) | Alta |
| Cockpit (F-001-02) | Presentar la cola y el estado de protección | Interna | Media |
| Claude Code | Recibir la decisión cuando opera vía MCP o vía Git crudo | Sin APIs privadas del agente (NFR-08). Integrar con sus permisos nativos: fuera del MVP | Media |

---

## 8. Características Únicas del Feature (PRD)

- **Políticas con semántica de Git**: las reglas hablan de ramas, rutas, diffs y commits, no de comandos genéricos (BRD § 3.3).
- **Dos capas con una sola decisión**: la misma operación recibe la misma respuesta por MCP o por Git crudo (BR-CONS-002).
- **Reglas del equipo que nadie relaja en su máquina**: los niveles personales solo endurecen (BR-CONS-001).
- **El humano decide, el agente espera**: las acciones de riesgo pueden quedar en cola para una decisión humana (BR-13).
- **Instalación que no rompe nada**: los hooks del usuario se conservan y la desinstalación deja el repo como estaba (NFR-01).
- **Bloqueos explicados y contados**: cada bloqueo dice qué regla lo causó y queda en el registro (KPI del BRD).

---

## 9. Glosario del Dominio (PRD)

| Término | Definición | Sinónimos/Notas |
|---------|------------|-----------------|
| **Operación gobernada** | Operación de Git que Guardrails evalúa antes de que ocurra: commit, push, force-push, `reset --hard`, borrar rama, rebase, merge, crear worktree, borrar worktree. | BR-VAL-002. |
| **Permiso** | Valor que la configuración asigna a una operación gobernada: **permitir**, **pedir confirmación** o **denegar**. | Valores: permitir / pedir confirmación / denegar. Orden de restricción: denegar > pedir confirmación > permitir. Los nombres en la configuración los fija el ADR P8 (motor-local). |
| **Política** | Regla con condición sobre la operación: ramas protegidas, prohibir force-push, prohibir `reset --hard`, límite de tamaño de diff, formato de commit, rutas prohibidas. | BR-VAL-003, BRD BR-11. |
| **Regla** | Un permiso o una política. | — |
| **Decisión** | Resultado de evaluar una operación: permitir, pedir confirmación o denegar, con la regla y el nivel que la causaron. | BR-CALC-001. |
| **Configuración del equipo** | Nivel de la configuración versionado con el repo y compartido por el equipo. Guardrails es su dueña. | BRD BR-11; Q23 de motor-local. Formato: ADR P8 (motor-local). |
| **Nivel personal** | El perfil del usuario o la configuración local personal del repo (no versionada). | Q23 de motor-local. |
| **Niveles admitidos** | Niveles en los que se puede definir un valor. Un valor en otro nivel no se tiene en cuenta. | Q24 de motor-local; BR-VAL-001. |
| **Endurecer / relajar** | Endurecer es hacer una regla más restrictiva; relajar, menos restrictiva. Un nivel personal solo endurece las reglas del equipo. | BR-CONS-001. |
| **Rama protegida** | Rama que solo cambia por una acción consciente del humano. Ningún agente hace commit, push, force-push, reset ni borrado sobre ella. | BR-VAL-003. |
| **Ruta prohibida** | Ruta del repo que un agente no puede modificar en un commit. | BR-VAL-003. |
| **Petición de confirmación** | Operación de riesgo en espera de una decisión humana. Estados: pendiente, aprobada, rechazada, caducada. | BR-WF-001, BRD BR-13. |
| **Cola de confirmación** | Conjunto de peticiones pendientes de un repo. La presenta el Cockpit. | BR-WF-001. |
| **Capa MCP** | Aplicación de la decisión en las herramientas MCP de GitRaptor, antes de ejecutar. | BRD BR-12 (a). Herramientas: F-001-05. |
| **Capa de hooks** | Aplicación de la decisión en los hooks de Git, para agentes que usan Git crudo. | BRD BR-12 (b). |
| **Protección de hooks** | Los hooks que Guardrails instala en un repo. Es una modificación operativa. | BR-AUTH-002, BR-CONS-005. |
| **Hooks previos** | Hooks que el usuario u otro gestor ya tenían en el repo antes de instalar la protección. Nunca se pierden. | BR-CONS-005, BR-EDGE-002. |
| **Estado de protección** | Qué capas aplican las reglas en un repo: completa (MCP + hooks), solo MCP, solo hooks o sin protección (ninguna capa activa). La falta de configuración no lo cambia: con una capa activa aplica el conjunto mínimo. Se acompaña de diagnósticos (relajación pendiente de confirmar; rama base no confirmada o pendiente), que no son estados. | BR-WF-002, Q-GRD-25. |
| **Excepción consciente** | Vía explícita del humano para saltarse una regla en una operación concreta. Queda registrada. | Q-GRD-1. Un agente no puede usarla. |
| **Registro de decisiones** | Historial de operaciones denegadas, pedidas (con su resultado) y excepciones conscientes, por repo, en el perfil. | BR-CONS-004. |
| **Conjunto mínimo por defecto** | Reglas que aplican en un repo sin configuración. | BR-EDGE-001, Q-GRD-5. Desactivarlo exige la confirmación del humano en cada máquina (Q-GRD-21). |
| **Rama principal** | La rama que el remoto marca como principal, o `main`. De ella se usa la copia que el repo ya conoce del remoto, sin consultarlo; si no hay remoto, la rama local. | Q-GRD-18, Q-GRD-20. Es la única fuente de las relajaciones del equipo y de la rama base. |
| **Rama base confirmada** | La rama base que el humano confirmó en su máquina. Es el único valor que usan Guardrails y el motor. Un cambio en la rama principal queda pendiente hasta su confirmación. | BR-CONS-003, Q-GRD-21. |
| **Relajación pendiente** | Cambio de la configuración del equipo en la rama principal que baja la protección (desactivar el mínimo, un "permitir", un valor menos restrictivo, otra rama base) y que el humano aún no confirmó en esa máquina. Mientras tanto rige la combinación más restrictiva, con aviso. | Q-GRD-21, BR-AUTH-001. |
| **Permiso explícito** | Principio del producto: autorización del humano a una modificación operativa concreta en un repo, tras ver qué, dónde, por qué y cómo se revierte. | BR-AUTH-002 (motor-local); BR-AUTH-002 de esta feature. |
| **Sin atribuir** | Valor del motor para lo que no asigna a ningún agente. Incluye al humano y a agentes sin registrar. | Q34, Q35 de motor-local. |

---

## 10. Estándares Aplicables (Arquitectura)

- **Interoperabilidad**: Git nativo y sus hooks estándar; MCP estándar (NFR-08). El resto: transversal (lo define el Arquitecto).
- **Codificación y Terminología**: Conventional Commits como formato de commit que el equipo puede exigir (domain-context). El MVP ofrece al menos ese formato (Confirmado por Rene Bonilla (2026-10-04)).
- **Seguridad y Autenticación**: sin autenticación de usuarios (herramienta local de un usuario). Distinguir al humano para acciones reservadas: transversal (lo define el Arquitecto).
- **Compliance**: solo políticas internas (ver § 5).

---

## 11. Referencias (BRD + PRD + Arquitectura)

- [BRD-GRP-001 — Documento de negocio](../../../business/gitraptor-documento-de-negocio.md) (v0.5): § 2 (P3), § 3.3, § 4, § 6.1 (BR-11 a BR-13), § 6.3 (BR-23, BR-24), § 7 (NFR-01, NFR-02, NFR-07), § 9 (KPIs), § 10 (riesgos), § 13 (demo).
- [Contexto del Motor local](../motor-local/context.md) (CTX-GRP-001): Q17, Q21-Q24, Q27, Q32, Q34-Q36; P8 (motor-local).
- [Reglas del Motor local](../motor-local/business-rules.md) (BR-GRP-001): BR-AUTH-002 (motor-local), BR-CONS-001 (motor-local), BR-CONS-006 (motor-local), BR-CONS-007 (motor-local), BR-TIME-001 (motor-local).
- [US-GRP-016](../motor-local/user-stories/US-GRP-016-rama-base-configuracion-equipo.md): rama base de la configuración del equipo (bloqueada por esta feature y P8 (motor-local)).
- [Backlog](../../backlog.md): E-001 / F-001-04.
- [Reglas de negocio de esta feature](./business-rules.md) (BR-GRD-001).
- [Contexto del dominio](../../../domain-context.md).

---

## Decisiones heredadas (de motor-local)

| Decisión | Qué fija para Guardrails |
|----------|--------------------------|
| Q17 de motor-local | Fuera del repo no se modifica nada del usuario (configuración global de Git, otros repos). Los hooks se instalan solo dentro del repo. |
| Q21 de motor-local | El motor no escribe en el repo; sus datos viven en el perfil, por repo. Guardrails sigue el mismo criterio para su registro (S-GRD-5). |
| Q22 de motor-local | El motor no instala hooks; los hooks son de Guardrails. El motor puede leer sus señales como información opcional. |
| Q23 de motor-local | Tres niveles: perfil < configuración del equipo (versionada) < configuración local personal (no versionada). El motor solo lee. Un nivel personal no puede relajar una prohibición del equipo (regla de Guardrails). **Refinada por Q-GRD-14** (2026-10-03): Q23 dejaba que el equipo ganara al perfil, con la única excepción de no relajar una prohibición del equipo. Q-GRD-14 amplía esa excepción: cualquier nivel personal (perfil o local) endurece cualquier regla del equipo y ninguno la relaja; un endurecimiento del perfil prevalece sobre un "permitir" del equipo (BR-CONS-001). No cambia la precedencia de los valores del motor (rama base, umbral de inactividad). |
| BR-CONS-007 (motor-local) | El motor tiene como supuesto ignorar un nivel ilegible de la configuración. **Roce con Q-GRD-12**: Guardrails, ante un nivel ilegible, aplica el conjunto mínimo y lo legible, nunca "todo permitido" (BR-EDGE-004). Dependencia abierta para el Arquitecto o para una revisión de motor-local: alinear cómo se trata la misma configuración ilegible según quién la lea. Este requerimiento no edita motor-local. |
| Q24 de motor-local | Cada valor declara sus niveles admitidos; dentro de ellos gana el más específico. Rama base: solo equipo, `main` por defecto. Umbral de inactividad: solo perfil y local personal, 5 minutos por defecto. |
| Q27 de motor-local | El comando para editar la configuración es de Guardrails. |
| Q32 de motor-local | Soporte completo solo para Claude Code; otros agentes como "otro agente" por registro explícito. Cursor es el editor del humano. |
| Q34, Q35 de motor-local | El motor emite "agente X" o "sin atribuir"; nunca "humano". |
| Q36 de motor-local | US-GRP-016 está bloqueada por Guardrails y por el ADR P8 (motor-local). |
| Q5 y Q12 de motor-local | Q5: el motor lee la rama base de la configuración del equipo; no la define. Q12: el motor no consulta el remoto por su cuenta. **Refinadas por Q-GRD-20 y Q-GRD-21** (2026-10-04, posteriores a la aprobación del requerimiento): la rama base se lee de la configuración commiteada en la copia que el repo ya conoce del remoto para la rama principal (sin consultarlo; si no hay remoto, la rama local), no del archivo en disco del worktree principal. El motor calcula el ahead/behind contra la rama base **confirmada** por el humano, el mismo valor que protege Guardrails. Un cambio de rama base en la rama principal queda para el motor como diagnóstico "pendiente de confirmar" hasta la confirmación. Mientras no hay confirmación inicial, el motor calcula contra la rama base leída y la marca como "no confirmada". Se lleva a BR-CONS-006 (motor-local) y US-GRP-016 como decisión heredada de Guardrails. |
| BR-AUTH-002 (motor-local) | Modelo de permiso explícito: qué, dónde, por qué, cómo se revierte; solo el humano concede; no se repregunta tras denegar; un permiso por modificación y repo. |

---

## Supuestos

| # | Supuesto | Estado |
|---|----------|--------|
| S-GRD-1 | Las reglas de permisos y políticas admiten los tres niveles; los personales solo endurecen (BR-VAL-001, BR-CONS-001). | ✅ Confirmado por Q-GRD-14 (2026-10-03) |
| S-GRD-2 | Si el equipo fija un formato de commit, un nivel personal no puede cambiarlo por otro; solo puede exigirlo donde el equipo no lo exige. | ✅ Confirmado por Rene Bonilla (2026-10-04) |
| S-GRD-3 | El comando de edición nunca hace commit. Un cambio en la configuración del equipo queda en el working tree y lo commitea el desarrollador. | ✅ Confirmado por Rene Bonilla (2026-10-04) |
| S-GRD-4 | Los cambios a la configuración del equipo entran por commit revisado, como cualquier cambio del repo (Q-GRD-7). | ✅ Confirmado por Q-GRD-7 (2026-10-03) |
| S-GRD-5 | El registro de decisiones vive en el perfil de GitRaptor, separado por repo, nunca en el repo (coherente con Q21 de motor-local). | ✅ Confirmado por Q-GRD-10 (2026-10-03) |
| S-GRD-6 | Instalar la protección de hooks en un repo cubre todos sus worktrees, los actuales y los que se creen después. | ✅ Confirmado por Rene Bonilla (2026-10-04). La viabilidad técnica sigue siendo comprobación del Arquitecto |
| S-GRD-7 | El límite de tamaño de diff se mide en líneas cambiadas (añadidas más eliminadas) por commit. | ✅ Confirmado por Rene Bonilla (2026-10-04) |
| S-GRD-8 | Una aprobación vale para la operación concreta pedida, una sola vez. No se convierte en un permiso permanente. | ✅ Confirmado por Rene Bonilla (2026-10-04) |
| S-GRD-9 | Mientras el modo "pedir confirmación" (BR-13, Should) no esté disponible, una operación con "pedir confirmación" se trata como "denegar" (fail-safe). | ✅ Confirmado por Rene Bonilla (2026-10-04) |

## Riesgos

| # | Riesgo | Prob. | Impacto | Mitigación (de negocio) |
|---|--------|-------|---------|-------------------------|
| R-GRD-1 | **Los agentes saltan también la capa de hooks** (usan Git crudo con hooks desactivados o hacen operaciones que no disparan hooks). **Difiere del riesgo del BRD § 10** ("los agentes ignoran el MCP y usan Git crudo", Alta / Medio): el BRD valora el riesgo con la doble capa como mitigación, y su impacto Medio supone que los hooks atrapan lo que el MCP no ve. Este riesgo es el residuo que queda cuando la segunda capa tampoco actúa: ahí la operación destructiva se ejecuta sin ninguna regla y el trabajo sin commitear se puede perder, por eso el impacto es Alto. La probabilidad baja a Media porque exige saltarse los hooks o usar una operación que no los dispara. | Media | Alto | Declarar el límite (BR-EDGE-003), incluido el renombrado de la rama base en repos reftable (Q-GRD-28); la Time Machine como red de seguridad; integración con permisos nativos del agente en fase posterior (Q-GRD-8). |
| R-GRD-2 | **Exceso de bloqueo**: las reglas aplicadas a "sin atribuir" (Q-GRD-1) bloquean al humano en su trabajo y acaba desactivando la protección. | Media | Alto | Excepción consciente registrada; medir su uso (§ 3); conjunto mínimo por defecto pequeño (Q-GRD-5). |
| R-GRD-3 | **Un agente actúa como humano**: usa la CLI de GitRaptor desde su terminal para aprobar su propia petición, relajar una regla o usar una excepción. | Media | Crítico | Las acciones reservadas al humano exigen una confirmación que un agente no pueda dar (BR-AUTH-001); lo resuelve el Arquitecto. Ninguna herramienta MCP las ofrece (BR-AUTH-004). **Riesgo aceptado en el MVP** (Q-GRD-19) para desinstalar, la excepción consciente y las confirmaciones de cambios del equipo (Q-GRD-22): anuncio, ventana para cancelar y auditoría. Relajar con el comando y aprobar en la cola no salen sin el factor de autenticación del sistema operativo: lo invoca el daemon, fuera del canal del agente, y sin él es fail-closed (ADR-GRD-008, aceptado el 2026-10-04). **Riesgo residual aceptado** para la adopción posterior del factor en modo preferente (OQ-GRD-008-3): un agente que vuelve el factor "no disponible" fuerza el mecanismo del MVP, sin empeorar lo de hoy. |
| R-GRD-4 | **Un agente relaja la configuración editando el archivo** en el working tree. | Media | Alto | La configuración es ruta prohibida para agentes por defecto (BR-AUTH-004, Q-GRD-7); los cambios del equipo entran por commit revisado (S-GRD-4). En los niveles personales (perfil y local), quitar un endurecimiento editando el archivo no rige hasta confirmarlo con el factor del sistema operativo (Q-GRD-32, ADR-GRD-008). Queda el riesgo residual de un registro confirmado forjado con el daemon parado (hueco auditado). |
| R-GRD-5 | **Instalar hooks rompe los del usuario** o los de otro gestor. | Media | Crítico | Nunca reemplazar; detectar, informar y encadenar solo con permiso (BR-EDGE-002, Q-GRD-4); desinstalar restaura el estado exacto (BR-CONS-005). |
| R-GRD-6 | **Fatiga de la cola**: demasiadas peticiones y el humano aprueba sin leer. | Media | Medio | "Pedir confirmación" solo para operaciones de riesgo; peticiones con contexto suficiente (qué, dónde, quién, qué regla); medir cuántas se aprueban. |
| R-GRD-7 | **Formato de la configuración sin decidir** (ADR P8 (motor-local)). | Alta | Medio | Este requerimiento fija solo valores, niveles y precedencia. Las historias que escriban o lean la configuración esperan al ADR. |
| R-GRD-8 | **Dos lecturas distintas de la misma configuración**: el motor ignora un nivel ilegible (supuesto de BR-CONS-007 (motor-local)) y Guardrails no puede hacerlo sin relajar reglas (BR-EDGE-004). | Media | Medio | Q-GRD-12 fija el comportamiento de Guardrails; alinear con el motor queda como dependencia para el Arquitecto o una revisión de motor-local (ver Decisiones heredadas). |
| R-GRD-9 | **Operación destructiva permitida sin snapshot**: la Time Machine no puede tomarlo y se pierde trabajo. | Baja | Crítico | Q-GRD-11: sin snapshot, la operación se deniega (BR-EDGE-005). |
| R-GRD-10 | **Un agente fabrica un cambio laxo en la rama principal y consigue que el humano lo confirme**: el agente altera la copia local de la rama principal y aprovecha una vía que no se detecta para dar la confirmación. | Baja | Alto | La confirmación muestra qué cambia y de dónde viene, se anuncia, se puede cancelar y queda auditada (Q-GRD-22). Se cierra cuando el factor de autenticación del sistema operativo se aplique también a estas confirmaciones (Q-GRD-19). Riesgo aceptado por Rene Bonilla (2026-10-04). |

## Decisiones tomadas

Rene Bonilla aceptó el 2026-10-03 las recomendaciones del PO para las 16 preguntas abiertas de esta feature. Cada decisión Q-GRD-n sale de la pregunta P-GRD-n con el mismo número. Q-GRD-17 y Q-GRD-18 son posteriores a la aprobación del requerimiento (2026-10-04). Q-GRD-19 a Q-GRD-27 también lo son: salen de la revisión de arquitectura y seguridad de Guardrails (decisiones D5 a D12 y KPI de Rene Bonilla, 2026-10-04). Q-GRD-32 sale de ADR-GRD-008 (OQ-GRD-008-8), con la misma autonomía. Q-GRD-28 a Q-GRD-31 salen de los resultados de SPIKE-GRD-001 en macOS: son decisiones del orquestador validadas por el Arquitecto y el PO bajo la autonomía que Rene Bonilla delegó el 2026-10-04; Rene no las confirmó una por una.

| # | Pregunta de origen | Decisión | Fecha | Decidido por | Reglas afectadas |
|---|--------------------|----------|-------|--------------|------------------|
| Q-GRD-1 | P-GRD-1: ¿a quién se aplican las reglas si el actor es "sin atribuir"? | A toda operación, sea cual sea el actor (fail-safe), porque un agente sin registrar es indistinguible del humano. El humano tiene una **excepción consciente** para saltarse una regla en una operación concreta, que queda registrada. Una acción que el humano confirma de forma explícita en una superficie de GitRaptor (p. ej. aprobar un merge en el Cockpit) cuenta como esa excepción. **Refinada por Q-GRD-24**: toda excepción, también esa aprobación, pasa por anuncio, ventana para cancelar y auditoría. | 2026-10-03 | Rene Bonilla | BR-AUTH-003, BR-AUTH-001, BR-VAL-003, BR-CONS-004 |
| Q-GRD-2 | P-GRD-2: ¿permisos iguales o distintos por agente? | MVP: la misma configuración para todos los agentes. Por agente, en una fase posterior. | 2026-10-03 | Rene Bonilla | BR-AUTH-003 — (alcance OUT: permisos por agente) |
| Q-GRD-3 | P-GRD-3: ¿instalación de hooks automática o explícita? | Explícita, con el modelo de permiso de BR-AUTH-002 (motor-local): qué, dónde, por qué, cómo se revierte; solo el humano concede; no se repregunta tras denegar. | 2026-10-03 | Rene Bonilla | BR-AUTH-002 |
| Q-GRD-4 | P-GRD-4: ¿repo con hooks propios u otro gestor? | Nunca reemplazarlos. Detectar e informar; encadenar solo con permiso. Si no se puede encadenar, Guardrails funciona solo con la capa MCP y lo avisa en el estado de protección. *Aclaración de redacción (Artifact Judge): el repo sigue sin la capa de hooks (Solo MCP o Sin protección, según la allowlist).* | 2026-10-03 | Rene Bonilla | BR-EDGE-002, BR-CONS-005, BR-WF-002 |
| Q-GRD-5 | P-GRD-5: ¿reglas en un repo sin configuración? | Un conjunto mínimo seguro: denegar force-push y denegar el borrado de la rama base. Visible para el desarrollador y desactivable por el equipo en su configuración. **Refinada por Q-GRD-20 y Q-GRD-21**: solo lo desactiva la configuración del equipo en la rama principal, y no se aplica hasta que el humano lo confirma en su máquina. | 2026-10-03 | Rene Bonilla | BR-EDGE-001, BR-VAL-001, BR-WF-002 |
| Q-GRD-6 | P-GRD-6: "pedir confirmación" en Git crudo y caducidad | Espera hasta un plazo; sin respuesta, se deniega (fail-safe). Plazo por defecto de **5 minutos**; al cumplirse, la petición pasa a caducada. | 2026-10-03 | Rene Bonilla | BR-TIME-001, BR-WF-001, BR-VAL-001 |
| Q-GRD-7 | P-GRD-7: ¿cómo se protege la configuración frente a los agentes? | Las rutas de la configuración de Guardrails son rutas prohibidas para los agentes por defecto. Los cambios a la configuración del equipo entran por commit revisado. | 2026-10-03 | Rene Bonilla | BR-AUTH-004, BR-AUTH-001, BR-VAL-001 |
| Q-GRD-8 | P-GRD-8: límite de la capa de hooks | Declararlo como límite del MVP y como riesgo (R-GRD-1). La mitigación completa es la Time Machine y, a futuro, integrar con los permisos nativos del agente (fuera del MVP). | 2026-10-03 | Rene Bonilla | BR-EDGE-003 — (alcance OUT: permisos nativos del agente) |
| Q-GRD-9 | P-GRD-9: ¿todas las políticas de BR-11 en el MVP? | Sí, priorizadas: primero ramas protegidas, force-push, `reset --hard` y rutas prohibidas; después tamaño de diff y formato de commit. | 2026-10-03 | Rene Bonilla | BR-VAL-003 |
| Q-GRD-10 | P-GRD-10: ¿dónde vive el registro y cuánto se conserva? | En el perfil de GitRaptor, separado por repo, como los datos del motor. Conservación de **90 días**. | 2026-10-03 | Rene Bonilla | BR-CONS-004, BR-TIME-002 |
| Q-GRD-11 | P-GRD-11: ¿y si la Time Machine no puede tomar el snapshot previo? | Se deniega con el motivo explicado (NFR-01, fail-safe). El humano puede usar una excepción consciente. | 2026-10-03 | Rene Bonilla | BR-EDGE-005 |
| Q-GRD-12 | P-GRD-12: ¿qué reglas aplican si un nivel no se puede leer? | Para permisos y políticas: avisar y aplicar el conjunto mínimo por defecto más lo legible de los demás niveles, nunca "todo permitido". Si el nivel ilegible es personal, solo se pierden sus endurecimientos. Alinear con el supuesto de BR-CONS-007 (motor-local), que ignora el nivel ilegible: queda como dependencia para el Arquitecto o una revisión de motor-local. **Ampliada por Q-GRD-26** (clave desconocida = nivel parcial, con el mínimo forzado). | 2026-10-03 | Rene Bonilla | BR-EDGE-004 |
| Q-GRD-13 | P-GRD-13: ¿se decide en la cola también desde la CLI? | Es decisión del Cockpit y de la CLI (presentación). Guardrails solo exige que quien decide sea el humano. | 2026-10-03 | Rene Bonilla | BR-WF-001, BR-AUTH-001 — (alcance OUT: presentación de la cola) |
| Q-GRD-14 | P-GRD-14: ¿un endurecimiento en el perfil prevalece sobre un "permitir" del equipo? | Sí. Se amplía la excepción de Q23 de motor-local: un nivel personal (perfil o local) puede endurecer cualquier regla del equipo y nunca relajarla. Motivo: cada persona puede ser más estricta con sus agentes en su máquina sin afectar al equipo. **Refina Q23 de motor-local.** | 2026-10-03 | Rene Bonilla | BR-CONS-001, BR-VAL-001 |
| Q-GRD-15 | P-GRD-15: ¿cuándo entra un repo en el alcance de Guardrails? | Al añadirlo a la observación del motor (BR-AUTH-001 (motor-local)), para que haya actor atribuido. La allowlist del MCP debería ser un subconjunto de los repos observados (a coordinar con F-001-05). Retirar un repo de la observación no desinstala sus hooks: siguen aplicando las reglas con actor "sin atribuir" y Guardrails avisa de ello. | 2026-10-03 | Rene Bonilla | BR-WF-002, BR-AUTH-002 |
| Q-GRD-16 | P-GRD-16: ¿y si el comando fija una rama base que no existe? | Avisar y pedir confirmación al humano; si confirma, guardarla (puede existir solo en el remoto o crearse después). El motor aplica Q42: indica que no puede calcular ahead/behind y no elige otra rama. | 2026-10-03 | Rene Bonilla | BR-CONS-003, BR-CONS-006 (comando, Guardrails), BR-CONS-006 (motor-local) (rama base inexistente, Q42) |
| Q-GRD-17 | P-GRD-17 (abierta en el índice de historias, 2026-10-04): ¿qué versión de la configuración del equipo es la efectiva? | La configuración del equipo que rige una operación es la última versión commiteada en el worktree donde ocurre esa operación; las ediciones sin commitear nunca cuentan. Junto con Q-GRD-7, que impide a los agentes commitear cambios en la configuración, un agente no puede relajarla. **Refinada por Q-GRD-20**: la versión commiteada en el worktree de la operación solo puede endurecer. | 2026-10-04 | Rene Bonilla | BR-VAL-001, BR-AUTH-004, BR-CONS-006, BR-EDGE-004 |
| Q-GRD-18 | Recomendación del PO tras Q-GRD-17 (2026-10-04): ¿de qué versión de la configuración del equipo se lee la rama base? | La rama base es una excepción a Q-GRD-17. Se lee de la configuración del equipo commiteada en la rama principal del repo, es decir, la que el remoto marca como principal, o `main` si no hay ninguna. Así hay un solo valor por repo, sin circularidad y coherente con BR-CONS-006 (motor-local) y Q24. **Refinada por Q-GRD-20** (qué copia de la rama principal) **y Q-GRD-21** (rama base confirmada). | 2026-10-04 | Rene Bonilla | BR-CONS-003, BR-EDGE-001, BR-VAL-001 |
| Q-GRD-19 | Revisión de seguridad de la arquitectura (2026-10-04, D5): ¿cómo se protegen en el MVP las acciones reservadas al humano, si no hay una confirmación que un agente no pueda dar? | **Riesgo aceptado en el MVP** para desinstalar la protección y para la excepción consciente: cada uso se anuncia, abre una ventana en la que se puede cancelar y queda auditado, con la aceptación del riesgo en cada acción. **Un factor de autenticación del sistema operativo, fuera del canal del agente, es requisito antes de US-GRD-013 (relajar con el comando de edición) y de US-GRD-015 (aprobar en la cola)**: sin él, esas acciones no se ofrecen. Mecanismo: ADR-GRD-008 (aceptado el 2026-10-04). | 2026-10-04 | Rene Bonilla | BR-AUTH-001; riesgo R-GRD-3 |
| Q-GRD-20 | Revisión de seguridad de la arquitectura (2026-10-04, D6): un agente puede relajar sus reglas con un commit fabricado en su worktree, sin pasar por la protección. ¿De dónde salen las relajaciones? | **Refina Q-GRD-17 y Q-GRD-18.** Lo que relaja (desactivar el conjunto mínimo, un "permitir" explícito, cualquier valor menos restrictivo que el valor por defecto) y la rama base se leen **solo** de la configuración del equipo commiteada en la rama principal. De la rama principal se usa la copia que el repo ya conoce del remoto, sin consultarlo; si no hay remoto, la rama local; si tampoco existe, aplican los valores por defecto. La versión commiteada en el worktree de la operación (Q-GRD-17) **solo puede endurecer**. Un endurecimiento que ya está en la rama principal rige en todos los worktrees, aunque no lo hayan integrado. | 2026-10-04 | Rene Bonilla | BR-VAL-001, BR-AUTH-004, BR-CONS-001, BR-CONS-003, BR-CONS-006, BR-EDGE-001 |
| Q-GRD-21 | Revisión de seguridad de la arquitectura (2026-10-04, D7): la copia de la rama principal también se puede alterar en local o recibir una configuración laxa. ¿Cuándo se aplica una relajación del equipo? | Toda relajación que llegue por un cambio de la configuración del equipo en la rama principal, incluidos desactivar el conjunto mínimo y cambiar la rama base, **no se aplica hasta que el humano la confirma en cada máquina** (acción reservada, BR-AUTH-001). Mientras tanto rige la combinación más restrictiva de la configuración confirmada y la nueva, y GitRaptor avisa. Un endurecimiento se aplica al momento. La confirmación inicial de la rama base y de la configuración del equipo la hace siempre el humano, al instalar la protección o de forma explícita (**precisado por Q-GRD-23**: nunca al añadir el repo a la observación). | 2026-10-04 | Rene Bonilla | BR-EDGE-001, BR-CONS-003, BR-VAL-001, BR-AUTH-001; BR-CONS-006 (motor-local) |
| Q-GRD-22 | Revisión de seguridad de la arquitectura (2026-10-04, D8): ¿con qué mecanismo se confirma un cambio de la configuración del equipo o de la rama base? | Con el **mismo mecanismo que las demás acciones reservadas del MVP** (Q-GRD-19): anuncio, ventana para cancelar, auditoría y aceptación del riesgo, mostrando qué cambia y de dónde viene. Cuando exista el factor de autenticación del sistema operativo se aplicará también aquí, pero **no bloquea** US-GRD-007 ni US-GRD-014, que aportan estas confirmaciones. | 2026-10-04 | Rene Bonilla | BR-AUTH-001; riesgo R-GRD-10 |
| Q-GRD-23 | Contradicción detectada por el PO al registrar Q-GRD-21 (2026-10-04, D9): ¿dónde se hace la confirmación inicial de la rama base y de la configuración del equipo? | **Al instalar la protección (US-GRD-001) o con una confirmación explícita (US-GRD-014)**, nunca en el flujo de añadir un repo del Motor local. Si la configuración del equipo trae relajaciones (por ejemplo, desactivar el conjunto mínimo), la confirmación inicial pasa por el anuncio y la ventana para cancelar de Q-GRD-19. Hasta que se confirma, Guardrails protege `main`, la rama principal y la rama base leída, y el motor marca la rama base como "no confirmada". | 2026-10-04 | Rene Bonilla | BR-AUTH-001, BR-CONS-003; BR-CONS-006 y BR-EDGE-007 (motor-local) |
| Q-GRD-24 | Contradicción detectada por el PO entre Q-GRD-1 y Q-GRD-19 (2026-10-04, D10): ¿la aprobación explícita en el Cockpit también pasa por la ventana? | Sí. **Toda** excepción consciente, incluida la aprobación explícita en el Cockpit u otra superficie de GitRaptor (Q-GRD-1), pasa por el mismo anuncio, ventana para cancelar y auditoría de Q-GRD-19. **Refina Q-GRD-1.** | 2026-10-04 | Rene Bonilla | BR-AUTH-001, BR-AUTH-003 |
| Q-GRD-25 | Hueco detectado por el PO en el estado de protección (2026-10-04, D11): ¿se ve lo que espera confirmación? | El estado de protección mantiene sus cuatro estados y añade dos **diagnósticos visibles**: "relajación pendiente de confirmar" y "rama base no confirmada o pendiente", cada uno con la acción para confirmar. | 2026-10-04 | Rene Bonilla | BR-WF-002 |
| Q-GRD-26 | Revisión de arquitectura (2026-10-04, D12): ¿qué pasa con una clave desconocida en los permisos o las políticas, por ejemplo una errata? | Ese nivel queda **parcial**: se aplica lo legible, se **fuerza el conjunto mínimo aunque el equipo lo hubiera desactivado** y se avisa. Una errata nunca relaja. **Amplía Q-GRD-12.** | 2026-10-04 | Rene Bonilla | BR-EDGE-004, BR-VAL-001 |
| Q-GRD-27 | Supuesto del Arquitecto en la revisión de arquitectura (2026-10-04): ¿qué cuenta el KPI "acciones peligrosas bloqueadas"? | Las operaciones denegadas, rechazadas y caducadas **verificadas por GitRaptor**. Las anotadas en modo degradado, que GitRaptor no pudo verificar, se muestran aparte y no entran por defecto; el desarrollador puede incluirlas de forma explícita. | 2026-10-04 | Rene Bonilla | BR-CONS-004 |
| Q-GRD-28 | SPIKE-GRD-001 (D10, D11): con el formato de refs reftable, renombrar la rama base no ejecuta ningún hook | En un repo que guarda sus ramas en formato reftable, **renombrar la rama base o renombrar otra rama sobre ella no se puede impedir con Git directo**. Guardrails se instala igual y protege todo lo demás. El límite se publica en la lista de ese repo y se dice al pedir el permiso. Lo mitiga la Time Machine; el motor ya indica cuando la rama base no existe (Q42 de motor-local). El aviso propio de Guardrails y la recuperación guiada quedan para después del MVP. **Excepción a Q-GRD-5 / BR-EDGE-001.** | 2026-10-04 | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO | BR-EDGE-001, BR-EDGE-003 |
| Q-GRD-29 | SPIKE-GRD-001 (§ 5.1): Git reformatea la entrada de la clave de hooks al restaurarla | Desinstalar deja las rutas operativas **como estaban** con un criterio semántico: la entrada que tocó Guardrails vuelve a su valor efectivo y a su nivel, y las demás entradas no cambian. Las únicas diferencias posibles son de formato, las introduce Git y se declaran | 2026-10-04 | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO | BR-CONS-005 |
| Q-GRD-30 | SPIKE-GRD-001 (§ 8): el coste se percibe por comando, no por evaluación | < 100 ms p95 por evaluación. Un comando habitual añade sus evaluaciones más ⚠️ ≤ 5 ms p95 por proceso de hook que no evalúa; techo visible ⚠️ ≤ 150 ms p95 por commit o cambio de rama habitual (**por confirmar por Rene Bonilla**). Las operaciones masivas tienen un coste proporcional al número de refs, declarado en la documentación y en la explicación del permiso | 2026-10-04 | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO | — (RNF "Decisión inmediata") |
| Q-GRD-31 | SPIKE-GRD-001 (F07, D21): la regla de forzado y la de ambigüedad deniegan de más en algunos casos | Se aceptan los falsos positivos hacia el lado seguro y se publican en un apartado propio de la lista, **"lo que se deniega de más"**: el push fast-forward desde un clon superficial y, en repos reftable, una rama que solo difiere de la base en mayúsculas. El motivo de la denegación lo explica, sin instrucciones para saltarse la protección | 2026-10-04 | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO | BR-EDGE-003 |
| Q-GRD-32 | ADR-GRD-008 (OQ-GRD-008-8): un agente puede quitar un endurecimiento personal editando a mano el perfil o la configuración local (R-GRD-4) | **Lleva la regla de Q-GRD-21 a los niveles personales.** Para los valores de Guardrails en los que los personales solo endurecen (permisos, ramas protegidas, tamaño de diff, rutas prohibidas, plazo de la cola): una edición a mano que **endurece** rige al instante. Una que **quita un endurecimiento** no rige hasta que el humano la confirma con el factor de autenticación del sistema operativo (ADR-GRD-008); mientras tanto rige lo más restrictivo y GitRaptor avisa con `personal-relax-pending`, con el valor anterior, el nuevo y el archivo de origen. Si una edición endurece y relaja, la parte que endurece rige al instante. **Sin factor, la relajación personal no rige**; la vía de escape es desinstalar la protección (Q-GRD-19) y volver a instalarla: la confirmación inicial (Q-GRD-23) muestra que relaja los niveles personales. Enmienda ADR-GRD-004 antes de la Dev Spec de US-GRD-013. | 2026-10-04 | Decisión del orquestador (2026-10-04), validada por Arquitecto/PO | BR-CONS-001, BR-AUTH-001, BR-AUTH-004; riesgo R-GRD-4 |

## Preguntas abiertas

Todas las preguntas de esta feature están resueltas salvo P-GRD-33 (ver [Decisiones tomadas](#decisiones-tomadas)). La tabla se conserva como historia.

| # | Pregunta | Recomendación del PO | Estado |
|---|----------|----------------------|------------|
| P-GRD-1 | ¿A quién se aplican las reglas, si el motor no distingue al humano de un agente sin registrar ("sin atribuir")? | A toda operación, sea cual sea el actor (fail-safe), porque un agente sin registrar es indistinguible del humano. El humano tiene una **excepción consciente** para saltarse una regla en una operación concreta, que queda registrada. Una acción que el humano confirma de forma explícita en una superficie de GitRaptor (p. ej. aprobar un merge en el Cockpit) cuenta como esa excepción. | Resuelta (Q-GRD-1) |
| P-GRD-2 | ¿Una sola configuración de permisos para todos los agentes o distinta por agente? | MVP: la misma para todos los agentes. Por agente, en una fase posterior. | Resuelta (Q-GRD-2) |
| P-GRD-3 | ¿La protección de hooks se instala sola al observar un repo o de forma explícita? | Explícita, con el modelo de permiso de BR-AUTH-002 (motor-local) (qué, dónde, por qué, cómo se revierte; solo el humano concede; no se repregunta tras denegar). | Resuelta (Q-GRD-3) |
| P-GRD-4 | ¿Qué pasa si el repo ya tiene hooks propios u otro gestor de hooks? | Nunca reemplazarlos. Detectar e informar; encadenar solo con permiso. Si no se puede encadenar, Guardrails funciona solo con la capa MCP y lo avisa en el estado de protección. | Resuelta (Q-GRD-4) |
| P-GRD-5 | ¿Qué reglas aplican en un repo sin configuración de Guardrails? | Un conjunto mínimo seguro: denegar force-push y denegar el borrado de la rama base. Visible para el desarrollador y desactivable por el equipo en su configuración. | Resuelta (Q-GRD-5) |
| P-GRD-6 | En la capa de hooks (Git crudo), "pedir confirmación", ¿espera la decisión o rechaza y se reintenta? ¿Cuándo caduca una petición? | Espera hasta un plazo; sin respuesta, se deniega (fail-safe). Plazo por defecto de 5 minutos; al cumplirse, la petición pasa a caducada. | Resuelta (Q-GRD-6) |
| P-GRD-7 | ¿Cómo se protege la propia configuración frente a un agente que la edita para relajarla? | Las rutas de la configuración de Guardrails son rutas prohibidas para los agentes por defecto. Los cambios a la configuración del equipo entran por commit revisado. | Resuelta (Q-GRD-7) |
| P-GRD-8 | La capa de hooks no cubre todo: hay operaciones destructivas que no disparan hooks y los hooks se pueden saltar a propósito. ¿Cómo se trata? | Declararlo como límite del MVP y como riesgo (R-GRD-1). La mitigación completa es la Time Machine y, a futuro, integrar con los permisos nativos del agente (fuera del MVP). | Resuelta (Q-GRD-8) |
| P-GRD-9 | ¿Entran en el MVP todas las políticas de BR-11? | Sí, priorizadas: primero ramas protegidas, force-push, `reset --hard` y rutas prohibidas; después tamaño de diff y formato de commit. | Resuelta (Q-GRD-9) |
| P-GRD-10 | ¿Dónde vive el registro de decisiones y cuánto se conserva? | En el perfil de GitRaptor, separado por repo, como los datos del motor (S-GRD-5). Conservación de 90 días. | Resuelta (Q-GRD-10) |
| P-GRD-11 | Si la Time Machine no puede tomar el snapshot previo de una operación destructiva permitida, ¿Guardrails la deja pasar? | No: se deniega con el motivo explicado (NFR-01, fail-safe). El humano puede usar una excepción consciente. | Resuelta (Q-GRD-11) |
| P-GRD-12 | Si un nivel de la configuración no se puede leer, ¿qué reglas aplican? | Para permisos y políticas: avisar y aplicar el conjunto mínimo por defecto más lo legible de los demás niveles, nunca "todo permitido". Si el nivel ilegible es personal, solo se pierden sus endurecimientos. Alinear con el supuesto de BR-CONS-007 (motor-local), que ignora el nivel ilegible. | Resuelta (Q-GRD-12) |
| P-GRD-13 | ¿Además del Cockpit, se puede decidir en la cola desde la CLI? | Decisión del Cockpit y de la CLI (presentación). Guardrails solo exige que quien decide sea el humano (BR-AUTH-001). | Resuelta (Q-GRD-13) |
| P-GRD-14 | **Refinamiento de Q23 de motor-local**: ¿un endurecimiento en el perfil prevalece sobre un "permitir" del equipo? Q23 deja que el equipo gane al perfil, con la única excepción de que un nivel personal no relaja una prohibición del equipo. | Sí. Ampliar la excepción de Q23: un nivel personal (perfil o local) puede endurecer cualquier regla del equipo y nunca relajarla. Motivo: cada persona puede ser más estricta con sus agentes en su máquina sin afectar al equipo. Alternativa fiel a Q23: el equipo gana al perfil siempre y solo la configuración local personal puede endurecer. | Resuelta (Q-GRD-14) |
| P-GRD-15 | ¿Cuándo entra un repo en el alcance de Guardrails: al ser observado por el motor o al estar en la allowlist del MCP (NFR-02)? | Al añadirlo a la observación del motor (BR-AUTH-001 (motor-local)), para que haya actor atribuido. La allowlist del MCP debería ser un subconjunto de los repos observados (a coordinar con F-001-05). Retirar un repo de la observación no desinstala sus hooks: siguen aplicando las reglas con actor "sin atribuir" y Guardrails avisa de ello. | Resuelta (Q-GRD-15) |
| P-GRD-16 | ¿Qué hace el comando de edición si se fija como rama base una rama que no existe en el repo? (relacionada con Q42 de motor-local) | Avisar y pedir confirmación al humano; si confirma, guardarla (puede existir solo en el remoto o crearse después). El motor aplica Q42: indica que no puede calcular ahead/behind y no elige otra rama. | Resuelta (Q-GRD-16) |
| P-GRD-33 | XP-30: con un repo que el Git del motor no puede leer (formato de refs reftable o versión más nueva que la del sistema), instalar la protección de hooks falla con un error interno y lo revierte todo; no se pierden datos, pero el desarrollador no sabe por qué. ¿Debe la instalación detectar que el Git del motor no puede leer el repo (formato de refs o versión) y mostrar un bloqueo claro con la acción (configurar el Git del motor), y debe el motor respetar `engine.gitPath`? | Sí a las dos. Comprobarlo antes de tocar nada; el estado de protección muestra el bloqueo y la acción. No cambia el límite publicado (Q-GRD-28). Que el motor use `engine.gitPath` depende de motor-local; lo resuelve el Arquitecto. Fuera del alcance de XP-30 | Abierta (2026-10-06; la registra el PO a petición del Arquitecto; decide Rene Bonilla; BR-CONS-005, BR-WF-002, BR-EDGE-004, NFR-01) |

---

## ✅ Quality Review (Auto-evaluación del Contexto)

> Revisión ejecutada el 2026-10-03 al terminar el documento (`methodology.md` § 7). Resultado: **7 ✅ · 7 ⚠️ · 0 🔴**. Las ⚠️ dependen sobre todo de preguntas abiertas (Q-GRD-1, Q-GRD-3 a Q-GRD-6, Q-GRD-10 a Q-GRD-12) y del ADR P8 (motor-local); ninguna impide que el Arquitecto empiece, pero varias historias no se podrán cerrar sin ellas.
>
> **Revisión 2026-10-03 (Artifact Judge, veredicto RESERVAS)**: (1) la precedencia "el perfil endurece sobre un 'permitir' del equipo" va más allá de Q23 de motor-local: se abre P-GRD-14 como refinamiento de Q23, se anota la diferencia en "Decisiones heredadas" y se marcan las filas afectadas de BR-CONS-001; "Configuración en tres niveles" pasa de ✅ a ⚠️. (2) BR-AUTH-001 deja de atribuir a Q27 una frase que no está en la fuente. (3) Convención de IDs: preguntas, supuestos y riesgos propios pasan a P-GRD-n, S-GRD-n y R-GRD-n; los IDs del Motor local se califican con "(motor-local)". (4) Nueva regla BR-CONS-006: el comando de edición no pisa cambios hechos a mano y escribe de forma recuperable (NFR-01). (5) BR-WF-002 completa estados y transiciones, se alinea con BR-EDGE-001 y abre P-GRD-15 (alcance). (6) US-GRP-016 la desbloquean el valor de equipo, la regla de lectura y el ADR P8 (motor-local); el comando no es prerrequisito; se abre P-GRD-16. (7) El glosario deja de dar nombres de claves. (8) R-GRD-1 justifica su diferencia con el BRD. Resultado: **7 ✅ · 8 ⚠️ · 0 🔴**.
>
> **Revisión 2026-10-03 (Q-GRD-1 a Q-GRD-16)**: Rene Bonilla aceptó las recomendaciones de las 16 preguntas P-GRD; se registran como decisiones Q-GRD-1 a Q-GRD-16 (tabla [Decisiones tomadas](#decisiones-tomadas)) y se llevan a las reglas. Q-GRD-14 refina Q23 de motor-local; el roce de Q-GRD-12 con BR-CONS-007 (motor-local) queda como dependencia para el Arquitecto o una revisión de motor-local. S-GRD-1, S-GRD-4 y S-GRD-5 quedan confirmados por Q-GRD-14, Q-GRD-7 y Q-GRD-10; el resto de supuestos sigue sin validar. Pasan a ✅ Usuarios/Actores, Integraciones y Configuración en tres niveles. Known Risks 3, 6 y 8 resueltos; el 7 se reduce a S-GRD-8 y S-GRD-9; los 1, 2, 4, 5 y 7 siguen pendientes de aceptación. Resultado: **10 ✅ · 5 ⚠️ · 0 🔴**.
>
> **Revisión 2026-10-04 (Q-GRD-17)**: decisión **posterior a la aprobación del requerimiento**. Sale de la pregunta P-GRD-17, que el Artifact Judge abrió al revisar las historias. Fija que la configuración del equipo que rige una operación es la última versión commiteada en el worktree de esa operación y cierra la vía por la que un agente podía relajarla sin commitear. Se lleva a BR-VAL-001, BR-AUTH-004, BR-CONS-006 y BR-EDGE-004. No cambia ninguna calificación: **10 ✅ · 5 ⚠️ · 0 🔴**.
>
> **Revisión 2026-10-04 (Q-GRD-18)**: decisión **posterior a la aprobación del requerimiento**. La rama base es una excepción a Q-GRD-17: se lee de la configuración del equipo commiteada en la rama principal del repo (la que el remoto marca como principal, o `main`). Todos los worktrees comparten la misma rama base, sea cual sea su commit. Se lleva a BR-CONS-003, BR-EDGE-001 y BR-VAL-001, y a "Qué entrega Guardrails al Motor local". No cambia ninguna calificación: **10 ✅ · 5 ⚠️ · 0 🔴**.
>
> **Revisión 2026-10-04 (cierre de pendientes)**: Rene Bonilla aceptó los Known Risks 1, 2, 4, 5 y 7 y confirmó los supuestos S-GRD-2, S-GRD-3, S-GRD-6, S-GRD-7, S-GRD-8 y S-GRD-9, además de la latencia de evaluación (< 100 ms), Conventional Commits como formato mínimo y la falta de línea base del ROI y del KPI "no estorbar". Los Known Risks 3, 6 y 8 siguen resueltos. La viabilidad técnica de S-GRD-6 (los hooks cubren todos los worktrees) sigue siendo comprobación del Arquitecto. Pasan a ✅ RNFs y Cola de confirmación; Valor Esperado / ROI y KPIs siguen ⚠️ como riesgos aceptados, y Restricciones sigue ⚠️ por la política de ASSA y el ADR P8 (motor-local). Resultado: **12 ✅ · 3 ⚠️ · 0 🔴**.
>
> **Revisión 2026-10-04 (Q-GRD-19 a Q-GRD-22 y KPI verificado)**: decisiones **posteriores a la aprobación del requerimiento**, tomadas por Rene Bonilla en la revisión de arquitectura y seguridad de Guardrails (D5 a D8). Q-GRD-20 refina Q-GRD-17 y Q-GRD-18: las relajaciones y la rama base salen solo de la configuración del equipo en la rama principal (la copia que el repo conoce del remoto) y el worktree solo endurece. Q-GRD-21: toda relajación que llegue a la rama principal, incluidos desactivar el mínimo y cambiar la rama base, espera la confirmación del humano en cada máquina; la confirmación inicial es siempre del humano. Q-GRD-22: esa confirmación usa el mecanismo MVP de las acciones reservadas. Q-GRD-19: riesgo aceptado en el MVP y factor de autenticación del sistema operativo antes de US-GRD-013 y US-GRD-015. El KPI "acciones peligrosas bloqueadas" cuenta solo lo verificado; lo anotado en modo degradado va aparte. Se llevan a BR-VAL-001, BR-AUTH-001, BR-AUTH-004, BR-CONS-001, BR-CONS-003, BR-CONS-004, BR-CONS-006 y BR-EDGE-001; Q-GRD-20 y Q-GRD-21 refinan además Q5 y Q12 de motor-local (Decisiones heredadas) y pasan a BR-CONS-006 (motor-local) y US-GRP-016. Nuevo riesgo R-GRD-10 (relajación fabricada y confirmada), aceptado. El glosario añade tres términos. No cambia ninguna calificación: **12 ✅ · 3 ⚠️ · 0 🔴**.
>
> **Revisión 2026-10-04 (Q-GRD-23 a Q-GRD-27)**: decisiones **posteriores a la aprobación del requerimiento**, de Rene Bonilla, que resuelven las tres contradicciones que el PO señaló al registrar Q-GRD-19 a Q-GRD-22 y una más de la arquitectura. Q-GRD-23: la confirmación inicial se hace al instalar la protección o de forma explícita, nunca al añadir el repo (se quita esa vía de BR-AUTH-001, BR-CONS-003, Q-GRD-21, BR-EDGE-007 (motor-local) y US-GRP-016). Q-GRD-24 refina Q-GRD-1: toda excepción, también la aprobada en el Cockpit, pasa por anuncio, ventana y auditoría. Q-GRD-25: diagnósticos visibles en el estado de protección, sin estados nuevos. Q-GRD-26 amplía Q-GRD-12: una errata deja el nivel parcial y fuerza el mínimo. Q-GRD-27 da ID a la decisión del KPI verificado. Las instalaciones huérfanas (retirar o adoptar) se asignan a US-GRD-003 (BR-CONS-005). No cambia ninguna calificación: **12 ✅ · 3 ⚠️ · 0 🔴**.

> **Revisión 2026-10-04 (Q-GRD-28 a Q-GRD-31, SPIKE-GRD-001)**: resultados del spike en macOS. Q-GRD-28: el renombrado de la rama base en repos reftable se declara como límite y es una excepción del mínimo seguro. Q-GRD-29: desinstalación con criterio semántico. Q-GRD-30: objetivo por comando (techo por confirmar por Rene Bonilla). Q-GRD-31: apartado "lo que se deniega de más". Decisiones del orquestador validadas por el Arquitecto y el PO. No cambia ninguna calificación: **12 ✅ · 3 ⚠️ · 0 🔴**.

> **Revisión 2026-10-04 (ADR-GRD-008 y Q-GRD-32)**: se acepta ADR-GRD-008, el factor de autenticación del sistema operativo que pedía Q-GRD-19, con sus nueve preguntas resueltas por el orquestador y validadas por el Arquitecto y el PO. US-GRD-013 y US-GRD-015 se desbloquean (su Dev Spec espera a SPIKE-GRD-002). Q-GRD-32 cierra R-GRD-4 en los niveles personales. R-GRD-3 cita el mecanismo. Pendiente del PO: extender la regla de Q-GRD-21 a los niveles personales en `business-rules.md`, la historia de la relajación personal pendiente y la de adopción del factor por las confirmaciones de D8, desinstalar y la excepción (OQ-GRD-008-3).

| Sección | Resultado | Nota |
|---------|-----------|------|
| Problema de Negocio | ✅ | Específico (BRD P3, § 3.3, riesgo de Git crudo), con impacto concreto: trabajo perdido y rama base rota. |
| Valor Esperado / ROI | ⚠️ | Sin línea base de incidentes; ROI solo cualitativo. |
| KPIs de Éxito | ⚠️ | Cuatro metas cuantificadas (0 por la capa MCP, 100% de lo interceptable, 0 hooks perdidos, ≥ 5 repos). El KPI principal del BRD ("acciones peligrosas bloqueadas") se mide sin meta, y "no estorbar" no tiene meta. |
| Usuarios/Actores | ✅ | Roles diferenciados y acciones reservadas al humano explícitas. A quién se aplican las reglas con actor "sin atribuir" está decidido (Q-GRD-1). Cómo se distingue al humano es del Arquitecto (riesgo R-GRD-3), no del negocio. |
| Alcance OUT of scope | ✅ | 13 exclusiones, cada una con su feature dueña, su fase o su decisión. |
| Restricciones | ⚠️ | NFR-01, NFR-02 y NFR-07 aterrizados; límite de la capa de hooks declarado. Siguen abiertas la política interna de ASSA (heredada) y el formato P8 (motor-local). |
| RNFs | ✅ | Fail-safe, cero pérdida y explicabilidad definidos. La latencia de evaluación (< 100 ms) la confirmó Rene Bonilla (2026-10-04). |
| Integraciones | ✅ | Integraciones internas claras. Convivencia con hooks previos (Q-GRD-4) y snapshot imposible (Q-GRD-11) decididas. Queda la dependencia con BR-CONS-007 (motor-local) para el Arquitecto (Q-GRD-12). |
| Glosario | ✅ | 26 términos (2026-10-04: rama principal, rama base confirmada y relajación pendiente), incluidos permiso, política, decisión, niveles admitidos, endurecer/relajar, excepción consciente y estado de protección. |
| Stakeholders | ✅ | Rene Bonilla y equipos piloto con expectativas explícitas. |
| Configuración en tres niveles | ✅ | Valores, niveles admitidos y precedencia fijados (BR-VAL-001, BR-CONS-001). Q-GRD-14 refina Q23 de motor-local y está anotada en Decisiones heredadas. El formato queda en P8 (motor-local), fuera de este documento. S-GRD-2 (formato de commit) sigue como supuesto. El comando de edición tiene garantía NFR-01 (BR-CONS-006 (comando, Guardrails)). |
| Entrega al Motor local (US-GRP-016) | ✅ | Los tres desbloqueantes (valor de equipo, regla de lectura, ADR P8 (motor-local)) están explícitos; el comando no es prerrequisito. |
| Estado de protección y alcance | ✅ | Estados y transiciones completos (BR-WF-002), alineados con BR-EDGE-001. Alcance decidido (Q-GRD-15). |
| Cola de confirmación (BR-13) | ✅ | Ciclo de vida, quién decide y plazo de 5 minutos decididos (BR-WF-001, Q-GRD-6). S-GRD-8 (una aprobación vale una vez) y S-GRD-9 ("pedir confirmación" como "denegar" mientras no exista la cola) confirmados por Rene Bonilla (2026-10-04). |
| Instalación de hooks | ✅ | Recuperabilidad y conservación de hooks previos fijadas (BR-CONS-005). Permiso explícito (Q-GRD-3) y convivencia con otros gestores (Q-GRD-4) decididos. La cobertura de todos los worktrees sigue como supuesto (S-GRD-6) a validar con el Arquitecto. |

## ⚠️ Known Risks (from Quality Review)

| # | Sección | Riesgo | Impacto | Aceptado por |
|---|---------|--------|---------|--------------|
| 1 | Valor Esperado / ROI | Sin línea base de incidentes por acciones destructivas de agentes. | No se podrá demostrar la reducción; el valor se juzga por los bloqueos medidos. | Rene Bonilla (2026-10-04) |
| 2 | KPIs de Éxito | "Acciones peligrosas bloqueadas" y "no estorbar" sin meta. | No hay umbral de éxito ni de exceso de bloqueo hasta tener datos de dogfooding. | Rene Bonilla (2026-10-04) |
| 3 | Usuarios/Actores | ~~A quién se aplican las reglas con actor "sin atribuir" (P-GRD-1).~~ | **Resuelto** por Q-GRD-1 (2026-10-03). Cómo se distingue al humano sigue como riesgo R-GRD-3, para el Arquitecto. | — |
| 4 | Restricciones | Formato de la configuración pendiente (ADR P8 (motor-local)). | Las historias que escriben o leen la configuración, y US-GRP-016, no se cierran hasta el ADR (R-GRD-7). | Rene Bonilla (2026-10-04) |
| 5 | RNFs | Latencia de evaluación sin valor del BRD. | El Arquitecto no tiene umbral firme; se usa el supuesto de < 100 ms. | Rene Bonilla (2026-10-04) |
| 6 | Integraciones | ~~Convivencia con hooks previos (P-GRD-4); snapshot imposible (P-GRD-11); forma del permiso de instalación (P-GRD-3).~~ | **Resuelto** por Q-GRD-4, Q-GRD-11 y Q-GRD-3 (2026-10-03). | — |
| 7 | Cola de confirmación | Plazo y caducidad resueltos por Q-GRD-6 (5 minutos). Siguen sin validar S-GRD-8 (una aprobación vale una sola vez) y S-GRD-9 ("pedir confirmación" como "denegar" mientras no exista la cola). | Los escenarios de BR-WF-001 y BR-VAL-002 sobre esos dos puntos quedan provisionales. | Rene Bonilla (2026-10-04) |
| 8 | Configuración en tres niveles / alcance | ~~Precedencia perfil frente a equipo (P-GRD-14) y criterio de alcance (P-GRD-15) sin decidir.~~ | **Resuelto** por Q-GRD-14 y Q-GRD-15 (2026-10-03). | — |
