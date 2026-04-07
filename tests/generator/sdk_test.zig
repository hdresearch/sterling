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
