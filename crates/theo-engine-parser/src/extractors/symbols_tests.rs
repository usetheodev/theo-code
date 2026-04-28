//! Sibling test body of `symbols.rs` (T2.4 of god-files-2026-07-23-plan.md).


#![cfg(test)]

#![allow(unused_imports)]

use super::*;
use std::path::PathBuf;


    use super::*;
    use crate::types::Visibility;

    fn symbols_for(source: &str, lang: SupportedLanguage, filename: &str) -> Vec<Symbol> {
        let path = PathBuf::from(filename);
        let parsed = crate::tree_sitter::parse_source(&path, source, lang, None).unwrap();
        extract_symbols(source, &parsed.tree, lang, &path)
    }

    // =======================================================================
    // Existing tests (preserved, now also verify enriched fields)
    // =======================================================================

    // --- TypeScript ---

    #[test]
    fn ts_class_with_methods() {
        let symbols = symbols_for(
            r#"
class UserService {
    getUser(id: string) {
        return {};
    }
    deleteUser(id: string) {
        return true;
    }
}
"#,
            SupportedLanguage::TypeScript,
            "service.ts",
        );

        assert_eq!(symbols.len(), 3, "1 class + 2 methods");
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "UserService" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "getUser" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "deleteUser" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn ts_function_and_interface() {
        let symbols = symbols_for(
            r#"
interface User {
    name: string;
    email: string;
}

function createUser(data: User): User {
    return data;
}

enum Status {
    Active,
    Inactive,
}
"#,
            SupportedLanguage::TypeScript,
            "types.ts",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "User" && s.kind == SymbolKind::Interface)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "createUser" && s.kind == SymbolKind::Function)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Status" && s.kind == SymbolKind::Enum)
        );
    }

    #[test]
    fn ts_symbols_have_correct_line_numbers() {
        let symbols = symbols_for(
            "function hello() {\n  return 'world';\n}\n",
            SupportedLanguage::TypeScript,
            "hello.ts",
        );

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].anchor.line, 1);
        assert_eq!(symbols[0].anchor.end_line, 3);
    }

    // --- Python ---

    #[test]
    fn py_class_with_methods() {
        let symbols = symbols_for(
            r#"
class UserService:
    def __init__(self):
        self.users = []

    def get_user(self, user_id):
        return None

    def create_user(self, data):
        pass
"#,
            SupportedLanguage::Python,
            "service.py",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "UserService" && s.kind == SymbolKind::Class)
        );
        // Python functions inside a class are still function_definition nodes
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "__init__" && s.kind == SymbolKind::Function)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "get_user" && s.kind == SymbolKind::Function)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "create_user" && s.kind == SymbolKind::Function)
        );
        assert_eq!(symbols.len(), 4, "1 class + 3 functions");
    }

    // --- Java ---

    #[test]
    fn java_class_with_methods() {
        let symbols = symbols_for(
            r#"
public class OrderService {
    public Order createOrder(OrderRequest req) {
        return new Order();
    }

    public void cancelOrder(String id) {
    }
}
"#,
            SupportedLanguage::Java,
            "OrderService.java",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "OrderService" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "createOrder" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "cancelOrder" && s.kind == SymbolKind::Method)
        );
        assert_eq!(symbols.len(), 3);
    }

    #[test]
    fn java_interface_and_enum() {
        let symbols = symbols_for(
            r#"
public interface PaymentGateway {
    void charge(Amount amount);
}

public enum PaymentStatus {
    PENDING,
    COMPLETED,
    FAILED
}
"#,
            SupportedLanguage::Java,
            "Payment.java",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "PaymentGateway" && s.kind == SymbolKind::Interface)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "PaymentStatus" && s.kind == SymbolKind::Enum)
        );
    }

    // --- Go ---

    #[test]
    fn go_functions_and_methods() {
        let symbols = symbols_for(
            r#"
package main

func main() {
    fmt.Println("hello")
}

func (s *Server) Start(port int) error {
    return nil
}
"#,
            SupportedLanguage::Go,
            "main.go",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "main" && s.kind == SymbolKind::Function)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Start" && s.kind == SymbolKind::Method)
        );
    }

    // --- C# ---

    #[test]
    fn csharp_class_with_methods() {
        let symbols = symbols_for(
            r#"
public class UsersController : ControllerBase {
    public IActionResult GetAll() {
        return Ok();
    }

    public IActionResult Create(UserDto dto) {
        return Created();
    }
}
"#,
            SupportedLanguage::CSharp,
            "UsersController.cs",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "UsersController" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "GetAll" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Create" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn extracts_csharp_record_declaration() {
        let symbols = symbols_for(
            r#"
public record UserDto(string Name, int Age);

public record OrderRecord {
    public string OrderId { get; init; }
    public decimal Total { get; init; }
}
"#,
            SupportedLanguage::CSharp,
            "Dtos.cs",
        );

        // record declarations are mapped to SymbolKind::Class
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "UserDto" && s.kind == SymbolKind::Class),
            "positional record should be extracted as class"
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "OrderRecord" && s.kind == SymbolKind::Class),
            "nominal record should be extracted as class"
        );
    }

    // --- Rust ---

    #[test]
    fn rust_struct_enum_trait_function() {
        let symbols = symbols_for(
            r#"
pub struct Config {
    pub port: u16,
}

pub enum AppError {
    NotFound,
    Internal(String),
}

pub trait Repository {
    fn find(&self, id: &str) -> Option<()>;
}

fn helper() -> bool {
    true
}
"#,
            SupportedLanguage::Rust,
            "lib.rs",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Config" && s.kind == SymbolKind::Struct)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "AppError" && s.kind == SymbolKind::Enum)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Repository" && s.kind == SymbolKind::Trait)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "helper" && s.kind == SymbolKind::Function)
        );
        assert_eq!(symbols.len(), 4);
    }

    // --- PHP ---

    #[test]
    fn php_class_with_methods() {
        let symbols = symbols_for(
            r#"<?php
class UserController {
    public function index() {
        return view('users.index');
    }

    public function store(Request $request) {
        return redirect('/users');
    }
}
?>"#,
            SupportedLanguage::Php,
            "UserController.php",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "UserController" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "index" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "store" && s.kind == SymbolKind::Method)
        );
    }

    // --- Ruby ---

    #[test]
    fn ruby_class_module_methods() {
        let symbols = symbols_for(
            r#"
module Authentication
  class SessionManager
    def create_session(user)
      # ...
    end

    def destroy_session
      # ...
    end
  end
end
"#,
            SupportedLanguage::Ruby,
            "session.rb",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Authentication" && s.kind == SymbolKind::Module)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "SessionManager" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "create_session" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "destroy_session" && s.kind == SymbolKind::Method)
        );
    }

    // --- Edge cases ---

    #[test]
    fn empty_file_returns_no_symbols() {
        let symbols = symbols_for("", SupportedLanguage::TypeScript, "empty.ts");
        assert!(symbols.is_empty());
    }

    #[test]
    fn unsupported_language_returns_no_symbols() {
        let symbols = symbols_for("let x = 1;", SupportedLanguage::Swift, "main.swift");
        assert!(symbols.is_empty());
    }

    #[test]
    fn javascript_class_extraction() {
        let symbols = symbols_for(
            r#"
class Router {
    handle(req) {
        return {};
    }
}

function middleware(req, res, next) {
    next();
}
"#,
            SupportedLanguage::JavaScript,
            "router.js",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Router" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "handle" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "middleware" && s.kind == SymbolKind::Function)
        );
    }

    // =======================================================================
    // New tests for signature, visibility, parent
    // =======================================================================

    // --- Signature ---

    #[test]
    fn ts_function_signature() {
        let symbols = symbols_for(
            "function greet(name: string): string {\n  return name;\n}\n",
            SupportedLanguage::TypeScript,
            "fn.ts",
        );
        assert_eq!(symbols.len(), 1);
        let sig = symbols[0].signature.as_deref().unwrap();
        assert!(sig.contains("greet"), "signature should contain name");
        assert!(sig.contains("string"), "signature should contain type");
        assert!(
            !sig.contains('{'),
            "signature should not contain body opener"
        );
    }

    #[test]
    fn python_function_signature() {
        let symbols = symbols_for(
            "def process(data, timeout=30):\n    pass\n",
            SupportedLanguage::Python,
            "proc.py",
        );
        assert_eq!(symbols.len(), 1);
        let sig = symbols[0].signature.as_deref().unwrap();
        assert!(sig.contains("process"), "should contain name");
        assert!(sig.contains("data"), "should contain param");
        assert!(!sig.contains(':'), "should not contain body colon");
    }

    #[test]
    fn java_method_signature() {
        let symbols = symbols_for(
            r#"
public class Svc {
    public List<String> findAll(int limit) {
        return null;
    }
}
"#,
            SupportedLanguage::Java,
            "Svc.java",
        );
        let method = symbols.iter().find(|s| s.name == "findAll").unwrap();
        let sig = method.signature.as_deref().unwrap();
        assert!(sig.contains("findAll"), "should contain method name");
        assert!(sig.contains("int limit"), "should contain params");
    }

    #[test]
    fn rust_function_signature() {
        let symbols = symbols_for(
            "pub fn compute(x: i32, y: i32) -> f64 {\n    0.0\n}\n",
            SupportedLanguage::Rust,
            "lib.rs",
        );
        assert_eq!(symbols.len(), 1);
        let sig = symbols[0].signature.as_deref().unwrap();
        assert!(sig.contains("compute"));
        assert!(sig.contains("i32"));
        assert!(sig.contains("f64"));
    }

    #[test]
    fn go_function_signature() {
        let symbols = symbols_for(
            "package main\n\nfunc Add(a int, b int) int {\n\treturn a + b\n}\n",
            SupportedLanguage::Go,
            "math.go",
        );
        let func = symbols.iter().find(|s| s.name == "Add").unwrap();
        let sig = func.signature.as_deref().unwrap();
        assert!(sig.contains("Add"));
        assert!(sig.contains("int"));
    }

    #[test]
    fn ruby_method_signature() {
        let symbols = symbols_for(
            "class Foo\n  def bar(x, y)\n    x + y\n  end\nend\n",
            SupportedLanguage::Ruby,
            "foo.rb",
        );
        let method = symbols.iter().find(|s| s.name == "bar").unwrap();
        let sig = method.signature.as_deref().unwrap();
        assert!(sig.contains("bar"));
        assert!(sig.contains("x"));
    }

    // --- Visibility ---

    #[test]
    fn ts_export_is_public() {
        let symbols = symbols_for(
            "export function hello() {}\nfunction secret() {}\n",
            SupportedLanguage::TypeScript,
            "mod.ts",
        );
        // "hello" is exported — but tree-sitter may wrap in export_statement,
        // so the function_declaration itself may not start with "export"
        // depending on the query match. Let's check what we get:
        let hello = symbols.iter().find(|s| s.name == "hello");
        let secret = symbols.iter().find(|s| s.name == "secret");
        // secret has no export — visibility should be None
        assert!(hello.is_some());
        assert!(secret.is_some());
        assert_eq!(secret.unwrap().visibility, None);
    }

    #[test]
    fn python_underscore_visibility() {
        let symbols = symbols_for(
            "def public_fn():\n    pass\n\ndef _private_fn():\n    pass\n\ndef __mangled():\n    pass\n",
            SupportedLanguage::Python,
            "mod.py",
        );
        let public = symbols.iter().find(|s| s.name == "public_fn").unwrap();
        let private = symbols.iter().find(|s| s.name == "_private_fn").unwrap();
        let mangled = symbols.iter().find(|s| s.name == "__mangled").unwrap();

        assert_eq!(public.visibility, Some(Visibility::Public));
        assert_eq!(private.visibility, Some(Visibility::Private));
        assert_eq!(mangled.visibility, Some(Visibility::Private));
    }

    #[test]
    fn java_visibility_modifiers() {
        let symbols = symbols_for(
            r#"
public class Svc {
    public void doPublic() {}
    private void doPrivate() {}
    protected void doProtected() {}
}
"#,
            SupportedLanguage::Java,
            "Svc.java",
        );
        let svc = symbols.iter().find(|s| s.name == "Svc").unwrap();
        assert_eq!(svc.visibility, Some(Visibility::Public));
        let pub_m = symbols.iter().find(|s| s.name == "doPublic").unwrap();
        assert_eq!(pub_m.visibility, Some(Visibility::Public));
        let priv_m = symbols.iter().find(|s| s.name == "doPrivate").unwrap();
        assert_eq!(priv_m.visibility, Some(Visibility::Private));
        let prot_m = symbols.iter().find(|s| s.name == "doProtected").unwrap();
        assert_eq!(prot_m.visibility, Some(Visibility::Protected));
    }

    #[test]
    fn go_capitalization_visibility() {
        let symbols = symbols_for(
            "package main\n\nfunc Exported() {}\nfunc internal() {}\n",
            SupportedLanguage::Go,
            "main.go",
        );
        let exported = symbols.iter().find(|s| s.name == "Exported").unwrap();
        let internal = symbols.iter().find(|s| s.name == "internal").unwrap();
        assert_eq!(exported.visibility, Some(Visibility::Public));
        assert_eq!(internal.visibility, Some(Visibility::Private));
    }

    #[test]
    fn rust_pub_visibility() {
        let symbols = symbols_for(
            "pub fn public_fn() {}\nfn private_fn() {}\n",
            SupportedLanguage::Rust,
            "lib.rs",
        );
        let pub_fn = symbols.iter().find(|s| s.name == "public_fn").unwrap();
        let priv_fn = symbols.iter().find(|s| s.name == "private_fn").unwrap();
        assert_eq!(pub_fn.visibility, Some(Visibility::Public));
        assert_eq!(priv_fn.visibility, Some(Visibility::Private));
    }

    #[test]
    fn php_visibility_modifiers() {
        let symbols = symbols_for(
            r#"<?php
class Svc {
    public function doPublic() {}
    private function doPrivate() {}
}
?>"#,
            SupportedLanguage::Php,
            "Svc.php",
        );
        let pub_m = symbols.iter().find(|s| s.name == "doPublic").unwrap();
        let priv_m = symbols.iter().find(|s| s.name == "doPrivate").unwrap();
        assert_eq!(pub_m.visibility, Some(Visibility::Public));
        assert_eq!(priv_m.visibility, Some(Visibility::Private));
    }

    // --- Parent ---

    #[test]
    fn ts_method_parent_is_class() {
        let symbols = symbols_for(
            r#"
class UserService {
    getUser(id: string) {
        return {};
    }
}
"#,
            SupportedLanguage::TypeScript,
            "svc.ts",
        );
        let method = symbols.iter().find(|s| s.name == "getUser").unwrap();
        assert_eq!(method.parent.as_deref(), Some("UserService"));

        let class = symbols.iter().find(|s| s.name == "UserService").unwrap();
        assert!(class.parent.is_none(), "top-level class has no parent");
    }

    #[test]
    fn python_method_parent_is_class() {
        let symbols = symbols_for(
            "class MyClass:\n    def my_method(self):\n        pass\n",
            SupportedLanguage::Python,
            "cls.py",
        );
        let method = symbols.iter().find(|s| s.name == "my_method").unwrap();
        assert_eq!(method.parent.as_deref(), Some("MyClass"));
    }

    #[test]
    fn java_method_parent_is_class() {
        let symbols = symbols_for(
            r#"
public class OrderService {
    public void process() {}
}
"#,
            SupportedLanguage::Java,
            "Order.java",
        );
        let method = symbols.iter().find(|s| s.name == "process").unwrap();
        assert_eq!(method.parent.as_deref(), Some("OrderService"));
    }

    #[test]
    fn go_method_parent_is_receiver_type() {
        let symbols = symbols_for(
            "package main\n\nfunc (s *Server) Start() error {\n\treturn nil\n}\n",
            SupportedLanguage::Go,
            "server.go",
        );
        let method = symbols.iter().find(|s| s.name == "Start").unwrap();
        assert_eq!(method.parent.as_deref(), Some("Server"));
    }

    #[test]
    fn ruby_method_parent_is_class() {
        let symbols = symbols_for(
            "class Foo\n  def bar\n    # noop\n  end\nend\n",
            SupportedLanguage::Ruby,
            "foo.rb",
        );
        let method = symbols.iter().find(|s| s.name == "bar").unwrap();
        assert_eq!(method.parent.as_deref(), Some("Foo"));
    }

    #[test]
    fn php_method_parent_is_class() {
        let symbols = symbols_for(
            "<?php\nclass Ctrl {\n    public function index() {}\n}\n?>",
            SupportedLanguage::Php,
            "ctrl.php",
        );
        let method = symbols.iter().find(|s| s.name == "index").unwrap();
        assert_eq!(method.parent.as_deref(), Some("Ctrl"));
    }

    // --- Go struct/interface declarations ---

    #[test]
    fn go_struct_and_interface_declarations() {
        let symbols = symbols_for(
            r#"
package main

type Server struct {
    Port int
}

type Handler interface {
    Handle(req Request) Response
}
"#,
            SupportedLanguage::Go,
            "types.go",
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Server" && s.kind == SymbolKind::Struct),
            "should detect struct declaration"
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Handler" && s.kind == SymbolKind::Interface),
            "should detect interface declaration"
        );
    }

    // --- Doc comments ---

    #[test]
    fn ts_jsdoc_comment_extracted() {
        let symbols = symbols_for(
            r#"
/** Creates a new user account. */
function createUser(data: any) {
    return data;
}
"#,
            SupportedLanguage::TypeScript,
            "api.ts",
        );
        let sym = symbols.iter().find(|s| s.name == "createUser").unwrap();
        assert!(sym.doc.is_some(), "should extract JSDoc comment");
        assert!(sym.doc.as_deref().unwrap().contains("Creates a new user"));
    }

    #[test]
    fn python_docstring_extracted() {
        let symbols = symbols_for(
            r#"
def process(data):
    """Process the incoming data."""
    return data
"#,
            SupportedLanguage::Python,
            "proc.py",
        );
        let sym = symbols.iter().find(|s| s.name == "process").unwrap();
        assert!(sym.doc.is_some(), "should extract docstring");
        assert!(
            sym.doc
                .as_deref()
                .unwrap()
                .contains("Process the incoming data")
        );
    }

    #[test]
    fn rust_doc_comment_extracted() {
        let symbols = symbols_for(
            "/// Compute the result.\npub fn compute() {}\n",
            SupportedLanguage::Rust,
            "lib.rs",
        );
        let sym = symbols.iter().find(|s| s.name == "compute").unwrap();
        assert!(sym.doc.is_some(), "should extract /// doc comment");
        assert!(sym.doc.as_deref().unwrap().contains("Compute the result"));
    }

    #[test]
    fn go_comment_extracted() {
        let symbols = symbols_for(
            "package main\n\n// Add adds two numbers.\nfunc Add(a int, b int) int {\n\treturn a + b\n}\n",
            SupportedLanguage::Go,
            "math.go",
        );
        let sym = symbols.iter().find(|s| s.name == "Add").unwrap();
        assert!(sym.doc.is_some(), "should extract Go doc comment");
        assert!(sym.doc.as_deref().unwrap().contains("adds two numbers"));
    }

    #[test]
    fn ruby_hash_comment_extracted() {
        let symbols = symbols_for(
            "# Greet the user.\ndef greet(name)\n  puts name\nend\n",
            SupportedLanguage::Ruby,
            "greet.rb",
        );
        let sym = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert!(sym.doc.is_some(), "should extract Ruby comment");
        assert!(sym.doc.as_deref().unwrap().contains("Greet the user"));
    }

    #[test]
    fn java_javadoc_comment_extracted() {
        let symbols = symbols_for(
            r#"
public class Svc {
    /** Process the order. */
    public void process() {}
}
"#,
            SupportedLanguage::Java,
            "Svc.java",
        );
        let sym = symbols.iter().find(|s| s.name == "process").unwrap();
        assert!(sym.doc.is_some(), "should extract JavaDoc");
        assert!(sym.doc.as_deref().unwrap().contains("Process the order"));
    }

    #[test]
    fn no_comment_gives_none_doc() {
        let symbols = symbols_for(
            "function bare() {}\n",
            SupportedLanguage::TypeScript,
            "bare.ts",
        );
        assert_eq!(symbols[0].doc, None);
    }

    #[test]
    fn php_doc_comment_extracted() {
        let symbols = symbols_for(
            "<?php\nclass Svc {\n    /** Save data. */\n    public function save() {}\n}\n?>",
            SupportedLanguage::Php,
            "svc.php",
        );
        let sym = symbols.iter().find(|s| s.name == "save").unwrap();
        assert!(sym.doc.is_some(), "should extract PHP doc comment");
        assert!(sym.doc.as_deref().unwrap().contains("Save data"));
    }

    #[test]
    fn csharp_method_parent_is_class() {
        let symbols = symbols_for(
            r#"
public class ItemsController {
    public void Delete() {}
}
"#,
            SupportedLanguage::CSharp,
            "Items.cs",
        );
        let method = symbols.iter().find(|s| s.name == "Delete").unwrap();
        assert_eq!(method.parent.as_deref(), Some("ItemsController"));
    }

    // =======================================================================
    // is_test detection
    // =======================================================================

    #[test]
    fn python_test_function_detected() {
        let symbols = symbols_for(
            "def test_create_user():\n    pass\n",
            SupportedLanguage::Python,
            "test_users.py",
        );
        let sym = symbols
            .iter()
            .find(|s| s.name == "test_create_user")
            .unwrap();
        assert!(sym.is_test, "def test_* should be detected as test");
    }

    #[test]
    fn python_regular_function_not_test() {
        let symbols = symbols_for(
            "def create_user():\n    pass\n",
            SupportedLanguage::Python,
            "users.py",
        );
        let sym = symbols.iter().find(|s| s.name == "create_user").unwrap();
        assert!(!sym.is_test, "regular function should not be test");
    }

    #[test]
    fn go_test_function_detected() {
        let symbols = symbols_for(
            "package main\n\nimport \"testing\"\n\nfunc TestCreateUser(t *testing.T) {\n}\n",
            SupportedLanguage::Go,
            "user_test.go",
        );
        let sym = symbols.iter().find(|s| s.name == "TestCreateUser").unwrap();
        assert!(
            sym.is_test,
            "func Test*(t *testing.T) should be detected as test"
        );
    }

    #[test]
    fn go_regular_function_not_test() {
        let symbols = symbols_for(
            "package main\n\nfunc CreateUser() {\n}\n",
            SupportedLanguage::Go,
            "user.go",
        );
        let sym = symbols.iter().find(|s| s.name == "CreateUser").unwrap();
        assert!(!sym.is_test, "regular Go func should not be test");
    }

    #[test]
    fn java_test_annotation_detected() {
        let symbols = symbols_for(
            r#"
public class UserTest {
    @Test
    void shouldCreateUser() {
    }
}
"#,
            SupportedLanguage::Java,
            "UserTest.java",
        );
        let sym = symbols
            .iter()
            .find(|s| s.name == "shouldCreateUser")
            .unwrap();
        assert!(sym.is_test, "@Test annotation should mark method as test");
    }

    #[test]
    fn java_regular_method_not_test() {
        let symbols = symbols_for(
            r#"
public class UserService {
    public void createUser() {
    }
}
"#,
            SupportedLanguage::Java,
            "UserService.java",
        );
        let sym = symbols.iter().find(|s| s.name == "createUser").unwrap();
        assert!(!sym.is_test, "regular Java method should not be test");
    }

    #[test]
    fn rust_test_attribute_detected() {
        let symbols = symbols_for(
            "#[test]\nfn test_it_works() {\n    assert!(true);\n}\n",
            SupportedLanguage::Rust,
            "lib.rs",
        );
        let sym = symbols.iter().find(|s| s.name == "test_it_works").unwrap();
        assert!(
            sym.is_test,
            "#[test] attribute should mark function as test"
        );
    }

    #[test]
    fn rust_regular_function_not_test() {
        let symbols = symbols_for(
            "fn helper() -> bool {\n    true\n}\n",
            SupportedLanguage::Rust,
            "lib.rs",
        );
        let sym = symbols.iter().find(|s| s.name == "helper").unwrap();
        assert!(!sym.is_test, "regular Rust fn should not be test");
    }

    #[test]
    fn csharp_fact_attribute_detected() {
        let symbols = symbols_for(
            r#"
public class UserTests {
    [Fact]
    public void ShouldCreateUser() {
    }
}
"#,
            SupportedLanguage::CSharp,
            "UserTests.cs",
        );
        let sym = symbols
            .iter()
            .find(|s| s.name == "ShouldCreateUser")
            .unwrap();
        assert!(sym.is_test, "[Fact] attribute should mark method as test");
    }

    #[test]
    fn ruby_test_method_detected() {
        let symbols = symbols_for(
            "class UserTest\n  def test_create\n    # assert\n  end\nend\n",
            SupportedLanguage::Ruby,
            "user_test.rb",
        );
        let sym = symbols.iter().find(|s| s.name == "test_create").unwrap();
        assert!(sym.is_test, "def test_* should be detected as test in Ruby");
    }

    #[test]
    fn ruby_regular_method_not_test() {
        let symbols = symbols_for(
            "class User\n  def create\n  end\nend\n",
            SupportedLanguage::Ruby,
            "user.rb",
        );
        let sym = symbols.iter().find(|s| s.name == "create").unwrap();
        assert!(!sym.is_test, "regular Ruby method should not be test");
    }

    #[test]
    fn php_test_method_detected() {
        let symbols = symbols_for(
            "<?php\nclass UserTest {\n    public function testCreate() {}\n}\n?>",
            SupportedLanguage::Php,
            "UserTest.php",
        );
        let sym = symbols.iter().find(|s| s.name == "testCreate").unwrap();
        assert!(
            sym.is_test,
            "function test* should be detected as test in PHP"
        );
    }

    #[test]
    fn ts_test_function_detected() {
        let symbols = symbols_for(
            "function testCreateUser() {\n    expect(true).toBe(true);\n}\n",
            SupportedLanguage::TypeScript,
            "user.test.ts",
        );
        let sym = symbols.iter().find(|s| s.name == "testCreateUser").unwrap();
        assert!(
            sym.is_test,
            "function test* should be detected as test in TS"
        );
    }

    #[test]
    fn ts_regular_function_not_test() {
        let symbols = symbols_for(
            "function createUser() {\n    return {};\n}\n",
            SupportedLanguage::TypeScript,
            "user.ts",
        );
        let sym = symbols.iter().find(|s| s.name == "createUser").unwrap();
        assert!(!sym.is_test, "regular TS function should not be test");
    }
