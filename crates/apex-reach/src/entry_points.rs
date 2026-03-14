/// Classification of function entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryPointKind {
    Test,
    Main,
    HttpHandler,
    PublicApi,
    CliEntry,
}

impl std::fmt::Display for EntryPointKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Test => write!(f, "test"),
            Self::Main => write!(f, "main"),
            Self::HttpHandler => write!(f, "http"),
            Self::PublicApi => write!(f, "api"),
            Self::CliEntry => write!(f, "cli"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_formats() {
        assert_eq!(EntryPointKind::Test.to_string(), "test");
        assert_eq!(EntryPointKind::HttpHandler.to_string(), "http");
    }
}
