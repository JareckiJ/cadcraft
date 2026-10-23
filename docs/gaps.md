# Where CADCraft falls short of AutoCAD

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-23 · **Change:** minor (DWG fidelity: Top 10 row 1 aligned with FF1 closed) · **Target:** Autodesk AutoCAD 2027

Every known shortfall, one row each, ranked within each section by user impact. This is the work
list: agents pick from here (top of a section first), and remove or update a row in the same PR
that closes it. Numbers and weights live in [target-app-parity.md](target-app-parity.md); hours
are Opus 5.5 agent-hours, calibrated as explained there. "Est." means a judgement from the cited
evidence. Rows marked **[alpha blocker]** are what fails the [alpha gate](roadmap.md#alpha-gate);
do them first.

## Top 10 across everything

| # | Gap | Why it ranks here | Est. | Doc |
|---|---|---|---|---|
| 1 | ~~**[alpha blocker]** **Saving DWG drops what we don't model**~~ **(done)**: unknown entities and objects (with their CLASS records) roundtrip through DXF and DWG via raw DWG stream cache bridge; foreign xdata, extension dictionaries, persistent reactors, DICTIONARYVAR, XRECORDs, layer states, colour books, table records, blocks and native MULTILEADERs preserved through DWG (#528, #459, FF1) | Opening a colleague's DWG and saving it still deletes some content: the one thing a CAD user can't forgive | 0 | [file formats](file-format-parity.md) |
| 2 | **[alpha blocker]** **No real-file test corpus and no check in AutoCAD** | All 36 file tests are synthetic; nobody has verified a CADCraft DWG/DXF opens cleanly in AutoCAD | 10–15 + human | [file formats](file-format-parity.md#tests) |
| 3 | **Ellipse and spline geometry is tessellated** (intersections, offsets, closest points; polyline offset ignores bulges) | Coordinates are subtly wrong; offsets of ellipses come out as polylines | 30–50 | [geometry](geometry-parity.md) |
| 4 | **[alpha blocker]** **No dynamic blocks** (in-place reference editing for blocks and xrefs with disk write-back, nested reference picking, dimmed locked drawing, banner, and ATTSYNC landed; xrefs, #525, groups, #417, Block Editor for static blocks, and reference/image clipping with XCLIP, IMAGECLIP and CLIP landed) | Most production drawings use them | 35–60 | [features](#features) |
| 5 | ~~**No autosave in the web build**~~ **(done)**: web build autosaves to browser localStorage with StorageHooks; desktop saves DWG with DXF fallback; AUDITCTL `.adt` log and tolerant RECOVER mode landed | Browser users lose unsaved work when the tab or the browser goes down | 0 | [features](#features) |
| 6 | ~~**No CI runs the Rust tests on pull requests**~~ **(done)**: `.github/workflows/ci.yml` runs `cargo xtask ci` on pull requests and pushes to `main` | open PRs land without a gate; regressions go unseen | 0 | [stability](#stability) |

## Features

| # | Gap | Evidence | User impact | Est. |
|---|---|---|---|---|
| F1 | ~~**[alpha blocker]** Prompt options that still print "not available yet"~~ **(done)**: -ATTEDIT one-at-a-time Position, Height, Angle, Style, and colour books in -ATTEDIT and -LAYER Color done; -RENAME of materials, plot styles and viewport configurations done. SPLINEDIT Join, Edit vertex Add and Elevate order (#559), PEDIT Edit vertex and the other SPLINEDIT options (#558) done: Edit vertex Tangent directions last for the PEDIT session only (LWPOLYLINE stores none). MTEXT Dynamic/Static columns done (#547). SPLINE, MTEXT and MLEADER options done (#408): SPLINE Object fits the polyline's vertices (no spline frame is kept) (STRETCH polygon, CP and WP, and fence selection done; TRIM/EXTEND, PLINE arc mode and Length, XLINE done (#397); OFFSET, ROTATE and FILLET options done earlier) | `cmd/blocks/attedit.rs`, `cmd/props/rename.rs` | None: closed | 0 |
| F2 | ~~XREF editing in place and clipping~~ **(done)**: XCLIP (rectangular/polygonal/polyline boundary, on/off, delete, invert, generate polyline, frame display with XCLIPFRAME), IMAGECLIP and unified CLIP landed. REFEDIT, REFSET and REFCLOSE edit block references and external references (XREFs) in place with write-back to the external drawing file on save, nested block hierarchy picking and cycling, working set management, dimmed look for locked drawing and faded copies, canvas banner with Save/Discard, and attribute sync of other references via ATTSYNC (#813). XATTACH, the External References palette, -XREF (Bind/Detach/Path/Unload/Reload/Overlay), XBIND, XOPEN, overlays, relative paths and loading at open are done (#525) | `cmd/clip.rs`, `cmd/blocks/xref.rs`, `cmd/blocks/refedit.rs`, `ui-egui/src/canvas/refedit.rs` | None: closed | 0 |
| F3 | **[alpha blocker]** (dynamic-block authoring is beta) No dynamic-block authoring: parameters and actions can't be added in the Block Editor. Dynamic blocks from other files are read and their references edited (visibility, distances, stretch and move, lookups, grips). BEDIT (with the Edit Block Definition dialog, which also starts new blocks), BSAVE, BSAVEAS and BCLOSE edit a static block definition, and every reference updates; REFEDIT edits a reference in place; no authoring palettes yet | issue #185 offers help | High: standard libraries are dynamic | 40–65 |
| F4 | ~~Autosave leftovers~~ **(done)**: web build autosaves to browser localStorage with StorageHooks; autosave writes DWG with DXF fallback; AUDITCTL `.adt` log generation; tolerant RECOVER mode in file readers (DXF and DWG salvage on corrupt lines / premature EOF) | `engine/src/autosave.rs`, `engine/src/audit.rs`, `io/src/dxf_read.rs` | None: closed | 0 |
| F5 | ~~Associative array grips and editing~~ **(done)**: grip editing of arrays (count, spacing, base, corner, fill angle, center grips), associative ARRAYPATH along path curves with spacing/count/rows, ARRAYEDIT item deletion (ERase) and RESet, and Key point base point selection | `cmd/array.rs`, `cmd/array/edit.rs`, `cmd/modify2.rs`, `cmd/gripcmds.rs` | None: closed | 0 |
| F7 | Dimensions: DIMSTYLE manager dialog landed; no smart DIM; DIMTFILL/DIMTFILLCLR, DIMALTTD/DIMALTTZ, DIMUPT and DIMTXTDIRECTION not modelled; jog height and break size not style settings; DIMBREAK breaks don't follow moved objects (style variables, DIMJOGGED, DIMJOGLINE, DIMBREAK, DIMEDIT, DIMINSPECT and QDIM modes done, #507) | `doc/src/tables.rs`, `cmd/annotate_dimedit.rs`, `cmd/annotate_qdim.rs` | Medium | 6–10 |
| F8 | ~~TOLERANCE leftovers~~ **(done)**: Geometric Tolerance dialog with symbol grid and dual-line frames, LEADER/QLEADER tolerance annotation, in-place editing of frame contents (TOLERANCE, TOLEDIT, TEXTEDIT/DDEDIT and canvas double-click) | `cmd/tolerance.rs`, `ui-egui/src/tolerancedlg.rs` | None: closed | 0 |
| F10 | MLEADEREDIT (add/remove leader), MLEADERALIGN and MLEADERCOLLECT landed, but a multileader has one landing and one block (a collection keeps the other blocks as block references beside it; MLEADERALIGN has no Options-menu preview); MLEADERSTYLE models leader line type (Straight, Spline, None), content type (None, MText, Block) and block name, inherited on new MLEADERs; block content has no attributes | `annotate.rs`, `cmd/mleader_content.rs`, `tables.rs:417` | Low-medium | 4–8 |
| F9 | MTEXT is typed in an on-canvas editor with a formatting toolbar (font, height, bold, italic, underline, overline, colour, alignment, lists, tab and special characters) and a ruler for indents and tab stops (#532); editing existing MTEXT in place via DDEDIT/MTEDIT/TEXTEDIT landed; left: the box does not lay the text out by the indents, tabs, columns or font weights while typing (bold shows wider, overline is not drawn), no justification (attachment) or columns controls in the editor, stacked fractions and other codes show as placeholders; no fields (FIELD, UPDATEFIELD) and no spell check (SPELL): open PR #551. Paragraph indents, tabs, bullets, numbering and columns landed (#547) | `ui-egui/mtext_editor.rs`, `engine/mtext_edit.rs` | Low-medium | 6–10 |
| F11 | Tables (cell styles, cell text wrap, Cell Styles tab in Table Style dialog, formulas, TABLEDIT, TABLEEXPORT, table breaking and CSV data links landed, #534): formulas aren't read from or written as AutoCAD FIELD objects (other programs see values); number formats apply to formula results only; custom cell styles, breaks and data links travel as CADCraft xdata (not through DWG); data links read CSV only (no Excel ranges, no write-back, not on the web) | `cmd/table/*`, `io/dxf_table.rs` | Low | 4–6 |
| F12 | Annotative scaling done (#468): scale list (SCALELISTEDIT), CANNOSCALE, OBJECTSCALE, ANNOALLVISIBLE/ANNOAUTOSCALE, viewport scale, DXF, status bar list. Left: per-scale positions (ANNORESET is a no-op), a viewport annotation scale apart from its viewport scale, annotative objects inside blocks and old LEADERs, AutoCAD files' per-object scale contexts (read as not annotative) | `cmd/annoscale.rs`, `doc/src/anno.rs` | Low-medium | 4–8 |
| F14 | ~~**[alpha blocker]** Plotting leftovers~~ **(done for alpha)**: Model-space page setup (`model_page`) in `Drawing`, `pagesetup` and Page Setup Manager UI; model-space PDF/PNG/SVG plots inherit device printable margins and paper size; page setup "Plot transparency" saved/restored in DXF xdata; binary AutoCAD `.ctb`/`.stb`/`.pc3` detected with diagnostic error guidance. Left: system printers via OS spooler (H3; beta work). The Plot dialog (#160), Plot Preview and PUBLISH (#186), plot styles (CTB and STB, [plot-styles.md](plot-styles.md), #410) with their end, join and fill styles, plotter devices with paper sizes and printable areas ([plotters.md](plotters.md), #564), and plotted transparency (page setup and Plot dialog option, PLOTTRANSPARENCYOVERRIDE) landed | `io/src/plotter.rs`, `cmd/plotters.rs`, `cmd/layout.rs` | Low | 0 |
| F13 | ~~Hatch leftovers~~ **(done)**: 29 original clean-room patterns; associative boundary copying maintains associativity and re-links loop sources; PICKSTYLE bit 2 reciprocal hatch/boundary selection; HATCH inherits properties (cmd & dialog); HATCHEDIT adds and removes boundaries with loop reconstruction (cmd & dialog); dialog loop preview; MPOLYGON creates composite polygon boundaries with associative hatch fills; HPGAPTOL closes gaps up to tolerance between curve ends | `doc/src/library.rs`, `cmd/hatch.rs`, `cmd/hatch/edit.rs`, `ui-egui/src/hatchdlg.rs` | None: closed | 0 |
| F15 | Viewports: MVIEW Restore puts saved viewport configurations into layouts (#626); clipped viewports move, scale, rotate, mirror, copy and erase coupled with their clip boundaries (F15 closed) | `cmd/layout/viewports.rs`, `cmd/modify.rs` | Low | 0 |
| F16 | ~~Groups: copying an entire group replicates the group for the new objects (#417); group bounding box and single grip when a group is selected (GROUPDISPLAYMODE)~~ **(done)**: GROUPDISPLAYMODE renders a dashed bounding box (mode 2) and a centre grip square (mode ≥ 1) for each selected named group; clicking the centre grip starts a GRIP stretch-move on all group members; Shift+click marks grips hot as usual; setting exposed as a sysvar and in OPTIONS; no Object Grouping dialog yet (GROUP, GROUPEDIT, UNGROUP, -GROUP, PICKSTYLE group selection and associative-hatch reciprocal selection landed, and the DXF/DWG round trip, #417) | `cmd/groups.rs`, `cmd/modify.rs`, `ui-egui/src/canvas.rs` | Low | 0 |
| F17 | Attributes (ATTSYNC, global -ATTEDIT, BATTMAN editing with verify/preset/lock-position modes and multiline switching, and DATAEXTRACTION/EATTEXT landed, #433): data extraction covers block references and attribute values (CSV or a table) only — no wizard, nested blocks, other objects' properties, `.dxe` templates or spreadsheet output | `cmd/blocks/battman.rs`, `cmd/blocks/extract.rs` | Low | 2–4 |
| F19 | MATCHPROP Property Settings dialog with AutoCAD-standard basic and special properties (material, shadow, center object); cross-space TEXTTOFRONT and HATCHTOBACK; OVERKILL compares polyline segments with lines, arcs, and 1-seg polylines (F19 closed) | `props/matchprop.rs`, `draworder.rs`, `overkill.rs`, `matchpropdlg.rs` | Low | 0 |
| F20 | ~~OPTIONS dialog leftovers~~ **(done)**: drawing-area element colours (crosshair, snap marker, grip colour customization), Right-Click Customization modal on User Preferences with time-sensitive right-click threshold and shortcut menu options, and Profiles export/import with JSON and file picker support; Display, Open and Save, Drafting, Selection, User Preferences and Profiles tabs, and the user profile survive restarts | `ui-egui/src/options/` | Low | 0 |
| F21 | MLINE, REGION and HELIX are objects since #510, with gaps: MLEDIT Merged Tee (MT) and multiline styles PURGE landed; UNION/SUBTRACT/INTERSECT only combine regions whose boundaries don't meet; regions are saved as closed polylines (no ACIS data) | `draw2/mline.rs`, `inquiry/regions.rs`, `dxf_region.rs` | Low | 4–6 |
| F22 | Inquiry leftovers (COUNT across spaces and nested blocks, the QuickCalc palette, COMPARE and COMPAREEXPORT report landed, #563; MASSPROP on regions, #510): COMPARE compares the tab shown, not every layout at once; QuickCalc keeps its history and variables for the session only | `file/compare.rs`, `palettes/quickcalc.rs` | Low | 1–2 |
| F23 | No Sheet Set Manager, eTransmit, Share, Traces, Markup Import, Activity Insights | menu items uncovered | Low-medium (team workflows) | 40–70 |
| F24 | 3D Modeling & Navigation landed: UCS management (world, origin, 3point, zaxis, x/y/z rotation, view, previous, face, named ucs/ucsman), PLAN view, ELEVATION, THICKNESS, TRANSPARENCY; 3D Views (VPOINT, 10 presets); Visual Styles (VSCURRENT, 10 styles); 3D Navigation (3D Orbit, Free Orbit, Adjust Distance, Swivel); 3D Operations (3D Move, 3D Rotate, 3D Align, 3D Mirror, 3D Array, Slice, Thicken, Interfere, ConvToSolid, ConvToSurface, XEdges, ExtractIsolines); 3D Solids & Modeling (Box, Wedge, Cone, Sphere, Cylinder, Torus, Pyramid, Polysolid, Extrude, Revolve, Sweep, Loft, Section Plane); 3D Meshes & Surfaces (3D Face, Meshbox, Meshwedge, Meshcone, Meshsphere, Meshcylinder, Meshtorus, Meshpyramid, Smooth Mesh, Revsurf, Tabsurf, Rulesurf, Edgesurf); View (Hide, UCS Icon On/Properties, Attribute Display, NavCube Settings, Light List) (menu breadth +44 to 373/491, 76% engine / 385/491, 78% total); gaps left: materials, lights, rendering | [parity-checklist](parity-checklist.md) | Medium (3D users) | 160–240 |

## UI/UX

Detail and evidence: [ui-parity.md](ui-parity.md).

| # | Gap | User impact | Est. |
|---|---|---|---|
| U4 | ~~Tab snap cycling offers the closest snap of each mode, not every candidate~~ **(done)**: `snap_candidates` gathers and cycles every candidate snap point under the cursor aperture (`snap::osnap_all`). Typed snap overrides (`END`, `MID` … `NON`), the Shift/Ctrl+right-click snap menu and Tab cycling landed (#393), the menu's Temporary track point in #441 | Low | 0 |
| U5 | ~~Multifunctional grips and grip menus~~ **(done)**: Multifunctional grip menus open on right-click and cursor hover (400ms pause) as an overlay popup; hot-grip option cycling via `Next`/`Ctrl`; extended multifunctional grips to MULTILEADER (Add Leader, Remove Leader); Add Vertex on arc segments preserves arc curvature and bulges; hot-grip options (Base point, Copy, Undo, Reference, eXit) and polyline/line/arc grip menus landed | Low-medium | 0 |
| U6 | ~~Selection: lasso and selection improvements~~ **(done)**: freehand polygonal lasso selection on press-and-drag (Window when moving right/clockwise, Crossing when moving left/counter-clockwise), lasso polygon fill and dashed/solid stroke; Selection cycling (cycles overlapping objects with SELECTIONCYCLING toggle and status bar button), CLass keyword, isolation saved with the drawing (OBJECTISOLATIONMODE 1), and status-bar isolate button landed; Press-drag windows (#63), the W, C, F, WP and CP keywords (#226), BOX, AUto, SIngle, Multiple, Group, Add, Remove and Undo, SELECTSIMILAR, and ISOLATEOBJECTS/HIDEOBJECTS/UNISOLATEOBJECTS (#428) landed | None: closed | 0 |
| U7 | ~~Right-click menus and layout management~~ **(done)**: in-place layout tab rename (double-click/menu), Move or Copy layout dialog (reorder/copy before or to end), Select All Layouts action, Paste to command line. Palette menus on Layers rows and Properties background; command menu, selection menu, command line, drawing tab and layout tab menus landed (#512) | None: closed | 0 |
| U10 | ~~SETVAR variable coverage expansion~~ **(done)**: SETVAR table covers drawing header variables, entity defaults (CECOLOR, CELTYPE, CELWEIGHT, CELTSCALE, LTSCALE, PSLTSCALE, PDMODE, PDSIZE, AUNITS, AUPREC, ANGDIR), and comprehensive dimension variables (DIMSCALE, DIMTXT, DIMASZ, DIMGAP, DIMEXE, DIMEXO, DIMDLI, DIMDLE, DIMLFAC, DIMTAD, DIMTIH, DIMTOH, DIMTOFL, DIMTIX, DIMSOXD, DIMSE1/2, DIMSD1/2, DIMDEC, DIMALT, etc.) with validation, units-aware defaults, and bare command line variable invocation | Low-medium | 0 |
| U11 | ~~View navigation / MVIEW Restore / VIEW Orthographic~~ **(done)**: MVIEW Restore puts saved viewport configurations into layouts (#626); VIEW command Orthographic option (Top, Bottom, Front, Back, Left, Right) with 3D preset view direction vectors and extents zoom landed; UCS and background settings, Zoom Realtime and Dynamic, ZOOMFACTOR, named views (VIEW) and tiled model viewports with saved configurations (VPORTS) landed | None: closed | 0 |
| U12 | Workspaces, navigation bar, CUI customisation, alias editor, ViewCube drag (#7) | Low-medium | 15–25 |
| U13 | ~~`.dxf` from Finder fails to open on macOS~~ **(done)**: macOS Finder process serial number argument (`-psn_...`) ignored in command-line argument parsing (#158); window controls fixed (#161) | None: closed | 0 |

## File formats

Detail: [file-format-parity.md](file-format-parity.md).

| # | Gap | User impact | Est. |
| FF1 | **[alpha blocker]** Round-trip what we don't model. DXF and DWG foreign data preservation: unknown entities and objects (with their CLASS records) roundtrip through DXF and DWG (raw DWG stream cache bridge); foreign xdata (hyperlinks included), extension dictionaries, persistent reactors, DICTIONARYVAR and XRECORDs, layer states, colour book colours, references remapped on save; IMAGE, VIEW/UCS elevation, groups, block descriptions, true colours, transparency; WIPEOUT and gradients; MTEXT background colours; unknown entities follow all common property edits (color, linetype, lineweight, visibility, transparency, layer); renamed layers, blocks and named tables preserve their foreign records | Done | — |
| FF2 | Multileader leftovers (native MULTILEADER read/write landed, #459): several landings per multileader (extra leader roots are folded into the lines), per-object overrides (leader colour and linetype, arrowhead symbol, text frame), annotative scale contexts; check the text placement of our MULTILEADER output in AutoCAD | Medium | 3–5 |
| FF3 | Save-version leftovers (DXF/DWG 2000–2018, binary DXF, SAVEAS `version`/`binary` and the default save version landed, #421): no R14 DWG or R12 DXF output; the Save dialog can't pick the version per file (the file picker doesn't report the chosen type; File > Default Save Format sets it); no command-line SAVEAS format prompt | Low | 2–4 |
| FF4 | Code-page leftovers (pre-2007 DXF read in their `$DWGCODEPAGE` landed, #505): Johab (ANSI_1361) and DOS 855/857/860–865/869 read as ANSI_1252; saving declares `ANSI_1252` instead of the file's code page (the text is pure ASCII, so still correct); acadrust approximates DOS code pages in pre-2007 DWG (DOS437 as 866, DOS850 as 1252) | Low | 1–2 |
| FF5 | **[alpha blocker]** Real-file corpus (`storytold/cadcraft-corpus`), open → save → reopen tests, black-box check in AutoCAD | High | 10–15 + human |
| FF6 | **[alpha blocker]** DWG throughput: a 10 MB DWG took ~2 min to fail (#57) | High for real projects | 6–10 |
| FF7 | Table record and object handles from other programs' files are renumbered on the first save (entity handles are kept; `*D` blocks no longer duplicate and once reopened, CADCraft's own files keep every handle across saves, #495) | Low | 4–6 |
| FF8 | SHX fonts (shape and big fonts) not read | Medium-high (text metrics differ) | 8–12 |
| FF9 | `.pc3` plotter configuration files; linetype shape (SHX) elements are skipped (templates and `.dwt`, user `.lin`/`.pat` and `.pgp` alias files landed, #552; `.ctb`/`.stb`, #410) | Low-medium | 5–10 |
| FF10 | PDF import and underlays (PDFATTACH); raster image attach (IMAGEATTACH). PDF export has real text in embedded TrueType subsets, PDF layers, several sheets per file and hyperlinks (#567, #571) | Medium | 20–35 |
| FF11 | DWF/DWFx, DGN, WMF, EPS, ACIS/STL and other exchange formats | Low-medium | 50–100 |
| FF12 | DWG in the web build | Medium | 6–10 |
| FF13 | R12 binary DXF, pre-R13 DWG | Low | 10–20 |
| FF14 | SVG import leftovers (shapes, paths, groups and Inkscape layers landed, #636): no CSS classes (`<style>`), `use` references, fills and gradients, text or embedded images; rounded rectangle corners (`rx`/`ry`) are ignored | Low-medium | 2–4 |

## Hardware

Detail: [hardware-parity.md](hardware-parity.md).

| # | Gap | Est. |
|---|---|---|
| H1 | 3Dconnexion SpaceMouse (needs the device and 3D views) | 6–10 + human |
| H2 | Multi-monitor: floating palettes on other screens, per-monitor DPI (PR #110 for Windows 11 DPI) | 4–6 |
| H3 | Plotters and system printers (PC3 devices) beyond PDF files | 6–10 |
| H4 | Pen tablets / digitizer (TABLET) | 3–5 + human |
| H5 | Screen magnifier tracking (#39, PR #187) | 1–2 |

## Localization

Detail: [localization-parity.md](localization-parity.md).

| # | Gap | Est. |
|---|---|---|
| L1 | Catalog covers the UI (989 rows, #36) but not engine diagnostics, CLI/MCP replies or command history | 4–6 |
| L2 | AutoCAD for Mac's other languages (zh-Hans, es, fr, ja, ko, it; de landed 2026-10-11) plus pt, hi, ar, id, vi from the standard's list | 35–55 + native review |
| L3 | Complex-script shaping for Devanagari and other Indic scripts in drawing text; RTL (mirrored) UI. Arabic-script shaping and bidi in drawing text landed (#287) | 20–30 |
| L4 | IME fixes on every platform (PRs #110–#119); vertical text styles | 6–10 |

## Stability

| # | Gap | Evidence | Est. |
|---|---|---|---|
| S1 | ~~No CI runs `cargo xtask ci` on pull requests~~ **(done)**: `.github/workflows/ci.yml` runs `cargo xtask ci` (fmt, clippy, tests, assets, layers, wasm) on PRs and push to main | `.github/workflows/ci.yml` | 0 |
| S2 | ~~`cargo fmt --check` fails on main~~ fixed on main (8fceaaf) | — | 0 |
| S3 | 40 open PRs, many community bug fixes awaiting review | `gh pr list` | 10–20 |
| S4 | Open correctness issues: extents union far-apart geometry (#166), lines not visible mid-command (#44), browser plot shows raw PDF bytes (#134) (closing a tab and browser saves fixed, #76, #133) | issues | 4–6 |
| S5 | No smoke test of every command through the UI (PR #42 proposes one) | — | 6–10 |

## Platforms

| # | Gap | Est. |
|---|---|---|
| P1 | Web build: plot shows raw PDF bytes (#134), no DWG (File > Open and save downloads fixed, #55, #133) | 3–5 |
| P2 | Windows and Linux get little runtime testing; Linux Mint mouse delay (#25); Windows magnifier (#39) | 6–10 |
| P3 | macOS file associations: opening `.dxf` from Finder fails (#158) | 1–2 |

## Ecosystem and automation

| # | Gap | Est. |
|---|---|---|
| E1 | AutoLISP beyond the core (#758 landed the language, `command`, entity lists, selection sets, `get…` input, APPLOAD, `(load)`, `c:` commands and `acad.lsp`/`acaddoc.lsp` at startup): Visual LISP ActiveX (`vlax-`, `vla-`), DCL dialogs, reactors, an APPLOAD dialog with a Startup Suite, UCS-aware `trans` | 30–50 |
| E3 | Express Tools equivalents (TXT2MTXT, BREAKLINE, BURST, NCOPY exists, SUPERHATCH, Move/Copy/Rotate) | 30–50 |
| E4 | Plug-in API (owner decision: Rust/WASM plug-ins rather than ObjectARX/.NET) | 30–50 |
| E5 | CUI customisation and alias (`.pgp`) editor | 15–25 |

## AI features

| # | Gap | Est. |
|---|---|---|
| A1 | Smart Blocks equivalents (placement suggestions, replace, detect and convert, search), using local open models | 25–50 + model licensing |
| A2 | An in-app assistant (AutoCAD has Autodesk Assistant); CADCraft's MCP server already lets any agent drive it | 10–20 |
| A3 | Markup Assist (import markups from PDFs/images) | 10–20 |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-19 | minor | CI workflow `.github/workflows/ci.yml` runs `cargo xtask ci` on PRs and pushes to main (S1 closed) |
| 2026-10-19 | minor | In-place external reference (XREF) editing with write-back to source drawing file on save, nested reference hierarchy picking and cycling (F2 closed; remaining to alpha ≈ 65–100 h) |
| 2026-10-19 | minor | Model-space page setup, margins, plot transparency in DXF, binary style error reporting (F14 closed for alpha; Alpha Gate: layers/layouts/plotting passes; remaining to alpha ≈ 70–110 h) |
| 2026-10-19 | minor | In-place reference editing (F2 narrowed): dimmed appearance for locked geometry outside the working set, canvas banner with Save/Discard and working set management, and REFCLOSE attribute synchronization (ATTSYNC) |
| 2026-10-19 | minor | Tab snap cycling candidates gather all candidate snaps under the aperture, not just one per mode (`snap::osnap_all`; U4 closed) |
| 2026-10-17 | minor | 3D Modeling Primitives (Box, Wedge, Cone, Sphere, Cylinder, Torus, Pyramid, Polysolid), Meshes (Meshbox, Meshwedge, Meshcone, Meshsphere, Meshcylinder, Meshtorus, Meshpyramid, Smooth Mesh, Revsurf, Tabsurf, Rulesurf, Edgesurf), Extrude, Revolve, Sweep, Loft, Section Plane, 3D Operations (Slice, Thicken, Interfere, ConvToSolid, ConvToSurface, XEdges, ExtractIsolines), 3D Navigation (3D Orbit, Free Orbit, Adjust Distance, Swivel), Format (Thickness, Transparency), UCS Face, and MPolygon (F13, F24 narrowed); menu breadth +44 to 371/491 (76% engine, 78% total) |
| 2026-10-17 | minor | 3D Views (VPOINT, 10 view presets), Visual Styles (VSCURRENT, 10 styles), and Hatch dialog controls (Inherit Properties, Add/Remove boundaries in edit mode, Preview); menu breadth +19 to 315/491 (64%) |
| 2026-10-17 | minor | 3D Foundations: UCS management (world, origin, 3point, zaxis, rotate, view, previous, named), PLAN view, ELEVATION, THICKNESS, UCS-aware coordinates and `*` WCS override (F24 narrowed, menu breadth +14 to 60%) |
| 2026-10-17 | minor | Hatch associativity: boundary copying remaps loop sources, PICKSTYLE bit 2 reciprocal selection, 6 new original patterns (29 total), HATCH Inherit, HATCHEDIT add/remove boundaries (F13 narrowed) |
| 2026-10-17 | minor | MATCHPROP Property Settings dialog, material/shadow/center settings; cross-space TEXTTOFRONT/HATCHTOBACK; polyline segment OVERKILL (F19 closed) |
| 2026-10-17 | minor | Geometric Tolerance dialog, in-place frame editing, and LEADER/QLEADER tolerance annotation (F8 closed) |
| 2026-10-17 | minor | Clipped viewports move, scale, rotate, mirror, copy and erase coupled with their clip boundaries (F15 closed) |
| 2026-10-17 | minor | Unstub prompt options (F1 closed): -ATTEDIT Position, Height, Angle, Style, and colour books in -ATTEDIT/-LAYER Color; -RENAME for materials, plot styles and viewport configurations |
| 2026-10-12 | minor | Object transparency (entity, ByLayer, ByBlock) is drawn: on screen (TRANSPARENCYDISPLAY, on by default), in PNG/SVG exports, and in plots when the page setup's "Plot transparency" is on (PDF alpha, PNG blending, SVG opacity; PLOTTRANSPARENCYOVERRIDE): F14 narrowed |
| 2026-10-12 | minor | Hatch and Gradient dialog (HATCH, GRADIENT; HPDLGMODE) and Hatch Edit dialog (HATCHEDIT, right-click, double-click a hatch): F13 narrowed to Preview, Inherit Properties and boundary add/remove in edit mode |
| 2026-10-12 | minor | A bare system variable name typed at the command line (ORTHOMODE, GRIDMODE…) runs SETVAR on that variable, as in AutoCAD; U10 narrowed |
| 2026-10-13 | minor | STRETCH takes crossing and window polygons (CP, WP), BOX and a fence (F), typed and as JSON (`polygon`, `fence`): F1 narrowed |
| 2026-10-19 | minor | FF1 closed: DWG/DXF custom-class entity/object roundtrip preservation via raw DWG caching, UCS elevation, MTEXT background colours, unknown entities follow all common property edits, and renamed records preserve foreign data |
| 2026-10-23 | minor | Dimension Style Manager dialog landed (DIMSTYLE, DDIM, D, DST, Format/Dimension menus); F7 narrowed |
| 2026-10-23 | minor | Multifunctional grips and menus closed (U5): hover popup menu, `Next`/`Ctrl` cycling, and MULTILEADER Add/Remove Leader grips landed |
| 2026-10-20 | minor | Autosave leftovers closed (F4, top-10 row 5): web build autosaves to browser localStorage with StorageHooks; autosave writes DWG with DXF fallback; AUDITCTL `.adt` log generation; tolerant RECOVER mode in DXF and DWG readers |
| 2026-10-12 | minor | Property-based and hostile-input tests for geometry numerics landed (deterministic xorshift generator, no new dependency; known answers at 1e6–1e7 coordinates): S6 removed |
| 2026-10-12 | trivial | F13: HATCHEDIT refuses a bad scale or angle before editing (#724) |
| 2026-10-12 | minor | F3: dynamic block references show their grips (distance arrows, position squares, visibility/lookup lists, per visibility state) and drag them (`blocks.reference.grips`, `blocks.reference.grip`) |
| 2026-10-12 | minor | Multiline attributes read, drawn and saved (they lost their text before): F17 narrowed to creating them |
| 2026-10-13 | minor | F3: opening a dynamic block's own drawing (authoring elements in model space) offers the Block Editor on `<Current Drawing>`, as AutoCAD does |
| 2026-10-12 | minor | Edit Block Definition dialog (block list, preview, description; a new name starts a new block): F3 text updated, hours unchanged |
| 2026-10-12 | minor | Block Editor (BEDIT, BSAVE, BSAVEAS, BCLOSE) for static blocks landed: rows 4 (65–100 → 55–90 h) and F3 (50–75 → 40–65 h) narrowed to REFEDIT and dynamic blocks |
| 2026-10-12 | minor | In-place MTEXT editor with formatting toolbar and indent/tab ruler landed (#532): F9 narrowed to the editor leftovers, fields and spell check |
| 2026-10-12 | minor | SCRIPT command reading `.scr` files landed (runs a file through the same script engine as CLI/MCP; nested scripts refused): E2 removed |
| 2026-10-11 | minor | Refreshed for merged features: code pages (#505), tracking leftovers (#441), dynamic input/grip input (#519, #425) and SPLINEDIT Join/Add/Elevate (#559) closed their top-10 rows (top 10 now has 6 rows until the next review re-ranks); MULTILEADER (#459) and xrefs (#525) narrowed rows 1 and 4; F1 narrowed to the remaining stubs (#558, #559, #547); F5, F7, F9, F12, F14, F15, F17, F20, F22, U5, U7, U10, FF2, FF4, FF10 narrowed; F18, U2, U8, U9 removed (done); U13, S3, S4, P1 updated for merged fixes |
| 2026-10-11 | minor | German interface catalog landed: L1 row count 936 → 989, L2 narrowed (de done; 40–60 → 35–55 h) |
| 2026-10-11 | minor | SPLINE Knots/Tangency/toLerance/Object, MTEXT Height/Justify/Line spacing/Rotation/Style/Width/Columns, MLEADER spline and no-leader types and block content landed (#408): top-10 row 5, F1 and F10 narrowed |
| 2026-10-11 | minor | Object snap tracking, Extension, Parallel and true Apparent Intersection landed (#379): top-10 row 3 and U1 narrowed to the leftovers |
| 2026-10-10 | minor | Merged main: closed or narrowed rows for the Ukrainian catalog, UNITS dialog, shortcut display, dynamic input pointer boxes, press-drag windows, SETVAR DIM*, WIPEOUT and gradients, fmt |
| 2026-10-10 | minor | Alpha blockers marked (rows failing the core-workflow gate) |
| 2026-10-10 | major | First gaps list: ranked top 10 and every known shortfall by dimension, with evidence and estimates |
