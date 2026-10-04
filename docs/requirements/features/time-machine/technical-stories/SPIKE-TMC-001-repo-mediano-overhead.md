---
id: SPIKE-TMC-001
title: "Repo mediano de referencia y viabilidad del snapshot en menos de 200 ms"
type: spike
status: ready
feature: time-machine
domain: GRP
priority: high
complexity: medium
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-TMC-006, ADR-TMC-001, ADR-TMC-004, ADR-GRP-011]
  stories: [US-TMC-001, US-TMC-004, US-TMC-020, INF-GRP-002, SPIKE-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [time-machine, spike, rendimiento, nfr-04, d-tmc-21, repo-mediano, almacen]
---

## SPIKE-TMC-001: Repo mediano de referencia y viabilidad del snapshot en menos de 200 ms

**Valor**: fija con números el "repo mediano" de NFR-04 (D-TMC-21) y confirma, o corrige, el almacén de ADR-TMC-001 y el reparto de ADR-TMC-006 antes de que US-TMC-001 entre en desarrollo.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-TMC-001-repo-mediano-overhead.md`. Es el spike (a) del BRD § 13. Prototipo aislado, sin código del daemon. **Depende de**: nada (arranca el día uno). **Valida**: ADR-TMC-001 § 3, ADR-TMC-004 § 2 y ADR-TMC-006. **Se apoya en**: el generador de repos y los runners de INF-GRP-002 y las cifras de SPIKE-GRP-002.

### Pregunta

¿Qué tamaño tiene un "repo mediano" para GitRaptor y, en ese repo, un snapshot previo escrito con Git del sistema en un almacén privado del perfil cumple un p95 menor de 200 ms en Windows, macOS y Linux, con un coste de disco y de siembra aceptable?

### Hipótesis

- Repo mediano ≈ 10.000 archivos con seguimiento, 300 MB de working tree y 50.000 commits (ADR-TMC-006 § 3).
- Con el almacén sembrado y un delta de hasta 100 archivos y 20 MB, el snapshot cumple el reparto por etapa de ADR-TMC-006 § 2, también en Windows.
- El camino rápido (nada cambió desde la última captura) cuesta menos de 50 ms.
- La siembra por enlace duro tarda segundos y casi no ocupa disco; la siembra por copia escala con el tamaño del historial.
- Una captura por observación con `Q` = 1 s y `M` = 5 s y 10 worktrees activos no mueve el p95 del motor por encima de 300 ms.

### Experimento

- **Corpus**: elegir repos públicos en los percentiles 50, 75 y 90 de archivos, working tree y commits; proponer el de referencia y uno mayor fuera de referencia.
- **Etapas**: medir el p95 de cada etapa de ADR-TMC-006 § 2 con un delta de 1, 10, 100 y 1.000 archivos, en los tres SO y en la máquina de dogfooding.
- **Coste de procesos**: comparar un proceso de Git por paso frente a procesos persistentes por almacén, y medir la ganancia de escribir el almacén con gitoxide (escalón 3 de ADR-TMC-006 § 5, preaprobado).
- **Siembra y disco**: tiempo y espacio con enlace duro y con copia; crecimiento del almacén tras una semana simulada de capturas.
- **Contenido**: coste de archivos LFS reales y de un archivo de 1 GB sin seguimiento ni ignorado.
- **Captura continua**: CPU, disco escrito y p95 del motor con 10 worktrees y una ráfaga de 1.000 archivos; ajustar `Q`, `M`, el límite de 50 MB y las cuotas de SEC-TMC-12.
- **Repo intacto**: el prototipo no cambia la huella del repo.

### Criterios de Éxito

- Rene aprueba el repo de referencia con números concretos, que pasan a ADR-TMC-006 § 3 y al banco de US-TMC-020.
- p95 total menor de 200 ms en el repo de referencia en los tres SO, con la cifra por etapa registrada en ADR-TMC-006.
- Valores medidos para `Q`, `M` y el límite de tamaño de la captura por observación.
- **Vía de fracaso**: si el p95 no se cumple, se aplican en orden los escalones de ADR-TMC-006 § 5 y se vuelve a medir; si solo cumple el tercero, se activa ese escalón (preaprobado, TQ-4 → a) y se anota en ADR-TMC-006. Si el disco del almacén es inaceptable, se lleva a Rene para revisar ADR-TMC-001.

### Time-box

⚠️ **ASSUMPTION**: 1 semana, en paralelo con SPIKE-GRP-002.
