//! Java/Kotlin semantic extraction from tree-sitter CSTs.
//!
//! Handles the JVM-like language family (Java, Kotlin, Scala).
//! Covers Spring Boot, the dominant JVM web framework:
//!
//! Extracts:
//! - Spring `@GetMapping`, `@PostMapping`, `@RequestMapping` route definitions
//! - Auth annotations (`@PreAuthorize`, `@Secured`, `@RolesAllowed`)
//! - HTTP client calls (`RestTemplate`, `WebClient`)
//! - Log sinks with PII detection

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::patterns;
use crate::tree_sitter::SupportedLanguage;
use crate::types::*;

use super::common::{self, anchor_from_node, node_text, node_text_ref, truncate_call_text};

/// Spring mapping annotations → HTTP methods.
const MAPPING_ANNOTATIONS: &[(&str, &str)] = &[
    ("GetMapping", "get"),
    ("PostMapping", "post"),
    ("PutMapping", "put"),
    ("PatchMapping", "patch"),
    ("DeleteMapping", "delete"),
];

/// Extract semantic information from a Java or Kotlin source file.
pub fn extract(
    file_path: &Path,
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
) -> FileExtraction {
    let root = tree.root_node();
    let mut extraction = common::new_extraction(file_path, language);

    extract_recursive(&root, source, file_path, &mut extraction);

    extraction
}

fn extract_recursive(node: &Node, source: &str, file_path: &Path, extraction: &mut FileExtraction) {
    match node.kind() {
        // Java method declarations can have annotations
        "method_declaration" => {
            try_extract_annotated_route(node, source, file_path, extraction);
        }
        // Kotlin function declarations
        "function_declaration" => {
            try_extract_annotated_route_kotlin(node, source, file_path, extraction);
        }
        // HTTP calls and log sinks
        "method_invocation" | "call_expression" => {
            try_extract_http_call(node, source, file_path, extraction);
            common::try_extract_log_sink(node, source, file_path, extraction);
        }
        _ => {}
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            extract_recursive(&child, source, file_path, extraction);
        }
    }
}

/// Try to extract a route from an annotated method/function.
///
/// Handles Spring Boot patterns:
/// ```java
/// @GetMapping("/users")
/// public List<User> listUsers() { ... }
///
/// @PostMapping("/api/orders")
/// @PreAuthorize("hasRole('ADMIN')")
/// public Order createOrder(@RequestBody OrderDto dto) { ... }
/// ```
fn try_extract_annotated_route(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let mut route_info: Option<(HttpMethod, String, SourceAnchor)> = None;
    let mut auth: Option<AuthKind> = None;

    // Look for annotations — they appear as siblings before the method declaration,
    // or in Java they can be child nodes depending on grammar version.
    // We check both the node's own annotation children and preceding siblings.
    let annotations = collect_annotations(node, source, file_path);

    for (ann_name, ann_text, ann_anchor) in &annotations {
        // Check for route mapping annotation
        if let Some((method, path)) = try_parse_mapping_annotation(ann_name, ann_text) {
            route_info = Some((method, path, ann_anchor.clone()));
        }

        // Check for auth annotation
        if auth.is_none() && is_auth_annotation(ann_name) {
            auth = Some(AuthKind::Annotation(ann_name.clone()));
        }
    }

    if let Some((method, path, anchor)) = route_info {
        // Extract method name from the method_declaration node
        let handler_name = node
            .child_by_field_name("name")
            .map(|n| node_text(&n, source));

        extraction.interfaces.push(Interface {
            method,
            path: path.clone(),
            auth,
            anchor,
            parameters: common::extract_path_params(&path),
            handler_name,
            request_body_type: None,
        });
    }
}

/// Try to extract a route from a Kotlin annotated function.
///
/// Kotlin's tree-sitter grammar structures annotations differently from Java:
/// the `function_declaration` may have a `modifiers` child containing
/// `annotation` nodes whose text contains the annotation name and arguments.
fn try_extract_annotated_route_kotlin(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let mut route_info: Option<(HttpMethod, String, SourceAnchor)> = None;
    let mut auth: Option<AuthKind> = None;

    // In Kotlin, annotations are in the modifiers or directly as children
    for i in 0..node.child_count() {
        let child = match node.child(i as u32) {
            Some(c) => c,
            None => continue,
        };

        // Collect annotation text from modifiers and direct annotation children
        let annotations = collect_kotlin_annotations(&child, source, file_path);
        for (ann_name, ann_text, ann_anchor) in &annotations {
            if let Some((method, path)) = try_parse_mapping_annotation(ann_name, ann_text) {
                route_info = Some((method, path, ann_anchor.clone()));
            }
            if auth.is_none() && is_auth_annotation(ann_name) {
                auth = Some(AuthKind::Annotation(ann_name.clone()));
            }
        }
    }

    // Also check preceding siblings (annotations might be outside the function node)
    let mut prev = node.prev_named_sibling();
    while let Some(sibling) = prev {
        let annotations = collect_kotlin_annotations(&sibling, source, file_path);
        if annotations.is_empty() {
            break;
        }
        for (ann_name, ann_text, ann_anchor) in &annotations {
            if let Some((method, path)) = try_parse_mapping_annotation(ann_name, ann_text) {
                route_info = Some((method, path, ann_anchor.clone()));
            }
            if auth.is_none() && is_auth_annotation(ann_name) {
                auth = Some(AuthKind::Annotation(ann_name.clone()));
            }
        }
        prev = sibling.prev_named_sibling();
    }

    if let Some((method, path, anchor)) = route_info {
        // Extract function name from the function_declaration node
        let handler_name = node
            .child_by_field_name("name")
            .map(|n| node_text(&n, source));

        extraction.interfaces.push(Interface {
            method,
            path: path.clone(),
            auth,
            anchor,
            parameters: common::extract_path_params(&path),
            handler_name,
            request_body_type: None,
        });
    }
}

/// Collect annotations from a Kotlin node (could be modifiers or annotation).
fn collect_kotlin_annotations(
    node: &Node,
    source: &str,
    file_path: &Path,
) -> Vec<(String, String, SourceAnchor)> {
    let mut result = Vec::new();

    match node.kind() {
        "modifiers" | "modifier_list" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    result.extend(collect_kotlin_annotations(&child, source, file_path));
                }
            }
        }
        "annotation" | "single_annotation" | "multi_annotation" => {
            let text = node_text(node, source);
            // Extract annotation name: strip @, get the identifier part
            if let Some(name) = extract_kotlin_annotation_name(node, source) {
                result.push((name, text, anchor_from_node(node, file_path)));
            }
        }
        _ => {}
    }

    result
}

/// Extract the annotation name from a Kotlin annotation node.
fn extract_kotlin_annotation_name(node: &Node, source: &str) -> Option<String> {
    // Walk children looking for identifiers or user_type nodes
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "user_type" | "constructor_invocation" => {
                    // Get the type identifier
                    for j in 0..child.child_count() {
                        if let Some(type_child) = child.child(j as u32) {
                            if type_child.kind() == "type_identifier"
                                || type_child.kind() == "simple_identifier"
                            {
                                return Some(node_text(&type_child, source));
                            }
                            // Nested in simple_user_type
                            if type_child.kind() == "simple_user_type" {
                                for k in 0..type_child.child_count() {
                                    if let Some(id) = type_child.child(k as u32)
                                        && id.kind() == "simple_identifier" {
                                            return Some(node_text(&id, source));
                                        }
                                }
                            }
                        }
                    }
                }
                "simple_identifier" | "type_identifier" => {
                    return Some(node_text(&child, source));
                }
                _ => {}
            }
        }
    }

    // Fallback: parse from text
    let text = node_text(node, source);
    let trimmed = text.trim_start_matches('@');
    let name = trimmed.split('(').next()?.trim();
    if !name.is_empty() {
        Some(name.to_string())
    } else {
        None
    }
}

/// Collect annotations from a method declaration and its context.
///
/// In Java's tree-sitter grammar, annotations can appear as:
/// - `marker_annotation`: `@GetMapping` (no arguments)
/// - `annotation`: `@GetMapping("/path")` (with arguments)
///
/// These can be children of the method_declaration node or of a
/// `modifiers` child node.
fn collect_annotations(
    node: &Node,
    source: &str,
    file_path: &Path,
) -> Vec<(String, String, SourceAnchor)> {
    let mut result = Vec::new();

    // Check direct children and modifiers children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "marker_annotation" | "annotation" => {
                    if let Some(name) = extract_annotation_name(&child, source) {
                        let text = node_text(&child, source);
                        result.push((name, text, anchor_from_node(&child, file_path)));
                    }
                }
                "modifiers" => {
                    // Annotations can be inside a modifiers node
                    for j in 0..child.child_count() {
                        if let Some(mod_child) = child.child(j as u32)
                            && (mod_child.kind() == "marker_annotation"
                                || mod_child.kind() == "annotation")
                                && let Some(name) = extract_annotation_name(&mod_child, source) {
                                    let text = node_text(&mod_child, source);
                                    result.push((
                                        name,
                                        text,
                                        anchor_from_node(&mod_child, file_path),
                                    ));
                                }
                    }
                }
                _ => {}
            }
        }
    }

    // Also check preceding sibling nodes (some grammars place annotations
    // as siblings rather than children)
    let mut prev = node.prev_named_sibling();
    while let Some(sibling) = prev {
        match sibling.kind() {
            "marker_annotation" | "annotation" => {
                if let Some(name) = extract_annotation_name(&sibling, source) {
                    let text = node_text(&sibling, source);
                    result.push((name, text, anchor_from_node(&sibling, file_path)));
                }
            }
            _ => break,
        }
        prev = sibling.prev_named_sibling();
    }

    result
}

/// Extract the annotation name (e.g., "GetMapping" from `@GetMapping("/path")`).
fn extract_annotation_name(node: &Node, source: &str) -> Option<String> {
    // marker_annotation: @Name
    // annotation: @Name(args)
    // Both have a "name" field in the Java tree-sitter grammar
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(node_text(&name_node, source));
    }

    // Fallback: find the first identifier child after @
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "identifier" {
                return Some(node_text(&child, source));
            }
    }

    None
}

/// Parse a Spring mapping annotation into HTTP method + path.
fn try_parse_mapping_annotation(ann_name: &str, ann_text: &str) -> Option<(HttpMethod, String)> {
    // Check specific mapping annotations: @GetMapping, @PostMapping, etc.
    for (mapping, method_str) in MAPPING_ANNOTATIONS {
        if ann_name == *mapping {
            let method = common::parse_http_method(method_str)?;
            let path = extract_path_from_annotation_text(ann_text).unwrap_or_default();
            return Some((method, path));
        }
    }

    // Check @RequestMapping(value="/path", method=RequestMethod.GET)
    if ann_name == "RequestMapping" {
        let path = extract_path_from_annotation_text(ann_text).unwrap_or_default();
        let method = extract_request_method_from_text(ann_text).unwrap_or(HttpMethod::All);
        return Some((method, path));
    }

    None
}

/// Extract a path string from annotation text like `@GetMapping("/users")`.
fn extract_path_from_annotation_text(ann_text: &str) -> Option<String> {
    // Find the first quoted string in the annotation text
    for quote in ['"', '\''] {
        if let Some(start) = ann_text.find(quote)
            && let Some(end) = ann_text[start + 1..].find(quote) {
                return Some(ann_text[start + 1..start + 1 + end].to_string());
            }
    }
    None
}

/// Extract HTTP method from `@RequestMapping(method = RequestMethod.GET)`.
fn extract_request_method_from_text(text: &str) -> Option<HttpMethod> {
    let text_upper = text.to_uppercase();
    if text_upper.contains("REQUESTMETHOD.GET") || text_upper.contains("METHOD.GET") {
        Some(HttpMethod::Get)
    } else if text_upper.contains("REQUESTMETHOD.POST") || text_upper.contains("METHOD.POST") {
        Some(HttpMethod::Post)
    } else if text_upper.contains("REQUESTMETHOD.PUT") || text_upper.contains("METHOD.PUT") {
        Some(HttpMethod::Put)
    } else if text_upper.contains("REQUESTMETHOD.DELETE") || text_upper.contains("METHOD.DELETE") {
        Some(HttpMethod::Delete)
    } else if text_upper.contains("REQUESTMETHOD.PATCH") || text_upper.contains("METHOD.PATCH") {
        Some(HttpMethod::Patch)
    } else {
        None
    }
}

/// Check if an annotation name indicates auth.
fn is_auth_annotation(name: &str) -> bool {
    let auth_annotations = ["PreAuthorize", "Secured", "RolesAllowed"];
    auth_annotations.contains(&name) || patterns::is_auth_indicator(name)
}

/// Try to extract an HTTP call from RestTemplate/WebClient/HttpClient usage.
fn try_extract_http_call(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let call_ref = node_text_ref(node, source);
    let call_lower = call_ref.to_lowercase();

    // RestTemplate methods
    let is_rest_template = call_lower.contains("resttemplate")
        && (call_lower.contains("getforobject")
            || call_lower.contains("getforentity")
            || call_lower.contains("postforobject")
            || call_lower.contains("postforentity")
            || call_lower.contains("exchange")
            || call_lower.contains("execute"));

    // WebClient methods
    let is_web_client = call_lower.contains("webclient")
        && (call_lower.contains(".get()")
            || call_lower.contains(".post()")
            || call_lower.contains(".put()")
            || call_lower.contains(".delete()"));

    // HttpClient
    let is_http_client = call_lower.contains("httpclient") && call_lower.contains("send");

    if !is_rest_template && !is_web_client && !is_http_client {
        return;
    }

    let display_text = truncate_call_text(call_ref.to_string(), 100);

    extraction.dependencies.push(Dependency {
        target: display_text,
        dependency_type: DependencyType::HttpCall,
        anchor: anchor_from_node(node, file_path),
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    

    fn extract_java(source: &str) -> FileExtraction {
        let path = PathBuf::from("Controller.java");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Java, None).unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Java)
    }

    fn extract_kotlin(source: &str) -> FileExtraction {
        let path = PathBuf::from("Controller.kt");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Kotlin, None)
                .unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Kotlin)
    }

    #[test]
    fn extracts_get_mapping_route() {
        let ext = extract_java(
            r#"
public class UserController {
    @GetMapping("/users")
    public List<User> listUsers() {
        return List.of();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn extracts_post_mapping_route() {
        let ext = extract_java(
            r#"
public class OrderController {
    @PostMapping("/api/orders")
    public Order createOrder(@RequestBody OrderDto dto) {
        return new Order();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/api/orders");
    }

    #[test]
    fn extracts_request_mapping_with_method() {
        let ext = extract_java(
            r#"
public class ItemController {
    @RequestMapping(value = "/items", method = RequestMethod.GET)
    public List<Item> listItems() {
        return List.of();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/items");
    }

    #[test]
    fn detects_pre_authorize_auth() {
        let ext = extract_java(
            r#"
public class AdminController {
    @PostMapping("/api/admin/users")
    @PreAuthorize("hasRole('ADMIN')")
    public void deleteUser(Long id) {}
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Annotation("PreAuthorize".into()))
        );
    }

    #[test]
    fn detects_secured_auth() {
        let ext = extract_java(
            r#"
public class SecureController {
    @GetMapping("/api/secure")
    @Secured("ROLE_USER")
    public String getSecure() {
        return "secure";
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn no_auth_when_absent() {
        let ext = extract_java(
            r#"
public class PublicController {
    @GetMapping("/health")
    public String health() {
        return "ok";
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_none());
    }

    #[test]
    fn extracts_rest_template_http_call() {
        let ext = extract_java(
            r#"
public class PaymentService {
    public void charge() {
        restTemplate.getForObject("https://api.payment.com/charge", String.class);
    }
}
"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn detects_pii_in_log() {
        let ext = extract_java(
            r#"
public class Handler {
    public void handle() {
        Logger.info("User email: " + user.email);
    }
}
"#,
        );
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
    }

    #[test]
    fn kotlin_get_mapping() {
        let ext = extract_kotlin(
            r#"
class UserController {
    @GetMapping("/api/users")
    fun listUsers(): List<User> {
        return emptyList()
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/api/users");
    }

    #[test]
    fn realistic_spring_controller() {
        let ext = extract_java(
            r#"
import org.springframework.web.bind.annotation.*;
import org.springframework.security.access.prepost.PreAuthorize;

@RestController
@RequestMapping("/api/v1")
public class ProductController {

    @GetMapping("/products")
    public List<Product> list() {
        Logger.info("Listing products");
        return productService.findAll();
    }

    @PostMapping("/products")
    @PreAuthorize("hasRole('ADMIN')")
    public Product create(@RequestBody ProductDto dto) {
        Logger.info("Creating product: " + dto.email);
        return productService.create(dto);
    }

    @DeleteMapping("/products/{id}")
    @PreAuthorize("hasRole('ADMIN')")
    public void delete(@PathVariable Long id) {
        restTemplate.getForObject("https://audit.service/log", String.class);
    }
}
"#,
        );
        // 3 method-level routes (@GetMapping, @PostMapping, @DeleteMapping)
        // Class-level @RequestMapping is not on a method — not extracted as a route
        assert_eq!(ext.interfaces.len(), 3);
        let authed: Vec<_> = ext.interfaces.iter().filter(|i| i.auth.is_some()).collect();
        assert_eq!(authed.len(), 2); // @PreAuthorize on create + delete
        assert!(!ext.sinks.is_empty());
    }
}
