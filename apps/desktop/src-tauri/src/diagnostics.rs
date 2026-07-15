use std::path::Path;

pub fn initialize() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ignored = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .without_time()
        .try_init();
}

#[must_use]
pub fn redact_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && let Ok(relative) = path.strip_prefix(home)
    {
        return format!("<home>/{}", relative.display());
    }
    "<external-path>".to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::redact_path;

    #[test]
    fn shareable_paths_hide_home_and_external_roots() {
        let home = Path::new("C:/Users/Example");
        assert_eq!(
            redact_path(Path::new("C:/Users/Example/project/file.png"), Some(home)),
            "<home>/project/file.png"
        );
        assert_eq!(
            redact_path(Path::new("D:/private/file.png"), Some(home)),
            "<external-path>"
        );
    }
}
