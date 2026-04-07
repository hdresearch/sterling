const std = @import("std");
const sdk = @import("sdk");

test "rust generator exists" {
    // Verify the generateRust function exists on SDKGenerator
    const T = sdk.SDKGenerator;
    try std.testing.expect(@hasDecl(T, "generateRust"));
}
