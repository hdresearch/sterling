const std = @import("std");

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
        try handleGenerate(allocator, args[2..]);
    } else if (std.mem.eql(u8, command, "version")) {
        std.debug.print("Sterling v0.1.0\n", .{});
        std.debug.print("OpenAPI SDK Generator in Zig\n", .{});
        std.debug.print("https://github.com/hdresearch/sterling\n", .{});
    } else if (std.mem.eql(u8, command, "init")) {
        try handleInit(allocator);
    } else {
        printUsage();
    }
}

fn printUsage() void {
    std.debug.print("Usage:\n", .{});
    std.debug.print("  sterling generate --spec <openapi.yaml> --config <sterling.toml>\n", .{});
    std.debug.print("  sterling init                    # Create example sterling.toml\n", .{});
    std.debug.print("  sterling version                 # Show version info\n", .{});
    std.debug.print("\nExamples:\n", .{});
    std.debug.print("  sterling generate --spec petstore.yaml --config sterling.toml\n", .{});
    std.debug.print("  sterling init && sterling generate --spec api.yaml --config sterling.toml\n", .{});
}

fn handleGenerate(allocator: std.mem.Allocator, args: [][:0]u8) !void {
    var spec_file: ?[]const u8 = null;
    var config_file: ?[]const u8 = null;

    var i: usize = 0;
    while (i < args.len) : (i += 1) {
        if (std.mem.eql(u8, args[i], "--spec") and i + 1 < args.len) {
            spec_file = args[i + 1];
            i += 1;
        } else if (std.mem.eql(u8, args[i], "--config") and i + 1 < args.len) {
            config_file = args[i + 1];
            i += 1;
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
    
    // TODO: Integrate with parser, generator, and config modules
    std.debug.print("\nTODO: Implement full generation pipeline\n", .{});
    std.debug.print("1. Parse OpenAPI specification\n", .{});
    std.debug.print("2. Load configuration\n", .{});
    std.debug.print("3. Generate SDKs for each target language\n", .{});
    std.debug.print("4. Push to configured repositories\n", .{});
    std.debug.print("5. Generate documentation\n", .{});
    
    _ = allocator;
}

fn handleInit(allocator: std.mem.Allocator) !void {
    const config_content =
        \\# Sterling SDK Generator Configuration
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
        \\model = "claude-3-sonnet-20240229"
        \\max_retries = 3
        \\
        \\[output.docs]
        \\format = "mintlify"
        \\repository = "https://github.com/your-org/docs"
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
    
    _ = allocator;
}
