// NPM package publishing automation
const std = @import("std");

pub const NPMPublisher = struct {
    allocator: std.mem.Allocator,
    api_token: []const u8,
    
    pub fn init(allocator: std.mem.Allocator, api_token: []const u8) NPMPublisher {
        return NPMPublisher{ 
            .allocator = allocator,
            .api_token = api_token,
        };
    }
    
    pub fn publish(self: *NPMPublisher, package_path: []const u8) !void {
        std.debug.print("📦 Publishing NPM package from {s}...\n", .{package_path});
        
        // Set npm auth token
        const auth_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && echo \"//registry.npmjs.org/:_authToken={s}\" > .npmrc",
            .{package_path, self.api_token}
        );
        defer self.allocator.free(auth_cmd);
        
        var auth_child = std.process.Child.init(&[_][]const u8{ "sh", "-c", auth_cmd }, self.allocator);
        try auth_child.spawn();
        _ = try auth_child.wait();
        
        // Publish package
        const publish_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && npm publish",
            .{package_path}
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
            std.debug.print("NPM publish error: {s}\n", .{stderr});
            return error.PublishFailed;
        }
        
        std.debug.print("✅ Successfully published NPM package\n");
    }
    
    pub fn buildPackage(self: *NPMPublisher, package_path: []const u8) !void {
        const build_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && npm run build",
            .{package_path}
        );
        defer self.allocator.free(build_cmd);
        
        var child = std.process.Child.init(&[_][]const u8{ "sh", "-c", build_cmd }, self.allocator);
        try child.spawn();
        _ = try child.wait();
    }
};
