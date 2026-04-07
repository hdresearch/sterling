const std = @import("std");
const sdk = @import("sdk");

test "template engine rust_type helper" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    var ctx = sdk.template.Context.init(a);
    try ctx.putString("type", "string");

    var engine = sdk.template.Engine.init(a);
    const result = try engine.render("{{rust_type type}}", &ctx);
    try std.testing.expectEqualStrings("String", result);
}

test "template engine snake_case helper" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    var ctx = sdk.template.Context.init(a);
    try ctx.putString("name", "listPets");

    var engine = sdk.template.Engine.init(a);
    const result = try engine.render("{{snake_case name}}", &ctx);
    try std.testing.expectEqualStrings("list_pets", result);
}

test "template context parent lookup" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    var parent = sdk.template.Context.init(a);
    try parent.putString("api_name", "PetStore");

    var child = try parent.createChild();
    try child.putString("op", "listPets");

    // Child can access parent values
    try std.testing.expectEqualStrings("PetStore", child.getString("api_name").?);
    try std.testing.expectEqualStrings("listPets", child.getString("op").?);
}

test "template if-else blocks" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    var ctx = sdk.template.Context.init(a);
    try ctx.putBool("has_body", true);

    var engine = sdk.template.Engine.init(a);
    const result = try engine.render("{{#if has_body}}body: T{{else}}no body{{/if}}", &ctx);
    try std.testing.expectEqualStrings("body: T", result);
}
