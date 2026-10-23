# Trying the electrical tools

Everything is under **Tools → Electrical**. Every command can also be typed (its name is in
brackets) and undone with Ctrl+Z, except the whole-project commands, which save the sheets.

## 1. Open the demo project

1. Double-click `cadcraft.exe` (if Windows warns that it is unrecognised: **More info → Run
   anyway**).
2. **Tools → Electrical → Project → Open Project...** and type the full path of
   `demo\Demo.ecproj` (tip: in Explorer, Shift + right-click the file → *Copy as path*, then paste
   with Ctrl+V). Both sheets open as tabs:
   - **Demo-01**: a control circuit on a ladder (+24V / 0V rails, a rung in each column) with
     stop/start push-buttons, a contactor coil `-Q101`, an e-stop, a relay `-K104`, their contacts
     and lamps.
   - **Demo-02**: the motor circuit: breaker, the contactor's three main contacts, overload,
     motor.

## 2. Add symbols and wires

- **Insert Symbol** (`ESYMBOL`): type a name (`?` lists them all: `IEC_COIL`, `IEC_NO`,
  `IEC_NC`, `IEC_PB_NO`, `IEC_PB_NC`, `IEC_ESTOP`, `IEC_SELECTOR`, `IEC_LAMP`, `IEC_FUSE`,
  `IEC_MCB_1P`, `IEC_MCB_3P`, `IEC_OVERLOAD_3P`, `IEC_OVERLOAD_NC`, `IEC_MAIN_CONTACTS_3P`,
  `IEC_MOTOR_3PH`, `IEC_TERMINAL`, `IEC_PSU`, `IEC_PE`), then click on an empty rung. The
  rung is cut at the symbol's connection points and a main device is tagged at once (`-K106`).
- **Draw Wire** (`EWIREDRAW`): click near a symbol's pin or a wire and the wire snaps to it,
  turning at right angles; landing on a pin or wire ends it, landing on the middle of a wire adds a
  junction dot.
- **Insert Ladder** (`ELADDER`) lays out rails and rungs on a new sheet.
- **Link Contacts** (`ELINK`): pick a coil, then its contacts, Enter: they take its tag.

## 3. Number and cross-reference the whole project

- **Project → Tag Devices (Whole Project)**: tags new devices, unique across all sheets
  (`-K104`: sheet 1, column 04; a second one in the column is `-K104A`).
- **Project → Number Wires (Whole Project)**: one number per connected network, placed at the
  start (left end) of the network and taken from the sheet and column there: `101`, `101A`, `204`.
- **Project → Cross-References (Whole Project)**: under the contactor coil `-Q101` appear its
  contacts and where they are (`NO 103  NO 202`), under each contact where its coil is (`/101`) —
  across sheets.

The single-drawing versions (**Tag Devices**, **Number Wires**, **Cross-References**) do the same
for the open drawing only.

## 4. Device data (the project database)

**Database → Electrical Devices** opens a panel listing every device. Click a device (or select
its symbol in the drawing) to edit its tag, manufacturer, catalogue number, descriptions, location
and technical ratings; **Apply** saves it in the project database (`Demo.ecdb`) and writes it into
the symbol, on whichever sheet it is. The panel also shows what the device is wired to. Every
electrical command keeps the database up to date; after ordinary edits use **Update from
Drawing**. **Database → Report** (`EDB.QUERY`) gives the parts list (`bom`) and wire list.

## 5. Make your own symbols

**Symbol Builder** (`ESYMBUILD`): draw the shape with lines, circles and arcs, select it, pick its
base point, click each connection point (Enter when done), then give a name, the IEC class letter
(K relay, S switch, Q contactor/breaker, F protection, M motor, P lamp, X terminal…) and the family
(Coil, NO, NC, Device, Terminal). It becomes a block with all the attributes; place it again with
**Insert Symbol** and its name. Edit it like any block, or WBLOCK it to a file to reuse.

## How it is stored (AutoCAD Electrical compatible)

Symbols are blocks with the AutoCAD Electrical attribute names: `TAG1` (main device) or `TAG2`
(contact), `DESC1`–`DESC3`, `MFG`, `CAT`, `LOC`, `INST`, `XREF`, `TERM01`… (pin numbers) and
invisible `X1TERM01`, `X4TERM02`… on the connection points (1 up, 2 right, 4 down, 8 left). Wires
are lines and polylines on layers named `WIRE…`; wire numbers are texts on `WIRENO`. A project is a
small `.ecproj` file listing its DXF sheets in order.
