const std = @import("std");
const testing = std.testing;
const pipeline = @import("pipeline");

test "PipelineOrchestrator initialization" {
    const orch = pipeline.PipelineOrchestrator.init(testing.allocator);
    try testing.expectEqual(@as(usize, 4), orch.config.languages.len);
    try testing.expect(orch.config.enable_validation);
    try testing.expect(orch.config.enable_docs);
    try testing.expect(!orch.config.enable_github_push);
}

test "PipelineOrchestrator custom config" {
    const orch = pipeline.PipelineOrchestrator.initWithConfig(testing.allocator, .{
        .languages = &.{"rust"},
        .enable_validation = false,
        .enable_docs = false,
    });
    try testing.expectEqual(@as(usize, 1), orch.config.languages.len);
    try testing.expect(!orch.config.enable_validation);
}

test "Execute pipeline with valid spec file" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();

    try tmp_dir.dir.writeFile(.{
        .sub_path = "openapi.yaml",
        .data =
        \\openapi: "3.0.0"
        \\info:
        \\  title: Test API
        \\  version: "1.0.0"
        \\paths:
        \\  /health:
        \\    get:
        \\      summary: Health check
        ,
    });

    var path_buf: [std.fs.max_path_bytes]u8 = undefined;
    const spec_path = try tmp_dir.dir.realpath("openapi.yaml", &path_buf);

    var orch = pipeline.PipelineOrchestrator.initWithConfig(testing.allocator, .{
        .languages = &.{ "typescript", "rust" },
        .enable_validation = true,
        .enable_docs = true,
        .enable_github_push = false,
    });

    const result = try orch.executePipeline(spec_path);
    defer {
        for (result.steps) |step| {
            if (std.mem.startsWith(u8, step.name, "generate-")) {
                testing.allocator.free(step.name);
            }
        }
        testing.allocator.free(result.steps);
        testing.allocator.free(result.languages_generated);
    }

    try testing.expect(result.success);
    // validate + parse + 2 generate + docs = 5
    try testing.expectEqual(@as(usize, 5), result.steps.len);
    try testing.expectEqual(@as(usize, 2), result.languages_generated.len);
    try testing.expectEqualStrings(spec_path, result.spec_path);
}

test "Execute pipeline fails with nonexistent spec" {
    var orch = pipeline.PipelineOrchestrator.init(testing.allocator);

    const result = try orch.executePipeline("/tmp/nonexistent_spec_file.yaml");
    defer {
        testing.allocator.free(result.steps);
        testing.allocator.free(result.languages_generated);
    }

    try testing.expect(!result.success);
    try testing.expectEqual(pipeline.StepStatus.failed, result.steps[0].status);
    try testing.expectEqualStrings("validate-spec", result.steps[0].name);
}

test "Pipeline skips validation when disabled" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    try tmp_dir.dir.writeFile(.{ .sub_path = "spec.yaml", .data = "openapi: 3.0.0" });

    var path_buf: [std.fs.max_path_bytes]u8 = undefined;
    const spec_path = try tmp_dir.dir.realpath("spec.yaml", &path_buf);

    var orch = pipeline.PipelineOrchestrator.initWithConfig(testing.allocator, .{
        .languages = &.{},
        .enable_validation = false,
        .enable_docs = false,
        .enable_github_push = false,
    });

    const result = try orch.executePipeline(spec_path);
    defer {
        testing.allocator.free(result.steps);
        testing.allocator.free(result.languages_generated);
    }

    try testing.expect(result.success);
    // Only parse step (no validation, no languages, no docs)
    try testing.expectEqual(@as(usize, 1), result.steps.len);
    try testing.expectEqualStrings("parse-spec", result.steps[0].name);
}

test "Pipeline github push step is skipped" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    try tmp_dir.dir.writeFile(.{ .sub_path = "spec.yaml", .data = "openapi: 3.0.0" });

    var path_buf: [std.fs.max_path_bytes]u8 = undefined;
    const spec_path = try tmp_dir.dir.realpath("spec.yaml", &path_buf);

    var orch = pipeline.PipelineOrchestrator.initWithConfig(testing.allocator, .{
        .languages = &.{},
        .enable_validation = false,
        .enable_docs = false,
        .enable_github_push = true,
    });

    const result = try orch.executePipeline(spec_path);
    defer {
        testing.allocator.free(result.steps);
        testing.allocator.free(result.languages_generated);
    }

    // parse + github-push = 2 steps
    try testing.expectEqual(@as(usize, 2), result.steps.len);
    try testing.expectEqual(pipeline.StepStatus.skipped, result.steps[1].status);
    try testing.expectEqualStrings("github-push", result.steps[1].name);
}

test "PipelineResult format output" {
    const steps = [_]pipeline.PipelineStep{
        .{ .name = "validate-spec", .status = .completed, .duration_ms = 1 },
        .{ .name = "parse-spec", .status = .completed, .duration_ms = 2 },
        .{ .name = "generate-rust", .status = .completed, .duration_ms = 50 },
    };

    const result = pipeline.PipelineResult{
        .success = true,
        .steps = &steps,
        .total_duration_ms = 53,
        .spec_path = "openapi.yaml",
        .languages_generated = &.{"rust"},
    };

    const formatted = try result.format(testing.allocator);
    defer testing.allocator.free(formatted);

    try testing.expect(std.mem.indexOf(u8, formatted, "SUCCEEDED") != null);
    try testing.expect(std.mem.indexOf(u8, formatted, "validate-spec") != null);
    try testing.expect(std.mem.indexOf(u8, formatted, "generate-rust") != null);
    try testing.expect(std.mem.indexOf(u8, formatted, "rust") != null);
}

test "PipelineResult format failure output" {
    const steps = [_]pipeline.PipelineStep{
        .{ .name = "validate-spec", .status = .failed, .error_message = "File not found", .duration_ms = 1 },
    };

    const result = pipeline.PipelineResult{
        .success = false,
        .steps = &steps,
        .total_duration_ms = 1,
        .spec_path = "missing.yaml",
        .languages_generated = &.{},
    };

    const formatted = try result.format(testing.allocator);
    defer testing.allocator.free(formatted);

    try testing.expect(std.mem.indexOf(u8, formatted, "FAILED") != null);
    try testing.expect(std.mem.indexOf(u8, formatted, "File not found") != null);
}
