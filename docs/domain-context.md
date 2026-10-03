# Contexto del Dominio: GitRaptor

> Documento prerequisito. Define el contexto del dominio del proyecto. Los agentes usan este documento para tomar decisiones informadas sobre requerimientos, arquitectura y planificación.
>
> **Fuente**: [BRD-GRP-001](business/gitraptor-documento-de-negocio.md) v0.3. Lo que no sale del BRD va marcado `[POR VERIFICAR]`.

## Descripción del Dominio

| Campo | Valor |
|-------|-------|
| **Dominio** | Herramientas de desarrollo (DevTools) |
| **Sub-dominio** | Control de versiones con Git para desarrollo asistido por agentes de IA (visibilidad, undo y políticas sobre el trabajo de varios agentes en paralelo) |
| **Descripción** | GitRaptor es el copiloto de Git para quien programa con agentes de IA: muestra en vivo qué hace cada agente en el repo (Cockpit), permite deshacer cualquier operación (Time Machine) e impide que los agentes rompan algo (Guardrails). Se entrega como CLI/TUI y servidor MCP. Arranca como **herramienta interna** (decisión D1), compatible con un futuro modelo open-core. |

---

## Regulaciones y Compliance

> El BRD no menciona regulaciones. Al ser una herramienta interna, 100% local y sin envío de datos fuera de la máquina (NFR-03), en principio ninguna regulación sectorial aplica. `[POR VERIFICAR]` con la política corporativa de ASSA.

| Regulación | Aplica | Requisitos Clave | Impacto en Desarrollo |
|------------|--------|------------------|-----------------------|
| GDPR / leyes locales de protección de datos | No (probable) `[POR VERIFICAR]` | No se tratan datos personales de clientes. El historial de Git sí contiene nombres y correos de autores. | Ninguno mientras todo quede en la máquina local (NFR-03). Revisar si la auditoría de Fase 3 (BR-24) exporta datos de autores. |
| Políticas internas de seguridad de la información (ASSA) | Sí (probable) `[POR VERIFICAR]` | Uso de herramientas internas sobre código fuente corporativo | Confirmar si hay requisitos de aprobación o inventario de herramientas internas. |
| Licenciamiento de software (NFR-11) | Sí | Licencia permisiva u open-core apta para empresa, sin AGPL en el núcleo. Clean-room respecto a los competidores. | Restringe qué dependencias y fuentes de inspiración se pueden usar. |
| HIPAA / PCI-DSS / SOX | No | No se manejan datos de salud, pagos ni información financiera. | Ninguno. |

---

## Estándares de Interoperabilidad

| Estándar | Tipo | Uso en el Proyecto |
|----------|------|--------------------|
| Git (formato y comportamiento estándar) | Control de versiones | GitRaptor usa Git nativo, sin modelo propio de ramas. Usa el Git del sistema (≥ 2.38, NFR-07) y respeta la configuración, los hooks y las credenciales del usuario. |
| Model Context Protocol (MCP) | Integración con agentes | Los agentes consumen GitRaptor vía MCP estándar, sin APIs privadas de ningún IDE (NFR-08). |
| Conventional Commits | Convención de commits | Formato de commit que las políticas (BR-11) pueden exigir. |

---

## Datos Sensibles del Dominio

| Tipo de Dato | Clasificación | Tratamiento Requerido |
|--------------|---------------|----------------------|
| Código fuente de los repos observados | Confidencial | Nunca sale de la máquina (NFR-03). Solo se procesan los repos que el usuario autoriza (allowlist, NFR-02). |
| Historial de Git (commits, autores, mensajes, refs) | Confidencial | Igual que el código fuente. Contiene nombres y correos de autores. |
| Trabajo sin commitear (working tree, stash, archivos sin seguimiento) | Confidencial y **no recuperable desde ningún otro lugar** | Es el dato más frágil del dominio: si se pierde, no existe en otro sitio. Cero pérdida de datos (NFR-01). |
| Secretos presentes en el repo o en el entorno (archivos `.env`, credenciales de Git) | Restringido | No se leen, no se muestran y no se registran más allá de lo que Git ya hace. `[POR VERIFICAR]` alcance exacto. |
| Metadatos de sesiones de agentes (qué agente, en qué worktree, cuándo) | Interno | Local a la máquina. La telemetría es opt-in (NFR-03). |

---

## Actores y Roles del Dominio

| Actor | Tipo | Descripción | Permisos Típicos |
|-------|------|-------------|------------------|
| Desarrollador orquestador ("agent wrangler") | Humano | Persona que lanza y supervisa varios agentes de IA en paralelo sobre uno o varios repos. En el MVP es una sola persona (D3), que además es producto, revisor e integrador. | Todo: observar, registrar agentes, aprobar, integrar, deshacer y definir políticas. |
| Dev junior o semi-senior con agentes | Humano | Usa Claude Code o Cursor y busca una red de seguridad. Persona secundaria en el MVP interno. `[POR VERIFICAR]` si hay usuarios así en los pilotos. | Observar, deshacer. |
| Tech lead / revisor | Humano | Revisa el trabajo de humanos y agentes. En el MVP coincide con el desarrollador orquestador (D3). | Observar, revisar, aprobar o descartar. |
| Agente Claude Code | Sistema (usuario no humano) | Agente de IA soportado en el MVP (D2). Trabaja en un worktree o rama y puede usar Git crudo o GitRaptor vía MCP. | Lo que permitan las políticas del repo (BR-11). |
| Agente Cursor | Sistema (usuario no humano) | Agente de IA sin soporte completo en el MVP (D2 revisada): se acepta como "otro agente" mediante registro explícito; tendrá soporte después de Codex. Además es el editor del humano. | Lo que permitan las políticas del repo (BR-11). |
| Codex, Copilot | Sistema | Fuera del MVP; se soportan en fases posteriores. | — |
| Git del sistema | Sistema | Fuente de la verdad sobre el estado del repo. GitRaptor no lo reemplaza. | — |

---

## Glosario del Dominio

| Término | Definición | Contexto de Uso |
|---------|-----------|-----------------|
| Agente | Herramienta de IA que modifica código de forma autónoma (Claude Code, Cursor). | Todo el producto. |
| Sesión de agente | Periodo en el que un agente concreto trabaja sobre un worktree o rama. | Motor local, Cockpit, Time Machine. |
| Worktree | Directorio de trabajo adicional de un mismo repo Git, con su propia rama. Patrón típico: un agente por worktree. | Motor local, Cockpit. |
| Working tree | Estado de los archivos en un directorio de trabajo, incluido lo no commiteado. | Motor local, Time Machine. |
| Rama base | Rama sobre la que se integra el trabajo de los agentes (normalmente `main`). | Cockpit, Guardrails. |
| Snapshot | Punto recuperable del estado del repo, incluido el trabajo sin commitear. | Time Machine. |
| Política / guardrail | Regla por repo que limita lo que un agente puede hacer con Git. | Guardrails, MCP. |
| Dogfooding | Usar GitRaptor para construir GitRaptor. | KPIs. |

---

## Restricciones del Dominio

### Legales
- Licencia sin AGPL en el núcleo y desarrollo clean-room respecto a competidores (NFR-11).

### Contractuales
- No se conocen. `[POR VERIFICAR]`

### Operativas
- Uso 100% local, en la máquina del desarrollador (NFR-03). No hay servidor, ventanas de mantenimiento ni SLA con clientes.
- Multiplataforma real: Windows, macOS y Linux (BR-03, NFR-06).
- Equipo: una persona orquestando agentes de IA, sin equipo humano adicional (D3). La planificación va por historias pequeñas y verificables, no por velocity.
- Agentes soportados en el MVP: solo Claude Code (D2, revisada el 2026-10-03). Después, Codex y luego Cursor.

---

## Notas

- Este documento es un **prerequisito** para los agentes AADD. Sin él, los agentes no pueden tomar decisiones informadas sobre seguridad, compliance o datos sensibles.
- Actualizar cuando cambien regulaciones, se agreguen actores (p. ej. Codex o Copilot) o se identifiquen nuevos datos sensibles.
- Los agentes usarán este documento para:
  - **PO**: validar que las User Stories consideren restricciones del dominio.
  - **Arquitecto**: diseñar ADRs y RNFs alineados con compliance.
  - **SM**: identificar riesgos de capacidad relacionados con el dominio.
