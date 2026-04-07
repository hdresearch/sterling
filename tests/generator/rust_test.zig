const std = @import("std");
const testing = std.testing;
const sdk = @import("sdk");

fn createTestSpec(allocator: std.mem.Allocator) !sdk.parser.OpenAPISpec {
    const content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Pet Store API
        \\  version: 1.0.0
        \\paths:
        \\  /pets:
        \\    get:
        \\      operationId: listPets
        \\      summary: List all pets
        \\      responses:
        \\        200:
        \\          description: A list of pets
        \\    post:
        \\      operationId: createPet
        \\      summary: Create a pet
        \\      responses:
        \\        201:
        \\          description: Pet created
        \\  /pets/{petId}:
        \\    get:
        \\      operationId: showPetById
        \\      summary: Info for a specific pet
        \\      responses:
        \\        200:
        \\          description: Expected response
    ;
    return try sdk.parser.parseOpenAPI(allocator, content);
}

fn createRustConfig(allocator: std.mem.Allocator) !sdk.config.Config {
    const content =
        \\[project]
        \\name = "petstore-sdk"
        \\version = "1.0.0"
        \\
        \\[[targets]]
        \\language = "rust"
        \\output_dir = "./test_rust_output/rust"
        \\
    ;
    return try sdk.config.parseConfig(allocator, content);
}

fn readTestFile(allocator: std.mem.Allocator, path: []const u8) ![]const u8 {
    return std.fs.cwd().readFileAlloc(allocator, path, 1024 * 1024);
}

fn cleanup() void {
    std.fs.cwd().deleteTree("test_rust_output") catch {};
}

test "rust generate creates directory structure" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    try gen.generateRust(cfg.targets[0]);

    // Verify directory structure exists
    const cargo = std.fs.cwd().openFile("test_rust_output/rust/Cargo.toml", .{});
    try testing.expect(cargo != error.FileNotFound);
    if (cargo) |f| f.close() else |_| {}

    const lib = std.fs.cwd().openFile("test_rust_output/rust/src/lib.rs", .{});
    try testing.expect(lib != error.FileNotFound);
    if (lib) |f| f.close() else |_| {}

    const client_file = std.fs.cwd().openFile("test_rust_output/rust/src/client.rs", .{});
    try testing.expect(client_file != error.FileNotFound);
    if (client_file) |f| f.close() else |_| {}

    const models = std.fs.cwd().openFile("test_rust_output/rust/src/models.rs", .{});
    try testing.expect(models != error.FileNotFound);
    if (models) |f| f.close() else |_| {}
}

test "rust cargo toml has correct dependencies" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    try gen.generateRust(cfg.targets[0]);

    const content = try readTestFile(allocator, "test_rust_output/rust/Cargo.toml");

    try testing.expect(std.mem.indexOf(u8, content, "reqwest") != null);
    try testing.expect(std.mem.indexOf(u8, content, "serde") != null);
    try testing.expect(std.mem.indexOf(u8, content, "tokio") != null);
    try testing.expect(std.mem.indexOf(u8, content, "thiserror") != null);
    try testing.expect(std.mem.indexOf(u8, content, "petstore-sdk") != null);
    try testing.expect(std.mem.indexOf(u8, content, "1.0.0") != null);
}

test "rust lib.rs exports modules" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    try gen.generateRust(cfg.targets[0]);

    const content = try readTestFile(allocator, "test_rust_output/rust/src/lib.rs");

    try testing.expect(std.mem.indexOf(u8, content, "pub mod client;") != null);
    try testing.expect(std.mem.indexOf(u8, content, "pub mod models;") != null);
    try testing.expect(std.mem.indexOf(u8, content, "pub use client::Client;") != null);
}

test "rust client.rs has async reqwest client" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    try gen.generateRust(cfg.targets[0]);

    const content = try readTestFile(allocator, "test_rust_output/rust/src/client.rs");

    // Check imports and structure
    try testing.expect(std.mem.indexOf(u8, content, "use reqwest") != null);
    try testing.expect(std.mem.indexOf(u8, content, "use serde") != null);
    try testing.expect(std.mem.indexOf(u8, content, "pub struct ClientConfig") != null);
    try testing.expect(std.mem.indexOf(u8, content, "pub struct Client") != null);
    try testing.expect(std.mem.indexOf(u8, content, "impl Client") != null);
    try testing.expect(std.mem.indexOf(u8, content, "pub fn new") != null);

    // Check authentication support
    try testing.expect(std.mem.indexOf(u8, content, "api_key") != null);
    try testing.expect(std.mem.indexOf(u8, content, "bearer_token") != null);
    try testing.expect(std.mem.indexOf(u8, content, "X-API-Key") != null);
    try testing.expect(std.mem.indexOf(u8, content, "bearer_auth") != null);

    // Check error handling
    try testing.expect(std.mem.indexOf(u8, content, "pub enum Error") != null);
    try testing.expect(std.mem.indexOf(u8, content, "pub type Result<T>") != null);

    // Check HTTP method operations are generated
    try testing.expect(std.mem.indexOf(u8, content, "pub async fn") != null);
    try testing.expect(std.mem.indexOf(u8, content, "reqwest::Method::GET") != null);
}

test "rust client.rs generates methods for operations" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    try gen.generateRust(cfg.targets[0]);

    const content = try readTestFile(allocator, "test_rust_output/rust/src/client.rs");

    // Check that methods are generated for operations
    try testing.expect(std.mem.indexOf(u8, content, "list_pets") != null);
    try testing.expect(std.mem.indexOf(u8, content, "create_pet") != null);
    try testing.expect(std.mem.indexOf(u8, content, "show_pet_by_id") != null);

    // POST method should have body parameter
    try testing.expect(std.mem.indexOf(u8, content, "reqwest::Method::POST") != null);

    // Path parameter method should take path_param
    try testing.expect(std.mem.indexOf(u8, content, "path_param") != null);
}

test "rust models.rs has serde structs" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    try gen.generateRust(cfg.targets[0]);

    const content = try readTestFile(allocator, "test_rust_output/rust/src/models.rs");

    try testing.expect(std.mem.indexOf(u8, content, "use serde") != null);
    try testing.expect(std.mem.indexOf(u8, content, "Serialize") != null);
    try testing.expect(std.mem.indexOf(u8, content, "Deserialize") != null);
    // Check response models generated
    try testing.expect(std.mem.indexOf(u8, content, "Response") != null);
    // POST operations should also have Request models
    try testing.expect(std.mem.indexOf(u8, content, "Request") != null);
}

test "rust snake_case conversion" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    var buf: [256]u8 = undefined;
    const result = gen.toSnakeCase("listPets", &buf);
    try testing.expectEqualStrings("list_pets", result);

    const result2 = gen.toSnakeCase("showPetById", &buf);
    try testing.expectEqualStrings("show_pet_by_id", result2);

    const result3 = gen.toSnakeCase("createPet", &buf);
    try testing.expectEqualStrings("create_pet", result3);
}

test "rust pascal_case conversion" {
    cleanup();
    defer cleanup();

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createRustConfig(allocator);
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    var buf: [256]u8 = undefined;
    const result = gen.toPascalCase("listPets", &buf);
    try testing.expectEqualStrings("ListPets", result);

    const result2 = gen.toPascalCase("create_pet", &buf);
    try testing.expectEqualStrings("CreatePet", result2);
}
