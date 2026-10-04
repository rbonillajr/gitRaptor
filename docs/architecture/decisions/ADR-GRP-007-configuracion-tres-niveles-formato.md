---
id: ADR-GRP-007
title: Configuración en tres niveles — formato JSON con `$schema`, estructura y precedencia
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
deciders: [Rene Bonilla]
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-004, ADR-GRP-005, ADR-GRP-006, ADR-GRP-008, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012]
tags: [configuracion, settings-json, json-schema, schemars, precedencia, niveles, guardrails, motor-local, p8, seguridad]
---

# ADR-GRP-007 — Configuración en tres niveles: formato y precedencia

> **Feature:** `motor-local` (F-001-01). Cierra la pregunta **P8** del contexto. **Coautoría:** las secciones `permissions` y `policies`, y la regla de que un nivel personal no relaja una prohibición del equipo, son de Guardrails (F-001-04). Este ADR fija solo el contenedor, el formato, los niveles y la sección `engine`. No pasa a `accepted` sin el context de Guardrails.

## Contexto

El motor **lee** su configuración en tres niveles y **nunca la escribe** (Q23, BR-CONS-007). Cada valor declara qué niveles lo admiten y, dentro de ellos, gana el más específico (Q24):

- **Rama base**: solo el nivel de equipo; `main` por defecto (BR-CONS-006).
- **Umbral de inactividad**: solo el perfil y el nivel local; 5 minutos por defecto (BR-TIME-001, Q20).

El comando para editar la configuración es de Guardrails (Q27). La misma configuración de equipo aloja las políticas por repo de BR-11, y el BRD v0.4 dejó de fijar el archivo `.gitraptor/policy.yaml` para que el formato lo decida un ADR. US-GRP-013 (umbral) y US-GRP-016 (rama base del equipo, diferida por Q36) están bloqueadas por esta decisión.

Restricciones: Rust (ADR-GRP-001), todo local y sin red (NFR-03), licencias permisivas (NFR-11) y frontera de solo lectura sobre el repo (BR-CONS-001).

## Decisión

**JSON estricto con `$schema`, al estilo de Claude Code**, en tres archivos con la misma estructura (decisión de Rene Bonilla, 2026-10-03, propuesta base del BRD v0.4 y PQ-4).

### Archivos

| Nivel | Archivo | Versionado | Dónde |
|---|---|---|---|
| Perfil | `settings.json` | No | Carpeta de configuración del perfil (ADR-GRP-006) |
| Equipo | `.gitraptor/settings.json` | Sí, con el repo | **Worktree principal** del repo (PQ-9, ver abajo) |
| Local | `settings.local.json` | No, por diseño | En el **perfil**, indexado por repo (ADR-GRP-008, PQ-3) |

### Estructura

Un solo documento JSON con secciones de primer nivel:

- `$schema`: URL del schema publicado. Sirve a los editores y el motor no la descarga nunca (NFR-03).
- `engine`: valores del motor. Los define este ADR.
- `permissions` (`allow` / `ask` / `deny`) y `policies`: las define el context de Guardrails. Hasta entonces el schema las admite como objetos que el motor no interpreta.

Claves de la sección `engine`:

| Clave | Tipo | Niveles admitidos | Por defecto | Regla |
|---|---|---|---|---|
| `engine.baseBranch` | string (nombre de rama) | Solo equipo | `"main"` | BR-CONS-006, Q24 |
| `engine.idleThresholdMinutes` | entero, de 1 a 1440 | Perfil y local | `5` | BR-TIME-001, Q20, Q24 |
| `engine.gitPath` | string (ruta absoluta al ejecutable de Git) | Solo perfil | sin valor (resolución automática) | ADR-GRP-009 § 4, Q28 |
| `engine.watcher.fallbackPollSeconds` | entero, de 5 a 3600 | Perfil y local | `30` | ADR-GRP-010 § 5 (sondeo de respaldo) |
| `engine.watcher.degradedPollSeconds` | entero, de 1 a 60 | Perfil y local | `2` | ADR-GRP-010 § 5 (modo degradado) |

Criterio de niveles de los valores nuevos:

- **`engine.gitPath`, solo perfil**: es una ruta de la máquina, y el motor resuelve un único Git para todos los repos (ADR-GRP-009). Un valor por repo (local) implicaría un Git distinto por repo, que el motor no admite; en el equipo sería una ruta de otra máquina.
- **Intervalos del watcher, perfil y local, nunca equipo**: son preferencias de coste de la máquina (CPU y disco). El nivel local permite ajustarlos para un repo concreto muy grande. En el equipo impondrían ese coste en las máquinas de todos.
- **`engine.gitPath` que no sirve** (no existe, no es ejecutable, es anterior a 2.38 o no supera la validación del ejecutable de ADR-GRP-009 § 4: absoluto, archivo regular, propiedad del usuario o de root y no escribible por grupo ni otros, SEC-10): no invalida el nivel. El candidato se descarta con diagnóstico y la resolución sigue con el resto de candidatos de ADR-GRP-009. Al cambiar el valor, el motor vuelve a resolver Git.

**Parámetros que no son configurables** (internos, fijados por su ADR): la ventana de debounce de 75 ms (forma parte del presupuesto de ADR-GRP-011; cambiarla exige revisar ese ADR), el intervalo de recomprobación de Git en "Esperando Git" (ADR-GRP-009), el intervalo de escaneo de procesos y la ventana Δ de atribución (ADR-GRP-012, los mide SPIKE-GRP-001), los tiempos del handshake y del arranque bajo demanda (ADR-GRP-005) y la persistencia del histograma de frescura (ADR-GRP-011). La variable de entorno de sobreescritura del perfil (ADR-GRP-006) tampoco es una clave de configuración: decide dónde están los archivos de configuración, así que no puede vivir dentro de ellos.

Ejemplos:

```jsonc
// Equipo: <worktree principal>/.gitraptor/settings.json
{
  "$schema": "<URL del schema publicado>",
  "engine": { "baseBranch": "develop" },
  "permissions": { "deny": ["…"] },   // semántica: Guardrails
  "policies": { }                      // semántica: Guardrails
}
```

```jsonc
// Perfil: <config del perfil>/settings.json
{ "$schema": "<URL del schema publicado>", "engine": { "idleThresholdMinutes": 10 } }
```

```jsonc
// Local: <config del perfil>/repos/<id-repo>/settings.local.json  (ADR-GRP-008)
{ "$schema": "<URL del schema publicado>", "engine": { "idleThresholdMinutes": 15 } }
```

Los archivos son JSON estricto: no admiten comentarios. Los `//` de los ejemplos son solo explicativos.

### Schema

- Se **genera desde los tipos Rust** con `schemars` (MIT). Los tipos viven en `crates/policy`, que pasa a ser el dueño del documento de configuración completo: carga, precedencia, validación y diagnósticos. El motor (`crates/core`) consume la sección `engine` ya resuelta. Guardrails añade los tipos de `permissions` y `policies` en el mismo crate.
- Cada clave lleva la anotación propia `x-gitraptor-levels` (p. ej. `["team"]` o `["profile", "local"]`), que se genera desde un atributo en el tipo Rust. Es la fuente única de los niveles admitidos.
- El schema se **embebe en el binario** (`include_str!`) para validar sin red, se versiona en el repo y se publica en la URL de `$schema`. Un test de CI falla si el schema generado difiere del versionado.

### Precedencia

Valor efectivo de una clave = el del nivel **más específico, entre los que la admiten**, que la defina; si ninguno la define, el valor por defecto. El orden de especificidad es perfil < equipo < local (BR-CONS-007).

| Clave | Perfil | Equipo | Local | Resultado |
|---|---|---|---|---|
| `baseBranch` | ignorada, con diagnóstico | **la usa** | ignorada, con diagnóstico | equipo, o `main` |
| `idleThresholdMinutes` | la usa | ignorada, con diagnóstico | **la usa y gana** | local, si no perfil, si no `5` |
| `gitPath` | **la usa** | ignorada, con diagnóstico | ignorada, con diagnóstico | perfil, si no resolución automática |
| `watcher.fallbackPollSeconds` | la usa | ignorada, con diagnóstico | **la usa y gana** | local, si no perfil, si no `30` |
| `watcher.degradedPollSeconds` | la usa | ignorada, con diagnóstico | **la usa y gana** | local, si no perfil, si no `2` |

La regla de que un nivel personal no relaja una prohibición del equipo se aplica a `permissions` y `policies`. Es de Guardrails y este ADR no la redefine.

### Validación y diagnósticos

| Situación | Comportamiento |
|---|---|
| Archivo ausente | El nivel no aporta valores. No es un error. |
| JSON inválido | Se **ignora el nivel entero** y se emite un diagnóstico con archivo, línea y columna. Los demás niveles siguen aplicando (decisión de Rene Bonilla, 2026-10-03, PQ-8). |
| El documento no valida contra el schema (tipo o rango incorrecto) | Se **ignora el nivel entero**, con diagnóstico y la ruta JSON de la clave (PQ-8). |
| Clave desconocida | Se ignora la clave y se emite un **diagnóstico, no un error fatal**. El resto del nivel aplica. |
| Clave en un nivel que no la admite | Se ignora esa clave, con diagnóstico (Q24). El resto del nivel aplica. |
| El archivo no es un archivo regular, es un enlace que sale del worktree (o de la carpeta de configuración del perfil) o supera el tamaño máximo | Se **ignora el nivel entero**, con diagnóstico **sin contenido** del archivo (SEC-11). |
| `engine.baseBranch` no es un nombre de rama válido según `check-ref-format` o empieza por `-` | Se ignora la clave, con diagnóstico, y nunca se pasa a Git. Como el equipo sí declaró una rama base, el motor no recurre a `main`: indica que no puede calcular ahead/behind, igual que con una rama inexistente (Q42) (SEC-11). |

**Por qué una clave desconocida no es fatal**: el archivo de equipo viaja con el repo y lo leen binarios de versiones distintas en las máquinas del equipo. Si una clave nueva invalidara el nivel en un binario antiguo, ese binario perdería la rama base y los demás valores del equipo. Además Guardrails y el motor amplían el documento por separado. El diagnóstico detecta las erratas sin romper la compatibilidad hacia adelante. Por eso el schema no declara `additionalProperties: false`, y el cargador compara las claves contra el schema para avisar.

Los diagnósticos se exponen a los clientes por el canal local (ADR-GRP-005) como una lista por repo con tipo, archivo, posición o ruta JSON, y nivel. **Nunca incluyen fragmentos del contenido** del archivo ni valores leídos: `.gitraptor/settings.json` viaja con el repo y lo puede escribir un agente o un PR, y podría apuntar a un secreto (SEC-11). El texto que ve el usuario lo traduce el cliente (i18n en/es). Un diagnóstico nunca detiene la observación.

### Qué configuración de equipo manda (PQ-9)

Cada worktree puede tener en su rama una versión distinta de `.gitraptor/settings.json`. Manda la del **worktree principal** del repo, leída del archivo en disco (decisión de Rene Bonilla, 2026-10-03, PQ-9). Así hay un único valor de repo, como la rama base, para todos los worktrees. Las versiones de los worktrees enlazados se ignoran para los valores del motor. Si el repo no tiene worktree principal con árbol de trabajo (repo bare), el nivel de equipo no aporta valores y se emite un diagnóstico. Coordinar con el context de Guardrails, que puede necesitar la misma regla para sus políticas.

### Lectura y recarga

El motor abre los tres archivos **solo para leer**, nunca los crea, y vigila sus cambios con el observador de ADR-GRP-010. Al cambiar un archivo, recarga ese nivel y reaplica la precedencia sin reiniciarse (BR-CONS-007).

## Alternativas consideradas

- **TOML**: admite comentarios y es idiomático en Rust, pero no tiene `$schema` en línea y no comparte el modelo mental de `.claude/settings.json`, que es la referencia del usuario objetivo. Descartada.
- **YAML (`.gitraptor/policy.yaml`, BRD ≤ v0.3)**: tipado ambiguo (el "problema de Noruega") y el crate `serde_yaml` está sin mantenimiento. Además, un solo archivo de equipo no cubre los tres niveles. Descartada; el BRD v0.4 ya la retiró.
- **JSONC o JSON5**: admiten comentarios, pero el soporte de editores y validadores es desigual y se aleja de Claude Code. Descartada. Se puede revisar si los usuarios piden comentarios.
- **Nivel local en el repo + `.gitignore`**, o **en `.git/info/exclude`**: ver ADR-GRP-008. Se descartan porque el motor no escribe en el repo (Q21) y no puede garantizar que el archivo no se versione.
- **Clave desconocida o schema inválido como error fatal**: rompe la compatibilidad entre versiones del binario y dejaría de observar por una errata, en contra del ASSUMPTION de BR-CONS-007 y de PQ-8.

## Consecuencias

- ✅ Un modelo familiar para quien ya usa `.claude/settings.json`, con validación y autocompletado en el editor gracias a `$schema`.
- ✅ Una sola fuente de verdad para los niveles admitidos: el tipo Rust genera el schema y la anotación `x-gitraptor-levels`.
- ✅ Desbloquea US-GRP-013 y, cuando Guardrails cierre su context, US-GRP-016.
- ✅ El motor sigue observando con cualquier configuración, válida o no.
- ⚠️ Ignorar el nivel de equipo entero cuando es inválido también deja sin efecto sus `permissions` y `policies`. **Mitigación:** el diagnóstico es visible en todos los clientes; la reacción de Guardrails ante una configuración de equipo inválida (p. ej. bloquear en lugar de permitir) la decide su context.
- ⚠️ JSON estricto no admite comentarios. **Mitigación:** las descripciones del schema se ven en el editor; se puede reconsiderar JSONC más adelante sin cambiar la estructura.
- ⚠️ Un archivo de equipo cambiado solo en un worktree enlazado no tiene efecto hasta que llega al principal (PQ-9). **Mitigación:** diagnóstico informativo cuando la versión de un worktree enlazado difiere de la del principal.
- ⚠️ Esta decisión obliga a actualizar ADR-GRP-002 (`crates/policy`) y ADR-GRP-004 (editor de configuración), que citaban `policy.yaml`. Hecho el 2026-10-03.

## Validación

Tests de `crates/policy`, siempre con repos y perfiles temporales (variable de entorno de sobreescritura del perfil, ADR-GRP-006), nunca con este repo:

1. **Precedencia**: un caso por fila de las tablas de BR-CONS-006 y BR-CONS-007 (p. ej. perfil 5 + local 15 → 15; umbral en el equipo → ignorado; `baseBranch` en local → ignorado).
2. **JSON inválido** en cada nivel: se ignora solo ese nivel, los demás aplican y el diagnóstico trae archivo, línea y columna.
3. **Schema inválido** (tipo o rango): se ignora el nivel entero y el diagnóstico trae la ruta JSON.
4. **Clave desconocida** y **clave fuera de nivel**: el nivel aplica y se emite el diagnóstico.
5. **Valores nuevos**: `gitPath` en local o equipo → ignorado con diagnóstico; `gitPath` inexistente en el perfil → diagnóstico y resolución automática (ADR-GRP-009); intervalos del watcher en el equipo → ignorados; local gana al perfil; un intervalo fuera de rango invalida el nivel (PQ-8).
6. **PQ-9**: worktree principal y enlazado con `baseBranch` distintos → manda el principal. Repo bare → diagnóstico y `main`.
7. **Recarga**: editar un archivo cambia el valor efectivo sin reiniciar el motor.
8. **Archivos hostiles (SEC-11)**: `.gitraptor/settings.json` como symlink a `~/.ssh/id_rsa` → nivel ignorado y diagnóstico sin contenido; FIFO o archivo de tamaño excesivo → ignorado sin bloquear; `baseBranch` `--upload-pack=x` → ignorada; `engine.gitPath` relativa o escribible por otros → descartada (SEC-10).
9. **Solo lectura**: tras cargar y recargar, los hashes y las fechas de modificación de los tres archivos y el estado observable del repo no cambian (BR-CONS-001).
10. **Deriva del schema**: el schema generado es igual al versionado y al embebido (CI).

## Referencias

- [BRD-GRP-001](../../business/gitraptor-documento-de-negocio.md): BR-11, changelog v0.4, NFR-03, NFR-11.
- [Contexto `motor-local`](../../requirements/features/motor-local/context.md): Q3, Q20, Q23, Q24, Q27, Q31, Q36; P8.
- [Reglas de negocio `motor-local`](../../requirements/features/motor-local/business-rules.md): BR-CONS-001, BR-CONS-006, BR-CONS-007, BR-TIME-001.
- Historias: US-GRP-012, US-GRP-013, US-GRP-016.
- ADRs: [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md), [ADR-GRP-002](./ADR-GRP-002-monorepo-nx.md), [ADR-GRP-004](./ADR-GRP-004-estado-frontend-ux.md), ADR-GRP-005, ADR-GRP-006, [ADR-GRP-008](./ADR-GRP-008-configuracion-local-no-versionada.md), ADR-GRP-010.
- Decisiones de Rene Bonilla, 2026-10-03: propuesta base del BRD v0.4, PQ-3, PQ-4, PQ-8, PQ-9.
- [Claude Code — Settings](https://docs.anthropic.com/en/docs/claude-code/settings) (modelo de referencia), [`schemars`](https://crates.io/crates/schemars).

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. No cambia formato, niveles ni precedencia.

| Hallazgo | Cómo se cubre |
|---|---|
| M10 · `.gitraptor/settings.json` y la ruta explícita de Git sin validar | Validación y diagnósticos: solo archivo regular, sin enlaces que salgan del worktree, tope de tamaño, diagnósticos sin contenido, `baseBranch` validada como ref (SEC-11); `engine.gitPath` validada según ADR-GRP-009 § 4 (SEC-10) |

Validación ampliada: SEC-11 y SEC-10 (punto 8).
