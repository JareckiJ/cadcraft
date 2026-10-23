# Trying the electrical tools

Everything is under **Tools → Electrical**, and the quickest way in is the **Electrical palette**
(**Window → Electrical Palette**, Ctrl+7): the project's sheets, every IEC symbol as a button,
the tools and the reports. Every command can also be typed (its name is in brackets) and undone
with Ctrl+Z, except the whole-project commands, which save the sheets.

## 1. Open the demo project

1. Double-click `cadcraft.exe` (if Windows warns that it is unrecognised: **More info → Run
   anyway**).
2. **Open Project...** on the Electrical palette (or **Tools → Electrical → Project → Open Project...**) and pick
   `demo\Demo.ecproj`. The palette lists its three sheets; click one to open it.
   - **Sheet 1**: a control circuit on a ladder (+24V / 0V rails) with stop/start push-buttons, a
     contactor coil `-Q101`, an e-stop, a relay `-K104`, their contacts and lamps.
   - **Sheet 2**: the motor circuit: breaker, the contactor's three main contacts, overload,
     motor, and the motor cable `-W102` (4G1.5).
   - **Sheet 3**: three sensors wired through terminal strip `-X2` to an 8-point PLC input card
     `-KF102` (addresses `I0.0`…`I0.7`).
   Every sheet has a title block (Pump station, sheet N of 3).

## 2. Draw

- **Symbols**: click a symbol button on the palette (or **Insert Symbol**, `ESYMBOL`, and a
  name; `?` lists the 41 symbols), then click on a wire or rung. The wire is cut at the symbol's
  connection points and a main device is tagged at once from its sheet and column (`-K106`).
  Copy a device and the copy gets its own tag; copied contacts keep their coil's tag.
- **Draw Wire** (`EWIRE`): click near a pin or a wire and the wire snaps to it, turning at right
  angles; crossing a symbol connects it; landing on the middle of a wire adds a junction dot.
- **Wire Type** (`EWIRETYPE`): colour and cross-section (layer `WIRE_BU_1.5`) for new wires, or
  change selected ones.
- **Ladder** (`ELADDER`): rails with a rung in every column. When the symbols are placed,
  **Remove Empty Rungs** (`ELADDER.CLEAN`) takes away the rungs nothing went on (each would short
  the rails).
- **Cable** (`ECABLE`): draw a line across the wires; they become the cable's cores (colours by
  IEC 60445).
- **PLC card** (`EPLC`): DI / DO / AI / AO, any number of points, addresses counted like a PLC.
- **Terminals**: `IEC_TERMINAL` symbols join strip `-X1` (or the strip you type) and are numbered
  in order; **Number Terminals** (`ETERMNUM`) renumbers.
- **Link Contacts** (`ELINK`): pick a coil, then its contacts: they take its tag.
- **Title Block** (`ETITLE`): fill in project, title, customer, drawing number, revision…

## 3. Tag, number and cross-reference the whole project

On the palette's Project section:
- **Tag**: tags new devices, unique across all sheets (`-K104`: sheet 1, column 04; a second
  one in the column is `-K104A`).
- **Wire numbers** (`EWIRENUM` per sheet): one number per connected network, at its start (left
  end), from the sheet and column there: `101`, `101A`, `302F`.
- **Cross-references**: under each coil its contacts and where they are (`NO 13-14 /2.5`), next to
  each contact where its coil is (`/1.2`), across sheets. Moving a symbol updates them at once.

## 4. Reports

The palette's **Reports (CSV)** buttons save a parts list, wire list (from–to with colour and
size), terminal plan, PLC I/O list, cable list and device list for the whole project, ready for
Excel. `EREPORT` can also draw any of them as a table in the drawing.

## 5. Device data, parts and the community library

**Devices** opens a panel listing every device: edit its tag, manufacturer, catalogue number,
descriptions, location and ratings; **Apply** writes it into the project database (`Demo.ecdb`)
and into the symbol, on whichever sheet it is. **Community Hub** searches manufacturer parts
shared by other users and, once you sign in with GitHub, shares yours (CC BY 4.0).

## 6. Generate schematics from a BOM and netlist

`cadcraft-cli generate --bom bom.csv --netlist netlist.csv --template template --out DIR` makes a
whole multi-sheet project (ladder rungs, branches, seal-in contacts, wire numbers, title blocks)
from a parts list and a connection list; `generator-example\` has an example (a pump starter). The demo
build includes its output in `generated\`.

## 7. Make your own symbols

**Symbol Builder** (`ESYMBUILD`): draw the shape, select it, pick its base point, click each
connection point (Enter when done), then give a name, the IEC class letter (K relay, S switch,
Q contactor/breaker, F protection, M motor, P lamp, X terminal…) and the family. It becomes a
block with all the attributes; place it with **Insert Symbol** and its name.

## How it is stored (AutoCAD Electrical compatible)

Symbols are blocks with the AutoCAD Electrical attribute names: `TAG1` (main device) or `TAG2`
(contact), `DESC1`–`DESC3`, `MFG`, `CAT`, `LOC`, `INST`, `XREF`, `TERM01`… (pin numbers) and
invisible `X1TERM01`, `X4TERM02`… on the connection points (1 up, 2 right, 4 down, 8 left), plus
`ECLASS`, `EFAMILY` and a hidden `EID` linking the symbol to the project database. Wires are lines
and polylines on layers named `WIRE…`; wire numbers are texts on `WIRENO`. A project is a small
`.ecproj` file listing its DXF sheets in order.
