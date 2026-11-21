---
id: TD-CKP-001
title: "La cola de consultas del canal responde query.cancel después de la cancelación y sus tests dependen de la velocidad del diff"
type: td
status: implemented
feature: cockpit
domain: CKP
priority: medium
complexity: small
created: 2026-11-21
updated: 2026-11-21
source: inline
related:
  adrs: [ADR-GRP-004]
  stories: [US-CKP-012, TS-CKP-006]
ado:
  id: null
  url: null
tags: [cockpit, deuda-tecnica, canal, consultas, tests-inestables]
---

## TD-CKP-001: La cola de consultas del canal responde query.cancel después de la cancelación y sus tests dependen de la velocidad del diff

**Valor**: el contrato de `query.cancel` (responde ya; la consulta termina después) se cumple por construcción, y los tests de la cola dejan de depender de qué tan rápido diffea la máquina.

### Descripción

**Como** desarrollador que mantiene el canal
**Quiero** que el orden de respuestas de `query.cancel` no dependa del planificador del sistema y que los tests de la cola sostengan el pool ocupado por señal
**Para** no ver `ckp006_007_a_full_queue_is_busy` ni `cancel_answers_now_and_the_query_gets_query_cancelled` en rojo de forma intermitente en el CI

**Origen**: CI de #366 y #368 (ubuntu y macOS).

### Causa raíz

1. **Carrera real en producción (`cancel_answers_now…`).** `QueryTable::cancel` ponía el token en `true` y devolvía; solo después el lector del canal empujaba `{cancelled: true}` al outbox. Entre las dos cosas, el hilo del pool, que sondea el token, podía ver la cancelación, terminar y empujar `query-cancelled` **antes** que la respuesta del propio cancel. El test comprueba justo ese orden (`at(cancel) < at(query)`).
2. **Test que apuesta a la velocidad (`ckp006_007_a_full_queue_is_busy`).** Esperaba que 6 conexiones llenaran el pool (1 en curso + 4 en cola) mientras el diff «tarda lo bastante». Si el hilo del pool vaciaba la cola entre dos escrituras (runner rápido, o el proceso de prueba desplanificado), nunca había `query-busy`.
3. **`tui_conflicts`**: no se reprodujo (0 de 30 con carga, y verde en este worktree desde `main`); sin causa raíz que arreglar. Ver el PR.

### Decisión

**Decisión del orquestador (2026-11-21).** Arreglo directo, sin ADR nuevo: no cambia el contrato (ya decía que `query.cancel` responde ya), solo lo cumple sin carrera.

- `QueryTable::cancel` recibe un cierre `announce` que empuja la respuesta de `query.cancel` y corre **antes** de poner el token, bajo el cerrojo de la tabla. El trabajo solo ve la cancelación después de que la respuesta ya está en el outbox.
- Costura de pruebas `ChannelConfig::test_query_gate` (solo debug, como `test_diff_deadline`): una puerta que retiene el hilo del pool antes de cada trabajo hasta que el test la abre. El test de la cola la usa: con el pool retenido, 6 consultas desbordan sí o sí (cupo máximo 5) y el `query-busy` llega mientras la puerta sigue cerrada. Sin `sleep` ni plazos mayores.
- **Qué comprueba cada test no cambia** (`cancel_answers_now…` queda intacto; el de la cola sigue exigiendo ≥ 1 `query-busy` y ningún otro error), solo cómo se llega al estado «pool lleno».

### Plan de Verificación

- Unitario: `cancel_announces_before_the_token_is_set` (la respuesta sale con el token aún sin poner).
- Integración: `ckp006_007_a_full_queue_is_busy` y `cancel_answers_now_and_the_query_gets_query_cancelled` 40 de 40 con 24 procesos `yes` de carga.

Implementado en: PR #373
