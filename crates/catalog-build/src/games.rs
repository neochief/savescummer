//! Parse `catalog/games.csv`, the authoritative list of supported games.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductFit {
    Keep,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GameRow {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(default, rename = "Info")]
    pub info: String,
    #[serde(rename = "Product fit")]
    pub product_fit: String,
}

impl GameRow {
    pub fn fit(&self) -> Option<ProductFit> {
        match self.product_fit.trim() {
            value if value.eq_ignore_ascii_case("keep") => Some(ProductFit::Keep),
            value if value.eq_ignore_ascii_case("remove") => Some(ProductFit::Remove),
            _ => None,
        }
    }
}

/// Parse the CSV by header name so human-added columns are ignored. Fails on
/// duplicate names and unknown `Product fit` values (Section 3.5).
pub fn parse(text: &str) -> Result<Vec<GameRow>, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let required = ["Name", "Info", "Product fit"];
    for column in required {
        if !headers.iter().any(|header| header == column) {
            return Err(format!("games.csv is missing the {column:?} column"));
        }
    }
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        let mut row: GameRow = record
            .deserialize(Some(&headers))
            .map_err(|e| e.to_string())?;
        row.name = row.name.trim().into();
        if row.name.is_empty() {
            return Err("games.csv has an empty Name".into());
        }
        if row.fit().is_none() {
            return Err(format!(
                "games.csv row {:?} has an unknown Product fit {:?}",
                row.name, row.product_fit
            ));
        }
        rows.push(row);
    }
    let mut seen = std::collections::BTreeSet::new();
    for row in &rows {
        if !seen.insert(row.name.to_lowercase()) {
            return Err(format!("duplicate game name {:?}", row.name));
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiline_info_and_ignores_extra_columns() {
        let rows = parse(
            "\u{feff}\"Name\",\"Extra1\",\"Extra2\",\"Info\",\"Product fit\",\"Extra3\"\n\
\"ADOM: Ancient Domains of Mystery\",\"yes\",\"cat\",\"Line one,\n\n\"\"quoted\"\" line two\",\"Keep\",\"\"\n\
\"Void War\",\"yes\",\"cat\",\"Info\",\"Keep\",\"\"\n\
\"Removed\",\"yes\",\"cat\",\"Info\",\"Remove\",\"\"\n",
        )
        .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "ADOM: Ancient Domains of Mystery");
        assert!(rows[0].info.contains("quoted"));
        assert_eq!(rows[2].fit(), Some(ProductFit::Remove));
    }

    #[test]
    fn rejects_duplicate_names_and_unknown_fit() {
        let header = "\"Name\",\"Info\",\"Product fit\"\n";
        assert!(
            parse(&format!(
                "{header}\"A\",\"i\",\"Keep\"\n\"a\",\"i\",\"Keep\"\n"
            ))
            .is_err()
        );
        assert!(parse(&format!("{header}\"A\",\"i\",\"Maybe\"\n")).is_err());
    }
}
