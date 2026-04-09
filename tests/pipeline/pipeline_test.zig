const std = @import("std");
const pipeline = @import("pipeline");

test "PipelineOrchestrator init" {
    const orch = pipeline.PipelineOrchestrator.init(std.testing.allocator);
    try std.testing.expectEqual(@as(usize, 4), orch.config.languages.len);
    try std.testing.expect(orch.config.enable_validation);
}

test "executePipeline fails with missing spec" {
    var orch = pipeline.PipelineOrchestrator.init(std.testing.allocator);

    const result = try orch.executePipeline("/nonexistent/path/openapi.yaml");
    defer {
        std.testing.allocator.free(result.steps);
        std.testing.allocator.free(result.languages_generated);
    }

    try std.testing.expect(!result.success);
    try std.testing.expectEqual(@as(usize, 1), result.steps.len);
    try std.testing.expectEqual(pipeline.StepStatus.failed, result.steps[0].status);
}

test "PipelineResult format" {
    const steps = [_]pipeline.PipelineStep{
        .{ .name = "validate-spec", .status = .completed, .duration_ms = 5 },
    };

    const result = pipeline.PipelineResult{
        .success = true,
        .steps = &steps,
        .total_duration_ms = 5,
        .spec_path = "test.yaml",
        .languages_generated = &.{"rust"},
    };

    const formatted = try result.format(std.testing.allocator);
    defer std.testing.allocator.free(formatted);

    try std.testing.expect(std.mem.indexOf(u8, formatted, "SUCCEEDED") != null);
}
