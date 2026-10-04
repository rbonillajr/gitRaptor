---
id: SPIKE-GRP-001
title: "Precisión de la detección de Claude Code en dogfooding"
type: spike
status: Dev Spec Pending
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-012, ADR-GRP-013, ADR-GRP-005]
  stories: [US-GRP-007, US-GRP-008, US-GRP-009]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, spike, deteccion, atribucion, claude-code, dogfooding, precision]
---

## SPIKE-GRP-001: Precisión de la detección de Claude Code en dogfooding

**Valor**: confirmar, antes de desarrollar US-GRP-007 y US-GRP-008, que las señales de ADR-GRP-012 alcanzan el 90% sin atribuir nunca trabajo humano a Claude Code.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-GRP-001-precision-deteccion.md`. Prototipo aislado, sin código del motor. **Depende de**: — (arranca el día uno). **Valida**: ADR-GRP-012; informa ADR-GRP-013 (evidencia por evento) y ADR-GRP-005 (supuesto de PQ-6).

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
- Si la shell de Claude Code sí tiene TTY interactiva: se revisa ADR-GRP-005 § 6 antes de implementar los comandos reservados.

### Time-box

2 semanas de dogfooding más 2 días de suite guionizada (BRD § 13, spike c). ⚠️ **ASSUMPTION**: al menos 50 sesiones reales para que el porcentaje sea significativo.
