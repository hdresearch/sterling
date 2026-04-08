const std = @import("std");
const testing = std.testing;
const openapi = @import("../../src/parser/openapi.zig");

test "parse basic openapi spec" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // Test parsing a simple OpenAPI spec
    const spec_content = 
        \\openapi: 3.0.0
        \\info:
        \\  title: Test API
        \\  version: 1.0.0
        \\paths:
        \\  /pets:
        \\    get:
        \\      operationId: listPets
        \\      responses:
        \\        '200':
        \\          description: OK
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    try testing.expectEqualStrings("Test API", spec.info.title);
    try testing.expectEqualStrings("1.0.0", spec.info.version);
}
