---
id: US-GRP-020
title: "El desarrollador ve los repos que aparecen en sus carpetas de código sin tener que añadirlos uno a uno"
type: us
status: draft
priority: medium
created: 2026-10-07
updated: 2026-10-07
feature: motor-local
source: inline
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-001
    - US-GRP-015
    - US-GRP-022
    - US-CKP-025
tags:
  - motor-local
  - repos-descubiertos
  - carpetas-de-codigo
  - primer-uso
  - should
---

# US-GRP-020: El desarrollador ve los repos que aparecen en sus carpetas de código sin tener que añadirlos uno a uno

## Descripción

**Como** desarrollador que clona repos con herramientas distintas (la terminal, VS Code, GitKraken), **quiero** declarar una vez mis carpetas de código y que GitRaptor me proponga cada repo que aparece en ellas, **para** no olvidar observar un repo en el que luego trabajan mis agentes.

**Valor**: hoy un repo clonado fuera de GitRaptor queda sin observar hasta que el desarrollador se acuerda de `raptor repo add`; mientras tanto la Time Machine no lo protege. Proponerlo cierra ese hueco sin quitarle al humano la decisión de observar (BR-AUTH-001).

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO, sobre la propuesta A1 aceptada por Rene Bonilla (2026-10-06). Aceptar o descartar un repo descubierto va en US-GRP-022; la pregunta en la TUI, en US-CKP-025.
>
> **Decisión de Rene (2026-10-07)**: la notificación nativa del sistema ("¿Observar *x*?") queda fuera del MVP; en el MVP el aviso solo sale en la TUI (Q46). Ratifica el supuesto del descarte por ruta: un repo descartado no se vuelve a preguntar (BR-AUTH-003). **Decisión de Rene (2026-10-07)**: la carpeta personal puede ser una raíz ("que el usuario dé el path de la ruta que quiere monitorear"); cierra el pendiente del PR #142. Una raíz amplia se declara con aviso de coste y confirmación, y dentro de cualquier raíz hay exclusiones fijas (BR-AUTH-003, condición 2; decisión del orquestador validada por el Arquitecto).

## Reglas cubiertas

BR-AUTH-003 (raíces declaradas, primer nivel, descubrir no es observar) · BR-AUTH-001 (enmienda 2026-10-07) · BR-CONS-001 · SEC-15 (raíces por comando reservado) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001 (añadir un repo es lo que hace aceptar uno descubierto).
- **Externas**: el Cockpit presenta el aviso (US-CKP-025); el MCP no expone nada de esta historia (F-001-05).
- **Consumo**: que un repo descubierto no consuma recursos de observación y lo que cuesta vigilar las raíces lo fija el Arquitecto (observación por niveles); transversal (lo define el Arquitecto).
- **Transversal**: verificado en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma**. Cero escrituras en el repo descubierto (BR-CONS-001).

## Criterios de Aceptación

**Escenario: Declarar una raíz lista sus repos sin observarlos**

Dado la carpeta "~/code" con los repos "shop" y "api" en su primer nivel y ningún repo observado
Cuando el desarrollador declara "~/code" como raíz de código
Entonces "shop" y "api" figuran como descubiertos en `raptor repo discovered`
  Y el motor emite un único aviso con el número de repos descubiertos
  Y ninguno de los dos está observado ni tiene eventos registrados

**Escenario: Un repo que aparece después se propone**

Dado "~/code" declarada como raíz
Cuando aparece en "~/code" el repo "billing", clonado con cualquier herramienta
Entonces "billing" figura como descubierto con la propuesta "¿Observar billing?"
  Y "billing" no está observado hasta que el desarrollador lo acepte

**Escenario: Solo se descubre el primer nivel de cada raíz**

Dado "~/code" declarada como raíz y el repo "shop" observado
Cuando aparecen el repo "~/code/clientes/acme" y un worktree de "shop" en "~/code/shop-feat"
Entonces ninguno de los dos figura como descubierto

**Escenario: Una ruta que no puede ser raíz se rechaza**

Dado el desarrollador sin raíces declaradas
Cuando intenta declarar como raíz la raíz del sistema de archivos, la carpeta que contiene las carpetas personales, una ruta que no existe o una carpeta que ya es un repo
Entonces cada intento se rechaza con su motivo y lo que puede hacer en su lugar
  Y la lista de raíces sigue vacía

**Escenario: Una raíz amplia, como la carpeta personal, exige confirmación**

Dado el desarrollador sin raíces declaradas, en su propia terminal
Cuando declara como raíz su carpeta personal entera
Entonces la CLI avisa de que es una raíz amplia y de su coste, y pregunta "[s/N]"
  Y si responde "N", pulsa Intro o no hay terminal interactiva, la lista de raíces sigue vacía
  Y si responde "s", la carpeta personal queda declarada como raíz

**Escenario: Dentro de una raíz no se miran las carpetas excluidas**

Dado la carpeta personal declarada como raíz en macOS, con el repo "dotlab" en "~/dotlab", el repo "~/.oh-my-zsh" y un repo en "~/Library/x"
Cuando el motor lista el primer nivel de la raíz
Entonces "dotlab" figura como descubierto
  Y ni "~/.oh-my-zsh" ni nada dentro de "~/Library" figura como descubierto

**Escenario: Una raíz escrita en un archivo de configuración no se tiene en cuenta**

Dado el repo observado "shop" cuya configuración de equipo declara "~/proyectos" como raíz de código
Cuando el motor lee su configuración
Entonces no vigila ninguna carpeta por esa declaración
  Y avisa de que las raíces solo se declaran con `raptor repo roots add`

**Escenario: Un agente no puede declarar raíces**

Dado "Claude Code" conectado por MCP al repo "shop" y otra sesión de agente con una terminal abierta
Cuando el agente consulta las herramientas del MCP o pide declarar "~/proyectos" como raíz desde su terminal
Entonces ninguna herramienta permite declarar o retirar raíces
  Y la petición desde la terminal del agente se rechaza y la lista de raíces no cambia

## Requisitos Técnicos

> Arquitecto, 2026-10-07. Decisión del orquestador, validada por el Arquitecto. Diseño en ADR-GRP-010, Enmienda (2026-10-07) N6 y N8, aceptada (**Decisión de Rene (2026-10-07)**); objetivos en SEC-15 y RES-11.

- **Raíces en el índice global del perfil** (ADR-GRP-006), nunca en `settings.json`, porque un agente puede escribir un archivo del perfil. Declarar y retirar una raíz son métodos **reservados** del daemon (`discovery.root.add`, `discovery.root.remove`; SEC-03), y `discovery.roots` es de lectura. Subcomandos: `raptor repo roots`, `raptor repo roots add <ruta>`, `raptor repo roots remove <ruta>` y `raptor repo discovered`. Nada de esto existe en el perfil `mcp` (SEC-MCP-01).
- **El daemon valida la raíz** según SEC-15 y devuelve el rechazo con un motivo tipado (código, sin texto; NFR-10). La CLI pone el texto y la alternativa en en y es.
- **Raíz amplia** *(enmienda 2026-10-07, **Decisión de Rene (2026-10-07)**; decisión del orquestador validada por el Arquitecto)*: `discovery.root.add` devuelve el código tipado `root_broad` con el motivo (`home`, `volume` o el número de entradas, > 512). La CLI avisa, pregunta `[s/N]` y reintenta con `confirm_broad: true`; el daemon vuelve a validar la ruta y al solicitante. Sin TTY se rechaza. Una raíz amplia no se vigila: su primer nivel se lista cada 60 s (RES-03). Exclusiones fijas según SEC-15 y ADR-GRP-010 N6.
- **Vigilancia de primer nivel, sin recursión**: una vigilancia no recursiva por raíz (en macOS, la notificación del propio directorio; si no es viable, un listado cada 30 s) y un listado del primer nivel cada 5 min, en clase `utility`. Tope de 4.096 entradas por raíz.
- **Detección mínima**: una entrada es candidata si `.git` es un directorio con `HEAD` o un archivo `gitdir:` de 4 KiB como mucho. No se siguen enlaces, no se lee configuración y no se ejecuta nada. Se deduplica por la clave de repo, así que un worktree de un repo observado no es candidato. Un clon en curso se vuelve a mirar en el listado siguiente.
- **Contrato aditivo**: el evento `repo.discovered` (capacidad `discovery.events`) y `discovery.candidates`. Al declarar una raíz se emite un único aviso con el número de candidatos. Los candidatos se guardan en el índice global y sobreviven a un reinicio.
- **Una raíz en una configuración de repo** es una clave desconocida para ese nivel: se ignora con un diagnóstico específico (PQ-8).
- **Verificación**: el corpus de SEC-15; un repo canario hostil en la raíz que no deja ningún marcador; la suite de INF-GRP-001 (descubrir no escribe nada en la raíz ni en el repo); y un cliente bajo un agente simulado al que se le rechaza declarar una raíz.

## Diseño y Dev Spec

- **Diseño:** presentación en la CLI según DSYS-GRP-001; el aviso en la TUI es de US-CKP-025.
- **Dev Spec:** pendiente.
