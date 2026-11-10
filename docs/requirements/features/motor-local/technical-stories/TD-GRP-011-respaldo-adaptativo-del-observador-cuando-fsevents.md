---
id: TD-GRP-011
title: "Respaldo adaptativo del observador cuando FSEvents pierde eventos bajo carga"
type: td
status: draft
feature: motor-local
domain: GRP
priority: medium
complexity: medium
created: 2026-11-10
updated: 2026-11-10
source: inline
related:
  adrs: [ADR-GRP-010, ADR-TMC-004, ADR-GRP-015]
  stories: [TD-GRP-006]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, observador, fsevents, carga, rpo, energia]
---

## TD-GRP-011: Respaldo adaptativo del observador cuando FSEvents pierde eventos bajo carga

**Valor**: el RPO no se degrada justo cuando hay agentes trabajando y la Mac está cargada.

> **Reserva (2026-11-10).** Solo la ficha: sin código. Origen: pedido del coordinador al firmar TD-GRP-006. Dueño: Arquitecto.

### Descripción

**Como** Arquitecto
**Quiero** evaluar un respaldo adaptativo que acorte el barrido de respaldo (y/o la reconciliación del worktree) cuando el observador detecta que FSEvents pierde eventos
**Para** decidir, con datos, si el RPO bajo carga justifica su coste en despertares y energía

**Hecho**: TD-GRP-006 midió que con carga 47–70 fseventsd pierde 23–25 de 30 eventos sin flag (ADR-GRP-010, Enmienda 2026-11-10). Entonces el RPO de las ediciones sin cambio en Git cae hasta la reconciliación de 5 min (ADR-TMC-004, Enmienda 2026-11-10) y los cambios Git tardan hasta 30 s. TD-GRP-006 no cambió el sondeo y añadió la métrica de vía de lectura (evento, despertar o respaldo) en la nota de lectura de S3.

### Alcance Técnico

- **Medir** en dogfooding la proporción de lecturas por vía (evento, despertar, respaldo), en reposo y con agentes trabajando.
- **Proponer** la señal de disparo: dominio de la vía de respaldo, atraso de la cola de FSEvents (p. ej. un evento centinela propio que no vuelve a tiempo) u otra, con su criterio de entrada y de salida.
- **Proponer** la cota inferior del intervalo del barrido de respaldo y de la reconciliación del worktree cuando la señal está activa.
- **Estimar** el coste en despertares y energía y su relación con el modo ahorro (RES, ADR-GRP-015).
- **Estimar** el efecto en el RPO de las ediciones sin cambio en Git y en la latencia de los cambios Git.
- **Registrar** la decisión como enmienda de ADR-GRP-010, y de ADR-TMC-004 si cambia el RPO.
- **Fuera de alcance**: la regla S3 (la atribución no se recupera con sondeo).

### Plan de Verificación

- Datos de dogfooding con la proporción de lecturas por vía, en reposo y bajo carga, adjuntos a la enmienda.
- Si se implementa: un test por señal muestra que, con los eventos tirados, una edición se captura dentro de la cota nueva.
- Si se implementa: sin regresión de energía en reposo (la señal no se activa sin pérdida de eventos).
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
