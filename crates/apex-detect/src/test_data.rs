//! Test Data Generation — generates realistic test data from SQL schemas.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct Column {
    pub name: String,
    pub col_type: String,
    pub nullable: bool,
    pub has_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
}

static CREATE_TABLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\S+)\s*\((.*?)\);").unwrap()
});

static COLUMN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(\w+)\s+(\w+(?:\([^)]*\))?)\s*(.*)$").unwrap()
});

pub fn parse_schema(sql: &str) -> Vec<Table> {
    let mut tables = Vec::new();
    for cap in CREATE_TABLE_RE.captures_iter(sql) {
        let name = cap[1].trim_matches('"').trim_matches('`').to_string();
        let body = &cap[2];
        let mut columns = Vec::new();
        for col_line in body.split(',') {
            let trimmed = col_line.trim();
            let upper = trimmed.to_uppercase();
            if upper.starts_with("PRIMARY")
                || upper.starts_with("FOREIGN")
                || upper.starts_with("UNIQUE")
                || upper.starts_with("CHECK")
                || upper.starts_with("CONSTRAINT")
            {
                continue;
            }
            if let Some(col_cap) = COLUMN_RE.captures(trimmed) {
                let col_name = col_cap[1].to_string();
                let col_type = col_cap[2].to_uppercase();
                let rest = col_cap[3].to_uppercase();
                columns.push(Column {
                    name: col_name,
                    col_type,
                    nullable: !rest.contains("NOT NULL"),
                    has_default: rest.contains("DEFAULT"),
                });
            }
        }
        tables.push(Table { name, columns });
    }
    tables
}

pub fn generate_inserts(tables: &[Table], rows: usize) -> String {
    let mut sql = String::new();
    for table in tables {
        let cols: Vec<&str> = table
            .columns
            .iter()
            .filter(|c| !c.col_type.contains("SERIAL") && !c.has_default)
            .map(|c| c.name.as_str())
            .collect();
        if cols.is_empty() {
            continue;
        }
        for i in 0..rows {
            let vals: Vec<String> = table
                .columns
                .iter()
                .filter(|c| !c.col_type.contains("SERIAL") && !c.has_default)
                .map(|c| gen_value(&c.col_type, &c.name, i, c.nullable))
                .collect();
            sql.push_str(&format!(
                "INSERT INTO {} ({}) VALUES ({});\n",
                table.name,
                cols.join(", "),
                vals.join(", ")
            ));
        }
        sql.push('\n');
    }
    sql
}

fn gen_value(col_type: &str, col_name: &str, row: usize, nullable: bool) -> String {
    let name_lower = col_name.to_lowercase();
    if nullable && row % 7 == 0 {
        return "NULL".into();
    }
    if name_lower.contains("email") {
        return format!("'user{}@example.com'", row + 1);
    }
    if name_lower.contains("name") {
        let names = ["Alice", "Bob", "Charlie", "Diana", "Eve", "Frank"];
        return format!("'{}'", names[row % names.len()]);
    }
    if name_lower.contains("phone") {
        return format!("'+1555{:07}'", row + 1000000);
    }
    match col_type {
        t if t.contains("INT") => format!("{}", row + 1),
        t if t.contains("VARCHAR") || t.contains("TEXT") => {
            format!("'{}_{}'", col_name, row + 1)
        }
        t if t.contains("BOOL") => {
            if row % 2 == 0 {
                "TRUE"
            } else {
                "FALSE"
            }
            .into()
        }
        t if t.contains("TIMESTAMP") || t.contains("DATE") => {
            format!("'2026-01-{:02} 12:00:00'", (row % 28) + 1)
        }
        t if t.contains("NUMERIC") || t.contains("DECIMAL") || t.contains("FLOAT") => {
            format!("{:.2}", (row as f64 + 1.0) * 9.99)
        }
        t if t.contains("UUID") => format!("'00000000-0000-0000-0000-{:012}'", row + 1),
        t if t.contains("JSON") => "'{}'".into(),
        _ => format!("'{}_{}'", col_name, row + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_table() {
        let sql =
            "CREATE TABLE users (id SERIAL PRIMARY KEY, name VARCHAR(100) NOT NULL, email TEXT);";
        let tables = parse_schema(sql);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "users");
        assert!(tables[0].columns.len() >= 2);
    }

    #[test]
    fn generates_inserts() {
        let sql = "CREATE TABLE items (id SERIAL, name VARCHAR(50) NOT NULL, price NUMERIC);";
        let tables = parse_schema(sql);
        let ins = generate_inserts(&tables, 3);
        assert!(ins.contains("INSERT INTO items"));
        assert_eq!(ins.lines().filter(|l| l.starts_with("INSERT")).count(), 3);
    }

    #[test]
    fn email_gets_email_value() {
        let sql = "CREATE TABLE t (email TEXT NOT NULL);";
        let tables = parse_schema(sql);
        let ins = generate_inserts(&tables, 1);
        assert!(ins.contains("@example.com"));
    }

    #[test]
    fn skips_serial_columns() {
        let sql = "CREATE TABLE t (id SERIAL, name TEXT);";
        let tables = parse_schema(sql);
        let ins = generate_inserts(&tables, 1);
        assert!(!ins.contains(", id,") && !ins.starts_with("INSERT INTO t (id"));
    }
}
