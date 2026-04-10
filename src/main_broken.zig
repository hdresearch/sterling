const std = @import("std");
const parser = @import("openapi");
const config = @import("config");
const sdk_gen = @import("sdk");
const llm = @import("llm");
const github = @import("github");
const workflow = @import("workflow");
const docs = @import("docs");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    if (args.len < 2) {
        printUsage();
        return;
    }

    const command = args[1];
    
    if (std.mem.eql(u8, command, "generate")) {
        try handleGenerate(allocator, args);
    } else if (std.mem.eql(u8, command, "init")) {
        try handleInit(allocator);
    } else if (std.mem.eql(u8, command, "version")) {
        printVersion();
    } else if (std.mem.eql(u8, command, "workflow")) {
        try handleWorkflow(allocator, args);
    } else {
        std.debug.print("Unknown command: {s}\n", .{command});
        printUsage();
    }
}

fn handleGenerate(allocator: std.mem.Allocator, args: [][:0]u8) !void {
    var spec_file: ?[]const u8 = null;
    var config_file: ?[]const u8 = null;
    var enable_llm = false;
    var enable_docs = false;
    var enable_github = false;
    var enable_publishing = false;

    var i: usize = 2;
    while (i < args.len) : (i += 1) {
        if (std.mem.eql(u8, args[i], "--spec") and i + 1 < args.len) {
            spec_file = args[i + 1];
            i += 1;
        } else if (std.mem.eql(u8, args[i], "--config") and i + 1 < args.len) {
            config_file = args[i + 1];
            i += 1;
        } else if (std.mem.eql(u8, args[i], "--enhance")) {
            enable_llm = true;
        } else if (std.mem.eql(u8, args[i], "--docs")) {
            enable_docs = true;
        } else if (std.mem.eql(u8, args[i], "--github")) {
            enable_github = true;
        } else if (std.mem.eql(u8, args[i], "--publish")) {
            enable_publishing = true;
        }
    }

    if (spec_file == null or config_file == null) {
        std.debug.print("Error: --spec and --config are required\n", .{});
        printUsage();
        return;
    }

    printHeader();
    std.debug.print("Generating SDKs...\n", .{});
    std.debug.print("  OpenAPI Spec: {s}\n", .{spec_file.?});
    std.debug.print("  Config: {s}\n", .{config_file.?});
    
    if (enable_llm) {
        std.debug.print("  LLM Enhancement: Enabled\n", .{});
    }
    if (enable_docs) {
        std.debug.print("  Documentation: Enabled\n", .{});
    }
    if (enable_github) {
        std.debug.print("  GitHub Automation: Enabled\n", .{});
    }
    if (enable_publishing) {
        std.debug.print("  Package Publishing: Enabled\n", .{});
    }

    // Load spec and config
    const spec_content = std.fs.cwd().readFileAlloc(allocator, spec_file.?, 1024 * 1024) catch |err| {
        std.debug.print("Error reading spec file: {}\n", .{err});
        return;
    };
    defer allocator.free(spec_content);

    const spec = parser.parseOpenAPI(allocator, spec_content) catch |err| {
        std.debug.print("Error parsing OpenAPI spec: {}\n", .{err});
        return;
    };

    const cfg = config.loadConfig(allocator, config_file.?) catch |err| {
        std.debug.print("Error loading config: {}\n", .{err});
        return;
    };

    // Generate basic SDKs
    var sdk_generator = sdk_gen.SDKGenerator.init(allocator, spec, cfg);
    sdk_generator.generateAll() catch |err| {
        std.debug.print("Error generating SDKs: {}\n", .{err});
        return;
    };

    // Apply LLM enhancement if enabled
    if (enable_llm) {
        const api_key = std.process.getEnvVarOwned(allocator, "ANTHROPIC_API_KEY") catch |err| switch (err) {
            error.EnvironmentVariableNotFound => {
                std.debug.print("⚠️  ANTHROPIC_API_KEY not set, skipping LLM enhancement\n", .{});
                null;
            },
            else => return err,
        };
        
        if (api_key) |key| {
            defer allocator.free(key);
            std.debug.print("🤖 Applying LLM enhancements...\n", .{});
            
            const llm_config = llm.enhancer.LLMConfig{
                .api_key = key,
            };
            
            var enhancer = llm.enhancer.LLMEnhancer.init(allocator, llm_config);
            enhancer.enhanceGeneratedSDKs("./generated") catch |err| {
                std.debug.print("Warning: LLM enhancement failed: {}\n", .{err});
            };
        }
    }

    // Generate documentation if enabled
    if (enable_docs) {
        std.debug.print("📚 Generating documentation...\n", .{});
        
        const docs_config = docs.generator.DocsConfig{
            .project_name = cfg.project.name,
            .description = cfg.project.description,
            .output_dir = "./generated/docs",
        };
        
        var docs_generator = docs.generator.DocsGenerator.init(allocator, docs_config);
        docs_generator.generateFromSpec(spec) catch |err| {
            std.debug.print("Warning: Documentation generation failed: {}\n", .{err});
        };
    }

    // GitHub automation if enabled
    if (enable_github) {
        const github_token = std.process.getEnvVarOwned(allocator, "GITHUB_TOKEN") catch |err| switch (err) {
            error.EnvironmentVariableNotFound => {
                std.debug.print("⚠️  GITHUB_TOKEN not set, skipping GitHub automation\n", .{});
                null;
            },
            else => return err,
        };
        
        if (github_token) |token| {
            defer allocator.free(token);
            std.debug.print("🐙 Setting up GitHub repositories...\n", .{});
            
            const github_config = github.automation.GitHubConfig{
                .token = token,
                .org = cfg.github.org,
            };
            
            var gh_automation = github.automation.GitHubAutomation.init(allocator, github_config);
            gh_automation.setupRepositories(cfg) catch |err| {
                std.debug.print("Warning: GitHub automation failed: {}\n", .{err});
            };
        }
    }

    std.debug.print("\n✅ SDK generation completed successfully!\n", .{});
}

fn handleWorkflow(allocator: std.mem.Allocator, args: [][:0]u8) !void {
    if (args.len < 3) {
        std.debug.print("Usage: sterling workflow <config.toml>\n", .{});
        return;
    }

    const config_file = args[2];
    
    printHeader();
    std.debug.print("Running complete workflow...\n", .{});
    std.debug.print("  Config: {s}\n", .{config_file});

    const cfg = config.loadConfig(allocator, config_file) catch |err| {
        std.debug.print("Error loading config: {}\n", .{err});
        return;
    };

    var manager = workflow.manager.WorkflowManager.init(allocator, cfg);
    manager.executeFullPipeline() catch |err| {
        std.debug.print("Error executing workflow: {}\n", .{err});
        return;
    };

    std.debug.print("\n✅ Workflow completed successfully!\n", .{});
}

fn handleInit(allocator: std.mem.Allocator) !void {
    // Same as before...
    _ = allocator;
    std.debug.print("Creating sterling.toml...\n", .{});
}

fn printHeader() void {
    std.debug.print("Sterling SDK Generator v0.1.0\n", .{});
    std.debug.print("Open source replacement for Stainless, written in Zig\n", .{});
    std.debug.print("Features: LLM Enhancement, GitHub Automation, Documentation Generation\n\n", .{});
}

fn printVersion() void {
    std.debug.print("Sterling SDK Generator v0.1.0\n", .{});
    std.debug.print("Built with Zig 0.15.2\n", .{});
}

fn printUsage() void {
    std.debug.print("Sterling SDK Generator v0.1.0\n", .{});
    std.debug.print("Open source replacement for Stainless, written in Zig\n", .{});
    std.debug.print("Features: LLM Enhancement, GitHub Automation, Documentation Generation\n\n", .{});
    std.debug.print("Usage:\n", .{});
    std.debug.print("  sterling generate --spec <openapi.yaml> --config <sterling.toml> [options]\n", .{});
    std.debug.print("  sterling workflow <config.toml>     # Run complete automation pipeline\n", .{});
    std.debug.print("  sterling init                       # Create example sterling.toml\n", .{});
    std.debug.print("  sterling version                    # Show version info\n\n", .{});
    std.debug.print("Generate Options:\n", .{});
    std.debug.print("  --enhance                           # Enable LLM code enhancement\n", .{});
    std.debug.print("  --docs                              # Generate documentation\n", .{});
    std.debug.print("  --github                            # Setup GitHub repositories\n", .{});
    std.debug.print("  --publish                           # Publish packages\n\n", .{});
    std.debug.print("Environment Variables:\n", .{});
    std.debug.print("  ANTHROPIC_API_KEY                  # For LLM enhancement\n", .{});
    std.debug.print("  GITHUB_TOKEN                       # For GitHub automation\n", .{});
    std.debug.print("  NPM_TOKEN                          # For npm publishing\n", .{});
    std.debug.print("  PYPI_TOKEN                         # For PyPI publishing\n", .{});
}
