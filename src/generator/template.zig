const std = @import("std");

/// A template value that can be a string, boolean, or list of child contexts.
pub const Value = union(enum) {
    string: []const u8,
    boolean: bool,
    list: []const *Context,
};

/// Template rendering context with key-value pairs and optional parent scope.
pub const Context = struct {
    values: std.StringArrayHashMap(Value),
    parent: ?*const Context,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) Context {
        return .{
            .values = std.StringArrayHashMap(Value).init(allocator),
            .parent = null,
            .allocator = allocator,
        };
    }

    pub fn put(self: *Context, key: []const u8, value: Value) !void {
        try self.values.put(key, value);
    }

    pub fn putString(self: *Context, key: []const u8, value: []const u8) !void {
        try self.values.put(key, .{ .string = value });
    }

    pub fn putBool(self: *Context, key: []const u8, value: bool) !void {
        try self.values.put(key, .{ .boolean = value });
    }

    pub fn putList(self: *Context, key: []const u8, items: []const *Context) !void {
        try self.values.put(key, .{ .list = items });
    }

    pub fn get(self: *const Context, key: []const u8) ?Value {
        if (self.values.get(key)) |v| return v;
        if (self.parent) |p| return p.get(key);
        return null;
    }

    pub fn getString(self: *const Context, key: []const u8) ?[]const u8 {
        const val = self.get(key) orelse return null;
        return switch (val) {
            .string => |s| s,
            .boolean => |b| if (b) "true" else "false",
            else => null,
        };
    }

    /// Create a child context with this as parent.
    pub fn createChild(self: *const Context) !*Context {
        const child = try self.allocator.create(Context);
        child.* = Context.init(self.allocator);
        child.parent = self;
        return child;
    }
};

/// Handlebars-style template engine.
pub const Engine = struct {
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) Engine {
        return .{ .allocator = allocator };
    }

    /// Render a template string with the given context.
    pub fn render(self: *Engine, template: []const u8, context: *const Context) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        errdefer buf.deinit();
        try self.renderInto(&buf, template, context);
        return try buf.toOwnedSlice();
    }

    fn renderInto(self: *Engine, buf: *std.array_list.Managed(u8), template: []const u8, context: *const Context) !void {
        var pos: usize = 0;

        while (pos < template.len) {
            // Find next {{
            if (indexOfFrom(template, pos, "{{")) |tag_start| {
                // Output text before tag
                try buf.appendSlice(template[pos..tag_start]);

                // Find matching }}
                const tag_end = indexOfFrom(template, tag_start + 2, "}}") orelse {
                    try buf.appendSlice(template[tag_start..]);
                    break;
                };

                const tag_content = std.mem.trim(u8, template[tag_start + 2 .. tag_end], " \t");
                const after_tag = tag_end + 2;

                if (std.mem.startsWith(u8, tag_content, "#each ")) {
                    const var_name = std.mem.trim(u8, tag_content[6..], " \t");
                    const block = try findBlockEnd(template, after_tag, "each");
                    pos = block.after;

                    if (context.get(var_name)) |val| {
                        switch (val) {
                            .list => |items| {
                                for (items) |item| {
                                    try self.renderInto(buf, template[after_tag..block.body_end], item);
                                }
                            },
                            else => {},
                        }
                    }
                } else if (std.mem.startsWith(u8, tag_content, "#if ")) {
                    const var_name = std.mem.trim(u8, tag_content[4..], " \t");
                    const block = try findBlockEnd(template, after_tag, "if");
                    pos = block.after;

                    const truthy = isTruthy(context.get(var_name));

                    // Check for {{else}}
                    if (findElse(template, after_tag, block.body_end)) |else_pos| {
                        if (truthy) {
                            try self.renderInto(buf, template[after_tag..else_pos], context);
                        } else {
                            try self.renderInto(buf, template[else_pos + 8 .. block.body_end], context);
                        }
                    } else {
                        if (truthy) {
                            try self.renderInto(buf, template[after_tag..block.body_end], context);
                        }
                    }
                } else if (std.mem.startsWith(u8, tag_content, "#unless ")) {
                    const var_name = std.mem.trim(u8, tag_content[8..], " \t");
                    const block = try findBlockEnd(template, after_tag, "unless");
                    pos = block.after;

                    if (!isTruthy(context.get(var_name))) {
                        try self.renderInto(buf, template[after_tag..block.body_end], context);
                    }
                } else if (tag_content.len > 0 and tag_content[0] == '/') {
                    // Closing tag encountered unexpectedly, skip
                    pos = after_tag;
                } else {
                    // Variable or helper function: {{var}} or {{helper var}}
                    const resolved = self.resolveExpression(tag_content, context);
                    try buf.appendSlice(resolved);
                    pos = after_tag;
                }
            } else {
                // No more tags
                try buf.appendSlice(template[pos..]);
                break;
            }
        }
    }

    /// Resolve a template expression (variable or helper call).
    fn resolveExpression(self: *Engine, expr: []const u8, context: *const Context) []const u8 {
        // Check for helper: "helper_name argument"
        if (std.mem.indexOfScalar(u8, expr, ' ')) |space_idx| {
            const helper_name = expr[0..space_idx];
            const arg_name = std.mem.trim(u8, expr[space_idx + 1 ..], " \t");
            const arg_value = context.getString(arg_name) orelse "";

            return self.applyHelper(helper_name, arg_value);
        }

        // Simple variable lookup
        return context.getString(expr) orelse "";
    }

    /// Apply a named helper function to a string value.
    fn applyHelper(self: *Engine, helper: []const u8, value: []const u8) []const u8 {
        if (std.mem.eql(u8, helper, "snake_case")) {
            return self.toSnakeCase(value);
        } else if (std.mem.eql(u8, helper, "pascal_case")) {
            return self.toPascalCase(value);
        } else if (std.mem.eql(u8, helper, "upper")) {
            return self.toUpper(value);
        } else if (std.mem.eql(u8, helper, "rust_type")) {
            return rustType(value);
        }
        return value;
    }

    fn toSnakeCase(self: *Engine, input: []const u8) []const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        for (input, 0..) |c, i| {
            if (std.ascii.isUpper(c)) {
                if (i > 0) buf.append('_') catch return input;
                buf.append(std.ascii.toLower(c)) catch return input;
            } else {
                buf.append(c) catch return input;
            }
        }
        return buf.toOwnedSlice() catch input;
    }

    fn toPascalCase(self: *Engine, input: []const u8) []const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var capitalize_next = true;
        for (input) |c| {
            if (c == '_' or c == '-') {
                capitalize_next = true;
                continue;
            }
            if (capitalize_next) {
                buf.append(std.ascii.toUpper(c)) catch return input;
                capitalize_next = false;
            } else {
                buf.append(c) catch return input;
            }
        }
        return buf.toOwnedSlice() catch input;
    }

    fn toUpper(self: *Engine, input: []const u8) []const u8 {
        var buf = self.allocator.alloc(u8, input.len) catch return input;
        for (input, 0..) |c, i| {
            buf[i] = std.ascii.toUpper(c);
        }
        return buf;
    }

    fn rustType(input: []const u8) []const u8 {
        if (std.mem.eql(u8, input, "string")) return "String";
        if (std.mem.eql(u8, input, "integer")) return "i64";
        if (std.mem.eql(u8, input, "number")) return "f64";
        if (std.mem.eql(u8, input, "boolean")) return "bool";
        if (std.mem.eql(u8, input, "array")) return "Vec<serde_json::Value>";
        if (std.mem.eql(u8, input, "object")) return "serde_json::Value";
        return input;
    }
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn isTruthy(val: ?Value) bool {
    const v = val orelse return false;
    return switch (v) {
        .string => |s| s.len > 0,
        .boolean => |b| b,
        .list => |l| l.len > 0,
    };
}

fn indexOfFrom(haystack: []const u8, start: usize, needle: []const u8) ?usize {
    if (start >= haystack.len) return null;
    return if (std.mem.indexOfPos(u8, haystack, start, needle)) |idx| idx else null;
}

const BlockEnd = struct {
    body_end: usize,
    after: usize,
};

fn findBlockEnd(template: []const u8, start: usize, tag_name: []const u8) !BlockEnd {
    var depth: usize = 1;
    var pos = start;

    while (pos < template.len) {
        const tag_start = indexOfFrom(template, pos, "{{") orelse break;
        const tag_end = indexOfFrom(template, tag_start + 2, "}}") orelse break;
        const content = std.mem.trim(u8, template[tag_start + 2 .. tag_end], " \t");

        if (content.len > 1 and content[0] == '#') {
            const rest = content[1..];
            if (startsWithTag(rest, tag_name)) {
                depth += 1;
            }
        } else if (content.len > 1 and content[0] == '/') {
            const rest = std.mem.trim(u8, content[1..], " \t");
            if (std.mem.eql(u8, rest, tag_name)) {
                depth -= 1;
                if (depth == 0) {
                    return .{ .body_end = tag_start, .after = tag_end + 2 };
                }
            }
        }
        pos = tag_end + 2;
    }
    return error.UnmatchedBlockTag;
}

/// Find {{else}} at depth 0 within a range.
fn findElse(template: []const u8, start: usize, end: usize) ?usize {
    var depth: usize = 0;
    var pos = start;

    while (pos < end) {
        const tag_start = indexOfFrom(template, pos, "{{") orelse break;
        if (tag_start >= end) break;
        const tag_end = indexOfFrom(template, tag_start + 2, "}}") orelse break;
        if (tag_end >= end) break;
        const content = std.mem.trim(u8, template[tag_start + 2 .. tag_end], " \t");

        if (content.len > 1 and content[0] == '#') {
            depth += 1;
        } else if (content.len > 1 and content[0] == '/') {
            if (depth > 0) depth -= 1;
        } else if (depth == 0 and std.mem.eql(u8, content, "else")) {
            return tag_start;
        }
        pos = tag_end + 2;
    }
    return null;
}

fn startsWithTag(text: []const u8, tag_name: []const u8) bool {
    if (!std.mem.startsWith(u8, text, tag_name)) return false;
    return text.len == tag_name.len or text[tag_name.len] == ' ';
}

// ── Tests ───────────────────────────────────────────────────────────────

test "simple variable substitution" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putString("name", "World");

    var engine = Engine.init(allocator);
    const result = try engine.render("Hello, {{name}}!", &ctx);
    try std.testing.expectEqualStrings("Hello, World!", result);
}

test "if block true" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putBool("show", true);

    var engine = Engine.init(allocator);
    const result = try engine.render("{{#if show}}visible{{/if}}", &ctx);
    try std.testing.expectEqualStrings("visible", result);
}

test "if block false" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putBool("show", false);

    var engine = Engine.init(allocator);
    const result = try engine.render("{{#if show}}visible{{/if}}", &ctx);
    try std.testing.expectEqualStrings("", result);
}

test "if-else block" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putBool("show", false);

    var engine = Engine.init(allocator);
    const result = try engine.render("{{#if show}}yes{{else}}no{{/if}}", &ctx);
    try std.testing.expectEqualStrings("no", result);
}

test "unless block" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putBool("required", false);

    var engine = Engine.init(allocator);
    const result = try engine.render("field{{#unless required}}?{{/unless}}: string", &ctx);
    try std.testing.expectEqualStrings("field?: string", result);
}

test "each block" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);

    var item1 = Context.init(allocator);
    try item1.putString("name", "Alice");
    var item2 = Context.init(allocator);
    try item2.putString("name", "Bob");

    const item1_ptr = try allocator.create(Context);
    item1_ptr.* = item1;
    const item2_ptr = try allocator.create(Context);
    item2_ptr.* = item2;

    const items = try allocator.alloc(*Context, 2);
    items[0] = item1_ptr;
    items[1] = item2_ptr;
    try ctx.putList("people", @ptrCast(items));

    var engine = Engine.init(allocator);
    const result = try engine.render("{{#each people}}Hello {{name}}\n{{/each}}", &ctx);
    try std.testing.expectEqualStrings("Hello Alice\nHello Bob\n", result);
}

test "helper snake_case" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putString("name", "listPets");

    var engine = Engine.init(allocator);
    const result = try engine.render("{{snake_case name}}", &ctx);
    try std.testing.expectEqualStrings("list_pets", result);
}

test "helper pascal_case" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putString("name", "list_pets");

    var engine = Engine.init(allocator);
    const result = try engine.render("{{pascal_case name}}", &ctx);
    try std.testing.expectEqualStrings("ListPets", result);
}

test "helper upper" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putString("method", "get");

    var engine = Engine.init(allocator);
    const result = try engine.render("{{upper method}}", &ctx);
    try std.testing.expectEqualStrings("GET", result);
}

test "nested each and if" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);

    var op1 = Context.init(allocator);
    try op1.putString("name", "get_pets");
    try op1.putBool("has_body", false);

    var op2 = Context.init(allocator);
    try op2.putString("name", "create_pet");
    try op2.putBool("has_body", true);

    const op1_ptr = try allocator.create(Context);
    op1_ptr.* = op1;
    const op2_ptr = try allocator.create(Context);
    op2_ptr.* = op2;

    const ops = try allocator.alloc(*Context, 2);
    ops[0] = op1_ptr;
    ops[1] = op2_ptr;
    try ctx.putList("operations", @ptrCast(ops));

    var engine = Engine.init(allocator);
    const result = try engine.render("{{#each operations}}fn {{name}}({{#if has_body}}body{{/if}})\n{{/each}}", &ctx);
    try std.testing.expectEqualStrings("fn get_pets()\nfn create_pet(body)\n", result);
}

test "parent context access in each" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putString("title", "MyAPI");

    var item1 = try ctx.createChild();
    try item1.putString("name", "op1");

    const items = try allocator.alloc(*Context, 1);
    items[0] = item1;
    try ctx.putList("ops", @ptrCast(items));

    var engine = Engine.init(allocator);
    const result = try engine.render("{{#each ops}}{{title}}: {{name}}\n{{/each}}", &ctx);
    try std.testing.expectEqualStrings("MyAPI: op1\n", result);
}

test "missing variable returns empty string" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    var engine = Engine.init(allocator);
    const result = try engine.render("Hello {{name}}!", &ctx);
    try std.testing.expectEqualStrings("Hello !", result);
}

test "rust_type helper" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    var ctx = Context.init(allocator);
    try ctx.putString("type", "string");

    var engine = Engine.init(allocator);
    const result = try engine.render("{{rust_type type}}", &ctx);
    try std.testing.expectEqualStrings("String", result);
}
