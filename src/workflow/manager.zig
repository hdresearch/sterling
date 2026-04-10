// Workflow management and orchestration
const std = @import("std");
const config = @import("../config.zig");
const generator = @import("../generator.zig");
const llm = @import("../llm/integration.zig");
const github = @import("../github/automation.zig");
const publishing = struct {
    const npm = @import("../publishing/npm.zig");
    const crates = @import("../publishing/crates.zig");
    const pypi = @import("../publishing/pypi.zig");
};
const integration = struct {
    const tester = @import("../integration/tester.zig");
    const validator = @import("../integration/validator.zig");
};

pub const WorkflowManager = struct {
    allocator: std.mem.Allocator,
    config_data: config.Config,
    
    pub fn init(allocator: std.mem.Allocator, config_data: config.Config) WorkflowManager {
        return WorkflowManager{ 
            .allocator = allocator,
            .config_data = config_data,
        };
    }
    
    pub fn runFullPipeline(self: *WorkflowManager, spec_path: []const u8, enhance: bool) !void {
        std.debug.print("🚀 Starting full Sterling pipeline...\n");
        
        // Step 1: Generate SDKs
        std.debug.print("📝 Step 1: Generating SDKs...\n");
        try self.generateAllSDKs(spec_path, enhance);
        
        // Step 2: Validate generated SDKs
        std.debug.print("🔍 Step 2: Validating SDKs...\n");
        try self.validateAllSDKs();
        
        // Step 3: Test SDKs
        std.debug.print("🧪 Step 3: Testing SDKs...\n");
        try self.testAllSDKs();
        
        // Step 4: Create GitHub repositories (if configured)
        if (self.config_data.github) |github_config| {
            std.debug.print("📦 Step 4: Creating GitHub repositories...\n");
            try self.createGitHubRepos(github_config);
        }
        
        // Step 5: Publish packages (if configured)
        std.debug.print("🚀 Step 5: Publishing packages...\n");
        try self.publishAllPackages();
        
        std.debug.print("✅ Pipeline completed successfully!\n");
    }
    
    fn generateAllSDKs(self: *WorkflowManager, spec_path: []const u8, enhance: bool) !void {
        var gen = generator.Generator.init(self.allocator);
        defer gen.deinit();
        
        for (self.config_data.targets) |target| {
            std.debug.print("  Generating {s} SDK...\n", .{target.language});
            try gen.generateSDK(spec_path, target, enhance);
        }
    }
    
    fn validateAllSDKs(self: *WorkflowManager) !void {
        var validator = integration.validator.SDKValidator.init(self.allocator);
        
        for (self.config_data.targets) |target| {
            const valid = try validator.validateGeneratedSDK(target.output_dir, target.language);
            if (!valid) {
                std.debug.print("❌ Validation failed for {s} SDK\n", .{target.language});
                return error.ValidationFailed;
            }
            std.debug.print("✅ {s} SDK validation passed\n", .{target.language});
        }
    }
    
    fn testAllSDKs(self: *WorkflowManager) !void {
        var tester = integration.tester.SDKTester.init(self.allocator);
        
        for (self.config_data.targets) |target| {
            const passed = try tester.testGeneratedSDK(target.output_dir, target.language);
            if (!passed) {
                std.debug.print("❌ Tests failed for {s} SDK\n", .{target.language});
                return error.TestsFailed;
            }
            std.debug.print("✅ {s} SDK tests passed\n", .{target.language});
        }
    }
    
    fn createGitHubRepos(self: *WorkflowManager, github_config: config.GitHubConfig) !void {
        var gh = github.GitHubAutomation.init(self.allocator, github_config.token, github_config.org);
        defer gh.deinit();
        
        for (self.config_data.targets) |target| {
            if (target.repository) |repo_url| {
                std.debug.print("  Creating repository for {s} SDK...\n", .{target.language});
                try gh.createRepository(target.language, "Generated SDK", false);
            }
        }
    }
    
    fn publishAllPackages(self: *WorkflowManager) !void {
        for (self.config_data.targets) |target| {
            if (std.mem.eql(u8, target.language, "typescript")) {
                // Publish to NPM
                if (std.process.getEnvVarOwned(self.allocator, "NPM_TOKEN")) |token| {
                    defer self.allocator.free(token);
                    var npm = publishing.npm.NPMPublisher.init(self.allocator, token);
                    try npm.publish(target.output_dir);
                } else |_| {
                    std.debug.print("⚠️  NPM_TOKEN not found, skipping NPM publish\n");
                }
            } else if (std.mem.eql(u8, target.language, "rust")) {
                // Publish to crates.io
                if (std.process.getEnvVarOwned(self.allocator, "CARGO_REGISTRY_TOKEN")) |token| {
                    defer self.allocator.free(token);
                    var crates = publishing.crates.CratesPublisher.init(self.allocator, token);
                    try crates.publish(target.output_dir);
                } else |_| {
                    std.debug.print("⚠️  CARGO_REGISTRY_TOKEN not found, skipping crates.io publish\n");
                }
            } else if (std.mem.eql(u8, target.language, "python")) {
                // Publish to PyPI
                if (std.process.getEnvVarOwned(self.allocator, "PYPI_TOKEN")) |token| {
                    defer self.allocator.free(token);
                    var pypi = publishing.pypi.PyPIPublisher.init(self.allocator, token);
                    try pypi.buildPackage(target.output_dir);
                    try pypi.publish(target.output_dir);
                } else |_| {
                    std.debug.print("⚠️  PYPI_TOKEN not found, skipping PyPI publish\n");
                }
            }
        }
    }
};
