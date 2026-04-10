// Integration testing and validation
const std = @import("std");

pub const SDKTester = struct {
    allocator: std.mem.Allocator,
    
    pub fn init(allocator: std.mem.Allocator) SDKTester {
        return SDKTester{ .allocator = allocator };
    }
    
    pub fn testGeneratedSDK(self: *SDKTester, sdk_path: []const u8, language: []const u8) !bool {
        std.debug.print("🧪 Testing {s} SDK at {s}...\n", .{language, sdk_path});
        
        if (std.mem.eql(u8, language, "rust")) {
            return self.testRustSDK(sdk_path);
        } else if (std.mem.eql(u8, language, "typescript")) {
            return self.testTypeScriptSDK(sdk_path);
        } else if (std.mem.eql(u8, language, "python")) {
            return self.testPythonSDK(sdk_path);
        } else if (std.mem.eql(u8, language, "go")) {
            return self.testGoSDK(sdk_path);
        }
        
        return false;
    }
    
    fn testRustSDK(self: *SDKTester, sdk_path: []const u8) !bool {
        const test_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && cargo check && cargo test",
            .{sdk_path}
        );
        defer self.allocator.free(test_cmd);
        
        return self.runCommand(test_cmd);
    }
    
    fn testTypeScriptSDK(self: *SDKTester, sdk_path: []const u8) !bool {
        const test_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && npm install && npm run build && npm test",
            .{sdk_path}
        );
        defer self.allocator.free(test_cmd);
        
        return self.runCommand(test_cmd);
    }
    
    fn testPythonSDK(self: *SDKTester, sdk_path: []const u8) !bool {
        const test_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && python -m pip install -e . && python -m pytest",
            .{sdk_path}
        );
        defer self.allocator.free(test_cmd);
        
        return self.runCommand(test_cmd);
    }
    
    fn testGoSDK(self: *SDKTester, sdk_path: []const u8) !bool {
        const test_cmd = try std.fmt.allocPrint(
            self.allocator,
            "cd {s} && go mod tidy && go build && go test ./...",
            .{sdk_path}
        );
        defer self.allocator.free(test_cmd);
        
        return self.runCommand(test_cmd);
    }
    
    fn runCommand(self: *SDKTester, cmd: []const u8) !bool {
        var child = std.process.Child.init(&[_][]const u8{ "sh", "-c", cmd }, self.allocator);
        child.stdout_behavior = .Pipe;
        child.stderr_behavior = .Pipe;
        
        try child.spawn();
        const stdout = try child.stdout.?.readToEndAlloc(self.allocator, 1024 * 1024);
        const stderr = try child.stderr.?.readToEndAlloc(self.allocator, 1024 * 1024);
        defer self.allocator.free(stdout);
        defer self.allocator.free(stderr);
        
        const term = try child.wait();
        if (term != .Exited or term.Exited != 0) {
            std.debug.print("Test failed: {s}\n", .{stderr});
            return false;
        }
        
        std.debug.print("✅ Tests passed\n");
        return true;
    }
};
