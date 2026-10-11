---
name: pagespace-spreadsheets
description: >
  Creates, reads, edits and formats PageSpace SHEET (spreadsheet) pages: range and column-value reads, cell edits, formulas and supported functions, cross-sheet references, and presentation (tables, currency/percent/date columns, totals, themes, conditional formatting). Use when working on a PageSpace sheet, budget, tracker or computed table, or its formulas and formatting.
---
> **Source:** PageSpace's built-in `/spreadsheets` skill, copied verbatim from the app
> (`apps/web/src/lib/ai/skills/bodies/spreadsheets.ts`). The body below was written for the in-app agent
> and names its tools. From outside the app, use the `pagespace` CLI, `@pagespace/sdk` or
> `pagespace mcp` (see /pagespace-cli, /pagespace-sdk) — the tool mapping is in the
> `pagespace-cli` skill (`references/tool-mapping.md`). Where a tool has no CLI verb
> the mapping says so.

# Spreadsheets (SHEET pages)

A SHEET page is PageSpace's interactive spreadsheet: a single grid of cells with a formula engine. New sheets start at 20 rows x 10 columns and grow automatically when you write outside the current bounds. Like any page, a sheet can be nested anywhere and can contain child pages.

## Two write paths: values and presentation

Sheets store structured cell data, not editable text. `replace_lines` (and other line/text editing tools) **refuse SHEET pages** with "Cannot use line editing on sheets" — the stored text is a generated serialization, and editing it as text would corrupt state. Values and formulas go through `edit_sheet_cells`; presentation (how a number displays, header bands, themes, highlight rules) goes through `format_sheet` and `set_conditional_format` (below), which never change a value.

`edit_sheet_cells` contract:

- `pageId` (optional) — the sheet page to edit; defaults to the page currently in view.
- `cells` — an array of `{ address, value }` updates. Batch cells into one call, up to **500 cells per call**; split anything larger. A batch of long values can exhaust your output budget well before 500 — if a large call fails to be produced at all, halve the batch.
- `address` — A1-style, e.g. `"A1"`, `"B2"`, `"AA100"`. Case-insensitive; normalized to uppercase.
- `value` — always a string:
  - Starts with `=` → stored as a formula.
  - Empty string `""` → clears the cell.
  - Looks like a number (`"42"`, `"-3.14"`, `".5"`) → treated as a number.
  - Anything else → plain text.

Writing to an address beyond the current grid (e.g. `Z50` on a 20x10 sheet) expands the sheet to include it. The result reports how many values, formulas, and clears were applied; verify with `read_sheet` afterwards (`read_page` shows only the first rows).

To create a new spreadsheet: `create_page` with `type: "SHEET"` (it initializes an empty 20x10 grid), then populate it with one batched `edit_sheet_cells` call.

## Addressing

- Cells: one or more letters + row number — `A1`, `B12`, `AA100`. Columns run A..Z, AA..AZ, etc.; rows are 1-indexed.
- Ranges: `A1:B10` (rectangular, inclusive, either corner order). Ranges **must be bounded** — whole-column (`A:A`) and whole-row (`1:1`) references do not exist and will fail to parse.
- Absolute references `$A$1`, `A$1`, `$A1` are accepted in formulas. They evaluate identically to `A1`; the `$` only pins the row/column when a user copies cells in the UI.

## Formulas

A formula is any cell value starting with `=`, e.g. `=SUM(B2:B10)`, `=A1*1.2`, `=IF(C2>100, "over", "under")`.

### Operators

`+` `-` `*` `/` `^` (power), `&` (string concatenation), and comparisons `=` `<>` `>` `<` `>=` `<=` (returning TRUE/FALSE).

- `+` tries numeric addition; if either side is non-numeric it silently falls back to string concatenation. Use `&` when you mean concatenation.
- `/` by zero is an error.
- `=` and `<>` compare numerically when both sides coerce to numbers, otherwise by string.
- Empty cells coerce to 0 in numeric contexts; numeric-looking strings coerce to numbers; other text in a numeric operation causes an error.
- String literals use double quotes only (`"text"`). There is no escape for a double quote inside a formula string.
- Boolean literals `TRUE` / `FALSE` work inside formulas. A raw cell containing `true` is just text (though `IF` still treats the strings "TRUE"/"FALSE" as booleans).

### Supported functions — the complete list

These 43 functions (46 names counting the aliases AVG, CONCATENATE, and POW) are **everything** the engine supports. Any other name — including common Excel functions — fails with "Unsupported function NAME" and the cell shows `#ERROR`.

**Math and aggregation**

| Function | Signature | Notes |
|---|---|---|
| SUM | `SUM(v1, ...)` | Non-numeric text in the range errors the formula |
| AVERAGE / AVG | `AVERAGE(v1, ...)` | Skips blanks and text; 0 when nothing numeric |
| MIN | `MIN(v1, ...)` | 0 on empty input |
| MAX | `MAX(v1, ...)` | 0 on empty input |
| COUNT | `COUNT(v1, ...)` | Numbers, booleans, numeric strings |
| COUNTA | `COUNTA(v1, ...)` | Non-empty values |
| ABS | `ABS(n)` | |
| ROUND | `ROUND(n, [digits])` | digits defaults to 0 |
| FLOOR | `FLOOR(n, [significance])` | significance defaults to 1, never 0 |
| CEILING | `CEILING(n, [significance])` | As FLOOR |
| INT | `INT(n)` | Floor to integer |
| SQRT | `SQRT(n)` | Errors on negative input |
| POWER / POW | `POWER(base, exp)` | Same as `^` |
| MOD | `MOD(n, divisor)` | Errors when divisor is 0 |
| SIGN | `SIGN(n)` | Returns -1, 0, or 1 |
| PI | `PI()` | |
| RAND | `RAND()` | Random in [0, 1) |
| RANDBETWEEN | `RANDBETWEEN(low, high)` | Inclusive integer |

**Logical and type tests**

| Function | Signature | Notes |
|---|---|---|
| IF | `IF(cond, then, [else])` | else defaults to "". **IF does NOT short-circuit**: `=IF(B2=0, "", A2/B2)` still errors when B2 is 0. Guard with IFERROR |
| IFERROR | `IFERROR(expr, fallback)` | The only lazy function and the only working error guard |
| AND | `AND(v1, ...)` | |
| OR | `OR(v1, ...)` | |
| NOT | `NOT(v)` | |
| ISBLANK | `ISBLANK(v)` | |
| ISNUMBER | `ISNUMBER(v)` | Actual numbers only, not numeric text |
| ISTEXT | `ISTEXT(v)` | |

**Text**

| Function | Signature | Notes |
|---|---|---|
| CONCAT / CONCATENATE | `CONCAT(v1, ...)` | Chained `&` |
| UPPER | `UPPER(text)` | |
| LOWER | `LOWER(text)` | |
| TRIM | `TRIM(text)` | |
| LEN | `LEN(text)` | |
| LEFT | `LEFT(text, [n])` | n defaults to 1 |
| RIGHT | `RIGHT(text, [n])` | n defaults to 1 |
| MID | `MID(text, start, count)` | start is 1-indexed |
| SUBSTITUTE | `SUBSTITUTE(text, old, new, [instance])` | All occurrences unless instance given |
| REPT | `REPT(text, times)` | times > 10000 errors |
| FIND | `FIND(needle, haystack, [start])` | Case-sensitive; 1-indexed; **errors** on a miss (wrap in IFERROR) |
| SEARCH | `SEARCH(needle, haystack, [start])` | Case-insensitive FIND |

**Date**

| Function | Signature | Notes |
|---|---|---|
| TODAY | `TODAY()` | The string "YYYY-MM-DD" |
| NOW | `NOW()` | ISO-8601 timestamp string |
| YEAR | `YEAR(dateText)` | Errors on invalid dates |
| MONTH | `MONTH(dateText)` | 1-12 |
| DAY | `DAY(dateText)` | |

Dates are plain strings — no serial numbers and no date arithmetic (no DATE, DATEDIF, EDATE, or subtracting dates). Caveat: YEAR/MONTH/DAY parse "YYYY-MM-DD" as UTC midnight but extract in local time, so the result can be off by one near day/month boundaries.

### Error values

Every failed cell **displays** `#ERROR` — no Excel-style `#DIV/0!`, `#N/A`, `#REF!` or `#VALUE!`, and circular references render the same. The detail is in the read: each errored cell carries `error = { type = "EVAL_ERROR" | "CIRCULAR_REF", message = "..." }`. Causes: unsupported function, non-numeric operand, division by zero, FIND miss, invalid date, unavailable cross-page reference, cycle.

An errored cell's value is empty, so formulas referencing it error too. Fix the offending cell with `edit_sheet_cells` (replace the function, quote text, or break the cycle). `IFERROR(expr, fallback)` is the only in-formula way to absorb an expected error. Edge: a non-finite result (`=2^2000`) displays `#ERROR` but stores no `error` and serializes as 0.

## Cross-sheet references

Reference cells in *another SHEET page* with an `@[...]` mention inside a formula:

- Single cell: `=@[Budget 2026]:B2`
- Range: `=SUM(@[Budget 2026]:B2:B20)`
- Disambiguated by page ID: `=@[Budget 2026](pageid123):B2` — the parenthesized identifier is the target page's ID and wins over the label.

Resolution: the identifier is tried first (must be a SHEET page); otherwise the label is matched case-insensitively against sheet page titles in the drive tree. Duplicate titles are ranked (siblings of the current sheet first, then most shared ancestry, then shallowest, then position) — so **include the page ID whenever you know it**. The label must be non-empty and the target must be a SHEET page.

Important: cross-page references only resolve in the live sheet view. The save-time evaluation has no page resolver, so right after writing one a read shows `#ERROR` with "Cross-page references are not supported in this context" in `errors`. The formula is stored correctly and evaluates for users viewing the sheet — do not "fix" it by removing the reference.

## Reading a sheet

Sheets are stored as rows in the database, not as text, and they are read as rows. Two tools:

**`read_sheet`** — the tool to reach for on any sheet with real data in it. One call does a row range, a lookup by column value, or a column projection:

- `pageId` (optional) — defaults to the sheet currently in view.
- **Range:** `startRow` (1-based, the number shown in front of each row and the row in its A1 addresses) + `limit`. Page on with the `nextStartRow` the response returns.
- **Lookup:** `where: { match: "all" | "any", conditions: [{ column, op, value }] }`. Ops: `eq neq gt gte lt lte contains startsWith endsWith isEmpty isNotEmpty in`. So "the row where column C is 28605" is `where: { conditions: [{ column: "C", op: "eq", value: "28605" }] }` — never read the sheet and filter it yourself while filtering is available (the one exception is an unmigrated sheet, below).
- **Projection:** `select: ["A", "C", "D"]` returns only those columns, on range reads and filtered reads alike — use it on any wide sheet you only need a few columns of. `orderBy: [{ column, direction, numeric }]` sorts (set `numeric` or "10" sorts before "9"). `offset` pages through matches.
- **Tabs:** `tabIndex` picks a tab, and every response lists the tabs the sheet has. A tab index that does not exist is refused, never answered with tab 0's rows.
- Filters and sorts run in the database against each cell's **computed, unformatted** value, so a formula column matches on its result. `cells` gives you the DISPLAY text, so a cell shown as `$1,200.00` is matched by `1200` and `85%` by `0.85` — never build a filter by parsing the displayed string; take the number from `unformatted` (below).
- `includeFormatting: true` adds a `formatting` block: the tab's `regions` (its declared structure), `layout` (freezes, widths, heights), `columnFormats`, the explicit `cellFormats` within the rows returned, and each conditional rule as `{ id, kind, ranges, summary }`. Ask for it before you restyle a sheet. `regions` can be handed straight back to `format_sheet`; a rule entry is a DESCRIPTION, not a rule you can write — to change one, rebuild it and remove the old `id`. `cellFormats` is capped and reports `formattingTruncated` when it drops cells. Works on unmigrated sheets too.
- `startRow` and `where`/`orderBy`/`offset` are different coordinate systems and cannot be combined — the call is rejected rather than quietly ignoring one.
- Returns at most 500 rows per call, with `dimensions`, `hasMore`, and each row as `{ rowNumber, cells, formulas?, errors?, unformatted? }` plus a rendered `table`. `cells` holds display values keyed by column letter; `formulas` holds the authored formula for the cells that have one.

**`read_page`** on a SHEET gives dimensions, tabs, and the first 25 rows as a table — an orientation read. `lineStart`/`lineEnd` select **row numbers** there. Enough to confirm a small write; for anything larger use `read_sheet`.

Reading either way:

- Only non-empty cells appear. A row's `rowNumber` is its A1 row, so a row read at 417 is written with `C417`.
- Formula cells report both the computed value (in `cells`) and the formula (in `formulas`) — you never need to re-derive results, and you never lose the formula.
- Formatted cells report both the display text and the number underneath. `read_sheet` puts the machine value on the row as `unformatted` (keyed by column letter: `unformatted: { "C": 1200 }` beside `cells: { "C": "$1,200.00" }`); `read_page` keys it by A1 address, like `formulas` and `errors`. Only cells whose display differs from their value appear — unless the cell is also in `formulas` and was never computed, in which case `cells` holds the formula text. Always filter, compute and write back from that number, never from the rendered string.
- Errored cells show `#ERROR` as their value and carry the message in `errors`, keyed by A1 address in `read_page` and by column letter within the row in `read_sheet`.
- Everything except the raw inputs (formula text and literal values) is **derived and regenerated on every save**. Never try to write values, types, errors or dependencies yourself; `edit_sheet_cells` recomputes them.
- Cell values are truncated at 120 characters in the rendered `table` only, and the response says how many were cut (`tableTruncatedCells`); the structured `rows` always carry the full text. Never write a value back that you read from the table if that count is non-zero.
- A sheet whose stored document cannot be parsed is reported as **unreadable**, not as empty. If you see that, do NOT write to the sheet — its data may still be intact, and writing would replace it.
- Two refusals you may hit on older sheets, both meaning "do not write here":
  - *"Page holds text, not a spreadsheet"* — the page is a SHEET but its content is plain text or HTML. Read it with `read_page`. Writing cells would replace that text.
  - *"Sheet not migrated to row storage"* — only from a FILTERED `read_sheet`. Filtering runs in the database and this sheet's rows are not there yet; reading never writes, so the tool will not migrate it for you. Read it positionally instead (`startRow`/`limit`) and filter the returned rows yourself — the one case where local filtering is correct. Page with `nextStartRow` until `hasMore` is false, using `select` to keep pages small. Do NOT edit a cell to make filtering available: that changes the user's sheet to serve a read. It migrates on its own the next time someone genuinely edits it.

## Structuring a new sheet

- Row 1: header labels (plain text). Data starts at row 2.
- One record per row, one field per column; keep a column consistently numeric or consistently text.
- Put totals/aggregates in a clearly labeled row below the data (e.g. `A11` = "Total", `B11` = `=SUM(B2:B10)`) or a summary block to the right — and leave a blank row so the data range can grow.
- Since blank cells coerce to 0, make aggregate ranges slightly generous (`=SUM(B2:B20)` over 10 data rows is safe for SUM/AVERAGE).
- Store numbers as bare numbers (`1200`, not `"$1,200"` — currency symbols and thousands separators make the value text and break math) — set the display format instead (pitfall 7).
- Store dates as `YYYY-MM-DD` strings so YEAR/MONTH/DAY can parse them.
- Send the whole initial population — headers, data, formulas — as one `edit_sheet_cells` batch, then one `format_sheet` call declaring the table.

## Formatting: describe the table, don't paint it

`format_sheet` takes `regions` first. A region declares what an area IS — its range, which rows are headers, what each column means, which rows are totals, an accent hue — and the header band, number formats, total emphasis and palette are DERIVED at render time; you never pick a colour. A budget table, declared in one call:

```
format_sheet({ regions: [{ name: "Budget", range: "A1:D", headerRows: 1,
  columns: [{ column: "C", role: "currency", currency: "USD" }, { column: "D", role: "percent" }],
  totalRows: [40], theme: "blue", freezeHeader: true }] })
```

- Column roles: `text number currency percent date datetime id`; `currency` and `decimals` refine one. A range with no end row (`"A1:D"`) runs to the bottom of the sheet. Themes are named hues (`slate blue cyan teal green amber orange red pink purple violet indigo`).
- **Ops applied to a range do NOT cover rows added later; regions do.**
- A region costs nothing per row. `A2:A5000` as ops costs 4,999 against the cell budget (100 ops, 20,000 cells per range op, 50,000 per call); a `columnFormat` op for a whole column is free too.
- `ops` is the escape hatch for what a region cannot say: `setFormat` (range + format) for one emphasised cell, `clearFormat`, `columnFormat`, `columnWidth`, `rowHeight`, `freeze`. A `format` is `{ number: { kind, decimals, currency }, bold, italic, align, wrap, color, background, … }`.
- Precedence: column default < region < the cell's own format < conditional rule.
- `regionMode: "merge"` (default) upserts by `id` — pass back the id from `read_sheet` to restyle an existing table; `"replaceAll"` keeps only the regions in the call (an empty list clears them all).
- All-or-nothing: one bad op refuses the whole call and nothing is applied. Verify with `read_sheet` and `includeFormatting: true`.

### Conditional formatting

`set_conditional_format` rules recolour cells by what they hold and stay correct when the numbers change. Four `kind`s, each over `ranges`:

- `cell` — `operator` (`greaterThan lessThan between equal contains isEmpty isError` …) + `value` (`value2` for between) + `format`. Values compare against the **computed, unformatted** value: `"1200"`, never `"$1,200.00"`.
- `formula` — `formula` anchored at the range's top-left, e.g. `"=C2>AVERAGE(C2:C40)"`, + `format`.
- `colorScale` — `min`/`mid`/`max` anchors `{ type: min|max|number|percent|percentile, value, color }`.
- `dataBar` — an in-cell bar; `color` is required.

`mode: "append"` (default) adds; `removeRuleIds` drops rules by id. `mode: "replaceAll"` keeps only the rules in the call — read the sheet with `includeFormatting: true` first, or you discard rules you never saw.

## Common pitfalls

1. **Guessing Excel functions.** VLOOKUP, XLOOKUP, INDEX, MATCH, SUMIF, SUMIFS, COUNTIF, AVERAGEIF, TEXT, DATE, DATEDIF, EOMONTH, TEXTJOIN, SPLIT, PROPER, MEDIAN, STDEV, PRODUCT, ROUNDUP, ROUNDDOWN, TRUNC, EXP, LN, LOG, ARRAYFORMULA — none exist. Only the table above is real. For conditional aggregation, add a helper column of `IF(...)` values and SUM it.
2. **Unbounded ranges.** `=SUM(A:A)` is a parse error. Always use bounded ranges like `A2:A50`.
3. **Newlines in cell values.** A line break inside a cell value is stored correctly and survives a round trip. The grid renders it on one line unless the cell is set to wrap, so prefer single-line values for readability — but a multi-line value is safe, not destructive.
4. **Leading `=` on literal text.** Any value starting with `=` becomes a formula; there is no apostrophe-escape. Don't start plain-text values with `=`.
5. **Text in numeric ranges.** One non-numeric string inside a `SUM`/`MIN`/`MAX` range errors the whole formula (AVERAGE skips it instead). Keep data columns clean; don't mix "N/A" into number columns — leave the cell empty.
6. **FIND/SEARCH on a miss is an error**, not -1 or blank. Wrap in `IFERROR` if absence is expected.
7. **Formatted numbers.** `"$1,200"` or `"85%"` are text. Enter `1200` and `0.85`, THEN make them *display* as currency or percent with a `format_sheet` column role. Formatting changes display only, so `SUM` still works and a `read_sheet` `where` still matches `"1200"`.
8. **Cross-sheet error right after writing** ("Cross-page references are not supported in this context") is expected on save — see Cross-sheet references above.
9. **Editing the sheet as text.** Never write SheetDoc TOML via any text tool — hand-built TOML that parses incorrectly resets the sheet to empty. All writes go through the sheet tools.
10. **Reading a sheet to search it.** Paging a whole sheet through `read_page` to find one row wastes the context the sheet was supposed to save you. Ask `read_sheet` with a `where` instead.
11. **Colouring cells instead of writing a rule.** Hand-filled colour goes stale the moment a number changes; a `set_conditional_format` rule follows the values.
12. **Formatting a table cell-by-cell instead of declaring a region.** Range ops stop at today's last row; a region does not.
