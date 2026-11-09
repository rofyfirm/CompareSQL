//! Representasi baris tanpa alokasi per sel, dan trait sumber baris.

use crate::model::Table;
use crate::Result;

/// Nilai sel milik sendiri. `None` = NULL. Byte = teks seperti protokol
/// teks MySQL (angka sebagai teks, biner sebagai byte mentah).
pub type Cell = Option<Vec<u8>>;

pub const KIND_STR: u8 = 0;
pub const KIND_NUM: u8 = 1;
pub const KIND_HEX: u8 = 2;
pub const KIND_BITS: u8 = 3;

/// Satu baris; semua byte sel di satu buffer.
#[derive(Default, Debug)]
pub struct RowBuf {
    pub data: Vec<u8>,
    cells: Vec<Option<(u32, u32)>>,
    kinds: Vec<u8>,
}

impl RowBuf {
    pub fn clear(&mut self) {
        self.data.clear();
        self.cells.clear();
        self.kinds.clear();
    }
    pub fn len(&self) -> usize {
        self.cells.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
    pub fn push_null(&mut self) {
        self.cells.push(None);
        self.kinds.push(KIND_STR);
    }
    pub fn push(&mut self, b: &[u8], kind: u8) {
        let s = self.data.len();
        self.data.extend_from_slice(b);
        self.cells.push(Some((s as u32, self.data.len() as u32)));
        self.kinds.push(kind);
    }
    /// Mulai sel yang ditulis langsung ke `data`.
    pub fn begin(&self) -> usize {
        self.data.len()
    }
    pub fn end(&mut self, start: usize, kind: u8) {
        self.cells.push(Some((start as u32, self.data.len() as u32)));
        self.kinds.push(kind);
    }
    pub fn get(&self, i: usize) -> Option<&[u8]> {
        self.cells.get(i).copied().flatten().map(|(s, e)| &self.data[s as usize..e as usize])
    }
    pub fn kind(&self, i: usize) -> u8 {
        self.kinds.get(i).copied().unwrap_or(KIND_STR)
    }
    /// Ganti isi sel i (byte baru ditaruh di akhir buffer).
    pub fn replace(&mut self, i: usize, b: &[u8], kind: u8) {
        let s = self.data.len();
        self.data.extend_from_slice(b);
        self.cells[i] = Some((s as u32, self.data.len() as u32));
        self.kinds[i] = kind;
    }
}

/// Baris dalam urutan kolom tabel. `map[col]` = indeks sel di buffer.
pub struct RowView<'a> {
    pub buf: &'a RowBuf,
    pub map: &'a [Option<usize>],
}

impl<'a> RowView<'a> {
    pub fn get(&self, col: usize) -> Option<&'a [u8]> {
        self.map.get(col).copied().flatten().and_then(|ci| self.buf.get(ci))
    }
}

pub type RowFn<'f> = dyn FnMut(&RowView<'_>) -> Result<()> + 'f;

/// Sumber baris per tabel: file dump atau DB live.
pub trait RowSource {
    /// False bila sumber hanya schema (tidak ada data sama sekali).
    fn has_data(&self) -> bool;
    /// Iterasi semua baris tabel; sel dalam urutan `table.columns`.
    fn for_each_row(&mut self, table: &Table, f: &mut RowFn<'_>) -> Result<()>;
}

pub struct EmptySource;

impl RowSource for EmptySource {
    fn has_data(&self) -> bool {
        true
    }
    fn for_each_row(&mut self, _: &Table, _: &mut RowFn<'_>) -> Result<()> {
        Ok(())
    }
}
