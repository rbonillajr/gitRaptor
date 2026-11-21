---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/mcp/dev-specs/US-MCP-023-dev-spec.md"
---

Lee este fichero y luego `US-MCP-023-dev-spec.md`; `US-MCP-023-dev-spec.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-21T16:19:00.110Z`
- Sha de aprobación: `b6fe2878403066b9c89553c0f25161dd2503cb3f2844686fd7580f2768534501:08faf0d279c0b289e995e143d029ee26eaa9b5394c5a610cc6496570a10aabbd:012d24b8ab4559065e396f7f71bb86923e2a59885fc5d5a758b38eb7df167c01`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES a1c3fdee5ba7>>>
```text
intent: Que un agente que trabaja por Bash lea la salida de raptor con un contrato JSON estable y versionado, y no falle en silencio cuando GitRaptor cambie de versión (BR-MCP-CONS-008).
endState: Toda salida raptor --json (salvo daemon ensure) sale por un único helper como una línea {schema_version, data} o {schema_version, error:{code,message,params,action}} con los códigos del catálogo; agent withdraw, daemon status y guard exec tienen --json; los errores de clap con --json salen en JSON; la instantánea de esquemas falla en CI si cambia un campo sin subir la versión; la referencia para agentes se genera de ella; la salida humana no cambia.
nonGoals:
  - No cambiar McpToolError ni los textos del MCP (apps/mcp).
  - No tipar ni congelar el data de las salidas del desarrollador (sessions, events, status --resources): llevan sobre pero sin esquema congelado en 1.x.
  - No envolver daemon ensure --json (protocolo interno con el lanzador de escritorio).
  - No añadir predicción de conflictos a status --json (US-MCP-024) ni la guía de agentes (US-MCP-020).
  - No cambiar la salida humana (sin --json) ni sus códigos de salida.
```
<<<FIN-DATOS-NO-CONFIABLES a1c3fdee5ba7>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 27177ba68e9a>>>
```text
[1] id: V1
statement: Cuando un agente ejecuta status, undo, redo, agent register o daemon status con --json en el repo habilitado, la salida es una sola línea JSON con schema_version 1.0 y data, y data se deserializa en el tipo del contrato
verify: cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^every_agent_subcommand_declares_the_contract_version: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json every_agent_subcommand_declares_the_contract_version -- --exact && cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^agent_outputs_deserialize_into_their_contract_type: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json agent_outputs_deserialize_into_their_contract_type -- --exact
tests:
  - apps/cli/tests/us_mcp_023_cli_json.rs

[2] id: V2
statement: Cuando un agente ejecuta sessions, timeline o guard log con --json recibe command-unavailable, y con agent withdraw o guard exec -- git commit recibe reserved-command del motor, todo en JSON con código, motivo y acción, salida distinta de 0, stderr vacío y sin ejecutar nada
verify: cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^outside_the_agent_view_is_a_json_refusal: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json outside_the_agent_view_is_a_json_refusal -- --exact && cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^an_agent_guard_exec_is_a_reserved_command_refusal: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json an_agent_guard_exec_is_a_reserved_command_refusal -- --exact
tests:
  - apps/cli/tests/us_mcp_023_cli_json.rs

[3] id: V3
statement: Cuando el desarrollador ejecuta agent withdraw --json, daemon status --json y guard exec --json, cada uno devuelve el sobre con la versión y su contenido (registro retirado, estado del motor, código de salida de Git)
verify: cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^the_developer_withdraws_with_json: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json the_developer_withdraws_with_json -- --exact && cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^the_developer_reads_daemon_status_and_runs_guard_exec_with_json: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json the_developer_reads_daemon_status_and_runs_guard_exec_with_json -- --exact
tests:
  - apps/cli/tests/us_mcp_023_cli_json.rs

[4] id: V4
statement: Cuando el motor no puede arrancar, raptor status --json devuelve en stdout un JSON con la versión, el código engine-unavailable, el motivo y la acción, salida distinta de 0 y ningún texto fuera del JSON
verify: cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^an_engine_that_cannot_start_answers_in_json: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json an_engine_that_cannot_start_answers_in_json -- --exact
tests:
  - apps/cli/tests/us_mcp_023_cli_json.rs

[5] id: V5
statement: Cuando un agente ejecuta status --json en un repo observado pero no habilitado, recibe repo-not-enabled con su acción, en JSON, sin ningún dato de ese repo
verify: cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^a_repo_not_enabled_gives_no_data_in_json: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json a_repo_not_enabled_gives_no_data_in_json -- --exact
tests:
  - apps/cli/tests/us_mcp_023_cli_json.rs

[6] id: V6
statement: Cuando una versión nueva quita o renombra un campo de una salida del agente sin subir el mayor, o añade uno sin subir el menor, la prueba de compatibilidad contra la instantánea falla; añadir un campo con el menor subido es compatible
verify: cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^compat_removing_or_renaming_a_field_is_breaking: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract compat_removing_or_renaming_a_field_is_breaking -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^compat_adding_a_field_is_additive: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract compat_adding_a_field_is_additive -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^compat_narrowing_a_type_or_making_optional_is_breaking: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract compat_narrowing_a_type_or_making_optional_is_breaking -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^compat_removing_an_enum_value_is_breaking: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract compat_removing_an_enum_value_is_breaking -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^the_schemas_match_the_snapshot_for_this_version: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract the_schemas_match_the_snapshot_for_this_version -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^each_output_schema_describes_its_data_not_the_envelope: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract each_output_schema_describes_its_data_not_the_envelope -- --exact
tests:
  - crates/api/tests/us_mcp_023_cli_contract.rs

[7] id: V7
statement: Cuando se busca el contrato JSON de la CLI, la referencia para agentes generada de los esquemas lista por cada subcomando de la vista del agente sus campos, su versión y los códigos de error que puede devolver, y coincide con el archivo del repo
verify: cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^the_agent_reference_is_generated_from_the_schemas: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract the_agent_reference_is_generated_from_the_schemas -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^every_agent_output_lists_fields_version_and_codes: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract every_agent_output_lists_fields_version_and_codes -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^the_version_constants_agree: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract the_version_constants_agree -- --exact && cargo test -p gitraptor-api --test us_mcp_023_cli_contract -- --list 2>/dev/null | grep -q '^cli_only_codes_are_kebab_and_disjoint_from_the_mcp_codes: test' && cargo test -p gitraptor-api --test us_mcp_023_cli_contract cli_only_codes_are_kebab_and_disjoint_from_the_mcp_codes -- --exact
tests:
  - crates/api/tests/us_mcp_023_cli_contract.rs

[8] id: V8
statement: Cuando un comando recibe argumentos inválidos y argv lleva --json, sale invalid-args en JSON con salida 2 sin repetir el valor tecleado; sin --json el error de clap es el de hoy
verify: cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^a_usage_error_with_json_is_json: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json a_usage_error_with_json_is_json -- --exact && cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^a_usage_error_does_not_echo_the_typed_value: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json a_usage_error_does_not_echo_the_typed_value -- --exact
tests:
  - apps/cli/tests/us_mcp_023_cli_json.rs

[9] id: V9
statement: Cuando el nombre del worktree o la rama del repo lleva ANSI, bidi o anchura cero, llega en el JSON como dato escapado (SEC-12) y la salida es una sola línea compacta; sin --json, status, sessions y events conservan su salida humana de hoy (sin sobre ni versión)
verify: cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^hostile_names_arrive_escaped_in_json: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json hostile_names_arrive_escaped_in_json -- --exact && cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^every_json_output_is_one_line: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json every_json_output_is_one_line -- --exact && cargo test -p gitraptor-cli --test us_mcp_023_cli_json -- --list 2>/dev/null | grep -q '^without_json_the_output_is_unchanged: test' && cargo test -p gitraptor-cli --test us_mcp_023_cli_json without_json_the_output_is_unchanged -- --exact
tests:
  - apps/cli/tests/us_mcp_023_cli_json.rs

[10] id: V11
statement: Ninguna salida --json de la CLI (salvo daemon ensure) se imprime fuera del helper json_out: no quedan to_string_pretty ni println del valor crudo del daemon en apps/cli/src fuera de json_out y de los tests
verify: ! grep -rnE 'to_string_pretty|println!\("\{value\}"\)' apps/cli/src --exclude=json_out.rs --exclude='*tests*.rs'
tests:
  (ninguno)
```
<<<FIN-DATOS-NO-CONFIABLES 27177ba68e9a>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES aa8286a48bd4>>>
```text
docs/requirements/features/mcp/dev-specs/US-MCP-023-dev-spec.md
```
<<<FIN-DATOS-NO-CONFIABLES aa8286a48bd4>>>

planSha: `b6fe2878403066b9c89553c0f25161dd2503cb3f2844686fd7580f2768534501` (base de la cadena de re-planes)

Léelo en `US-MCP-023-dev-spec.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 2fc37c11b86b>>>
```text
docs/dev-briefs/us-mcp-023-cli-json-contract.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 2fc37c11b86b>>>

contractSha: `08faf0d279c0b289e995e143d029ee26eaa9b5394c5a610cc6496570a10aabbd`

## Tests de criterio

testsSha: `012d24b8ab4559065e396f7f71bb86923e2a59885fc5d5a758b38eb7df167c01`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 50917b284209>>>
```text
173288033b4b464414ea4bd46fa6784c604209d8df6392be086a2dbb4b6b6736  apps/cli/tests/us_mcp_023_cli_json.rs
1c6334104749509a031924043676e618c3648484a830065c9af625dd58cd84ce  crates/api/tests/us_mcp_023_cli_contract.rs
```
<<<FIN-DATOS-NO-CONFIABLES 50917b284209>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 2 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `b6fe2878403066b9c89553c0f25161dd2503cb3f2844686fd7580f2768534501`
- contractSha: `08faf0d279c0b289e995e143d029ee26eaa9b5394c5a610cc6496570a10aabbd`
- testsSha: `012d24b8ab4559065e396f7f71bb86923e2a59885fc5d5a758b38eb7df167c01`
- approvalSha: `b6fe2878403066b9c89553c0f25161dd2503cb3f2844686fd7580f2768534501:08faf0d279c0b289e995e143d029ee26eaa9b5394c5a610cc6496570a10aabbd:012d24b8ab4559065e396f7f71bb86923e2a59885fc5d5a758b38eb7df167c01`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
