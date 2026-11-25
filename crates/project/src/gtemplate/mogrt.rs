//! Importing motion graphics template packages (`.mogrt`) as FilmCraft graphics.
//!
//! A `.mogrt` is a ZIP archive. FilmCraft opens only the user's own package, reads the archive
//! directory (PKWARE APPNOTE, the public ZIP specification) and the package's JSON description,
//! and takes from it just the template's name, its frame size and duration, and the default text
//! of its editable text fields. It then builds an original FilmCraft graphic from those texts with
//! its own layout and the bundled font. Nothing else in the package (artwork, fonts, project files,
//! previews) is decoded, rendered or kept, and nothing of it ships with FilmCraft.
//!
//! Every byte is hostile: sizes, offsets and counts are checked, decompression is capped, and the
//! JSON walk is bounded in depth and node count. A package without readable text still imports
//! as a title carrying the template's name.

use filmcraft_time::{TICKS_PER_SECOND, Tick};
use serde_json::Value;

use super::{GraphicsTemplate, TemplateControl, TemplateError, ctl, rgb, set, slider, slug, text};
use crate::keyframe::ParamValue;

/// File extension of motion graphics template packages.
pub const MOGRT_EXTENSION: &str = "mogrt";

/// Largest package read (bytes).
pub const MAX_PACKAGE: usize = 512 * 1024 * 1024;
/// Largest JSON description inflated (bytes).
const MAX_JSON: usize = 8 * 1024 * 1024;
/// Most archive entries looked at.
const MAX_ENTRIES: usize = 65_536;
/// Most text fields turned into layers.
const MAX_TEXTS: usize = 6;
/// Longest text kept per field (chars).
const MAX_TEXT_CHARS: usize = 2_000;
/// JSON walk limits.
const MAX_DEPTH: usize = 48;
const MAX_NODES: usize = 200_000;

/// Whether `path` names a `.mogrt` package.
pub fn is_mogrt_path(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".mogrt")
}

/// What a `.mogrt` package describes, before it becomes a template.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MogrtInfo {
    pub name: String,
    /// `(label, default text)` of each editable text field, in package order.
    pub texts: Vec<(String, String)>,
    pub canvas: Option<[u32; 2]>,
    pub seconds: Option<f64>,
}

fn foreign(m: impl Into<String>) -> TemplateError {
    TemplateError::NotTemplate(format!("unreadable .mogrt package: {}", m.into()))
}

// ------------------------------------------------------------------------------------------------
// ZIP (PKWARE APPNOTE 6.3): end of central directory, central directory, local headers
// ------------------------------------------------------------------------------------------------

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

struct Entry {
    name: String,
    method: u16,
    compressed: usize,
    size: usize,
    local: usize,
}

/// The archive's central directory.
fn entries(b: &[u8]) -> Result<Vec<Entry>, TemplateError> {
    const EOCD: u32 = 0x0605_4b50;
    const CEN: u32 = 0x0201_4b50;
    if b.len() < 22 {
        return Err(foreign("too short to be a ZIP archive"));
    }
    // the end record is in the last 22 + 65 535 (comment) bytes
    let lo = b.len().saturating_sub(22 + 65_535);
    let eocd = (lo..=b.len() - 22).rev().find(|&i| u32_at(b, i) == Some(EOCD)).ok_or_else(|| foreign("no ZIP directory"))?;
    let count = usize::from(u16_at(b, eocd + 10).ok_or_else(|| foreign("truncated ZIP directory"))?);
    let cd_off = u32_at(b, eocd + 16).ok_or_else(|| foreign("truncated ZIP directory"))? as usize;
    if count == 0xFFFF || cd_off == 0xFFFF_FFFF {
        return Err(foreign("ZIP64 packages are not supported"));
    }
    // offsets stay within the buffer, so the field arithmetic below cannot overflow
    if cd_off > b.len() {
        return Err(foreign("damaged ZIP directory"));
    }
    let mut out = Vec::with_capacity(count.min(MAX_ENTRIES));
    let mut at = cd_off;
    for _ in 0..count.min(MAX_ENTRIES) {
        if u32_at(b, at) != Some(CEN) {
            return Err(foreign("damaged ZIP directory"));
        }
        let field = |o: usize| u16_at(b, at + o).ok_or_else(|| foreign("truncated ZIP directory"));
        let method = field(10)?;
        let compressed = u32_at(b, at + 20).ok_or_else(|| foreign("truncated ZIP directory"))? as usize;
        let size = u32_at(b, at + 24).ok_or_else(|| foreign("truncated ZIP directory"))? as usize;
        let (n, x, c) = (usize::from(field(28)?), usize::from(field(30)?), usize::from(field(32)?));
        let local = u32_at(b, at + 42).ok_or_else(|| foreign("truncated ZIP directory"))? as usize;
        let name = b.get(at + 46..at + 46 + n).ok_or_else(|| foreign("truncated ZIP directory"))?;
        out.push(Entry { name: String::from_utf8_lossy(name).replace('\\', "/"), method, compressed, size, local });
        at = at.checked_add(46 + n + x + c).ok_or_else(|| foreign("damaged ZIP directory"))?;
    }
    Ok(out)
}

/// An entry's bytes, at most `limit` of them.
fn read_entry(b: &[u8], e: &Entry, limit: usize) -> Result<Vec<u8>, TemplateError> {
    const LOC: u32 = 0x0403_4b50;
    if e.size > limit {
        return Err(foreign(format!("`{}` is too large", e.name)));
    }
    if e.local > b.len() {
        return Err(foreign("damaged ZIP entry"));
    }
    if u32_at(b, e.local) != Some(LOC) {
        return Err(foreign("damaged ZIP entry"));
    }
    let n = usize::from(u16_at(b, e.local + 26).ok_or_else(|| foreign("truncated ZIP entry"))?);
    let x = usize::from(u16_at(b, e.local + 28).ok_or_else(|| foreign("truncated ZIP entry"))?);
    let start = e.local.checked_add(30 + n + x).ok_or_else(|| foreign("damaged ZIP entry"))?;
    let end = start.checked_add(e.compressed).ok_or_else(|| foreign("damaged ZIP entry"))?;
    let data = b.get(start..end).ok_or_else(|| foreign("truncated ZIP entry"))?;
    match e.method {
        0 => Ok(data.to_vec()),
        8 => filmcraft_media::still::exr::inflate::inflate(data, e.size).map_err(|m| foreign(format!("`{}`: {m}", e.name))),
        m => Err(foreign(format!("`{}` uses compression method {m}", e.name))),
    }
}

fn is_zip(b: &[u8]) -> bool {
    b.starts_with(b"PK\x03\x04") || b.starts_with(b"PK\x05\x06")
}

/// The package's JSON description: `definition.json` at the top level, or inside a nested
/// package (some packages wrap their description in an inner archive).
fn definition(b: &[u8], nesting: usize) -> Result<Option<Value>, TemplateError> {
    let list = entries(b)?;
    let json = |e: &Entry| -> Option<Value> {
        let bytes = read_entry(b, e, MAX_JSON).ok()?;
        let text = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).ok()?;
        serde_json::from_str(text).ok()
    };
    let base = |e: &Entry| e.name.rsplit('/').next().unwrap_or_default().to_ascii_lowercase();
    if let Some(v) = list.iter().filter(|e| base(e) == "definition.json").find_map(json) {
        return Ok(Some(v));
    }
    if nesting < 2 {
        for e in list.iter().filter(|e| e.size <= MAX_PACKAGE / 4 && !e.name.ends_with('/')) {
            let Ok(inner) = read_entry(b, e, MAX_PACKAGE / 4) else { continue };
            if is_zip(&inner)
                && let Ok(Some(v)) = definition(&inner, nesting + 1)
            {
                return Ok(Some(v));
            }
        }
    }
    // any other top-level JSON file that names the template
    Ok(list.iter().filter(|e| base(e).ends_with(".json")).find_map(json))
}

// ------------------------------------------------------------------------------------------------
// The JSON description
// ------------------------------------------------------------------------------------------------

/// A displayable string: a plain string, or a localized string table (an object whose `strDB`
/// array holds `{localeString, str}` pairs; English is preferred), or an object with `str` /
/// `value` / `text`.
fn localized(v: &Value, depth: usize) -> Option<String> {
    if depth > 6 {
        return None;
    }
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => {
            if let Some(Value::Array(db)) = o.get("strDB") {
                let en = db.iter().find(|e| e.get("localeString").and_then(Value::as_str).is_some_and(|l| l.to_ascii_lowercase().starts_with("en")));
                return en.or_else(|| db.first()).and_then(|e| e.get("str")).and_then(|s| localized(s, depth + 1));
            }
            ["str", "text", "textEditValue", "value"].iter().find_map(|k| o.get(*k).and_then(|x| localized(x, depth + 1)))
        }
        _ => None,
    }
}

fn clean(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control() || *c == '\n').take(MAX_TEXT_CHARS).collect();
    s.replace("\r\n", "\n").trim().to_string()
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).filter(|x: &f64| x.is_finite())
}

/// Reads what FilmCraft needs from a package's description, walking it with bounds.
fn describe(v: &Value) -> MogrtInfo {
    let mut info = MogrtInfo::default();
    let mut nodes = 0usize;
    let mut stack: Vec<(&Value, usize)> = vec![(v, 0)];
    while let Some((node, depth)) = stack.pop() {
        nodes += 1;
        if nodes > MAX_NODES {
            break;
        }
        match node {
            Value::Object(o) => {
                for (k, x) in o {
                    let key = k.to_ascii_lowercase();
                    match key.as_str() {
                        "capsulename" | "templatename" if info.name.is_empty() => info.name = localized(x, 0).map(|s| clean(&s)).unwrap_or_default(),
                        "durationinseconds" | "duration_seconds" if info.seconds.is_none() => info.seconds = number(x).filter(|s| *s > 0.0),
                        "framesize" | "resolution" if info.canvas.is_none() => {
                            let (w, h) = match x {
                                Value::Array(a) => (a.first().and_then(number), a.get(1).and_then(number)),
                                Value::Object(m) => (m.get("width").or(m.get("w")).and_then(number), m.get("height").or(m.get("h")).and_then(number)),
                                _ => (None, None),
                            };
                            if let (Some(w), Some(h)) = (w, h)
                                && (16.0..=16384.0).contains(&w)
                                && (16.0..=16384.0).contains(&h)
                            {
                                info.canvas = Some([w as u32, h as u32]);
                            }
                        }
                        "clientcontrols" => {
                            for c in x.as_array().into_iter().flatten() {
                                if info.texts.len() >= MAX_TEXTS {
                                    break;
                                }
                                let label = c.get("uiName").and_then(|n| localized(n, 0)).map(|s| clean(&s)).unwrap_or_default();
                                let value = c.get("value").and_then(|n| localized(n, 0)).map(|s| clean(&s)).unwrap_or_default();
                                // text fields only: numbers, colours and toggles carry non-text values
                                if !value.is_empty() && value.parse::<f64>().is_err() && !value.starts_with('#') {
                                    info.texts.push((if label.is_empty() { format!("Text {}", info.texts.len() + 1) } else { label }, value));
                                }
                            }
                        }
                        _ => {}
                    }
                    if depth < MAX_DEPTH && key != "clientcontrols" {
                        stack.push((x, depth + 1));
                    }
                }
            }
            Value::Array(a) if depth < MAX_DEPTH => stack.extend(a.iter().rev().map(|x| (x, depth + 1))),
            _ => {}
        }
    }
    info
}

/// Reads a `.mogrt` package.
pub fn read_mogrt(bytes: &[u8]) -> Result<MogrtInfo, TemplateError> {
    if bytes.len() > MAX_PACKAGE {
        return Err(foreign("the package is too large"));
    }
    if !is_zip(bytes) {
        return Err(foreign("not a ZIP archive"));
    }
    Ok(definition(bytes, 0)?.map(|v| describe(&v)).unwrap_or_default())
}

/// A FilmCraft graphics template made from a `.mogrt` package: an original centred layout with
/// one text layer per text field of the package (the first is the title, the rest stack beneath
/// it), each one an editable property. `fallback_name` names it when the package does not.
pub fn template_from_mogrt(bytes: &[u8], fallback_name: &str) -> Result<GraphicsTemplate, TemplateError> {
    let mut info = read_mogrt(bytes)?;
    if info.name.trim().is_empty() {
        info.name = clean(fallback_name);
    }
    if info.name.trim().is_empty() {
        info.name = "Imported Graphic".into();
    }
    if info.texts.is_empty() {
        info.texts.push(("Title".into(), info.name.clone()));
    }
    let [w, h] = info.canvas.unwrap_or([1920, 1080]);
    let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
    let unit = f64::from(h) / 1080.0;
    let mut layers = Vec::with_capacity(info.texts.len() + 1);
    let mut controls: Vec<TemplateControl> = Vec::new();
    let n = info.texts.len();
    // the title, then smaller lines 70 px apart, the block centred vertically
    let block = 120.0 * unit + (n.saturating_sub(1)) as f64 * 70.0 * unit;
    let mut y = cy - block / 2.0 + 60.0 * unit;
    let mut used = std::collections::HashSet::new();
    for (i, (label, value)) in info.texts.iter().enumerate() {
        let uid = i as u64 + 1;
        let (size, style) = if i == 0 { (96.0 * unit, "Bold") } else { (44.0 * unit, "Regular") };
        let mut layer = text(uid, label, value, (cx, y), size, style);
        set(&mut layer, "align", ParamValue::Choice(1));
        set(&mut layer, "fill_color", rgb(if i == 0 { 0xffffff } else { 0xe5e7eb }));
        set(&mut layer, "shadow", ParamValue::Bool(true));
        set(&mut layer, "shadow_blur", ParamValue::Float(14.0));
        set(&mut layer, "shadow_opacity", ParamValue::Float(55.0));
        layers.push(layer);
        let mut id = slug(label).replace('-', "_");
        while !used.insert(id.clone()) {
            id.push('_');
        }
        controls.push(ctl(&id, label, uid, "text"));
        y += if i == 0 { 120.0 * unit } else { 70.0 * unit };
    }
    controls.push(slider("title_size", "Title Size", 1, "size", 24.0, 320.0));
    controls.push(ctl("title_color", "Title Color", 1, "fill_color"));
    controls.push(ctl("font", "Font", 1, "font"));
    let seconds = info.seconds.unwrap_or(5.0).clamp(0.5, 600.0);
    let t = GraphicsTemplate {
        id: format!("user:{}", slug(&info.name)),
        name: info.name.clone(),
        category: "Imported (.mogrt)".into(),
        description: "Imported from a .mogrt package: its text fields, in a FilmCraft layout.".into(),
        canvas: [w, h],
        duration: Tick((seconds * TICKS_PER_SECOND as f64).round() as i64),
        layers,
        controls,
        tags: vec!["mogrt".into()],
        ..Default::default()
    };
    t.validate()?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored (uncompressed) ZIP with the given entries.
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data) in files {
            let off = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&0u32.to_le_bytes()); // crc (unchecked)
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    const DEF: &str = r#"{
        "capsuleName": {"strDB": [{"localeString": "en_US", "str": "Neon Lower Third"}]},
        "sourceInfoLocalized": {"en_US": {"durationInSeconds": 7, "frameSize": [3840, 2160]}},
        "clientControls": [
            {"uiName": {"strDB": [{"localeString": "en_US", "str": "Name"}]}, "value": {"strDB": [{"localeString": "en_US", "str": "Ada Lovelace"}]}},
            {"uiName": "Opacity", "value": 80},
            {"uiName": "Role", "value": {"strDB": [{"localeString": "fr_FR", "str": "Mathématicienne"}]}}
        ]
    }"#;

    #[test]
    fn reads_name_texts_size_and_duration() {
        let pkg = zip(&[("thumb.png", b"\x89PNG"), ("definition.json", DEF.as_bytes())]);
        let t = template_from_mogrt(&pkg, "fallback").unwrap();
        assert_eq!(t.name, "Neon Lower Third");
        assert_eq!(t.canvas, [3840, 2160]);
        assert_eq!(t.duration, Tick(7 * TICKS_PER_SECOND));
        assert_eq!(t.layers.len(), 2, "the numeric control is not a text");
        let names: Vec<_> = t.controls.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(&names[..2], ["Name", "Role"]);
        assert!(t.validate().is_ok());
    }

    #[test]
    fn nested_package_definition_is_found() {
        let inner = zip(&[("definition.json", DEF.as_bytes())]);
        let pkg = zip(&[("project.aegraphic", &inner)]);
        assert_eq!(read_mogrt(&pkg).unwrap().name, "Neon Lower Third");
    }

    #[test]
    fn package_without_description_becomes_a_named_title() {
        let pkg = zip(&[("thumb.png", b"x")]);
        let t = template_from_mogrt(&pkg, "My Title").unwrap();
        assert_eq!(t.name, "My Title");
        assert_eq!(t.layers.len(), 1);
    }

    #[test]
    fn hostile_packages_are_errors_not_panics() {
        let good = zip(&[("definition.json", DEF.as_bytes())]);
        assert!(read_mogrt(b"").is_err());
        assert!(read_mogrt(b"PK\x03\x04").is_err());
        for cut in 0..good.len() {
            let _ = std::panic::catch_unwind(|| template_from_mogrt(&good[..cut], "x")).expect("truncation must not panic");
        }
        for i in 0..good.len() {
            for bit in [0x01u8, 0x80, 0xff] {
                let mut b = good.clone();
                b[i] ^= bit;
                let _ = std::panic::catch_unwind(|| template_from_mogrt(&b, "x")).expect("bit flip must not panic");
            }
        }
    }

    #[test]
    fn deep_json_is_bounded() {
        let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
        let pkg = zip(&[("definition.json", deep.as_bytes())]);
        // serde_json refuses past its own recursion limit; either way no panic and a template
        assert!(template_from_mogrt(&pkg, "Deep").is_ok());
    }
}
