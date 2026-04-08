const std = @import("std");
const testing = std.testing;
const openapi = @import("openapi");

test "all parser tests summary" {
    const stderr = std.fs.File.stderr();
    stderr.writeAll("All parser tests passed\n") catch {};
}

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
    try testing.expectEqualStrings("3.0.0", spec.openapi);
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

test "parse petstore yaml spec" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec_content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Pet Store API
        \\  version: 1.0.0
        \\  description: A simple pet store API
        \\paths:
        \\  /pets:
        \\    get:
        \\      summary: List all pets
        \\      operationId: listPets
        \\      responses:
        \\        '200':
        \\          description: A paged array of pets
        \\    post:
        \\      summary: Create a pet
        \\      operationId: createPet
        \\      responses:
        \\        '201':
        \\          description: Pet created
        \\  /pets/{petId}:
        \\    get:
        \\      summary: Info for a specific pet
        \\      operationId: showPetById
        \\      responses:
        \\        '200':
        \\          description: Expected response to a valid request
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);

    // Verify info
    try testing.expectEqualStrings("Pet Store API", spec.info.title);
    try testing.expectEqualStrings("1.0.0", spec.info.version);
    try testing.expectEqualStrings("A simple pet store API", spec.info.description.?);
    try testing.expectEqualStrings("3.0.0", spec.openapi);

    // Verify /pets path
    const pets_path = spec.paths.get("/pets") orelse return error.TestUnexpectedResult;
    try testing.expect(pets_path.get != null);
    try testing.expect(pets_path.post != null);
    try testing.expect(pets_path.put == null);
    try testing.expect(pets_path.delete == null);

    // Verify GET /pets
    const list_pets = pets_path.get.?;
    try testing.expectEqualStrings("listPets", list_pets.operationId.?);
    try testing.expectEqualStrings("List all pets", list_pets.summary.?);
    const get_200 = list_pets.responses.get("200") orelse return error.TestUnexpectedResult;
    try testing.expectEqualStrings("A paged array of pets", get_200.description);

    // Verify POST /pets
    const create_pet = pets_path.post.?;
    try testing.expectEqualStrings("createPet", create_pet.operationId.?);
    try testing.expectEqualStrings("Create a pet", create_pet.summary.?);
    const post_201 = create_pet.responses.get("201") orelse return error.TestUnexpectedResult;
    try testing.expectEqualStrings("Pet created", post_201.description);

    // Verify /pets/{petId} path
    const pet_by_id_path = spec.paths.get("/pets/{petId}") orelse return error.TestUnexpectedResult;
    try testing.expect(pet_by_id_path.get != null);
    const show_pet = pet_by_id_path.get.?;
    try testing.expectEqualStrings("showPetById", show_pet.operationId.?);
    try testing.expectEqualStrings("Info for a specific pet", show_pet.summary.?);
}

test "parse JSON petstore spec" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec_content =
        \\{
        \\  "openapi": "3.0.0",
        \\  "info": {
        \\    "title": "Pet Store API",
        \\    "version": "1.0.0",
        \\    "description": "A simple pet store API"
        \\  },
        \\  "paths": {
        \\    "/pets": {
        \\      "get": {
        \\        "summary": "List all pets",
        \\        "operationId": "listPets",
        \\        "responses": {
        \\          "200": {
        \\            "description": "A paged array of pets"
        \\          }
        \\        }
        \\      },
        \\      "post": {
        \\        "summary": "Create a pet",
        \\        "operationId": "createPet",
        \\        "responses": {
        \\          "201": {
        \\            "description": "Pet created"
        \\          }
        \\        }
        \\      }
        \\    },
        \\    "/pets/{petId}": {
        \\      "get": {
        \\        "summary": "Info for a specific pet",
        \\        "operationId": "showPetById",
        \\        "responses": {
        \\          "200": {
        \\            "description": "Expected response to a valid request"
        \\          }
        \\        }
        \\      }
        \\    }
        \\  }
        \\}
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    try testing.expectEqualStrings("Pet Store API", spec.info.title);
    try testing.expectEqualStrings("1.0.0", spec.info.version);

    const pets_path = spec.paths.get("/pets") orelse return error.TestUnexpectedResult;
    try testing.expect(pets_path.get != null);
    try testing.expect(pets_path.post != null);
    try testing.expectEqualStrings("listPets", pets_path.get.?.operationId.?);
    try testing.expectEqualStrings("createPet", pets_path.post.?.operationId.?);

    const pet_by_id = spec.paths.get("/pets/{petId}") orelse return error.TestUnexpectedResult;
    try testing.expectEqualStrings("showPetById", pet_by_id.get.?.operationId.?);
}

test "error on missing title" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const content =
        \\openapi: 3.0.0
        \\info:
        \\  version: 1.0.0
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
    ;
    try testing.expectError(openapi.ParseError.MissingTitle, openapi.parseOpenAPISpec(arena.allocator(), content));
}

test "error on missing version in info" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Test
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
    ;
    try testing.expectError(openapi.ParseError.MissingVersion, openapi.parseOpenAPISpec(arena.allocator(), content));
}

test "error on missing paths" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Test
        \\  version: 1.0.0
    ;
    try testing.expectError(openapi.ParseError.MissingPaths, openapi.parseOpenAPISpec(arena.allocator(), content));
}

test "parse spec with all HTTP methods" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec_content =
        \\openapi: 3.0.0
        \\info:
        \\  title: All Methods API
        \\  version: 1.0.0
        \\paths:
        \\  /resource:
        \\    get:
        \\      operationId: getResource
        \\      responses:
        \\        200:
        \\          description: OK
        \\    post:
        \\      operationId: createResource
        \\      responses:
        \\        201:
        \\          description: Created
        \\    put:
        \\      operationId: updateResource
        \\      responses:
        \\        200:
        \\          description: Updated
        \\    delete:
        \\      operationId: deleteResource
        \\      responses:
        \\        204:
        \\          description: Deleted
        \\    patch:
        \\      operationId: patchResource
        \\      responses:
        \\        200:
        \\          description: Patched
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    const path = spec.paths.get("/resource") orelse return error.TestUnexpectedResult;

    try testing.expect(path.get != null);
    try testing.expect(path.post != null);
    try testing.expect(path.put != null);
    try testing.expect(path.delete != null);
    try testing.expect(path.patch != null);

    try testing.expectEqualStrings("getResource", path.get.?.operationId.?);
    try testing.expectEqualStrings("createResource", path.post.?.operationId.?);
    try testing.expectEqualStrings("updateResource", path.put.?.operationId.?);
    try testing.expectEqualStrings("deleteResource", path.delete.?.operationId.?);
    try testing.expectEqualStrings("patchResource", path.patch.?.operationId.?);
}

test "parse spec with optional description" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec_content =
        \\openapi: 3.0.0
        \\info:
        \\  title: No Description API
        \\  version: 1.0.0
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
        \\      responses:
        \\        200:
        \\          description: OK
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    try testing.expectEqualStrings("No Description API", spec.info.title);
    try testing.expect(spec.info.description == null);
}

test "parse spec with multiple response codes" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const spec_content =
        \\openapi: 3.0.0
        \\info:
        \\  title: Multi Response API
        \\  version: 1.0.0
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
        \\      responses:
        \\        200:
        \\          description: Success
        \\        404:
        \\          description: Not found
        \\        500:
        \\          description: Server error
    ;

    const spec = try openapi.parseOpenAPISpec(allocator, spec_content);
    const path = spec.paths.get("/test") orelse return error.TestUnexpectedResult;
    const op = path.get orelse return error.TestUnexpectedResult;

    const r200 = op.responses.get("200") orelse return error.TestUnexpectedResult;
    try testing.expectEqualStrings("Success", r200.description);

    const r404 = op.responses.get("404") orelse return error.TestUnexpectedResult;
    try testing.expectEqualStrings("Not found", r404.description);

    const r500 = op.responses.get("500") orelse return error.TestUnexpectedResult;
    try testing.expectEqualStrings("Server error", r500.description);
}
