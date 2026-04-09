const std = @import("std");
const openapi = @import("openapi");

test "parse yaml spec" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    const content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Test API
        \\  version: 1.0.0
        \\paths:
        \\  /pets:
        \\    get:
        \\      operationId: listPets
        \\      responses:
        \\        200:
        \\          description: OK
    ;

    const spec = try openapi.parseOpenAPISpec(a, content);
    try std.testing.expectEqualStrings("Test API", spec.info.title);
    try std.testing.expectEqualStrings("1.0.0", spec.info.version);
}

test "parse json spec" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    const content =
        \\{
        \\  "openapi": "3.0.0",
        \\  "info": {
        \\    "title": "JSON API",
        \\    "version": "2.0.0"
        \\  },
        \\  "paths": {
        \\    "/items": {
        \\      "get": {
        \\        "operationId": "listItems",
        \\        "responses": {
        \\          "200": {
        \\            "description": "Success"
        \\          }
        \\        }
        \\      }
        \\    }
        \\  }
        \\}
    ;

    const spec = try openapi.parseOpenAPISpec(a, content);
    try std.testing.expectEqualStrings("JSON API", spec.info.title);
}

test "missing openapi version errors" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const content =
        \\info:
        \\  title: Test
        \\  version: 1.0.0
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
    ;
    try std.testing.expectError(openapi.ParseError.MissingOpenAPIVersion, openapi.parseOpenAPISpec(arena.allocator(), content));
}
