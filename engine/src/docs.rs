//! The owner's private documents: notes and records moved out of Notion, as
//! `docs.py` does it.
//!
//! `bin/cashcast notion import EXPORT` reads a Notion "Markdown & CSV" export
//! (the .zip Notion emails, or its unzipped folder) on this computer. Nothing
//! goes over the network. Pages become Markdown files, databases become
//! Markdown tables, and images and PDFs are copied, all under the user's
//! `docs/notion/` (folders 700, files 600). The CLI prints counts only.

use crate::core::paths::{AppError, UserHome};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const FILE_TYPES: [&str; 7] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".pdf"];
const MAX_FILE: u64 = 25 * 1024 * 1024;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
pub const IMPORT_DIR: &str = "notion";
const TOO_BIG: &str = "the export is larger than 1 GB; export the Finances page alone";

fn docs_err(msg: impl Into<String>) -> AppError {
    AppError::new("docserror", msg)
}

/// A case Python stopped on with a traceback: one error line, exit 1.
fn unported(msg: impl Into<String>) -> AppError {
    AppError::new("crash", msg)
}

#[derive(Default)]
pub struct ImportReport {
    pub pages: i64,
    pub tables: i64,
    pub files: i64,
    pub skipped: Vec<&'static str>, // reasons only, never names
}

impl ImportReport {
    pub fn render(&self, where_: &str) -> String {
        let mut lines = vec![format!(
            "Imported {} pages, {} tables and {} files into {where_}.",
            self.pages, self.tables, self.files
        )];
        if !self.skipped.is_empty() {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for r in &self.skipped {
                *counts.entry(r).or_default() += 1;
            }
            let parts: Vec<String> = counts.iter().map(|(r, n)| format!("{n} ({r})")).collect();
            lines.push(format!("Skipped: {}.", parts.join(", ")));
        }
        lines.push("Open them in the web app (bin/cashcast web → Documents).".into());
        lines.join("\n")
    }
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// `NOTION_ID.sub("", name)`: drops `\s+` + 32 hex digits when followed by the
/// end, a dot, or `_all.csv` at the end (any case).
fn drop_ids(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let mut e = i;
            while e < chars.len() && chars[e].is_whitespace() {
                e += 1;
            }
            let h = e + 32;
            if h <= chars.len() && chars[e..h].iter().all(|c| is_hex(*c)) {
                let rest: String = chars[h..].iter().collect();
                if rest.is_empty() || rest.starts_with('.') || rest.to_lowercase() == "_all.csv" {
                    i = h;
                    continue;
                }
            }
            out.extend(&chars[i..e]);
            i = e;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 'Taxes 57c71170a84945e5a726884773c09e90.md' → 'Taxes.md'.
pub fn clean_part(name: &str) -> String {
    let out: String = drop_ids(name)
        .trim()
        .chars()
        .map(|c| {
            if "<>:\"\\|?*".contains(c) || (c as u32) < 0x20 {
                '-'
            } else {
                c
            }
        })
        .collect();
    if out.is_empty() {
        "untitled".into()
    } else {
        out
    }
}

/// A clean relative path as its parts, or None for an unsafe one.
pub fn clean_path(rel: &str) -> Option<Vec<String>> {
    if rel.starts_with('/') {
        return None;
    }
    let parts: Vec<&str> = rel
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.is_empty() || parts.contains(&"..") {
        return None;
    }
    let cleaned: Vec<String> = parts.iter().map(|p| clean_part(p)).collect();
    // Dropping a Notion id can turn a part into ".." or "."; refuse it then too.
    if cleaned.iter().any(|p| p == ".." || p == ".") {
        return None;
    }
    Some(cleaned)
}

/// `PurePath.suffix` of the last part.
fn suffix(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => &name[i..],
        _ => "",
    }
}

fn stem(name: &str) -> &str {
    let s = suffix(name);
    &name[..name.len() - s.len()]
}

/// `re.sub(r"(_all)?\.csv$", ".md", path, flags=re.I)`.
fn as_md(path: &str) -> String {
    let low = path.to_lowercase();
    if low.ends_with("_all.csv") && path.is_char_boundary(path.len() - 8) {
        format!("{}.md", &path[..path.len() - 8])
    } else if low.ends_with(".csv") && path.is_char_boundary(path.len() - 4) {
        format!("{}.md", &path[..path.len() - 4])
    } else {
        path.to_string()
    }
}

fn all_rows_name(name: &str) -> String {
    if name.to_lowercase().ends_with(".csv") {
        format!("{}_all.csv", &name[..name.len() - 4])
    } else {
        name.to_string()
    }
}

// --- Reading the export ----------------------------------------------------

struct Entries {
    files: BTreeMap<String, Vec<u8>>,
    oversized: BTreeSet<String>,
    total: u64,
}

impl Entries {
    fn add(&mut self, name: String, data: Vec<u8>) -> Result<(), AppError> {
        self.total += data.len() as u64;
        if self.total > MAX_TOTAL {
            return Err(docs_err(TOO_BIG));
        }
        self.files.insert(name, data);
        Ok(())
    }

    fn read_zip<R: Read + std::io::Seek>(
        &mut self,
        mut z: zip::ZipArchive<R>,
        nested: bool,
    ) -> Result<(), AppError> {
        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(|e| unported(e.to_string()))?;
            if f.is_dir() {
                continue;
            }
            let name = f.name().map_err(|e| unported(e.to_string()))?.to_string();
            let size = f.size();
            if name.to_lowercase().ends_with(".zip") && !nested {
                if size > MAX_TOTAL {
                    return Err(docs_err(TOO_BIG));
                }
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)
                    .map_err(|e| unported(e.to_string()))?;
                let inner = zip::ZipArchive::new(std::io::Cursor::new(buf))
                    .map_err(|e| unported(e.to_string()))?;
                self.read_zip(inner, true)?;
            } else if size > MAX_FILE {
                self.oversized.insert(name);
            } else {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)
                    .map_err(|e| unported(e.to_string()))?;
                self.add(name, buf)?;
            }
        }
        Ok(())
    }

    fn read_dir(&mut self, root: &Path, dir: &Path) -> Result<(), AppError> {
        let mut items: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| unported(e.to_string()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        items.sort();
        for p in items {
            let meta = std::fs::symlink_metadata(&p).map_err(|e| unported(e.to_string()))?;
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                self.read_dir(root, &p)?;
            } else if meta.is_file() {
                let rel = p.strip_prefix(root).map_err(|e| unported(e.to_string()))?;
                let name = rel
                    .to_str()
                    .ok_or_else(|| unported("a file name that is not UTF-8"))?
                    .to_string();
                if meta.len() > MAX_FILE {
                    self.oversized.insert(name);
                } else {
                    let data = std::fs::read(&p).map_err(|e| unported(e.to_string()))?;
                    self.add(name, data)?;
                }
            }
        }
        Ok(())
    }
}

fn entries(source: &Path) -> Result<Entries, AppError> {
    let mut out = Entries {
        files: BTreeMap::new(),
        oversized: BTreeSet::new(),
        total: 0,
    };
    if source.is_dir() {
        out.read_dir(source, source)?;
        return Ok(out);
    }
    let file = std::fs::File::open(source).map_err(|e| unported(e.to_string()))?;
    match zip::ZipArchive::new(file) {
        Ok(z) => out.read_zip(z, false)?,
        Err(_) => {
            return Err(docs_err(
                "not a Notion export: give the .zip Notion sent, or its unzipped folder",
            ));
        }
    }
    Ok(out)
}

/// Python's `csv.reader` with the default dialect, enough for Notion's CSV.
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false; // inside a quoted field
    let mut started = false; // the record has a character
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                started = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                started = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if started || !field.is_empty() {
                    row.push(std::mem::take(&mut field));
                }
                rows.push(std::mem::take(&mut row));
                started = false;
            }
            _ => {
                field.push(c);
                started = true;
            }
        }
    }
    if started || !field.is_empty() || quoted {
        row.push(field);
        rows.push(row);
    }
    rows
}

fn table_md(title: &str, data: &[u8]) -> String {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let text = String::from_utf8_lossy(data);
    let rows = csv_rows(&text);
    if rows.is_empty() {
        return format!("# {title}\n\n(empty table)\n");
    }
    let cell = |c: &str| {
        c.replace('|', "\\|")
            .replace(['\r', '\n'], " ")
            .trim()
            .to_string()
    };
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let line = |r: &Vec<String>| {
        let mut cells: Vec<String> = r.iter().map(|c| cell(c)).collect();
        cells.resize(width, String::new());
        format!("| {} |", cells.join(" | "))
    };
    let mut out = vec![
        format!("# {title}"),
        String::new(),
        line(&rows[0]),
        format!("|{}", "---|".repeat(width)),
    ];
    out.extend(rows[1..].iter().map(line));
    out.join("\n") + "\n"
}

/// `posixpath.normpath`.
fn normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let lead = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        if p.is_empty() || p == "." {
            continue;
        }
        if p != ".." || (lead.is_empty() && parts.is_empty()) || parts.last() == Some(&"..") {
            parts.push(p);
        } else if !parts.is_empty() {
            parts.pop();
        }
    }
    let out = format!("{lead}{}", parts.join("/"));
    if out.is_empty() { ".".into() } else { out }
}

fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => {
            let head = &path[..=i];
            let trimmed = head.trim_end_matches('/');
            if trimmed.is_empty() { head } else { trimmed }
        }
        None => "",
    }
}

fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') || a.is_empty() {
        b.to_string()
    } else if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

fn hexval(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// `urllib.parse.unquote` (UTF-8, errors replaced).
fn unquote(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hexval(b[i + 1]), hexval(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `urllib.parse.quote(s)` with the default safe '/'.
fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn has_scheme(target: &str) -> bool {
    let mut cs = target.chars();
    if !cs.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in cs {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || "+.-".contains(c)) {
            return false;
        }
    }
    false
}

/// Links between exported pages become `doc:notion/<path>` links to the
/// cleaned files. `page` is the page's name in the export; `final_` maps each
/// name in the export to the file it became.
fn rewrite_links(md: &str, page: &str, final_: &HashMap<String, String>) -> String {
    let folder = dirname(page);
    let fix = |label: &str, bang: &str, target: &str, whole: &str| -> String {
        if has_scheme(target) {
            return whole.to_string();
        }
        let raw = unquote(target.split('#').next().unwrap_or(""));
        let joined = if raw.is_empty() {
            page.to_string()
        } else {
            normpath(&join(folder, &raw))
        };
        if joined.starts_with('/') || joined.starts_with("../") || joined == ".." {
            return label.to_string();
        }
        let dest = match final_.get(&joined) {
            Some(d) => d.clone(),
            None => {
                let Some(parts) = clean_path(&joined) else {
                    return label.to_string();
                };
                let d = parts.join("/");
                if suffix(parts.last().map_or("", String::as_str)).to_lowercase() == ".csv" {
                    as_md(&d)
                } else {
                    d
                }
            }
        };
        format!("{bang}[{label}](doc:{IMPORT_DIR}/{})", quote(&dest))
    };
    // LINK = (!?)\[([^\]]*)\]\(([^)\s]+)\), scanned left to right.
    let mut out = String::new();
    let mut i = 0;
    let b = md.as_bytes();
    while i < md.len() {
        let bang = b[i] == b'!';
        let open = if bang { i + 1 } else { i };
        if open < md.len() && b[open] == b'[' {
            let rest = &md[open + 1..];
            if let Some(close) = rest.find(']') {
                let label = &rest[..close];
                let after = &rest[close + 1..];
                if let Some(t) = after.strip_prefix('(') {
                    let end = t
                        .char_indices()
                        .find(|(_, c)| *c == ')' || c.is_whitespace())
                        .map(|(j, _)| j);
                    if let Some(j) = end
                        && j > 0
                        && t[j..].starts_with(')')
                    {
                        let target = &t[..j];
                        let stop = open + 1 + close + 1 + 1 + j + 1;
                        let whole = &md[i..stop];
                        out.push_str(&fix(label, if bang { "!" } else { "" }, target, whole));
                        i = stop;
                        continue;
                    }
                }
            }
        }
        let c = md[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `rel`, or `name (2).md` when another file already has its name. Pages with
/// the same title are common, and macOS ignores case in file names.
fn unique(rel: &str, taken: &mut HashSet<String>) -> String {
    let (dir, name) = match rel.rfind('/') {
        Some(i) => (&rel[..=i], &rel[i + 1..]),
        None => ("", rel),
    };
    let mut out = rel.to_string();
    let mut n = 2;
    while taken.contains(&out.to_lowercase()) {
        out = format!("{dir}{} ({n}){}", stem(name), suffix(name));
        n += 1;
    }
    taken.insert(out.to_lowercase());
    out
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| docs_err(e.to_string()))
}

fn private_dir(p: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(p).map_err(|e| docs_err(e.to_string()))?;
    chmod(p, 0o700)
}

/// Reads `source` and replaces `docs/notion/` with what it holds.
pub fn import_notion(user: &UserHome, source: &Path) -> Result<ImportReport, AppError> {
    if !source.exists() {
        return Err(docs_err(format!(
            "no such file or folder: {}",
            source.display()
        )));
    }
    let Entries {
        files, oversized, ..
    } = entries(source)?;
    let mut report = ImportReport {
        skipped: vec!["over 25 MB"; oversized.len()],
        ..Default::default()
    };
    let names: HashSet<&String> = files.keys().chain(oversized.iter()).collect();

    // 1. Where each file goes, decided before any page is written, so links
    // follow files renamed to keep both of two same-named pages.
    let mut final_: HashMap<String, String> = HashMap::new();
    let mut kinds: Vec<(String, &'static str)> = Vec::new();
    let mut aliases: Vec<(String, String)> = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();
    for name in files.keys() {
        let Some(parts) = clean_path(name) else {
            report.skipped.push("unsafe path");
            continue;
        };
        let mut rel = parts.join("/");
        let suf = suffix(parts.last().map_or("", String::as_str)).to_lowercase();
        let kind = if suf == ".csv" {
            let all_rows = all_rows_name(name);
            if !name.to_lowercase().ends_with("_all.csv") && names.contains(&all_rows) {
                aliases.push((name.clone(), all_rows)); // the _all file has every row
                continue;
            }
            rel = as_md(&rel);
            "table"
        } else if suf == ".md" {
            "page"
        } else if FILE_TYPES.contains(&suf.as_str()) {
            "file"
        } else {
            report.skipped.push("unsupported file type");
            continue;
        };
        kinds.push((name.clone(), kind));
        final_.insert(name.clone(), unique(&rel, &mut taken));
    }
    for (name, target) in aliases {
        if let Some(t) = final_.get(&target).cloned() {
            final_.insert(name, t);
        }
    }

    // 2. The files themselves.
    let mut staged: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, kind) in &kinds {
        let data = &files[name];
        let rel = final_[name].clone();
        match *kind {
            "page" => {
                let text = String::from_utf8_lossy(data);
                staged.push((rel, rewrite_links(&text, name, &final_).into_bytes()));
                report.pages += 1;
            }
            "table" => {
                let base = name.rsplit('/').next().unwrap_or(name);
                let low = base.to_lowercase();
                let base = if low.ends_with("_all.csv") {
                    format!("{}.csv", &base[..base.len() - 8])
                } else {
                    base.to_string()
                };
                staged.push((rel, table_md(&clean_part(stem(&base)), data).into_bytes()));
                report.tables += 1;
            }
            _ => {
                staged.push((rel, data.clone()));
                report.files += 1;
            }
        }
    }
    let docs = user.docs();
    let dest = docs.join(IMPORT_DIR);
    let tmp = docs.join(format!(".{IMPORT_DIR}-new"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| docs_err(e.to_string()))?;
    }
    private_dir(&docs)?;
    private_dir(&tmp)?;
    for (rel, data) in staged {
        let path = tmp.join(&rel);
        if let Some(parent) = path.parent() {
            private_dir(parent)?;
        }
        std::fs::write(&path, data).map_err(|e| docs_err(e.to_string()))?;
        chmod(&path, 0o600)?;
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| docs_err(e.to_string()))?;
    }
    std::fs::rename(&tmp, &dest).map_err(|e| docs_err(e.to_string()))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_part_drops_notion_ids() {
        let id = "57c71170a84945e5a726884773c09e90";
        assert_eq!(clean_part(&format!("Taxes {id}.md")), "Taxes.md");
        assert_eq!(
            clean_part(&format!("Budget {id}_all.csv")),
            "Budget_all.csv"
        );
        assert_eq!(clean_part(&format!("Folder {id}")), "Folder");
        assert_eq!(clean_part("a:b?"), "a-b-");
        assert_eq!(clean_part("  "), "untitled");
    }

    #[test]
    fn clean_path_refuses_parts_that_become_dot_dot() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(clean_path(&format!(".. {id}/x.md")), None);
        assert_eq!(clean_path(&format!(". {id}/x.md")), None);
        assert_eq!(clean_path("../x.md"), None);
        assert!(clean_path(&format!("A {id}/x.md")).is_some());
    }

    #[test]
    fn csv_rows_match_python() {
        assert_eq!(
            csv_rows("a,\"b,c\"\n\n\"x\"\"y\",\r\n"),
            vec![
                vec!["a".to_string(), "b,c".into()],
                vec![],
                vec!["x\"y".into(), "".into()]
            ]
        );
    }

    #[test]
    fn normpath_matches_posixpath() {
        assert_eq!(normpath("a/./b/../c"), "a/c");
        assert_eq!(normpath("../x"), "../x");
        assert_eq!(normpath("a/../.."), "..");
        assert_eq!(normpath("/a/../.."), "/");
    }

    #[test]
    fn links_point_at_cleaned_files() {
        let mut f = HashMap::new();
        f.insert(
            "A/B 0123456789abcdef0123456789abcdef.md".to_string(),
            "A/B (2).md".to_string(),
        );
        let md = "see [b](B%200123456789abcdef0123456789abcdef.md) and [w](https://x.y) ![i](../../out.png)";
        assert_eq!(
            rewrite_links(md, "A/page.md", &f),
            "see [b](doc:notion/A/B%20%282%29.md) and [w](https://x.y) i"
        );
    }
}
