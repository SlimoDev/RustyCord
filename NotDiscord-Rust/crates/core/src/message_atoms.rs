//! Typed, source-preserving atoms. No clock, locale, network or user lookup.
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp { pub seconds: i64, pub format: char }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind { Timestamp(Timestamp), Game(String) }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom { pub range: Range<usize>, pub kind: Kind }

pub fn token(token: &str) -> Option<Kind> {
    if let Some(id) = token.strip_prefix("<@$").and_then(|s|s.strip_suffix('>')) {
        return crate::parsing::valid_id(id).then(||Kind::Game(id.into()));
    }
    let body=token.strip_prefix("<t:")?.strip_suffix('>')?;
    let (seconds, format)=body.split_once(':').map_or((body,'f'),|(seconds,style)|
        (seconds, if style.len()==1 {style.chars().next().unwrap()} else {'\0'}));
    if !matches!(format,'t'|'T'|'d'|'D'|'f'|'F'|'s'|'S'|'R') {return None;}
    let digits=seconds.strip_prefix('-').unwrap_or(seconds);
    if digits.is_empty() || digits.len()>17 || !digits.bytes().all(|c|c.is_ascii_digit()) {return None;}
    let seconds=seconds.parse::<i64>().ok()?;
    // The presentation uses the user's Windows calendar (years 1..9999).
    let date=chrono::DateTime::from_timestamp(seconds,0)?;
    use chrono::Datelike;
    (1..=9999).contains(&date.year()).then_some(Kind::Timestamp(Timestamp{seconds,format}))
}

pub fn collect(source: &str, document: &crate::markdown::Document) -> Vec<Atom> {
    let mut atoms=Vec::new();
    let mut candidates=0;
    let mut lexical=crate::mentions::PlainTextCursor::default();
    for block in &document.blocks {
        for span in block.spans.iter().filter(|s|!s.syntax && !s.style.code && s.link.is_none()) {
            if span.range.start>=64*1024 {continue;}
            let mut end=span.range.end.min(64*1024);while !source.is_char_boundary(end){end-=1;}
            let Some(text)=source.get(span.range.start..end) else {continue;};
            for (offset,_) in text.match_indices('<') {
                candidates+=1;if atoms.len()>=256||candidates>512 {return atoms;}
                let start=span.range.start+offset;
                if !lexical.at(source,start) {continue;}
                let Some(end)=text[offset..].find('>').filter(|end|*end<=64) else {continue;};
                if let Some(kind)=token(&text[offset..offset+end+1]) {atoms.push(Atom{range:start..start+end+1,kind});}
            }
        }
    }
    atoms
}

/// Project only the shared typed atoms. The original text remains the model.
pub fn project(source:&str,mut render:impl FnMut(&Kind)->String)->String{
    let doc=crate::markdown::parse(source);let mut text=String::with_capacity(source.len());let mut cursor=0;
    for atom in doc.atoms{if atom.range.start<cursor{continue;}text.push_str(&source[cursor..atom.range.start]);text.push_str(&render(&atom.kind));cursor=atom.range.end;}
    text.push_str(&source[cursor..]);text
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn typed_atoms_preserve_ranges_and_ignore_literal_code_and_escapes(){
        let text=r"α **<t:1793114201:S>** `<@$123>` \<t:123:R> <@$421150694683312138>";
        let doc=crate::markdown::parse(text);
        assert_eq!(doc.atoms.len(),2);
        assert_eq!(&text[doc.atoms[0].range.clone()],"<t:1793114201:S>");
        assert!(matches!(doc.atoms[1].kind,Kind::Game(_)));
        for invalid in ["<t:1:Q>","<t:+1:R>","<t:99999999999999999:S>","<@$abc>"] {assert!(token(invalid).is_none());}
    }
}
