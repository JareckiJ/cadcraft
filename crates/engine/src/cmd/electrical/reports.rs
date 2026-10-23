//! EREPORT: project reports — parts list (BOM), device list, wire list (from–to), terminal plan,
//! PLC I/O list — as rows, as CSV (returned or saved to a file for Excel), or drawn into the
//! drawing as a table on the `REPORT` layer (a report sheet).

use cadcraft_doc::{EntityKind, HAlign, Line, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, bad, point_param, str_param};
use crate::{EngineError, Result, Session};

const REPORT_LAYER: &str = "REPORT";
const TEXT_HEIGHT: f64 = 2.5;
const ROW: f64 = 6.0;
/// Most rows drawn in one table.
const MAX_DRAWN_ROWS: usize = 2_000;
const KINDS: &[&str] = &["bom", "devices", "wirelist", "terminals", "plcio", "cables"];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("ereport", "Electrical Report", run_report).menu(&["Tools", "Electrical", "Reports", "Report..."]).params(
        "{kind: bom | devices | wirelist | terminals | plcio | cables, csv?: bool (return CSV text), path? (save CSV there), \
                 draw?: {at: [x, y]} (draw it as a table)} → {kind, columns, rows, csv?}",
    )]
}

fn text(at: Vec2, value: &str, halign: HAlign, height: f64) -> EntityKind {
    let p = Vec3::new(at.x, at.y, 0.0);
    EntityKind::Text(Text {
        insert: p,
        align_pt: Some(p),
        height,
        value: value.to_string(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign,
        valign: VAlign::Middle,
    })
}

fn line(a: Vec2, b: Vec2) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) })
}

/// Draw a table with its top-left corner at `at`: a title, a header row and `rows`, on the
/// REPORT layer. Returns its height.
pub(super) fn draw_table(s: &mut Session, at: Vec2, title: &str, headers: &[&str], rows: &[Vec<String>]) -> Result<f64> {
    if !at.is_finite() {
        return Err(EngineError::Other("The table position is not a point.".into()));
    }
    let rows = &rows[..rows.len().min(MAX_DRAWN_ROWS)];
    let char_w = TEXT_HEIGHT * 0.75;
    let widths: Vec<f64> = (0..headers.len())
        .map(|c| {
            let longest = rows.iter().filter_map(|r| r.get(c)).map(|v| v.chars().count()).chain([headers[c].chars().count()]).max().unwrap_or(4);
            (longest.min(60) as f64 * char_w + 4.0).max(12.0)
        })
        .collect();
    let width: f64 = widths.iter().sum();
    let mut common = s.current_common();
    common.layer = REPORT_LAYER.to_string();
    let space = s.space();
    let add = |s: &mut Session, k: EntityKind| -> Result<()> {
        s.doc_mut()?.add(&space, common.clone(), k)?;
        Ok(())
    };
    add(s, text(Vec2::new(at.x, at.y - ROW / 2.0), title, HAlign::Left, TEXT_HEIGHT * 1.4))?;
    let top = at.y - ROW;
    let n = rows.len() + 1;
    let bottom = top - ROW * n as f64;
    // Grid: horizontal lines, then column lines.
    for i in 0..=n {
        let y = top - ROW * i as f64;
        add(s, line(Vec2::new(at.x, y), Vec2::new(at.x + width, y)))?;
    }
    let mut x = at.x;
    add(s, line(Vec2::new(x, top), Vec2::new(x, bottom)))?;
    for w in &widths {
        x += w;
        add(s, line(Vec2::new(x, top), Vec2::new(x, bottom)))?;
    }
    for (i, cells) in std::iter::once(headers.iter().map(|h| h.to_string()).collect::<Vec<_>>()).chain(rows.iter().cloned()).enumerate() {
        let y = top - ROW * i as f64 - ROW / 2.0;
        let mut x = at.x;
        for (c, w) in widths.iter().enumerate() {
            let v: String = cells.get(c).map(|v| v.chars().take(60).collect()).unwrap_or_default();
            if !v.is_empty() {
                add(s, text(Vec2::new(x + 2.0, y), &v, HAlign::Left, TEXT_HEIGHT))?;
            }
            x += w;
        }
    }
    Ok(at.y - bottom)
}

/// One CSV field, quoted when it needs to be.
fn csv_field(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r', ';']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_string() }
}

pub(super) fn to_csv(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for r in std::iter::once(headers.iter().map(|h| h.to_string()).collect::<Vec<_>>()).chain(rows.iter().cloned()) {
        out.push_str(&r.iter().map(|v| csv_field(v)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

fn strs(v: &Value) -> String {
    match v {
        Value::Array(a) => a.iter().map(strs).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "),
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The columns and rows of report `kind`.
pub(super) fn report(s: &mut Session, kind: &str) -> Result<(Vec<&'static str>, Vec<Vec<String>>)> {
    // The database first catches up with ordinary edits.
    super::sync::auto(s);
    let q = |s: &mut Session, what: &str| s.execute("edb.query", &json!({ "what": what }));
    Ok(match kind {
        "bom" => {
            let v = q(s, "bom")?;
            let rows = v
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| vec![strs(&r["quantity"]), strs(&r["manufacturer"]), strs(&r["catalog"]), strs(&r["description"]), strs(&r["tags"])])
                .collect();
            (vec!["Qty", "Manufacturer", "Catalogue no.", "Description", "Tags"], rows)
        }
        "devices" => {
            let v = q(s, "devices")?;
            let mut rows: Vec<Vec<String>> = v
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| {
                    let r = &d["graphic"]["reference"];
                    let place = match (r[0].as_u64(), r[1].as_u64()) {
                        (Some(a), Some(b)) => format!("/{a}.{b}"),
                        _ => String::new(),
                    };
                    vec![
                        strs(&d["tag"]),
                        strs(&d["graphic"]["block"]),
                        place,
                        strs(&d["part"]["manufacturer"]),
                        strs(&d["part"]["catalog"]),
                        strs(&d["location"]),
                    ]
                })
                .collect();
            rows.sort();
            (vec!["Tag", "Symbol", "Place", "Manufacturer", "Catalogue no.", "Location"], rows)
        }
        "wirelist" => {
            let v = q(s, "wirelist")?;
            let rows = v.as_array().into_iter().flatten().map(|w| vec![strs(&w["wire"]), strs(&w["from"]), strs(&w["to"])]).collect();
            (vec!["Wire", "From", "To"], rows)
        }
        "terminals" => {
            let mut rows = Vec::new();
            for st in super::terminals::strip_report(s, None)? {
                for t in st["terminals"].as_array().into_iter().flatten() {
                    rows.push(vec![
                        strs(&st["strip"]),
                        strs(&t["number"]),
                        strs(&t["wireTop"]),
                        strs(&t["top"]),
                        strs(&t["bottom"]),
                        strs(&t["wireBottom"]),
                    ]);
                }
            }
            (vec!["Strip", "Terminal", "Wire", "Target (top)", "Target (bottom)", "Wire"], rows)
        }
        "cables" => {
            let mut rows = Vec::new();
            for c in super::conductors::cables(s)? {
                for core in c["cores"].as_array().into_iter().flatten() {
                    rows.push(vec![
                        strs(&c["tag"]),
                        strs(&c["type"]),
                        strs(&core["core"]),
                        strs(&core["wire"]),
                        strs(&core["from"]),
                        strs(&core["to"]),
                    ]);
                }
            }
            (vec!["Cable", "Type", "Core", "Wire", "From", "To"], rows)
        }
        "plcio" => {
            let rows = super::plc::io_list(s)?
                .into_iter()
                .map(|p| vec![p.address, p.card, p.point, p.connected.join(", "), p.wire.unwrap_or_default()])
                .collect();
            (vec!["Address", "Card", "Point", "Connected", "Wire"], rows)
        }
        _ => return Err(bad("ereport", format!("`kind` must be one of {}", KINDS.join(", ")))),
    })
}

fn run_report(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "ereport";
    let kind = str_param(p, "kind").map(str::to_ascii_lowercase).ok_or_else(|| bad(CMD, format!("`kind` ({}) is required", KINDS.join(", "))))?;
    let (headers, rows) = report(s, &kind)?;
    let mut out = json!({ "kind": kind, "columns": headers, "rows": rows });
    let csv = to_csv(&headers, &rows);
    if p.get("csv").and_then(Value::as_bool).unwrap_or(false) {
        out["csv"] = json!(csv);
    }
    if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // A UTF-8 byte order mark, so Excel reads accented and Cyrillic text right.
            let bytes = [&[0xEF, 0xBB, 0xBF][..], csv.as_bytes()].concat();
            crate::atomic_write::write_atomic(std::path::Path::new(path), &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            out["path"] = json!(path);
        }
        #[cfg(target_arch = "wasm32")]
        return Err(bad(CMD, "saving files is not available on the web; pass csv: true"));
    }
    if let Some(at) = p.get("draw").and_then(|d| point_param(d, "at")) {
        let title = match kind.as_str() {
            "bom" => "Parts list",
            "devices" => "Device list",
            "wirelist" => "Wire list",
            "terminals" => "Terminal plan",
            "cables" => "Cable list",
            _ => "PLC I/O list",
        };
        let h = draw_table(s, at, title, &headers, &rows)?;
        out["height"] = json!(h);
    }
    s.log.push(format!("{} report: {} row(s).", kind, rows.len()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn csv_quoting() {
        let c = to_csv(&["A", "B"], &[vec!["x, y".into(), "say \"hi\"".into()], vec!["plain".into(), String::new()]]);
        assert_eq!(c, "A,B\r\n\"x, y\",\"say \"\"hi\"\"\"\r\nplain,\r\n");
    }

    #[test]
    fn reports_from_a_small_circuit() {
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = "reports-test".into();
        }
        s.execute("eladder", &json!({ "top": 240, "bottom": 40, "from": 0, "to": 84 })).unwrap();
        let k = s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [21, 140], "values": { "MFG": "Acme", "CAT": "R-24" } })).unwrap();
        s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [63, 140], "values": { "MFG": "Acme", "CAT": "R-24" } })).unwrap();
        let bom = s.execute("ereport", &json!({ "kind": "bom", "csv": true })).unwrap();
        assert_eq!(bom["rows"][0][0], "2", "{bom}");
        assert!(bom["csv"].as_str().unwrap().starts_with("Qty,Manufacturer"));
        let wl = s.execute("ereport", &json!({ "kind": "wirelist" })).unwrap();
        assert!(!wl["rows"].as_array().unwrap().is_empty());
        let dv = s.execute("ereport", &json!({ "kind": "devices", "draw": { "at": [200, 280] } })).unwrap();
        assert!(dv["rows"].as_array().unwrap().iter().any(|r| r[0] == k["tag"]));
        assert!(dv["height"].as_f64().unwrap() > 10.0);
        assert!(s.execute("ereport", &json!({ "kind": "nonsense" })).is_err());
        assert!(s.execute("ereport", &json!({})).is_err());
        let path = std::env::temp_dir().join(format!("cadcraft-report-{}.csv", std::process::id()));
        s.execute("ereport", &json!({ "kind": "bom", "path": path.to_string_lossy() })).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let _ = std::fs::remove_file(&path);
    }
}
