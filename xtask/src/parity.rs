//! `cargo xtask parity`: compare the reference app's menu tree (feature names only, in
//! `xtask/data/menu-catalog.txt`) with the live command registry (`cadcraft-cli commands`), and
//! write `docs/parity-checklist.md` (dated; the assessment that interprets it is
//! `docs/target-app-parity.md`).

use std::path::Path;
use std::process::Command;

/// Leaf menu items with their path ("Draw > Circle > 3 Points").
pub fn leaves(catalog: &str) -> Vec<String> {
    let lines: Vec<&str> = catalog.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let ind = l.len() - l.trim_start().len();
        while stack.last().is_some_and(|(d, _)| *d >= ind) {
            stack.pop();
        }
        stack.push((ind, l.trim().to_string()));
        let next_ind = lines.get(i + 1).map(|n| n.len() - n.trim_start().len()).unwrap_or(0);
        if next_ind <= ind && ind > 0 {
            out.push(stack.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join(" > "));
        }
    }
    out
}

/// Where the generated checklist goes.
pub const OUT: &str = "docs/parity-checklist.md";

/// Today's UTC date as `YYYY-MM-DD` (from the system clock).
fn today() -> String {
    date(std::time::SystemTime::now())
}

/// `t` as a UTC `YYYY-MM-DD`.
fn date(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(t).format("%Y-%m-%d").to_string()
}

pub fn run(root: &Path) -> Result<(), String> {
    let catalog = std::fs::read_to_string(root.join("xtask/data/menu-catalog.txt")).map_err(|e| format!("menu catalog: {e}"))?;
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args(["run", "-q", "-p", "cadcraft-cli", "--", "commands"])
        .output()
        .map_err(|e| format!("cadcraft-cli: {e}"))?;
    let cmds: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("commands JSON: {e}"))?;
    let paths: Vec<String> = cmds
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["menu"].as_array())
                .filter(|m| !m.is_empty())
                .map(|m| m.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" > ").to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let labels: Vec<String> = paths.iter().filter_map(|p| p.rsplit(" > ").next().map(str::to_string)).collect();
    let ls = leaves(&catalog);
    let mut by_menu: Vec<(String, usize, usize)> = Vec::new();
    let mut missing = Vec::new();
    let mut hit = 0;
    for l in &ls {
        let top = l.split(" > ").next().unwrap_or("").to_string();
        let lower = l.to_ascii_lowercase();
        let leaf = lower.rsplit(" > ").next().unwrap_or("").to_string();
        let ok = paths.contains(&lower) || labels.contains(&leaf);
        if ok {
            hit += 1;
        } else {
            missing.push(l.clone());
        }
        match by_menu.iter_mut().find(|(m, ..)| *m == top) {
            Some(e) => {
                e.1 += 1;
                e.2 += usize::from(ok);
            }
            None => by_menu.push((top, 1, usize::from(ok))),
        }
    }
    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
    let mut md = format!(
        "# Parity checklist\n\n> **Generated:** {} by `cargo xtask parity`. Do not edit by hand.\n\nMenu-breadth parity: a reference menu item counts as covered when a CADCraft command is registered under the same menu label. It measures breadth, not depth; see [target-app-parity.md](target-app-parity.md) for the weighted assessment.\n\n**Menu breadth: {hit} / {} items ({:.0}%)** · {} registered commands.\n\n| Menu | Items | Covered | % |\n|---|---|---|---|\n",
        today(),
        ls.len(),
        pct(hit, ls.len()),
        cmds.as_array().map(Vec::len).unwrap_or(0)
    );
    for (m, n, k) in &by_menu {
        md += &format!("| {m} | {n} | {k} | {:.0}% |\n", pct(*k, *n));
    }
    md += "\n## Not yet covered\n\n";
    for m in &missing {
        md += &format!("- {m}\n");
    }
    std::fs::write(root.join(OUT), md).map_err(|e| format!("{OUT}: {e}"))?;
    println!("parity: {hit}/{} menu items ({:.0}%) → {OUT}", ls.len(), pct(hit, ls.len()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        let day = |d: u64| date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(d * 86_400));
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(11_016), "2000-02-29");
        assert_eq!(day(20_736), "2026-10-10");
    }

    #[test]
    fn leaves_with_paths() {
        let c = "Draw\n    Line\n    Circle\n        3 Points\n        2 Points\nModify\n    Move\n";
        assert_eq!(leaves(c), vec!["Draw > Line", "Draw > Circle > 3 Points", "Draw > Circle > 2 Points", "Modify > Move"]);
    }
}
