# Trying the electrical commands

1. Double-click `cadcraft.exe`. Windows may warn that the app is unrecognised (it isn't signed):
   click **More info → Run anyway**.
2. **File → Open** `demo-schematic.dxf`. It has two A3 sheets (columns 1–10 along the top), nine
   symbols without tags and red wires without numbers.
3. **Tools → Electrical → Tag Devices** (or type `ETAG` and press Enter twice). Every symbol gets an
   IEC 81346 tag from its sheet and column: the relay coil in column 2 of sheet 1 becomes `-K1.2`.
4. **Tools → Electrical → Number Wires** (or `EWIRE`). Each connected potential gets one number,
   e.g. `1.6` on the +24V rail, written on the `WIRENO` layer.
5. **Tools → Electrical → Link Contacts** (`ELINK`): pick the relay coil in column 2, then the
   contact above the lamp in column 5, then press Enter. The contact takes the coil's tag.
6. **Tools → Electrical → Cross-References** (`EXREF`): under the coil appears `NO 1.5` (its
   normally open contact is in column 5 of sheet 1), under the contact `/1.2` (its coil's place).
7. **Tools → Electrical → List Devices** (`EDEVICES`) lists the tags and any duplicates.

Ctrl+Z undoes each command. Run a command again after adding symbols or wires: existing tags and
numbers are kept and only new ones are added; type `ETAG` then `All` to renumber everything.

## Using your own drawings

- A **device** is any block with an attribute called `TAG`. Its class letter comes from the tag
  already typed (`-K`, `-Q?`) or from the block's name (RELAY → K, CONTACTOR/BREAKER → Q, FUSE → F,
  MOTOR → M, PUSHBUTTON/SWITCH → S, SENSOR → B, LAMP → P, TERMINAL → X, …).
- A **contact** is a block whose name contains `CONTACT` (but not `CONTACTOR`) or `AUX`; it
  carries its coil's tag instead of its own, and `NC` in its name marks it normally closed.
  Renumbering everything renames a coil's contacts with it.
- A **wire** is a line or polyline on a layer whose name starts with `WIRE`. Wires that touch end to
  end or meet in a T share a number; wires that only cross do not.
- Sheets are 420 wide, side by side from x = 0, with 10 columns each.
