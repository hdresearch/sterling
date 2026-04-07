const std = @import("std");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    std.debug.print("Sterling SDK Generator v0.1.0\n", .{});
    
    if (args.len < 2) {
        printUsage();
        return;
    }

    const command = args[1];
    if (std.mem.eql(u8, command, "generate")) {
        std.debug.print("Generate command - TODO: implement OpenAPI parsing and SDK generation\n", .{});
    } else if (std.mem.eql(u8, command, "version")) {
        std.debug.print("Sterling v0.1.0 - OpenAPI SDK Generator in Zig\n", .{});
    } else {
        printUsage();
    }
}

fn printUsage() void {
    std.debug.print("Usage:\n", .{});
    std.debug.print("  sterling generate --spec <openapi.yaml> --config <sterling.toml>\n", .{});
    std.debug.print("  sterling version\n", .{});
}
