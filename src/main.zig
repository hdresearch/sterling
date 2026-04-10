const std = @import("std");
const parser = @import("parser/openapi.zig");
const config = @import("config/config.zig");
const sdk_gen = @import("generator/sdk.zig");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    std.debug.print("Sterling SDK Generator v0.1.0\n", .{});
    std.debug.print("Open source replacement for Stainless, written in Zig\n", .{});
    std.debug.print("Features: LLM Enhancement, GitHub Automation, Documentation Generation\n\n", .{});

    if (args.len < 2) {
        printUsage();
        return;
    }

    const command = args[1];
    if (std.mem.eql(u8, command, "generate")) {
        try handleGenerate(allocator, args[2..]);
    } else if (std.mem.eql(u8, command, "version")) {
        std.debug.print("Sterling v0.1.0\n", .{});
        std.debug.print("OpenAPI SDK Generator in Zig\n", .{});
        std.debug.print("Features: LLM Enhancement, GitHub Automation, Documentation Generation\n", .{});
        std.debug.print("https://github.com/hdresearch/sterling\n", .{});
    } else if (std.mem.eql(u8, command, "init")) {
        try handleInit(allocator);
    } else {
        std.debug.print("Unknown command: {s}\n", .{command});
        printUsage();
    }
}

fn printUsage() void {
    std.debug.print("Usage:\n", .{});
    std.debug.print("  sterling generate --spec <openapi.yaml> --config <sterling.toml> [--enhance]\n", .{});
    std.debug.print("  sterling init                    # Create example sterling.toml\n", .{});
    std.debug.print("  sterling version                 # Show version info\n", .{});
    std.debug.print("\nNew Features:\n", .{});
    std.debug.print("  --enhance                        # Enable LLM code enhancement\n", .{});
    std.debug.print("\nEnvironment Variables:\n", .{});
    std.debug.print("  ANTHROPIC_API_KEY               # For LLM enhancement\n", .{});
    std.debug.print("  GITHUB_TOKEN                    # For GitHub automation\n", .{});
    std.debug.print("\n", .{});
}

fn handleGenerate(allocator: std.mem.Allocator, args: [][]const u8) !void {
    var spec_file: ?[]const u8 = null;
    var config_file: ?[]const u8 = null;
    var enhance = false;

    var i: usize = 0;
    while (i < args.len) : (i += 1) {
        if (std.mem.eql(u8, args[i], "--spec") and i + 1 < args.len) {
            spec_file = args[i + 1];
            i += 1;
        } else if (std.mem.eql(u8, args[i], "--config") and i + 1 < args.len) {
            config_file = args[i + 1];
            i += 1;
        } else if (std.mem.eql(u8, args[i], "--enhance")) {
            enhance = true;
        }
    }

    if (spec_file == null or config_file == null) {
        std.debug.print("Error: --spec and --config are required\n", .{});
        printUsage();
        return;
    }

    std.debug.print("Generating SDKs...\n", .{});
    std.debug.print("  OpenAPI Spec: {s}\n", .{spec_file.?});
    std.debug.print("  Config: {s}\n", .{config_file.?});
    if (enhance) {
        std.debug.print("  LLM Enhancement: Enabled\n", .{});
    }

    // Load configuration
    const cfg = config.loadConfigFile(allocator, config_file.?) catch |err| {
        std.debug.print("Error loading config: {}\n", .{err});
        return;
    };
    defer cfg.deinit();

    // Parse OpenAPI spec
    const spec = parser.parseOpenAPIFile(allocator, spec_file.?) catch |err| {
        std.debug.print("Error parsing OpenAPI spec: {}\n", .{err});
        return;
    };
    defer spec.deinit();

    // Generate SDKs for each enabled language
    if (cfg.languages.rust) {
        try generateLanguageSDK(allocator, "rust", spec, cfg, enhance);
    }
    if (cfg.languages.go) {
        try generateLanguageSDK(allocator, "go", spec, cfg, enhance);
    }
    if (cfg.languages.typescript) {
        try generateLanguageSDK(allocator, "typescript", spec, cfg, enhance);
    }
    if (cfg.languages.python) {
        try generateLanguageSDK(allocator, "python", spec, cfg, enhance);
    }
    if (cfg.languages.zig) {
        try generateLanguageSDK(allocator, "zig", spec, cfg, enhance);
    }

    std.debug.print("\n✅ SDK generation completed successfully!\n", .{});
}

fn generateLanguageSDK(allocator: std.mem.Allocator, language: []const u8, spec: anytype, cfg: anytype, enhance: bool) !void {
    const output_dir = try std.fmt.allocPrint(allocator, "./generated/{s}", .{language});
    defer allocator.free(output_dir);

    std.debug.print("Generating {s} SDK to {s}\n", .{ language, output_dir });

    // Create output directory
    std.fs.cwd().makeDir(output_dir) catch |err| switch (err) {
        error.PathAlreadyExists => {},
        else => return err,
    };

    // Generate SDK using the sdk_gen module
    try sdk_gen.generateSDK(allocator, language, spec, cfg, output_dir, enhance);

    std.debug.print("Generated {s} SDK at {s}\n", .{ std.fmt.titleCase(language), output_dir });
}

fn handleInit(allocator: std.mem.Allocator) !void {
    const example_config = 
        \\# Sterling SDK Generator Configuration
        \\
        \\[project]
        \\name = "my-api"
        \\version = "1.0.0"
        \\description = "My API SDK"
        \\
        \\[languages]
        \\typescript = true
        \\rust = true
        \\python = true
        \\go = true
        \\zig = false
        \\
        \\[output]
        \\directory = "./generated"
        \\
        \\[github]
        \\organization = "my-org"
        \\create_repos = false
        \\auto_publish = false
        \\
        \\[llm]
        \\provider = "anthropic"
        \\model = "claude-3-5-sonnet-20241022"
        \\enhance_code = false
    ;

    try std.fs.cwd().writeFile("sterling.toml", example_config);
    std.debug.print("Created sterling.toml with example configuration\n", .{});
    std.debug.print("Edit the file to customize for your project\n", .{});

    _ = allocator;
}
