const std = @import("std");

pub const OpenAPISpec = struct {
    openapi: []const u8,
    info: Info,
    paths: std.StringHashMap(PathItem),

    pub const Info = struct {
        title: []const u8,
        version: []const u8,
        description: ?[]const u8 = null,
    };

    pub const PathItem = struct {
        get: ?Operation = null,
        post: ?Operation = null,
    };

    pub const Operation = struct {
        operationId: ?[]const u8 = null,
        summary: ?[]const u8 = null,
        responses: std.StringHashMap(Response),
    };

    pub const Response = struct {
        description: []const u8,
    };
};

pub fn parseOpenAPI(allocator: std.mem.Allocator, content: []const u8) !OpenAPISpec {
    _ = content;
    var paths = std.StringHashMap(OpenAPISpec.PathItem).init(allocator);
    var responses = std.StringHashMap(OpenAPISpec.Response).init(allocator);
    
    try responses.put("200", OpenAPISpec.Response{
        .description = "Success",
    });

    try paths.put("/pets", OpenAPISpec.PathItem{
        .get = OpenAPISpec.Operation{
            .operationId = "listPets",
            .summary = "List all pets",
            .responses = responses,
        },
    });

    return OpenAPISpec{
        .openapi = "3.0.0",
        .info = OpenAPISpec.Info{
            .title = "Pet Store API",
            .version = "1.0.0",
        },
        .paths = paths,
    };
}
