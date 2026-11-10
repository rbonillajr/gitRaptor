---
id: CTX-WTL-001
title: "Contexto — Ciclo de vida del worktree"
type: context
status: draft
created: 2026-11-10
updated: 2026-11-10
domain: GRP
epic: E-001
feature: worktree-lifecycle
scope: feature
stakeholders:
  - rene-bonilla
related:
  business:
    - BRD-GRP-001
  research:
    - RES-GRP-COMP-2026-10
  context:
    - CTX-CKP-001
    - CTX-TMC-001
    - CTX-GRD-001
    - CTX-MCP-001
    - CTX-DSK-001
  adrs:
    - ADR-CKP-002
    - ADR-GRD-001
    - ADR-GRP-006
    - ADR-GRP-012
    - ADR-GRP-015
    - ADR-GRP-017
    - ADR-MCP-001
    - ADR-TMC-001
    - ADR-TMC-007
  rules:
    - BR-WTL-001
    - BR-WTL-002
    - BR-WTL-003
    - BR-WTL-004
    - BR-WTL-005
    - BR-WTL-006
    - BR-WTL-007
    - BR-WTL-008
    - BR-WTL-009
    - BR-WTL-010
    - BR-WTL-011
    - BR-WTL-012
    - BR-WTL-013
    - BR-WTL-014
    - BR-WTL-015
    - BR-WTL-016
tags:
  - worktrees
  - ciclo-de-vida
  - copy-on-write
  - worktreeinclude
  - semilla
  - descarte-reversible
  - estado-del-agente
  - claude-code
  - convivencia
---

# Contexto del Feature: Ciclo de vida del worktree

> **Fundamento**: enfoque híbrido BRD-PRD en lenguaje de negocio. Lo técnico lo define el Arquitecto a partir de este documento. Entradas: diario de ideas del 2026-11-08 (worktrunk y la app de Codex), prueba de copy-on-write del 2026-11-09 con su diario de medición, y decisión de superficie para agentes del 2026-11-10 (`bitacora/ideas/`).

---

## 1. Visión General

La feature cubre el ciclo completo del worktree de un agente: crearlo con su rama y sus artefactos de compilación listos, lanzar el agente en él, saber en qué estado está ese agente, devolverlo o traerlo a la copia del desarrollador y descartarlo sin perder nada. Hoy GitRaptor ya prepara un worktree (US-CKP-018, US-MCP-019) y lo descarta con Deshacer (US-CKP-017); esta feature completa BR-07 con el paso que falta para que el desarrollador sume un agente a la flota con una sola orden, y extiende la red de seguridad a los worktrees que el propio agente crea.

worktrunk y la app de Codex ya resuelven crear, copiar ignorados y limpiar; GitKraken lanza agentes en su worktree y muestra su estado. Ninguno de los tres hace reversible el descarte de un worktree con una red de seguridad común a todos los agentes, ni aplica políticas ni predice conflictos entre agentes de distintos proveedores. La feature pone cada paso del ciclo de vida bajo la Time Machine, Guardrails y el cockpit, que son las capacidades que ya diferencian a GitRaptor (BRD § 3.3).

### Problema de Negocio

- **Crear un worktree de agente cuesta disco y pasos manuales.** Un worktree compilado en frío ocupaba 30–50 GB antes del perfil ligero de #328; con ese perfil, un `target/` completo ocupa ~10 GiB (medido el 2026-11-09). Con 4–5 agentes a la vez, el disco se llena: el 2026-11-08 ocurrió en el dogfooding. Hoy el orquestador crea el worktree con Orca, copia los artefactos con worktrunk (`wt step copy-ignored`) y lanza el agente a mano.
- **Copiar artefactos de un worktree vivo anula el ahorro.** Clonar desde el worktree de un worker que seguía compilando hizo bajar el disco libre de 106 a 53 GiB en una hora, aunque cada clon costó 1–1,5 GB al copiarse (prueba 3, 2026-11-09). El ahorro solo se sostiene si el origen está quieto.
- **Copiar ignorados sin límite filtra estado de otro agente.** Sin un `.worktreeinclude` acotado, la copia incluiría `.claude/runs`, el estado de `/implement` de otro worker.
- **Descartar un worktree en las herramientas vecinas no tiene vuelta atrás.** GitKraken advierte que «Remove worktree and delete branch» borra la rama aunque sus commits no estén integrados y que no se puede deshacer. worktrunk no guarda snapshot al borrar.
- **El estado real del agente no se ve.** Para saber si un agente espera una decisión, espera el CI o se colgó, el orquestador de GitRaptor usa scripts propios (supervisor y watchdog). El motor distingue hoy activo, inactivo y terminado (ADR-GRP-012).
- **Los worktrees que crea Claude Code por su cuenta quedan fuera de GitRaptor.** Cuando el agente crea o borra un worktree, no pasa por la creación con artefactos ni por el descarte reversible.

Sin la feature, GitRaptor queda por detrás de worktrunk, Codex y GitKraken en la parte más visible del flujo con agentes (crear y lanzar), y el usuario sigue juntando tres herramientas para un ciclo que GitRaptor ya protege a medias.

### Valor Esperado

- ROI estimado: `sin dato`. El ahorro medido es de disco, no de tiempo: la primera compilación tras clonar desde la semilla recompiló solo los crates del workspace (3,7 s en una prueba, solo `gitraptor-desktop`), y con sccache el tiempo total por worker cambia en minutos (prueba 2, 2026-11-09).
- KPIs de éxito: ver § 3. Líneas base medidas en el diario del 2026-11-09; las metas las fija Rene (Preguntas abiertas 1 y 2).
- Beneficiarios: el desarrollador orquestador que lanza varios agentes a la vez; el desarrollador novato de la app de escritorio (D7), que no va a usar la terminal para crear un worktree; el orquestador del propio proyecto, que retira sus scripts de estado.

---

## 2. Dominio Específico

- Tipo de funcionalidad: acciones sobre el repo (crear, descartar, traer y devolver worktrees) y estado en vivo en el cockpit, en la CLI, la TUI y la app de escritorio.
- Usuarios principales: desarrollador orquestador, desarrollador novato de la app, el agente de IA (Claude Code en el MVP, D2).
- Casos de uso principales:
  - El desarrollador crea un worktree con rama y artefactos listos y lanza Claude Code con una tarea en una sola orden.
  - El desarrollador descarta el worktree de un agente terminado y ve cuánto disco recuperó; si se equivocó, lo restaura.
  - El desarrollador ve qué agentes esperan su decisión, cuáles esperan el CI y cuáles están colgados, y atiende primero esos.
  - Claude Code crea o borra un worktree por su cuenta y GitRaptor lo registra, lo protege y lo deja deshacer.
  - El desarrollador trae a su copia el trabajo de un agente para probarlo en su editor y luego lo devuelve (M4).
- Alcance:
  - IN scope (M3, Must):
    - `raptor new <rama> --agent claude -- "tarea"`: worktree con rama con nombre, copia copy-on-write de los ignorados desde una semilla quieta según `.worktreeinclude`, lanzamiento de Claude Code con la tarea e instalación de la skill de la superficie para agentes (feature mcp, trabajo W160; aún sin historia con id). En la app de escritorio, la misma acción como «Nuevo agente» (alcance de la app según CTX-DSK-001).
    - Descarte reversible: snapshot de la Time Machine antes de borrar, protección de los worktrees con agente vivo e informe del disco recuperado.
    - Estado del agente en el cockpit: activo, esperando una decisión humana, esperando el CI, colgado o terminado, con los que piden atención arriba.
    - Worktrees que crea y borra Claude Code (`WorktreeCreate` y `WorktreeRemove`): registrados como del agente, con snapshot al borrarlos y bajo Guardrails.
    - Convivencia: reconocer los worktrees de wt, Codex, GitKraken y Orca sin modificarlos; los de HEAD separado se muestran como «ajeno, no tocar».
  - IN scope (M4, Should, v0.1.0):
    - Semilla mantenida por el daemon: se reconstruye en un momento tranquilo cuando `main` avanza, una por toolchain y perfil de compilación.
    - Handoff «Traer a mi copia» y «Devolver», con snapshot, en la CLI y en la app.
    - Limpieza de artefactos de compilación por política del repo o del perfil (como cargo-sweep o equivalente).
    - Hooks de setup al crear un worktree, definidos y aprobados por el humano.
  - Después de v0.1.0 (Could):
    - Integrar en local (`land`) con predicción de conflictos, políticas y snapshot.
    - Resumen de CI y PR por rama (con BR-17 y BR-19).
    - Puerto por worktree.
  - OUT of scope:
    - HEAD separado como modo de creación: la atribución y las políticas de GitRaptor se apoyan en ramas con nombre.
    - Hooks de setup con shell libre que un agente o el MCP puedan disparar (NFR-02).
    - Lanzar Codex, Cursor u otros agentes desde `raptor new` antes de que tengan soporte completo (D2); para ellos se crea el worktree y se muestra la orden de lanzamiento, como en US-CKP-018.
    - Gestionar el modelo, los prompts posteriores o la conversación del agente (BRD § 6.4).
    - Lanzar un agente por el MCP o a petición de otro agente (NFR-02; revisable más adelante).
    - Leer en M3 un CI distinto de GitHub Actions, o lanzar, reintentar o cancelar ejecuciones de CI.
    - Mensajes de commit generados con un LLM y atajos tipo `pr:123`.
    - Copiar ignorados desde un worktree con trabajo en curso.
    - Una herramienta MCP nueva que no necesite el estado vivo del daemon ni una acción protegida (decisión del 2026-11-10).

---

## 3. Objetivos de Negocio

> Una métrica lleva valor y fuente. Sin fuente, el valor se escribe `sin línea base` y el punto pasa a Preguntas abiertas.

| Objetivo | Métrica de Éxito | Fuente / línea base | Prioridad |
|----------|------------------|---------------------|-----------|
| Crear un worktree de agente sin gastar disco en dependencias | Disco real que baja al crear el worktree con su copia; meta `sin dato` (Pregunta 1) | Medido 2026-11-09: clon desde semilla quieta en 7–10 s y entre -1002 MB y +72 MB de `df` en 25 clones | Alta |
| Que el disco de un worktree crezca solo por lo que diverge | Disco liberado al borrar un worktree tras una historia completa; meta `sin dato` (Pregunta 1) | Medido 2026-11-09: entre 1,8 y 24,3 GB reales liberados al borrar; lo máximo compartible por worker es ~10 GiB con el perfil de #328 | Alta |
| Que ningún descarte pierda trabajo | Descartes restaurables dentro de la retención: 100 %; incidentes de pérdida de datos: 0 | NFR-01; BRD § 9 | Alta |
| Atención del humano donde hace falta | Agentes que esperan decisión, CI o están colgados visibles en la zona de atención del cockpit; los scripts de estado del orquestador dejan de hacer falta | Necesidad del orquestador (supervisor y watchdog en `bitacora/orquestador/`); `sin línea base` de tiempo perdido (Pregunta 2) | Alta |
| Ningún worktree de agente fuera de la red de seguridad | Worktrees creados por Claude Code que quedan registrados y con snapshot al borrarse: 100 % de los que pasan por sus eventos de creación y borrado | Decisión del coordinador, 2026-11-10 | Alta |
| Dogfooding | GitRaptor crea los worktrees de los workers del propio proyecto en lugar de Orca más worktrunk | BRD § 9 (100 % de las sesiones con agentes); `sin línea base` de adopción de esta feature | Media |

---

## 4. Stakeholders y Actores

### Stakeholders de Negocio

| Stakeholder | Interés | Expectativa |
|-------------|---------|-------------|
| Rene Bonilla (dueño del producto) | Ser competitivo frente a worktrunk, la app de Codex y GitKraken | Paridad en crear y lanzar, y diferencial en descartar sin miedo, estado del agente y protección común |
| Orquestador del proyecto (dogfooding) | Dejar de juntar Orca, worktrunk y scripts propios | Una sola orden para crear el worktree de un worker y un estado fiable de cada agente |

### Actores del Sistema

| Actor | Descripción | Permisos/Capacidades |
|-------|-------------|----------------------|
| Desarrollador orquestador | Supervisa varios agentes desde la terminal o la TUI | Crea worktrees y lanza Claude Code; descarta y restaura; aprueba los hooks de setup (M4); trae y devuelve trabajo (M4); define la política de limpieza (M4) |
| Desarrollador novato | Usa la app de escritorio (D7) | Las mismas acciones desde la app, sin terminal; las restricciones de autorización son las mismas que en la CLI (CTX-DSK-001) |
| Agente de IA (Claude Code) | Trabaja en su worktree; puede crear y borrar worktrees propios | Sus worktrees quedan registrados y bajo Guardrails; no lanza agentes ni dispara hooks de setup por el MCP; usa la skill y la CLI con salida JSON |
| Otro agente (Codex, Cursor) | Sin soporte completo en el MVP (D2) | Se reconoce por registro explícito; para él, `raptor new` crea el worktree y muestra la orden para lanzarlo |
| Herramienta vecina (wt, app de Codex, GitKraken, Orca) | Crea y borra worktrees en el mismo repo | Sus worktrees se reconocen y se muestran; GitRaptor no modifica su configuración; los de HEAD separado no se tocan |

---

## 5. Restricciones

### Regulatorias y compliance
- No hay restricciones regulatorias específicas: la feature opera en la máquina del desarrollador sobre sus propios repos. Aplican las políticas internas del producto: cero pérdida de datos (NFR-01) y seguridad del MCP (NFR-02).
- La licencia del núcleo (FSL-1.1-ALv2, D4) cubre esta feature: forma parte de Cockpit y Time Machine individuales.

### De negocio
- Hitos: Must en M3 (beta multiplataforma), Should en M4 (v0.1.0) y Could después de v0.1.0. Decisión del coordinador por delegación de Rene, 2026-11-10. Las fechas de M3 y M4 siguen por recalcular en el plan de releases.
- Solo Claude Code tiene soporte completo en el MVP; Codex y luego Cursor después, uno por uno (D2).
- La app de escritorio ofrece estas acciones como cliente más del motor, sin vía de escritura propia (D7, BR-27).

### Técnicas impuestas por el negocio
> Solo lo que el negocio mismo impone. La solución técnica la define el Arquitecto.

- Toda operación que modifique un repo es recuperable y los tests usan repos temporales, nunca un repo real (NFR-01).
- El MCP no ejecuta shell, valida las entradas y opera solo sobre repos de la allowlist (NFR-02). Ningún hook de setup se dispara desde el MCP.
- Compatibilidad con `.worktreeinclude`, el formato que ya usan worktrunk y la app de Codex, para convivir con ellos.
- Toda herramienta MCP nueva entra solo si necesita el estado vivo del daemon o es una acción protegida; el resto va a la CLI con salida JSON estable y a la skill (decisión del 2026-11-10).
- GitRaptor no escribe fuera de su perfil sin permiso del humano (Q17 y Q22 de motor-local, recogidas en ADR-GRP-012). Interceptar los eventos de worktree de Claude Code toca su configuración: lo activa el humano, solo registra, guarda el snapshot y aplica Guardrails, y nunca concede permisos (decisión del 2026-11-10). Cómo se hace de forma reversible es transversal (lo define el Arquitecto, como enmienda de ADR-GRP-012).
- GitRaptor lanza un agente solo a petición explícita del humano, nunca por el MCP ni a petición de otro agente (decisión del 2026-11-10; enmienda de BRD § 6.4 pendiente de aplicar).
- Plataforma: macOS verificado primero. Linux y Windows, y el comportamiento sin copy-on-write, quedan para la etapa de validación multiplataforma.

---

## 6. Expectativas de servicio (no funcionales, en lenguaje de negocio)

> Lo que el negocio espera del servicio, dicho como lo diría quien lo usa. Cifras, SLA y cómo se logran los define el Arquitecto.

| Expectativa | Qué espera el negocio | Fuente |
|-------------|-----------------------|--------|
| Tiempos de espera | Crear un worktree con su copia no tarda más que lo medido con worktrunk desde una semilla quieta (7–10 s en el repo de GitRaptor) | Diario de medición, 2026-11-09 |
| Espacio en disco | Un worktree nuevo no duplica las dependencias de compilación; el informe del descarte dice el espacio que el disco gana de verdad | Pruebas 2 y 3 y diario, 2026-11-09 |
| Recuperación | Un worktree descartado se restaura con su rama y su trabajo sin commitear durante la retención de la Time Machine (30 días por defecto, D-TMC-15) | NFR-01; CTX-TMC-001 |
| Fiabilidad del estado | Un agente nunca aparece activo si está colgado ni terminado si sigue vivo; pasa a «colgado» tras 15 min sin actividad del agente ni del repo y sin CI en curso, configurable | Decisión del coordinador, 2026-11-10 |
| Seguridad y privacidad de datos | La copia no lleva a un worktree el estado ni los secretos de otro agente que el repo no haya listado; ningún agente dispara un hook de setup ni lanza otro agente por el MCP | Prueba del 2026-11-09 (`.claude/runs`); NFR-02 |
| Consumo del daemon | Mantener la semilla (M4) no compite con el trabajo de los agentes: se hace en un momento tranquilo; qué es «tranquilo» es `sin dato` (Pregunta 6) | Decisión del coordinador, 2026-11-10; ADR-GRP-015 |

---

## 7. Sistemas e integraciones que el negocio impone

| Sistema | Para qué (negocio) | Fuente |
|---------|--------------------|--------|
| Claude Code | Lanzarlo con la tarea en el worktree nuevo, registrar los worktrees que crea y borra, y conocer su estado | D2; decisión del coordinador, 2026-11-10 |
| Skill de GitRaptor para agentes (feature mcp, W160) | Que el agente lanzado sepa trabajar con GitRaptor desde el primer momento | Decisión del 2026-11-10 (superficie lean) |
| Time Machine | Snapshot antes de descartar, traer o devolver, y restauración | NFR-01; CTX-TMC-001 |
| Guardrails | Aplicar las políticas del repo a los worktrees del agente | BR-11; CTX-GRD-001 |
| GitHub Actions (solo lectura) | Saber si un agente espera el resultado del CI; en M3 no se lee otro CI | Decisión del coordinador, 2026-11-10 |
| `.worktreeinclude` de worktrunk y la app de Codex | Mismo archivo para decidir qué ignorados se copian | Convivencia con wt y Codex |

---

## 8. Reglas de negocio

> Las historias escriben el valor en su escenario y enlazan la regla solo en su frontmatter.

### BR-WTL-001 — Rama con nombre
Todo worktree que crea GitRaptor sale con una rama con nombre; nunca con HEAD separado. Si la rama pedida ya está sacada en otro worktree, la creación se rechaza y dice en qué worktree está.

### BR-WTL-002 — Copia acotada por `.worktreeinclude`
Solo se copian al worktree nuevo los ignorados que lista el `.worktreeinclude` del repo. Sin `.worktreeinclude`, no se copia ningún ignorado y el worktree se crea igual.

### BR-WTL-003 — Origen quieto de la copia
La copia sale de una semilla del mismo toolchain y perfil de compilación en la que nadie trabaja; nunca de un worktree con trabajo en curso ni con una sesión de agente. En M3, la primera vez que se usa `raptor new` en un repo crea la semilla desde `main`, y el humano la reconstruye cuando quiere con `raptor seed refresh`; en M4 la mantiene el daemon (BR-WTL-012).

### BR-WTL-004 — Quién lanza el agente
GitRaptor lanza un agente una sola vez, al crear el worktree, y solo a petición explícita del humano: `raptor new` en la CLI o el botón «Nuevo agente» en la app. Nunca lo lanza a petición de un agente ni por el MCP. En el MVP solo se lanza Claude Code (D2); con cualquier otro agente se crea el worktree y se muestra la orden para lanzarlo. Preparar un worktree sin lanzar ningún agente sigue disponible (US-CKP-018).

### BR-WTL-005 — Skill disponible al lanzar
Al lanzar Claude Code en un worktree nuevo, la skill de GitRaptor para agentes queda disponible en ese worktree. Quitarla deja el worktree como estaba antes de instalarla.

### BR-WTL-006 — Descarte reversible
Antes de borrar un worktree y su rama, GitRaptor guarda un snapshot de la Time Machine con la rama y el trabajo sin commitear. Se restaura durante la retención de la Time Machine (30 días por defecto). Los archivos ignorados, artefactos de compilación incluidos, no se guardan (D-TMC-16) y la confirmación lo dice antes de borrar.

### BR-WTL-007 — Agente vivo protegido
Un worktree con una sesión de agente activa, esperando una decisión, esperando el CI o colgada no se descarta: la petición se rechaza y dice qué agente lo ocupa.

### BR-WTL-008 — Disco recuperado real
Al terminar un descarte, GitRaptor informa el espacio que ganó el disco, no la suma del tamaño de los archivos borrados, porque los bloques compartidos con la semilla no se liberan.

### BR-WTL-009 — Estados del agente
Cada sesión de agente está en uno de cinco estados: activo, esperando una decisión humana, esperando el CI, colgado o terminado. Las sesiones que esperan una decisión o están colgadas suben a la zona de atención del cockpit. Una sesión está colgada cuando pasan 15 minutos sin actividad del agente ni del repo y sin una ejecución de CI en curso para su rama; los 15 minutos son configurables. En M3, «esperando el CI» solo lee GitHub Actions y en solo lectura: GitRaptor no lanza, reintenta ni cancela ejecuciones.

### BR-WTL-010 — Worktrees creados por el agente
Un worktree que Claude Code crea por su cuenta queda registrado como de ese agente y cumple BR-WTL-001 y BR-WTL-002. Cuando Claude Code lo borra, el borrado sigue BR-WTL-006 y BR-WTL-007. Las operaciones de Git en él pasan por Guardrails como cualquier otro worktree. Este registro solo sirve para registrar, guardar el snapshot y aplicar Guardrails; nunca concede un permiso al agente. Lo activa el humano. Si el registro falla, un borrado pedido por el agente no se ejecuta sin snapshot: se rechaza.

### BR-WTL-011 — Convivencia con worktrees ajenos
Los worktrees que crean wt, la app de Codex, GitKraken, Orca o el propio usuario se muestran con su origen cuando se puede saber. Uno con HEAD separado se marca «ajeno, no tocar»: GitRaptor no lo descarta, no lo limpia y no copia hacia él. GitRaptor no modifica la configuración de esas herramientas.

### BR-WTL-012 — Semilla mantenida (M4)
GitRaptor mantiene una semilla por toolchain y perfil de compilación. Cuando la rama base avanza, la reconstruye en un momento tranquilo; mientras se reconstruye, nadie copia de ella.

### BR-WTL-013 — Traer a mi copia y Devolver (M4)
Traer el trabajo de un agente a la copia del desarrollador, o devolverlo, guarda antes un snapshot de los dos lados y se puede deshacer.

### BR-WTL-014 — Limpieza de artefactos por política (M4)
La limpieza automática solo borra artefactos de compilación ignorados de worktrees creados por GitRaptor, según la política del repo o del perfil. Nunca borra archivos rastreados ni trabajo sin commitear, ni toca worktrees ajenos.

### BR-WTL-015 — Hooks de setup aprobados (M4)
Un hook de setup lo define el humano y no se ejecuta hasta que el humano lo aprueba. Nunca lo dispara el MCP ni un agente.

### BR-WTL-016 — Herramientas MCP de la feature
Una acción de esta feature se ofrece como herramienta MCP solo si necesita el estado vivo del daemon o es una acción protegida; las demás se ofrecen por la CLI con salida JSON y por la skill.

---

## Panorama Competitivo

> Cada fila con su fuente y la fecha de consulta. Lo que la fuente no confirma va como `?` o `[POR VERIFICAR]`.

| Producto | Qué hace respecto a este feature | Fuente | Consultado |
|----------|----------------------------------|--------|------------|
| worktrunk (`wt`) | CLI en Rust (MIT o Apache-2.0, ~12,4k estrellas). Crea worktree y rama con `wt switch -c`, lanza el agente con `-x`, comparte `target/` y `node_modules/` con copy-on-write en APFS, btrfs y XFS. `wt merge` hace squash, rebase, fast-forward y borra worktree y rama. No documenta undo, políticas, MCP ni interfaz gráfica | https://github.com/max-sixty/worktrunk | 2026-11-10 |
| worktrunk, plugin de Claude Code | Sus hooks llevan `WorktreeCreate` y `WorktreeRemove` de Claude Code a `wt switch --create` y `wt remove`; marcas de actividad «trabajando» y «esperando» en `wt list` (pueden quedar obsoletas si el agente muere); instala una skill `/worktrunk` | https://worktrunk.dev/claude-code/ | 2026-11-10 |
| App de Codex (OpenAI) | Crea el worktree desde la rama elegida con HEAD separado por defecto; scripts de setup opcionales; Handoff entre Local y Worktree; copia ignorados según `.worktreeinclude`; conserva los 15 worktrees más recientes; no borra los de un chat fijado o en curso; guarda un snapshot antes de borrar y ofrece restaurarlo | https://learn.chatgpt.com/docs/environments/git-worktrees | 2026-11-10 |
| GitKraken Desktop | Cada sesión de agente en su rama con nombre y su worktree; comandos de setup por repo antes de lanzar el agente (no documenta aprobación); lanza Claude Code, Codex CLI, Copilot CLI, Cursor CLI, Gemini CLI y OpenCode; estado en vivo (Running, Awaiting approval, Needs response, Waiting for input, Error) para cuatro de ellos | https://help.gitkraken.com/gitkraken-desktop/agents/ | 2026-11-10 |
| GitKraken Desktop, descarte | «Remove worktree and delete branch» borra worktree, carpeta y rama local aunque sus commits no estén integrados, y la página dice que no se puede deshacer; pide confirmación extra con cambios sin commitear | https://help.gitkraken.com/gitkraken-desktop/review-and-undo-agent-changes/ | 2026-11-10 |
| GitKraken, copia de ignorados | La documentación de agentes no dice si copia ignorados ni si lee `.worktreeinclude` `[POR VERIFICAR]` | https://help.gitkraken.com/gitkraken-desktop/agents/ | 2026-11-10 |

## Diferencial frente a worktrunk, Codex y GitKraken

Cada fila cita las fuentes del Panorama con su fecha de consulta. ✔ sí · ≈ parcial · ✗ no · ? no verificado. Las celdas de GitRaptor son compromisos de esta feature y del backlog (pre-MVP).

| Capacidad | worktrunk | App de Codex | GitKraken Desktop | GitRaptor | Fuente (consultado 2026-11-10) |
|-----------|-----------|--------------|-------------------|-----------|--------------------------------|
| Crear worktree y lanzar el agente en un paso | ✔ | ✔ | ✔ | ✔ M3 (Claude Code) | https://github.com/max-sixty/worktrunk · https://learn.chatgpt.com/docs/environments/git-worktrees · https://help.gitkraken.com/gitkraken-desktop/agents/, 2026-11-10 |
| Rama con nombre por defecto | ✔ | ✗ (HEAD separado) | ✔ | ✔ M3 | https://github.com/max-sixty/worktrunk · https://learn.chatgpt.com/docs/environments/git-worktrees · https://help.gitkraken.com/gitkraken-desktop/agents/, 2026-11-10 |
| Copia de ignorados con copy-on-write | ✔ | ≈ (copia `.worktreeinclude`; copy-on-write ?) | ? `[POR VERIFICAR]` | ✔ M3, desde semilla quieta | https://github.com/max-sixty/worktrunk · https://learn.chatgpt.com/docs/environments/git-worktrees, 2026-11-10 |
| Semilla quieta mantenida sola | ✗ (no documentado) | ✗ (no documentado) | ? | ✔ M4 | https://github.com/max-sixty/worktrunk · https://learn.chatgpt.com/docs/environments/git-worktrees, 2026-11-10 |
| Descartar con snapshot y restaurar | ✗ (no documentado) | ✔ (worktrees que gestiona Codex) | ✗ (la ayuda dice que no se deshace) | ✔ M3, también los del agente | https://learn.chatgpt.com/docs/environments/git-worktrees · https://help.gitkraken.com/gitkraken-desktop/review-and-undo-agent-changes/, 2026-11-10 |
| Estado «esperando al humano» | ≈ (marca «esperando») | ? | ✔ | ✔ M3 | https://worktrunk.dev/claude-code/ · https://help.gitkraken.com/gitkraken-desktop/agents/, 2026-11-10 |
| Estado «esperando el CI» y «colgado» | ✗ (no documentado) | ? | ≈ (Error; colgado ?) | ✔ M3 | https://worktrunk.dev/claude-code/ · https://help.gitkraken.com/gitkraken-desktop/agents/, 2026-11-10 |
| Worktrees creados por Claude Code bajo la herramienta | ✔ (sin snapshot) | — | ? | ✔ M3, con snapshot y Guardrails | https://worktrunk.dev/claude-code/, 2026-11-10 |
| Coordinación entre agentes de distintos proveedores | ≈ (lista común, sin coordinación) | ✗ (solo Codex) | ≈ (vista común; coordinación ?) | ✔ (flota común; soporte completo de Claude Code en el MVP, D2) | https://help.gitkraken.com/gitkraken-desktop/agents/ · BRD-GRP-001 § 3.2 (RES-GRP-COMP-2026-10, consultado 2026-10-05) |
| Predicción de conflictos entre agentes, con lo sin commitear | ≈ (simula merge contra `main`) | ✗ (no documentado) | ≈ (solo commiteado, de pago, según RES-GRP-COMP-2026-10) | ✔ (BR-05) | https://github.com/max-sixty/worktrunk · BRD-GRP-001 § 3.2, consultado 2026-10-05 |
| Políticas comunes a todos los agentes | ✗ | ✗ | ✗ (la ayuda solo da avisos y confirmaciones) | ✔ (Guardrails) | https://help.gitkraken.com/gitkraken-desktop/review-and-undo-agent-changes/, 2026-11-10 |
| Undo universal (Git crudo y sin commitear) | ✗ | ≈ (snapshot solo al borrar) | ≈ (última acción en su app, según RES-GRP-COMP-2026-10) | ✔ (Time Machine) | https://learn.chatgpt.com/docs/environments/git-worktrees · BRD-GRP-001 § 3.2, consultado 2026-10-05 |
| Hooks de setup aprobados por el humano | ✔ (aprobación la primera vez, según el diario del 2026-11-08 ⚠️ ASSUMPTION, la página no lo confirma) | ? | ✗ (no documenta aprobación) | ✔ M4 | https://worktrunk.dev/claude-code/ · https://help.gitkraken.com/gitkraken-desktop/agents/, 2026-11-10 |

**Lectura:** en crear y lanzar, GitRaptor llega a paridad. El diferencial está en tres filas que ninguno de los tres cubre juntas: descartar con snapshot también lo que crea el agente, políticas comunes y undo universal; y en la predicción de conflictos con lo sin commitear, que ya es de GitRaptor (BR-05). GitKraken ya muestra «esperando al humano» (hallazgo del 2026-11-10 en su ayuda de agentes), así que ese estado es paridad. El diferencial declarado del estado del agente es «esperando el CI», «colgado» y una vista común de agentes de varios proveedores con las mismas protecciones (decisión del coordinador, 2026-11-10).

**Vigencia:** revisar antes de 2027-02-10. GitKraken publica cada mes; pasada esa fecha, el análisis se vuelve a comprobar antes de citarlo.

---

## 9. Glosario del Dominio

| Término | Definición | Sinónimos/Notas |
|---------|------------|-----------------|
| Semilla | Worktree en la rama base en el que nadie trabaja, con los artefactos de compilación de un toolchain y un perfil, del que se copian los ignorados a los worktrees nuevos | `seed-main` en el dogfooding |
| Copy-on-write | Copia que comparte los bloques del disco con el origen hasta que uno de los dos lados escribe | reflink; APFS, btrfs, XFS, ReFS |
| `.worktreeinclude` | Archivo del repo que lista qué ignorados se copian a un worktree nuevo | Formato de worktrunk y la app de Codex |
| Descartar | Borrar un worktree y su rama dejando un snapshot restaurable | US-CKP-017 |
| Handoff | Mover el trabajo de un agente entre su worktree y la copia del desarrollador | «Traer a mi copia» / «Devolver» |
| Worktree ajeno | Worktree que no creó GitRaptor (wt, Codex, GitKraken, Orca, a mano) | Con HEAD separado: «ajeno, no tocar» |
| Zona de atención | Parte superior de la lista del cockpit con lo que pide al humano | US-CKP-002 |
| Colgado | Sesión de agente viva sin actividad del agente ni del repo durante 15 min (configurable) y sin CI en curso | Distinto de «inactivo» de ADR-GRP-012 |
| Toolchain y perfil | Versión del compilador y perfil de compilación con los que se generaron los artefactos; si cambian, los artefactos copiados no sirven | Prueba 1 del 2026-11-09 |

---

## Dependencias con otras features

| Dependencia | Qué aporta | Estado |
|-------------|------------|--------|
| US-CKP-018 y US-MCP-019 | Crear worktree sobre la base confirmada | Existen; US-CKP-018 se mantiene para preparar sin lanzar ningún agente |
| US-CKP-017 | Descartar con Deshacer | Existe; esta feature añade la protección del agente vivo, el informe de disco y los worktrees del agente |
| ADR-GRP-012 | Detección de sesiones de Claude Code: activo, inactivo, terminado | Aceptado; los estados nuevos de BR-WTL-009 y la interceptación de BR-WTL-010 lo amplían. Enmienda pendiente, a cargo del Arquitecto |
| BRD-GRP-001 § 6.4 | Fuera de alcance «lanzar prompts» | Enmienda pendiente de aplicar: GitRaptor puede lanzar un agente una vez, a petición explícita del humano |
| Superficie para agentes (feature mcp, W160) | Skill de GitRaptor y `raptor configure-agents` | En backlog, sin historia con id |
| Time Machine (ADR-TMC-001, ADR-TMC-007) | Snapshot, retención de 30 días, ignorados fuera | Aceptados |

---

## Preguntas abiertas

> Todo valor sin fuente se registra aquí. El supuesto provisional vive solo en esta tabla.

| # | Pregunta | Quién responde | Supuesto provisional | ¿Bloquea historias? (sí/no) |
|---|----------|----------------|----------------------|-----------------------------|
| 1 | ¿Qué meta de disco se fija para crear y para mantener un worktree de agente? | Rene | [POR VERIFICAR] Sin meta numérica; se reporta el `df` medido como en el diario del 2026-11-09 | no |
| 2 | ¿Cuánto tiempo pierde hoy el orquestador en detectar agentes que esperan o se colgaron? | Rene / orquestador | [POR VERIFICAR] `sin línea base`; se mide al retirar los scripts | no |
| 3 | ¿Qué CI se lee en M3? | Coordinador | Resuelta 2026-11-10: solo GitHub Actions, en solo lectura (Decisiones del humano) | no |
| 4 | ¿Cuándo pasa un agente a «colgado»? | Coordinador | Resuelta 2026-11-10: 15 min sin actividad del agente ni del repo y sin CI en curso, configurable (BR-WTL-009) | no |
| 5 | En M3, antes de que el daemon mantenga la semilla (M4), ¿quién la crea y la reconstruye? | Coordinador | Resuelta 2026-11-10: la crea `raptor new` la primera vez desde `main`; se reconstruye con `raptor seed refresh` (BR-WTL-003) | no |
| 5b | Si la semilla existe pero no es del toolchain o el perfil actuales (por ejemplo, tras cambiar de versión del compilador), ¿`raptor new` la reconstruye, crea el worktree sin copia o pide `raptor seed refresh`? | Rene | [POR VERIFICAR] Crea el worktree sin copia y avisa de que hace falta `raptor seed refresh` | no |
| 6 | ¿Qué es un «momento tranquilo» para reconstruir la semilla (sin agentes activos, cierre del día, carga baja)? | Rene + Arquitecto | [POR VERIFICAR] `sin dato` | no (M4) |
| 7 | ¿Se enmienda BRD § 6.4 para que GitRaptor pueda lanzar el agente? | Coordinador | Resuelta 2026-11-10: sí, una vez y a petición explícita del humano; US-CKP-018 se mantiene sin lanzar. Enmienda del BRD pendiente de aplicar | no |
| 8 | ¿Puede un agente lanzar otro por el MCP? | Coordinador | Resuelta 2026-11-10: no (NFR-02); se puede revisar más adelante | no |
| 9 | ¿Dónde vive la semilla por toolchain y perfil (perfil de GitRaptor, junto al repo, carpeta de worktrees) y cuánto disco puede ocupar? | Arquitecto, con el visto bueno de Rene | [POR VERIFICAR] `sin dato` | no |
| 10 | ¿Hay un límite de worktrees retenidos o descartados restaurables (la app de Codex guarda 15), aparte de los 30 días de la Time Machine? | Rene | [POR VERIFICAR] Solo la retención de 30 días de la Time Machine | no |
| 11 | Copy-on-write en APFS (macOS) está medido. ¿Cómo se comporta en btrfs y XFS (Linux), en ReFS (Windows) y sin reflink (ext4, NTFS)? ¿Sin reflink se copia, se omite o se pregunta? Pendiente: etapa de validación multiplataforma | Arquitecto; validación en máquinas reales | [POR VERIFICAR] Sin reflink, no se copia y se avisa | no (M3 macOS) |
| 12 | ¿Se aceptan los hooks de Claude Code para interceptar `WorktreeCreate` y `WorktreeRemove`, que ADR-GRP-012 descartó para detectar sesiones? | Coordinador | Resuelta 2026-11-10: sí, con otro uso que la detección: solo registrar, snapshot y Guardrails, nunca conceder permisos; los configura el humano; si fallan, el borrado se rechaza. Enmienda de ADR-GRP-012 pendiente, a cargo del Arquitecto | no |
| 13 | ¿Se permite descartar un worktree con agente vivo si el humano lo fuerza, o siempre hay que terminar antes el agente? | Rene | [POR VERIFICAR] Siempre hay que terminarlo antes (BR-WTL-007, sin excepción) | no |
| 14 | ¿Se reutiliza una rama que ya existe y no está sacada en ningún worktree, o `raptor new` exige rama nueva? | Rene | [POR VERIFICAR] Se reutiliza, como `wt switch` | no |
| 15 | Un ignorado copiado por `.worktreeinclude` (por ejemplo `.env`) no entra en el snapshot (D-TMC-16). ¿Basta con que la confirmación lo avise, o algún ignorado debe guardarse? | Rene | [POR VERIFICAR] Basta el aviso: el original sigue en la semilla o en el repo principal | no |
| 16 | ¿Hay ignorados que nunca se copian aunque el repo los liste (estado de otros agentes como `.claude/runs`)? | Rene + Arquitecto | [POR VERIFICAR] Sí, una lista fija de exclusiones | no |
| 17 | ¿Un hook de setup aprobado vuelve a pedir aprobación cuando cambia su contenido? | Rene | [POR VERIFICAR] Sí | no (M4) |

---

## Decisiones del humano

| Decisión | Valor | Quién | Fecha |
|----------|-------|-------|-------|
| Prioridad Must, M3 | `raptor new <rama> --agent claude -- "tarea"` con rama con nombre, copia copy-on-write desde semilla compatible con `.worktreeinclude`, lanzamiento del agente e instalación de la skill de la superficie para agentes | Coordinador, por delegación de Rene | 2026-11-10 |
| Prioridad Must, M3 | Descarte reversible con snapshot, protección del agente vivo e informe de disco recuperado | Coordinador, por delegación de Rene | 2026-11-10 |
| Prioridad Must, M3 | Estado del agente: activo, esperando decisión humana, esperando el CI, colgado, terminado | Coordinador, por delegación de Rene | 2026-11-10 |
| Prioridad Must, M3 | Interceptar `WorktreeCreate` y `WorktreeRemove` de Claude Code: registrados, con snapshot y bajo Guardrails | Coordinador, por delegación de Rene | 2026-11-10 |
| Prioridad Should, M4 | Semilla mantenida por el daemon (por toolchain y perfil); handoff con snapshot en CLI y app; limpieza de artefactos por política; hooks de setup aprobados por el humano, nunca disparados por el MCP | Coordinador, por delegación de Rene | 2026-11-10 |
| Prioridad Could, después de v0.1.0 | `land` con predicción, políticas y snapshot; resumen de CI y PR por rama; puerto por worktree | Coordinador, por delegación de Rene | 2026-11-10 |
| No copiar | HEAD separado por defecto; hooks con shell libre que un agente pueda disparar | Rene (diario del 2026-11-08) | 2026-11-08 |
| Superficie para agentes | MCP para estado vivo y seguridad; CLI con salida JSON y skill para el resto; sin code mode | Rene | 2026-11-10 |
| Copiar desde semilla quieta | La función gestiona una semilla quieta por toolchain y perfil; nunca copia desde worktrees vivos | Hallazgo de la prueba 3, adoptado por el coordinador | 2026-11-09 |
| Lanzar el agente (Pregunta 7) | Se enmienda BRD § 6.4: GitRaptor puede lanzar un agente una vez, a petición explícita del humano (`raptor new`, botón «Nuevo agente»); nunca a petición de un agente ni por el MCP. US-CKP-018 se mantiene para preparar sin lanzar. La enmienda del BRD queda pendiente de aplicar | Coordinador, por delegación de Rene | 2026-11-10 |
| Interceptar eventos de worktree de Claude Code (Pregunta 12) | Es un uso distinto de la detección que ADR-GRP-012 descartó: solo registrar el worktree, snapshot y Guardrails, nunca conceder permisos; lo configura el humano; si el hook falla, el agente no queda sin protección y el borrado se rechaza. Va como enmienda de ADR-GRP-012, a cargo del Arquitecto, pendiente | Coordinador, por delegación de Rene | 2026-11-10 |
| CI en M3 (Pregunta 3) | Solo GitHub Actions, leído con `gh`, en solo lectura | Coordinador, por delegación de Rene | 2026-11-10 |
| Umbral de «colgado» (Pregunta 4) | Sin actividad del agente ni del repo durante 15 min y sin CI en curso; configurable | Coordinador, por delegación de Rene | 2026-11-10 |
| Semilla en M3 (Pregunta 5) | La crea `raptor new` la primera vez desde `main`; se reconstruye con `raptor seed refresh` explícito; en M4 la mantiene el daemon | Coordinador, por delegación de Rene | 2026-11-10 |
| Agente que lanza otro agente (Pregunta 8) | No se permite por el MCP (NFR-02); revisable más adelante | Coordinador, por delegación de Rene | 2026-11-10 |
| Diferencial frente a GitKraken | GitKraken ya muestra «esperando al humano»: ese estado es paridad. El diferencial declarado del estado del agente es «esperando el CI», «colgado» y la vista común de agentes de varios proveedores | Coordinador, por delegación de Rene | 2026-11-10 |

---

## Riesgos conocidos

| # | Sección | Riesgo | Impacto | Aceptado por |
|---|---------|--------|---------|--------------|
| 1 | Alcance | Lanzar el agente queda decidido, pero la enmienda de BRD § 6.4 sigue sin aplicar | Hasta aplicarla, el BRD y esta feature dicen cosas distintas | Coordinador, 2026-11-10 (enmienda pendiente) |
| 2 | Reglas | «Esperando el CI» lee GitHub Actions en M3 aunque el resumen de CI por rama sea Could | Una parte pequeña de BR-19 (solo lectura del estado del CI) entra en M3; los repos con otro CI no ven ese estado | Coordinador, 2026-11-10 |
| 3 | Restricciones | Interceptar los eventos de Claude Code necesita la enmienda de ADR-GRP-012, a cargo del Arquitecto y aún pendiente | BR-WTL-010 no se puede diseñar hasta que exista la enmienda | Coordinador, 2026-11-10 (enmienda pendiente) |
| 4 | Valor | El ahorro es de disco y depende del perfil de compilación: con #328 lo compartible baja a ~10 GiB por worker; un repo con dependencias ligeras apenas ahorra | El argumento de venta pierde fuerza fuera de repos con compilación pesada | Pendiente de Rene |
| 5 | Expectativas | Copy-on-write solo está medido en APFS; en Linux y Windows sin reflink la copia ocupa el tamaño completo (Pregunta 11) | La promesa de disco no vale en todas las plataformas de M3 | Pendiente: etapa de validación multiplataforma |
| 6 | Competencia | GitKraken publica cada mes y podría añadir snapshot al descartar o políticas | El diferencial de descarte reversible se reduce | Vigilancia trimestral (BRD § 10) |
| 7 | Reglas | Un worktree de Codex con HEAD separado no se puede descartar desde GitRaptor (BR-WTL-011) | El usuario de Codex limpia esos worktrees con Codex | Pendiente de Rene |

---

## Revisión de calidad

| Sección | Nivel | Observación | Riesgo aguas abajo |
|---------|-------|-------------|--------------------|
| Problema de Negocio | Completo | Seis problemas con evidencia medida y fecha | — |
| Valor Esperado / ROI | Vago | ROI `sin dato`; el valor es disco medido, no dinero | No se puede priorizar contra otras features por retorno; aceptable para un producto interno |
| KPIs de éxito | Vago | Líneas base medidas; metas numéricas `sin dato` (Preguntas 1 y 2) | El QA no puede fijar un umbral de aceptación de disco; los escenarios usarán «no duplica dependencias» |
| Usuarios/Actores | Completo | Cinco actores con capacidades distintas, incluido el agente y las herramientas vecinas | — |
| Alcance OUT of scope | Completo | Siete exclusiones explícitas | — |
| Restricciones | Completo | Las dos contradicciones están decididas (Preguntas 7 y 12); quedan pendientes de aplicar la enmienda de BRD § 6.4 y la de ADR-GRP-012 | La historia de los worktrees de Claude Code espera la enmienda de ADR-GRP-012 para su diseño técnico |
| Expectativas de servicio | Vago en una fila | «Momento tranquilo» sin definir (Pregunta 6, M4); «colgado» ya tiene umbral | Solo afecta a M4 |
| Sistemas e integraciones | Completo | CI acotado a GitHub Actions en solo lectura | — |
| Reglas de negocio | Completo | 16 reglas con valores concretos; los umbrales sin fuente quedan como `sin dato` y van a Preguntas | — |
| Panorama Competitivo | Completo, con huecos marcados | Cada fila con URL y fecha; GitKraken con `?` y `[POR VERIFICAR]` donde la ayuda no lo confirma; una fila de worktrunk marcada ⚠️ ASSUMPTION | Revisar antes de 2027-02-10 |
| Glosario | Completo | Nueve términos | — |

**Vaguedad que bloquea historias:** ninguna desde las decisiones del 2026-11-10 (Preguntas 3, 4, 5, 7, 8 y 12 resueltas). Las que siguen abiertas tienen supuesto provisional y no bloquean.

**Riesgos aguas abajo:**
- **Arquitecto:** tiene a su cargo la enmienda de ADR-GRP-012 para interceptar los eventos de Claude Code, incluido el rechazo del borrado si el registro falla; también decide dónde vive la semilla (Pregunta 9) y cómo se mide el umbral de 15 min.
- **UX:** la confirmación del descarte debe decir que los ignorados no se guardan (BR-WTL-006) y la app necesita la misma protección del agente vivo; sin la Pregunta 13 cerrada no se sabe si hay botón de forzar.
- **QA:** las pruebas de copy-on-write solo se pueden certificar en macOS hasta la etapa de validación multiplataforma.
- **SM:** cuatro Must en M3 sobre una base que también tiene pendientes los XP Must; las fechas de M3 están por recalcular.
