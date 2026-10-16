# Terminal snapshot dependency patches

PurePoint vendors the published source files, package manifests, README, changelog and licenses for these exact crates.io versions. Cargo's `[patch.crates-io]` entries select them locally; nothing is fetched from or published to an external fork.

| Package | Original Cargo.lock registry checksum | License retained |
| --- | --- | --- |
| vt100 0.16.2 | `054ff75fb8fa83e609e685106df4faeffdf3a735d3c74ebce97ec557d5d36fd9` | `vt100-0.16.2/LICENSE` (MIT) |
| vte 0.15.0 | `a5924018406ce0063cd67f8e008104968b74b563ee1b85dde3ed1f7cb87d3dbd` | `vte-0.15.0/LICENSE-MIT`, `LICENSE-APACHE` |

The source baseline is the corresponding published registry package. Local path packages have no registry checksum in Cargo.lock; the checksums above preserve provenance. The vendored sources add approximately 356 KiB to the repository. Most lines are unchanged upstream source, including optional modules; the behavioral patch is limited to the files below.

## vte API additions

- `vte-0.15.0/src/lib.rs`: `Parser::is_ground()` includes the partial UTF-8 buffer. `Parser::screen_pending_formatted()` emits bounded equivalent pending syntax directly from the real parser's private state. This avoids a second ANSI state machine.
- `vte-0.15.0/src/params.rs`: serializes accumulated parameters/subparameters plus the unfinished saturated `u16` parameter. Private markers, intermediates, ignore states, DCS hook/passthrough behavior, ESC restarts and split UTF-8 survive replay. Parameter carry is bounded by upstream's 32 parameters and two intermediates, independent of raw digit/string length.

The carry API is deliberately **screen-only**: it omits collected OSC payload and DCS opaque data. vt100's default screen callbacks ignore those payloads; the carry restores their parsing/termination state, including ESC before a split backslash. The canonical OSC opcode is unsupported `999;` and the DCS passthrough final is unsupported `~`; these prevent discarded title/opaque suffix data from becoming a new clipboard/title/sixel command. Observer regression tests validate the action scope. Terminal titles, clipboard commands and opaque DCS side effects are not reconstructed. They were never part of `Screen` or the snapshot contract. No additional unbounded pending-byte history is retained.

## vt100 API additions

- `vt100-0.16.2/src/parser.rs`: forwards the two parser APIs above.
- `vt100-0.16.2/src/grid.rs`: serializes saved cursor, saved/current origin mode, scrolling margins and pending wrap using standard terminal sequences. An origin-relative cursor restored outside changed margins uses DECRC and relative positioning instead of a clamping CUP.
- `vt100-0.16.2/src/screen.rs`: `Screen::continuation_formatted()` restores saved drawing attributes and delegates grid continuation state. Existing upstream formatting APIs are unchanged.

The engine composes visible contents, plain-text primary scrollback, both primary/alternate buffers, continuation state and pending syntax under one output-buffer lock. Its live offset refers to exactly that capture. It switches buffers with mode 47 so reconstruction does not accidentally overwrite saved cursors via mode 1049.

## Maintenance and validation

On a dependency update, compare the five patched files against the new upstream parser/grid implementation. Keep the public screen-only scope; do not replace parser-state access with timing or byte-pattern guesses. Remove these local patches if equivalent upstream APIs become available.

`crates/pu-engine/src/output_buffer.rs` contains an every-byte-split differential matrix against uninterrupted vt100 rendering at two geometries. It covers CSI subparameters/saturation/overflow/intermediates, C0 inside CSI, CAN/SUB, ESC restarts, valid and malformed UTF-8, OSC/DCS/SOS termination, saved cursor/attributes, changed margins with origin mode, pending wrap, alternate buffers and input modes. Additional tests cover old-screen recovery after more than 4 MiB, scrollback, snapshot/offset races and resize ordering/failure. Run `cargo test -p pu-engine` and `cargo clippy --workspace --all-targets -- -D warnings` after any patch change.
