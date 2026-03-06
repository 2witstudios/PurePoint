/// Validate a resource name used for file-system paths (template, agent def, swarm def).
/// Rejects names that could escape the target directory or create hidden files.
pub fn validate_name(name: &str) -> Result<(), std::io::Error> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "name must not be empty or whitespace-only",
        ));
    }
    if trimmed.starts_with('.') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("name must not start with '.': {name}"),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("name contains invalid characters: {name}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_valid_name_should_succeed() {
        assert!(validate_name("my-template").is_ok());
        assert!(validate_name("code-review").is_ok());
        assert!(validate_name("test_123").is_ok());
    }

    #[test]
    fn given_path_traversal_should_reject() {
        assert!(validate_name("../evil").is_err());
        assert!(validate_name("../../etc/foo").is_err());
        assert!(validate_name("foo/../bar").is_err());
    }

    #[test]
    fn given_slash_should_reject() {
        assert!(validate_name("foo/bar").is_err());
        assert!(validate_name("foo\\bar").is_err());
    }

    #[test]
    fn given_hidden_name_should_reject() {
        assert!(validate_name(".hidden").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
    }

    #[test]
    fn given_empty_should_reject() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name("\t").is_err());
    }
}
