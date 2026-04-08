const std = @import("std");
const testing = std.testing;
const sdk = @import("sdk");

fn createTestSpec(allocator: std.mem.Allocator) !sdk.parser.OpenAPISpec {
    const content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Test API
        \\  version: 1.0.0
        \\paths:
        \\  /pets:
        \\    get:
        \\      operationId: listPets
        \\      summary: List all pets
        \\      responses:
        \\        200:
        \\          description: OK
    ;
    return try sdk.parser.parseOpenAPI(allocator, content);
}

fn createTestConfig(allocator: std.mem.Allocator) !sdk.config.Config {
    const content =
        \\[project]
        \\name = "test-sdk"
        \\version = "0.1.0"
        \\
        \\[[targets]]
        \\language = "typescript"
        \\output_dir = "./test_output/typescript"
        \\
    ;
    return try sdk.config.parseConfig(allocator, content);
}

test "sdk generator init" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = try createTestSpec(allocator);
    const cfg = try createTestConfig(allocator);
    const gen = sdk.SDKGenerator.init(allocator, spec, cfg);
    _ = gen;
}

test "template engine simple variable substitution" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = sdk.template.Context.init(allocator);
    try ctx.putString("name", "World");

    var engine = sdk.template.Engine.init(allocator);
    const result = try engine.render("Hello, {{name}}!", &ctx);
    try testing.expectEqualStrings("Hello, World!", result);
}

test "template engine each block renders items" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = sdk.template.Context.init(allocator);

    var item1 = sdk.template.Context.init(allocator);
    try item1.putString("name", "Alice");
    var item2 = sdk.template.Context.init(allocator);
    try item2.putString("name", "Bob");

    const item1_ptr = try allocator.create(sdk.template.Context);
    item1_ptr.* = item1;
    const item2_ptr = try allocator.create(sdk.template.Context);
    item2_ptr.* = item2;

    const items = try allocator.alloc(*sdk.template.Context, 2);
    items[0] = item1_ptr;
    items[1] = item2_ptr;
    try ctx.putList("people", @ptrCast(items));

    var engine = sdk.template.Engine.init(allocator);
    const result = try engine.render("{{#each people}}Hi {{name}} {{/each}}", &ctx);
    try testing.expectEqualStrings("Hi Alice Hi Bob ", result);
}

test "template engine if block conditional" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = sdk.template.Context.init(allocator);
    try ctx.putBool("show", true);
    try ctx.putBool("hide", false);

    var engine = sdk.template.Engine.init(allocator);
    const r1 = try engine.render("{{#if show}}YES{{/if}}", &ctx);
    try testing.expectEqualStrings("YES", r1);

    const r2 = try engine.render("{{#if hide}}YES{{/if}}", &ctx);
    try testing.expectEqualStrings("", r2);
}

test "template engine unless block" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = sdk.template.Context.init(allocator);
    try ctx.putBool("required", false);

    var engine = sdk.template.Engine.init(allocator);
    const result = try engine.render("name{{#unless required}}?{{/unless}}: string", &ctx);
    try testing.expectEqualStrings("name?: string", result);
}

test "template engine helper functions" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = sdk.template.Context.init(allocator);
    try ctx.putString("op", "listPets");
    try ctx.putString("method", "get");

    var engine = sdk.template.Engine.init(allocator);

    const snake = try engine.render("{{snake_case op}}", &ctx);
    try testing.expectEqualStrings("list_pets", snake);

    const pascal = try engine.render("{{pascal_case op}}", &ctx);
    try testing.expectEqualStrings("ListPets", pascal);

    const upper = try engine.render("{{upper method}}", &ctx);
    try testing.expectEqualStrings("GET", upper);
}
