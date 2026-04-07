const std = @import("std");
const testing = std.testing;
const openapi = @import("openapi");

test "parse basic openapi spec" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

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
        \\        200:
        \\          description: OK
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    try testing.expectEqualStrings("Test API", spec.info.title);
    try testing.expectEqualStrings("1.0.0", spec.info.version);
}

test "parse spec with multiple paths and methods" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec_content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Multi Path API
        \\  version: 2.0.0
        \\  description: A test API with multiple paths
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
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    try testing.expectEqualStrings("Multi Path API", spec.info.title);
    try testing.expectEqualStrings("2.0.0", spec.info.version);
    try testing.expectEqualStrings("A test API with multiple paths", spec.info.description.?);

    const pets_path = spec.paths.get("/pets") orelse return error.TestUnexpectedResult;
    try testing.expect(pets_path.get != null);
    try testing.expect(pets_path.post != null);
    try testing.expectEqualStrings("listPets", pets_path.get.?.operationId.?);
    try testing.expectEqualStrings("createPet", pets_path.post.?.operationId.?);
}

test "parse JSON format openapi spec" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec_content =
        \\{
        \\  "openapi": "3.0.0",
        \\  "info": {
        \\    "title": "JSON Test API",
        \\    "version": "1.0.0"
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

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    try testing.expectEqualStrings("JSON Test API", spec.info.title);
    try testing.expectEqualStrings("1.0.0", spec.info.version);

    const items_path = spec.paths.get("/items") orelse return error.TestUnexpectedResult;
    try testing.expect(items_path.get != null);
    try testing.expectEqualStrings("listItems", items_path.get.?.operationId.?);
}

test "error on missing required fields" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // Missing openapi version
    const no_version =
        \\info:
        \\  title: Test
        \\  version: 1.0.0
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
    ;
    try testing.expectError(openapi.ParseError.MissingOpenAPIVersion, openapi.parseOpenAPISpec(allocator, no_version));

    // Missing info
    const no_info =
        \\openapi: 3.0.0
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
    ;
    try testing.expectError(openapi.ParseError.MissingInfo, openapi.parseOpenAPISpec(allocator, no_info));
}
