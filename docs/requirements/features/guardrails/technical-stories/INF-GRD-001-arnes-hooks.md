---
id: INF-GRD-001
title: "Arnés de la capa de hooks: repos con hooks previos, huella, interrupción y matriz de CI"
type: inf
status: draft
feature: guardrails
domain: GRP
priority: critical
complexity: medium
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-005, ADR-GRD-007]
  stories: [US-GRD-001, US-GRD-002, US-GRD-003, US-GRD-004, US-GRD-005, US-GRD-006, US-GRD-007, SPIKE-GRD-001]
  specs: []
ado:
  id: null
  url: null
tags: [guardrails, ci, arnes, hooks-git, nfr-01, nfr-12, interrupcion, huella, matriz-so]
---

## INF-GRD-001: Arnés de la capa de hooks: repos con hooks previos, huella, interrupción y matriz de CI

**Valor**: ninguna historia de Guardrails se mergea si pierde un hook del usuario, si deja una instalación a medias o si publica una lista de operaciones no impedibles que no coincide con Git.

### Descripción

**Como** Arquitecto
**Quiero** un arnés que monte repos temporales con hooks previos y gestores de hooks y que compare la huella antes y después de instalar y desinstalar, con cortes del proceso en cada paso, como gate de CI en los tres SO
**Para** verificar NFR-01 y NFR-12 de la capa de hooks con un criterio binario, y que la lista publicada de ADR-GRD-002 coincide con el comportamiento real de Git

> Dev Spec: `dev-specs/INF-GRD-001-arnes-hooks.md` | Pendiente
>
> **Depende de**: el núcleo de INF-GRP-001 (huella del repo y de fuera del repo, ejecución de control y guarda), que se reutiliza y amplía, y de SPIKE-GRD-001 (fixtures y matriz). **Núcleo y suites**: el núcleo de este INF no depende de código de Guardrails. Cada suite entra con su historia dueña (US-GRD-001 instalación y mínimo; US-GRD-002 encadenado; US-GRD-003 desinstalación e interrupción; US-GRD-004 pérdida externa y lista publicada; US-GRD-005 registro fuera del repo; US-GRD-006 token) y bloquea solo el merge de su historia y de las siguientes, como regresión. **ADRs**: ADR-GRD-001 (Validación), ADR-GRD-002 (Validación), ADR-GRD-005 (Validación 2). **Seguridad**: SEC-GRD-02, SEC-GRD-08 y SEC-GRD-10.

### Alcance Técnico

- **Crear** fixtures de repos temporales: sin hooks, con hooks propios, con husky, con lefthook, con pre-commit, con varios worktrees, con configuración por worktree y con inclusiones condicionales.
- **Ampliar** la huella de INF-GRP-001 con una excepción por escenario: tras una instalación explícita solo se admiten la clave local de hooks y la carpeta de Guardrails; tras desinstalar, cero diferencias.
- **Implementar** la comparación del archivo de configuración del repo con el criterio **semántico** que fijó SPIKE-GRD-001 (Q-GRD-29; ADR-GRD-001 § 4, Enmienda 2026-10-04): mismo valor efectivo y nivel de la clave y demás entradas sin cambios; el resto de rutas, byte a byte.
- **Implementar** puntos de corte nombrados en cada paso de la transacción, activables solo en builds de prueba, para matar el proceso antes y después de cada paso.
- **Implementar** la comprobación de recuperación: tras cada corte, el arranque del daemon deja el repo completo o idéntico al anterior.
- **Crear** el ejecutor de la matriz de interceptabilidad: lanza cada operación del catálogo con Git crudo y compara el resultado con la lista publicada del binario.
- **Crear** el medidor de latencia por evaluación gobernada y de la vía rápida, con el p95 como gate, medido en un runner en reposo.
- **Crear** el contador de procesos de hook por comando (commit, `switch`, `rebase`, `fetch`, `stash`) y versión de Git, contra una tabla de referencia: gate determinista del coste por comando (ADR-GRD-002 § 5, Enmienda 2026-10-04).
- **Crear** los casos de SPIKE-GRD-001 como regresión (Enmienda 2026-10-04): *prune* de `pack-refs` y `gc` con y sin binario (D12–D14), borrados a través de `HEAD` en Git 2.38 (D09), alias de mayúsculas en todas las líneas (D08, D21, F11), NFC sin `precomposeUnicode` (D19b), estado `preparing`, valores `ref:`, renombrado sobre la base con el backend de archivos (D11) y renombrado con **reftable** en cada versión de la matriz (D10, D11), para retirar la fila de la lista si Git lo corrige.
- **Crear** los escenarios de pérdida externa: reinstalación de gestores (incluidos `lefthook install --force` y `--reset-hooks-path`), edición de dispatchers, carpeta borrada, binario movido, repo movido, configuración por worktree, cambio de backend de refs (`git refs migrate`) y hook previo añadido después de instalar (`hook-previo-no-encadenado`).
- **Crear** los escenarios de integridad: edición a la vez del dispatcher y del manifiesto, binario sin firma o con otra huella, y perfil borrado con la protección instalada (instalación huérfana).
- **Crear** los escenarios de actualización del binario: nuevo destino del enlace estable y plantilla de dispatchers nueva, con cortes en cada paso del refresco.
- **Crear** los escenarios hostiles sobre la carpeta de Guardrails: carpeta o temporal sustituidos por enlaces, archivos ajenos añadidos y configuración del repo sustituida tras la escritura.
- **Configurar** la matriz de CI en Windows, macOS y Linux con Git 2.38 y con la última versión estable, y el registro de cada suite como gate de su historia dueña.
- **Reutilizar** la guarda de INF-GRP-001: el arnés se niega a ejecutarse dentro del repo de GitRaptor.
- **Fuera de alcance**: la huella y el arnés del motor (INF-GRP-001); las pruebas de la capa MCP (US-GRD-016); la base de CI del monorepo, salvo que no exista (⚠️ **ASSUMPTION**: viene de motor-local).

### Plan de Verificación

#### Pruebas Automatizadas

Las del núcleo se cumplen al cerrar este INF. Las de cada suite, al cerrar su historia dueña.

- **Sensibilidad**: una build de prueba cuyo instalador cambia un byte de un hook previo hace fallar el arnés. Otra que escribe en la configuración global de Git, también.
- **Interrupción (suite de US-GRD-003)**: un corte en cada punto nombrado; los tres SO quedan siempre completos o idénticos al estado anterior.
- **Encadenado (suite de US-GRD-002)**: con cada gestor, el hook previo rechaza lo mismo antes y después de instalar.
- **Matriz (suite de US-GRD-004)**: la lista publicada coincide con el resultado del ejecutor en cada SO y versión de Git. Una discrepancia bloquea el merge.
- **Pérdida externa (suite de US-GRD-004)**: cada escenario lleva el estado a inactivo con la causa correcta dentro del plazo de NFR-GRD-10.
- **Latencia**: el p95 por evaluación gobernada cumple NFR-GRD-04 en los tres SO.
- **Integridad (suite de US-GRD-004)**: la edición combinada del dispatcher y del manifiesto, y el binario sin firma, se detectan. El perfil borrado da instalación huérfana.
- **Actualización (suite de US-GRD-001)**: el refresco no pide permiso, deja la protección activa y resiste los cortes.
- **Carpeta hostil (suite de US-GRD-003)**: ningún escenario escribe ni borra fuera de la carpeta de Guardrails.
- **Registro (suite de US-GRD-005)**: tras denegaciones, la huella del repo no cambia y no aparece nada nuevo para commitear.
- **Gate**: un PR que rompe cualquiera de los anteriores queda bloqueado en CI. Una historia anterior no queda bloqueada por una suite que aún no existe.

#### Verificación Manual / Sandbox

- Revisar el informe de una ejecución fallida provocada a propósito y comprobar que nombra el escenario, el paso de la transacción, la ruta y el tipo de cambio.
