//! Shared detection patterns used across all language extractors.
//!
//! These patterns are language-agnostic: PII field names, auth middleware
//! indicators, and log method names are naming conventions that transcend
//! any specific programming language.

/// PII field names matched as substrings in log arguments.
///
/// Any log sink containing one of these substrings (case-insensitive)
/// is flagged as potentially leaking personally identifiable information.
pub const PII_PATTERNS: &[&str] = &[
    "email",
    "password",
    "passwd",
    "secret",
    "ssn",
    "social_security",
    "credit_card",
    "creditcard",
    "card_number",
    "cardnumber",
    "phone",
    "address",
    "date_of_birth",
    "dateofbirth",
    "dob",
    "token",
    "api_key",
    "apikey",
];

/// Names that indicate auth middleware or decorators.
///
/// Used to detect whether a route/endpoint has authentication applied.
/// Matched case-insensitively as substring (e.g., "verifyJwtToken" matches "jwt").
pub const AUTH_INDICATORS: &[&str] = &["auth", "jwt", "protect", "verify", "guard", "login"];

/// Object/module names that indicate logging across languages.
///
/// Covers: JS (console, winston, pino), Python (logging, logger),
/// Java (Logger, log), Go (log, slog), Ruby (Logger), etc.
pub const LOG_OBJECTS: &[&str] = &[
    "console", // JS/TS
    "logger",  // Universal
    "log",     // Universal
    "winston", // JS
    "pino",    // JS
    "logging", // Python
    "slog",    // Go
    "Logger",  // Java/Ruby/C#
    "Log",     // C#/Kotlin
];

/// Method names that indicate a log call.
pub const LOG_METHODS: &[&str] = &[
    "log",
    "info",
    "warn",
    "error",
    "debug",
    "trace",
    "fatal",
    // Python
    "warning",
    "critical",
    "exception",
    // Go (capitalized methods used by log, slog, and popular loggers)
    "Println",
    "Printf",
    "Print",
    "Fatalf",
    "Panicf",
    "Info",
    "Warn",
    "Error",
    "Debug",
    "Fatal",
    // Java
    "severe",
    // Ruby
    "puts",
    // C# ASP.NET Core ILogger<T>
    "LogInformation",
    "LogWarning",
    "LogError",
    "LogDebug",
    "LogTrace",
    "LogCritical",
];

/// HTTP method names recognized as route registrations.
///
/// Used by framework-specific extractors to identify endpoint definitions
/// like `app.get(...)`, `router.post(...)`, etc.
pub const ROUTE_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "all",
    // Go/Java capitalized variants
    "Get", "Post", "Put", "Patch", "Delete", "Options", "Head",
    // Python uppercase decorators
    "GET", "POST", "PUT", "PATCH", "DELETE",
];

/// Detect PII field names in a text string (case-insensitive).
pub fn contains_pii(text: &str) -> bool {
    let text_lower = text.to_lowercase();
    PII_PATTERNS
        .iter()
        .any(|pattern| text_lower.contains(pattern))
}

/// Check if a name indicates auth middleware (case-insensitive).
pub fn is_auth_indicator(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    AUTH_INDICATORS.iter().any(|ind| name_lower.contains(ind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pii_in_various_forms() {
        assert!(contains_pii("user.email"));
        assert!(contains_pii("req.body.password"));
        assert!(contains_pii("customer.creditCard"));
        assert!(contains_pii("SSN_NUMBER"));
        assert!(!contains_pii("user.name"));
        assert!(!contains_pii("count"));
    }

    #[test]
    fn detects_auth_indicators() {
        assert!(is_auth_indicator("authMiddleware"));
        assert!(is_auth_indicator("verifyJwtToken"));
        assert!(is_auth_indicator("protectRoute"));
        assert!(is_auth_indicator("guardAdmin"));
        assert!(!is_auth_indicator("handleRequest"));
        assert!(!is_auth_indicator("middleware"));
    }
}
