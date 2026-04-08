# Phase 4: LLM Integration (0% → 100%)

## Target: AI-powered code enhancement and documentation generation

## Week 1: LLM Infrastructure

### Core LLM Integration Module
```zig
// src/llm/provider.zig
const std = @import("std");
const http = std.http;
const json = std.json;

pub const LLMProvider = enum {
    anthropic,
    openai,
};

pub const LLMConfig = struct {
    provider: LLMProvider,
    api_key: []const u8,
    model: []const u8,
    max_retries: u32 = 3,
    timeout_ms: u32 = 30000,
};

pub const LLMClient = struct {
    allocator: std.mem.Allocator,
    config: LLMConfig,
    http_client: http.Client,

    pub fn init(allocator: std.mem.Allocator, config: LLMConfig) LLMClient {
        return LLMClient{
            .allocator = allocator,
            .config = config,
            .http_client = http.Client{ .allocator = allocator },
        };
    }

    pub fn enhanceCode(self: *LLMClient, code: []const u8, language: []const u8) ![]u8 {
        const prompt = try self.buildEnhancementPrompt(code, language);
        return self.callLLM(prompt);
    }

    pub fn generateDocumentation(self: *LLMClient, spec: OpenAPISpec) ![]u8 {
        const prompt = try self.buildDocPrompt(spec);
        return self.callLLM(prompt);
    }

    fn callLLM(self: *LLMClient, prompt: []const u8) ![]u8 {
        return switch (self.config.provider) {
            .anthropic => self.callAnthropic(prompt),
            .openai => self.callOpenAI(prompt),
        };
    }
};
```

### Prompt Templates System
```zig
// src/llm/prompts.zig
pub const PromptTemplate = struct {
    template: []const u8,
    variables: std.StringHashMap([]const u8),

    pub fn render(self: *PromptTemplate, allocator: std.mem.Allocator) ![]u8 {
        var result = std.ArrayList(u8).init(allocator);
        defer result.deinit();
        
        // Template rendering logic
        return result.toOwnedSlice();
    }
};

pub const ENHANCEMENT_PROMPT = 
\\You are an expert software engineer. Enhance this {{language}} code:
\\
\\```{{language}}
\\{{code}}
\\```
\\
\\Improvements needed:
\\1. Add comprehensive documentation
\\2. Improve error handling
\\3. Add usage examples
\\4. Optimize performance
\\5. Follow best practices
\\
\\Return only the enhanced code with no explanations.
;

pub const DOCUMENTATION_PROMPT = 
\\Generate comprehensive API documentation for this OpenAPI specification:
\\
\\{{spec}}
\\
\\Include:
\\- Overview and getting started guide
\\- Authentication methods
\\- All endpoints with examples
\\- Error codes and handling
\\- SDK usage examples in multiple languages
\\
\\Format as Markdown.
;
```

## Week 2: Code Enhancement Features

### SDK Code Enhancement
```zig
// src/llm/enhancer.zig
pub const CodeEnhancer = struct {
    llm_client: *LLMClient,
    allocator: std.mem.Allocator,

    pub fn enhanceSDK(self: *CodeEnhancer, sdk_files: []SDKFile) ![]SDKFile {
        var enhanced_files = std.ArrayList(SDKFile).init(self.allocator);
        defer enhanced_files.deinit();

        for (sdk_files) |file| {
            const enhanced = try self.enhanceFile(file);
            try enhanced_files.append(enhanced);
        }

        return enhanced_files.toOwnedSlice();
    }

    fn enhanceFile(self: *CodeEnhancer, file: SDKFile) !SDKFile {
        const enhanced_content = try self.llm_client.enhanceCode(
            file.content, 
            file.language
        );

        return SDKFile{
            .path = file.path,
            .content = enhanced_content,
            .language = file.language,
        };
    }
};
```

## Week 3: Advanced LLM Features

### Documentation Generator
```zig
// src/llm/docs.zig
pub const DocumentationGenerator = struct {
    llm_client: *LLMClient,
    allocator: std.mem.Allocator,

    pub fn generateAPIReference(self: *DocumentationGenerator, spec: OpenAPISpec) ![]u8 {
        const sections = [_][]const u8{
            "overview",
            "authentication", 
            "endpoints",
            "models",
            "errors",
            "examples"
        };

        var docs = std.ArrayList(u8).init(self.allocator);
        defer docs.deinit();

        for (sections) |section| {
            const content = try self.generateSection(spec, section);
            try docs.appendSlice(content);
            try docs.appendSlice("\n\n");
        }

        return docs.toOwnedSlice();
    }

    fn generateSection(self: *DocumentationGenerator, spec: OpenAPISpec, section: []const u8) ![]u8 {
        const prompt = try std.fmt.allocPrint(self.allocator,
            "Generate the {s} section for API documentation based on this OpenAPI spec:\n{s}",
            .{ section, spec.raw }
        );
        defer self.allocator.free(prompt);

        return self.llm_client.callLLM(prompt);
    }
};
```

## Integration with Sterling Core

### Enhanced SDK Generator
```zig
// src/generator/sdk.zig - Enhanced with LLM
pub fn generateWithLLM(self: *SDKGenerator, use_llm: bool) !void {
    // Generate base SDK
    try self.generate();

    if (use_llm and self.config.llm) |llm_config| {
        var llm_client = LLMClient.init(self.allocator, llm_config);
        var enhancer = CodeEnhancer{ 
            .llm_client = &llm_client, 
            .allocator = self.allocator 
        };

        // Enhance generated files
        const files = try self.loadGeneratedFiles();
        const enhanced = try enhancer.enhanceSDK(files);
        try self.writeEnhancedFiles(enhanced);

        // Generate documentation
        var doc_gen = DocumentationGenerator{
            .llm_client = &llm_client,
            .allocator = self.allocator
        };
        const docs = try doc_gen.generateAPIReference(self.spec);
        try self.writeFile("README.md", docs);
    }
}
```

## Success Criteria

- [ ] Anthropic Claude API integration working
- [ ] OpenAI GPT API integration working  
- [ ] Code enhancement for all supported languages
- [ ] Automatic documentation generation
- [ ] Error handling and retry logic
- [ ] Rate limiting and cost management
- [ ] Quality validation of LLM outputs
- [ ] Configurable enhancement levels

This phase adds AI superpowers to Sterling, making it generate not just functional code but polished, well-documented, production-ready SDKs.
