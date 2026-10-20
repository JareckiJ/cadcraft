# DXF for DOCFA (Italian cadastre) and other Release 12 readers

> **Status:** landed in the `feature/saveas-dxf-r12-docfa` branch (R12 DXF writer `crates/io/src/dxf_r12.rs`, SAVEAS `version: R12` / `docfa`, the Save As dialog, `cadcraft-cli run --save … --docfa`).

DOCFA is the Agenzia delle Entrate's program for cadastral declarations (*Documenti Catasto
Fabbricati*). It imports a floor plan (*planimetria*) as a DXF file and, from closed polylines on
one layer, computes the surfaces of the unit. Its importer is old: it reads **AutoCAD Release 12
DXF** (`$ACADVER` AC1009) best, and chokes on what later versions added. CADCraft writes that
format from a *reduced copy* of the drawing, with a **DOCFA profile** that applies what the DOCFA
guides and practitioners ask for.

## How to export

- **Desktop:** File ▸ Save As… (Ctrl+Shift+S / ⇧⌘S) ▸ file type *Drawing (DXF)* ▸ tick **DOCFA
  profile (Italian cadastre)** ▸ pick the drawing's unit (m, cm, mm) ▸ Save…. The suggested name is
  `<drawing>-docfa.dxf`. The export does not rename the drawing: keep saving the drawing itself
  as DXF/DWG 2018 and export for DOCFA when a sheet is ready.
- **Command line / scripts / MCP:** `saveas {"path": "plan-docfa.dxf", "docfa": true, "unit": "cm"}`
  or `export {"path": "plan-docfa.dxf", "docfa": true, "unit": "cm"}`. `scale` takes a number
  instead of `unit` (drawing units × scale = metres). The result's `notes` and the command line
  say what was converted or left out.
- **Headless:** `cadcraft-cli run plan.dwg --save plan-docfa.dxf --docfa --unit cm`.
- **Plain R12** (other legacy readers, no DOCFA rules): the same places with version **R12**
  instead of the profile: `saveas {"path": "plan.dxf", "version": "R12"}`, the dialog's version
  list, `--save-version R12`.

## What the DOCFA profile writes

| In the drawing | In the file |
|---|---|
| Model space | Written. Paper-space layouts are left out (R12 has no layouts). |
| Block references (doors, windows, north arrow, title block) | Exploded into their lines, arcs and text, nested blocks too (DOCFA guides: *eliminare i blocchi*). Attributes become TEXT. |
| LWPOLYLINE | Old-style POLYLINE / VERTEX / SEQEND, bulges and widths kept. |
| MTEXT | One TEXT per line, placed by the attachment point; formatting dropped. |
| Ellipses, splines, helixes, regions, multilines, leaders, multileaders | Polylines (tessellated), lines and text. |
| Dimensions, tables | Their lines, arrowheads (SOLID) and text. |
| Hatches, images, wipeouts, viewports, rays, xlines | Left out (*senza retini*). |
| Objects on layers that are off or frozen | Left out (the guides say to switch off the frame and polygon layers you don't want). |
| Layers | All written, white (7) and CONTINUOUS: DOCFA ignores layer colours. Names become R12 names (upper case, `A–Z 0–9 $ - _`, 31 characters). |
| Colours | Only on the polylines of layer **DOCFA_POLIGONI** (group 62, the object's own or its layer's colour, as an ACI). Everything else is ByLayer, so DOCFA draws it in its own colour. |
| Text | Transliterated to ASCII: `è` → `e`, `°` → `%%d`, `±` → `%%p`, `Ø` → `%%c`; other characters → `?`. Old readers show `\U+00E8` otherwise. |
| Units | Multiplied by `scale` (`unit` m = 1, cm = 0.01, mm = 0.001): DOCFA takes **metres**. |
| Header | `$ACADVER` AC1009, `$EXTMIN/$EXTMAX` of what was written, `$LIMMIN` 0,0, `$INSBASE` 0, `$LUNITS` 2, no handles, no `$HANDLING`. |
| Tables | VPORT (`*ACTIVE` showing everything), LTYPE (CONTINUOUS), LAYER, STYLE (STANDARD, `txt`). No DIMSTYLE, UCS, VIEW or APPID records, no CLASSES or OBJECTS sections. |

The plain R12 version keeps block references as INSERT (with ATTRIB), writes the drawing's own
linetypes, text styles and entity colours, writes hatch pattern lines as LINEs (solid fills as
their boundary polylines), and keeps the drawing's `$INSBASE`, `$LIMMIN/$LIMMAX` and header
variables. Everything else is the same conversion.

## DOCFA checks

The export reports, on the command line and in the result's `notes`:

- how many surface polygons layer DOCFA_POLIGONI holds, and how many are **not closed** (DOCFA
  needs closed polylines: *tutte le polilinee devono essere rigorosamente chiuse*);
- objects on DOCFA_POLIGONI that aren't polylines;
- polygons whose colour is outside 1–8 (the area classes below);
- a drawing extending left of or below the origin: DOCFA expects the sheet frame's lower-left
  corner at **0,0**;
- a drawing spanning more than 1000 units: probably not in metres yet (use `unit`).

## Preparing the drawing (from the DOCFA guides)

- **One CAD unit = one metre** in the file: draw in metres, or export with `unit: "cm"` / `"mm"`.
- **Sheet frame at the origin:** the frame's lower-left corner at 0,0. At 1:200 the frames are
  A4 = 39 × 47 units, A3 = 52 × 81 units (the Agenzia's own DXF templates come in A4/A3 at 1:100,
  1:200 and 1:500). Put the frame on its own layer (`RIQUADRO` / `CART`) and switch that layer off
  before exporting, or leave it on if your DOCFA version expects it.
- **Layer `0` for the plan** (walls, partitions, doors, stairs, texts); plain lines, no
  lineweights, no hatching, no furniture.
- **Layer `DOCFA_POLIGONI`** for the surface polygons, one closed polyline per area, each in the
  colour of its class (DXF group 62):

  | Class | Colour (ACI) | Meaning |
  |---|---|---|
  | A | 1 red | main rooms and their direct accessories |
  | B | 2 yellow | indirect-service accessories, communicating |
  | C | 3 green | indirect-service accessories, non-communicating |
  | D | 4 cyan | balconies, terraces, loggias, communicating |
  | E | 5 blue | balconies, terraces, loggias, non-communicating |
  | F | 6 magenta | exclusive outdoor areas (courtyards, gardens) |
  | G | 7 white | spaces not relevant for the calculation |
  | A2 | 8 grey | accessories serving C/1 and C/6 units directly |

  Guides differ on the exact class ↔ colour table between DOCFA versions; check the DOCFA
  release notes and the Agenzia's templates for the version you file with.
- **Required contents:** the scale, the height of the rooms in metres (rounded to 5 cm), a north
  symbol outside the plan, the floor (S1, S2 for basements), the accesses, exclusive courtyards.
- **Text:** plain words (`cucina`/`K`, `bagno`, `ripostiglio`…), no multiline text (exploded by
  the export anyway), no neighbours' names (use cadastral identifiers or *altra u.i.u.*).

## Sources

Italian practitioners' guides; DOCFA itself is closed and undocumented beyond its release notes:

- GeoLIVE, *Planimetrie CAD… come inserirle in DocFa* — version 12, units, frames, the
  DOCFA_POLIGONI layer and its colours.
- professionearchitetto.it, *Planimetria DoCFa, come impostarla e tutte le regole da rispettare* —
  layers, colours by object, required contents, what makes a filing refused.
- forum.topgeometri.it, *Problema con importazione file dxf in docfa* — start from a clean R12
  file, plain lines, no hatches, no blocks, the title block at 0,0.
- Agenzia delle Entrate, *Software Docfa* downloads — the DXF templates (A4/A3, 1:100/1:200/1:500).

No DOCFA file, template or code was copied: the format comes from Autodesk's public DXF
Reference (R12 section) and CADCraft's own synthetic tests (`crates/io/src/dxf_r12/tests.rs`).
