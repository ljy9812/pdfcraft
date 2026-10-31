//! Type 3 fallback fonts for text the standard fonts can't draw (Helvetica and the rest of the
//! standard 14, in WinAnsiEncoding, cover Western European only).
//!
//! A Type 3 font's glyphs are content-stream paths, so the craft-fonts Japanese and Chinese (Hans)
//! outlines — which no embeddable program is bundled for — can be drawn without embedding a font
//! program at all. The bundled Latin (Inter) face is drawn the same way behind them, covering the
//! letters the CJK faces lack (#2593). One font takes at most [`MAX_TYPE3_GLYPHS`] unique
//! characters; a longer text is split across several (`PCJp`, `PCJp1`, …), each with a ToUnicode
//! map so the text stays copyable and searchable.
//!
//! The font *construction* lives here because both crates that draw generated text need it: the
//! edit crate (replacement text, the Add-text tool) and the annot crate (a FreeText comment's
//! appearance). The caller passes the document to add the font objects to and the resource
//! dictionary the fonts land in; everything else is shared.

use std::collections::HashMap;

use pdfcraft_cos::{Dict, Document, Object, PdfString, Stream};

use crate::{
    CraftFont, GlyphError, document_hebrew_font, document_japanese_font, document_japanese_fonts_for_style, document_latin_fonts,
    japanese_glyph_from, ui_chinese_fonts, win_ansi_encodable,
};

/// The error [`type3_font`] fails with: a message for the user, the wording each caller surfaces
/// as its own "invalid" error (byte for byte, so tests and users see one wording).
#[derive(Debug)]
pub enum Type3Error {
    /// The message.
    Invalid(String),
}

impl std::fmt::Display for Type3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type3Error::Invalid(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Type3Error {}

/// The `/Name` of the Type 3 fonts generated for Japanese-style text.
pub const JAPANESE_FONT_NAME: &str = "PCJapanese";
/// The `/Name` of the Type 3 fonts generated for Hebrew text.
pub const HEBREW_FONT_NAME: &str = "PCHebrew";
/// The `/Name` of the Type 3 fonts generated from the bundled Latin (Inter) face.
pub const LATIN_FONT_NAME: &str = "PCLatin";

/// Unique characters one generated font shows at most (codes 1…240 in a single byte).
pub const MAX_TYPE3_GLYPHS: usize = 240;

/// Whether `font` is one of those generated fonts (Japanese, Hebrew or Latin): a replacement longer
/// than one of them is split across several, which read back as one line.
pub fn is_type3_fallback(font: &Dict) -> bool {
    font.name(b"Subtype") == Some(b"Type3")
        && font
            .name(b"Name")
            .is_some_and(|n| n == JAPANESE_FONT_NAME.as_bytes() || n == HEBREW_FONT_NAME.as_bytes() || n == LATIN_FONT_NAME.as_bytes())
}

/// The generated font's `/Name` and its resource prefix: the bundled Latin face, Hebrew, or the
/// Japanese ones.
pub fn fallback_names(face: &CraftFont, hebrew: bool) -> (&'static str, &'static str) {
    if hebrew {
        (HEBREW_FONT_NAME, "PCHe")
    } else if face.family == "Inter" {
        (LATIN_FONT_NAME, "PCLat")
    } else {
        (JAPANESE_FONT_NAME, "PCJp")
    }
}

/// The generated fonts for one text: one per run of at most `max_glyphs` unique characters, in
/// order of first appearance. Also drawn with by the Add-text tool's CJK path and a FreeText
/// comment's appearance.
#[derive(Clone)]
pub struct Type3Fallback {
    /// The family its glyphs come from ("Shippori Mincho").
    pub family: &'static str,
    /// The fonts' resource names in order; the first is `PCJp` (`PCLat` for the bundled Latin
    /// face, `PCHe` for Hebrew text).
    pub names: Vec<String>,
    /// Every unique character with its advance (em), in order of first appearance.
    pub widths: Vec<(char, f64)>,
    /// Where each character is shown: the index of its font in `names`, and its code there.
    pub codes: HashMap<char, (usize, u8)>,
}

/// One glyph of a generated font: its code there, its character, its glyph procedure and advance.
/// Public (fields too) because the edit crate prepares a mixed-style rewrite's glyphs itself and
/// hands them to [`write_type3_glyphs`].
pub struct Type3Glyph {
    pub code: u8,
    pub ch: char,
    pub path: Vec<u8>,
    pub width: f64,
}

fn pdf_num(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v:.4}").trim_end_matches('0').trim_end_matches('.').to_string() }
}

fn type3_path(face: &CraftFont, ch: char) -> Result<(Vec<u8>, f64), Type3Error> {
    let glyph = japanese_glyph_from(face, ch).map_err(|e| match e {
        GlyphError::NoFont => no_japanese_font(),
        GlyphError::Missing => Type3Error::Invalid(format!("the {} fallback font has no glyph for U+{:04X}", face.family, ch as u32)),
        GlyphError::TooComplex => Type3Error::Invalid(format!("the {} fallback glyph U+{:04X} is too complex", face.family, ch as u32)),
    })?;
    let (dx, width) = proportional(ch, &glyph);
    let scale = 1000.0;
    // d1 is `wx wy llx lly urx ury` (ISO 32000-2 §9.6.4) with a box enclosing the glyph;
    // Acrobat draws a bullet in place of a glyph whose d1 is malformed.
    let b = if glyph.contours.is_empty() { [0.0; 4] } else { glyph.bbox };
    let mut out = format!(
        "{} 0 {} {} {} {} d1\n",
        pdf_num(width * scale),
        pdf_num(((b[0] + dx) * scale).floor()),
        pdf_num((b[1] * scale).floor()),
        pdf_num(((b[2] + dx) * scale).ceil()),
        pdf_num((b[3] * scale).ceil())
    )
    .into_bytes();
    for contour in glyph.contours {
        let Some(first) = contour.first() else { continue };
        out.extend_from_slice(format!("{} {} m\n", pdf_num((first[0] + dx) * scale), pdf_num(first[1] * scale)).as_bytes());
        for p in contour.iter().skip(1) {
            out.extend_from_slice(format!("{} {} l\n", pdf_num((p[0] + dx) * scale), pdf_num(p[1] * scale)).as_bytes());
        }
        out.extend_from_slice(b"h\n");
    }
    out.extend_from_slice(b"f\n");
    Ok((out, width))
}

/// Side bearing (em) given to a respaced glyph on each side.
const SIDE_BEARING: f64 = 0.05;

/// Shift and advance for a fallback glyph. Japanese faces draw Cyrillic and Greek full-width
/// (one em each, as in JIS X 0208) and have no proportional (`palt`) metrics for them, so a
/// word set that way reads as letter-spaced. Those letters get their ink width plus a side
/// bearing instead; everything else keeps the face's own advance.
fn proportional(ch: char, glyph: &crate::GlyphOutline) -> (f64, f64) {
    let respace = matches!(ch, '\u{0370}'..='\u{03FF}' | '\u{0400}'..='\u{052F}' | '\u{1F00}'..='\u{1FFF}');
    let ink = glyph.bbox[2] - glyph.bbox[0];
    if !respace || glyph.contours.is_empty() || !(ink > 0.0 && ink < glyph.width) {
        return (0.0, glyph.width);
    }
    (SIDE_BEARING - glyph.bbox[0], ink + 2.0 * SIDE_BEARING)
}

/// `ch` as UTF-16BE hex, for an ActualText span's or ToUnicode entry's string.
pub fn unicode_hex(ch: char) -> String {
    let mut units = [0u16; 2];
    let encoded = ch.encode_utf16(&mut units);
    encoded.iter().map(|u| format!("{u:04X}")).collect()
}

/// The same outline for a face that may simply lack the glyph: `None` then, so the caller can try
/// the next face (and report the character once none of them has it).
pub fn type3_glyph(face: &CraftFont, ch: char) -> Result<Option<(Vec<u8>, f64)>, Type3Error> {
    match japanese_glyph_from(face, ch) {
        Err(GlyphError::Missing) => Ok(None),
        _ => type3_path(face, ch).map(Some),
    }
}

/// The character the craft-fonts Japanese faces are for: kana, CJK ideographs and their
/// compatibility and full-width forms.
fn needs_japanese(ch: char) -> bool {
    matches!(u32::from(ch), 0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// No face this build has can draw `text`. Only Japanese text is about the Japanese face: other
/// characters are missing from the paragraph's font, and choosing a font that has them works too.
pub fn no_fallback_font(text: &str) -> Type3Error {
    if text.chars().any(needs_japanese) {
        return no_japanese_font();
    }
    let Some(ch) = text.chars().find(|c| !c.is_whitespace() && !win_ansi_encodable(*c)) else { return no_japanese_font() };
    Type3Error::Invalid(format!(
        "the paragraph's font has no glyph for {ch} (U+{:04X}) and this build has no fallback font for it; \
         choose a font that has it (Load font… or Installed fonts…), or use an official release, which includes \
         PdfCraft's fallback fonts (to build them in, set CRAFT_FONTS_DIR to a craft-fonts checkout)",
        u32::from(ch)
    ))
}

/// The chosen face has no outline for `ch`, and neither has any other: say which character it is and
/// what the writer can do about it. A CJK character without a Japanese face in the build is about
/// that build input, which is the one answer that changes it.
pub fn no_glyph_for(face: &CraftFont, ch: char) -> Type3Error {
    if needs_japanese(ch) && document_japanese_font().is_none() {
        return no_japanese_font();
    }
    Type3Error::Invalid(format!(
        "the {} fallback font has no glyph for {ch} (U+{:04X}), and no fallback face covers the text; \
         choose a font that has it (Load font… or Installed fonts…), or use an official release, which includes \
         PdfCraft's fallback fonts (to build them in, set CRAFT_FONTS_DIR to a craft-fonts checkout)",
        face.family,
        u32::from(ch)
    ))
}

/// This build has no Japanese face to draw fallback text with.
pub fn no_japanese_font() -> Type3Error {
    Type3Error::Invalid(
        "this text needs PdfCraft's Japanese fallback font, which this build doesn't include \
         (official releases do; to build it in, set CRAFT_FONTS_DIR to a craft-fonts checkout)"
            .into(),
    )
}

/// This build has no Hebrew face to draw fallback text with.
fn no_hebrew_font() -> Type3Error {
    Type3Error::Invalid(
        "this text needs PdfCraft's Hebrew fallback font (Noto Sans Hebrew), which this build doesn't include \
         (to build it in, set CRAFT_FONTS_DIR to a craft-fonts checkout that has it)"
            .into(),
    )
}

/// Hebrew letters, points, cantillation marks and punctuation (and the presentation forms): text
/// with any of them takes the craft-fonts Hebrew face first. A broader range than the one that
/// dispatches to a shaped path, so marks that only the shaped path can place are included.
fn is_hebrew(c: char) -> bool {
    matches!(u32::from(c), 0x0590..=0x05FF | 0xFB1D..=0xFB4F)
}

/// Hebrew points (niqqud) and cantillation marks, which a one-glyph-per-character font can't
/// place on their letter: text with any is refused.
fn is_hebrew_mark(c: char) -> bool {
    matches!(u32::from(c), 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7 | 0xFB1E)
}

/// CJK ideographs, kana, the extensions and full-width forms: the text the Japanese and Chinese
/// (Hans) faces stand in for, drawn as a Type 3 font.
pub fn is_cjk(c: char) -> bool {
    matches!(u32::from(c), 0x3000..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// The first of `faces` (best match first) that has a glyph for every one of `chars`, else the first.
pub fn fallback_face<'a>(faces: &[&'a CraftFont], chars: &[char], has_glyph: impl Fn(&CraftFont, char) -> bool) -> Option<&'a CraftFont> {
    faces.iter().copied().find(|face| chars.iter().all(|ch| has_glyph(face, *ch))).or_else(|| faces.first().copied())
}

/// The advance (em) [`type3_path`] gives `ch`, so text wrapped against a face is laid out the way
/// the same face's Type 3 font draws it. `None` when the face has no glyph for `ch`.
pub fn glyph_advance(face: &CraftFont, ch: char) -> Option<f64> {
    let glyph = japanese_glyph_from(face, ch).ok()?;
    Some(proportional(ch, &glyph).1)
}

/// The advance (em) each character of `text` is drawn with when the Type 3 fallback draws it: the
/// face [`type3_font`] would choose and that face's glyph advance, for every character (its Latin
/// too, which the fallback draws as well, so a mixed line measures the way it draws). Hebrew text
/// takes the Hebrew face first and each character it lacks from the next face that has it, as in
/// `type3_font`. `None` when the fallback wouldn't draw the text at all — no craft-fonts, or a
/// character missing from every face — so a caller measuring for a `?` keeps the standard fonts'
/// measurement.
pub fn fallback_advances(text: &str, serif: bool, bold: bool) -> Option<HashMap<char, f64>> {
    let (faces, hebrew) = type3_faces(text, serif, bold).ok()?;
    let mut chars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ch in text.chars() {
        if seen.insert(ch) {
            chars.push(ch);
        }
    }
    if chars.is_empty() {
        return None;
    }
    let others: &[&CraftFont] = if hebrew { faces.get(1..).unwrap_or_default() } else { &[] };
    let face = if hebrew { faces.first().copied() } else { fallback_face(&faces, &chars, |face, ch| japanese_glyph_from(face, ch).is_ok()) };
    let face = face?;
    let mut advances = HashMap::with_capacity(chars.len());
    for ch in chars {
        // Hebrew text: the first face with the glyph, else the Hebrew face's miss (`None`).
        let advance = others.iter().fold(glyph_advance(face, ch), |found, f| found.or_else(|| glyph_advance(f, ch)));
        advances.insert(ch, advance?);
    }
    Some(advances)
}

/// The faces a Type 3 fallback may draw `text` with, best first. Text with Hebrew takes the
/// Hebrew face first (its Latin and digits too, so a line keeps one look), then the other faces
/// for anything it lacks; other text takes the Japanese faces, then the Chinese (Hans) ones,
/// which stand in for the Simplified-only code points the Japanese faces lack (欢, 导, 们, …),
/// and the bundled Latin face behind them, which covers the Latin, Greek and Cyrillic letters
/// they lack (#2593: the Japanese faces have no қ, ғ, ҳ, ә, ө, ү, ұ or һ). `serif` picks the
/// Japanese document face for the style (Mincho for serif, Gothic otherwise).
pub fn type3_faces(text: &str, serif: bool, bold: bool) -> Result<(Vec<&'static CraftFont>, bool), Type3Error> {
    let faces: Vec<&CraftFont> =
        document_japanese_fonts_for_style(serif, bold).into_iter().chain(ui_chinese_fonts()).chain(document_latin_fonts(bold)).collect();
    if !text.chars().any(is_hebrew) {
        return Ok((faces, false));
    }
    // Points are drawn on their letter with the font's positioning, which a one-glyph-per-
    // character Type 3 font can't do; refuse rather than print them beside the letter.
    if text.chars().any(is_hebrew_mark) {
        return Err(Type3Error::Invalid("Hebrew points (niqqud) can't be written into a PDF yet; type the text without them".into()));
    }
    let hebrew = document_hebrew_font(bold).ok_or_else(no_hebrew_font)?;
    Ok((std::iter::once(hebrew).chain(faces).collect(), true))
}

/// Build the Type 3 fonts that show `text` (all of it: its Latin and spaces too, so one drawing
/// keeps one look), adding their objects to `doc` and their entries to `fonts_res`, which the
/// caller then uses as (or merges into) the drawing's `/Resources /Font`. `serif` picks the
/// Japanese document face for the style; `bold` the bold variant. Fails without craft-fonts
/// ([`no_japanese_font`]) or when a character is missing from every face.
pub fn type3_font(
    doc: &mut Document,
    fonts_res: &mut Dict,
    text: &str,
    serif: bool,
    bold: bool,
    max_glyphs: usize,
) -> Result<Type3Fallback, Type3Error> {
    let (faces, hebrew) = type3_faces(text, serif, bold)?;
    // Each unique character once, in order of first appearance.
    let mut chars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ch in text.chars() {
        if seen.insert(ch) {
            chars.push(ch);
        }
    }
    if chars.is_empty() {
        return Err(Type3Error::Invalid("replacement text is empty".into()));
    }
    // The faces differ in coverage (e.g. of Cyrillic), so use the best face that has every
    // character. When none has them all, the best face reports the character it lacks. Hebrew
    // text keeps the Hebrew face and takes each character it lacks from the next face that has it.
    let face = if hebrew { faces.first().copied() } else { fallback_face(&faces, &chars, |face, ch| japanese_glyph_from(face, ch).is_ok()) }
        .ok_or_else(|| no_fallback_font(text))?;
    let others: &[&CraftFont] = if hebrew { faces.get(1..).unwrap_or_default() } else { &[] };
    // Every outline is read before the document is touched, so a character that fails leaves it
    // as it was. Each font takes the next `per_font` characters; codes restart at 1 in each.
    let per_font = max_glyphs.clamp(1, MAX_TYPE3_GLYPHS);
    let mut glyphs = Vec::with_capacity(chars.len());
    for (i, ch) in chars.into_iter().enumerate() {
        // The chosen face first, then the faces after it (Hebrew text takes a character the Hebrew
        // face lacks from the next face that has it); a character no face draws is reported once.
        let mut drawn = type3_glyph(face, ch)?;
        for f in others.iter().copied() {
            if drawn.is_some() {
                break;
            }
            drawn = type3_glyph(f, ch)?;
        }
        let Some((path, width)) = drawn else { return Err(no_glyph_for(face, ch)) };
        let code = u8::try_from((i % per_font).saturating_add(1))
            .map_err(|_| Type3Error::Invalid("replacement text has too many unique characters".into()))?;
        glyphs.push(Type3Glyph { code, ch, path, width });
    }
    Ok(write_type3_glyphs(doc, fonts_res, face, hebrew, glyphs, per_font))
}

/// Write already validated outlines; callers can prepare every face before adding any objects.
/// Public because the edit crate's mixed-style rewrite prepares its fallback glyphs up front (one
/// group per face) and writes them in one pass once every face has been read.
pub fn write_type3_glyphs(
    doc: &mut Document,
    fonts_res: &mut Dict,
    face: &CraftFont,
    hebrew: bool,
    glyphs: Vec<Type3Glyph>,
    per_font: usize,
) -> Type3Fallback {
    let per_font = per_font.clamp(1, MAX_TYPE3_GLYPHS);
    let mut codes = HashMap::with_capacity(glyphs.len());
    let mut widths = Vec::with_capacity(glyphs.len());
    let mut names = Vec::new();
    let mut glyphs = glyphs.into_iter();
    loop {
        let group: Vec<Type3Glyph> = glyphs.by_ref().take(per_font).collect();
        if group.is_empty() {
            break;
        }
        for glyph in &group {
            codes.insert(glyph.ch, (names.len(), glyph.code));
            widths.push((glyph.ch, glyph.width));
        }
        names.push(add_type3_font(doc, fonts_res, face, hebrew, group));
    }
    Type3Fallback { family: face.family, names, widths, codes }
}

/// Adds one generated font whose codes 1 to `glyphs.len()` show `glyphs`, and returns its resource
/// name (`PCLat`, `PCJp`, then `PCJp1`, ...; `PCHe`, ... for Hebrew text). Nothing here can fail:
/// the outlines were read already.
fn add_type3_font(doc: &mut Document, fonts_res: &mut Dict, face: &CraftFont, hebrew: bool, glyphs: Vec<Type3Glyph>) -> String {
    let (font_name, prefix) = fallback_names(face, hebrew);
    let last_char = glyphs.len();
    let mut charprocs = Dict::new();
    let mut widths = Vec::with_capacity(last_char);
    let mut differences = vec![Object::Int(1)];
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CMapType 2 def\n1 begincodespacerange\n<01> <FF>\nendcodespacerange\n",
    );
    cmap.push_str(&format!("{last_char} beginbfchar\n"));
    for glyph in glyphs {
        let glyph_name = format!("g{:02X}", glyph.code);
        let mut pd = Dict::new();
        pd.set(b"Length".to_vec(), glyph.path.len() as i64);
        let proc_ref = doc.add(Object::Stream(Stream::from_raw(pd, glyph.path)));
        charprocs.set(glyph_name.as_bytes().to_vec(), Object::Ref(proc_ref));
        differences.push(Object::name(&glyph_name));
        widths.push(Object::Real((glyph.width * 1000.0).round()));
        cmap.push_str(&format!("<{:02X}> <{}>\n", glyph.code, unicode_hex(glyph.ch)));
    }
    cmap.push_str("endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let mut cmap_dict = Dict::new();
    cmap_dict.set(b"Length".to_vec(), cmap.len() as i64);
    let cmap_ref = doc.add(Object::Stream(Stream::from_raw(cmap_dict, cmap.into_bytes())));
    let mut encoding = Dict::new();
    encoding.set(b"Type".to_vec(), Object::name("Encoding"));
    encoding.set(b"Differences".to_vec(), Object::Array(differences));
    let mut font = Dict::new();
    font.set(b"Type".to_vec(), Object::name("Font"));
    font.set(b"Subtype".to_vec(), Object::name("Type3"));
    font.set(b"Name".to_vec(), Object::name(font_name));
    let mut descriptor = Dict::new();
    descriptor.set(b"Type".to_vec(), Object::name("FontDescriptor"));
    descriptor.set(b"FontName".to_vec(), Object::name(&format!("{}-{}", face.family, face.style).replace(' ', "")));
    descriptor.set(b"FontFamily".to_vec(), Object::String(PdfString::literal(face.family.as_bytes().to_vec())));
    descriptor.set(b"Flags".to_vec(), Object::Int(if face.family.contains("Mincho") { 6 } else { 4 }));
    descriptor.set(b"ItalicAngle".to_vec(), Object::Int(0));
    // PDF 1.7 tables 5.9 and 5.19: a Type 3 descriptor is indirect; Ascent/Descent may be omitted.
    font.set(b"FontDescriptor".to_vec(), Object::Ref(doc.add(Object::Dict(descriptor))));
    font.set(b"FontBBox".to_vec(), Object::Array(vec![Object::Int(0), Object::Int(-300), Object::Int(1000), Object::Int(1000)]));
    font.set(
        b"FontMatrix".to_vec(),
        Object::Array(vec![Object::Real(0.001), Object::Int(0), Object::Int(0), Object::Real(0.001), Object::Int(0), Object::Int(0)]),
    );
    font.set(b"FirstChar".to_vec(), Object::Int(1));
    font.set(b"LastChar".to_vec(), Object::Int(last_char as i64));
    font.set(b"Widths".to_vec(), Object::Array(widths));
    font.set(b"Encoding".to_vec(), Object::Dict(encoding));
    font.set(b"CharProcs".to_vec(), Object::Dict(charprocs));
    font.set(b"ToUnicode".to_vec(), Object::Ref(cmap_ref));
    let mut name = String::from(prefix);
    let mut suffix = 0usize;
    while fonts_res.contains(name.as_bytes()) {
        suffix = suffix.saturating_add(1);
        name = format!("{prefix}{suffix}");
    }
    fonts_res.set(name.as_bytes().to_vec(), Object::Dict(font));
    name
}

/// CJK wrapping: every character is a break opportunity (the way CJK text is set), while a run of
/// other characters (a Latin word) breaks as one, each measured with `advances` (a character's
/// advance at its size, the one the Type 3 font draws it with). Break-opportunity wrapping, not
/// kinsoku (line-start 禁則) processing.
pub fn cjk_wrap(para: &str, width: f64, advances: &HashMap<char, f64>) -> Vec<String> {
    let w = |s: &str| s.chars().map(|c| advances.get(&c).copied().unwrap_or(0.0)).sum::<f64>();
    // Atoms, the pieces that wrap as one: a CJK character, a space, or a run of others (a word).
    let mut atoms: Vec<(String, f64)> = Vec::new();
    let mut word = String::new();
    for c in para.chars() {
        if is_cjk(c) || c == ' ' {
            if !word.is_empty() {
                let atom = std::mem::take(&mut word);
                let aw = w(&atom);
                atoms.push((atom, aw));
            }
            let atom = c.to_string();
            let aw = w(&atom);
            atoms.push((atom, aw));
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        let aw = w(&word);
        atoms.push((word, aw));
    }
    let mut lines = Vec::new();
    let (mut line, mut used) = (String::new(), 0.0);
    for (atom, aw) in atoms {
        if !line.is_empty() && used + aw > width {
            // The line break takes the place of the space it breaks at.
            if atom == " " {
                continue;
            }
            lines.push(std::mem::take(&mut line));
            used = 0.0;
        }
        line.push_str(&atom);
        used += aw;
    }
    lines.push(line);
    lines
}

/// The first character of `text` the Type 3 fallback wouldn't draw, which a generated appearance
/// would show as `?` — mirroring [`type3_font`]'s decision exactly (same faces, same refusal of
/// Hebrew points), without building anything. `None` when all of it draws, including text that's
/// plain WinAnsi (the standard fonts draw that themselves).
pub fn first_undrawable_fallback(text: &str, serif: bool, bold: bool) -> Option<char> {
    let nonstandard = |c: char| !win_ansi_encodable(c) && !matches!(c, '\n' | '\r');
    if !text.chars().any(nonstandard) {
        return None;
    }
    // Hebrew points are refused whichever faces exist (a Type 3 font can't place them).
    if text.chars().any(is_hebrew) && text.chars().any(is_hebrew_mark) {
        return text.chars().find(|c| is_hebrew_mark(*c));
    }
    // An error is only the missing Hebrew face: no face in the (empty) list has the Hebrew
    // letters, so they report undrawable the way the build would.
    let faces = type3_faces(text, serif, bold).map(|(faces, _)| faces).unwrap_or_default();
    text.chars().find(|c| nonstandard(*c) && faces.iter().all(|face| japanese_glyph_from(face, *c).is_err()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_ansi_text_needs_no_fallback() {
        assert_eq!(first_undrawable_fallback("hello — déjà vu", false, false), None);
    }

    #[test]
    fn cjk_wrap_breaks_anywhere_except_words() {
        let advances: HashMap<char, f64> =
            [('你', 1.0), ('好', 1.0), ('世', 1.0), ('界', 1.0), ('a', 0.5), ('b', 0.5), (' ', 0.0)].into_iter().collect();
        assert_eq!(cjk_wrap("你好世界", 2.0, &advances), vec!["你好", "世界"]);
        // The space weighs nothing, so "ab 你" still measures 2.0; 好 breaks the line.
        assert_eq!(cjk_wrap("ab 你好", 2.0, &advances), vec!["ab 你", "好"]);
    }

    #[test]
    fn unicode_hex_is_utf16be() {
        assert_eq!(unicode_hex('A'), "0041");
        assert_eq!(unicode_hex('你'), "4F60");
    }

    #[test]
    fn fallback_face_moves_on_when_the_best_face_lacks_a_character() {
        static JPAN: &[&str] = &["Jpan"];
        let mincho = CraftFont { family: "Mincho", style: "Regular", scripts: JPAN, bytes: b"" };
        let gothic = CraftFont { family: "Gothic", style: "Regular", scripts: JPAN, bytes: b"" };
        let faces = [&mincho, &gothic];
        // Only the Gothic face has the Cyrillic letter.
        let has = |face: &CraftFont, ch: char| ch.is_ascii() || face.family == "Gothic";
        assert_eq!(fallback_face(&faces, &['a', 'ф'], has).map(|f| f.family), Some("Gothic"));
        // Characters the best face has keep it.
        assert_eq!(fallback_face(&faces, &['a', 'b'], has).map(|f| f.family), Some("Mincho"));
        // No face has them all: the best face stays, and names the missing glyph.
        assert_eq!(fallback_face(&faces, &['a', 'ф'], |_, ch| ch.is_ascii()).map(|f| f.family), Some("Mincho"));
        assert!(fallback_face(&[], &['a'], has).is_none());
    }

    #[test]
    fn hebrew_japanese_and_latin_fallback_shards_are_recognized_as_ours() {
        static HEBR: &[&str] = &["Hebr"];
        static LATN: &[&str] = &["Latn"];
        let face = CraftFont { family: "FakeHebrew", style: "Regular", scripts: HEBR, bytes: b"" };
        let latin = CraftFont { family: "Inter", style: "Regular", scripts: LATN, bytes: b"" };
        let glyph = |code, ch| Type3Glyph { code, ch, path: b"0 0 0 0 0 0 d1\nf\n".to_vec(), width: 0.5 };
        let mut doc = Document::new_empty();
        let mut fonts = Dict::new();
        // A replacement split across two Hebrew fonts, then one Japanese font, then one Latin one.
        let he = add_type3_font(&mut doc, &mut fonts, &face, true, vec![glyph(1, 'א')]);
        let he1 = add_type3_font(&mut doc, &mut fonts, &face, true, vec![glyph(1, 'ב')]);
        let jp = add_type3_font(&mut doc, &mut fonts, &face, false, vec![glyph(1, 'あ')]);
        let lat = add_type3_font(&mut doc, &mut fonts, &latin, false, vec![glyph(1, 'ғ')]);
        assert_eq!([he.as_str(), he1.as_str(), jp.as_str(), lat.as_str()], ["PCHe", "PCHe1", "PCJp", "PCLat"]);
        for name in [&he, &he1, &jp, &lat] {
            let font = fonts.get(name.as_bytes()).and_then(Object::as_dict).cloned().unwrap_or_default();
            assert!(is_type3_fallback(&font), "{name} isn't recognized as a generated fallback font");
        }
        // Someone else's Type 3 font is not ours.
        let mut other = Dict::new();
        other.set(b"Subtype".to_vec(), Object::name("Type3"));
        other.set(b"Name".to_vec(), Object::name("PCHebrewX"));
        assert!(!is_type3_fallback(&other));
    }
}
