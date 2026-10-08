# Third-party material shipped in the image

Rust crate dependencies and their licences are checked by `cargo deny check`
(see `deny.toml`). Non-crate material that is embedded in the kernel image:

| Material | Where | Licence | Notes |
|---|---|---|---|
| Inter 4.0 (Regular, Medium, SemiBold), by The Inter Project Authors, <https://github.com/rsms/inter> | `assets/fonts/Inter-*.subset.ttf`, embedded by `kernel/src/text.rs` | SIL Open Font License 1.1 (`assets/fonts/OFL.txt`) | Subset to Basic Latin, Latin-1 (Portuguese accents), common punctuation, arrows and keyboard symbols with `tools/subset-font.sh` (fonttools `pyftsubset`; hinting and unused tables dropped). The subsets keep the original copyright and licence name records. Not sold on its own and not renamed as a standalone font, as the OFL requires. |
| JetBrains Mono (Regular), by The JetBrains Mono Project Authors, <https://github.com/JetBrains/JetBrainsMono> | `assets/fonts/JetBrainsMono-Regular.subset.ttf`, embedded by `kernel/src/text.rs` for the terminal and the editor | SIL Open Font License 1.1 (`assets/fonts/OFL-JetBrainsMono.txt`) | Subset to Basic Latin, Latin-1, common punctuation, arrows and box drawing with `tools/subset-font.sh` (second argument; hinting, kerning and layout tables dropped: every glyph advances 600/1000 em). Licence and copyright records kept; not sold on its own, not renamed. |

The OFL permits embedding in software and redistributing the (modified, subset)
font files together with the licence text; the licence file travels with the
fonts in `assets/fonts/`.
