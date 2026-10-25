//! System printers as plot devices: the host's print queue, listed beside the plotter
//! configurations ([`super::plotters::devices`]) in the Plot dialog and in PLOT's `device`.
//!
//! - **Linux, macOS and the BSDs**: CUPS. `lpstat -e` lists the printers, `lpstat -d` names the
//!   default one, and `lp -d PRINTER -n COPIES -t TITLE -o media=… -o print-scaling=none -` takes
//!   the sheet as PDF on standard input (to scale, as a CAD plot must be).
//! - **Windows**: PowerShell. `Get-CimInstance Win32_Printer` lists the printers and the default
//!   one, and a temporary script prints the sheet as an image through `System.Drawing.Printing`
//!   (the printer's own driver, paper and copies), which needs no PDF viewer: `PrintTo` works only
//!   when a PDF program with that verb is installed, and Edge, the usual Windows PDF viewer,
//!   has none.
//!
//! A plot to a printer renders the sheet as every plot does, in the format the queue takes
//! ([`PrintFormat`]), and hands it over ([`print`]); nothing is written to disk unless `path` is
//! given too. The web has no print queue: `printers.list` is empty there and a plot to a printer
//! reports it. Hosts and tests may install their own queue with [`set_printing`].

use std::sync::OnceLock;

use cadcraft_doc::{DevicePaper, Orientation, PlotOutput, PlotterConfig};
use serde_json::{Value, json};

use super::*;

/// What a print queue takes the sheet as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintFormat {
    /// A vector PDF (CUPS).
    Pdf,
    /// A raster image at the plotter's resolution (Windows' print API draws images).
    Png,
}

impl PrintFormat {
    fn output(self) -> PlotOutput {
        match self {
            PrintFormat::Pdf => PlotOutput::Pdf,
            PrintFormat::Png => PlotOutput::Png,
        }
    }
}

/// What is sent to a printer: the sheet in the queue's [`PrintFormat`] and how to print it.
pub struct PrintJob<'a> {
    pub printer: &'a str,
    pub data: &'a [u8],
    pub format: PrintFormat,
    /// The paper's name (a standard size, `A4`, `ISO A3 (297.00 x 420.00 MM)`, `Letter`…).
    pub paper: &'a str,
    /// The paper's size, portrait or landscape as the page is set up.
    pub width_mm: f64,
    pub height_mm: f64,
    pub landscape: bool,
    pub copies: u32,
    /// The job's title in the queue (the drawing and the sheet).
    pub title: &'a str,
}

/// The printers of a queue and which one the system uses by default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrinterList {
    pub names: Vec<String>,
    pub default: Option<String>,
}

/// A print queue: its printers, what it takes, and sending a job (returns the queue's job id).
#[derive(Clone, Copy)]
pub struct Printing {
    pub list: fn() -> std::result::Result<PrinterList, String>,
    pub print: fn(&PrintJob) -> std::result::Result<String, String>,
    pub format: PrintFormat,
}

static PRINTING: OnceLock<Printing> = OnceLock::new();

/// Install the host's print queue (first call wins). Without one, native builds ask the system's
/// (CUPS or PowerShell), the web has none.
pub fn set_printing(p: Printing) {
    let _ = PRINTING.set(p);
    refresh();
}

fn printing() -> Option<Printing> {
    PRINTING.get().copied().or(system::PRINTING)
}

/// Printers beyond this many are not listed.
pub const MAX_PRINTERS: usize = 200;
/// The most copies one PLOT prints.
pub const MAX_COPIES: u32 = 99;
/// The description of a printer's device ([`device`]).
pub const DESCRIPTION: &str = "System printer";

/// The queue's printer names, in its order, at most [`MAX_PRINTERS`]; empty without a queue or
/// when asking it fails (`printers.list` reports the failure). The list is asked at most every
/// few seconds: the Plot dialog refreshes its devices often.
pub fn printers() -> Vec<String> {
    list_cached().map(|l| l.names).unwrap_or_default()
}

/// The system's default printer, if the queue names one.
pub fn default_printer() -> Option<String> {
    list_cached().ok().and_then(|l| l.default)
}

/// What the queue takes: a PDF when there is none (a plot with no queue writes PDF).
pub fn format() -> PrintFormat {
    printing().map_or(PrintFormat::Pdf, |p| p.format)
}

#[cfg(not(target_arch = "wasm32"))]
static CACHE: std::sync::Mutex<Option<(std::time::Instant, PrinterList)>> = std::sync::Mutex::new(None);

/// Ask the queue for its printers again at the next call (a printer was added, or a new queue was
/// installed).
pub fn refresh() {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(mut c) = CACHE.lock() {
        *c = None;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn list_cached() -> std::result::Result<PrinterList, String> {
    use std::time::{Duration, Instant};
    const FOR: Duration = Duration::from_secs(10);
    // The lock is held while the queue is asked, so a slow answer never overwrites a newer one.
    let mut c = CACHE.lock().map_err(|_| "the printer list is unavailable".to_string())?;
    if let Some((at, list)) = c.as_ref()
        && at.elapsed() < FOR
    {
        return Ok(list.clone());
    }
    let list = list_now()?;
    *c = Some((Instant::now(), list.clone()));
    Ok(list)
}

#[cfg(target_arch = "wasm32")]
fn list_cached() -> std::result::Result<PrinterList, String> {
    list_now()
}

fn list_now() -> std::result::Result<PrinterList, String> {
    let Some(p) = printing() else { return Ok(PrinterList::default()) };
    let mut list = (p.list)()?;
    list.names.retain(|n| !n.trim().is_empty());
    list.names.truncate(MAX_PRINTERS);
    for n in &mut list.names {
        *n = n.trim().chars().take(255).collect();
    }
    list.default = list.default.map(|d| d.trim().to_string()).filter(|d| list.names.iter().any(|n| n.eq_ignore_ascii_case(d)));
    Ok(list)
}

/// Whether `name` is one of the queue's printers (any case).
pub fn is_printer(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && printers().iter().any(|p| p.eq_ignore_ascii_case(n))
}

/// The printer called `name` as a plot device, if the queue has it.
pub fn device(name: &str) -> Option<PlotterConfig> {
    let n = name.trim();
    printers().into_iter().find(|p| p.eq_ignore_ascii_case(n)).map(|p| device_for(&p))
}

/// A printer as a plot device: the queue's output format, the standard paper sizes with their
/// margins, portrait, A4 by default (the queue picks the tray; a custom size is sent as such).
pub fn device_for(printer: &str) -> PlotterConfig {
    let papers = cadcraft_render::PAPER_SIZES
        .iter()
        .map(|p| DevicePaper { name: p.name.into(), width: p.width_mm, height: p.height_mm, margins: p.margins_mm() })
        .collect();
    PlotterConfig {
        name: printer.into(),
        description: DESCRIPTION.into(),
        output: format().output(),
        resolution: 300,
        orientation: Orientation::Portrait,
        default_paper: cadcraft_render::paper_size("A4").map(|p| p.name.to_string()).unwrap_or_default(),
        papers,
    }
}

/// Send a job to its printer: the queue's job id.
pub fn print(job: &PrintJob) -> std::result::Result<String, String> {
    let Some(p) = printing() else { return Err("there is no print queue here (the web has none): plot to a PDF file instead".into()) };
    if !is_printer(job.printer) {
        return Err(format!("unknown printer `{}` (printers: {})", job.printer.chars().take(80).collect::<String>(), printers().join(", ")));
    }
    if job.data.is_empty() {
        return Err("nothing to print".into());
    }
    if !(1..=MAX_COPIES).contains(&job.copies) {
        return Err(format!("`copies` must be 1 to {MAX_COPIES}"));
    }
    (p.print)(job)
}

/// The `copies` parameter of a plot: 1 by default, at most [`MAX_COPIES`].
pub fn copies_param(cmd: &str, p: &Value) -> Result<u32> {
    match p.get("copies") {
        None | Some(Value::Null) => Ok(1),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (1..=MAX_COPIES).contains(n))
            .ok_or_else(|| bad(cmd, format!("`copies` must be 1 to {MAX_COPIES}"))),
    }
}

/// The CUPS media name of a paper: the standard size's short name (`A4`, `Letter`, `Tabloid`),
/// else a custom size in millimetres.
pub fn cups_media(paper: &str, width_mm: f64, height_mm: f64) -> String {
    let short = cadcraft_render::paper_size(paper).and_then(|p| p.aliases.first().copied()).unwrap_or("");
    match short.to_ascii_uppercase().as_str() {
        "A0" | "A1" | "A2" | "A3" | "A4" | "A5" | "LETTER" | "LEGAL" | "TABLOID" | "LEDGER" => short.to_string(),
        "ANSI A" => "Letter".into(),
        "ANSI B" => "Tabloid".into(),
        "ANSI C" => "AnsiC".into(),
        "ANSI D" => "AnsiD".into(),
        "ANSI E" => "AnsiE".into(),
        "ARCH A" => "ArchA".into(),
        "ARCH B" => "ArchB".into(),
        "ARCH C" => "ArchC".into(),
        "ARCH D" => "ArchD".into(),
        "ARCH E" => "ArchE".into(),
        _ => {
            if !(width_mm.is_finite() && height_mm.is_finite() && width_mm > 0.0 && height_mm > 0.0) {
                return "A4".into();
            }
            let (w, h) = (width_mm.min(height_mm), width_mm.max(height_mm));
            format!("Custom.{w:.0}x{h:.0}mm")
        }
    }
}

/// The `lp` arguments (after the program name) of a job: the printer, copies, title, paper and
/// true scale; the sheet itself goes on standard input (`-`). The page's own orientation is the
/// PDF's, which CUPS turns to fit the paper, so no orientation option is given.
pub fn lp_args(job: &PrintJob) -> Vec<String> {
    let title: String = job.title.chars().filter(|c| !c.is_control()).take(120).collect();
    let title = if title.trim().is_empty() { "CADCraft".to_string() } else { title };
    vec![
        "-d".into(),
        job.printer.into(),
        "-n".into(),
        job.copies.to_string(),
        "-t".into(),
        title,
        "-o".into(),
        format!("media={}", cups_media(job.paper, job.width_mm, job.height_mm)),
        "-o".into(),
        "print-scaling=none".into(),
        "-".into(),
    ]
}

/// What `lpstat -d` says: `system default destination: NAME`, or that there is none.
pub fn parse_lpstat_default(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.to_ascii_lowercase().contains("default destination"))?;
    let (head, name) = line.split_once(':')?;
    if head.to_ascii_lowercase().contains("no ") { None } else { Some(name.trim().to_string()).filter(|n| !n.is_empty()) }
}

/// Windows' printer list: one `Default|Name` line per printer.
pub fn parse_windows_printers(text: &str) -> PrinterList {
    let mut list = PrinterList::default();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        let Some((default, name)) = line.split_once('|') else { continue };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if default.trim().eq_ignore_ascii_case("true") && list.default.is_none() {
            list.default = Some(name.to_string());
        }
        list.names.push(name.to_string());
    }
    list
}

/// The PowerShell that prints a sheet image on Windows. Its inputs come from the environment
/// (see [`windows_print_env`]), so no quoting reaches the script.
pub const WINDOWS_PRINT_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$inv = [Globalization.CultureInfo]::InvariantCulture
$printer = $env:CADCRAFT_PRINTER
$file = $env:CADCRAFT_IMAGE
$copies = [int]$env:CADCRAFT_COPIES
$wmm = [double]::Parse($env:CADCRAFT_WIDTH_MM, $inv)
$hmm = [double]::Parse($env:CADCRAFT_HEIGHT_MM, $inv)
$img = [System.Drawing.Image]::FromFile($file)
try {
  $doc = New-Object System.Drawing.Printing.PrintDocument
  $doc.PrinterSettings.PrinterName = $printer
  if (-not $doc.PrinterSettings.IsValid) { throw "unknown printer '$printer'" }
  $doc.PrinterSettings.Copies = [int16]$copies
  $doc.DocumentName = $env:CADCRAFT_TITLE
  $short = [Math]::Min($wmm, $hmm)
  $long = [Math]::Max($wmm, $hmm)
  $paper = $null
  foreach ($p in $doc.PrinterSettings.PaperSizes) {
    $pw = $p.Width * 0.254
    $ph = $p.Height * 0.254
    if ([Math]::Abs([Math]::Min($pw, $ph) - $short) -lt 3 -and [Math]::Abs([Math]::Max($pw, $ph) - $long) -lt 3) { $paper = $p; break }
  }
  if ($null -eq $paper) {
    $paper = New-Object System.Drawing.Printing.PaperSize('CADCraft', [int]($short / 0.254), [int]($long / 0.254))
  }
  $doc.DefaultPageSettings.PaperSize = $paper
  $doc.DefaultPageSettings.Landscape = ($img.Width -gt $img.Height)
  $doc.DefaultPageSettings.Margins = New-Object System.Drawing.Printing.Margins(0, 0, 0, 0)
  $doc.OriginAtMargins = $false
  $doc.add_PrintPage({
    param($sender, $e)
    $g = $e.Graphics
    $g.TranslateTransform(-$e.PageSettings.HardMarginX, -$e.PageSettings.HardMarginY)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($img, 0, 0, $e.PageBounds.Width, $e.PageBounds.Height)
    $e.HasMorePages = $false
  })
  $doc.Print()
} finally {
  $img.Dispose()
}
"#;

/// The environment [`WINDOWS_PRINT_SCRIPT`] reads for a job whose image is `image`.
pub fn windows_print_env(job: &PrintJob, image: &str) -> Vec<(&'static str, String)> {
    let title: String = job.title.chars().filter(|c| !c.is_control()).take(120).collect();
    vec![
        ("CADCRAFT_PRINTER", job.printer.to_string()),
        ("CADCRAFT_IMAGE", image.to_string()),
        ("CADCRAFT_COPIES", job.copies.to_string()),
        ("CADCRAFT_TITLE", if title.trim().is_empty() { "CADCraft".to_string() } else { title }),
        ("CADCRAFT_WIDTH_MM", format!("{:.3}", job.width_mm)),
        ("CADCRAFT_HEIGHT_MM", format!("{:.3}", job.height_mm)),
    ]
}

fn run_list(_s: &mut Session, _p: &Value) -> Result<Value> {
    let list = list_cached().map_err(|e| bad("printers.list", e))?;
    Ok(json!({ "printers": list.names, "default": list.default, "format": match format() { PrintFormat::Pdf => "pdf", PrintFormat::Png => "png" } }))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("printers.list", "System Printers", run_list)
            .params("→ {printers: [name], default: name | null, format: pdf | png}: the host's print queue (CUPS; PowerShell on Windows), each a `device` for PLOT; empty on the web")
            .enabled(always)
            .noundo(),
    ]
}

/// The system's print queue.
#[cfg(not(target_arch = "wasm32"))]
mod system {
    use super::{PrintFormat, PrintJob, PrinterList, Printing};

    #[cfg(windows)]
    const FORMAT: PrintFormat = PrintFormat::Png;
    #[cfg(not(windows))]
    const FORMAT: PrintFormat = PrintFormat::Pdf;

    pub const PRINTING: Option<Printing> = Some(Printing { list, print, format: FORMAT });

    /// A command's output, or `None` when the program isn't installed. `stdin` is written to the
    /// program; `env` is added to its environment.
    fn output(
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        env: &[(&str, String)],
    ) -> std::result::Result<Option<std::process::Output>, String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut cmd = Command::new(program);
        cmd.args(args).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{program}: {e}")),
        };
        if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
            // The program may stop reading early; what it took is what it prints.
            let _ = pipe.write_all(bytes);
            drop(pipe);
        }
        child.wait_with_output().map(Some).map_err(|e| format!("{program}: {e}"))
    }

    fn text(out: &std::process::Output) -> String {
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn failure(program: &str, out: &std::process::Output) -> String {
        let err = String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect::<String>();
        if err.is_empty() { format!("{program} failed ({})", out.status) } else { format!("{program}: {err}") }
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_string()).collect()
    }

    /// Get-CimInstance gives the default printer too; Get-Printer (names only) is the fallback.
    #[cfg(windows)]
    fn list() -> std::result::Result<PrinterList, String> {
        let script = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
            try { Get-CimInstance -ClassName Win32_Printer -ErrorAction Stop | ForEach-Object { '{0}|{1}' -f $_.Default, $_.Name } } \
            catch { Get-Printer | ForEach-Object { 'False|' + $_.Name } }";
        let args = strings(&["-NoProfile", "-NonInteractive", "-Command", script]);
        let Some(out) = output("powershell", &args, None, &[])? else { return Ok(PrinterList::default()) };
        if !out.status.success() {
            return Err(failure("Get-Printer", &out));
        }
        Ok(super::parse_windows_printers(&text(&out)))
    }

    /// `lpstat -e` lists every destination, one per line (CUPS 1.4 and later); `lpstat -d` names
    /// the default one.
    #[cfg(not(windows))]
    fn list() -> std::result::Result<PrinterList, String> {
        let Some(out) = output("lpstat", &strings(&["-e"]), None, &[])? else { return Ok(PrinterList::default()) };
        if !out.status.success() {
            // No destinations at all: lpstat says so on stderr and fails.
            let err = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
            if err.contains("no destinations") || err.contains("unable to connect") || err.contains("scheduler is not running") {
                return Ok(PrinterList::default());
            }
            return Err(failure("lpstat", &out));
        }
        let names = text(&out).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
        let default = output("lpstat", &strings(&["-d"]), None, &[]).ok().flatten().and_then(|o| super::parse_lpstat_default(&text(&o)));
        Ok(PrinterList { names, default })
    }

    /// A temporary PowerShell script prints the sheet image through the printer's driver
    /// ([`super::WINDOWS_PRINT_SCRIPT`]); both files are removed afterwards.
    #[cfg(windows)]
    fn print(job: &PrintJob) -> std::result::Result<String, String> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir();
        let stem = format!("cadcraft-print-{}-{n}", std::process::id());
        let (image, script) = (dir.join(format!("{stem}.png")), dir.join(format!("{stem}.ps1")));
        std::fs::write(&image, job.data).map_err(|e| format!("{}: {e}", image.display()))?;
        // A BOM makes Windows PowerShell read the script as UTF-8.
        let mut body = vec![0xEF, 0xBB, 0xBF];
        body.extend_from_slice(super::WINDOWS_PRINT_SCRIPT.as_bytes());
        let result = std::fs::write(&script, body).map_err(|e| format!("{}: {e}", script.display())).and_then(|()| {
            let args = vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                script.to_string_lossy().to_string(),
            ];
            let env = super::windows_print_env(job, &image.to_string_lossy());
            match output("powershell", &args, None, &env)? {
                None => Err("powershell is not available: plot to a PDF file and print it".to_string()),
                Some(out) if out.status.success() => Ok(stem.clone()),
                Some(out) => Err(failure("printing", &out)),
            }
        });
        let _ = std::fs::remove_file(&image);
        let _ = std::fs::remove_file(&script);
        result
    }

    /// `lp` with the PDF on standard input; the job id from "request id is NAME-123 (1 file(s))".
    #[cfg(not(windows))]
    fn print(job: &PrintJob) -> std::result::Result<String, String> {
        let Some(out) = output("lp", &super::lp_args(job), Some(job.data), &[])? else {
            return Err("lp is not available (CUPS is not installed): plot to a PDF file and print it".into());
        };
        if !out.status.success() {
            return Err(failure("lp", &out));
        }
        let said = text(&out);
        let id = said.split("request id is ").nth(1).and_then(|r| r.split_whitespace().next()).map(str::to_string);
        Ok(id.unwrap_or_else(|| said.trim().chars().take(80).collect()))
    }
}

#[cfg(target_arch = "wasm32")]
mod system {
    pub const PRINTING: Option<super::Printing> = None;
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::Session;

    /// What the fake queue printed: (printer, bytes, media, landscape, copies, title).
    pub(crate) static PRINTED: Mutex<Vec<(String, usize, String, bool, u32, String)>> = Mutex::new(Vec::new());

    fn fake_list() -> std::result::Result<PrinterList, String> {
        Ok(PrinterList { names: vec!["Office".into(), "Plotter HP".into()], default: Some("plotter hp".into()) })
    }

    fn fake_print(job: &PrintJob) -> std::result::Result<String, String> {
        let media = cups_media(job.paper, job.width_mm, job.height_mm);
        if let Ok(mut p) = PRINTED.lock() {
            p.push((job.printer.to_string(), job.data.len(), media, job.landscape, job.copies, job.title.to_string()));
        }
        Ok(format!("{}-42", job.printer))
    }

    /// The fake queue, installed once per test binary (first call wins).
    pub(crate) fn install_fake_queue() {
        set_printing(Printing { list: fake_list, print: fake_print, format: PrintFormat::Pdf });
    }

    fn fake_plot(_: &cadcraft_doc::Drawing, space: &cadcraft_doc::Space, o: &Value) -> std::result::Result<Vec<u8>, String> {
        Ok(format!("%PDF-1.4 {space:?} {o}").into_bytes())
    }

    #[test]
    fn printers_are_devices_and_a_plot_to_one_sends_the_sheet() {
        install_fake_queue();
        super::super::file::set_io(super::super::file::IoHooks {
            read: |_, _| Err("no reader in tests".into()),
            write: |_, _| Err("no writer in tests".into()),
            plot: Some(fake_plot),
        });
        if super::super::file::io().and_then(|h| h.plot).is_none() {
            return; // another test installed hooks without a plotter first
        }
        assert_eq!(printers(), vec!["Office".to_string(), "Plotter HP".to_string()]);
        assert_eq!(default_printer().as_deref(), Some("plotter hp"), "the default is one of the printers");
        assert!(is_printer("office") && !is_printer("Kitchen"));
        let devices = super::plotters::devices();
        let office = devices.iter().find(|d| d.name == "Office").expect("a printer is a device");
        assert_eq!(office.description, DESCRIPTION);
        assert!(office.papers.iter().any(|p| p.name.contains("A4")));
        assert_eq!(office.output, PlotOutput::Pdf, "the queue takes PDF");
        assert_eq!(super::plotters::find_device("plotter hp").map(|d| d.name), Some("Plotter HP".to_string()));

        let mut s = Session::new();
        let r = s.execute("printers.list", &json!({})).unwrap();
        assert_eq!(r["printers"], json!(["Office", "Plotter HP"]));
        assert_eq!(r["default"], "plotter hp");
        assert_eq!(r["format"], "pdf");

        let before = PRINTED.lock().unwrap().len();
        let r = s.execute("plot", &json!({ "device": "Office", "paper": "A3", "landscape": true, "copies": 2 })).unwrap();
        assert_eq!(r["printer"], "Office");
        assert_eq!(r["job"], "Office-42");
        assert_eq!(r["copies"], 2);
        assert!(r.get("data").is_none() && r.get("path").is_none(), "{r}");
        let printed = PRINTED.lock().unwrap();
        let (printer, len, media, landscape, copies, title) = printed.get(before).cloned().unwrap();
        drop(printed);
        assert_eq!((printer.as_str(), media.as_str(), landscape, copies), ("Office", "A3", true, 2));
        assert!(len > 0 && title.contains("Model"), "{title}");
        assert_eq!(r["message"], "Printed Model on Office (job Office-42, 2 copies)");

        // EXPORTPDF never prints, whatever the device; a plot to a PDF device still returns data.
        let r = s.execute("exportpdf", &json!({ "device": "Office" })).unwrap();
        assert!(r["data"].is_string() && r.get("printer").is_none());
        assert_eq!(PRINTED.lock().unwrap().len(), before + 1);
        let r = s.execute("plot", &json!({ "device": "CADCraft PDF" })).unwrap();
        assert!(r["data"].is_string() && r.get("printer").is_none());

        // Copies are bounded; an unknown printer is reported.
        assert!(s.execute("plot", &json!({ "device": "Office", "copies": 0 })).is_err());
        assert!(s.execute("plot", &json!({ "device": "Office", "copies": 100 })).is_err());
        assert!(s.execute("plot", &json!({ "device": "Kitchen" })).is_err());
    }

    #[test]
    fn cups_media_names_standard_sizes_and_custom_ones() {
        assert_eq!(cups_media("A4", 210.0, 297.0), "A4");
        assert_eq!(cups_media("ISO A3 (297.00 x 420.00 MM)", 297.0, 420.0), "A3");
        assert_eq!(cups_media("Letter", 215.9, 279.4), "Letter");
        assert_eq!(cups_media("ANSI B", 279.4, 431.8), "Tabloid");
        assert_eq!(cups_media("ARCH D", 609.6, 914.4), "ArchD");
        assert_eq!(cups_media("Odd", 300.0, 400.0), "Custom.300x400mm");
        assert_eq!(cups_media("Odd", 400.0, 300.0), "Custom.300x400mm", "a custom size is given short side first");
        assert_eq!(cups_media("Odd", f64::NAN, 400.0), "A4");
    }

    #[test]
    fn lp_gets_the_printer_copies_title_paper_and_true_scale() {
        let job = PrintJob {
            printer: "Ufficio HP",
            data: b"%PDF",
            format: PrintFormat::Pdf,
            paper: "ISO A3 (297.00 x 420.00 MM)",
            width_mm: 420.0,
            height_mm: 297.0,
            landscape: true,
            copies: 3,
            title: "Planimetria\n– Layout1",
        };
        let args = lp_args(&job);
        assert_eq!(
            args,
            ["-d", "Ufficio HP", "-n", "3", "-t", "Planimetria– Layout1", "-o", "media=A3", "-o", "print-scaling=none", "-"],
            "no orientation option: the PDF page already has the sheet's orientation"
        );
    }

    #[test]
    fn the_system_default_is_read_from_lpstat_and_windows_listings() {
        assert_eq!(parse_lpstat_default("system default destination: Office_HP\n").as_deref(), Some("Office_HP"));
        assert_eq!(parse_lpstat_default("no system default destination\n"), None);
        assert_eq!(parse_lpstat_default("destinazione predefinita del sistema: Ufficio\n").as_deref(), None, "only the English message is read");
        assert_eq!(parse_lpstat_default(""), None);
        let list =
            parse_windows_printers("\u{feff}False|Microsoft Print to PDF\nTrue|HP LaserJet 400\r\n\nFalse|\nbroken line\nTrue|Second default\n");
        assert_eq!(list.names, ["Microsoft Print to PDF", "HP LaserJet 400", "Second default"]);
        assert_eq!(list.default.as_deref(), Some("HP LaserJet 400"), "the first default wins");
    }

    #[test]
    fn the_windows_print_script_reads_its_job_from_the_environment() {
        let job = PrintJob {
            printer: "HP LaserJet 400",
            data: b"png",
            format: PrintFormat::Png,
            paper: "A4",
            width_mm: 210.0,
            height_mm: 297.0,
            landscape: false,
            copies: 2,
            title: "Plan – Model",
        };
        let env = windows_print_env(&job, r"C:\Temp\sheet.png");
        let get = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("CADCRAFT_PRINTER"), Some("HP LaserJet 400"));
        assert_eq!(get("CADCRAFT_IMAGE"), Some(r"C:\Temp\sheet.png"));
        assert_eq!((get("CADCRAFT_COPIES"), get("CADCRAFT_WIDTH_MM"), get("CADCRAFT_HEIGHT_MM")), (Some("2"), Some("210.000"), Some("297.000")));
        // Every variable the environment sets is one the script reads, and nothing from the job is
        // pasted into the script text: a printer name can hold any character.
        for (k, _) in &env {
            assert!(WINDOWS_PRINT_SCRIPT.contains(&format!("$env:{k}")), "{k}");
        }
        assert!(!WINDOWS_PRINT_SCRIPT.contains("HP LaserJet"));
        for must in ["PrintDocument", "PrinterSettings.PrinterName", "HardMarginX", "DrawImage", "Copies", "Landscape", "$doc.Print()"] {
            assert!(WINDOWS_PRINT_SCRIPT.contains(must), "{must}");
        }
        // Balanced braces and parentheses: a cheap guard against a broken edit of the script.
        let count = |c: char| WINDOWS_PRINT_SCRIPT.chars().filter(|x| *x == c).count();
        assert_eq!((count('{'), count('(')), (count('}'), count(')')));
    }
}
