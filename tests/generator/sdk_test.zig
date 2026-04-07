const std = @import("std");
const sdk = @import("sdk");

test "sdk generator init" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = sdk.parser.OpenAPISpec{
        .openapi = "3.0.0",
        .info = .{ .title = "Test API", .version = "1.0.0" },
        .paths = std.StringHashMap(sdk.parser.OpenAPISpec.PathItem).init(allocator),
    };

    const cfg = sdk.config.Config{
        .project = .{ .name = "test-api", .version = "1.0.0" },
        .targets = &.{},
    };

    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);
    _ = &gen;
}

test "snake_case conversion via public api" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = sdk.parser.OpenAPISpec{
        .openapi = "3.0.0",
        .info = .{ .title = "Test", .version = "1.0.0" },
        .paths = std.StringHashMap(sdk.parser.OpenAPISpec.PathItem).init(allocator),
    };
    const cfg = sdk.config.Config{
        .project = .{ .name = "test", .version = "1.0.0" },
        .targets = &.{},
    };
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    var buf: [256]u8 = undefined;
    const result = gen.toSnakeCase("listPets", &buf);
    try std.testing.expectEqualStrings("list_pets", result);
}

test "pascal_case conversion via public api" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec = sdk.parser.OpenAPISpec{
        .openapi = "3.0.0",
        .info = .{ .title = "Test", .version = "1.0.0" },
        .paths = std.StringHashMap(sdk.parser.OpenAPISpec.PathItem).init(allocator),
    };
    const cfg = sdk.config.Config{
        .project = .{ .name = "test", .version = "1.0.0" },
        .targets = &.{},
    };
    var gen = sdk.SDKGenerator.init(allocator, spec, cfg);

    var buf: [256]u8 = undefined;
    const result = gen.toPascalCase("list_pets", &buf);
    try std.testing.expectEqualStrings("ListPets", result);
}
