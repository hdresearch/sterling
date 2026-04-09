const std = @import("std");

/// A simple TOML parser supporting:
/// - Key-value pairs (string, integer, boolean)
/// - [section] headers
/// - [[array_of_tables]] headers
/// - Comments (#)
/// - Quoted string values
pub const TomlParser = struct {
    allocator: std.mem.Allocator,

    pub const Value = union(enum) {
        string: []const u8,
        integer: i64,
        boolean: bool,
    };

    pub const Table = struct {
        values: std.StringHashMap(Value),

        pub fn init(allocator: std.mem.Allocator) Table {
            return .{ .values = std.StringHashMap(Value).init(allocator) };
        }

        pub fn getString(self: *const Table, key: []const u8) ?[]const u8 {
            if (self.values.get(key)) |v| {
                return switch (v) {
                    .string => |s| s,
                    else => null,
                };
            }
            return null;
        }

        pub fn getInt(self: *const Table, key: []const u8) ?i64 {
            if (self.values.get(key)) |v| {
                return switch (v) {
                    .integer => |i| i,
                    else => null,
                };
            }
            return null;
        }

        pub fn getBool(self: *const Table, key: []const u8) ?bool {
            if (self.values.get(key)) |v| {
                return switch (v) {
                    .boolean => |b| b,
                    else => null,
                };
            }
            return null;
        }
    };

    /// Stored array of tables: each key maps to a list of Table structs
    const TableList = struct {
        items: []Table,
        len: usize,
        capacity: usize,
        allocator: std.mem.Allocator,

        fn init(allocator: std.mem.Allocator) TableList {
            return .{
                .items = &.{},
                .len = 0,
                .capacity = 0,
                .allocator = allocator,
            };
        }

        fn append(self: *TableList, table: Table) !void {
            if (self.len == self.capacity) {
                const new_cap = if (self.capacity == 0) @as(usize, 4) else self.capacity * 2;
                const new_items = try self.allocator.alloc(Table, new_cap);
                if (self.len > 0) {
                    @memcpy(new_items[0..self.len], self.items[0..self.len]);
                }
                self.items = new_items.ptr[0..new_cap];
                self.capacity = new_cap;
            }
            self.items[self.len] = table;
            self.len += 1;
        }

        fn slice(self: *const TableList) []Table {
            return self.items[0..self.len];
        }
    };

    pub const ParseResult = struct {
        sections: std.StringHashMap(Table),
        array_sections: std.StringHashMap(TableList),
        root: Table,

        pub fn getSection(self: *const ParseResult, name: []const u8) ?*const Table {
            return if (self.sections.getPtr(name)) |ptr| ptr else null;
        }

        pub fn getArraySection(self: *const ParseResult, name: []const u8) ?[]Table {
            if (self.array_sections.getPtr(name)) |arr| {
                return arr.slice();
            }
            return null;
        }
    };

    pub const ParseError = error{
        UnterminatedString,
        InvalidLine,
        UnterminatedSection,
        OutOfMemory,
    };

    pub fn init(allocator: std.mem.Allocator) TomlParser {
        return .{ .allocator = allocator };
    }

    pub fn parse(self: *TomlParser, content: []const u8) ParseError!ParseResult {
        var result = ParseResult{
            .sections = std.StringHashMap(Table).init(self.allocator),
            .array_sections = std.StringHashMap(TableList).init(self.allocator),
            .root = Table.init(self.allocator),
        };

        var current_table: *Table = &result.root;
        var lines = std.mem.splitScalar(u8, content, '\n');

        while (lines.next()) |raw_line| {
            const line = std.mem.trim(u8, raw_line, " \t\r");

            if (line.len == 0 or line[0] == '#') continue;

            // [[array_of_tables]]
            if (line.len >= 4 and line[0] == '[' and line[1] == '[') {
                const end = std.mem.indexOf(u8, line, "]]") orelse return error.UnterminatedSection;
                const section_name = std.mem.trim(u8, line[2..end], " \t");
                const name_copy = self.allocator.dupe(u8, section_name) catch return error.OutOfMemory;

                const new_table = Table.init(self.allocator);

                if (!result.array_sections.contains(name_copy)) {
                    result.array_sections.put(name_copy, TableList.init(self.allocator)) catch return error.OutOfMemory;
                }
                var arr = result.array_sections.getPtr(name_copy).?;
                arr.append(new_table) catch return error.OutOfMemory;
                current_table = &arr.items[arr.len - 1];
                continue;
            }

            // [section]
            if (line[0] == '[') {
                const end = std.mem.indexOfScalar(u8, line, ']') orelse return error.UnterminatedSection;
                const section_name = std.mem.trim(u8, line[1..end], " \t");
                const name_copy = self.allocator.dupe(u8, section_name) catch return error.OutOfMemory;

                if (!result.sections.contains(name_copy)) {
                    result.sections.put(name_copy, Table.init(self.allocator)) catch return error.OutOfMemory;
                }
                current_table = result.sections.getPtr(name_copy).?;
                continue;
            }

            // key = value
            if (std.mem.indexOfScalar(u8, line, '=')) |eq_pos| {
                const key = std.mem.trim(u8, line[0..eq_pos], " \t");
                const val_raw = std.mem.trim(u8, line[eq_pos + 1 ..], " \t");

                const key_copy = self.allocator.dupe(u8, key) catch return error.OutOfMemory;
                const value = try parseValue(self.allocator, val_raw);
                current_table.values.put(key_copy, value) catch return error.OutOfMemory;
            }
        }

        return result;
    }

    fn parseValue(allocator: std.mem.Allocator, raw: []const u8) ParseError!Value {
        if (raw.len == 0) return Value{ .string = "" };

        // Quoted string
        if (raw[0] == '"') {
            var buf: [4096]u8 = undefined;
            var len: usize = 0;
            var i: usize = 1;
            while (i < raw.len) {
                if (raw[i] == '\\' and i + 1 < raw.len) {
                    const c: u8 = switch (raw[i + 1]) {
                        '"' => '"',
                        '\\' => '\\',
                        'n' => '\n',
                        't' => '\t',
                        else => raw[i + 1],
                    };
                    buf[len] = c;
                    len += 1;
                    i += 2;
                } else if (raw[i] == '"') {
                    const result = allocator.dupe(u8, buf[0..len]) catch return error.OutOfMemory;
                    return Value{ .string = result };
                } else {
                    buf[len] = raw[i];
                    len += 1;
                    i += 1;
                }
            }
            return error.UnterminatedString;
        }

        // Boolean
        if (std.mem.eql(u8, raw, "true")) return Value{ .boolean = true };
        if (std.mem.eql(u8, raw, "false")) return Value{ .boolean = false };

        // Integer
        if (std.fmt.parseInt(i64, raw, 10)) |int_val| {
            return Value{ .integer = int_val };
        } else |_| {}

        // Unquoted string fallback
        const copy = allocator.dupe(u8, raw) catch return error.OutOfMemory;
        return Value{ .string = copy };
    }
};
