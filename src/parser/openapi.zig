const std = @import("std");

pub const OpenAPISpec = struct {
    openapi: []const u8,
    info: Info,
    paths: std.StringHashMap(PathItem),

    /// Free all hash-map memory owned by this spec.
    pub fn deinit(self: *OpenAPISpec) void {
        var pit = self.paths.iterator();
        while (pit.next()) |entry| {
            var path_item = entry.value_ptr;
            inline for (.{ "get", "post", "put", "delete", "patch" }) |method| {
                if (@field(path_item, method)) |*op| {
                    op.responses.deinit();
                }
            }
        }
        self.paths.deinit();
    }

    pub const Info = struct {
        title: []const u8,
        version: []const u8,
        description: ?[]const u8 = null,
    };

    pub const PathItem = struct {
        get: ?Operation = null,
        post: ?Operation = null,
        put: ?Operation = null,
        delete: ?Operation = null,
        patch: ?Operation = null,
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

pub const ParseError = error{
    InvalidYaml,
    MissingOpenAPIVersion,
    MissingInfo,
    MissingTitle,
    MissingVersion,
    MissingPaths,
    UnexpectedToken,
    OutOfMemory,
    InvalidJson,
};

// ── YAML node tree ──────────────────────────────────────────────────────

const YamlNode = struct {
    map: std.StringArrayHashMap(*YamlNode),
    scalar: ?[]const u8,
    allocator: std.mem.Allocator,

    fn init(allocator: std.mem.Allocator) !*YamlNode {
        const node = try allocator.create(YamlNode);
        node.* = .{
            .map = std.StringArrayHashMap(*YamlNode).init(allocator),
            .scalar = null,
            .allocator = allocator,
        };
        return node;
    }

    fn getChild(self: *YamlNode, key: []const u8) ?*YamlNode {
        return self.map.get(key);
    }

    fn getChildScalar(self: *YamlNode, key: []const u8) ?[]const u8 {
        const child = self.map.get(key) orelse return null;
        return child.scalar;
    }
};

// ── Line-info for YAML parsing ──────────────────────────────────────────

const LineInfo = struct {
    indent: usize,
    key: []const u8,
    value: ?[]const u8,
};

// ── Detect JSON vs YAML ─────────────────────────────────────────────────

fn isJsonContent(content: []const u8) bool {
    for (content) |c| {
        if (c == ' ' or c == '\t' or c == '\n' or c == '\r') continue;
        return c == '{';
    }
    return false;
}

// ── YAML parser ─────────────────────────────────────────────────────────

fn parseYaml(allocator: std.mem.Allocator, content: []const u8) !*YamlNode {
    const root = try YamlNode.init(allocator);

    // Two-pass: count then fill
    var line_count: usize = 0;
    {
        var it = std.mem.splitScalar(u8, content, '\n');
        while (it.next()) |raw| {
            const ln = stripCR(raw);
            const t = std.mem.trim(u8, ln, " \t");
            if (t.len == 0 or t[0] == '#' or t[0] == '-') continue;
            if (std.mem.indexOfScalar(u8, t, ':') != null) line_count += 1;
        }
    }

    const lines = try allocator.alloc(LineInfo, line_count);
    var idx: usize = 0;
    {
        var it = std.mem.splitScalar(u8, content, '\n');
        while (it.next()) |raw| {
            const ln = stripCR(raw);
            const t = std.mem.trim(u8, ln, " \t");
            if (t.len == 0 or t[0] == '#' or t[0] == '-') continue;

            if (std.mem.indexOfScalar(u8, t, ':')) |cp| {
                var indent: usize = 0;
                for (ln) |c| {
                    if (c == ' ') indent += 1 else break;
                }
                const key = std.mem.trim(u8, t[0..cp], " \t'\"");
                const after = std.mem.trim(u8, t[cp + 1 ..], " \t");
                const value: ?[]const u8 = if (after.len > 0) stripQuotes(after) else null;
                lines[idx] = .{ .indent = indent, .key = key, .value = value };
                idx += 1;
            }
        }
    }

    try buildYamlTree(allocator, root, lines, 0, lines.len, 0);
    return root;
}

fn stripCR(s: []const u8) []const u8 {
    if (s.len > 0 and s[s.len - 1] == '\r') return s[0 .. s.len - 1];
    return s;
}

fn stripQuotes(s: []const u8) []const u8 {
    if (s.len >= 2) {
        if ((s[0] == '\'' and s[s.len - 1] == '\'') or
            (s[0] == '"' and s[s.len - 1] == '"'))
            return s[1 .. s.len - 1];
    }
    return s;
}

fn buildYamlTree(
    allocator: std.mem.Allocator,
    parent: *YamlNode,
    lines: []const LineInfo,
    start: usize,
    end: usize,
    expected_indent: usize,
) !void {
    var i = start;
    while (i < end) {
        const line = lines[i];
        if (line.indent < expected_indent) break;
        if (line.indent > expected_indent) {
            i += 1;
            continue;
        }

        if (line.value) |val| {
            const node = try YamlNode.init(allocator);
            node.scalar = val;
            try parent.map.put(line.key, node);
            i += 1;
        } else {
            const node = try YamlNode.init(allocator);
            const child_start = i + 1;
            var child_end = child_start;
            while (child_end < end) {
                if (lines[child_end].indent <= expected_indent) break;
                child_end += 1;
            }
            if (child_start < child_end) {
                const child_indent = lines[child_start].indent;
                try buildYamlTree(allocator, node, lines, child_start, child_end, child_indent);
            }
            try parent.map.put(line.key, node);
            i = child_end;
        }
    }
}

// ── JSON parser ─────────────────────────────────────────────────────────

const JsonParseError = ParseError || std.mem.Allocator.Error;

const JsonParser = struct {
    allocator: std.mem.Allocator,
    content: []const u8,
    pos: usize,

    fn skipWS(self: *JsonParser) void {
        while (self.pos < self.content.len) {
            const c = self.content[self.pos];
            if (c == ' ' or c == '\t' or c == '\n' or c == '\r') self.pos += 1 else break;
        }
    }

    fn parseValue(self: *JsonParser) JsonParseError!*YamlNode {
        self.skipWS();
        if (self.pos >= self.content.len) return ParseError.InvalidJson;
        return switch (self.content[self.pos]) {
            '{' => self.parseObject(),
            '"' => self.parseString(),
            '[' => self.parseArray(),
            else => self.parseRaw(),
        };
    }

    fn parseObject(self: *JsonParser) JsonParseError!*YamlNode {
        const node = try YamlNode.init(self.allocator);
        self.pos += 1;
        self.skipWS();
        if (self.pos < self.content.len and self.content[self.pos] == '}') {
            self.pos += 1;
            return node;
        }
        while (self.pos < self.content.len) {
            self.skipWS();
            if (self.pos >= self.content.len or self.content[self.pos] != '"') return ParseError.InvalidJson;
            const kn = try self.parseString();
            const key = kn.scalar orelse return ParseError.InvalidJson;
            self.skipWS();
            if (self.pos >= self.content.len or self.content[self.pos] != ':') return ParseError.InvalidJson;
            self.pos += 1;
            const val = try self.parseValue();
            try node.map.put(key, val);
            self.skipWS();
            if (self.pos >= self.content.len) break;
            if (self.content[self.pos] == ',') {
                self.pos += 1;
            } else if (self.content[self.pos] == '}') {
                self.pos += 1;
                break;
            }
        }
        return node;
    }

    fn parseArray(self: *JsonParser) JsonParseError!*YamlNode {
        const node = try YamlNode.init(self.allocator);
        self.pos += 1;
        var depth: usize = 1;
        while (self.pos < self.content.len and depth > 0) {
            switch (self.content[self.pos]) {
                '[' => depth += 1,
                ']' => depth -= 1,
                '"' => {
                    self.pos += 1;
                    while (self.pos < self.content.len and self.content[self.pos] != '"') {
                        if (self.content[self.pos] == '\\') self.pos += 1;
                        self.pos += 1;
                    }
                },
                else => {},
            }
            self.pos += 1;
        }
        return node;
    }

    fn parseString(self: *JsonParser) JsonParseError!*YamlNode {
        const node = try YamlNode.init(self.allocator);
        self.pos += 1;
        const start = self.pos;
        while (self.pos < self.content.len and self.content[self.pos] != '"') {
            if (self.content[self.pos] == '\\') self.pos += 1;
            self.pos += 1;
        }
        node.scalar = self.content[start..self.pos];
        if (self.pos < self.content.len) self.pos += 1;
        return node;
    }

    fn parseRaw(self: *JsonParser) JsonParseError!*YamlNode {
        const node = try YamlNode.init(self.allocator);
        const start = self.pos;
        while (self.pos < self.content.len) {
            const c = self.content[self.pos];
            if (c == ',' or c == '}' or c == ']' or c == '\n' or c == '\r') break;
            self.pos += 1;
        }
        node.scalar = std.mem.trim(u8, self.content[start..self.pos], " \t");
        return node;
    }
};

fn parseJson(allocator: std.mem.Allocator, content: []const u8) JsonParseError!*YamlNode {
    var p = JsonParser{ .allocator = allocator, .content = content, .pos = 0 };
    return p.parseValue();
}

// ── Public API ──────────────────────────────────────────────────────────

pub fn parseOpenAPISpec(allocator: std.mem.Allocator, content: []const u8) ParseError!OpenAPISpec {
    // Use an internal arena for the temporary YAML/JSON parse tree.
    // All string data in the resulting OpenAPISpec are slices into `content`,
    // so it is safe to free the tree after building the spec.
    var tree_arena = std.heap.ArenaAllocator.init(allocator);
    defer tree_arena.deinit();
    const tree_alloc = tree_arena.allocator();

    const root = if (isJsonContent(content))
        parseJson(tree_alloc, content) catch return ParseError.InvalidJson
    else
        parseYaml(tree_alloc, content) catch return ParseError.InvalidYaml;

    const openapi_version = root.getChildScalar("openapi") orelse
        return ParseError.MissingOpenAPIVersion;

    const info_node = root.getChild("info") orelse return ParseError.MissingInfo;
    const title = info_node.getChildScalar("title") orelse return ParseError.MissingTitle;
    const version = info_node.getChildScalar("version") orelse return ParseError.MissingVersion;
    const description = info_node.getChildScalar("description");

    const paths_node = root.getChild("paths") orelse return ParseError.MissingPaths;

    var paths = std.StringHashMap(OpenAPISpec.PathItem).init(allocator);

    var path_iter = paths_node.map.iterator();
    while (path_iter.next()) |entry| {
        const path_name = entry.key_ptr.*;
        const path_node = entry.value_ptr.*;
        var path_item = OpenAPISpec.PathItem{};

        inline for (.{ "get", "post", "put", "delete", "patch" }) |method| {
            if (path_node.getChild(method)) |method_node| {
                var responses = std.StringHashMap(OpenAPISpec.Response).init(allocator);
                if (method_node.getChild("responses")) |responses_node| {
                    var ri = responses_node.map.iterator();
                    while (ri.next()) |re| {
                        const rn = re.value_ptr.*;
                        const rd = rn.getChildScalar("description") orelse "No description";
                        try responses.put(re.key_ptr.*, OpenAPISpec.Response{ .description = rd });
                    }
                }
                @field(path_item, method) = OpenAPISpec.Operation{
                    .operationId = method_node.getChildScalar("operationId"),
                    .summary = method_node.getChildScalar("summary"),
                    .responses = responses,
                };
            }
        }
        try paths.put(path_name, path_item);
    }

    return OpenAPISpec{
        .openapi = openapi_version,
        .info = .{ .title = title, .version = version, .description = description },
        .paths = paths,
    };
}

/// Backward-compatible alias
pub const parseOpenAPI = parseOpenAPISpec;

// ── Inline tests ────────────────────────────────────────────────────────

test "parse yaml basic" {
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

    const spec = try parseOpenAPISpec(a, content);
    try std.testing.expectEqualStrings("Test API", spec.info.title);
    try std.testing.expectEqualStrings("1.0.0", spec.info.version);
    try std.testing.expectEqualStrings("3.0.0", spec.openapi);

    const path = spec.paths.get("/pets") orelse return error.TestUnexpectedResult;
    try std.testing.expect(path.get != null);
    try std.testing.expectEqualStrings("listPets", path.get.?.operationId.?);
    const resp = path.get.?.responses.get("200") orelse return error.TestUnexpectedResult;
    try std.testing.expectEqualStrings("OK", resp.description);
}

test "parse json basic" {
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

    const spec = try parseOpenAPISpec(a, content);
    try std.testing.expectEqualStrings("JSON API", spec.info.title);
    try std.testing.expectEqualStrings("2.0.0", spec.info.version);
}

test "missing openapi version" {
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
    try std.testing.expectError(ParseError.MissingOpenAPIVersion, parseOpenAPISpec(arena.allocator(), content));
}

test "missing info" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const content =
        \\openapi: 3.0.0
        \\paths:
        \\  /test:
        \\    get:
        \\      operationId: test
    ;
    try std.testing.expectError(ParseError.MissingInfo, parseOpenAPISpec(arena.allocator(), content));
}
