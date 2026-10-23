# Plotters

A plotter (output device) decides what a plot writes and on which paper sizes. Page setups
and plots pick a device and one of its papers. Each paper carries margins, which bound its
printable area. Layouts show the printable area as a dashed outline on the sheet.

## Devices

The built-in devices come first:

| Device | Output |
|---|---|
| `CADCraft PDF` | vector PDF |
| `CADCraft PNG` | raster PNG on white paper at the device resolution (300 dpi) |
| `CADCraft SVG` | vector SVG |

They offer the standard sizes:

- ISO 216 A0–A6 and B0–B5
- ANSI A–E and Legal
- ARCH A–E and E1

ISO sheets have 5 mm margins. Inch sheets have quarter-inch (6.35 mm) margins. A page setup
with the device `None` plots to PDF on the page setup's paper, as before.

User devices are files in our own format, kept in the `plotters` folder of the CADCraft
settings directory:

| System | Folder |
|---|---|
| Linux and FreeBSD | `$XDG_CONFIG_HOME/cadcraft/plotters` or `~/.config/cadcraft/plotters` |
| macOS | `~/Library/Application Support/CADCraft/plotters` |
| Windows | `%APPDATA%\CADCraft\plotters` |

`CADCRAFT_CONFIG_DIR` replaces the settings directory. On the web, devices last for the
session unless the host provides storage (`cmd::plotters::set_plotter_store`). Binary `.pc3`
files are not read; we haven't found a public specification of that format.

## Files

`<name>.plotter.json`, UTF-8 JSON with a format marker:

```json
{
  "cadcraftPlotter": 1,
  "name": "Office A3",
  "description": "Raster proofs",
  "output": "png",
  "resolution": 150,
  "orientation": "landscape",
  "defaultPaper": "A3 borderless",
  "papers": [
    { "name": "A3 borderless", "width": 297, "height": 420, "margins": [0, 0, 0, 0] },
    { "name": "ISO A4 (210.00 x 297.00 MM)", "width": 210, "height": 297, "margins": [5, 5, 5, 5] }
  ]
}
```

- `output` is `pdf`, `png` or `svg`.
- `resolution` is in dots per inch (50–2400) and applies to PNG.
- `orientation` (`portrait` or `landscape`) is used when a page setup switches to the device.
- Paper sizes are in millimetres on the portrait sheet. `margins` are left, bottom, right and
  top, in mm.

Reading fixes hostile values:

- Sizes are kept within 1–5080 mm, and width and height are swapped to portrait.
- Margins are kept within 0–1000 mm. Margins that leave no printable area are dropped.
- Paper names are unique, and a device keeps at most 512 papers.
- A missing or unknown `defaultPaper` becomes the first paper.
- Files over 1 MB are refused.

In the file name, characters other than letters, digits, spaces and `-_()+,` become `_`.

## Commands

- `plottermanager {action, …}` (also **File ▸ Plotter Manager...**) manages the devices:
  - `list` (the default) shows the built-in devices first.
  - `get {name}` returns a device.
  - `new {name, from?, output?, …}` copies `from`, or the built-in device for `output` with
    the standard papers.
  - `set {name, description?, output?, resolution?, orientation?, defaultPaper?, paper?: {name,
    width, height, margins?}, removePaper?}` edits a device. `paper` adds a paper or replaces
    the one of that name.
  - `delete {name}` removes a device.
  - `load {text, name?}` stores a device file's contents.
  - `save {name, path?}` writes the file to `path`, or returns it as `text`.

  Built-in devices can't be edited or deleted.
- Typed `PLOTTERMANAGER` or `-PLOTTERMANAGER` prompts for an option:
  - `?` lists the devices.
  - `New` asks for a name and an output (`Pdf`, `PNg`, `Svg`).
  - `Delete` asks for a device.
  - `Paper` asks for a device, a paper name and `width,height` in mm, and adds that paper.
- `pagesetup {layout, device, paper?}` sets the layout's device. `paper` names one of the
  device's papers, or a standard size by its short name (`A3`, `Letter`). The paper's
  margins become the layout's printable area. Without `paper`, a device change keeps the
  current paper if the device has it, else takes the device's default paper. A device change
  also takes the device's orientation unless `landscape` is given. `device: "None"` clears it.
- `plot {device?, paper?, …}` plots with `device`, or with the page setup's device.
  - The device sets the output and resolution.
  - `paper` (one of the device's papers) sets the size and margins for this plot.
  - A model-space plot without `paper` on another device than its page setup's uses the
    device's default paper and orientation.
  - For Model plots, `center` defaults to `true`, preserving the existing centered output. Set
    `center: false` with `offset: [x, y]` to place the plotted bounds' lower-left corner from the
    printable area's lower-left origin; offsets are millimetres, default to `[0, 0]`, and are
    bounded to ±100,000 mm. PDF and PNG support these controls. SVG plots keep their extents view;
    an active offset is rejected rather than ignored. Layout plots do not use model offsets.
  - `exportpdf` takes the same options and always writes PDF.

PDF and PNG plots apply the paper margins from the page setup or plot device for placement
and clipping. SVG plots show a layout's whole sheet, or the model-space extents.

## Model space page setup

Model space has a page setup of its own, as each layout has.

- `pagesetup {layout: "Model", …}` (or `pagesetup` without `layout` on the Model tab) stores
  it, one undo step. It takes the layout options and a plot area of `extents`, `display` or
  `limits`. Saving a model plot window through PAGESETUP or Apply to Layout is not available yet.
- Without `layout`, PAGESETUP targets the displayed tab. Editing model space through a layout
  viewport (MSPACE) still reads and updates that layout's page setup.
- Until it is set, model space plots the drawing extents fitted and centred, on A4 in metric
  drawings and ANSI A otherwise.
- PLOT, EXPORTPDF and PREVIEW take its device, paper, orientation, margins, plot style
  table, lineweights and transparency, as they take a layout's. Its plot area, scale and
  centring are what the Plot dialog opens with; `plot` without them still plots the extents,
  fitted and centred.
- The Plot dialog's Apply to Layout saves the dialog's settings in the page setup of the
  layout plotted, Model included, without plotting. The X and Y offsets are not kept.
- DXF and DWG files keep it in the Model layout's plot settings.

## Plot transparency

Objects plot with their transparency (their own, their layer's when ByLayer, the block
reference's when ByBlock) only when asked, as in AutoCAD:

- `pagesetup {layout, plotTransparency: bool}` stores the layout's (or Model's) "Plot
  transparency" (off by default). The Page Setup Manager and the Plot dialog have the checkbox.
- `plot {plotTransparency?: bool}` (and `exportpdf`, `preview`) overrides it for one plot.
- `PLOTTRANSPARENCYOVERRIDE` decides first: 0 never plots transparency, 1 (default) uses the
  setting above, 2 always plots it.

PDF plots write it as alpha (`/ExtGState` with `/ca` and `/CA`), PNG plots blend it, SVG plots use
`opacity`. A plot without transparency is written exactly as before. The DXF plot layout flags
have no bit for the option, so it isn't saved in DXF or DWG files.

On screen, `TRANSPARENCYDISPLAY` (on by default; the status bar's Transparency button) shows
transparency; PNG and SVG exports of a drawing always draw it.
