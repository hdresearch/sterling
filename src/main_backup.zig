const std = @import("std");
const parser = @import("openapi");
const config = @import("config");
const sdk_gen = @import("sdk_gen");

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
}

fn handleGenerate(raw_allocator: std.mem.Allocator, args: [][:0]u8) !void {
    var arena = std.heap.ArenaAllocator.init(raw_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();
    
    var spec_file: ?[]const u8 = null;
    var config_file: ?[]const u8 = null;
    var enable_llm = false;

    var i: usize = 0;
    while (i < args.len) : (i += 1) {
        if (std.mem.eql(u8, args[i], "--spec") and i + 1 < args.len) {
            spec_file = args[i + 1];
            i += 1;
        } else if (std.mem.eql(u8, args[i], "--config") and i + 1 < args.len) {
            config_file = args[i + 1];
            i += 1;
        } else if (std.mem.eql(u8, args[i], "--enhance")) {
            enable_llm = true;
        }
    }

    if (spec_file == null) {
        std.debug.print("Error: --spec <openapi.yaml> is required\n", .{});
        return;
    }

    if (config_file == null) {
        std.debug.print("Error: --config <sterling.toml> is required\n", .{});
        return;
    }

    std.debug.print("Generating SDKs...\n", .{});
    std.debug.print("  OpenAPI Spec: {s}\n", .{spec_file.?});
    std.debug.print("  Config: {s}\n", .{config_file.?});
    if (enable_llm) {
        std.debug.print("  LLM Enhancement: Enabled\n", .{});
    }

    const spec_content = std.fs.cwd().readFileAlloc(allocator, spec_file.?, 1024 * 1024) catch |err| {
        std.debug.print("Error reading spec file: {}\n", .{err});
        return;
    };

    const spec = parser.parseOpenAPI(allocator, spec_content) catch |err| {
        std.debug.print("Error parsing OpenAPI spec: {}\n", .{err});
        return;
    };

    const cfg = config.loadConfig(allocator, config_file.?) catch |err| {
        std.debug.print("Error loading config: {}\n", .{err});
        return;
    };

    var sdk_generator = sdk_gen.SDKGenerator.init(allocator, spec, cfg);
    sdk_generator.generateAll() catch |err| {
        std.debug.print("Error generating SDKs: {}\n", .{err});
        return;
    };

    if (enable_llm) {
        std.debug.print("\n🤖 LLM enhancement available but not implemented in this demo\n", .{});
        std.debug.print("Set ANTHROPIC_API_KEY to enable LLM features\n", .{});
    }

    std.debug.print("\n✅ SDK generation completed successfully!\n", .{});
}

fn handleInit(allocator: std.mem.Allocator) !void {
    const config_content =
        \\# Sterling SDK Generator Configuration
        \\
        \\[project]
        \\name = "my-api"
        \\version = "1.0.0"
        \\description = "My API SDK"
        \\
        \\[targets.typescript]
        \\language = "typescript"
        \\repository = "https://github.com/your-org/typescript-sdk"
        \\output_dir = "./generated/typescript"
        \\branch = "main"
        \\
        \\[targets.rust]
        \\language = "rust"
        \\repository = "https://github.com/your-org/rust-sdk"
        \\output_dir = "./generated/rust"
        \\branch = "main"
        \\
        \\[targets.python]
        \\language = "python"
        \\repository = "https://github.com/your-org/python-sdk"
        \\output_dir = "./generated/python"
        \\branch = "main"
        \\
        \\[targets.go]
        \\language = "go"
        \\repository = "https://github.com/your-org/go-sdk"
        \\output_dir = "./generated/go"
        \\branch = "main"
        \\
        \\[llm]
        \\provider = "anthropic"
        \\api_key = "${ANTHROPIC_API_KEY}"
        \\model = "claude-3-5-sonnet-20241022"
        \\
        \\[github]
        \\token = "${GITHUB_TOKEN}"
        \\org = "your-org"
        \\
        \\[output.docs]
        \\format = "mintlify"
        \\output_dir = "./generated/docs"
        \\
    ;

    const file = std.fs.cwd().createFile("sterling.toml", .{}) catch |err| switch (err) {
        error.PathAlreadyExists => {
            std.debug.print("sterling.toml already exists\n", .{});
            return;
        },
        else => return err,
    };
    defer file.close();

    try file.writeAll(config_content);
    std.debug.print("Created sterling.toml configuration file\n", .{});
    std.debug.print("Edit the file to configure your target repositories and settings\n", .{});
    std.debug.print("\nTo enable LLM enhancement:\n", .{});
    std.debug.print("  export ANTHROPIC_API_KEY=your_key_here\n", .{});
    std.debug.print("\nTo enable GitHub automation:\n", .{});
    std.debug.print("  export GITHUB_TOKEN=your_token_here\n", .{});

    _ = allocator;
}
