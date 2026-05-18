use camino::{Utf8Path, Utf8PathBuf};

pub fn clamp_u64(val: u64, min: u64, max: u64) -> u64 {
    val.clamp(min, max)
}

pub fn clamp_usize(val: usize, min: usize, max: usize) -> usize {
    val.clamp(min, max)
}

pub fn resolve_path(root: &Utf8Path, candidate: &str) -> Option<Utf8PathBuf> {
    let candidate = Utf8Path::new(candidate);
    if candidate.as_str().contains("..") {
        return None;
    }
    let resolved = if candidate.as_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(candidate)
    };
    // `Utf8Path::join` uses the platform separator (`\` on Windows);
    // normalize to `/` so resolved paths are stable across platforms.
    let resolved = Utf8PathBuf::from(resolved.as_str().replace('\\', "/"));
    let root_norm = root.as_str().replace('\\', "/");
    if resolved.as_str().starts_with(&root_norm) {
        Some(resolved)
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn valid_subpath_is_accepted() {
        let root = Utf8Path::new("/repo");
        let resolved = resolve_path(root, "src/main.rs");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().as_str(), "/repo/src/main.rs");
    }

    #[test]
    fn traversal_with_dotdot_is_rejected() {
        let root = Utf8Path::new("/repo");
        assert!(resolve_path(root, "../etc/passwd").is_none());
    }

    #[test]
    fn empty_path_is_accepted() {
        let root = Utf8Path::new("/repo");
        let resolved = resolve_path(root, "");
        assert_eq!(resolved.unwrap().as_str(), "/repo");
    }

    #[test]
    fn clamp_u64_bounds() {
        assert_eq!(clamp_u64(0, 1, 100), 1);
        assert_eq!(clamp_u64(50, 1, 100), 50);
        assert_eq!(clamp_u64(200, 1, 100), 100);
    }
}
