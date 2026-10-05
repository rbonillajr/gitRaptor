---
mode: draft
status: expanded
generated: 2026-10-03
updated: 2026-10-05
generator: architect
domain: GRP
feature: motor-local
total_artifacts: 11
expanded: 11
approved: 0
related:
  context: [CTX-GRP-001]
  rules: [BR-GRP-001]
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013, ADR-GRP-014, ADR-GRP-015]
---

# Technical Stories — INDEX: Motor local

> Índice. Cada historia vive en su archivo dentro de [`technical-stories/`](./technical-stories/), con status `Dev Spec Pending` (TS e INF; la Dev Spec se genera con `/aadd-devspec <id>`) o `Research Pending` (SPIKE, que no tienen Dev Spec: llevan un Research Brief).
>
> **Criterio de inclusión (Enabler Decision Gate)**: solo trabajo técnico **sin historia de usuario dueña y sin resultado observable** por el usuario. Lo observable ya está en US-GRP-001..016. Hay 11 enablers para 19 historias. INF-GRP-003 e INF-GRP-004 (release y distribución, NFR-06) no habilitan ninguna US: son el camino para entregar el binario.
>
> **Trabajo técnico de una sola historia**: va en la Dev Spec de esa historia, no aquí. Ver "Trabajo técnico que vive en la US" en el [architecture overview](../../../architecture/architecture-overview.md).

## Índice

| ID | Tipo | Título | Valor (1 línea) | ADR | US que habilita | Depende de | Complejidad | Status |
|----|------|--------|-----------------|-----|-----------------|-----------|-------------|--------|
| [TS-GRP-001](./technical-stories/TS-GRP-001-almacen-perfil.md) | TS | Almacén de datos del motor en el perfil | Persistencia por repo en el perfil, que sobrevive a reinicios y aísla la corrupción | ADR-GRP-006, ADR-GRP-013 | US-GRP-001, 004, 005, 006, 007, 009, 010, 011, 015 | — | Medium | Done (PR #16, [Dev Spec](./dev-specs/TS-GRP-001-dev-spec.md)). ⚠️ Pendiente: comprobación de la ACL en Windows (SEC-06), requisito antes de cualquier release en Windows |
| [TS-GRP-002](./technical-stories/TS-GRP-002-lectura-git.md) | TS | Capa de lectura de Git sin escrituras | Lecturas con frontera estricta: cero escrituras, cero locks, cero programas del usuario; resolución de Git ≥ 2.38 | ADR-GRP-009 | US-GRP-001, 002, 003, 012, 014 | — | High | Done (PR #20, [Dev Spec](./dev-specs/TS-GRP-002-dev-spec.md)). ⚠️ Pendiente antes de soportar Windows: comprobar las ACE del `git.exe` (hoy se rechaza, *fail-closed*) |
| [TD-GRP-001](./technical-stories/TD-GRP-001-acl-windows.md) | TD | Verificación de ACL en Windows: `git.exe` (SEC-10) y perfil (SEC-06) | Cierra los dos pendientes *fail-closed* de Windows de TS-GRP-001 y TS-GRP-002 | ADR-GRP-006, ADR-GRP-009, ADR-GRP-002 | US-GRP-001..016 en Windows | TS-GRP-001, TS-GRP-002 | Medium | In Progress ([Dev Spec](./dev-specs/TD-GRP-001-dev-spec.md)) |
| [TS-GRP-003](./technical-stories/TS-GRP-003-proceso-motor.md) | TS | Proceso del motor en segundo plano por usuario | `raptor daemon`: instancia única, ciclo de vida y parada ordenada | ADR-GRP-005 | US-GRP-001, 002, 004, 005, 014, 015 | TS-GRP-001, TS-GRP-002 | High | Dev Spec Pending |
| [TS-GRP-004](./technical-stories/TS-GRP-004-canal-clientes.md) | TS | Canal local de clientes y contrato de mensajes | Canal solo del usuario, contrato JSON-RPC, comandos reservados y biblioteca cliente con arranque bajo demanda | ADR-GRP-005, ADR-GRP-011, ADR-GRP-013 | US-GRP-001..016 | TS-GRP-003 | High | Dev Spec Pending |
| [TS-GRP-005](./technical-stories/TS-GRP-005-clases-trabajo-ahorro-energia.md) | TS | Clases de trabajo del daemon y mecanismo de ahorro de energía | El trabajo de fondo corre con prioridad baja del SO y lo que el usuario espera nunca baja (RES-06, RES-07, RES-08) | ADR-GRP-015 | US-GRP-017 (clase por pool), US-GRP-019; consumidores: TS-TMC-001, US-TMC-004, TS-CKP-001 | TS-GRP-003, US-GRP-002, INF-GRP-002 (RES-07) | High | Dev Spec Pending (M1, Should) |
| [INF-GRP-001](./technical-stories/INF-GRP-001-arnes-repo-intacto.md) | INF | Arnés de verificación "repo intacto" en los tres SO | Gate de CI: cero diferencias imputables al motor en el repo y fuera del perfil; repo canario y auditoría de `exec` (SEC-09) | ADR-GRP-009, ADR-GRP-006 | Todas, de forma transversal (BR-CONS-001) | TS-GRP-002 (núcleo); suites incrementales con TS-GRP-003, TS-GRP-004, US-GRP-002, US-GRP-004 y US-GRP-007 | Medium | Dev Spec lista; núcleo implementado, pendiente de la primera ejecución del gate en CI |
| [INF-GRP-002](./technical-stories/INF-GRP-002-banco-frescura-escala.md) | INF | Banco de medición de frescura y escala | Gate de CI de NFR-04 y NFR-05 que nombra la etapa que se pasó | ADR-GRP-011, ADR-GRP-010 | US-GRP-001, 002, 012 | TS-GRP-004, US-GRP-002 | Medium | Dev Spec Pending |
| [INF-GRP-003](./technical-stories/INF-GRP-003-pipeline-release.md) | INF | Pipeline de release: 6 targets, firma, checksums, SBOM y borrador de GitHub Release | Un tag `v*` produce los binarios de NFR-06 verificables y un borrador; nada se publica sin un paso humano | ADR-GRP-014 | — (NFR-06, transversal) | — (release real en Windows: pendientes de TS-GRP-001 y TS-GRP-002) | Medium | In Progress (brief compacto; firma y attestation preparadas, requieren secreto) |
| [INF-GRP-004](./technical-stories/INF-GRP-004-canales-distribucion.md) | INF | Canales de distribución: Homebrew, winget, npm y scripts verificados | Canales generados y probados en cada release; publicar solo depende de secretos y un paso humano | ADR-GRP-014 | — (NFR-06; US de instalación pendiente para el PO) | INF-GRP-003 | Medium | In Progress (publicación pendiente del tap, la reserva en npm y los secretos) |
| [SPIKE-GRP-001](./technical-stories/SPIKE-GRP-001-precision-deteccion.md) | SPIKE | Precisión de la detección de Claude Code en dogfooding | Medir el 90% sin atribuir trabajo humano; S2a frente a S2b; TTY de la shell de Claude Code | ADR-GRP-012 (valida) | US-GRP-007, 008 | — (prototipo aislado) | Medium | Research Pending |
| [SPIKE-GRP-002](./technical-stories/SPIKE-GRP-002-viabilidad-observador.md) | SPIKE | Viabilidad del observador de cambios a escala en los tres SO | Medir las cifras de ADR-GRP-011, el debounce de 75 ms, límites, bloqueos en Windows y huecos | ADR-GRP-010, ADR-GRP-011 (valida) | US-GRP-002, 003, 004, 005 | — (prototipo aislado) | Medium | Done en macOS ([resultados](./research/SPIKE-GRP-002-resultados.md); enmiendas aplicadas en ADR-GRP-010 y ADR-GRP-011). ⚠️ Pendiente: Linux y Windows, con el procedimiento del README del prototipo |

**Cambios de complejidad respecto al outline**: TS-GRP-002, TS-GRP-003 y TS-GRP-004 pasan de Medium a High (riesgo de pérdida de datos o superficie de seguridad en tres SO y ruta crítica). TS-GRP-003 añade la dependencia de TS-GRP-002 (resuelve Git al arrancar). El arranque bajo demanda pasa a TS-GRP-004 (biblioteca cliente) para no crear un ciclo entre TS-GRP-003 y TS-GRP-004.

## Ruta de ejecución sugerida

- **Antes del esqueleto andante**: TS-GRP-001 y TS-GRP-002 en paralelo; luego TS-GRP-003 y después TS-GRP-004. El núcleo de INF-GRP-001 sale tras TS-GRP-002 y bloquea el merge de cualquier historia (BR-CONS-001 es transversal); sus suites incrementales entran con TS-GRP-003, TS-GRP-004, US-GRP-002, US-GRP-004 y US-GRP-007, y cada una bloquea solo el merge de su historia dueña.
- **En paralelo desde el día uno**: SPIKE-GRP-001 y SPIKE-GRP-002, que no dependen de código del motor. Sus resultados pueden cambiar ADR-GRP-012, ADR-GRP-010, ADR-GRP-011 y ADR-GRP-005 § 6 antes de que US-GRP-002 y US-GRP-007 entren en desarrollo.
- **Después de US-GRP-002**: INF-GRP-002 y, tras él, TS-GRP-005 (su merge lo bloquea RES-07 en el banco; decisión del orquestador, 2026-10-05, validada por Arquitecto y PO).
- **Sin dependencias de código**: INF-GRP-003 y después INF-GRP-004. Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO: el release se divide en dos enablers y no se publica nada hasta decidir la licencia y el nombre.
- ⚠️ **ASSUMPTION**: la base de CI del monorepo con runners de Windows, macOS y Linux viene del spike del stack (ADR-GRP-001) o de un bootstrap común. Si no existe, INF-GRP-001 la incorpora. **Resuelto (2026-10-04)**: no existía; INF-GRP-001 propone el primer workflow (`.github/workflows/repo-intact.yml`).
