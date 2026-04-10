pub fn join_errors(errors: &[impl ToString], sep: &str) -> String {
    errors
        .iter()
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slice_returns_empty_string() {
        let errors: Vec<String> = vec![];
        assert_eq!(join_errors(&errors, ", "), "");
    }

    #[test]
    fn single_error() {
        let errors = vec!["something broke"];
        assert_eq!(join_errors(&errors, ", "), "something broke");
    }

    #[test]
    fn multiple_errors_with_comma_separator() {
        let errors = vec!["err1", "err2", "err3"];
        assert_eq!(join_errors(&errors, ", "), "err1, err2, err3");
    }

    #[test]
    fn multiple_errors_with_semicolon_separator() {
        let errors = vec!["err1", "err2"];
        assert_eq!(join_errors(&errors, "; "), "err1; err2");
    }

    #[test]
    fn works_with_string_type() {
        let errors = vec![String::from("a"), String::from("b")];
        assert_eq!(join_errors(&errors, " | "), "a | b");
    }

    #[test]
    fn works_with_io_errors() {
        let errors = vec![
            std::io::Error::new(std::io::ErrorKind::NotFound, "file missing"),
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied"),
        ];
        assert_eq!(join_errors(&errors, "; "), "file missing; access denied");
    }
}
