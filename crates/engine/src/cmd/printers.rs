//! System printers as plot devices: the host's print queue — CUPS (`lpstat`, `lp`) on Linux,
//! macOS and the BSDs, PowerShell (`Get-Printer`, `Start-Process -Verb PrintTo`) on Windows —
//! listed beside the plotter configurations ([`super::plotters::devices`]) in the Plot dialog and
//! in PLOT's `device`. A plot to a printer renders the sheet to PDF as every plot does and hands
//! the PDF to the queue ([`print`]); nothing is written to disk unless `path` is given too.
//!
//! The web has no print queue: `printers.list` is empty there and a plot to a printer reports it.
//! Hosts and tests may install their own queue with [`set_printing`].

use std::sync::OnceLock;

use cadcraft_doc::{DevicePaper, Orientation, PlotOutput, PlotterConfig};
use serde_json::{Value, json};

use super::*;

/// What is sent to a printer: the sheet as PDF and how the queue should print it.
pub struct PrintJob<'a> {
    pub printer: &'a str,
    pub pdf: &'a [u8],
    /// The paper's name (a standard size, `A4`, `ISO A3 (297.00 x 420.00 MM)`, `Letter`…).
    pub paper: &'a str,
    pub width_mm: f64,
    pub height_mm: f64,
    pub landscape: bool,
    pub copies: u32,
    /// The job's title in the queue (the drawing and the sheet).
    pub title: &'a str,
}

/// A print queue: its printers, and sending a job. Returns the queue's job id.
#[derive(Clone, Copy)]
pub struct Printing {
    pub list: fn() -> std::result::Result<Vec<String>, String>,
    pub print: fn(&PrintJob) -> std::result::Result<String, String>,
}

static PRINTING: OnceLock<Printing> = OnceLock::new();

/// Install the host's print queue (first call wins). Without one, native builds ask the system's
/// (CUPS or PowerShell), the web has none.
pub fn set_printing(p: Printing) {
    let _ = PRINTING.set(p);
    forget_cache();
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
    list_cached().unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
static CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<String>)>> = std::sync::Mutex::new(None);

/// Drop the cached printer list (a new queue was installed).
fn forget_cache() {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(mut c) = CACHE.lock() {
        *c = None;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn list_cached() -> std::result::Result<Vec<String>, String> {
    use std::time::{Duration, Instant};
    const FOR: Duration = Duration::from_secs(10);
    // The lock is held while the queue is asked, so a slow answer never overwrites a newer one.
    let mut c = CACHE.lock().map_err(|_| "the printer list is unavailable".to_string())?;
    if let Some((at, names)) = c.as_ref()
        && at.elapsed() < FOR
    {
        return Ok(names.clone());
    }
    let names = list_now()?;
    *c = Some((Instant::now(), names.clone()));
    Ok(names)
}

#[cfg(target_arch = "wasm32")]
fn list_cached() -> std::result::Result<Vec<String>, String> {
    list_now()
}

fn list_now() -> std::result::Result<Vec<String>, String> {
    let Some(p) = printing() else { return Ok(Vec::new()) };
    let mut names = (p.list)()?;
    names.retain(|n| !n.trim().is_empty());
    names.truncate(MAX_PRINTERS);
    for n in &mut names {
        *n = n.trim().chars().take(255).collect();
    }
    Ok(names)
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

/// A printer as a plot device: PDF output, the standard paper sizes with their margins,
/// portrait, A4 by default (the queue picks the tray; a custom size is sent as such).
pub fn device_for(printer: &str) -> PlotterConfig {
    let papers = cadcraft_render::PAPER_SIZES
        .iter()
        .map(|p| DevicePaper { name: p.name.into(), width: p.width_mm, height: p.height_mm, margins: p.margins_mm() })
        .collect();
    PlotterConfig {
        name: printer.into(),
        description: DESCRIPTION.into(),
        output: PlotOutput::Pdf,
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
    if job.pdf.is_empty() {
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
            let (w, h) = (width_mm, height_mm);
            if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 { format!("Custom.{w:.0}x{h:.0}mm") } else { "A4".into() }
        }
    }
}

fn run_list(_s: &mut Session, _p: &Value) -> Result<Value> {
    let names = list_cached().map_err(|e| bad("printers.list", e))?;
    Ok(json!({ "printers": names }))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("printers.list", "System Printers", run_list)
            .params("→ {printers: [name]}: the host's print queue (CUPS, or PowerShell on Windows), each a `device` for PLOT; empty on the web")
            .enabled(always)
            .noundo(),
    ]
}

/// The system's print queue.
#[cfg(not(target_arch = "wasm32"))]
mod system {
    use super::{PrintJob, Printing};

    pub const PRINTING: Option<Printing> = Some(Printing { list, print });

    /// A command's standard output, or `None` when the program isn't installed.
    fn output(program: &str, args: &[&str], stdin: Option<&[u8]>) -> std::result::Result<Option<std::process::Output>, String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut cmd = Command::new(program);
        cmd.args(args).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
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

    #[cfg(windows)]
    fn list() -> std::result::Result<Vec<String>, String> {
        let args = ["-NoProfile", "-NonInteractive", "-Command", "Get-Printer | Select-Object -ExpandProperty Name"];
        let Some(out) = output("powershell", &args, None)? else { return Ok(Vec::new()) };
        if !out.status.success() {
            return Err(failure("Get-Printer", &out));
        }
        Ok(text(&out).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// `lpstat -e` lists every destination, one per line (CUPS 1.4 and later).
    #[cfg(not(windows))]
    fn list() -> std::result::Result<Vec<String>, String> {
        let Some(out) = output("lpstat", &["-e"], None)? else { return Ok(Vec::new()) };
        if !out.status.success() {
            // No destinations at all: lpstat says so on stderr and fails.
            let err = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
            if err.contains("no destinations") || err.contains("unable to connect") || err.contains("scheduler is not running") {
                return Ok(Vec::new());
            }
            return Err(failure("lpstat", &out));
        }
        Ok(text(&out).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// `Start-Process -Verb PrintTo` hands the PDF to the system's PDF handler for the printer;
    /// copies are up to that program. The temporary file stays for it to read.
    #[cfg(windows)]
    fn print(job: &PrintJob) -> std::result::Result<String, String> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("cadcraft-print-{}-{n}.pdf", std::process::id()));
        std::fs::write(&path, job.pdf).map_err(|e| format!("{}: {e}", path.display()))?;
        let printer = job.printer.replace('\'', "''");
        let file = path.to_string_lossy().replace('\'', "''");
        let script = format!("Start-Process -FilePath '{file}' -Verb PrintTo -ArgumentList '\"{printer}\"' -WindowStyle Hidden");
        let Some(out) = output("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None)? else {
            return Err("powershell is not available: plot to a PDF file and print it".into());
        };
        if !out.status.success() {
            return Err(failure("Start-Process -Verb PrintTo", &out));
        }
        Ok(path.to_string_lossy().to_string())
    }

    /// `lp -d printer -n copies -t title -o media=… [-o landscape] -` with the PDF on stdin;
    /// the job id from "request id is NAME-123 (1 file(s))".
    #[cfg(not(windows))]
    fn print(job: &PrintJob) -> std::result::Result<String, String> {
        let media = format!("media={}", super::cups_media(job.paper, job.width_mm, job.height_mm));
        let copies = job.copies.to_string();
        let title: String = job.title.chars().filter(|c| !c.is_control()).take(120).collect();
        let title = if title.trim().is_empty() { "CADCraft".to_string() } else { title };
        let mut args = vec!["-d", job.printer, "-n", &copies, "-t", &title, "-o", &media];
        if job.landscape {
            args.extend(["-o", "landscape"]);
        }
        args.push("-");
        let Some(out) = output("lp", &args, Some(job.pdf))? else {
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

    /// What the fake queue printed: (printer, PDF bytes, media, landscape, copies, title).
    pub(crate) static PRINTED: Mutex<Vec<(String, usize, String, bool, u32, String)>> = Mutex::new(Vec::new());

    fn fake_list() -> std::result::Result<Vec<String>, String> {
        Ok(vec!["Office".into(), "Plotter HP".into()])
    }

    fn fake_print(job: &PrintJob) -> std::result::Result<String, String> {
        let media = cups_media(job.paper, job.width_mm, job.height_mm);
        if let Ok(mut p) = PRINTED.lock() {
            p.push((job.printer.to_string(), job.pdf.len(), media, job.landscape, job.copies, job.title.to_string()));
        }
        Ok(format!("{}-42", job.printer))
    }

    /// The fake queue, installed once per test binary (first call wins).
    pub(crate) fn install_fake_queue() {
        set_printing(Printing { list: fake_list, print: fake_print });
    }

    fn fake_plot(_: &cadcraft_doc::Drawing, space: &cadcraft_doc::Space, o: &Value) -> std::result::Result<Vec<u8>, String> {
        Ok(format!("%PDF-1.4 {space:?} {o}").into_bytes())
    }

    #[test]
    fn printers_are_devices_and_a_plot_to_one_sends_the_pdf() {
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
        assert!(is_printer("office") && !is_printer("Kitchen"));
        let devices = super::plotters::devices();
        let office = devices.iter().find(|d| d.name == "Office").expect("a printer is a device");
        assert_eq!(office.description, DESCRIPTION);
        assert!(office.papers.iter().any(|p| p.name.contains("A4")));
        assert_eq!(super::plotters::find_device("plotter hp").map(|d| d.name), Some("Plotter HP".to_string()));

        let mut s = Session::new();
        let r = s.execute("printers.list", &json!({})).unwrap();
        assert_eq!(r["printers"], json!(["Office", "Plotter HP"]));

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
        assert_eq!(cups_media("Odd", f64::NAN, 400.0), "A4");
    }
}
