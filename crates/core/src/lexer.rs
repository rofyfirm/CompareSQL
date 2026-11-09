//! Statement splitter (streaming) dan tokenizer SQL dialek MySQL.
//!
//! Splitter membaca per baris, memotong statement pada delimiter di luar
//! string/komentar, dan paham perintah klien `DELIMITER`. Offset byte tiap
//! statement dicatat supaya INSERT bisa dibaca ulang dengan seek.

use std::collections::VecDeque;
use std::io::{self, BufRead};

use memchr::{memchr, memchr2, memmem};

#[derive(Debug, Clone)]
pub struct Statement {
    /// Offset byte pertama statement di file.
    pub offset: u64,
    /// Teks statement tanpa delimiter, spasi akhir dibuang.
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Normal,
    Single,
    Double,
    Backtick,
    LineComment,
    BlockComment,
}

pub struct StatementReader<R> {
    reader: R,
    pos: u64,
    delimiter: Vec<u8>,
    line: Vec<u8>,
    cur: Vec<u8>,
    cur_offset: u64,
    started: bool,
    state: State,
    pending: VecDeque<Statement>,
    eof: bool,
}

impl<R: BufRead> StatementReader<R> {
    pub fn new(reader: R) -> Self {
        StatementReader {
            reader,
            pos: 0,
            delimiter: b";".to_vec(),
            line: Vec::with_capacity(64 * 1024),
            cur: Vec::new(),
            cur_offset: 0,
            started: false,
            state: State::Normal,
            pending: VecDeque::new(),
            eof: false,
        }
    }

    /// Byte yang sudah dibaca dari reader.
    pub fn position(&self) -> u64 {
        self.pos
    }

    pub fn next_statement(&mut self) -> io::Result<Option<Statement>> {
        loop {
            if let Some(s) = self.pending.pop_front() {
                return Ok(Some(s));
            }
            if self.eof {
                return Ok(None);
            }
            self.line.clear();
            let n = self.reader.read_until(b'\n', &mut self.line)?;
            if n == 0 {
                self.eof = true;
                self.flush();
                continue;
            }
            let line_start = self.pos;
            self.pos += n as u64;
            self.process_line(line_start);
        }
    }

    fn flush(&mut self) {
        while matches!(self.cur.last(), Some(b) if b.is_ascii_whitespace()) {
            self.cur.pop();
        }
        if !self.cur.is_empty() {
            let bytes = std::mem::take(&mut self.cur);
            self.pending.push_back(Statement { offset: self.cur_offset, bytes });
        }
        self.started = false;
    }

    fn process_line(&mut self, line_start: u64) {
        let line = std::mem::take(&mut self.line);
        let n = line.len();

        if self.state == State::Normal && !self.started {
            let t = trim_start(&line);
            if t.len() >= 9
                && t[..9].eq_ignore_ascii_case(b"DELIMITER")
                && (t.len() == 9 || t[9].is_ascii_whitespace())
            {
                let d = trim(&t[9..]);
                if !d.is_empty() {
                    self.delimiter = d.to_vec();
                }
                self.line = line;
                return;
            }
        }

        let mut i = 0usize;
        let mut seg = 0usize;
        while i < n {
            match self.state {
                State::Normal => {
                    let b = line[i];
                    if !self.started {
                        if b.is_ascii_whitespace() {
                            i += 1;
                            seg = i;
                            continue;
                        }
                        if is_line_comment(&line, i) {
                            i = n;
                            seg = n;
                            continue;
                        }
                        self.started = true;
                        self.cur_offset = line_start + i as u64;
                        seg = i;
                    }
                    if b == self.delimiter[0] && line[i..].starts_with(&self.delimiter) {
                        self.cur.extend_from_slice(&line[seg..i]);
                        self.flush();
                        i += self.delimiter.len();
                        seg = i;
                        continue;
                    }
                    match b {
                        b'\'' => self.state = State::Single,
                        b'"' => self.state = State::Double,
                        b'`' => self.state = State::Backtick,
                        b'#' => self.state = State::LineComment,
                        b'-' if is_line_comment(&line, i) => self.state = State::LineComment,
                        b'/' if i + 1 < n && line[i + 1] == b'*' => {
                            self.state = State::BlockComment;
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                State::Single | State::Double => {
                    let q = if self.state == State::Single { b'\'' } else { b'"' };
                    match memchr2(q, b'\\', &line[i..]) {
                        None => i = n,
                        Some(off) => {
                            let j = i + off;
                            if line[j] == b'\\' {
                                i = j + 2;
                            } else {
                                self.state = State::Normal;
                                i = j + 1;
                            }
                        }
                    }
                }
                State::Backtick => match memchr(b'`', &line[i..]) {
                    None => i = n,
                    Some(off) => {
                        self.state = State::Normal;
                        i += off + 1;
                    }
                },
                State::LineComment => match memchr(b'\n', &line[i..]) {
                    None => i = n,
                    Some(off) => {
                        self.state = State::Normal;
                        i += off + 1;
                    }
                },
                State::BlockComment => match memmem::find(&line[i..], b"*/") {
                    None => i = n,
                    Some(off) => {
                        self.state = State::Normal;
                        i += off + 2;
                    }
                },
            }
        }
        if self.started && seg < n {
            self.cur.extend_from_slice(&line[seg..n]);
        }
        self.line = line;
    }
}

fn is_line_comment(s: &[u8], i: usize) -> bool {
    if s[i] == b'#' {
        return true;
    }
    s[i] == b'-'
        && i + 1 < s.len()
        && s[i + 1] == b'-'
        && (i + 2 == s.len() || s[i + 2] <= b' ')
}

pub fn trim_start(s: &[u8]) -> &[u8] {
    let k = s.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(s.len());
    &s[k..]
}

pub fn trim(s: &[u8]) -> &[u8] {
    let s = trim_start(s);
    let e = s.iter().rposition(|b| !b.is_ascii_whitespace()).map(|p| p + 1).unwrap_or(0);
    &s[..e]
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// Kata tanpa quote: keyword atau identifier.
    Word(String),
    /// Identifier dalam backtick.
    Ident(String),
    /// String literal (sudah di-unescape).
    Str(Vec<u8>),
    Num(String),
    /// 0x.. atau X'..' (sudah di-decode).
    Hex(Vec<u8>),
    /// b'..' atau 0b.. (digit biner).
    Bits(String),
    Punct(String),
    /// @var atau @@var
    Var(String),
}

impl Tok {
    pub fn is_word(&self, w: &str) -> bool {
        matches!(self, Tok::Word(x) if x.eq_ignore_ascii_case(w))
    }
    pub fn is_punct(&self, p: &str) -> bool {
        matches!(self, Tok::Punct(x) if x == p)
    }
    /// Nama objek: Word atau Ident.
    pub fn name(&self) -> Option<&str> {
        match self {
            Tok::Word(s) | Tok::Ident(s) => Some(s),
            _ => None,
        }
    }
}

pub struct Tokenizer<'a> {
    s: &'a [u8],
    i: usize,
    in_cond: bool,
}

impl<'a> Tokenizer<'a> {
    pub fn new(s: &'a [u8]) -> Self {
        Tokenizer { s, i: 0, in_cond: false }
    }

    pub fn pos(&self) -> usize {
        self.i
    }

    /// Lewati spasi dan komentar. Isi `/*! ... */` dianggap kode biasa.
    /// `/*M! ... */` (khusus MariaDB) dianggap komentar.
    pub fn skip_trivia(&mut self) {
        let s = self.s;
        loop {
            while self.i < s.len() && s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i >= s.len() {
                return;
            }
            let c = s[self.i];
            if c == b'#' || (c == b'-' && is_line_comment(s, self.i)) {
                match memchr(b'\n', &s[self.i..]) {
                    Some(o) => self.i += o + 1,
                    None => self.i = s.len(),
                }
                continue;
            }
            if c == b'*' && self.in_cond && s.get(self.i + 1) == Some(&b'/') {
                self.in_cond = false;
                self.i += 2;
                continue;
            }
            if c == b'/' && s.get(self.i + 1) == Some(&b'*') {
                if s.get(self.i + 2) == Some(&b'!') && !self.in_cond {
                    self.i += 3;
                    while self.i < s.len() && s[self.i].is_ascii_digit() {
                        self.i += 1;
                    }
                    self.in_cond = true;
                    continue;
                }
                match memmem::find(&s[self.i + 2..], b"*/") {
                    Some(o) => self.i += 2 + o + 2,
                    None => self.i = s.len(),
                }
                continue;
            }
            return;
        }
    }

    pub fn next_tok(&mut self) -> Option<Tok> {
        self.skip_trivia();
        let s = self.s;
        if self.i >= s.len() {
            return None;
        }
        let c = s[self.i];
        let next = s.get(self.i + 1).copied();
        match c {
            b'\'' | b'"' => {
                let (v, end) = decode_string(s, self.i);
                self.i = end;
                Some(Tok::Str(v))
            }
            b'`' => {
                let mut out = Vec::new();
                let mut j = self.i + 1;
                while j < s.len() {
                    if s[j] == b'`' {
                        if s.get(j + 1) == Some(&b'`') {
                            out.push(b'`');
                            j += 2;
                            continue;
                        }
                        j += 1;
                        break;
                    }
                    out.push(s[j]);
                    j += 1;
                }
                self.i = j;
                Some(Tok::Ident(String::from_utf8_lossy(&out).into_owned()))
            }
            b'0' if matches!(next, Some(b'x') | Some(b'X')) => {
                let start = self.i + 2;
                let mut j = start;
                while j < s.len() && s[j].is_ascii_hexdigit() {
                    j += 1;
                }
                self.i = j;
                Some(Tok::Hex(hex_decode(&s[start..j])))
            }
            b'0' if matches!(next, Some(b'b')) && matches!(s.get(self.i + 2), Some(b'0') | Some(b'1')) => {
                let start = self.i + 2;
                let mut j = start;
                while j < s.len() && (s[j] == b'0' || s[j] == b'1') {
                    j += 1;
                }
                self.i = j;
                Some(Tok::Bits(String::from_utf8_lossy(&s[start..j]).into_owned()))
            }
            b'0'..=b'9' => Some(Tok::Num(self.read_number())),
            b'.' if matches!(next, Some(b'0'..=b'9')) => Some(Tok::Num(self.read_number())),
            b'x' | b'X' if next == Some(b'\'') => {
                let (v, end) = decode_string(s, self.i + 1);
                self.i = end;
                Some(Tok::Hex(hex_decode(&v)))
            }
            b'b' | b'B' if next == Some(b'\'') => {
                let (v, end) = decode_string(s, self.i + 1);
                self.i = end;
                Some(Tok::Bits(String::from_utf8_lossy(&v).into_owned()))
            }
            b'n' | b'N' if next == Some(b'\'') => {
                let (v, end) = decode_string(s, self.i + 1);
                self.i = end;
                Some(Tok::Str(v))
            }
            b'@' => {
                let start = self.i;
                let mut j = self.i + 1;
                if s.get(j) == Some(&b'@') {
                    j += 1;
                }
                while j < s.len() && (is_ident_byte(s[j]) || s[j] == b'.') {
                    j += 1;
                }
                self.i = j;
                Some(Tok::Var(String::from_utf8_lossy(&s[start..j]).into_owned()))
            }
            c if is_ident_start(c) => {
                let start = self.i;
                let mut j = self.i;
                while j < s.len() && is_ident_byte(s[j]) {
                    j += 1;
                }
                self.i = j;
                Some(Tok::Word(String::from_utf8_lossy(&s[start..j]).into_owned()))
            }
            _ => {
                const TWO: [&[u8]; 12] =
                    [b"<=>", b"->>", b"<=", b">=", b"<>", b"!=", b"||", b"&&", b":=", b"->", b"<<", b">>"];
                for op in TWO {
                    if s[self.i..].starts_with(op) {
                        self.i += op.len();
                        return Some(Tok::Punct(String::from_utf8_lossy(op).into_owned()));
                    }
                }
                self.i += 1;
                Some(Tok::Punct((c as char).to_string()))
            }
        }
    }

    fn read_number(&mut self) -> String {
        let s = self.s;
        let start = self.i;
        let mut j = self.i;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        if j < s.len() && s[j] == b'.' {
            j += 1;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
        }
        if j < s.len() && (s[j] == b'e' || s[j] == b'E') {
            let mut k = j + 1;
            if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
                k += 1;
            }
            if k < s.len() && s[k].is_ascii_digit() {
                while k < s.len() && s[k].is_ascii_digit() {
                    k += 1;
                }
                j = k;
            }
        }
        // identifier yang diawali angka, mis. `1abc`
        if j < s.len() && is_ident_byte(s[j]) {
            while j < s.len() && is_ident_byte(s[j]) {
                j += 1;
            }
            self.i = j;
            return String::from_utf8_lossy(&s[start..j]).into_owned();
        }
        self.i = j;
        String::from_utf8_lossy(&s[start..j]).into_owned()
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = Tok;
    fn next(&mut self) -> Option<Tok> {
        self.next_tok()
    }
}

pub fn tokenize(s: &[u8]) -> Vec<Tok> {
    Tokenizer::new(s).collect()
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

pub(crate) fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

/// Decode string literal yang diawali quote di `s[start]`.
/// Kembalikan (isi, posisi setelah quote penutup).
pub fn decode_string(s: &[u8], start: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let end = decode_string_into(s, start, &mut out);
    (out, end)
}

/// Seperti `decode_string`, tetapi menulis ke buffer yang sudah ada.
pub fn decode_string_into(s: &[u8], start: usize, out: &mut Vec<u8>) -> usize {
    let q = s[start];
    let mut i = start + 1;
    loop {
        match memchr2(q, b'\\', &s[i..]) {
            None => {
                out.extend_from_slice(&s[i..]);
                return s.len();
            }
            Some(off) => {
                let j = i + off;
                out.extend_from_slice(&s[i..j]);
                if s[j] == b'\\' {
                    if j + 1 >= s.len() {
                        return s.len();
                    }
                    match s[j + 1] {
                        b'0' => out.push(0),
                        b'b' => out.push(8),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'Z' => out.push(0x1a),
                        b'%' | b'_' => {
                            out.push(b'\\');
                            out.push(s[j + 1]);
                        }
                        c => out.push(c),
                    }
                    i = j + 2;
                } else if s.get(j + 1) == Some(&q) {
                    out.push(q);
                    i = j + 2;
                } else {
                    return j + 1;
                }
            }
        }
    }
}

pub fn hex_decode(h: &[u8]) -> Vec<u8> {
    fn v(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => 0,
        }
    }
    let mut out = Vec::with_capacity(h.len() / 2 + 1);
    let mut it = h;
    if h.len() % 2 == 1 {
        out.push(v(h[0]));
        it = &h[1..];
    }
    for p in it.as_chunks::<2>().0 {
        out.push(v(p[0]) << 4 | v(p[1]));
    }
    out
}

/// Render token kembali ke SQL dengan spasi seragam. Dipakai untuk ekspresi
/// (default, generated column, CHECK, partition) supaya perbandingan stabil.
pub fn render_tokens(toks: &[Tok]) -> String {
    let mut out = String::new();
    let mut prev: Option<&Tok> = None;
    for (i, t) in toks.iter().enumerate() {
        // buang introducer charset: _utf8mb4'x' -> 'x'
        if let Tok::Word(w) = t {
            if w.starts_with('_') && matches!(toks.get(i + 1), Some(Tok::Str(_)) | Some(Tok::Hex(_))) {
                continue;
            }
        }
        let text = tok_text(t);
        if let Some(p) = prev {
            let no_space = matches!(p, Tok::Punct(x) if x == "(" || x == "." || x == "@")
                || matches!(t, Tok::Punct(x) if x == ")" || x == "," || x == ".")
                || (matches!(t, Tok::Punct(x) if x == "(")
                    && matches!(p, Tok::Word(_) | Tok::Ident(_)));
            if !no_space {
                out.push(' ');
            }
        }
        out.push_str(&text);
        prev = Some(t);
    }
    out
}

/// Ekspresi kanonik: buang kurung luar yang membungkus seluruh ekspresi,
/// lalu render. `((a >= 0))` dan `(a >= 0)` jadi `a >= 0`.
pub fn canonical_expr(toks: &[Tok]) -> String {
    let mut t = toks;
    while t.len() >= 2 && t[0].is_punct("(") && t[t.len() - 1].is_punct(")") && wraps_all(t) {
        t = &t[1..t.len() - 1];
    }
    render_tokens(t)
}

fn wraps_all(t: &[Tok]) -> bool {
    let mut depth = 0;
    for (i, x) in t.iter().enumerate() {
        if x.is_punct("(") {
            depth += 1;
        } else if x.is_punct(")") {
            depth -= 1;
            if depth == 0 && i != t.len() - 1 {
                return false;
            }
        }
    }
    true
}

/// Kanonik dari teks ekspresi (tokenize ulang).
pub fn canonical_expr_str(s: &str) -> String {
    canonical_expr(&tokenize(s.as_bytes()))
}

pub fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Word(w) => w.to_ascii_lowercase(),
        Tok::Ident(w) => quote_ident(w),
        Tok::Str(s) => quote_str(s),
        Tok::Num(n) => n.clone(),
        Tok::Hex(h) => format!("0x{}", hex_encode(h)),
        Tok::Bits(b) => format!("b'{b}'"),
        Tok::Punct(p) => p.clone(),
        Tok::Var(v) => v.clone(),
    }
}

pub fn quote_ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

pub fn quote_str(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    escape_into(s, &mut out);
    out.push('\'');
    out
}

/// Escape byte string untuk literal SQL. Byte non-UTF-8 dipertahankan lewat
/// lossy hanya kalau valid; pemanggil yang butuh biner memakai hex.
pub fn escape_into(s: &[u8], out: &mut String) {
    let text = String::from_utf8_lossy(s);
    for ch in text.chars() {
        match ch {
            '\0' => out.push_str("\\0"),
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{1a}' => out.push_str("\\Z"),
            c => out.push(c),
        }
    }
}

pub fn hex_encode(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(sql: &str) -> Vec<String> {
        let mut r = StatementReader::new(sql.as_bytes());
        let mut out = vec![];
        while let Some(s) = r.next_statement().unwrap() {
            out.push(String::from_utf8(s.bytes).unwrap());
        }
        out
    }

    #[test]
    fn splits_basic_and_strings() {
        let v = split("-- c\nSET a=1;\nINSERT INTO t VALUES ('a;b','c\\'d;'),(\"x;\");\n");
        assert_eq!(v, vec!["SET a=1", "INSERT INTO t VALUES ('a;b','c\\'d;'),(\"x;\")"]);
    }

    #[test]
    fn splits_conditional_comments_and_delimiter() {
        let sql = "/*!40101 SET NAMES utf8 */;\nDELIMITER ;;\nCREATE TRIGGER t BEGIN SELECT 1; SELECT 2; END ;;\nDELIMITER ;\nSELECT 3;";
        let v = split(sql);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], "/*!40101 SET NAMES utf8 */");
        assert!(v[1].starts_with("CREATE TRIGGER") && v[1].ends_with("END"));
        assert_eq!(v[2], "SELECT 3");
    }

    #[test]
    fn statement_offsets_point_into_file() {
        let sql = "-- head\n\n  SELECT 1;\nSELECT 'x\ny';";
        let mut r = StatementReader::new(sql.as_bytes());
        while let Some(s) = r.next_statement().unwrap() {
            let o = s.offset as usize;
            assert_eq!(&sql.as_bytes()[o..o + s.bytes.len()], &s.bytes[..]);
        }
    }

    #[test]
    fn tokenizes_mysql_literals() {
        let t = tokenize(b"`a``b` 'it''s\\n' 0x4142 X'43' b'101' -1.5e3 @@x /*!50100 KEY */ /*M!999999\\- x */ y");
        assert_eq!(t[0], Tok::Ident("a`b".into()));
        assert_eq!(t[1], Tok::Str(b"it's\n".to_vec()));
        assert_eq!(t[2], Tok::Hex(b"AB".to_vec()));
        assert_eq!(t[3], Tok::Hex(b"C".to_vec()));
        assert_eq!(t[4], Tok::Bits("101".into()));
        assert_eq!(t[5], Tok::Punct("-".into()));
        assert_eq!(t[6], Tok::Num("1.5e3".into()));
        assert_eq!(t[7], Tok::Var("@@x".into()));
        assert_eq!(t[8], Tok::Word("KEY".into()));
        assert_eq!(t[9], Tok::Word("y".into()));
    }
}
