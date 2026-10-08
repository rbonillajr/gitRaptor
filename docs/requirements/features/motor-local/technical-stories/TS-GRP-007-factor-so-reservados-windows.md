---
id: TS-GRP-007
title: "Factor del SO (Windows Hello) para los comandos reservados de alto riesgo en Windows"
type: ts
status: draft
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-08
updated: 2026-10-08
related:
  adrs: [ADR-GRP-005, ADR-GRD-007, ADR-GRD-008]
  stories: [TS-GRP-004, SPIKE-GRD-002]
  specs: [DS-TS-GRP-004]
ado:
  id: null
  url: null
tags: [motor-local, windows, comandos-reservados, presencia-humana, windows-hello, escritorio-seguro, factor-fuera-de-banda, tq-14, m-01]
---

## TS-GRP-007: Factor del SO (Windows Hello) para los comandos reservados de alto riesgo en Windows

**Valor**: en Windows, un agente que elige padre (`PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`, M-01) hereda la consola y la ascendencia limpia de la PowerShell del desarrollador y pasa la prueba de presencia por consola de TQ-14 (opción A, DS-TS-GRP-004 § 9). Es un riesgo residual **mayor que en Unix**, donde no hay una primitiva equivalente. Esta historia lo cierra para los comandos de alto riesgo con un factor del SO que un proceso del mismo usuario no puede fabricar.

### Descripción

**Opción B de la decisión de Rene Bonilla (2026-10-08)**, registrada como mejora posterior a la opción A. Los comandos reservados de alto riesgo (los que relajan protección según ADR-GRD-007 § 1 y ADR-GRP-005 § 6: desinstalar la protección, la excepción consciente, parar el daemon, retirar repos) piden en Windows, además de los controles 1 a 3, una verificación de **Windows Hello en el escritorio seguro** lanzada por el daemon. Sin ella, el comando se rechaza (fail-closed). Reutiliza el mecanismo de [ADR-GRD-008](../../../../architecture/decisions/ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) y el resultado de [SPIKE-GRD-002](../../guardrails/technical-stories/SPIKE-GRD-002-factor-so-daemon.md) en Windows; no redefine ese ADR.

### Alcance Técnico

- Qué comandos reservados son "de alto riesgo" en Windows (lista cerrada, decidida en la Dev Spec con el Arquitecto y el PO).
- Llamada al factor desde el daemon (sesión interactiva del solicitante, C3 de DS-TS-GRP-004 § 9) y rechazo si no está disponible.
- Auditoría: el resultado del factor queda junto a la cadena de ascendencia y al host de la consola.
- Fuera de alcance: macOS y Linux (ya cubiertos por ADR-GRD-008 donde aplique).

### Plan de Verificación

- En la máquina Windows real: un comando de alto riesgo lanzado por un proceso con padre suplantado (PowerShell del desarrollador como padre) se rechaza sin la verificación de Windows Hello y se acepta con ella.
- Sin Windows Hello configurado, el comando se rechaza (fail-closed) con un mensaje que explica cómo configurarlo.

### Dependencias

- SPIKE-GRD-002 (parte Windows) y ADR-GRD-008.
- DS-TS-GRP-004 § 9 (TQ-14, opción A), ya implementada.
