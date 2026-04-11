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
    std.debug.print("Open source replacement for Stainless, written in Zig\n\n", .{});

    if (args.len < 2) {
        printUsage();
        return;
    }

    const command = args[1];
    if (std.mem.eql(u8, command, "generate")) {
        try handleGenerate(allocator, args);
    } else if (std.mem.eql(u8, command, "version")) {
        std.debug.print("Sterling v0.1.0\n", .{});
        std.debug.print("https://github.com/hdresearch/sterling\n", .{});
    } else if (std.mem.eql(u8, command, "init")) {
        try handleInit();
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
    std.debug.print("\nOptions:\n", .{});
    std.debug.print("  --enhance                        # Enable LLM code enhancement\n", .{});
    std.debug.print("\nEnvironment Variables:\n", .{});
    std.debug.print("  ANTHROPIC_API_KEY               # For LLM enhancement\n", .{});
    std.debug.print("  GITHUB_TOKEN                    # For GitHub automation\n", .{});
    std.debug.print("\n", .{});
}

fn handleGenerate(allocator: std.mem.Allocator, args: [][:0]u8) !void {
    var spec_file: ?[]const u8 = null;
    var config_file: ?[]const u8 = null;
    var enhance = false;

    // Skip args[0] (binary) and args[1] ("generate")
    var i: usize = 2;
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
    const cfg = config.loadConfig(allocator, config_file.?) catch |err| {
        std.debug.print("Error loading config: {}\n", .{err});
        return;
    };

    // Parse OpenAPI spec
    var spec = parser.parseOpenAPIFile(allocator, spec_file.?) catch |err| {
        std.debug.print("Error parsing OpenAPI spec: {}\n", .{err});
        return;
    };
    _ = &spec;

    // Generate SDKs for each target in config
    var generator = sdk_gen.SDKGenerator.init(allocator, spec, cfg);

    for (cfg.targets) |target| {
        const lang_name = @tagName(target.language);
        std.debug.print("Generating {s} SDK to {s}\n", .{ lang_name, target.output_dir });
        generator.generateTarget(target) catch |err| {
            std.debug.print("Error generating {s} SDK: {}\n", .{ lang_name, err });
            continue;
        };
    }

    std.debug.print("\n✅ SDK generation completed successfully!\n", .{});
}

fn handleInit() !void {
    const example_config =
        \\# Sterling SDK Generator Configuration
        \\
        \\[project]
        \\name = "my-api"
        \\version = "1.0.0"
        \\
        \\[targets.typescript]
        \\language = "typescript"
        \\output_dir = "./generated/typescript"
        \\
        \\[targets.rust]
        \\language = "rust"
        \\output_dir = "./generated/rust"
        \\
        \\[targets.python]
        \\language = "python"
        \\output_dir = "./generated/python"
        \\
        \\[targets.go]
        \\language = "go"
        \\output_dir = "./generated/go"
        \\
        \\[llm]
        \\provider = "anthropic"
        \\api_key = "${ANTHROPIC_API_KEY}"
        \\model = "claude-3-5-sonnet-20241022"
    ;

    const file = try std.fs.cwd().createFile("sterling.toml", .{});
    defer file.close();
    try file.writeAll(example_config);
    std.debug.print("Created sterling.toml with example configuration\n", .{});
    std.debug.print("Edit the file to customize for your project\n", .{});
}
