//! Sibling test body of `csharp.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `csharp.rs` via `#[path = "csharp_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use std::path::PathBuf;

    use super::*;
    

    fn extract_cs(source: &str) -> FileExtraction {
        let path = PathBuf::from("Controller.cs");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::CSharp, None)
                .unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::CSharp)
    }

    #[test]
    fn extracts_http_get_route() {
        let ext = extract_cs(
            r#"
public class UsersController : ControllerBase {
    [HttpGet("users")]
    public IActionResult GetUsers() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn extracts_http_post_with_path() {
        let ext = extract_cs(
            r#"
public class OrdersController : ControllerBase {
    [HttpPost("api/orders")]
    public IActionResult CreateOrder([FromBody] OrderDto dto) {
        return Created();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/api/orders");
    }

    #[test]
    fn detects_authorize_attribute() {
        let ext = extract_cs(
            r#"
public class SecureController : ControllerBase {
    [HttpGet("api/secure")]
    [Authorize]
    public IActionResult GetSecure() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Attribute("Authorize".into()))
        );
    }

    #[test]
    fn detects_authorize_with_roles() {
        let ext = extract_cs(
            r#"
public class AdminController : ControllerBase {
    [HttpDelete("api/items/{id}")]
    [Authorize(Roles = "Admin")]
    public IActionResult DeleteItem(int id) {
        return NoContent();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn no_auth_on_allow_anonymous() {
        let ext = extract_cs(
            r#"
public class PublicController : ControllerBase {
    [HttpGet("health")]
    [AllowAnonymous]
    public IActionResult Health() {
        return Ok("healthy");
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_none());
    }

    #[test]
    fn extracts_http_client_call() {
        let ext = extract_cs(
            r#"
public class PaymentService {
    public async Task<string> Charge() {
        var client = new HttpClient();
        var response = await client.GetAsync("https://payment.api/charge");
        return await response.Content.ReadAsStringAsync();
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
        let ext = extract_cs(
            r#"
public class Handler {
    public void Handle() {
        Logger.info("User email: " + user.email);
    }
}
"#,
        );
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
    }

    // --- Class-level [Route] prefix ---

    #[test]
    fn composes_class_route_prefix_with_method() {
        let ext = extract_cs(
            r#"
[Route("api/v1/products")]
public class ProductsController : ControllerBase {
    [HttpGet("")]
    public IActionResult List() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].path, "/api/v1/products");
    }

    #[test]
    fn replaces_controller_token() {
        let ext = extract_cs(
            r#"
[Route("api/[controller]")]
public class ProductsController : ControllerBase {
    [HttpGet("{id}")]
    public IActionResult Get(int id) {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].path, "/api/products/{id}");
    }

    #[test]
    fn class_authorize_applies_to_methods() {
        let ext = extract_cs(
            r#"
[Route("api/[controller]")]
[Authorize]
public class SecureController : ControllerBase {
    [HttpGet("")]
    public IActionResult List() {
        return Ok();
    }

    [HttpPost("")]
    public IActionResult Create() {
        return Ok();
    }

    [HttpGet("public")]
    [AllowAnonymous]
    public IActionResult Public() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        // List and Create inherit class [Authorize]
        assert!(ext.interfaces[0].auth.is_some(), "List has auth");
        assert!(ext.interfaces[1].auth.is_some(), "Create has auth");
        // Public has [AllowAnonymous] which nullifies class auth
        assert!(
            ext.interfaces[2].auth.is_none(),
            "Public has no auth (AllowAnonymous)"
        );
    }

    // --- Minimal API ---

    #[test]
    fn extracts_minimal_api_get_route() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
app.MapGet("/items", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/items");
    }

    #[test]
    fn extracts_minimal_api_post_route() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
app.MapPost("/items", (ItemDto item) => Results.Created());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/items");
    }

    #[test]
    fn detects_minimal_api_require_authorization() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
app.MapGet("/secret", () => Results.Ok()).RequireAuthorization();
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Attribute("RequireAuthorization".into()))
        );
    }

    #[test]
    fn realistic_minimal_api_program() {
        let ext = extract_cs(
            r#"
var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

app.MapGet("/health", () => Results.Ok("healthy"));

app.MapGet("/api/items", () => Results.Ok(new List<Item>()));

app.MapPost("/api/items", (ItemDto item) => {
    Logger.info("Creating item for: " + item.email);
    return Results.Created();
}).RequireAuthorization();

app.MapDelete("/api/items/{id}", (int id) => Results.NoContent())
    .RequireAuthorization();

app.Run();
"#,
        );
        assert_eq!(ext.interfaces.len(), 4, "4 Minimal API routes");
        assert!(ext.interfaces[0].auth.is_none(), "/health has no auth");
        assert!(
            ext.interfaces[1].auth.is_none(),
            "GET /api/items has no auth"
        );
        assert!(ext.interfaces[2].auth.is_some(), "POST /api/items has auth");
        assert!(ext.interfaces[3].auth.is_some(), "DELETE has auth");
        assert!(
            ext.sinks.iter().any(|s| s.contains_pii),
            "PII in log detected"
        );
    }

    #[test]
    fn realistic_aspnet_controller() {
        let ext = extract_cs(
            r#"
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Authorization;

[ApiController]
[Route("api/v1/products")]
public class ProductsController : ControllerBase {

    [HttpGet("")]
    public IActionResult List() {
        Logger.info("Listing products");
        return Ok();
    }

    [HttpPost("")]
    [Authorize(Roles = "Admin")]
    public IActionResult Create([FromBody] ProductDto dto) {
        Logger.info("Creating product for: " + dto.email);
        return Created();
    }

    [HttpDelete("{id}")]
    [Authorize]
    public async Task<IActionResult> Delete(int id) {
        var client = new HttpClient();
        await client.PostAsync("https://audit.service/log", null);
        return NoContent();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        let authed: Vec<_> = ext.interfaces.iter().filter(|i| i.auth.is_some()).collect();
        assert_eq!(authed.len(), 2);
        assert_eq!(ext.dependencies.len(), 1);
        assert!(!ext.sinks.is_empty());
    }

    // --- MapGroup prefix tracking ---

    #[test]
    fn mapgroup_prefix_basic() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
var api = app.MapGroup("/api");
api.MapGet("/items", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/api/items");
    }

    #[test]
    fn mapgroup_prefix_nested() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
var api = app.MapGroup("/api");
var v1 = api.MapGroup("/v1");
v1.MapGet("/items", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/api/v1/items");
    }

    #[test]
    fn mapgroup_with_auth() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
var admin = app.MapGroup("/admin").RequireAuthorization();
admin.MapGet("/users", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].path, "/admin/users");
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Attribute("RequireAuthorization".into())),
            "Route inherits auth from MapGroup"
        );
    }
