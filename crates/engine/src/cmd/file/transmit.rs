//! ETRANSMIT: a portable ZIP of a snapshot and its drawing-owned file references.
//! Paths are rewritten only in copies. Dependencies use the same path policy as xref loading.

use serde_json::{Value, json};

use super::super::{CommandSpec, bad};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const CMD: &str = "etransmit";

pub(super) fn spec() -> CommandSpec {
    CommandSpec::new(CMD, "eTransmit...", run)
        .menu(&["File", "eTransmit..."])
        .params("{path: new .zip file, drawing?: root .dxf/.dwg name, strict?: bool (default false)} → {path, drawing, files, missing, warnings, complete}")
        .noundo()
        .interactive(|_| Ok(Box::new(TransmitM)))
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::run(s, p)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad(CMD, "drawing packages need local dependency files and are not available on the web yet"))
    }
}

struct TransmitM;
impl Interactive for TransmitM {
    fn name(&self) -> &'static str {
        "ETRANSMIT"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Enter name of new ZIP package", Accept::TEXT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Cancel | Input::Enter => Ok(Step::Cancel),
            Input::Text(t) if !t.trim().is_empty() => {
                let p = t.trim().trim_matches('"');
                let path = if p.to_ascii_lowercase().ends_with(".zip") { p.to_string() } else { format!("{p}.zip") };
                match run(s, &json!({"path": path})) {
                    Ok(_) => Ok(Step::Done),
                    Err(e) => {
                        s.echo(e.to_string());
                        Ok(Step::Continue)
                    }
                }
            }
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::{BTreeMap, VecDeque};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use cadcraft_doc::{Drawing, EntityKind, EntityStore, MAX_BLOCK_DEPTH};
    use serde::Serialize;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::cmd::file::{drops_unknown, io, not_saved, not_saved_message};

    const MAX_FILES: usize = 1000;
    const MAX_REFERENCES: usize = 10_000;
    const MAX_OBJECTS: usize = 1_000_000;
    const MAX_FILE_BYTES: u64 = 64 << 20;
    const MAX_TOTAL_BYTES: u64 = 256 << 20;
    const LIMITATIONS: &[&str] = &[
        "Includes modeled xrefs, raster images and table CSV data links only; references in unmodeled objects are not collected.",
        "Fonts, external plot configurations and application support files are not included.",
        "Xrefs use their on-disk content; unsaved edits in other open drawings are not included.",
    ];

    #[derive(Serialize)]
    struct Entry {
        path: String,
        kind: &'static str,
        bytes: usize,
    }

    #[derive(Serialize)]
    struct Missing {
        drawing: String,
        kind: &'static str,
        reference: String,
        reason: String,
    }

    struct Pending {
        doc: Drawing,
        source: Option<PathBuf>,
        name: String,
        depth: usize,
    }

    struct Package {
        zip: ZipWriter<File>,
        // A file used as an asset is distinct from a drawing whose paths must be rewritten.
        names: BTreeMap<(PathBuf, bool), String>,
        pending: VecDeque<Pending>,
        files: Vec<Entry>,
        missing: Vec<Missing>,
        warnings: Vec<String>,
        input_bytes: u64,
        output_bytes: u64,
        references: usize,
        objects: usize,
    }

    fn error(e: impl std::fmt::Display) -> crate::EngineError {
        bad(CMD, e.to_string())
    }

    pub(super) fn run(s: &mut Session, p: &Value) -> Result<Value> {
        let params = p.as_object().ok_or_else(|| error("expected {path, drawing?, strict?}"))?;
        if let Some(key) = params.keys().find(|k| !matches!(k.as_str(), "path" | "drawing" | "strict")) {
            return Err(error(format!("unknown parameter `{key}`")));
        }
        let path = params.get("path").and_then(Value::as_str).filter(|p| !p.trim().is_empty()).ok_or_else(|| error("`path` is required"))?;
        let target = Path::new(path);
        if !target.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
            return Err(error("`path` must name a .zip file"));
        }
        let strict = match params.get("strict") {
            None => false,
            Some(Value::Bool(v)) => *v,
            _ => return Err(error("`strict` must be true or false")),
        };
        if target.try_exists().map_err(error)? {
            return Err(error("the destination already exists; choose a new package name"));
        }
        let st = s.state()?;
        let source = st.path.as_deref().map(std::path::absolute).transpose().map_err(error)?;
        let default_name = source.as_ref().and_then(|p| p.file_name()).and_then(|p| p.to_str()).unwrap_or("Drawing.dxf");
        let default_name = if Path::new(default_name).extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf")) {
            default_name.to_string()
        } else {
            Path::new(default_name).with_extension("dxf").to_string_lossy().into_owned()
        };
        let requested = match params.get("drawing") {
            None => &default_name,
            Some(Value::String(n)) => n,
            _ => return Err(error("`drawing` must be a file name")),
        };
        if requested.is_empty() || requested.contains(['/', '\\']) {
            return Err(error("`drawing` must be a .dxf or .dwg file name without folders"));
        }
        let root = safe_name(requested);
        if !Path::new(&root).extension().is_some_and(|e| e.eq_ignore_ascii_case("dxf") || e.eq_ignore_ascii_case("dwg")) {
            return Err(error("`drawing` must have a .dxf or .dwg extension"));
        }
        let doc = st.drawing().committed().into_owned();
        let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let temp = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
        let mut package = Package {
            zip: ZipWriter::new(temp.reopen().map_err(error)?),
            names: BTreeMap::new(),
            pending: VecDeque::new(),
            files: Vec::new(),
            missing: Vec::new(),
            warnings: Vec::new(),
            input_bytes: 0,
            output_bytes: 0,
            references: 0,
            objects: 0,
        };
        if let Some(src) = &source {
            package.names.insert((canonical(src), true), root.clone());
        }
        package.pending.push_back(Pending { doc, source, name: root.clone(), depth: 0 });
        while let Some(mut drawing) = package.pending.pop_front() {
            package.rewrite(&mut drawing)?;
            for note in &drawing.doc.read_notes {
                package.warnings.push(format!("{}: {note}", drawing.name));
            }
            if drops_unknown(&drawing.name)
                && let Some(note) = not_saved_message(&not_saved(&drawing.doc))
            {
                package.warnings.push(format!("{}: {note}", drawing.name));
            }
            let hooks = io().ok_or_else(|| error("file formats are not available in this build"))?;
            let bytes = (hooks.write)(&drawing.doc, &drawing.name).map_err(error)?;
            package.add(&drawing.name, "drawing", &bytes)?;
        }
        let complete = package.missing.is_empty() && package.warnings.is_empty();
        let details: Vec<String> = package
            .missing
            .iter()
            .map(|m| format!("{}: {} ({}): {}", m.drawing, m.kind, m.reference, m.reason))
            .chain(package.warnings.iter().cloned())
            .take(5)
            .map(|line| line.chars().take(500).collect())
            .collect();
        if strict && !complete {
            return Err(error(format!(
                "package is incomplete: {} missing dependencies, {} warnings; no package written. {}",
                package.missing.len(),
                package.warnings.len(),
                details.join("; ")
            )));
        }
        let report = json!({
            "schema": 1, "drawing": root, "files": package.files, "missing": package.missing,
            "warnings": package.warnings, "complete": complete, "limitations": LIMITATIONS,
        });
        let manifest = serde_json::to_vec_pretty(&report).map_err(error)?;
        package.add("manifest.json", "manifest", &manifest)?;
        let archive = package.zip.finish().map_err(error)?;
        archive.sync_all().map_err(error)?;
        drop(archive);
        // No existing file is replaced, including one created since the initial check.
        temp.persist_noclobber(target).map_err(error)?;
        let mut result = report;
        if let Some(o) = result.as_object_mut() {
            o.insert("path".into(), json!(path));
        }
        for line in details {
            s.echo(line);
        }
        s.echo(format!(
            "Packaged {root}: {} files, {} missing dependencies, {} warnings.",
            package.files.len().saturating_sub(1),
            package.missing.len(),
            package.warnings.len()
        ));
        Ok(result)
    }

    impl Package {
        fn add(&mut self, name: &str, kind: &'static str, bytes: &[u8]) -> Result<()> {
            let len = u64::try_from(bytes.len()).map_err(error)?;
            if len > MAX_FILE_BYTES || self.output_bytes.saturating_add(len) > MAX_TOTAL_BYTES {
                return Err(error("package exceeds the 64 MiB per-file or 256 MiB total output limit"));
            }
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored).unix_permissions(0o644);
            self.zip.start_file(name, opts).map_err(error)?;
            self.zip.write_all(bytes).map_err(error)?;
            self.output_bytes += len;
            self.files.push(Entry { path: name.into(), kind, bytes: bytes.len() });
            Ok(())
        }

        fn rewrite(&mut self, drawing: &mut Pending) -> Result<()> {
            self.objects = self.objects.saturating_add(drawing.doc.blocks.len()).saturating_add(drawing.doc.layouts.len());
            if self.objects > MAX_OBJECTS {
                return Err(error("package exceeds the 1,000,000 object scan limit"));
            }
            let host = drawing.source.as_deref();
            for b in drawing.doc.blocks.values_mut().filter(|b| !b.name.contains('|')) {
                let b = Arc::make_mut(b);
                if let Some(path) = &mut b.xref_path {
                    if let Some(new) = self.dependency(path, true, "xref", host, &drawing.name, drawing.depth)? {
                        *path = new;
                    }
                } else {
                    self.store(&mut b.entities, host, &drawing.name, drawing.depth)?;
                }
            }
            self.store(&mut drawing.doc.model, host, &drawing.name, drawing.depth)?;
            for layout in &mut drawing.doc.layouts {
                self.store(&mut layout.entities, host, &drawing.name, drawing.depth)?;
            }
            Ok(())
        }

        fn store(&mut self, store: &mut EntityStore, host: Option<&Path>, owner: &str, depth: usize) -> Result<()> {
            self.objects = self.objects.saturating_add(store.len());
            if self.objects > MAX_OBJECTS {
                return Err(error("package exceeds the 1,000,000 object scan limit"));
            }
            // Keep only the references to change, not a copy of the whole entity store.
            let mut edits = Vec::new();
            for e in store.iter() {
                let (path, kind) = match &e.kind {
                    EntityKind::Image(i) => (&i.path, "image"),
                    EntityKind::Table(t) => match &t.data_link {
                        Some(link) => (&link.path, "dataLink"),
                        None => continue,
                    },
                    _ => continue,
                };
                if let Some(new) = self.dependency(path, false, kind, host, owner, depth)? {
                    edits.push((e.handle, new));
                }
            }
            for (handle, path) in edits {
                store.modify(handle, |e| match &mut e.kind {
                    EntityKind::Image(i) => i.path = path,
                    EntityKind::Table(t) => {
                        if let Some(link) = &mut t.data_link {
                            link.path = path;
                        }
                    }
                    _ => {}
                });
            }
            Ok(())
        }

        fn dependency(
            &mut self,
            stored: &str,
            drawing: bool,
            kind: &'static str,
            host: Option<&Path>,
            owner: &str,
            depth: usize,
        ) -> Result<Option<String>> {
            self.references = self.references.saturating_add(1);
            if self.references > MAX_REFERENCES {
                return Err(error("package exceeds the 10,000 reference limit"));
            }
            let result = self.collect(stored, drawing, host, depth);
            match result {
                Ok((name, bytes)) => {
                    if let Some(bytes) = bytes {
                        // A failed archive write aborts rather than publishing a damaged ZIP.
                        self.add(&name, kind, &bytes)?;
                    }
                    Ok(Some(if owner.contains('/') { format!("../{name}") } else { name }))
                }
                Err(reason) => {
                    self.missing.push(Missing { drawing: owner.into(), kind, reference: stored.chars().take(4096).collect(), reason });
                    Ok(None)
                }
            }
        }

        fn collect(
            &mut self,
            stored: &str,
            drawing: bool,
            host: Option<&Path>,
            depth: usize,
        ) -> std::result::Result<(String, Option<Vec<u8>>), String> {
            if stored.len() > 4096 {
                return Err("reference path exceeds 4096 bytes".into());
            }
            let file = find(stored, host)?;
            let key = (canonical(&file), drawing);
            if let Some(name) = self.names.get(&key) {
                return Ok((name.clone(), None));
            }
            if self.names.len() >= MAX_FILES {
                return Err("package exceeds 1000 dependency files".into());
            }
            if drawing && depth >= MAX_BLOCK_DEPTH {
                return Err("xref nesting is too deep".into());
            }
            let bytes = self.read(&file)?;
            let basename = file.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            let name = format!("{}/{:04}-{}", if drawing { "drawings" } else { "assets" }, self.names.len(), safe_name(basename));
            if drawing {
                if !Path::new(&name)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("dxf") || e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dwt"))
                {
                    return Err("xref must be a DXF, DWG or DWT drawing".into());
                }
                let hooks = io().ok_or("file formats are not available in this build")?;
                let doc = (hooks.read)(&bytes, &name)?;
                self.pending.push_back(Pending { doc, source: Some(file), name: name.clone(), depth: depth.saturating_add(1) });
            }
            self.names.insert(key, name.clone());
            Ok((name, if drawing { None } else { Some(bytes) }))
        }

        fn read(&mut self, path: &Path) -> std::result::Result<Vec<u8>, String> {
            let file = File::open(path).map_err(|e| e.to_string())?;
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err("dependency is not a regular file".into());
            }
            let limit = MAX_FILE_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(self.input_bytes));
            if meta.len() > limit {
                return Err("dependency exceeds the 64 MiB per-file or 256 MiB total input limit".into());
            }
            let mut bytes = Vec::new();
            file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            let len = u64::try_from(bytes.len()).map_err(|e| e.to_string())?;
            self.input_bytes = self.input_bytes.saturating_add(len);
            if len > limit {
                return Err("dependency grew past the input limit while reading".into());
            }
            Ok(bytes)
        }
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// Resolve stored paths with the existing policy, then try the basename beside the host,
    /// just as xref loading does. File URLs and network paths obey the document-path policy.
    fn find(stored: &str, host: Option<&Path>) -> std::result::Result<PathBuf, String> {
        if stored.trim().is_empty() {
            return Err("empty reference path".into());
        }
        let resolved = crate::doc_path::resolve(stored, host);
        if let Ok(path) = &resolved
            && path.is_file()
        {
            return Ok(path.clone());
        }
        let basename = stored.rsplit(['/', '\\']).next().unwrap_or(stored);
        if host.is_some()
            && let Ok(path) = crate::doc_path::resolve(basename, host)
            && path.is_file()
        {
            return Ok(path);
        }
        Err(match resolved {
            Ok(_) => "file not found".into(),
            Err(reason) => format!("path not followed: {reason}"),
        })
    }

    /// Archive components are generated, never taken verbatim from drawing-owned paths.
    fn safe_name(name: &str) -> String {
        let base: String = name.chars().take(120).map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
        let base = base.trim_matches('.');
        let stem = base.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes().get(3).is_some_and(u8::is_ascii_digit));
        format!("{}{}", if reserved { "file_" } else { "" }, if base.is_empty() { "file" } else { base })
    }
}
