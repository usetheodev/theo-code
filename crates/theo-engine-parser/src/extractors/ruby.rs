//! Ruby semantic extraction from tree-sitter CSTs.
//!
//! Handles the Ruby language family, focusing on Ruby on Rails:
//!
//! Extracts:
//! - Rails route definitions: `get '/path'`, `post '/path'`, `resources :name`
//! - Auth detection: `before_action :authenticate_user!`
//! - HTTP client calls: HTTParty, Faraday, RestClient, Net::HTTP
//! - Log sinks with PII detection

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::patterns;
use crate::tree_sitter::SupportedLanguage;
use crate::types::*;

use super::common::{
    self, anchor_from_node, extract_string_value, node_text, node_text_ref, truncate_call_text,
};

/// Rails route DSL method names.
const RAILS_ROUTE_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// Extract semantic information from a Ruby source file.
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
        "call" | "method_call" => {
            try_extract_route(node, source, file_path, extraction);
            try_extract_before_action(node, source, file_path, extraction);
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

/// Try to extract a Rails route from `get '/path', to: 'controller#action'`.
///
/// Handles:
/// ```ruby
/// get '/users', to: 'users#index'
/// post '/api/orders', to: 'orders#create'
/// resources :products
/// ```
fn try_extract_route(node: &Node, source: &str, file_path: &Path, extraction: &mut FileExtraction) {
    // Get the method name being called
    let method_name = get_call_method_name(node, source);
    let method_name = match method_name {
        Some(n) => n,
        None => return,
    };

    // Check for resources/resource (expands to REST routes)
    if method_name == "resources" || method_name == "resource" {
        try_extract_resources_route(node, source, file_path, extraction);
        return;
    }

    // Check for route methods: get, post, put, patch, delete
    if !RAILS_ROUTE_METHODS.contains(&method_name.as_str()) {
        return;
    }

    let http_method = match common::parse_http_method(&method_name) {
        Some(m) => m,
        None => return,
    };

    // Find the first string argument (the path)
    let route_path = match find_first_string_in_call(node, source) {
        Some(p) => p,
        None => return,
    };

    // Don't match if the string doesn't look like a path
    if !route_path.starts_with('/') {
        return;
    }

    let handler_name = extract_to_action(node, source);

    extraction.interfaces.push(Interface {
        method: http_method,
        path: route_path.clone(),
        auth: None, // Rails auth is at controller level, not route level
        anchor: anchor_from_node(node, file_path),
        parameters: common::extract_path_params(&route_path),
        handler_name,
        request_body_type: None,
    });
}

/// Extract routes from `resources :products`.
fn try_extract_resources_route(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    // Find the symbol argument (e.g., :products)
    let resource_name = match find_first_symbol_in_call(node, source) {
        Some(name) => name,
        None => return,
    };

    // resources :products expands to standard REST routes
    let path = format!("/{resource_name}");

    extraction.interfaces.push(Interface {
        method: HttpMethod::All,
        path: path.clone(),
        auth: None,
        anchor: anchor_from_node(node, file_path),
        parameters: common::extract_path_params(&path),
        handler_name: None,
        request_body_type: None,
    });
}

/// Try to extract auth from `before_action :authenticate_user!`.
fn try_extract_before_action(
    node: &Node,
    source: &str,
    _file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let method_name = match get_call_method_name(node, source) {
        Some(n) => n,
        None => return,
    };

    if method_name != "before_action" && method_name != "before_filter" {
        return;
    }

    // Find the symbol argument (the callback name)
    let callback_name = match find_first_symbol_in_call(node, source) {
        Some(name) => name,
        None => return,
    };

    if patterns::is_auth_indicator(&callback_name) {
        // Mark all routes in this extraction as having auth
        // (controller-level auth applies to all actions)
        for iface in &mut extraction.interfaces {
            if iface.auth.is_none() {
                iface.auth = Some(AuthKind::Middleware(callback_name.clone()));
            }
        }
    }
}

/// Try to extract HTTP client calls.
///
/// Handles: HTTParty, Faraday, RestClient, Net::HTTP
fn try_extract_http_call(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let call_ref = node_text_ref(node, source);
    let call_lower = call_ref.to_lowercase();

    let is_http_call = (call_lower.contains("httparty")
        || call_lower.contains("faraday")
        || call_lower.contains("restclient")
        || call_lower.contains("net::http"))
        && (call_lower.contains(".get(")
            || call_lower.contains(".post(")
            || call_lower.contains(".put(")
            || call_lower.contains(".delete(")
            || call_lower.contains(".patch(")
            || call_lower.contains("::get(")
            || call_lower.contains("::post("));

    if !is_http_call {
        return;
    }

    let display_text = truncate_call_text(call_ref.to_string(), 100);

    extraction.dependencies.push(Dependency {
        target: display_text,
        dependency_type: DependencyType::HttpCall,
        anchor: anchor_from_node(node, file_path),
    });
}

/// Get the method name from a call or method_call node.
fn get_call_method_name(node: &Node, source: &str) -> Option<String> {
    // In Ruby's tree-sitter grammar:
    // `call` has `method` field
    // `method_call` has a method identifier as child
    if let Some(method_node) = node.child_by_field_name("method") {
        return Some(node_text(&method_node, source));
    }

    // Fallback for method_call: first identifier child
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "identifier" {
                return Some(node_text(&child, source));
            }
        }
    }

    None
}

/// Find the first string literal in a call's arguments.
fn find_first_string_in_call(node: &Node, source: &str) -> Option<String> {
    // Walk all descendants looking for string nodes
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "argument_list" || child.kind() == "arguments" {
                return find_first_string_in_node(&child, source);
            }
            // Ruby method calls might have args directly as children
            let text = node_text(&child, source);
            if let Some(val) = extract_string_value(&text) {
                return Some(val);
            }
        }
    }
    None
}

/// Find the first string in a node's children.
fn find_first_string_in_node(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "string" || child.kind() == "string_literal" {
                let text = node_text(&child, source);
                return Some(common::strip_quotes(&text));
            }
            let text = node_text(&child, source);
            if let Some(val) = extract_string_value(&text) {
                return Some(val);
            }
        }
    }
    None
}

/// Find the first symbol (`:name`) in a call's arguments.
fn find_first_symbol_in_call(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "simple_symbol" || child.kind() == "symbol" {
                let text = node_text(&child, source);
                return Some(text.trim_start_matches(':').to_string());
            }
            if child.kind() == "argument_list" || child.kind() == "arguments" {
                if let Some(sym) = find_first_symbol_in_node(&child, source) {
                    return Some(sym);
                }
            }
        }
    }
    None
}

/// Find the first symbol in a node's children.
fn find_first_symbol_in_node(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "simple_symbol" || child.kind() == "symbol" {
                let text = node_text(&child, source);
                return Some(text.trim_start_matches(':').to_string());
            }
        }
    }
    None
}

/// Extract the action name from a Rails `to: 'controller#action'` keyword argument.
///
/// Walks the call node's children looking for a `pair` node where the key is
/// `to` (a hash key or a simple symbol `:to`) and the value is a string like
/// `'users#index'`. Returns the action part (after `#`) if found.
fn extract_to_action(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        let child = node.child(i as u32)?;
        if let Some(action) = find_to_pair_in_subtree(&child, source) {
            return Some(action);
        }
    }
    None
}

/// Recursively search for a `pair` node with key `to` and a string value
/// containing `#`, returning the action portion.
fn find_to_pair_in_subtree(node: &Node, source: &str) -> Option<String> {
    if node.kind() == "pair" {
        // Check if the key is `to:` or `:to`
        if let Some(key_node) = node.child_by_field_name("key") {
            let key_text = node_text_ref(&key_node, source);
            let key_name = key_text.trim_start_matches(':').trim_end_matches(':');
            if key_name == "to" {
                if let Some(val_node) = node.child_by_field_name("value") {
                    let val_text = node_text(&val_node, source);
                    let val_str = common::strip_quotes(&val_text);
                    // Extract the action part after '#'
                    if let Some(hash_pos) = val_str.find('#') {
                        let action = &val_str[hash_pos + 1..];
                        if !action.is_empty() {
                            return Some(action.to_string());
                        }
                    }
                }
            }
        }
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if let Some(action) = find_to_pair_in_subtree(&child, source) {
                return Some(action);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    

    fn extract_rb(source: &str) -> FileExtraction {
        let path = PathBuf::from("routes.rb");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Ruby, None).unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Ruby)
    }

    #[test]
    fn extracts_rails_get_route() {
        let ext = extract_rb(
            r#"
get '/users', to: 'users#index'
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn extracts_rails_post_route() {
        let ext = extract_rb(
            r#"
post '/api/orders', to: 'orders#create'
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/api/orders");
    }

    #[test]
    fn extracts_resources_route() {
        let ext = extract_rb(
            r#"
resources :products
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::All);
        assert_eq!(ext.interfaces[0].path, "/products");
    }

    #[test]
    fn detects_before_action_auth() {
        // before_action is controller-level, applied retroactively to routes
        let ext = extract_rb(
            r#"
get '/api/profile', to: 'profiles#show'
before_action :authenticate_user!
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn extracts_httparty_call() {
        let ext = extract_rb(
            r#"
response = HTTParty.get('https://api.example.com/data')
"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn no_auth_when_missing() {
        let ext = extract_rb(
            r#"
get '/health', to: 'health#show'
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_none());
    }

    #[test]
    fn detects_pii_in_log() {
        let ext = extract_rb(
            r#"
logger.info("User email: #{user.email}")
"#,
        );
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
    }

    #[test]
    fn realistic_rails_routes() {
        let ext = extract_rb(
            r#"
Rails.application.routes.draw do
  get '/health', to: 'health#show'
  post '/api/payments', to: 'payments#create'
  delete '/api/users/:id', to: 'users#destroy'
  resources :products

  logger.info("Routes loaded")
end
"#,
        );
        assert_eq!(ext.interfaces.len(), 4);
        assert!(ext.sinks.len() >= 1);
    }
}
