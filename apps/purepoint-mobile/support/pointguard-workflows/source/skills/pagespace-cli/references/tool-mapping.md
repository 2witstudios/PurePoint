# In-app agent tool → CLI / SDK

The built-in page-type skills (`/pagespace-canvas-websites`, `-spreadsheets`, `-task-management`,
`-writing-documents`) are written for the in-app agent. Outside the app use the same operations through
the CLI, `@pagespace/sdk`, or `pagespace mcp`. Verify names with `pagespace help` / the SDK types — this
table is a guide, not a contract.

| In-app tool | CLI | SDK |
|---|---|---|
| `list_drives` | `drives list` | `drives.list` |
| `list_pages` | `pages list` / `pages tree` | `pages.list` |
| `read_page` | `pages read <id> [--start --end]` | `pages.read` |
| `create_page` | `pages create <title> <type> [parent] --drive` | `pages.create` |
| `replace_lines` | `pages replace-lines <id> --start --end --file` | `pages.replaceLines` |
| `insert_content` | no verb — rewrite the range with `replace-lines` | `pages.insertLines` |
| (delete lines) | no verb — rewrite the range with `replace-lines` | `pages.deleteLines` |
| `copy_content`, `provision_form_target` | in-app only | — |
| file/image upload | `files upload` | `uploadFile(client, …)` |
| `read_sheet` | `sheets query` / `sheets rows` / `sheets describe` | `sheets.queryRows` / `getRows` / `describe` |
| `edit_sheet_cells` | `sheets edit-cells` / `update-cells` / `append` | `pages.editCells` / `sheets.updateCells` / `appendRows` |
| `format_sheet` | `sheets format` (ops JSON) | `sheets.applyFormat` |
| `set_conditional_format` | `sheets format` with `addConditionalRule` ops | `sheets.applyFormat` |
| read formatting | `sheets formatting` | `sheets.readFormatting` |
| `create_task` / `update_task` / `delete_task` | `tasks create` / `update` / `delete` | `tasks.create` / `update` / `delete` |
| `reorder_task` | `tasks reorder` | `tasks.reorder` |
| `create_task_status` | `tasks create-status` | `tasks.createStatus` |
| `get_assigned_tasks` | `tasks assigned` | `tasks.getAssigned` |
| `set_task_trigger` / `delete_task_trigger` | no verb | `tasks.setTrigger` / `deleteTrigger` |
