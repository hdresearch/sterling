const std = @import("std");
const sdk = @import("sdk");

test "SDKGenerator init" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    var paths = std.StringHashMap(sdk.parser.OpenAPISpec.PathItem).init(a);
    try paths.put("/pets", sdk.parser.OpenAPISpec.PathItem{});

    const spec = sdk.parser.OpenAPISpec{
        .openapi = "3.0.0",
        .info = .{ .title = "Test API", .version = "1.0.0" },
        .paths = paths,
    };

    const cfg = sdk.config.Config{
        .project = .{ .name = "test-sdk", .version = "1.0.0" },
        .targets = &.{},
    };

    var gen = sdk.SDKGenerator.init(a, spec, cfg);
    _ = &gen;
}

test "template engine variable substitution" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    var ctx = sdk.template.Context.init(a);
    try ctx.putString("name", "TestAPI");

    var engine = sdk.template.Engine.init(a);
    const result = try engine.render("Hello {{name}}!", &ctx);
    try std.testing.expectEqualStrings("Hello TestAPI!", result);
}

test "template engine each loop" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    var ctx = sdk.template.Context.init(a);

    var item1 = sdk.template.Context.init(a);
    try item1.putString("op", "listPets");
    var item2 = sdk.template.Context.init(a);
    try item2.putString("op", "createPet");

    const p1 = try a.create(sdk.template.Context);
    p1.* = item1;
    const p2 = try a.create(sdk.template.Context);
    p2.* = item2;

    const items = try a.alloc(*sdk.template.Context, 2);
    items[0] = p1;
    items[1] = p2;
    try ctx.putList("ops", @ptrCast(items));

    var engine = sdk.template.Engine.init(a);
    const result = try engine.render("{{#each ops}}fn {{op}}()\n{{/each}}", &ctx);
    try std.testing.expectEqualStrings("fn listPets()\nfn createPet()\n", result);
}

test "makeDirRecursive creates nested directories" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    const paths = std.StringHashMap(sdk.parser.OpenAPISpec.PathItem).init(a);
    const spec = sdk.parser.OpenAPISpec{
        .openapi = "3.0.0",
        .info = .{ .title = "Test", .version = "1.0.0" },
        .paths = paths,
    };
    const cfg = sdk.config.Config{
        .project = .{ .name = "test", .version = "1.0.0" },
        .targets = &.{},
    };

    var gen = sdk.SDKGenerator.init(a, spec, cfg);

    // Use a tmp directory to test
    var tmp_dir = std.testing.tmpDir(.{});
    defer tmp_dir.cleanup();

    var path_buf: [std.fs.max_path_bytes]u8 = undefined;
    const tmp_path = try tmp_dir.dir.realpath(".", &path_buf);

    const nested = try std.fmt.allocPrint(a, "{s}/a/b/c", .{tmp_path});
    try gen.makeDirRecursive(nested);

    // Verify directory exists
    var dir = try std.fs.cwd().openDir(nested, .{});
    dir.close();
}

test "renderTemplate with file" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    const paths = std.StringHashMap(sdk.parser.OpenAPISpec.PathItem).init(a);
    const spec = sdk.parser.OpenAPISpec{
        .openapi = "3.0.0",
        .info = .{ .title = "Test", .version = "1.0.0" },
        .paths = paths,
    };
    const cfg = sdk.config.Config{
        .project = .{ .name = "test", .version = "1.0.0" },
        .targets = &.{},
    };

    var gen = sdk.SDKGenerator.init(a, spec, cfg);

    // Create temp files
    var tmp_dir = std.testing.tmpDir(.{});
    defer tmp_dir.cleanup();

    // Write a template file
    try tmp_dir.dir.writeFile(.{ .sub_path = "test.template", .data = "Hello {{name}}! Version {{version}}." });

    var path_buf: [std.fs.max_path_bytes]u8 = undefined;
    const tmpl_path = try tmp_dir.dir.realpath("test.template", &path_buf);

    var out_buf: [std.fs.max_path_bytes]u8 = undefined;
    const tmp_base = try tmp_dir.dir.realpath(".", &out_buf);
    const out_path = try std.fmt.allocPrint(a, "{s}/output.txt", .{tmp_base});

    var ctx = sdk.template.Context.init(a);
    try ctx.putString("name", "World");
    try ctx.putString("version", "2.0");

    try gen.renderTemplate(tmpl_path, out_path, &ctx);

    // Read output and verify
    const output = try std.fs.cwd().readFileAlloc(a, out_path, 1024);
    try std.testing.expectEqualStrings("Hello World! Version 2.0.", output);
}
