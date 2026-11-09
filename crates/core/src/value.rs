//! Parser cepat `VALUES (...),(...)` dan format literal untuk script.

use crate::lexer::{decode_string_into, hex_decode, hex_encode, is_ident_byte, escape_into};
use crate::model::TypeClass;
use crate::normalize::looks_numeric;
use crate::row::*;
use crate::{Error, Result};

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Parse tuple mulai posisi `i` (setelah kata VALUES). Panggil `f` tiap baris.
pub fn parse_values<F>(s: &[u8], mut i: usize, row: &mut RowBuf, mut f: F) -> Result<u64>
where
    F: FnMut(&mut RowBuf) -> Result<()>,
{
    let n = s.len();
    let mut count = 0u64;
    loop {
        i = skip_ws(s, i);
        if i >= n || s[i] != b'(' {
            break;
        }
        i += 1;
        row.clear();
        loop {
            i = skip_ws(s, i);
            if i >= n {
                return Err(Error::parse("tuple VALUES terpotong"));
            }
            if s[i] == b')' && row.is_empty() {
                i += 1;
                break;
            }
            i = parse_value(s, i, row)?;
            i = skip_ws(s, i);
            match s.get(i) {
                Some(b',') => i += 1,
                Some(b')') => {
                    i += 1;
                    break;
                }
                _ => return Err(Error::parse(format!("karakter tak terduga di posisi {i} dalam VALUES"))),
            }
        }
        f(row)?;
        count += 1;
        i = skip_ws(s, i);
        if i < n && s[i] == b',' {
            i += 1;
            continue;
        }
        break;
    }
    Ok(count)
}

fn bits_to_decimal(bits: &[u8]) -> Vec<u8> {
    let mut v: u128 = 0;
    for &b in bits {
        if b == b'0' || b == b'1' {
            v = v.wrapping_shl(1) | (b - b'0') as u128;
        }
    }
    v.to_string().into_bytes()
}

/// Byte big-endian -> desimal (untuk kolom BIT yang di-dump sebagai 0x..).
pub fn bytes_to_decimal(b: &[u8]) -> Vec<u8> {
    let mut v: u128 = 0;
    for &x in b.iter().rev().take(16).collect::<Vec<_>>().into_iter().rev() {
        v = (v << 8) | x as u128;
    }
    v.to_string().into_bytes()
}

fn parse_value(s: &[u8], i: usize, row: &mut RowBuf) -> Result<usize> {
    let n = s.len();
    let c = s[i];
    let next = s.get(i + 1).copied();
    match c {
        b'\'' | b'"' => {
            let st = row.begin();
            let end = decode_string_into(s, i, &mut row.data);
            row.end(st, KIND_STR);
            Ok(end)
        }
        b'N' | b'n' if n - i >= 4 && s[i..i + 4].eq_ignore_ascii_case(b"NULL") && !s.get(i + 4).is_some_and(|&b| is_ident_byte(b)) => {
            row.push_null();
            Ok(i + 4)
        }
        b'0' if matches!(next, Some(b'x') | Some(b'X')) => {
            let mut j = i + 2;
            while j < n && s[j].is_ascii_hexdigit() {
                j += 1;
            }
            row.push(&hex_decode(&s[i + 2..j]), KIND_HEX);
            Ok(j)
        }
        b'0' if next == Some(b'b') && matches!(s.get(i + 2), Some(b'0') | Some(b'1')) => {
            let mut j = i + 2;
            while j < n && (s[j] == b'0' || s[j] == b'1') {
                j += 1;
            }
            row.push(&bits_to_decimal(&s[i + 2..j]), KIND_BITS);
            Ok(j)
        }
        b'x' | b'X' if next == Some(b'\'') => {
            let mut tmp = Vec::new();
            let end = decode_string_into(s, i + 1, &mut tmp);
            row.push(&hex_decode(&tmp), KIND_HEX);
            Ok(end)
        }
        b'b' | b'B' if next == Some(b'\'') => {
            let mut tmp = Vec::new();
            let end = decode_string_into(s, i + 1, &mut tmp);
            row.push(&bits_to_decimal(&tmp), KIND_BITS);
            Ok(end)
        }
        b'_' => {
            // introducer: _binary 'x', _utf8mb4 0x..
            let mut j = i + 1;
            while j < n && is_ident_byte(s[j]) {
                j += 1;
            }
            let j = skip_ws(s, j);
            if j >= n {
                return Err(Error::parse("introducer tanpa nilai"));
            }
            parse_value(s, j, row)
        }
        b'-' | b'+' | b'.' | b'0'..=b'9' => {
            let start = if c == b'+' { i + 1 } else { i };
            let mut j = i + 1;
            while j < n {
                let d = s[j];
                if d.is_ascii_digit() || d == b'.' {
                    j += 1;
                } else if (d == b'e' || d == b'E') && j + 1 < n {
                    j += 1;
                    if s[j] == b'-' || s[j] == b'+' {
                        j += 1;
                    }
                } else {
                    break;
                }
            }
            row.push(&s[start..j], KIND_NUM);
            Ok(j)
        }
        b'T' | b't' if n - i >= 4 && s[i..i + 4].eq_ignore_ascii_case(b"TRUE") => {
            row.push(b"1", KIND_NUM);
            Ok(i + 4)
        }
        b'F' | b'f' if n - i >= 5 && s[i..i + 5].eq_ignore_ascii_case(b"FALSE") => {
            row.push(b"0", KIND_NUM);
            Ok(i + 5)
        }
        _ => Err(Error::parse(format!("nilai tidak dikenal di posisi {i}"))),
    }
}

/// Tulis literal SQL untuk satu sel sesuai kelas tipe kolom.
pub fn write_literal(out: &mut String, cell: Option<&[u8]>, class: TypeClass) {
    match cell {
        None => out.push_str("NULL"),
        Some(b) if class.is_binary() => {
            if b.is_empty() {
                out.push_str("''");
            } else {
                out.push_str("0x");
                out.push_str(&hex_encode(b));
            }
        }
        Some(b) if class.is_numeric() && looks_numeric(b) => {
            out.push_str(std::str::from_utf8(b).unwrap_or("0"));
        }
        Some(b) => match std::str::from_utf8(b) {
            Ok(_) => {
                out.push('\'');
                escape_into(b, out);
                out.push('\'');
            }
            Err(_) => {
                out.push_str("0x");
                out.push_str(&hex_encode(b));
            }
        },
    }
}

pub fn literal(cell: Option<&[u8]>, class: TypeClass) -> String {
    let mut s = String::new();
    write_literal(&mut s, cell, class);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &str) -> Vec<Vec<Option<Vec<u8>>>> {
        let mut rb = RowBuf::default();
        let mut out = vec![];
        parse_values(s.as_bytes(), 0, &mut rb, |r| {
            out.push((0..r.len()).map(|i| r.get(i).map(|b| b.to_vec())).collect());
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn parses_tuples() {
        let r = rows("(1,'a\\'b',NULL,-2.5e3,0x4142,_binary 'x\\0',b'101'),(2,'',NULL,+3,X'43',_utf8mb4'z',0b11);");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0][1].as_deref(), Some(&b"a'b"[..]));
        assert_eq!(r[0][2], None);
        assert_eq!(r[0][3].as_deref(), Some(&b"-2.5e3"[..]));
        assert_eq!(r[0][4].as_deref(), Some(&b"AB"[..]));
        assert_eq!(r[0][5].as_deref(), Some(&b"x\0"[..]));
        assert_eq!(r[0][6].as_deref(), Some(&b"5"[..]));
        assert_eq!(r[1][3].as_deref(), Some(&b"3"[..]));
        assert_eq!(r[1][6].as_deref(), Some(&b"3"[..]));
    }

    #[test]
    fn literals() {
        assert_eq!(literal(None, TypeClass::Text), "NULL");
        assert_eq!(literal(Some(b"12.5"), TypeClass::Decimal), "12.5");
        assert_eq!(literal(Some(b"it's\n"), TypeClass::Text), "'it\\'s\\n'");
        assert_eq!(literal(Some(&[0xff, 0]), TypeClass::Binary), "0xFF00");
        assert_eq!(literal(Some(b"abc"), TypeClass::Integer), "'abc'");
        assert_eq!(bytes_to_decimal(&[1, 0]), b"256");
    }
}
