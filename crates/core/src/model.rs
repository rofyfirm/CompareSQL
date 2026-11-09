//! Model internal schema. File dump dan DB live sama-sama menghasilkan model ini.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Schema {
    pub tables: Vec<Table>,
    /// Objek di luar scope MVP (view, trigger, routine, event).
    pub skipped: Vec<SkippedObject>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedObject {
    pub kind: String,
    pub name: String,
}

impl Schema {
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.name == name)
    }
    pub fn table_ci(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.name.eq_ignore_ascii_case(name))
    }
    /// Tambah atau ganti tabel (CREATE setelah DROP pada nama sama).
    pub fn upsert(&mut self, t: Table) {
        if let Some(pos) = self.tables.iter().position(|x| x.name == t.name) {
            self.tables[pos] = t;
        } else {
            self.tables.push(t);
        }
    }
    pub fn remove(&mut self, name: &str) {
        self.tables.retain(|t| t.name != name);
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub primary_key: Option<Index>,
    pub indexes: Vec<Index>,
    pub foreign_keys: Vec<ForeignKey>,
    pub checks: Vec<Check>,
    pub options: TableOptions,
}

impl Table {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Column {
    pub name: String,
    /// Tipe ter-normalisasi, huruf kecil: `int unsigned`, `varchar(255)`, `enum('a','b')`.
    pub data_type: String,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub nullable: bool,
    pub default: Option<DefaultValue>,
    pub auto_increment: bool,
    pub on_update: Option<String>,
    pub comment: Option<String>,
    pub generated: Option<Generated>,
    pub invisible: bool,
    /// CHECK di level kolom (MariaDB).
    pub check: Option<String>,
    /// Atribut lain yang tidak dikenali, disimpan apa adanya.
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value")]
pub enum DefaultValue {
    Null,
    Str(Vec<u8>),
    Num(String),
    Bits(String),
    Hex(Vec<u8>),
    /// CURRENT_TIMESTAMP dengan presisi opsional.
    CurrentTimestamp(Option<u8>),
    /// Ekspresi lain, sudah dirender kanonik tanpa kurung luar.
    Expr(String),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Generated {
    pub expr: String,
    pub stored: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum IndexKind {
    Primary,
    Unique,
    Index,
    Fulltext,
    Spatial,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Index {
    pub name: String,
    pub kind: IndexKind,
    pub parts: Vec<IndexPart>,
    pub using: Option<String>,
    pub comment: Option<String>,
    pub invisible: bool,
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IndexPart {
    pub column: Option<String>,
    /// Functional key part, mis. `((lower(name)))`.
    pub expr: Option<String>,
    pub prefix: Option<u32>,
    pub desc: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Check {
    pub name: Option<String>,
    pub expr: String,
    pub enforced: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TableOptions {
    pub engine: Option<String>,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub comment: Option<String>,
    pub row_format: Option<String>,
    pub auto_increment: Option<u64>,
    /// Opsi lain (KEY_BLOCK_SIZE, PAGE_CHECKSUM, ...).
    pub other: BTreeMap<String, String>,
    pub partition: Option<String>,
}

/// Kelas tipe untuk format literal dan normalisasi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeClass {
    Integer,
    Decimal,
    Float,
    Bit,
    Binary,
    Text,
    Temporal,
    Json,
    Spatial,
    Other,
}

pub fn base_type(data_type: &str) -> &str {
    let end = data_type
        .find(['(', ' '])
        .unwrap_or(data_type.len());
    &data_type[..end]
}

pub fn type_class(data_type: &str) -> TypeClass {
    match base_type(data_type) {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "year" => TypeClass::Integer,
        "decimal" | "numeric" | "dec" | "fixed" => TypeClass::Decimal,
        "float" | "double" | "real" => TypeClass::Float,
        "bit" => TypeClass::Bit,
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" => TypeClass::Binary,
        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext" | "enum" | "set" => {
            TypeClass::Text
        }
        "date" | "datetime" | "timestamp" | "time" => TypeClass::Temporal,
        "json" => TypeClass::Json,
        "geometry" | "point" | "linestring" | "polygon" | "multipoint" | "multilinestring"
        | "multipolygon" | "geometrycollection" | "geomcollection" => TypeClass::Spatial,
        _ => TypeClass::Other,
    }
}

impl TypeClass {
    pub fn is_numeric(self) -> bool {
        matches!(self, TypeClass::Integer | TypeClass::Decimal | TypeClass::Float | TypeClass::Bit)
    }
    pub fn is_binary(self) -> bool {
        matches!(self, TypeClass::Binary | TypeClass::Spatial)
    }
}
