const std = @import("std");
const toml = @import("toml.zig");

pub const Config = struct {
    project: Project,
    targets: []const Target,
    llm: ?LLMConfig = null,

    pub const Project = struct {
        name: []const u8,
        version: []const u8,
    };

    pub const Target = struct {
        language: Language,
        repository: []const u8 = "",
        output_dir: []const u8,
        branch: []const u8 = "main",

        pub const Language = enum {
            typescript,
            rust,
            python,
            go,
            zig,
            java,
            kotlin,
            ruby,
            php,
            csharp,
        };
    };

    pub const LLMConfig = struct {
        provider: Provider,
        api_key: []const u8,
        model: []const u8,

        pub const Provider = enum {
            anthropic,
            openai,
        };
    };
};

pub const ConfigError = error{
    MissingProjectSection,
    MissingProjectName,
    MissingProjectVersion,
    MissingTargets,
    MissingLanguage,
    MissingOutputDir,
    InvalidLanguage,
    InvalidProvider,
    OutOfMemory,
    UnterminatedString,
    InvalidLine,
    UnterminatedSection,
};

pub fn parseConfig(allocator: std.mem.Allocator, content: []const u8) ConfigError!Config {
    var parser = toml.TomlParser.init(allocator);
    const result = try parser.parse(content);

    const project_section = result.getSection("project") orelse return error.MissingProjectSection;
    const project = Config.Project{
        .name = project_section.getString("name") orelse return error.MissingProjectName,
        .version = project_section.getString("version") orelse return error.MissingProjectVersion,
    };

    var target_count: usize = 0;

    const array_targets = result.getArraySection("targets");
    if (array_targets) |at| {
        target_count += at.len;
    }

    var sections_iter = result.sections.iterator();
    while (sections_iter.next()) |entry| {
        if (std.mem.startsWith(u8, entry.key_ptr.*, "targets.")) {
            target_count += 1;
        }
    }

    if (target_count == 0) return error.MissingTargets;

    const targets = allocator.alloc(Config.Target, target_count) catch return error.OutOfMemory;
    var idx: usize = 0;

    if (array_targets) |at| {
        for (at) |table| {
            const lang_str = table.getString("language") orelse return error.MissingLanguage;
            const language = parseLanguage(lang_str) orelse return error.InvalidLanguage;
            const output_dir = table.getString("output_dir") orelse return error.MissingOutputDir;

            targets[idx] = Config.Target{
                .language = language,
                .repository = table.getString("repository") orelse "",
                .output_dir = output_dir,
                .branch = table.getString("branch") orelse "main",
            };
            idx += 1;
        }
    }

    sections_iter = result.sections.iterator();
    while (sections_iter.next()) |entry| {
        if (std.mem.startsWith(u8, entry.key_ptr.*, "targets.")) {
            const table = entry.value_ptr;
            const lang_str = table.getString("language") orelse continue;
            const language = parseLanguage(lang_str) orelse continue;
            const output_dir = table.getString("output_dir") orelse continue;

            targets[idx] = Config.Target{
                .language = language,
                .repository = table.getString("repository") orelse "",
                .output_dir = output_dir,
                .branch = table.getString("branch") orelse "main",
            };
            idx += 1;
        }
    }

    var llm_config: ?Config.LLMConfig = null;
    if (result.getSection("llm")) |llm_section| {
        const provider_str = llm_section.getString("provider");
        const api_key = llm_section.getString("api_key");
        const model = llm_section.getString("model");

        if (provider_str != null and api_key != null and model != null) {
            const provider: Config.LLMConfig.Provider = if (std.mem.eql(u8, provider_str.?, "anthropic"))
                .anthropic
            else if (std.mem.eql(u8, provider_str.?, "openai"))
                .openai
            else
                return error.InvalidProvider;

            llm_config = Config.LLMConfig{
                .provider = provider,
                .api_key = api_key.?,
                .model = model.?,
            };
        }
    }

    return Config{
        .project = project,
        .targets = targets[0..idx],
        .llm = llm_config,
    };
}

fn parseLanguage(lang: []const u8) ?Config.Target.Language {
    if (std.mem.eql(u8, lang, "typescript")) return .typescript;
    if (std.mem.eql(u8, lang, "rust")) return .rust;
    if (std.mem.eql(u8, lang, "python")) return .python;
    if (std.mem.eql(u8, lang, "go")) return .go;
    if (std.mem.eql(u8, lang, "zig")) return .zig;
    if (std.mem.eql(u8, lang, "java")) return .java;
    if (std.mem.eql(u8, lang, "kotlin")) return .kotlin;
    if (std.mem.eql(u8, lang, "ruby")) return .ruby;
    if (std.mem.eql(u8, lang, "php")) return .php;
    if (std.mem.eql(u8, lang, "csharp")) return .csharp;
    return null;
}

pub fn loadConfig(allocator: std.mem.Allocator, path: []const u8) !Config {
    const content = try std.fs.cwd().readFileAlloc(allocator, path, 1024 * 1024);
    return try parseConfig(allocator, content);
}
