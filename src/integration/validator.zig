// Integration testing and validation
const std = @import("std");

pub const SDKValidator = struct {
    allocator: std.mem.Allocator,
    
    pub fn init(allocator: std.mem.Allocator) SDKValidator {
        return SDKValidator{ .allocator = allocator };
    }
    
    pub fn validateGeneratedSDK(self: *SDKValidator, sdk_path: []const u8, language: []const u8) !bool {
        std.debug.print("🔍 Validating {s} SDK at {s}...\n", .{language, sdk_path});
        
        // Check if directory exists
        const dir = std.fs.cwd().openDir(sdk_path, .{}) catch {
            std.debug.print("❌ SDK directory not found: {s}\n", .{sdk_path});
            return false;
        };
        dir.close();
        
        // Language-specific validation
        if (std.mem.eql(u8, language, "rust")) {
            return self.validateRustSDK(sdk_path);
        } else if (std.mem.eql(u8, language, "typescript")) {
            return self.validateTypeScriptSDK(sdk_path);
        } else if (std.mem.eql(u8, language, "python")) {
            return self.validatePythonSDK(sdk_path);
        } else if (std.mem.eql(u8, language, "go")) {
            return self.validateGoSDK(sdk_path);
        }
        
        return false;
    }
    
    fn validateRustSDK(self: *SDKValidator, sdk_path: []const u8) !bool {
        return self.checkFileExists(sdk_path, "Cargo.toml") and
               self.checkFileExists(sdk_path, "src/lib.rs") and
               self.checkFileExists(sdk_path, "src/client.rs") and
               self.checkFileExists(sdk_path, "src/models.rs");
    }
    
    fn validateTypeScriptSDK(self: *SDKValidator, sdk_path: []const u8) !bool {
        return self.checkFileExists(sdk_path, "package.json") and
               self.checkFileExists(sdk_path, "src/index.ts") and
               self.checkFileExists(sdk_path, "src/client.ts") and
               self.checkFileExists(sdk_path, "src/models.ts");
    }
    
    fn validatePythonSDK(self: *SDKValidator, sdk_path: []const u8) !bool {
        return self.checkFileExists(sdk_path, "setup.py") and
               self.checkFileExists(sdk_path, "src/__init__.py") and
               self.checkFileExists(sdk_path, "src/client.py") and
               self.checkFileExists(sdk_path, "src/models.py");
    }
    
    fn validateGoSDK(self: *SDKValidator, sdk_path: []const u8) !bool {
        return self.checkFileExists(sdk_path, "go.mod") and
               self.checkFileExists(sdk_path, "client.go") and
               self.checkFileExists(sdk_path, "models.go");
    }
    
    fn checkFileExists(self: *SDKValidator, base_path: []const u8, file_path: []const u8) bool {
        _ = self;
        const full_path = std.fmt.allocPrint(
            std.heap.page_allocator,
            "{s}/{s}",
            .{base_path, file_path}
        ) catch return false;
        defer std.heap.page_allocator.free(full_path);
        
        const file = std.fs.cwd().openFile(full_path, .{}) catch return false;
        file.close();
        return true;
    }
};
