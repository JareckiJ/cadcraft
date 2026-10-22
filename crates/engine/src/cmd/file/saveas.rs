//! SAVEAS typed at the command line, in a script or with FILEDIA 0: the file format and version
//! first, then the file name, as AutoCAD asks without its Save dialog. FILEDIA itself (a profile
//! setting, like QNEWTEMPLATE) is kept here; the interface reads it to choose between its Save
//! dialog and this prompt sequence. JSON calls (`saveas {path, version?, binary?}`) never prompt.
//!
//! ```text
//! Command: SAVEAS
//! Current file format: DXF 2018
//! Enter file format or [2000/2004/2007/2010/2013/2018/DXF/Template] <DXF>:
//! Enter DXF version or [2000/2004/2007/2010/2013/2018] <2018>:
//! Enter DXF encoding or [Ascii/Binary] <Ascii>:
//! Save drawing as <Drawing1.dxf>:
//! ```
//!
//! A year chooses DWG at that version. R14 DWG and R12 DXF are not written yet (gaps FF13).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use serde_json::json;

use super::{Version, drawing_version, run_saveas_reporting};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// The drawing file formats SAVEAS writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileFormat {
    /// DXF, CADCraft's native format (ASCII or binary).
    #[default]
    Dxf,
    Dwg,
    /// A drawing template (`.dwt`), written as DWG.
    Template,
}

impl FileFormat {
    pub const ALL: [FileFormat; 3] = [FileFormat::Dxf, FileFormat::Dwg, FileFormat::Template];

    /// The file extension ("dxf", "dwg", "dwt").
    pub fn ext(self) -> &'static str {
        match self {
            FileFormat::Dxf => "dxf",
            FileFormat::Dwg => "dwg",
            FileFormat::Template => "dwt",
        }
    }

    /// How the command line names it ("DXF", "DWG", "DWG template").
    pub fn label(self) -> &'static str {
        match self {
            FileFormat::Dxf => "DXF",
            FileFormat::Dwg => "DWG",
            FileFormat::Template => "DWG template",
        }
    }

    /// The format a file name's extension names; `None` for another extension (an export such as
    /// SVG or PDF). A name without an extension is saved as DXF.
    pub fn of_path(path: &str) -> Option<FileFormat> {
        let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "" | "dxf" => Some(FileFormat::Dxf),
            "dwg" => Some(FileFormat::Dwg),
            "dwt" => Some(FileFormat::Template),
            _ => None,
        }
    }
}

static FILEDIA: AtomicBool = AtomicBool::new(true);

/// FILEDIA: 1 (default) file commands typed or chosen from a menu open the interface's file
/// dialogs; 0 they ask on the command line. Process-wide, like a profile setting.
pub fn filedia() -> bool {
    FILEDIA.load(Ordering::Relaxed)
}

pub fn set_filedia(on: bool) {
    FILEDIA.store(on, Ordering::Relaxed);
}

fn untitled_default() -> MutexGuard<'static, (FileFormat, Version)> {
    static DEFAULT: OnceLock<Mutex<(FileFormat, Version)>> = OnceLock::new();
    DEFAULT.get_or_init(|| Mutex::new((FileFormat::Dxf, Version::default()))).lock().unwrap_or_else(PoisonError::into_inner)
}

/// The format and version the SAVEAS prompt offers first for a drawing that has no file yet
/// (the interface's Default Save Format; DXF 2018 until a host sets it). A drawing with a file
/// is offered its own.
pub fn untitled_save_default() -> (FileFormat, Version) {
    *untitled_default()
}

pub fn set_untitled_save_default(format: FileFormat, version: Version) {
    *untitled_default() = (format, version);
}

/// The longest file name the prompt takes (hostile input stays bounded).
const MAX_NAME: usize = 4096;

const YEARS: [&str; 6] = ["2000", "2004", "2007", "2010", "2013", "2018"];

#[derive(Clone, Debug, PartialEq, Eq)]
enum Stage {
    Format,
    DxfVersion,
    DxfEncoding,
    Name,
    /// The file exists: replace it?
    Replace(String),
}

/// The SAVEAS prompt sequence.
pub(super) struct SaveAsM {
    stage: Stage,
    format: FileFormat,
    version: Version,
    binary: bool,
}

impl SaveAsM {
    /// Start from the drawing's own format, version and encoding (an untitled drawing: the
    /// [`untitled_save_default`]).
    pub(super) fn new(s: &Session) -> SaveAsM {
        let st = s.state().ok();
        let path = st.and_then(|st| st.path.clone());
        let (format, version) = match path.as_deref().and_then(|p| FileFormat::of_path(p).map(|f| (f, p))) {
            Some((f, p)) => (f, drawing_version(s, p)),
            None => untitled_save_default(),
        };
        let binary = format == FileFormat::Dxf && st.is_some_and(|st| st.binary_dxf);
        SaveAsM { stage: Stage::Format, format, version, binary }
    }

    fn current(&self) -> String {
        match self.format {
            FileFormat::Dxf if self.binary => format!("binary DXF {}", self.version.year()),
            f => format!("{} {}", f.label(), self.version.year()),
        }
    }

    /// The file name offered: the drawing's file (or its title) with this format's extension.
    fn default_name(&self, s: &Session) -> String {
        let st = s.state().ok();
        let base = st.and_then(|st| st.path.clone()).or_else(|| st.map(|st| st.title.clone())).filter(|b| !b.trim().is_empty());
        with_extension(base.as_deref().unwrap_or("Drawing"), self.format)
    }

    /// Format chosen: DXF asks for its version and encoding, the others for the file name.
    fn after_format(&mut self) -> Step {
        self.stage = if self.format == FileFormat::Dxf { Stage::DxfVersion } else { Stage::Name };
        Step::Continue
    }

    fn save(&mut self, s: &mut Session, path: &str) -> Step {
        let binary = self.format == FileFormat::Dxf && self.binary;
        let p = json!({ "path": path, "version": self.version.year(), "binary": binary });
        match run_saveas_reporting(s, &p) {
            Ok(_) => s.echo(format!("Saved {path} ({}).", self.current())),
            // A file that could not be written (or a refused save) is reported; the drawing keeps
            // its unsaved changes and the command ends, as a failed save in the dialog does.
            Err(e) => s.echo(e.to_string()),
        }
        Step::Done
    }
}

/// `name` with `format`'s extension: kept when it has it, put in place of another drawing
/// extension, added after anything else.
fn with_extension(name: &str, format: FileFormat) -> String {
    let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some(e) if e == format.ext() => name.to_string(),
        Some("dxf" | "dwg" | "dwt") => {
            let dot = name.rfind('.').unwrap_or(name.len());
            format!("{}.{}", name.get(..dot).unwrap_or(name), format.ext())
        }
        _ => format!("{name}.{}", format.ext()),
    }
}

/// A typed version: a year, "R2018" or an `$ACADVER` code.
fn typed_version(t: &str) -> Option<Version> {
    Version::parse(t.trim())
}

/// Versions older than 2000, which CADCraft cannot write yet.
fn too_old(t: &str) -> bool {
    let t = t.trim().to_ascii_lowercase();
    matches!(t.as_str(), "r14" | "14" | "r12" | "12" | "lt98" | "lt97" | "r13" | "13" | "ac1014" | "ac1009" | "ac1012")
}

fn short(t: &str) -> String {
    t.trim().chars().take(40).collect()
}

const KEYWORDS: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };

impl Interactive for SaveAsM {
    fn name(&self) -> &'static str {
        "SAVEAS"
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!("Current file format: {}", self.current()));
        Ok(Step::Continue)
    }

    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            Stage::Format => {
                let default = match self.format {
                    FileFormat::Dwg => self.version.year().to_string(),
                    FileFormat::Dxf => "DXF".into(),
                    FileFormat::Template => "Template".into(),
                };
                let mut kw: Vec<&str> = YEARS.to_vec();
                kw.extend(["DXF", "Template"]);
                Prompt::new("Enter file format", KEYWORDS).kw(&kw).default(default)
            }
            Stage::DxfVersion => Prompt::new("Enter DXF version", KEYWORDS).kw(&YEARS).default(self.version.year()),
            Stage::DxfEncoding => {
                Prompt::new("Enter DXF encoding", KEYWORDS).kw(&["Ascii", "Binary"]).default(if self.binary { "Binary" } else { "Ascii" })
            }
            Stage::Name => Prompt::new("Save drawing as", Accept::TEXT).default(self.default_name(s)),
            Stage::Replace(_) => Prompt::new("A file with this name already exists. Replace it?", KEYWORDS).kw(&["Yes", "No"]).default("No"),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        match self.stage.clone() {
            Stage::Format => match i {
                Input::Enter => Ok(self.after_format()),
                Input::Keyword(k) | Input::Text(k) => {
                    let t = k.trim();
                    let lower = t.to_ascii_lowercase();
                    if lower == "dxf" {
                        self.format = FileFormat::Dxf;
                    } else if lower == "template" || lower == "dwt" {
                        self.format = FileFormat::Template;
                    } else if lower == "dwg" {
                        self.format = FileFormat::Dwg;
                    } else if let Some(v) = typed_version(t) {
                        self.format = FileFormat::Dwg;
                        self.version = v;
                    } else if too_old(t) {
                        s.echo(format!("{} files are not available yet: the oldest version CADCraft writes is 2000.", short(t).to_ascii_uppercase()));
                        return Ok(Step::Continue);
                    } else {
                        s.echo(format!("Unknown file format \"{}\": enter 2000, 2004, 2007, 2010, 2013, 2018, DXF or Template.", short(t)));
                        return Ok(Step::Continue);
                    }
                    Ok(self.after_format())
                }
                _ => Ok(Step::Continue),
            },
            Stage::DxfVersion => match i {
                Input::Enter => {
                    self.stage = Stage::DxfEncoding;
                    Ok(Step::Continue)
                }
                Input::Keyword(k) | Input::Text(k) => {
                    match typed_version(&k) {
                        Some(v) => {
                            self.version = v;
                            self.stage = Stage::DxfEncoding;
                        }
                        None if too_old(&k) => s.echo(format!(
                            "{} DXF is not available yet: the oldest version CADCraft writes is 2000.",
                            short(&k).to_ascii_uppercase()
                        )),
                        None => s.echo(format!("Unknown DXF version \"{}\": enter 2000, 2004, 2007, 2010, 2013 or 2018.", short(&k))),
                    }
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Stage::DxfEncoding => match i {
                Input::Enter => {
                    self.stage = Stage::Name;
                    Ok(Step::Continue)
                }
                Input::Keyword(k) | Input::Text(k) => {
                    match k.trim().to_ascii_lowercase().as_str() {
                        "ascii" | "a" => self.binary = false,
                        "binary" | "b" => self.binary = true,
                        _ => {
                            s.echo(format!("Unknown encoding \"{}\": enter Ascii or Binary.", short(&k)));
                            return Ok(Step::Continue);
                        }
                    }
                    self.stage = Stage::Name;
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Stage::Name => {
                let typed = match i {
                    Input::Enter => String::new(),
                    Input::Text(t) | Input::Keyword(t) => t,
                    _ => return Ok(Step::Continue),
                };
                let t = typed.trim();
                // A name with spaces may come quoted, as AutoCAD accepts it.
                let t = t.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(t).trim();
                if t.chars().count() > MAX_NAME {
                    s.echo(format!("The file name is too long (more than {MAX_NAME} characters)."));
                    return Ok(Step::Continue);
                }
                let path = if t.is_empty() { self.default_name(s) } else { with_extension(t, self.format) };
                if exists(&path) {
                    self.stage = Stage::Replace(path);
                    return Ok(Step::Continue);
                }
                Ok(self.save(s, &path))
            }
            Stage::Replace(path) => match i {
                Input::Keyword(k) | Input::Text(k) if matches!(k.trim().to_ascii_lowercase().as_str(), "yes" | "y") => Ok(self.save(s, &path)),
                Input::Keyword(_) | Input::Text(_) | Input::Enter => {
                    // No (the default): ask for another name.
                    self.stage = Stage::Name;
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
        }
    }
}

/// Whether saving to `path` replaces a file (never on the web: a download replaces nothing).
fn exists(path: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::path::Path::new(path).exists()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_get_the_format_extension() {
        assert_eq!(with_extension("plan", FileFormat::Dwg), "plan.dwg");
        assert_eq!(with_extension("plan.DWG", FileFormat::Dwg), "plan.DWG");
        assert_eq!(with_extension("dir.v2/plan.dxf", FileFormat::Dwg), "dir.v2/plan.dwg");
        assert_eq!(with_extension("plan.v2", FileFormat::Dxf), "plan.v2.dxf");
        assert_eq!(with_extension("plan.dwg", FileFormat::Template), "plan.dwt");
        assert_eq!(FileFormat::of_path("a/b.DWT"), Some(FileFormat::Template));
        assert_eq!(FileFormat::of_path("a/b"), Some(FileFormat::Dxf));
        assert_eq!(FileFormat::of_path("a/b.svg"), None);
    }
}
