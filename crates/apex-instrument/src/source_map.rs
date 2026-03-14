use apex_core::{hash::fnv1a_hash, types::BranchId};
use std::{collections::HashMap, path::{Path, PathBuf}};
use tracing::warn;

/// Remap branch IDs from emitted JS locations to original TS/source locations.
pub fn remap_source_maps(
    branches: Vec<BranchId>,
    file_paths: &HashMap<u64, PathBuf>,
    target: &Path,
) -> (Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut remapped_branches = Vec::new();
    let mut remapped_file_paths = HashMap::new();

    // Pre-load source maps for each unique file_id
    let mut source_maps: HashMap<u64, Option<sourcemap::SourceMap>> = HashMap::new();
    for (&file_id, rel_path) in file_paths {
        let abs_path = target.join(rel_path);
        let sm = load_source_map(&abs_path);
        source_maps.insert(file_id, sm);
    }

    for branch in branches {
        let sm_opt = source_maps.get(&branch.file_id).and_then(|s| s.as_ref());

        if let Some(sm) = sm_opt {
            let line_0 = branch.line.saturating_sub(1);
            let col = branch.col as u32;

            if let Some(token) = sm.lookup_token(line_0, col) {
                if let Some(source) = token.get_source() {
                    let source_root = sm.get_source_root().unwrap_or("");
                    let original_path = if source_root.is_empty() {
                        PathBuf::from(source)
                    } else {
                        PathBuf::from(source_root).join(source)
                    };

                    let original_rel = original_path.to_string_lossy();
                    let new_file_id = fnv1a_hash(&original_rel);
                    let new_line = token.get_src_line() + 1; // back to 1-based
                    let new_col = token.get_src_col().min(u16::MAX as u32) as u16;

                    remapped_file_paths.insert(new_file_id, original_path);
                    remapped_branches.push(BranchId::new(new_file_id, new_line, new_col, branch.direction));
                    continue;
                }
            }
            // Source map exists but no mapping found — generated code, drop it
        } else {
            // No source map — keep original location
            if let Some(path) = file_paths.get(&branch.file_id) {
                remapped_file_paths.insert(branch.file_id, path.clone());
            }
            remapped_branches.push(branch);
        }
    }

    (remapped_branches, remapped_file_paths)
}

/// Try to load a source map for the given JS file.
fn load_source_map(js_path: &Path) -> Option<sourcemap::SourceMap> {
    // Try .map sidecar
    let map_path = js_path.with_extension("js.map");
    if map_path.exists() {
        match std::fs::read(&map_path) {
            Ok(bytes) => return sourcemap::SourceMap::from_reader(&bytes[..]).ok(),
            Err(e) => warn!(path = %map_path.display(), error = %e, "failed to read source map"),
        }
    }

    // Try inline source map in the JS file
    if let Ok(content) = std::fs::read_to_string(js_path) {
        if let Some(pos) = content.rfind("//# sourceMappingURL=data:") {
            let data_url = &content[pos + 26..];
            if let Some(comma_pos) = data_url.find(',') {
                let b64 = data_url[comma_pos + 1..].trim();
                if let Ok(decoded) = base64_decode(b64) {
                    return sourcemap::SourceMap::from_reader(&decoded[..]).ok();
                }
            }
        }
    }

    None
}

/// Simple base64 decode (avoid extra dependency).
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.trim_end_matches('=');
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in input.as_bytes() {
        let val = TABLE.iter().position(|&c| c == byte).ok_or(())? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_no_source_maps_passes_through() {
        let mut file_paths = HashMap::new();
        file_paths.insert(42, PathBuf::from("src/app.js"));
        let branches = vec![BranchId::new(42, 10, 5, 0)];
        let (remapped, new_files) = remap_source_maps(branches, &file_paths, Path::new("/nonexistent"));
        assert_eq!(remapped.len(), 1);
        assert_eq!(remapped[0].file_id, 42);
        assert_eq!(remapped[0].line, 10);
        assert!(new_files.contains_key(&42));
    }

    #[test]
    fn base64_decode_basic() {
        let encoded = "SGVsbG8=";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn base64_decode_no_padding() {
        let encoded = "SGVsbG8";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn load_source_map_nonexistent_file() {
        assert!(load_source_map(Path::new("/no/such/file.js")).is_none());
    }
}
