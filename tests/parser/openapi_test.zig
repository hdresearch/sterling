const std = @import("std");
const openapi = @import("openapi");

test "parse petstore yaml" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

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
    ;

    const spec = try openapi.parseOpenAPI(a, content);
    try std.testing.expectEqualStrings("Pet Store API", spec.info.title);
    try std.testing.expectEqualStrings("1.0.0", spec.info.version);

    const pets_path = spec.paths.get("/pets") orelse return error.TestUnexpectedResult;
    try std.testing.expect(pets_path.get != null);
    try std.testing.expect(pets_path.post != null);
    try std.testing.expectEqualStrings("listPets", pets_path.get.?.operationId.?);
    try std.testing.expectEqualStrings("createPet", pets_path.post.?.operationId.?);
}
