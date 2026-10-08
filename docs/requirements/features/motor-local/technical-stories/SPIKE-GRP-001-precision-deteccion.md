---
id: SPIKE-GRP-001
title: "Precisión de la detección de Claude Code en dogfooding"
type: spike
status: ready
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-03
updated: 2026-10-07
related:
  adrs: [ADR-GRP-012, ADR-GRP-013, ADR-GRP-005]
  stories: [US-GRP-007, US-GRP-008, US-GRP-009]
  specs: [DS-US-GRP-007]
ado:
  id: null
  url: null
tags: [motor-local, spike, deteccion, atribucion, claude-code, dogfooding, precision]
---

## SPIKE-GRP-001: Precisión de la detección de Claude Code en dogfooding

**Valor**: confirmar, antes de desarrollar US-GRP-007 y US-GRP-008, que las señales de ADR-GRP-012 alcanzan el 90% sin atribuir nunca trabajo humano a Claude Code.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-GRP-001-precision-deteccion.md`. Prototipo aislado, sin código del motor. **Depende de**: — (arranca el día uno). **Valida**: ADR-GRP-012; informa ADR-GRP-013 (evidencia por evento) y ADR-GRP-005 (supuesto de PQ-6).
>
> **Cambio (2026-10-05, US-GRP-007)**: por instrucción del coordinador para el hito M1, el spike se valida **con el motor real** de US-GRP-007 en dogfooding, y **solo para S1 + S3**; S2a y S2b quedan abiertas hasta US-GRP-008. El procedimiento (duración, verdad de referencia, suite guionizada en macOS y métricas) está en el § 5 de la [Dev Spec de US-GRP-007](../dev-specs/US-GRP-007-dev-spec.md), y la enmienda en ADR-GRP-012. Pendiente de que Rene ratifique el cambio de secuencia.

### Pregunta

¿La combinación S1 + S2a + S2b + S3 (± S4) de ADR-GRP-012 detecta las sesiones de Claude Code con una precisión ≥ 90% (Q9) y con 0 cambios humanos atribuidos a Claude Code (BR-EDGE-004)?

### Hipótesis

- La variante **S2b (metadatos)** alcanza el 90% en sesiones y atribuye la mayoría de los cambios sin commitear.
- La variante **S2a (solo mtime)** detecta bien las sesiones pero deja casi todos los cambios sin commitear "sin atribuir".
- S3 pierde la carrera con los `git` cortos en una fracción medible de los commits; S4 la elimina cuando existe.
- La herramienta de shell de Claude Code no ofrece una terminal interactiva a sus comandos (supuesto de PQ-6 en ADR-GRP-005).

### Experimento

- **Precisión de sesiones (Q9)**: sesiones detectadas correctamente frente al total real; cuenta como fallo cada sesión registrada o corregida a mano.
- **Errores humano → Claude Code**: cambios del humano atribuidos a Claude Code (BR-EDGE-004).
- **Comparar S2a frente a S2b** (PQ-2): variante solo mtime (S1 + S2a + S3 ± S4) frente a variante metadatos (S1 + S2a + S2b + S3 ± S4), con y sin señales de hooks de Guardrails (R7).
- **Parámetros internos**: tasa de carreras perdidas de S3, intervalo de escaneo de S1 y valor de la ventana Δ de S2b.
- **Transiciones de estado**: activo, inactivo y terminado llegan dentro del umbral; `--resume` y `--continue` producen una sesión nueva.
- **Robustez del adaptador**: con un transcript de forma no reconocida, S2b se desactiva sola y no produce atribuciones falsas.
- **TTY de la shell de Claude Code**: comprobar si los comandos que lanza Claude Code tienen entrada y salida estándar conectadas a una terminal interactiva, y si un agente podría superar la confirmación de ADR-GRP-005 § 6.
- **Privacidad**: el prototipo no extrae del transcript nada fuera de herramienta, ruta, marca de tiempo, id de sesión y cwd.
- **Verdad de referencia**:
  - Suite guionizada con etiquetas exactas, en los tres SO: Claude Code y edición humana en el mismo worktree y en otro; Claude Code que modifica otro directorio (R3); dos sesiones en un worktree; cierre forzado y suspensión.
  - Dogfooding real en macOS, con revisión diaria en la que Rene marca cada sesión como correcta o incorrecta y añade las que faltaron.

### Criterios de Éxito

- Precisión ≥ 90% en dogfooding (macOS) y en la suite guionizada en los tres SO.
- **0** cambios humanos atribuidos a Claude Code en la suite guionizada.
- Respuesta documentada sobre la TTY de la shell de Claude Code.
- **Vía de fracaso**: precisión < 90%, o cualquier atribución humano → Claude Code que "ante la duda, sin atribuir" no evite. Entonces se replantea ADR-GRP-012 con **S5 (telemetría OpenTelemetry opt-in) como vía preferente y el refuerzo del registro explícito**, y se escala a Rene el posible replanteo de US-GRP-007 y US-GRP-008.
- Si la shell de Claude Code sí tiene TTY interactiva: se anota como dato; tras la revisión de seguridad ADR-GRP-005 § 6 ya no depende de ello (el daemon comprueba terminal de control y líder de sesión, I4).

### Time-box

2 semanas de dogfooding más 2 días de suite guionizada (BRD § 13, spike c). ⚠️ **ASSUMPTION**: al menos 50 sesiones reales para que el porcentaje sea significativo.

### Mediciones

#### 2026-10-06 — primer dato real de la carrera S3 (macOS, dogfooding)

- **Observado**: en la salida de `raptor events` revisada, los **3 commits** del worktree `dehotspot` (09:46:26) salieron "sin atribuir", y el push del worktree `xp30` salió "Claude Code, detected". `raptor sessions` veía las 3 sesiones activas. En ese momento `dehotspot` tenía una sola sesión, así que la causa es la carrera de S3: el `git` terminó antes de la muestra.
- **Dato**: 3 commits sin atribuir por la carrera, frente a 1 push detectado en la misma salida. No se contó el total N de eventos del día: **N queda pendiente** de la revisión diaria.
- **Cómo lo cambia la regla** ([enmienda de ADR-GRP-012 del 2026-10-06](../../../../architecture/decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md)): esos 3 commits **siguen contando como "sin atribuir"** en la precisión de atribución. Ahora además llevan la pista `inferred` (Claude Code, la sesión única del worktree). Se mide aparte:
  - **Pistas emitidas**: eventos sin atribuir que llevan `inferred`.
  - **Pistas correctas**: las que Rene confirma en la revisión diaria como de esa sesión.
  - **Pistas sobre trabajo humano**: el riesgo residual de la enmienda. No es un error humano → Claude Code de BR-EDGE-004, porque el actor sigue "sin atribuir", pero se cuenta para decidir si S4 o S5 son urgentes.
- **Pendiente**: repetir la medición con la regla desplegada, con N real y la tasa de carreras perdidas de S3 (eventos `s3_evidence` con `outcome=no-sighting` del log del daemon).

#### 2026-10-07 — S4: el hook de Guardrails atribuye los commits rápidos (suite guionizada, macOS)

- **Hallazgo del dogfooding**: en la prueba de Guardrails, el hook detectó que los commits los hacía un agente (y bloqueó los que incumplían la política), pero en `raptor events` los commits rápidos del agente que sí entraron salieron "sin atribuir". Guardrails resuelve el actor con el cliente vivo que le habla; S3 mira el árbol de procesos después y pierde la carrera.
- **Cambio**: S4 según la [enmienda del 2026-10-07 de ADR-GRP-012](../../../../architecture/decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md) y [DS-US-GRP-007 § 7](../dev-specs/US-GRP-007-dev-spec.md). La resolución del hook en el `reference-transaction` `prepared` se asocia al evento del mismo movimiento de rama, y el evento queda atribuido con evidencia `{"signals":["s4"]}`.
- **Medición**: `measure_quick_commit_attribution` en `apps/cli/tests/events_s4_hook_attribution.rs` (`#[ignore]`; se ejecuta con `RAPTOR_S4_RUNS=20 cargo test -p gitraptor-cli --test events_s4_hook_attribution measure_quick_commit_attribution -- --ignored --nocapture`). Usa el binario real como daemon y hook, el Claude Code simulado de larga vida y 20 `git commit -qm` sin hook lento, con un repo y un perfil temporales. Una sola ejecución en el Mac de desarrollo:

  | Hooks | Commits rápidos del agente | Atribuidos | Sin atribuir |
  |---|---|---|---|
  | Sin Guardrails | 20 | 5 (S3, 25 %) | 15 (75 %) |
  | Guardrails instalado | 20 | 20 (S4, 100 %) | 0 |

- **Lectura**: con los hooks instalados, la carrera de S3 deja de pesar en los commits gobernados. Sin hooks no cambia nada: la tasa de carreras perdidas de S3 en esta suite es del 75 % de los commits rápidos.
- **Errores humano → Claude Code**: 0 en los e2e. El commit del desarrollador con la sesión del agente viva sigue "sin atribuir", con y sin hooks.
- **Observación sin investigar**: en esta suite, los 15 commits sin atribuir no llevaban la pista `single-session` (evidencia vacía). Puede ser una condición de la pista (sesión activa, actividad) o del montaje. Queda anotado para la revisión diaria; no se tocó en esta rama. **Investigada el 2026-10-08** (entrada siguiente): era el montaje, y la fila "Sin Guardrails" de la tabla está sesgada.
- **Pendiente**:
  - repetir en el dogfooding real con N de eventos del día (log `s3_evidence`, con el resultado nuevo `outcome=s4`);
  - medir el p95 que añade la resolución en el hilo de conexión por cada `reference-transaction` (lo pidió el Arquitecto, con un presupuesto supuesto de ≤ 5 ms sin multiplexor);
  - validar en Linux y Windows (etapa multiplataforma).

#### 2026-10-08 — La pista de sesión única que faltaba sin hooks: la ponía en duda el propio arnés

- **Causa raíz**: tras cada commit, el arnés leía el oid nuevo con `git rev-parse HEAD`, lanzado por el test (sin el agente entre sus ancestros) dentro del repo. Ese `git` caía en la ventana S3 del mismo evento. Con el log `s3_evidence` y un volcado temporal de las muestras se vio que los 17 commits sin atribuir de una repetición daban `outcome=ambiguous`, nunca `no-sighting`: S3 **sí veía** el `git` de la sesión y, además, ese `git` ajeno en el repo. Por la regla de ADR-GRP-012 (enmienda del 2026-10-06, "Ambigüedad"), con un `git` ajeno no hay atribución ni pista. El motor cumplía la regla. Se descartaron la sesión inactiva, el worktree distinto y el trailer (#144): no intervenían.
- **Arreglo**: el arnés lee `HEAD` de los archivos de `.git`, sin lanzar un `git`. La regla de la pista no cambia (solo con exactamente una sesión activa, sin `git` ajeno, nunca con `human-author`).
- **Test**: `without_guardrails_every_quick_agent_commit_is_attributed_or_hinted` (mismo archivo, no ignorado). Con 10 commits rápidos sin hooks, cada uno sale atribuido por S3 o, sin atribuir, con la pista `single-session` confirmada por su trailer. Falla con el `head()` anterior.
- **Medición corregida** (una ejecución, Mac de desarrollo):

  | Hooks | Commits rápidos del agente | Atribuidos | Sin atribuir con pista | Sin atribuir sin pista |
  |---|---|---|---|---|
  | Sin Guardrails | 20 | 19 (S3) | 1 | 0 |
  | Guardrails instalado | 20 | 20 (S4) | 0 | 0 |

- **Lectura**: en esta suite, sin hooks, S3 pierde la carrera en ~5 % de los commits rápidos y no en el 75 %, y la pista cubre esos casos. La ventaja de S4 sigue siendo la misma: elimina la carrera.
- **Riesgo real que deja ver (sin cambio en esta rama)**: un `git` de solo lectura de otro proceso en el repo justo después del commit del agente (la extensión de Git de un editor o el prompt de la terminal) produce el mismo `ambiguous`: el commit queda sin atribuir y sin pista. Cuántas veces pasa se medirá en el dogfooding real (log `s3_evidence` con `outcome=ambiguous`). Relajar la regla es una decisión del PO o de Rene.
