---
id: TD-GRP-010
title: "Los tests de caja blanca del observador esperan un evento FS con cota de pared y fallan si FSEvents lo pierde bajo carga"
type: td
status: draft
feature: motor-local
domain: GRP
priority: low
complexity: low
created: 2026-11-10
updated: 2026-11-10
source: inline
related:
  adrs: [ADR-GRP-010]
  stories: [TD-GRP-006]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, observador, fsevents, tests, carga]
---

## TD-GRP-010: Los tests de caja blanca del observador esperan un evento FS con cota de pared y fallan si FSEvents lo pierde bajo carga

**Valor**: las suites del observador no fallan en una Mac cargada por un evento que el sistema operativo pierde.

> **Reserva (2026-11-10).** Solo la ficha: sin código. Origen: decisión 5 de TD-GRP-006 (2026-11-10).

### Descripción

**Como** Arquitecto
**Quiero** que los tests de caja blanca del observador no dependan de que llegue un evento FS concreto dentro de una cota de reloj de pared
**Para** que la pérdida de eventos de FSEvents bajo carga no los vuelva intermitentes

**Hecho**: bajo carga fseventsd pierde eventos sin ningún flag de pérdida (ADR-GRP-010, Enmienda 2026-11-10; sonda: con carga ~47, 23 de 30 escrituras nunca llegan). Las suites de vigilancia y de niveles de observación (espera de 10 s) y la de clases de observación (espera de 15 s, escrituras de los ficheros de listo y de después) esperan ese evento. 0 fallos en CI en ~1000 corridas, pero en una Mac cargada pueden fallar.

### Alcance Técnico

- **Esperar** por la lectura forzada en los tests que prueban la clasificación, en vez de por el evento.
- **Marcar** como sensibles a carga, o reescribir hasta ver contando intentos, los pocos tests que prueban la entrega del evento (patrón de #353/#356: intentos, no reloj de pared).
- **Fuera de alcance**: cambios de producto.

### Plan de Verificación

- Las tres suites pasan 10 de 10 seguidas con otra suite en paralelo en macOS.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
