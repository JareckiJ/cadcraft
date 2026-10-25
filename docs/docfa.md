# DXF and DWG compatible with DOCFA (Italian cadastre)

> **Status:** File ▸ Save As… file types *DXF compatible with DOCFA* and *DWG compatible with
> DOCFA*, also File ▸ Save as DXF/DWG compatible with DOCFA… and Options ▸ Open and Save ▸ DOCFA;
> `export {docfa: true, unit}` for scripts, the control channel and MCP. Writer:
> `crates/io/src/docfa.rs` (`docfa::drawing`, `write_docfa`).

DOCFA is the Agenzia delle Entrate's program for cadastral declarations (*Documenti Catasto
Fabbricati*). It imports a floor plan (*planimetria*) from a DXF or DWG file and, from closed
polylines on the layer `DOCFA_POLIGONI` of a DXF, computes the surfaces of the unit.

## What DOCFA takes: the sources

- **The Agenzia's software page** ([Software Docfa 4.00.5](https://www.agenziaentrate.gov.it/portale/software-docfa-4005),
  version 4.00.5 of 03/07/2019) names DXF and DWG as the formats of the import. It states no
  version.
- **The Agenzia's own DXF templates** (`modelli.zip` on that page) are AutoCAD R12 DXF, in
  metres, with no declared drawing unit, each a closed polyline on the layer `RIQUADRO` with its
  lower-left corner at **0,0**: the printable area of the sheet at the scale.

  | Template | Frame (metres) |
  |---|---|
  | A4 1:100 | 19.5 × 23.5 |
  | A4 1:200 | 39 × 47 |
  | A4 1:500 | 97.5 × 117.5 |
  | A3 1:100 | 26 × 40.5 |
  | A3 1:200 | 52 × 81 |
  | A3 1:500 | 130 × 202.5 |

- **Practitioners** report that Docfa 4 loads DXF and DWG saved as **AutoCAD 2000 or earlier**
  and not newer ones, that the polygon layer is read from DXF only, and that some DXF 2000 files
  came in at the wrong scale
  ([GeoLIVE](https://www.geolive.org/guide/procedure-e-metodologie/docfa/docfa-4-come-importare-i-dwg-e-dxf-21123/),
  [professionearchitetto.it](https://www.professionearchitetto.it/news/notizie/30244/Planimetria-DoCFa-come-impostarla-e-tutte-le-regole-da-rispettare),
  [architetticampagna](https://architetticampagna.blogspot.com/2013/01/docfa-trucchi-e-suggerimenti.html)).

## What CADCraft writes

A **reduced copy** of the drawing beside it (the drawing keeps its own file), written by the
normal DXF and DWG writers at **AutoCAD 2000** (AC1015), the newest version DOCFA reads. There is
no version to pick.

| In the drawing | In the file |
|---|---|
| Model space | Written. Paper-space layouts are left out. |
| Block references (doors, windows, north arrow, title block) | Exploded into their lines, arcs and text, nested blocks and arrays too. Visible attributes become TEXT. |
| MTEXT | One TEXT per line, placed by the attachment point; formatting dropped. |
| Ellipses, splines, helixes, regions, multilines, leaders, multileaders | Polylines (tessellated), lines and text. |
| Dimensions, tables | Their lines, arrowheads and text. |
| Hatches, images, wipeouts, viewports, rays, xlines | Left out. |
| Objects on layers that are off or frozen | Left out (switch off what DOCFA must not see, such as the frame). |
| Layers | All kept, white (7) and continuous: DOCFA ignores layer colours. The polygon layer is named exactly `DOCFA_POLIGONI` whatever its case in the drawing. |
| Colours | Only on the objects of layer `DOCFA_POLIGONI` (the object's own or its layer's colour, as an ACI). Everything else is ByLayer. |
| Text | Transliterated to ASCII: `è` → `e`, `°` → `%%d`, `±` → `%%p`, `Ø` → `%%c`; other characters → `?`. |
| Units | Scaled to **metres** from the drawing's unit (m, cm or mm). |
| Header | `$INSUNITS` 0 (no declared unit, as the Agenzia's templates: a reader that converts a declared unit into its own would scale the plan, which is what the wrong-scale reports point to), `$INSBASE` 0,0, `$LUNITS` 2. |

## DOCFA checks

The export says on the command line, and in the result's `notes`, what it converted and left
out, and:

- how many surface polygons layer `DOCFA_POLIGONI` holds and how many are **not closed** (DOCFA
  needs closed polylines);
- objects on `DOCFA_POLIGONI` that aren't polylines, and polygons whose colour is outside 1–8;
- a drawing extending left of or below the origin (DOCFA expects the sheet frame's lower-left
  corner at 0,0);
- a drawing spanning more than 1000 m or less than 1 m once in metres: probably the wrong unit;
- for DWG, that DOCFA reads the polygons from DXF only.

## How to export

- **Desktop:** File ▸ Save As… and the file type *DXF compatible with DOCFA* (or DWG), or
  directly File ▸ Save as DXF compatible with DOCFA… (or DWG), or Options ▸ Open and Save ▸
  DOCFA. The dialog asks for the drawing's unit (it starts from the drawing's insertion units,
  `$INSUNITS`) and then for the file, named `<drawing>-docfa.dxf` by default.
- **Scripts, control channel, MCP:** `export {"path": "plan-docfa.dxf", "docfa": true, "unit": "cm"}`,
  or a `.dwg` path. Without `unit` the drawing's `$INSUNITS` decides (none counts as metres;
  another unit than m, cm or mm is refused).

## Preparing the drawing (from the DOCFA guides)

- **Draw the plan inside the Agenzia's frame**, its lower-left corner at 0,0 (at 1:200: A4 =
  39 × 47 m, A3 = 52 × 81 m).
- **Layer `0` for the plan** (walls, partitions, doors, stairs, texts); plain lines, no
  hatching, no furniture.
- **Layer `DOCFA_POLIGONI`** for the surface polygons, one closed polyline per area, each in the
  colour of its class:

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

  Guides differ on the exact class ↔ colour table between DOCFA versions; check the release
  notes of the DOCFA version you file with.

No DOCFA file, template or code was copied: the format comes from Autodesk's public DXF
Reference, the sources above, and CADCraft's own synthetic tests (`crates/io/src/docfa/tests.rs`).
