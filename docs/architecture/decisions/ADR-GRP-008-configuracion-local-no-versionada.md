---
id: ADR-GRP-008
title: Configuración local personal sin versionar — `settings.local.json` en el perfil, indexado por repo
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-007]
tags: [configuracion, settings-local-json, perfil, no-versionado, solo-lectura, motor-local, p10]
---

# ADR-GRP-008 — Configuración local personal sin versionar

> **Feature:** `motor-local` (F-001-01). Cierra la pregunta **P10** del contexto, que **desaparece por diseño**. El formato del archivo lo fija ADR-GRP-007.

## Contexto

El tercer nivel de la configuración guarda los ajustes personales del desarrollador para un repo en su máquina, como el umbral de inactividad (Q20, Q23, BR-TIME-001). No puede versionarse: si se commitea, la preferencia de una persona se impone al equipo.

Hasta ahora ese nivel vivía "en el repo, sin versionar" (tabla de datos de BR-CONS-001). Eso deja un problema sin resolver (P10): el motor no escribe nada en el repo (Q21, BR-CONS-001) ni hace modificaciones operativas (Q22), así que no puede crear un `.gitignore` ni tocar `.git/info/exclude` para garantizarlo. Además, un archivo sin rastrear solo existe en el worktree donde se creó, mientras que el umbral se define "por repo" (Q3).

## Decisión

El nivel local vive **en el perfil de GitRaptor, fuera del repo, indexado por repo** (decisión de Rene Bonilla, 2026-10-03, PQ-3):

```
<config del perfil>/                      ← carpeta de configuración del perfil (ADR-GRP-006)
├─ settings.json                          ← nivel perfil
└─ repos/
   └─ <id-repo>/
      └─ settings.local.json              ← nivel local de ese repo
```

```json
{ "$schema": "<URL del schema publicado>", "engine": { "idleThresholdMinutes": 15 } }
```

- **Clave**: `<id-repo>` es la clave de repo de ADR-GRP-006, compartida por todos los worktrees. El ajuste vale **por repo**, no por worktree (Q3).
- **Formato, niveles admitidos y validación**: los de ADR-GRP-007. El archivo es del usuario. El motor solo lo lee (Q23) y no crea ni el archivo ni la carpeta `repos/<id-repo>/`. Los edita el desarrollador a mano o con el comando de Guardrails (Q27).
- **Encontrar el archivo**: el motor expone por el canal local (ADR-GRP-005), en solo lectura, la ruta esperada del `settings.local.json` de cada repo, exista o no. Los clientes y el comando de Guardrails la usan para que el usuario no tenga que conocer el id.
- **Carpeta de configuración, no de datos**: el archivo va junto a los demás archivos que edita el usuario y separado de la base que escribe el motor: son carpetas distintas en los tres SO, y en macOS las subcarpetas `config/` y `data/` de la carpeta de la app (ADR-GRP-006 § 1). Así una corrupción o un borrado de los datos del motor no lo arrastra.
- **Retirar un repo** (Q25) no borra su `settings.local.json`, igual que no borra sus datos.

**P10 desaparece**: el archivo no está en ningún working tree, así que no hay forma de versionarlo ni hace falta `.gitignore`, `.git/info/exclude` ni una advertencia.

**Consecuencia para el PO (no se edita aquí)**: hay que actualizar la tabla "Qué datos propios genera el motor y dónde viven" de BR-CONS-001, que dice "Repo, no versionada" para la configuración local personal, y la descripción de BR-CONS-007 (nivel 3, "no versionada"), para que digan "perfil, indexada por repo". También el punto 6 de BR-CONS-001 ("ni la configuración local personal del repo") deja de aplicar al repo.

## Precedencia y niveles admitidos

Sin cambios respecto a ADR-GRP-007, cuya tabla de precedencia y niveles admitidos es la completa y la que manda (incluye `gitPath` y los intervalos del watcher). El nivel local sigue siendo el más específico aunque viva en el perfil. Extracto de las claves que afectan a este ADR:

| Clave | Perfil (`settings.json`) | Equipo (`.gitraptor/settings.json`) | Local (`repos/<id-repo>/settings.local.json`) |
|---|---|---|---|
| `engine.baseBranch` | no admitida | **admitida** | no admitida |
| `engine.idleThresholdMinutes` | admitida | no admitida | **admitida, gana** |

## Alternativas consideradas

- **En el repo, con un `.gitraptor/.gitignore` versionado** que crea el usuario o Guardrails: depende de que alguien lo cree y lo mantenga. El motor no puede garantizarlo (Q21), el archivo solo existe en un worktree y obliga a decidir cuál se lee. Descartada.
- **En el repo, con una entrada en `.git/info/exclude`** escrita por Guardrails: no se versiona y vale por clon, pero es una ruta operativa que escribe otra feature (BR-CONS-001, Q22) y el motor seguiría sin poder verificarlo. Descartada.
- **En el repo, con detección y aviso** si `settings.local.json` aparece rastreado por Git: solo detecta el problema después de que ocurre. Complementaba a las anteriores y ya no hace falta.
- **Sin nivel local** (solo perfil y equipo): pierde el ajuste por repo que fijan Q3 y Q20. Descartada.

## Consecuencias

- ✅ Imposible versionar el nivel local por accidente, sin depender de `.gitignore` ni de otra feature.
- ✅ Coherente con Q21: el motor no lee ni espera nada propio dentro del repo, salvo la configuración del equipo.
- ✅ Un valor por repo para todos sus worktrees, como pide Q3. Desaparece el problema de qué worktree tiene el archivo.
- ✅ Coherente con Q31: el nivel local no viaja a otra máquina.
- ⚠️ Rompe la convención de Claude Code, donde `settings.local.json` vive en el repo. **Mitigación:** el motor expone la ruta de cada repo y el comando de Guardrails la abre o la crea. La documentación lo explica.
- ⚠️ Si se pierde el perfil (Q26), también se pierden los ajustes locales y el umbral vuelve a 5 minutos. No se pierde trabajo del usuario (NFR-01). **Mitigación:** el archivo vive en la carpeta de configuración, separado de la base de datos del motor.
- ⚠️ El nivel local depende de la clave de repo de ADR-GRP-006: si el repo se mueve y obtiene una clave nueva, el ajuste deja de aplicarse. **Mitigación:** la que adopte ADR-GRP-006 para mover repos; mientras tanto, el umbral vuelve al del perfil y la ruta expuesta permite localizar el archivo.
- ⚠️ El PO tiene que actualizar BR-CONS-001 y BR-CONS-007 (ver Decisión).

## Validación

Con repos y perfiles temporales (ADR-GRP-006), nunca con este repo:

1. Con un `settings.local.json` en `repos/<id-repo>/` del perfil temporal, el umbral efectivo del repo es el del archivo, en todos sus worktrees.
2. Dos repos con archivos locales distintos no se mezclan.
3. Sin archivo local, el umbral es el del perfil, y si tampoco existe, 5 minutos.
4. Añadir, observar y retirar un repo no crea `settings.local.json` ni la carpeta `repos/<id-repo>/`, y no deja ningún archivo nuevo en el repo (`git status --porcelain --ignored` igual antes y después).
5. La ruta expuesta por el canal coincide con la que el motor lee.
6. Retirar un repo no borra su `settings.local.json` (Q25).

## Referencias

- [Contexto `motor-local`](../../requirements/features/motor-local/context.md): Q3, Q20, Q21, Q22, Q23, Q25, Q26, Q27, Q31; P10.
- [Reglas de negocio `motor-local`](../../requirements/features/motor-local/business-rules.md): BR-CONS-001, BR-CONS-007, BR-TIME-001.
- Historia: US-GRP-013.
- ADRs: ADR-GRP-005, ADR-GRP-006, [ADR-GRP-007](./ADR-GRP-007-configuracion-tres-niveles-formato.md).
- Decisión de Rene Bonilla, 2026-10-03, PQ-3.
