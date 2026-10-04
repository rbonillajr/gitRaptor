---
id: ADR-GRP-012
title: Detección de sesiones de Claude Code sin hooks propios
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-009, ADR-GRP-010, ADR-GRP-013, SPIKE-GRP-001]
tags: [deteccion, atribucion, claude-code, sesiones, procesos, transcripts, privacidad, nfr-08, seguridad]
---

# ADR-GRP-012 — Detección de sesiones de Claude Code sin hooks propios

## Contexto

El motor tiene que saber qué sesión de Claude Code trabaja en cada worktree, en qué estado está y qué eventos le pertenecen (US-GRP-007), sin atribuirle nunca el trabajo que el desarrollador hace en su editor (US-GRP-008, BR-EDGE-004). Claude Code es el único agente con soporte completo en el MVP (Q32).

Restricciones:

- **Sin hooks propios** (Q22): el motor no instala hooks de Git ni de Claude Code, y no toca `~/.claude/settings.json`. Fuera del repo solo escribe en su perfil (Q17, Q21).
- **Sin APIs privadas** (NFR-08).
- **Solo dos valores de actor**: "agente X" con su origen, o "sin atribuir". Nunca "humano" (Q34, Q35).
- **Meta de precisión**: 90% de sesiones detectadas correctamente en dogfooding (Q9).
- **Estados**: activo, inactivo y terminado según el umbral de inactividad (BR-WF-001, BR-TIME-001; perfil o configuración local, 5 min por defecto, Q24). Una sesión terminada no se reactiva (Q41).
- **Instalación tardía**: Claude Code instalado después se detecta sin reconfigurar GitRaptor (BR-EDGE-006, Q29).
- **Límite clave**: el SO no informa de qué proceso escribió un archivo. La ubicación sola no distingue a Claude Code del editor humano abierto en el mismo worktree (R2).

**Decisión de producto de Rene Bonilla (2026-10-03, PQ-2)**: se permite leer `~/.claude/projects` limitado a **metadatos** (herramienta, ruta de archivo, marca de tiempo, id de sesión y cwd). Nunca se leen prompts, respuestas ni contenido de código. La lectura va detrás de una comprobación de versión y forma del formato que desactiva la señal si no lo reconoce.

## Decisión

Se adopta la **opción 3 del outline**: detección por proceso y cwd, con atribución por evidencia positiva y un adaptador de transcripts versionado y degradable.

### Señales

| Señal | Qué observa | Papel | Peso |
|---|---|---|---|
| **S1 · Proceso y cwd** | Procesos de Claude Code y su cwd, con `sysinfo` configurado con un `ProcessRefreshKind` que **no carga `cmd` ni `environ`** (SEC-04). **Identificación**: por la **ruta del ejecutable**. Solo si el ejecutable es un intérprete (`node`), una **lectura acotada de argv[1]** (el script de entrada) con una API por SO que no carga el resto de argv ni `environ`, con tope de bytes, y que se descarta en cuanto se clasifica el proceso. La viabilidad por SO la mide SPIKE-GRP-001 | **Existencia y fin de la sesión.** La identidad de la sesión es `(pid, hora de inicio)` para evitar la reutilización de PID | Necesaria. Sin S1 no hay sesión detectada |
| **S2a · mtime de transcripts** | mtime de `~/.claude/projects/<cwd-codificado>/*.jsonl` | Correlaciona el proceso con su transcript y desempata dos sesiones del mismo worktree | Auxiliar. **Nunca atribuye un cambio por sí sola** |
| **S2b · Metadatos de transcripts** | Por registro: herramienta, ruta de archivo, marca de tiempo, id de sesión y cwd | **Atribución por archivo.** Una escritura del worktree cuya ruta coincide con una herramienta de edición de esa sesión dentro de la ventana Δ | Evidencia positiva. Se desactiva sola si no reconoce el formato |
| **S3 · Ascendencia de procesos** | Árbol de procesos (y la marca de entorno heredada, si el SO deja leerla) de los `git` vivos al detectar un evento de Git | **Atribución de commits y operaciones de Git**. También responde PQ-6: si un llamante del MCP desciende de una sesión detectada | Evidencia positiva. Es una carrera con procesos cortos: si se pierde, no hay evidencia |
| **S4 · Hooks de Guardrails** | Señales que dejan los hooks de Guardrails, si existen (R8) | Atribución de commits sin carrera, porque el hook se ejecuta dentro del proceso `git` | Evidencia positiva y opcional. El motor no depende de ella |

**Regla de combinación**:

1. **La sesión existe** si S1 encuentra un proceso de Claude Code cuyo cwd está dentro de un worktree observado. Su origen es "detectado".
2. **Un evento o cambio se atribuye a una sesión** solo con evidencia positiva que apunte a **esa** sesión: S2b, S3, S4 o el **registro explícito** del punto 3.
3. **La co-ubicación de una sesión detectada nunca basta.** Es más estricto que el outline: el SO no permite saber, sin APIs privadas, si un editor o una terminal del desarrollador tienen abierto el worktree. **Excepción, el registro explícito** (BR-VAL-001, BR-EDGE-004, US-GRP-009): quien registra declara que ese agente trabaja en el worktree, así que el registro es evidencia positiva para los eventos del worktree **mientras su sesión sea la única presente en él**. **Solo aplica a sesiones creadas por el registro de un agente sin detección automática** ("otro agente"). **Confirmar una sesión detectada** (Q39) **no activa esta evidencia**, aunque con P16 su origen pase a "registrado": para esa sesión siguen valiendo solo S2b, S3 y S4, y las ediciones del humano en ese worktree nunca se atribuyen a Claude Code (US-GRP-008, BR-EDGE-004). Si hay varias sesiones presentes (worktree compartido), se vuelve a la evidencia por evento (S2b, S3 o S4) y, ante la duda, "sin atribuir" (punto 4 y ADR-GRP-013 § 3).
4. **Dos sesiones en el mismo worktree** (detectadas o registradas): si la evidencia no distingue entre ellas, el evento queda "sin atribuir", porque cada evento apunta a una sola sesión (ADR-GRP-013).
5. **Trabajo en otro directorio** (R3): una escritura con S2b en un worktree distinto del cwd se atribuye a la sesión. La sesión sigue asociada al worktree de su cwd.
6. **En cualquier otro caso, "sin atribuir"**. Cubre el commit simultáneo del humano y de Claude Code en el mismo worktree cuando S3 pierde la carrera y no hay S4.

**Salida del motor**: `Claude Code (detectado)`, el agente registrado con origen "registrado" (incluido "otro agente: <nombre>") o `sin atribuir`. Nunca "humano".

### Ciclo de vida de la sesión

- **Aparece** (activo): S1 ve un proceso nuevo de Claude Code en un worktree observado. El escaneo es periódico; el intervalo es un parámetro interno que mide SPIKE-GRP-001.
- **Activo ↔ inactivo**: se rige por la "actividad" de BR-WF-001, es decir, cambios en los archivos del worktree o eventos de Git dentro del umbral. S2a no cuenta como actividad.
- **Terminado**: el proceso `(pid, hora de inicio)` desaparece (cierre normal o forzado). No se reactiva (Q41). Un `claude --resume` o `--continue` es un proceso nuevo y, por tanto, una sesión nueva, aunque reutilice el id de sesión del transcript.
- **Suspensión del equipo**: el proceso sigue vivo, así que la sesión no termina. Al reanudar, pasa a inactivo si se superó el umbral.
- **Reinicio del motor**: una sesión con la misma `(pid, hora de inicio)` continúa. Si el proceso murió mientras el motor estaba parado, la sesión se cierra como terminada al reconciliar (ADR-GRP-010 y ADR-GRP-013).
- **Registro explícito** de un Claude Code ya detectado: confirma la sesión, sin duplicarla (Q39, US-GRP-009).
- **Retiro del registro**: una sesión registrada figura presente hasta que se retira su registro (Q41, BR-WF-001). El retiro la pasa a terminado con causa "registro retirado" y no se reactiva. Quién puede retirar lo fija ADR-GRP-013 § 2 (⚠️ ASSUMPTION).

### Adaptador de transcripts (S2)

- **Aislamiento**: vive en un adaptador por agente y por versión de formato (`claude_code::transcripts::v1`) detrás de un trait común. Ninguna otra parte del motor conoce el formato.
- **Comprobación de forma**: al arrancar y cuando cambia la versión de Claude Code, valida una muestra de registros: campos esperados, tipos y versión declarada.
  - **Forma reconocida**: S2b activa. Una versión nueva con forma válida se acepta y se anota en el diagnóstico.
  - **Forma no reconocida**, o una tasa de registros ilegibles por encima del límite en ejecución: S2b se desactiva sola y el motor sigue con S1 + S2a + S3 + S4. Lo que S2b habría atribuido queda "sin atribuir". El diagnóstico lo indica; no hay error ni bloqueo.
- **Lectura mínima**: lectura en streaming. De cada registro se extraen los campos permitidos y el resto se descarta sin copiarse.
- **Lectura defensiva (SEC-04, M4)**: `~/.claude` es de un tercero y lo puede escribir un agente. El adaptador abre con `O_NOFOLLOW` (en Windows, sin seguir reparse points), acepta solo **archivos regulares** dentro de `~/.claude/projects`, lee en modo **no bloqueante** (rechaza FIFOs y dispositivos), aplica un **tope por línea y por escaneo** y un **timeout**; si se excede, descarta la línea o el archivo y lo anota en el diagnóstico sin contenido. Las rutas extraídas de un registro **solo se comparan** con las del worktree; nunca se abren.

### Encaje con NFR-08

NFR-08 prohíbe las APIs privadas de un IDE. Leer archivos locales del usuario, con sus permisos y sin llamar a ningún proceso ni servicio de Claude Code, **no es usar una API privada**. Aun así, el formato no es contractual y puede cambiar en cualquier versión. **Riesgo aceptado por Rene Bonilla (PQ-2)**, mitigado con el adaptador versionado, la desactivación automática y el registro explícito como respaldo (R1).

## Alternativas consideradas

1. **Hooks de Claude Code** (`SessionStart`, `PostToolUse` en `~/.claude/settings.json`): es la señal más precisa, pero exige escribir fuera del perfil. **Descartada** por Q17 y Q22.
2. **OpenTelemetry opt-in de Claude Code (S5)**: interfaz documentada y estable, pero el usuario tiene que configurarla en `~/.claude`. **Queda como evolución**: un adaptador más que el usuario activa por su cuenta, y la vía preferente si SPIKE-GRP-001 fracasa o el formato de transcripts se rompe a menudo.
3. **Solo procesos (S1 + S3, opción 1 y parte de la 2)**: detecta las sesiones y los commits con S3, pero casi todos los cambios sin commitear quedan "sin atribuir". Es la **degradación automática** de esta decisión, no la decisión.
4. **Solo transcripts (S2)**: sin S1 no hay un fin de sesión fiable, porque un transcript sin escrituras no indica si el proceso murió. Además, la detección dependería entera de un formato no contractual. **Descartada**.

## Consecuencias

**Positivas**

- Detecta Claude Code sin instalar nada y sin reconfigurar GitRaptor tras instalarlo (BR-EDGE-006).
- Un cambio del formato degrada la precisión, pero nunca produce atribuciones falsas: R1 se convierte en más cambios "sin atribuir".
- El árbol de procesos sirve también a la seguridad del MCP (PQ-6).

**Negativas y riesgos**

- **Commit de Claude Code sin S4**: depende de que S3 gane la carrera. Si la pierde, el commit queda "sin atribuir" y el escenario de commit de US-GRP-007 falla en esa ejecución. SPIKE-GRP-001 mide la tasa.
- **Humano y Claude Code editando el mismo archivo dentro de Δ**: S2b no los distingue. Riesgo residual de atribuir al agente un cambio humano. Lo mide la suite guionizada; si aparece, se reduce Δ o se exige S2b + escritura sin otra escritura en la ventana.
- **Activo no implica que Claude Code esté trabajando**: según BR-WF-001, las ediciones del humano en el worktree también mantienen activa la sesión.
- **Coste de mantenimiento**: el adaptador v1 se revisa con cada cambio de formato de Claude Code.
- **Lectura de cwd y entorno de otros procesos**: varía por SO (Windows y macOS restringen el entorno). S3 con marca de entorno es "si el SO lo permite" y lee **solo esa variable**, nunca el entorno completo; la ascendencia es la base. ADR-GRP-005 usa la ascendencia con identificadores no reutilizables (pidfd, audit token, handle) para los comandos reservados.

## Validación

La valida **SPIKE-GRP-001** (prototipo aislado, sin código del motor):

- **Precisión de sesiones (Q9)**: ≥ 90% en dogfooding en macOS y en la suite guionizada en los tres SO. Cuenta como fallo cada sesión registrada o corregida a mano.
- **0 cambios humanos atribuidos a Claude Code** en la suite guionizada (BR-EDGE-004).
- **Aportación por señal**: se mide por separado la variante **solo mtime** (S1 + S2a + S3 ± S4) y la variante **metadatos** (S1 + S2a + S2b + S3 ± S4). También la tasa de carreras perdidas de S3, el intervalo de escaneo de S1 y el valor de Δ.
- **Seguridad (SEC-04, SEC-05)**: hash de `~/.claude` idéntico antes y después; una línea de 100 MB, un symlink a `/dev/zero` o una FIFO no bloquean ni tumban el daemon; con `claude -p "CANARY"` y un transcript con canario, el canario no aparece en el perfil, los logs ni el stream IPC. Estos tests viven en INF-GRP-001.
- **Criterio de fracaso**: precisión < 90%, o cualquier atribución humano → Claude Code que la regla "ante la duda, sin atribuir" no evite. En ese caso **se replantea este ADR** (S5 opt-in como vía preferente y refuerzo del registro explícito) y se escala a Rene el posible replanteo de US-GRP-007 y 008.

## Privacidad

| Se lee | Nunca se lee | Se persiste en el perfil |
|---|---|---|
| Ruta del ejecutable; solo si es un intérprete (`node`), argv[1] mediante una lectura acotada por SO, descartado al clasificar el proceso. PID, hora de inicio, PPID y cwd | El resto de argv de `claude` (puede llevar el prompt, como en `claude -p "..."`) y `environ`: ni `sysinfo` ni la lectura acotada los cargan | La sesión: agente, origen, worktree, `(pid, hora de inicio)`, estados y sus horas |
| mtime de los transcripts | — | — |
| De cada registro del transcript: herramienta, ruta de archivo, marca de tiempo, id de sesión y cwd | Prompts, respuestas, contenido de código, diffs y el comando de las herramientas de shell | La evidencia de cada atribución: tipo de señal y hora. Su forma la fija ADR-GRP-013 |
| Marca de entorno de Claude Code en los `git` (si el SO lo permite) | Cualquier otra variable de entorno | Nada de los transcripts más allá de lo anterior. **Nunca** prompts, respuestas ni código |

El motor no escribe en `~/.claude` ni en ningún otro lugar fuera de su perfil (Q17, Q21).

## Referencias

- [Índice de decisiones](./index.md), fila y ficha de ADR-GRP-012.
- [Context de motor-local](../../requirements/features/motor-local/context.md): Q9, Q17, Q21, Q22, Q24, Q29, Q32-Q35, Q39 y Q41; riesgos R1, R2, R3, R7 y R8.
- [Reglas de negocio](../../requirements/features/motor-local/business-rules.md): BR-WF-001, BR-TIME-001, BR-EDGE-003, BR-EDGE-004, BR-EDGE-006 y BR-AUTH-002.
- [US-GRP-007](../../requirements/features/motor-local/user-stories/US-GRP-007-sesiones-claude-code.md) y [US-GRP-008](../../requirements/features/motor-local/user-stories/US-GRP-008-editor-humano-sin-atribuir.md).
- [Historias técnicas](../../requirements/features/motor-local/technical-stories.md): SPIKE-GRP-001.
- [Documento de negocio](../../business/gitraptor-documento-de-negocio.md): NFR-08, D2.
- ADR-GRP-005 (comandos reservados y ascendencia), ADR-GRP-006 (perfil), ADR-GRP-007 (umbral), ADR-GRP-009, ADR-GRP-010 (observador y huecos) y ADR-GRP-013 (modelo persistido de atribución).

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. No cambia las señales ni la regla de combinación.

| Hallazgo | Cómo se cubre |
|---|---|
| M3 · `sysinfo` carga `cmd`/`environ` completos por defecto | Señal S1 y tabla de Privacidad: `ProcessRefreshKind` sin `cmd` ni `environ`; identificación por la ruta del ejecutable y, solo con un intérprete, lectura acotada de argv[1] descartada al clasificar (SEC-04) |
| M4 · Lector de transcripts sin límites | Adaptador de transcripts: `O_NOFOLLOW`, solo archivos regulares, lectura no bloqueante, topes por línea y por escaneo y timeout; rutas extraídas solo se comparan (SEC-04) |
| I4 · Supuesto "la shell de Claude Code no tiene TTY" | Deja de ser crítico: ADR-GRP-005 ya no se apoya en la terminal del cliente, sino en comprobaciones del daemon (terminal de control y líder de sesión). SPIKE-GRP-001 lo sigue midiendo como dato |

Validación ampliada: SEC-04 y SEC-05 (punto "Seguridad"), con los tests en INF-GRP-001.
