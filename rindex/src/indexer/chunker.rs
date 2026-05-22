use crate::indexer::parser::{parse_code, Symbol};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub chunk_type: String,
    pub name: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

pub fn chunk_file(content: &str, language: &str) -> Result<Vec<Chunk>> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let symbols = parse_code(content, language)?;
    if !symbols.is_empty() {
        return Ok(symbols_to_chunks(&symbols));
    }
    Ok(paragraph_chunk(content))
}

fn symbols_to_chunks(symbols: &[Symbol]) -> Vec<Chunk> {
    symbols
        .iter()
        .map(|s| Chunk {
            chunk_type: s.symbol_type.clone(),
            name: s.name.clone(),
            start_line: s.start_line,
            end_line: s.end_line,
            content: s.content.clone(),
        })
        .collect()
}

fn paragraph_chunk(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut start = 0;
    let mut buf = String::new();

    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() && !buf.trim().is_empty() {
            chunks.push(Chunk {
                chunk_type: "paragraph".to_string(),
                name: None,
                start_line: start + 1,
                end_line: i + 1,
                content: buf.trim().to_string(),
            });
            buf.clear();
            start = i + 1;
        } else {
            if buf.is_empty() {
                start = i;
            }
            buf.push_str(line);
            buf.push('\n');
        }
    }

    if !buf.trim().is_empty() {
        chunks.push(Chunk {
            chunk_type: "paragraph".to_string(),
            name: None,
            start_line: start + 1,
            end_line: lines.len(),
            content: buf.trim().to_string(),
        });
    }

    chunks
}
