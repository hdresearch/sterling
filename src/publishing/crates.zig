// Crates.io package publishing automation
const std = @import("std");

pub const CratesPublisher = struct {
    allocator: std.mem.Allocator,
    api_token: []const u8,
    
    pub fn init(allocator: std.mem.Allocator, api_token: []const u8) CratesPublisher {
        return CratesPublisher{ 
            .allocator = allocator,
            .api_token = api_token,
        };
    }
    
    pub fn publish(self: *CratesPublisher, package_path: []const u8) !void {
        std.debug.print("📦 Publishing Rust crate from {s}...\n", .{package_path});
        
        const publish_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && cargo publish --token {s}",
            .{package_path, self.api_token}
        );
        defer self.allocator.free(publish_cmd);
        
        var child = std.process.Child.init(&[_][]const u8{ "sh", "-c", publish_cmd }, self.allocator);
        child.stdout_behavior = .Pipe;
        child.stderr_behavior = .Pipe;
        
        try child.spawn();
        const stdout = try child.stdout.?.readToEndAlloc(self.allocator, 1024 * 1024);
        const stderr = try child.stderr.?.readToEndAlloc(self.allocator, 1024 * 1024);
        defer self.allocator.free(stdout);
        defer self.allocator.free(stderr);
        
        const term = try child.wait();
        if (term != .Exited or term.Exited != 0) {
            std.debug.print("Cargo publish error: {s}\n", .{stderr});
            return error.PublishFailed;
        }
        
        std.debug.print("✅ Successfully published Rust crate\n");
    }
};
