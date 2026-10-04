---
id: TS-GRP-002
title: "Capa de lectura de Git sin escrituras"
type: ts
status: Dev Spec Pending
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-009, ADR-GRP-007]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-012, US-GRP-014]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, git, gitoxide, solo-lectura, allowlist, resolucion-git]
---

## TS-GRP-002: Capa de lectura de Git sin escrituras

**Valor**: el motor lee repos y worktrees sin provocar ni una escritura, ni un lock, ni la ejecución de un programa del usuario.

### Descripción

**Como** Arquitecto
**Quiero** una capa única de lectura de Git con frontera estricta de solo lectura y resolución del Git del sistema
**Para** que BR-CONS-001 se verifique con un criterio binario de cero diferencias y el motor nunca compita por un lock con los agentes

> Dev Spec: `dev-specs/TS-GRP-002-lectura-git.md` | Pendiente
>
> **Depende de**: — (arranca el día uno). **ADRs**: ADR-GRP-009 (§ 1 a § 4; la tabla del § 2 es la frontera que la Dev Spec debe seguir), ADR-GRP-007 (`gitPath`).
>
> **Complejidad alta**: riesgo de pérdida de datos y seguridad (NFR-01, NFR-02), tres SO y una frontera más estricta que la tolerancia de BR-CONS-001.

### Alcance Técnico

- **Crear** la capa de lectura en `crates/git` como único punto del monorepo que toca repos observados (ADR-GRP-009).
- **Implementar** las lecturas del camino caliente con gitoxide en solo lectura: refs, HEAD, índice, estado del working tree, metadatos de worktrees, marcadores de operación en curso y reglas de ignore.
- **Implementar** las primitivas de historia (base de fusión y recuentos) que usan ahead/behind, acotadas para repos de más de 100K commits.
- **Implementar** la invocación del Git CLI solo para la allowlist cerrada de ADR-GRP-009 § 3, expuesta como funciones tipadas, sin shell y con ejecutable por ruta absoluta.
- **Neutralizar** en ambas vías todo programa configurado por el usuario: filtros, `textconv`, diff externo, firma, pager, hooks, fsmonitor, credenciales y trazas (ADR-GRP-009 § 2).
- **Garantizar** cero escrituras en el repo y fuera del perfil, incluidas las transitorias como los locks, con aperturas que dejan a otros procesos borrar y renombrar.
- **Implementar** un tiempo máximo por invocación cuyo resultado es "no disponible temporalmente", nunca un dato inventado.
- **Implementar** el registro de cada argv ejecutado en modo diagnóstico, para que INF-GRP-001 audite la allowlist.
- **Implementar** la resolución de Git por candidatos (`gitPath` del perfil, PATH heredado y rutas conocidas por SO) y la verificación de la versión mínima 2.38 (ADR-GRP-009 § 4).
- **Evitar** invocar el shim de Git de macOS cuando no hay toolchain de desarrollador, para no abrir el diálogo de instalación.
- **Fuera de alcance**: el estado "Esperando Git", su recomprobación y su exposición (US-GRP-014); el observador de cambios (US-GRP-002); ahead/behind frente a la rama base (US-GRP-012); la predicción de conflictos, porque `merge-tree --write-tree` está prohibido en el motor (ADR del Cockpit).

### Plan de Verificación

#### Pruebas Automatizadas

- **Huella**: para cada lectura, la huella del directorio Git común, de los worktrees enlazados y de cada working tree (rutas, tamaño, contenido y mtime de archivos y directorios) es idéntica antes y después.
- **fsmonitor**: con `core.fsmonitor` activado, la lectura no arranca el daemon ni crea archivos en el directorio Git.
- **Índice**: con untracked cache y split index activados y stat sucio, el índice no se reescribe y no aparece ningún lock.
- **Programas del usuario**: un filtro, un `textconv`, un diff externo, un programa de firma, un pager, un destino de traza y un hook `post-index-change` de prueba, cada uno con un marcador que escribe un archivo, nunca se ejecutan.
- **Concurrencia**: un `git add` de un agente simulado durante una lectura nunca falla por un lock del motor.
- **Allowlist**: el registro de argv solo contiene subcomandos y opciones de la lista; una comprobación estática exige que el lanzamiento de procesos aparezca solo en el módulo de invocación.
- **Repo hostil**: en Windows, un `git.exe` en el working tree no se ejecuta.
- **Resolución**: con PATH mínimo se encuentra Git en las rutas conocidas; en macOS sin Command Line Tools el shim no se lanza; un Git anterior a 2.38 se reporta como insuficiente; un `gitPath` inválido deja diagnóstico y la resolución sigue con los demás candidatos.
- **Tiempo máximo**: una invocación colgada se termina y devuelve "no disponible temporalmente".
- **safe.directory**: un repo rechazado se reporta "no disponible" sin tocar la configuración global de Git.

#### Verificación Manual / Sandbox

- Ejecutar la batería en los tres SO con repos temporales, nunca con este repo, antes de que INF-GRP-001 la convierta en gate.
