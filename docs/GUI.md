# Look and operation of the user interface

Overhaul of 2026-10-30. All icons are MrPDF's own drawings
(`src/ui/icons.rs`); nothing is taken from other programs.

## Icons

* One stroke width (`size / 13`), slightly rounded corners, outline in the
  theme's icon colour ("ink").
* At most one topic colour per icon:

  | Colour | Topic |
  | --- | --- |
  | blue | pages, view, navigation |
  | green | add, insert, check passed |
  | red | delete, remove |
  | amber / orange | comments and drawing tools |
  | purple | form fields, signatures |
  | teal | convert, export/import, measure |

* A round badge in the lower right corner tells related page icons apart:
  `+` (insert, new, duplicate), `×` (delete), arrow (extract, export,
  import, insert from file), pen (save as).
* Paper is white in light mode and dark grey in dark mode.
* Disabled icons: the faded ink also turns the topic colours grey.
* New icons for commands that used to share one: the six align and eight
  distribute variants, same width/height/size, flip horizontal/vertical, the
  four stacking orders, cut, paste, duplicate, edit text, edit objects, spell
  check, calculator, barcode, rotate 180°.
* The test `icon_gallery` (tests/ui.rs) renders every icon at 26 and 16 px in
  light and dark mode into `target/ui-screenshots/icons-*.png`.

## Ribbon

* Fitting into narrow windows, in steps: 0 full size → 1 dense (large buttons
  narrower, less space between groups) → 2 small buttons without labels →
  3… groups collapse from the right into drop-down buttons. The first group
  of a tab (the most used commands) never collapses.
* Group captions are centred under their group; more space between groups.
* Active tool / selected button: accent fill, outline and an accent bar under
  large buttons. Hover: light accent tint; pressed: stronger tint.
* Every tab has a topic colour: a bar under the active tab, a faint one on
  hover.
* The contextual tab "Anordnen" now shows small buttons with their own icons.
* The test `ribbon_tour` saves every tab in light and dark mode
  (`ribbon-NN-light/dark.png`).

## Operation

* One-letter tool keys (no modifier key, no text field focused, not while
  "Textkorrektur" takes typed letters): H Hand, V Textauswahl,
  A Anmerkungen bearbeiten, Z Vergrößerung, G Bildschirmfoto, E Text
  bearbeiten, O Objekte bearbeiten, M Text hervorheben, U Unterstreichen,
  K Durchstreichen, N Kommentar, T Schreibmaschine, X Textfeld, R Rechteck,
  L Linie, P Bleistift, S Stempel. The keys are shown in the tooltips
  (`TOOL_KEYS` in `src/ui/tools.rs`).
* Undo/redo name the step: tooltip "Rückgängig: Seiten löschen (Strg+Z)" and
  the same text in the status bar after undoing (`src/ui/undo_names.rs`
  translates the core's step labels).
* Error messages in plain words: file not found, no access (read-only or open
  elsewhere), page missing; errors of the PDF libraries say that the step did
  not work and the document was not changed, with the technical detail last.
* Tooltips: name, shortcut and, for the main commands, one sentence on what
  the command does (`explain` in `src/ui/app/commands.rs`).

## Comments: selecting and moving (2026-10-30)

* "Anmerkungen bearbeiten" (A): dragging on empty page area draws a
  selection rectangle; comments whose centre lies inside are selected.
  Shift+drag adds, Shift+click adds or removes one comment
  (`DocTab::multi_annots`, valid while its first entry is `selected_annot`).
* Dragging a selected comment moves the whole selection; Entf deletes it.
  One undo step each ("Anmerkungen verschieben").
* While dragging, the comments are shown at the new place at once: the page
  area is rendered with and without comments, the difference is the comment
  picture (`draw_move_preview` in `src/ui/canvas.rs`).
* Comment edits only change their own page: `Document::page_revision` keeps
  the rendered pictures, texts and object lists of all other pages, so only
  one page is redrawn after a move instead of every page and thumbnail.
* A rectangle of "Objekte bearbeiten" around comments says which tool
  selects them (comments are not page content).

## Style presets, undo list, speed, drawing (2026-10-31)

* Comment tool buttons have a small arrow in the lower right corner.
  Holding the mouse button down (0.4 s) opens a palette of style
  presets: Rot, Violett, Indigo, Blau, Grün, Gelb and Schwarz (Grau for
  highlights and comments); closed shapes also offer "… Rahmen" (outline
  only). The palette stays open after releasing the button; choosing a
  preset sets the tool's style and picks the tool. "Formatpalette für
  Anmerkungen…" opens "Formate für Anmerkungen"
  (`src/ui/app/style_flyout.rs`). Colours are our own values.
* "▾" next to Rückgängig/Wiederholen lists the steps; hovering marks a step
  and all newer ones, a click takes them back in one go
  (`Document::undo_steps`, the document is loaded once).
* Comment edits no longer re-read the size of every page after reloading:
  moving a comment in a 274 MB / 1476 page file went from 2.95 s to 0.26 s
  (timings with real files: `tests/real_files.rs`, ignored by default).
* Application icon (2026-11-01): Mr. M' drawing `assets/mrpdf-icon-source.webp`
  is prepared by `cargo run --release --example make_icon` into
  `assets/mrpdf.png` (256 px, transparent corners) and `assets/mrpdf.ico`
  (16–256 px). Used for the window and taskbar (`main.rs`), the logo inside
  the program (`Icon::Logo`, drawn from the PNG with mipmaps) and the
  program file (`build.rs` adds icon and version information with the MinGW
  resource compiler).
* One program window (2026-11-01): a second start hands its files to the
  running MrPDF, which opens them as tabs and comes to the front; the second
  start ends (`src/instance.rs`). Since 2026-11-02 through a named pipe
  `\\.\pipe\MrPDF-<hash>` on Windows (remote clients rejected, default
  security: other users may not write) and a Unix socket in the private
  folder `ipc/` (mode 0700) of MrPDF's data folder on Linux and macOS; no
  network port. `--new-window` starts a separate window anyway.
* Drawing goes through wgpu (DirectX on Windows) instead of OpenGL, which
  showed brief black flashes on Windows; without a usable driver MrPDF falls
  back to OpenGL, `--opengl` or `MRPDF_RENDERER=opengl` forces it.
